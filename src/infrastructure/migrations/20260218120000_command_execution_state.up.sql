CREATE TABLE IF NOT EXISTS sak_state (
    -- skuffen id
    sak_id uuid PRIMARY KEY,
    saksnummer varchar(64),
    status varchar(2) NOT NULL,
    opprettet boolean NOT NULL DEFAULT false,
    created_at timestamp NOT NULL DEFAULT now(),
    updated_at timestamp NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS journalpost_state (
    -- Skuffen-id
    journalpost_id uuid PRIMARY KEY,
    -- Skuffen-id
    sak_id uuid NOT NULL REFERENCES sak_state(sak_id),
    journalpostnummer integer,
    -- Journalposttype brukes som I/U/X (arkivfaglige koder). Dette speiles bevisst i state for enkel mapping til Sikri.
    journalposttype varchar(1) NOT NULL,
    med_utsending boolean NOT NULL DEFAULT false,
    journalfoert boolean NOT NULL DEFAULT false,
    avskrevet boolean NOT NULL DEFAULT false,
    ekspedert boolean NOT NULL DEFAULT false,
    -- Aggregert flagg for best-effort: blir true når minst ett dokument på journalposten har irrecoverable_feil.
    -- Brukes til å blokkere journalføring og avslutning av sak.
    har_feilede_dokumenter boolean NOT NULL DEFAULT false,
    created_at timestamp NOT NULL DEFAULT now(),
    updated_at timestamp NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS dokument_state (
    -- Skuffen-id
    dokument_id uuid PRIMARY KEY,
    -- Skuffen-id
    journalpost_id uuid NOT NULL REFERENCES journalpost_state(journalpost_id),
    lagt_til boolean NOT NULL DEFAULT false,
    irrecoverable_feil boolean NOT NULL DEFAULT false,
    created_at timestamp NOT NULL DEFAULT now(),
    updated_at timestamp NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS command_execution (
    command_id uuid PRIMARY KEY,
    correlation_id uuid,
    payload jsonb NOT NULL,
    status varchar(16) NOT NULL,
    attempts integer NOT NULL DEFAULT 0,
    last_error text,
    next_retry_at timestamp,
    locked_at timestamp,
    locked_by varchar(64),
    created_at timestamp NOT NULL DEFAULT now(),
    updated_at timestamp NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS ix_journalpost_state_sak_id
    ON journalpost_state (sak_id);

CREATE INDEX IF NOT EXISTS ix_sak_state_saksnummer
    ON sak_state (saksnummer);

CREATE INDEX IF NOT EXISTS ix_dokument_state_journalpost_id
    ON dokument_state (journalpost_id);

CREATE INDEX IF NOT EXISTS ix_command_execution_status
    ON command_execution (status);

CREATE INDEX IF NOT EXISTS ix_command_execution_ready
    ON command_execution (status, next_retry_at);
