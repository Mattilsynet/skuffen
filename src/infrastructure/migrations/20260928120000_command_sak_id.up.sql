-- Hvilken sak kommandoen gjelder, registrert ved mottak.
--
-- `NULL` betyr at saken ikke var kjent i
-- Skuffen da kommandoen ble mottatt, eller at raden er eldre enn kolonnen.

ALTER TABLE command ADD COLUMN sak_id UUID REFERENCES entitet(skuffen_id);

-- Dekomponerte kommandoer har koblingen allerede via operasjonene.
UPDATE command c
SET sak_id = o.sak_id
FROM (
    SELECT DISTINCT ON (command_id) command_id, sak_id
    FROM operasjon
    ORDER BY command_id, created_at
) o
WHERE o.command_id = c.command_id;

CREATE INDEX ix_command_sak_id ON command (sak_id) WHERE sak_id IS NOT NULL;
