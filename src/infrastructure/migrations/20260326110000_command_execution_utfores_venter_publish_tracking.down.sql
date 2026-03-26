DROP INDEX IF EXISTS ix_command_execution_utfores_venter_published_at;

ALTER TABLE command_execution
    DROP COLUMN IF EXISTS utfores_venter_published_at;
