-- Revert add command_id to id_mapping table
DROP INDEX IF EXISTS ix_id_mapping_command_id;
ALTER TABLE id_mapping DROP COLUMN IF EXISTS command_id;
