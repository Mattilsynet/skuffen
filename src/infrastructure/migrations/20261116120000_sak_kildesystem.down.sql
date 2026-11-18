DO $$
BEGIN
    LOCK TABLE sak_tilstand IN ACCESS EXCLUSIVE MODE;
    IF EXISTS (SELECT 1 FROM sak_tilstand WHERE kildesystem IS NOT NULL) THEN
        RAISE EXCEPTION 'Kan ikke fjerne kildesystem fra sak_tilstand uten å miste data';
    END IF;
END;
$$;

ALTER TABLE sak_tilstand DROP COLUMN kildesystem;
