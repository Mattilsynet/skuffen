ALTER TABLE id_mapping
    DROP CONSTRAINT IF EXISTS id_mapping_requires_reference;

DROP INDEX IF EXISTS uq_id_mapping_client_reference_not_null;

ALTER TABLE id_mapping
    ADD CONSTRAINT uq_id_mapping_client_reference UNIQUE (client_reference);

ALTER TABLE id_mapping
    ALTER COLUMN client_reference SET NOT NULL;
