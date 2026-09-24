-- Kjørbarhet er en forfallsklokke, ikke en statuscache (SKU-0020 R2).
--
-- `neste_forsok_at` blir obligatorisk og gjelder alle ikke-terminale statuser.
-- Workeren plukker det som har forfalt, i forfallsrekkefølge, så en permanent
-- blokkert rad ikke kan sulte ut resten av køen.

UPDATE operasjon SET neste_forsok_at = now() WHERE neste_forsok_at IS NULL;
ALTER TABLE operasjon ALTER COLUMN neste_forsok_at SET DEFAULT now();
ALTER TABLE operasjon ALTER COLUMN neste_forsok_at SET NOT NULL;
ALTER TABLE operasjon DROP CONSTRAINT operasjon_neste_forsok_at_check;

-- Varsling om ukjent utfall markeres i databasen, ikke utledet av hvor mange
-- rader recovery flyttet (SKU-0020 R6).
ALTER TABLE operasjon ADD COLUMN avklaring_varslet_at TIMESTAMPTZ;

DROP INDEX ix_operasjon_kjorbar;
CREATE INDEX ix_operasjon_kjorbar ON operasjon (neste_forsok_at)
    WHERE status IN ('klar', 'retry_venter', 'blokkert');
