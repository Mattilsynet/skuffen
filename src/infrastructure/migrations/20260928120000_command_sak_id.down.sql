-- Kolonnen og innholdet ble innført av up-migrasjonen (SKU-0022 R3). For
-- dekomponerte kommandoer finnes koblingen fortsatt i `operasjon.sak_id`.

DROP INDEX ix_command_sak_id;
ALTER TABLE command DROP COLUMN sak_id;
