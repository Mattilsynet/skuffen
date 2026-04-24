-- Add sett_saksansvarlig command type and saksansvarlig tracking columns.
--
-- saksansvarlig is Noark 5 metadata M306 on Saksmappe.
-- We track ønsket vs nåværende so the state machine can drive
-- the operation and AvsluttSak can guard on it.

-- 1. Add saksansvarlig tracking columns to sak_tilstand
ALTER TABLE sak_tilstand
    ADD COLUMN oensket_saksansvarlig_id TEXT,
    ADD COLUMN oensket_saksansvarlig_enhet TEXT,
    ADD COLUMN naavaerende_saksansvarlig_id TEXT,
    ADD COLUMN naavaerende_saksansvarlig_enhet TEXT;

-- Both ønsket fields must be set together or both NULL
ALTER TABLE sak_tilstand
    ADD CONSTRAINT chk_oensket_saksansvarlig_pair
        CHECK (
            (oensket_saksansvarlig_id IS NULL AND oensket_saksansvarlig_enhet IS NULL)
            OR (oensket_saksansvarlig_id IS NOT NULL AND oensket_saksansvarlig_enhet IS NOT NULL)
        );

ALTER TABLE sak_tilstand
    ADD CONSTRAINT chk_naavaerende_saksansvarlig_pair
        CHECK (
            (naavaerende_saksansvarlig_id IS NULL AND naavaerende_saksansvarlig_enhet IS NULL)
            OR (naavaerende_saksansvarlig_id IS NOT NULL AND naavaerende_saksansvarlig_enhet IS NOT NULL)
        );

-- 2. Widen command_execution.command_type CHECK to include sett_saksansvarlig
--    and widen the sak_id/journalpost_id routing CHECK.
--
--    The base migration uses unnamed inline CHECKs. Postgres auto-names
--    single-column checks as {table}_{column}_check and multi-column checks
--    sequentially as {table}_check, {table}_check1, etc.
--
--    We drop ALL check constraints and re-add the full set with explicit names.
--    This is safe because we re-add unchanged versions of the non-modified ones.

-- Drop auto-named single-column checks
ALTER TABLE command_execution DROP CONSTRAINT IF EXISTS command_execution_command_type_check;
ALTER TABLE command_execution DROP CONSTRAINT IF EXISTS command_execution_status_check;
ALTER TABLE command_execution DROP CONSTRAINT IF EXISTS command_execution_attempt_no_check;

-- Drop auto-named multi-column checks (and any previously named variants)
ALTER TABLE command_execution DROP CONSTRAINT IF EXISTS command_execution_sak_jp_routing_check;
ALTER TABLE command_execution DROP CONSTRAINT IF EXISTS command_execution_check;
ALTER TABLE command_execution DROP CONSTRAINT IF EXISTS command_execution_check1;
ALTER TABLE command_execution DROP CONSTRAINT IF EXISTS command_execution_check2;
ALTER TABLE command_execution DROP CONSTRAINT IF EXISTS command_execution_check3;
ALTER TABLE command_execution DROP CONSTRAINT IF EXISTS command_execution_check4;
ALTER TABLE command_execution DROP CONSTRAINT IF EXISTS command_execution_check5;

-- Re-add all with explicit names (widened for sett_saksansvarlig)
ALTER TABLE command_execution
    ADD CONSTRAINT command_execution_command_type_check
        CHECK (command_type IN (
            'opprett_sak',
            'opprett_inngaaende_journalpost',
            'opprett_utgaaende_journalpost',
            'opprett_internt_notat_journalpost',
            'avslutt_sak',
            'sett_saksansvarlig'
        ));

ALTER TABLE command_execution
    ADD CONSTRAINT command_execution_status_check
        CHECK (status IN ('klar', 'kjorer', 'blokkert_venter', 'retry_venter', 'ok', 'feil'));

ALTER TABLE command_execution
    ADD CONSTRAINT command_execution_retry_ready_at_check
        CHECK ((status = 'retry_venter' AND retry_ready_at IS NOT NULL) OR (status <> 'retry_venter' AND retry_ready_at IS NULL));

ALTER TABLE command_execution
    ADD CONSTRAINT command_execution_finished_at_check
        CHECK ((status IN ('ok', 'feil') AND finished_at IS NOT NULL) OR (status NOT IN ('ok', 'feil') AND finished_at IS NULL));

ALTER TABLE command_execution
    ADD CONSTRAINT command_execution_attempt_no_check
        CHECK (attempt_no >= 0);

ALTER TABLE command_execution
    ADD CONSTRAINT command_execution_sak_jp_routing_check
        CHECK (
            (command_type IN ('opprett_sak', 'avslutt_sak', 'sett_saksansvarlig')
                AND sak_id IS NOT NULL AND journalpost_id IS NULL)
            OR (command_type IN (
                'opprett_inngaaende_journalpost',
                'opprett_utgaaende_journalpost',
                'opprett_internt_notat_journalpost')
                AND sak_id IS NOT NULL AND journalpost_id IS NOT NULL)
        );
