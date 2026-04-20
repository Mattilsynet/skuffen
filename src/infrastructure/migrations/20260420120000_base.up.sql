-- Entity type enum (for id_mapping)
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'entity_type') THEN
        CREATE TYPE entity_type AS ENUM ('sak', 'journalpost', 'dokument');
    END IF;
END
$$;

-- id_mapping: stable ID mapping between client references and arkiv IDs
CREATE TABLE IF NOT EXISTS id_mapping (
    skuffen_id UUID PRIMARY KEY,
    entity_type entity_type NOT NULL,
    client_reference UUID NULL,
    command_id UUID NULL,
    arkiv_id TEXT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT id_mapping_requires_reference CHECK (client_reference IS NOT NULL OR arkiv_id IS NOT NULL)
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_id_mapping_client_reference_not_null
    ON id_mapping (client_reference) WHERE client_reference IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS uq_id_mapping_type_arkiv
    ON id_mapping (entity_type, arkiv_id) WHERE arkiv_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS ix_id_mapping_client_reference ON id_mapping (client_reference);
CREATE INDEX IF NOT EXISTS ix_id_mapping_type_arkiv ON id_mapping (entity_type, arkiv_id) WHERE arkiv_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS ix_id_mapping_command_id ON id_mapping (command_id);

-- Trigger function for updated_at
CREATE OR REPLACE FUNCTION set_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = now();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_id_mapping_set_updated_at ON id_mapping;
CREATE TRIGGER trg_id_mapping_set_updated_at
BEFORE UPDATE ON id_mapping
FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- sak_tilstand: entity state for sak
CREATE TABLE sak_tilstand (
    sak_id UUID PRIMARY KEY,
    tilstand VARCHAR(20) NOT NULL,
    oensket_tilstand VARCHAR(20) NOT NULL,
    sikri_id BIGINT NULL,
    saksnummer VARCHAR(64) NULL,
    opprettet_av_command_id UUID NOT NULL,
    feil_detalj TEXT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (tilstand IN ('ikke_realisert', 'opprettet', 'avsluttet', 'feilet_permanent')),
    CHECK (oensket_tilstand IN ('opprettet', 'avsluttet')),
    CHECK (tilstand <> 'avsluttet' OR saksnummer IS NOT NULL),
    CHECK (tilstand <> 'feilet_permanent' OR feil_detalj IS NOT NULL)
);

DROP TRIGGER IF EXISTS trg_sak_tilstand_set_updated_at ON sak_tilstand;
CREATE TRIGGER trg_sak_tilstand_set_updated_at
BEFORE UPDATE ON sak_tilstand
FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- journalpost_tilstand: entity state for journalpost
CREATE TABLE journalpost_tilstand (
    journalpost_id UUID PRIMARY KEY,
    sak_id UUID NOT NULL REFERENCES sak_tilstand(sak_id),
    journalposttype VARCHAR(1) NOT NULL,
    med_utsending BOOLEAN NOT NULL DEFAULT false,
    tilstand VARCHAR(30) NOT NULL,
    oensket_tilstand VARCHAR(30) NOT NULL,
    sikri_id BIGINT NULL,
    journalpostnummer INTEGER NULL,
    opprettet_av_command_id UUID NOT NULL,
    feil_detalj TEXT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (journalposttype IN ('I', 'U', 'X')),
    CHECK (tilstand IN (
        'ikke_realisert', 'opprettet', 'dokumenter_under_arbeid',
        'klar_for_journalforing', 'venter_paa_utsending',
        'journalfoert', 'avskrevet', 'feilet_permanent'
    )),
    CHECK (oensket_tilstand IN ('journalfoert', 'avskrevet')),
    CHECK (NOT med_utsending OR journalposttype = 'U'),
    CHECK (tilstand <> 'feilet_permanent' OR feil_detalj IS NOT NULL)
);

DROP TRIGGER IF EXISTS trg_journalpost_tilstand_set_updated_at ON journalpost_tilstand;
CREATE TRIGGER trg_journalpost_tilstand_set_updated_at
BEFORE UPDATE ON journalpost_tilstand
FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- dokument_tilstand: entity state for dokument
CREATE TABLE dokument_tilstand (
    dokument_id UUID PRIMARY KEY,
    journalpost_id UUID NOT NULL REFERENCES journalpost_tilstand(journalpost_id),
    tilstand VARCHAR(20) NOT NULL DEFAULT 'ikke_realisert',
    oensket_tilstand VARCHAR(20) NOT NULL DEFAULT 'ok',
    opprettet_av_command_id UUID NOT NULL,
    feil_detalj TEXT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (tilstand IN ('ikke_realisert', 'ok', 'feilet_permanent')),
    CHECK (oensket_tilstand IN ('ok')),
    CHECK (tilstand <> 'feilet_permanent' OR feil_detalj IS NOT NULL)
);

DROP TRIGGER IF EXISTS trg_dokument_tilstand_set_updated_at ON dokument_tilstand;
CREATE TRIGGER trg_dokument_tilstand_set_updated_at
BEFORE UPDATE ON dokument_tilstand
FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- tilstand_historikk: audit trail for state transitions
CREATE TABLE tilstand_historikk (
    id BIGSERIAL PRIMARY KEY,
    entity_type VARCHAR(20) NOT NULL,
    entity_id UUID NOT NULL,
    command_id UUID NOT NULL,
    fra_tilstand VARCHAR(30) NOT NULL,
    til_tilstand VARCHAR(30) NOT NULL,
    operasjon VARCHAR(64) NOT NULL,
    feil_detalj TEXT NULL,
    tidspunkt TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (entity_type IN ('sak', 'journalpost', 'dokument'))
);

-- command_execution: job queue and scheduling (revised - no wait columns)
CREATE TABLE command_execution (
    command_id UUID PRIMARY KEY,
    correlation_id UUID,
    payload JSONB NOT NULL,
    command_type VARCHAR(64) NOT NULL,
    sak_id UUID REFERENCES sak_tilstand(sak_id),
    journalpost_id UUID REFERENCES journalpost_tilstand(journalpost_id),
    status VARCHAR(16) NOT NULL,
    attempt_no INTEGER NOT NULL DEFAULT 0,
    retry_ready_at TIMESTAMPTZ NULL,
    last_detail TEXT NULL,
    utfores_venter_publisert_at TIMESTAMPTZ NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at TIMESTAMPTZ NULL,
    finished_at TIMESTAMPTZ NULL,
    CHECK (command_type IN (
        'opprett_sak',
        'opprett_inngaaende_journalpost',
        'opprett_utgaaende_journalpost',
        'opprett_internt_notat_journalpost',
        'avslutt_sak'
    )),
    CHECK (status IN ('klar', 'kjorer', 'blokkert_venter', 'retry_venter', 'ok', 'feil')),
    CHECK ((status = 'retry_venter' AND retry_ready_at IS NOT NULL) OR (status <> 'retry_venter' AND retry_ready_at IS NULL)),
    CHECK ((status IN ('ok', 'feil') AND finished_at IS NOT NULL) OR (status NOT IN ('ok', 'feil') AND finished_at IS NULL)),
    CHECK (attempt_no >= 0),
    CHECK (
        (command_type IN ('opprett_sak', 'avslutt_sak') AND sak_id IS NOT NULL AND journalpost_id IS NULL)
        OR (command_type IN ('opprett_inngaaende_journalpost', 'opprett_utgaaende_journalpost', 'opprett_internt_notat_journalpost') AND sak_id IS NOT NULL AND journalpost_id IS NOT NULL)
    )
);

DROP TRIGGER IF EXISTS trg_command_execution_set_updated_at ON command_execution;
CREATE TRIGGER trg_command_execution_set_updated_at
BEFORE UPDATE ON command_execution
FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- command_execution_attempt: tracks individual execution attempts
CREATE TABLE command_execution_attempt (
    command_id UUID NOT NULL REFERENCES command_execution(command_id) ON DELETE CASCADE,
    attempt_no INTEGER NOT NULL,
    executor_id VARCHAR(64) NOT NULL,
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ NULL,
    outcome VARCHAR(16) NULL,
    detail TEXT NULL,
    PRIMARY KEY (command_id, attempt_no),
    CHECK (attempt_no > 0),
    CHECK (outcome IS NULL OR outcome IN ('ok', 'blokkert_venter', 'retry_venter', 'feil', 'avbrutt')),
    CHECK ((finished_at IS NULL AND outcome IS NULL) OR (finished_at IS NOT NULL AND outcome IS NOT NULL))
);

-- Indexes
CREATE INDEX ix_journalpost_tilstand_sak_id ON journalpost_tilstand(sak_id);
CREATE INDEX ix_dokument_tilstand_journalpost_id ON dokument_tilstand(journalpost_id);
CREATE INDEX ix_tilstand_historikk_entity ON tilstand_historikk(entity_type, entity_id);
CREATE INDEX ix_tilstand_historikk_command ON tilstand_historikk(command_id);
CREATE INDEX ix_command_execution_runnable ON command_execution(status, retry_ready_at, created_at);
CREATE INDEX ix_command_execution_sak ON command_execution(sak_id);
CREATE INDEX ix_command_execution_attempt_open ON command_execution_attempt(finished_at) WHERE finished_at IS NULL;
CREATE UNIQUE INDEX ux_command_execution_attempt_open_command ON command_execution_attempt(command_id) WHERE finished_at IS NULL;
