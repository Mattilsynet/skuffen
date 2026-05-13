ALTER TABLE dokument_tilstand DROP CONSTRAINT IF EXISTS dokument_tilstand_rendered_shape_check;
ALTER TABLE dokument_tilstand DROP CONSTRAINT IF EXISTS dokument_tilstand_template_shape_check;
ALTER TABLE dokument_tilstand DROP CONSTRAINT IF EXISTS dokument_tilstand_feilet_detalj_check;
ALTER TABLE dokument_tilstand DROP CONSTRAINT IF EXISTS dokument_tilstand_oensket_tilstand_check;
ALTER TABLE dokument_tilstand DROP CONSTRAINT IF EXISTS dokument_tilstand_tilstand_check;

ALTER TABLE dokument_tilstand
    ADD CONSTRAINT dokument_tilstand_tilstand_check
        CHECK (tilstand IN ('ikke_realisert', 'ok', 'feilet_permanent')),
    ADD CONSTRAINT dokument_tilstand_oensket_tilstand_check
        CHECK (oensket_tilstand IN ('ok')),
    ADD CONSTRAINT dokument_tilstand_feilet_detalj_check
        CHECK (tilstand <> 'feilet_permanent' OR feil_detalj IS NOT NULL);

ALTER TABLE dokument_tilstand
    DROP COLUMN rendered_dokument_referanse,
    DROP COLUMN felter,
    DROP COLUMN mal_referanse;
