-- Reverser sett_saksansvarlig-migrasjonen.

-- 1. Fjern alle navngitte CHECK-constraints og gjenopprett originale (unnamed).
ALTER TABLE command_execution DROP CONSTRAINT IF EXISTS command_execution_command_type_check;
ALTER TABLE command_execution DROP CONSTRAINT IF EXISTS command_execution_status_check;
ALTER TABLE command_execution DROP CONSTRAINT IF EXISTS command_execution_retry_ready_at_check;
ALTER TABLE command_execution DROP CONSTRAINT IF EXISTS command_execution_finished_at_check;
ALTER TABLE command_execution DROP CONSTRAINT IF EXISTS command_execution_attempt_no_check;
ALTER TABLE command_execution DROP CONSTRAINT IF EXISTS command_execution_sak_jp_routing_check;

-- Re-add original inline constraints (matching base migration, without sett_saksansvarlig).
-- Using unnamed inline style to match original.
ALTER TABLE command_execution
    ADD CHECK (command_type IN (
        'opprett_sak',
        'opprett_inngaaende_journalpost',
        'opprett_utgaaende_journalpost',
        'opprett_internt_notat_journalpost',
        'avslutt_sak'
    ));

ALTER TABLE command_execution
    ADD CHECK (status IN ('klar', 'kjorer', 'blokkert_venter', 'retry_venter', 'ok', 'feil'));

ALTER TABLE command_execution
    ADD CHECK ((status = 'retry_venter' AND retry_ready_at IS NOT NULL) OR (status <> 'retry_venter' AND retry_ready_at IS NULL));

ALTER TABLE command_execution
    ADD CHECK ((status IN ('ok', 'feil') AND finished_at IS NOT NULL) OR (status NOT IN ('ok', 'feil') AND finished_at IS NULL));

ALTER TABLE command_execution
    ADD CHECK (attempt_no >= 0);

ALTER TABLE command_execution
    ADD CHECK (
        (command_type IN ('opprett_sak', 'avslutt_sak')
            AND sak_id IS NOT NULL AND journalpost_id IS NULL)
        OR (command_type IN (
            'opprett_inngaaende_journalpost',
            'opprett_utgaaende_journalpost',
            'opprett_internt_notat_journalpost')
            AND sak_id IS NOT NULL AND journalpost_id IS NOT NULL)
    );

-- 2. Fjern saksansvarlig-kolonner og constraints fra sak_tilstand
ALTER TABLE sak_tilstand DROP CONSTRAINT IF EXISTS chk_naavaerende_saksansvarlig_pair;
ALTER TABLE sak_tilstand DROP CONSTRAINT IF EXISTS chk_oensket_saksansvarlig_pair;

ALTER TABLE sak_tilstand
    DROP COLUMN IF EXISTS naavaerende_saksansvarlig_enhet,
    DROP COLUMN IF EXISTS naavaerende_saksansvarlig_id,
    DROP COLUMN IF EXISTS oensket_saksansvarlig_enhet,
    DROP COLUMN IF EXISTS oensket_saksansvarlig_id;
