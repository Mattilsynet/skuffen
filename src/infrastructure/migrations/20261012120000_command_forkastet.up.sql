-- En operatør har forkastet kommandoen (SKU-0024). Skuffen gjør ikke mer
-- arbeid med den, og den holder ikke lenger saken åpen.

ALTER TABLE command ADD COLUMN forkastet_at TIMESTAMPTZ;
