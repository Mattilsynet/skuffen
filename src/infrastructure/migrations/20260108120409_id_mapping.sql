-- Lager typen 'entity_type' og setter sak, jp, doc
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'entity_type') THEN
        CREATE TYPE entity_type AS ENUM (
            'sak',
            'journalpost',
            'dokument'
        );
    END IF;
END
$$;

CREATE TABLE IF NOT EXISTS id_mapping (
    -- Skuffens interne, stabile id
    skuffen_id UUID PRIMARY KEY,

    -- Type entitet (sak/journalpost/dokument)
    entity_type entity_type NOT NULL,

    -- Klientens referanse (UUID, globalt unik)
    client_reference UUID NOT NULL,

    -- ID i eksternt arkiv (kan være NULL mens arkivet er nede)
    arkiv_id TEXT NULL,

    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT uq_id_mapping_client_reference UNIQUE (client_reference),

    -- Arkiv-ID er unik innenfor type når den finnes
    CONSTRAINT uq_id_mapping_type_arkiv
        UNIQUE (entity_type, arkiv_id)
);

-- Index: (entity_type, client_reference) -> skuffen_id
CREATE INDEX IF NOT EXISTS ix_id_mapping_type_client_reference
    ON id_mapping (entity_type, client_reference);

-- Index: (entity_type, arkiv_id) -> skuffen_id
CREATE INDEX IF NOT EXISTS ix_id_mapping_type_arkiv
    ON id_mapping (entity_type, arkiv_id)
    WHERE arkiv_id IS NOT NULL;

-- Hold updated_at automatisk oppdatert
CREATE OR REPLACE FUNCTION set_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = now();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_id_mapping_set_updated_at ON id_mapping;
CREATE TRIGGER trg_id_mapping_set_updated_at
BEFORE UPDATE ON id_mapping
FOR EACH ROW
EXECUTE FUNCTION set_updated_at();
