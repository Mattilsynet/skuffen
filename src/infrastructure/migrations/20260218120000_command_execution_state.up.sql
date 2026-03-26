CREATE TABLE IF NOT EXISTS sak_state (
    sak_id uuid PRIMARY KEY,
    saksnummer varchar(64),
    status varchar(2) NOT NULL,
    opprettet boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CHECK (status IN ('B', 'F', 'A')),
    CHECK (status <> 'A' OR saksnummer IS NOT NULL)
);

CREATE TABLE IF NOT EXISTS journalpost_state (
    journalpost_id uuid PRIMARY KEY,
    sak_id uuid NOT NULL REFERENCES sak_state(sak_id),
    journalpostnummer integer,
    journalposttype varchar(1) NOT NULL,
    med_utsending boolean NOT NULL DEFAULT false,
    journalfoert boolean NOT NULL DEFAULT false,
    avskrevet boolean NOT NULL DEFAULT false,
    ekspedert boolean NOT NULL DEFAULT false,
    har_feilede_dokumenter boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CHECK (journalposttype IN ('I', 'U', 'X')),
    CHECK (NOT journalfoert OR journalpostnummer IS NOT NULL),
    CHECK (NOT avskrevet OR journalfoert),
    CHECK (NOT avskrevet OR journalposttype = 'I'),
    CHECK (NOT med_utsending OR journalposttype = 'U'),
    CHECK (NOT ekspedert OR journalposttype = 'U'),
    CHECK (NOT ekspedert OR med_utsending),
    CHECK (NOT ekspedert OR journalfoert)
);

CREATE UNIQUE INDEX IF NOT EXISTS ux_journalpost_state_sak_journalpost
    ON journalpost_state (sak_id, journalpost_id);

CREATE TABLE IF NOT EXISTS dokument_state (
    dokument_id uuid PRIMARY KEY,
    journalpost_id uuid NOT NULL REFERENCES journalpost_state(journalpost_id),
    lagt_til boolean NOT NULL DEFAULT false,
    irrecoverable_feil boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS command_execution (
    command_id uuid PRIMARY KEY,
    correlation_id uuid,
    payload jsonb NOT NULL,
    command_type varchar(64) NOT NULL,
    sak_id uuid REFERENCES sak_state(sak_id),
    journalpost_id uuid REFERENCES journalpost_state(journalpost_id),
    status varchar(16) NOT NULL,
    attempt_no integer NOT NULL DEFAULT 0,
    retry_ready_at timestamptz NULL,
    wait_kind varchar(64) NULL,
    wait_sak_id uuid NULL REFERENCES sak_state(sak_id),
    wait_journalpost_id uuid NULL REFERENCES journalpost_state(journalpost_id),
    last_detail text NULL,
    utfores_venter_publisert_at timestamptz NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    started_at timestamptz NULL,
    finished_at timestamptz NULL,
    CHECK (command_type IN (
        'opprett_sak',
        'opprett_inngaaende_journalpost',
        'opprett_utgaaende_journalpost',
        'opprett_internt_notat_journalpost',
        'avslutt_sak'
    )),
    CHECK (status IN ('klar', 'kjorer', 'venter', 'retry_venter', 'ok', 'feil')),
    CHECK ((status = 'retry_venter' AND retry_ready_at IS NOT NULL) OR (status <> 'retry_venter' AND retry_ready_at IS NULL)),
    CHECK ((status = 'venter' AND wait_kind IS NOT NULL) OR (status <> 'venter' AND wait_kind IS NULL)),
    CHECK ((status = 'venter') OR (wait_sak_id IS NULL AND wait_journalpost_id IS NULL)),
    CHECK ((status IN ('ok', 'feil') AND finished_at IS NOT NULL) OR (status NOT IN ('ok', 'feil') AND finished_at IS NULL)),
    CHECK (attempt_no >= 0),
    CHECK (wait_kind IS NULL OR wait_kind IN (
        'sak_opprettet',
        'saksnummer_tildelt',
        'journalpost_opprettet',
        'journalpostnummer_tildelt',
        'journalpost_journalfoert',
        'sak_har_uferdige_journalposter'
    )),
    CHECK (
        wait_kind IS NULL
        OR (
            wait_kind IN ('sak_opprettet', 'saksnummer_tildelt', 'sak_har_uferdige_journalposter')
            AND wait_sak_id IS NOT NULL
            AND wait_journalpost_id IS NULL
        )
        OR (
            wait_kind IN ('journalpost_opprettet', 'journalpostnummer_tildelt', 'journalpost_journalfoert')
            AND wait_journalpost_id IS NOT NULL
            AND wait_sak_id IS NULL
        )
    ),
    CHECK (
        (command_type IN ('opprett_sak', 'avslutt_sak') AND sak_id IS NOT NULL AND journalpost_id IS NULL)
        OR (command_type IN ('opprett_inngaaende_journalpost', 'opprett_utgaaende_journalpost', 'opprett_internt_notat_journalpost') AND sak_id IS NOT NULL AND journalpost_id IS NOT NULL)
    )
);

CREATE UNIQUE INDEX IF NOT EXISTS ux_command_execution_command_sak_journalpost
    ON command_execution (command_id, sak_id, journalpost_id);

ALTER TABLE command_execution
    ADD CONSTRAINT fk_command_execution_journalpost_belongs_to_sak
    FOREIGN KEY (sak_id, journalpost_id)
    REFERENCES journalpost_state (sak_id, journalpost_id);

CREATE TABLE IF NOT EXISTS command_execution_attempt (
    command_id uuid NOT NULL REFERENCES command_execution(command_id) ON DELETE CASCADE,
    attempt_no integer NOT NULL,
    executor_id varchar(64) NOT NULL,
    started_at timestamptz NOT NULL DEFAULT now(),
    finished_at timestamptz NULL,
    outcome varchar(16) NULL,
    detail text NULL,
    PRIMARY KEY (command_id, attempt_no),
    CHECK (attempt_no > 0),
    CHECK (outcome IS NULL OR outcome IN ('ok', 'venter', 'retry_venter', 'feil', 'avbrutt')),
    CHECK ((finished_at IS NULL AND outcome IS NULL) OR (finished_at IS NOT NULL AND outcome IS NOT NULL))
);

CREATE INDEX IF NOT EXISTS ix_journalpost_state_sak_id ON journalpost_state (sak_id);
CREATE INDEX IF NOT EXISTS ix_sak_state_saksnummer ON sak_state (saksnummer);
CREATE INDEX IF NOT EXISTS ix_dokument_state_journalpost_id ON dokument_state (journalpost_id);
CREATE INDEX IF NOT EXISTS ix_command_execution_runnable ON command_execution (status, retry_ready_at, created_at);
CREATE INDEX IF NOT EXISTS ix_command_execution_wait_sak ON command_execution (wait_sak_id) WHERE status = 'venter';
CREATE INDEX IF NOT EXISTS ix_command_execution_wait_journalpost ON command_execution (wait_journalpost_id) WHERE status = 'venter';
CREATE INDEX IF NOT EXISTS ix_command_execution_utfores_venter_publisert_at ON command_execution (utfores_venter_publisert_at);
CREATE INDEX IF NOT EXISTS ix_command_execution_attempt_open ON command_execution_attempt (finished_at) WHERE finished_at IS NULL;
CREATE UNIQUE INDEX IF NOT EXISTS ux_command_execution_attempt_open_command
    ON command_execution_attempt (command_id)
    WHERE finished_at IS NULL;
