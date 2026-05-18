ALTER TABLE dokument_tilstand DROP CONSTRAINT IF EXISTS dokument_tilstand_rendered_shape_check;
ALTER TABLE dokument_tilstand DROP CONSTRAINT IF EXISTS dokument_tilstand_template_shape_check;
ALTER TABLE dokument_tilstand DROP CONSTRAINT IF EXISTS dokument_tilstand_tilstand_check;

ALTER TABLE dokument_tilstand
    ADD CONSTRAINT dokument_tilstand_tilstand_check
        CHECK (tilstand IN ('ikke_realisert', 'ok', 'feilet_permanent'));

ALTER TABLE dokument_tilstand
    DROP COLUMN rendered_dokument_referanse,
    DROP COLUMN felter,
    DROP COLUMN mal_referanse;
