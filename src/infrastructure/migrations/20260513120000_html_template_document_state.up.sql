ALTER TABLE dokument_tilstand
    ADD COLUMN mal_referanse UUID NULL,
    ADD COLUMN felter JSONB NULL,
    ADD COLUMN rendered_dokument_referanse UUID NULL;

ALTER TABLE dokument_tilstand DROP CONSTRAINT IF EXISTS dokument_tilstand_tilstand_check;
ALTER TABLE dokument_tilstand DROP CONSTRAINT IF EXISTS dokument_tilstand_oensket_tilstand_check;
ALTER TABLE dokument_tilstand DROP CONSTRAINT IF EXISTS dokument_tilstand_feilet_detalj_check;
ALTER TABLE dokument_tilstand DROP CONSTRAINT IF EXISTS dokument_tilstand_check;
ALTER TABLE dokument_tilstand DROP CONSTRAINT IF EXISTS dokument_tilstand_check1;
ALTER TABLE dokument_tilstand DROP CONSTRAINT IF EXISTS dokument_tilstand_check2;
ALTER TABLE dokument_tilstand DROP CONSTRAINT IF EXISTS dokument_tilstand_check3;

ALTER TABLE dokument_tilstand
    ADD CONSTRAINT dokument_tilstand_tilstand_check
        CHECK (tilstand IN ('ikke_realisert', 'avventer_rendring', 'ok', 'feilet_permanent')),
    ADD CONSTRAINT dokument_tilstand_oensket_tilstand_check
        CHECK (oensket_tilstand IN ('ok')),
    ADD CONSTRAINT dokument_tilstand_feilet_detalj_check
        CHECK (tilstand <> 'feilet_permanent' OR feil_detalj IS NOT NULL),
    ADD CONSTRAINT dokument_tilstand_template_shape_check
        CHECK (
            (mal_referanse IS NULL AND felter IS NULL)
            OR (mal_referanse IS NOT NULL AND felter IS NOT NULL AND jsonb_typeof(felter) = 'array')
        ),
    ADD CONSTRAINT dokument_tilstand_rendered_shape_check
        CHECK (rendered_dokument_referanse IS NULL OR mal_referanse IS NOT NULL);
