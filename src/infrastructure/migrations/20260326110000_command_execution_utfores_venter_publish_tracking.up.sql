ALTER TABLE command_execution
    ADD COLUMN IF NOT EXISTS utfores_venter_published_at timestamp;

CREATE INDEX IF NOT EXISTS ix_command_execution_utfores_venter_published_at
    ON command_execution (utfores_venter_published_at);
