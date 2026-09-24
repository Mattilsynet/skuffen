-- Execution v3 — operasjonsbasert eksekvering (SKU-0016).
--
-- Ren migrasjonshistorikk. Tre lag med tydelig eierskap:
--   identitet   entitet
--   fakta       sak_tilstand, journalpost_tilstand, dokument_tilstand
--   eksekvering command, operasjon, operasjon_forsok
--
-- Testen for hvor noe hører hjemme: sletter du alle operasjonsrader, skal
-- systemet fortsatt kunne svare på «hva er sant om denne saken?».

CREATE TYPE entitet_type AS ENUM ('sak', 'journalpost', 'dokument');

CREATE TYPE operasjon_status AS ENUM (
    'blokkert', 'klar', 'kjorer', 'sendt', 'retry_venter',
    'ok', 'feilet', 'krever_avklaring'
);

CREATE OR REPLACE FUNCTION set_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = now();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- ---------------------------------------------------------------------------
-- Identitet
-- ---------------------------------------------------------------------------

-- Master for skuffen_id.
CREATE TABLE entitet (
    skuffen_id       UUID PRIMARY KEY,
    entitet_type     entitet_type NOT NULL,
    client_reference UUID UNIQUE,
    arkiv_id         TEXT,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT entitet_unik_arkiv_id UNIQUE (entitet_type, arkiv_id),
    CONSTRAINT entitet_krever_referanse
        CHECK (client_reference IS NOT NULL OR arkiv_id IS NOT NULL)
);

CREATE INDEX ix_entitet_arkiv_id ON entitet (entitet_type, arkiv_id)
    WHERE arkiv_id IS NOT NULL;

CREATE TRIGGER trg_entitet_set_updated_at
BEFORE UPDATE ON entitet
FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- ---------------------------------------------------------------------------
-- Mottaksjournal
-- ---------------------------------------------------------------------------

-- Idempotency-nøkkelen er dispatchet_at, ikke radens eksistens (D24).
-- Raden skrives ved mottak; dispatchet_at settes etter vellykket dispatch.
--
-- Ingen payload-kolonne: `dekomponer` trenger bare command_type pluss det som
-- allerede er materialisert i state, og det klienten faktisk sendte ligger i
-- `arkiv_command_inbox`. 
CREATE TABLE command (
    command_id     UUID PRIMARY KEY,
    correlation_id UUID,
    command_type   TEXT NOT NULL,
    mottatt_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    dispatchet_at  TIMESTAMPTZ,
    dekomponert_at TIMESTAMPTZ,
    CONSTRAINT command_command_type_check CHECK (command_type IN (
        'opprett_sak',
        'opprett_inngaaende_journalpost',
        'opprett_utgaaende_journalpost',
        'opprett_internt_notat_journalpost',
        'avslutt_sak',
        'sett_saksansvarlig'
    ))
);

CREATE INDEX ix_command_udispatchet ON command (mottatt_at)
    WHERE dispatchet_at IS NULL;

-- ---------------------------------------------------------------------------
-- Fakta
-- ---------------------------------------------------------------------------

CREATE TABLE sak_tilstand (
    sak_id                          UUID PRIMARY KEY REFERENCES entitet(skuffen_id),
    tilstand                        TEXT NOT NULL,
    -- Materialisert ved dekomponering (D26). Executor leser aldri payload.
    sakstittel                      TEXT,
    arkivdel                        TEXT,
    ordningsverdi                   TEXT,
    saksbehandler_id                TEXT,
    saksbehandler_enhet             TEXT,
    tilgangskode                    TEXT,
    tilgangshjemmel                 TEXT,
    oensket_saksansvarlig_id        TEXT,
    oensket_saksansvarlig_enhet     TEXT,
    naavaerende_saksansvarlig_id    TEXT,
    naavaerende_saksansvarlig_enhet TEXT,
    opprettet_av_command_id         UUID NOT NULL REFERENCES command(command_id),
    created_at                      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at                      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT sak_tilstand_tilstand_check
        CHECK (tilstand IN ('ikke_opprettet', 'opprettet', 'avsluttet')),
    CONSTRAINT sak_tilstand_tilgang_par_check
        CHECK ((tilgangskode IS NULL) = (tilgangshjemmel IS NULL))
);

CREATE TRIGGER trg_sak_tilstand_set_updated_at
BEFORE UPDATE ON sak_tilstand
FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TABLE journalpost_tilstand (
    journalpost_id          UUID PRIMARY KEY REFERENCES entitet(skuffen_id),
    sak_id                  UUID NOT NULL REFERENCES sak_tilstand(sak_id),
    tilstand                TEXT NOT NULL,
    journalposttype         TEXT NOT NULL,
    med_utsending           BOOLEAN NOT NULL DEFAULT false,
    -- Materialisert ved dekomponering (D26).
    tittel                  TEXT,
    dokument_dato           TEXT,
    saksbehandler_id        TEXT,
    saksbehandler_enhet     TEXT,
    tilgangskode            TEXT,
    tilgangshjemmel         TEXT,
    korrespondanseparter    JSONB,
    kildesystem             TEXT,
    opprettet_av_command_id UUID NOT NULL REFERENCES command(command_id),
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT journalpost_tilstand_journalposttype_check
        CHECK (journalposttype IN ('I', 'U', 'X')),
    -- Skuffen oppretter aldri direkte i journalfoert; hver overgang er en
    -- egen operasjon (SKU-0016 R10).
    CONSTRAINT journalpost_tilstand_tilstand_check
        CHECK (tilstand IN (
            'ikke_opprettet', 'opprettet', 'klar_for_ekspedering',
            'ekspedert', 'journalfoert', 'avskrevet'
        )),
    CONSTRAINT journalpost_tilstand_utsending_check
        CHECK (NOT med_utsending OR journalposttype = 'U'),
    CONSTRAINT journalpost_tilstand_tilgang_par_check
        CHECK ((tilgangskode IS NULL) = (tilgangshjemmel IS NULL)),
    CONSTRAINT journalpost_tilstand_korrespondanseparter_check
        CHECK (korrespondanseparter IS NULL OR jsonb_typeof(korrespondanseparter) = 'array')
);

CREATE INDEX ix_journalpost_tilstand_sak_id ON journalpost_tilstand (sak_id);
CREATE INDEX ix_journalpost_tilstand_command ON journalpost_tilstand (opprettet_av_command_id);

CREATE TRIGGER trg_journalpost_tilstand_set_updated_at
BEFORE UPDATE ON journalpost_tilstand
FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TABLE dokument_tilstand (
    dokument_id                 UUID PRIMARY KEY REFERENCES entitet(skuffen_id),
    journalpost_id              UUID NOT NULL REFERENCES journalpost_tilstand(journalpost_id),
    tilstand                    TEXT NOT NULL,
    -- Hoveddokument gjøres eksplisitt fremfor posisjonelt (D27).
    rekkefolge                  INT NOT NULL,
    er_hoveddokument            BOOLEAN NOT NULL,
    -- Materialisert ved dekomponering (D26).
    tittel                      TEXT,
    filtype                     TEXT,
    dokument_referanse          UUID,
    mal_referanse               UUID,
    felter                      JSONB,
    rendered_dokument_referanse UUID,
    opprettet_av_command_id     UUID NOT NULL REFERENCES command(command_id),
    created_at                  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at                  TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT dokument_tilstand_tilstand_check
        CHECK (tilstand IN ('avventer_rendring', 'klar', 'ok')),
    -- Hoveddokumentet er første dokument i kommandoens liste. Denne ene
    -- invarianten holdes i databasen fordi den ellers bare overlever som en
    -- bivirkning av at id-ene genereres i payload-rekkefølge (D27). Unik
    -- rekkefolge er det som gjør at «er_hoveddokument» betyr *nøyaktig ett*.
    --
    -- Resten av dokumentreglene — at bytes og mal utelukker hverandre, og at
    -- en rendret referanse forutsetter en mal — lever i domenekoden.
    CONSTRAINT dokument_tilstand_hoveddokument_check
        CHECK (er_hoveddokument = (rekkefolge = 0)),
    CONSTRAINT dokument_tilstand_unik_rekkefolge UNIQUE (journalpost_id, rekkefolge)
);

CREATE INDEX ix_dokument_tilstand_journalpost_id ON dokument_tilstand (journalpost_id);
CREATE INDEX ix_dokument_tilstand_command ON dokument_tilstand (opprettet_av_command_id);

CREATE TRIGGER trg_dokument_tilstand_set_updated_at
BEFORE UPDATE ON dokument_tilstand
FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- ---------------------------------------------------------------------------
-- Eksekvering
-- ---------------------------------------------------------------------------

CREATE TABLE operasjon (
    operasjon_id    UUID PRIMARY KEY,
    command_id      UUID NOT NULL REFERENCES command(command_id),
    operasjonstype  TEXT NOT NULL,
    -- Svak FK: databasen garanterer at entiteten finnes, ikke at typen passer
    -- operasjonstypen. Domeneregler lever i domenekoden (D28).
    entitet_id      UUID NOT NULL REFERENCES entitet(skuffen_id),
    -- Denormalisert partisjonsnøkkel, ikke identitet (D29).
    sak_id          UUID NOT NULL REFERENCES sak_tilstand(sak_id),
    status          operasjon_status NOT NULL DEFAULT 'blokkert',
    attempt_no      INT NOT NULL DEFAULT 0,
    neste_forsok_at TIMESTAMPTZ,
    blokkert_av     UUID REFERENCES operasjon(operasjon_id),
    siste_detalj    TEXT,
    sendt_at        TIMESTAMPTZ,
    ferdig_at       TIMESTAMPTZ,
    -- Advisory 24-timersvarsel (D11). Markeres for å ikke gjenta seg.
    varslet_at      TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Gjør dekomponering idempotent strukturelt: en replay setter inn null
    -- rader, og rows_affected er signalet om det var første gang.
    CONSTRAINT operasjon_unik_per_command UNIQUE (command_id, operasjonstype, entitet_id),
    CONSTRAINT operasjon_attempt_no_check CHECK (attempt_no >= 0),
    CONSTRAINT operasjon_ferdig_at_check
        CHECK ((status IN ('ok', 'feilet')) = (ferdig_at IS NOT NULL)),
    CONSTRAINT operasjon_neste_forsok_at_check
        CHECK (status <> 'retry_venter' OR neste_forsok_at IS NOT NULL)
);

CREATE INDEX ix_operasjon_kjorbar ON operasjon (neste_forsok_at)
    WHERE status IN ('klar', 'retry_venter');
CREATE INDEX ix_operasjon_blokkert ON operasjon (sak_id)
    WHERE status = 'blokkert';
CREATE INDEX ix_operasjon_command_id ON operasjon (command_id);
CREATE INDEX ix_operasjon_sak_id ON operasjon (sak_id);
-- Recovery: operasjoner med ukjent utfall etter crash (D9).
CREATE INDEX ix_operasjon_sendt ON operasjon (sendt_at) WHERE status = 'sendt';
-- 24-timersvarsel: ikke-terminale operasjoner som ennå ikke er varslet.
CREATE INDEX ix_operasjon_uvarslet ON operasjon (created_at)
    WHERE varslet_at IS NULL AND status NOT IN ('ok', 'feilet');

CREATE TRIGGER trg_operasjon_set_updated_at
BEFORE UPDATE ON operasjon
FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TABLE operasjon_forsok (
    operasjon_id UUID NOT NULL REFERENCES operasjon(operasjon_id) ON DELETE CASCADE,
    attempt_no   INT NOT NULL,
    executor_id  TEXT NOT NULL,
    startet_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    avsluttet_at TIMESTAMPTZ,
    utfall       TEXT,
    detalj       TEXT,
    PRIMARY KEY (operasjon_id, attempt_no),
    CONSTRAINT operasjon_forsok_attempt_no_check CHECK (attempt_no > 0),
    CONSTRAINT operasjon_forsok_utfall_check
        CHECK (utfall IS NULL OR utfall IN (
            'ok', 'blokkert', 'retry_venter', 'feilet', 'krever_avklaring', 'avbrutt'
        )),
    CONSTRAINT operasjon_forsok_avsluttet_check
        CHECK ((avsluttet_at IS NULL) = (utfall IS NULL))
);

CREATE INDEX ix_operasjon_forsok_apen ON operasjon_forsok (operasjon_id)
    WHERE avsluttet_at IS NULL;
