ALTER TABLE id_mapping
    ALTER COLUMN client_reference DROP NOT NULL;

ALTER TABLE id_mapping
    DROP CONSTRAINT IF EXISTS uq_id_mapping_client_reference;

CREATE UNIQUE INDEX IF NOT EXISTS uq_id_mapping_client_reference_not_null
    ON id_mapping (client_reference)
    WHERE client_reference IS NOT NULL;

ALTER TABLE id_mapping
    ADD CONSTRAINT id_mapping_requires_reference
    CHECK (client_reference IS NOT NULL OR arkiv_id IS NOT NULL);
