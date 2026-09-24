-- Gjenoppretter skjemaet fra før forfallsklokka, i motsatt rekkefølge.
--
-- Backfilte `neste_forsok_at`-verdier blir stående. Hvilke rader som var NULL
-- er ikke lenger kjent, og SKU-0022 R2 krever at ingen rad forsvinner — ikke
-- bit-for-bit symmetri.

DROP INDEX ix_operasjon_kjorbar;
CREATE INDEX ix_operasjon_kjorbar ON operasjon (neste_forsok_at)
    WHERE status IN ('klar', 'retry_venter');

ALTER TABLE operasjon DROP COLUMN avklaring_varslet_at;

ALTER TABLE operasjon ADD CONSTRAINT operasjon_neste_forsok_at_check
    CHECK (status <> 'retry_venter' OR neste_forsok_at IS NOT NULL);
ALTER TABLE operasjon ALTER COLUMN neste_forsok_at DROP NOT NULL;
ALTER TABLE operasjon ALTER COLUMN neste_forsok_at DROP DEFAULT;
