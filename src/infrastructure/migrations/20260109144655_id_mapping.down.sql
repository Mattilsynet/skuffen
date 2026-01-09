DROP TRIGGER IF EXISTS trg_id_mapping_set_updated_at ON id_mapping;
DROP FUNCTION IF EXISTS set_updated_at();

DROP TABLE IF EXISTS id_mapping;

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_type WHERE typname = 'entity_type') THEN
        DROP TYPE entity_type;
    END IF;
END
$$;
