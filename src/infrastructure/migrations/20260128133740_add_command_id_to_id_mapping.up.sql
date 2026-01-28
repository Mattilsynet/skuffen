-- Add command_id to id_mapping table
ALTER TABLE id_mapping ADD COLUMN IF NOT EXISTS command_id UUID;
CREATE INDEX IF NOT EXISTS ix_id_mapping_command_id ON id_mapping(command_id);
