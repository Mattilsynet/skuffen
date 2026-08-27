# Plan: ferdigbehandle eksisterende journalpost

Status: klar for implementasjonsoppstart; produksjonssetting blokkeres av
avklaringene i §4.
Skrevet 2026-08-26 mot Skuffen `fc2dbae` og intern `lib-schemas`
`9b47b329`.

Planen er selvstendig. En ny agentsesjon skal kunne starte implementasjonen uten
å rekonstruere samtalen som førte hit.

---

## 1. Brukerreise og formål

Dette er en menneskestyrt saksbehandlingsflyt, ikke automatisk opprydding ved
avslutning av sak:

1. Et klientsystem sender `AvsluttSak`.
2. Arkivet avviser fordi en journalpost som kom inn fra et annet sted, ikke er
   ferdigbehandlet.
3. Klientsystemet viser en tydelig feil til et menneske.
4. Mennesket henter journalposten og dokumentinnholdet fra arkivet, leser og
   saksbehandler det.
5. Mennesket beslutter at journalposten kan ferdigbehandles.
6. Klientsystemet sender `FerdigbehandleJournalpost` til Skuffen med nok
   informasjon til å identifisere saken og journalposten.
7. Skuffen finner selv journalposttype og observert arkivstatus, materialiserer
   journalposten og dokumentene som lokal state, og oversetter beslutningen til
   eksisterende arkivoperasjoner.
8. Etter terminal `Fullfort` sender klienten en ny `AvsluttSak`.

Kommandoens forretningsbetydning er:

> Et menneske har lest og behandlet denne journalposten og har godkjent at
> Skuffen ferdigstiller den etter arkivreglene for dens faktiske type.

---

## 2. Avklarte beslutninger

Disse beslutningene er avtalt og skal ikke gjenåpnes under implementasjonen:

1. Det skal være **én generisk command**, ikke én per journalposttype.
2. Navnet er `FerdigbehandleJournalpost`.
3. Public payload inneholder bare:
   - `sak_key`
   - journalpostens arkiv-ID (`journalpost_id`)
4. Public payload inneholder **ikke** journalposttype.
5. Public payload inneholder **ikke** avskrivningsmåte. Skuffens eksisterende
   `Avskriv` bruker alltid `TE`.
6. Skuffen henter selv journalposttype og status fra arkivet.
7. Validator gjør arkiv-I/O:
   - resolver saken
   - henter saken med journalposter
   - henter mål-journalposten med dokumenter
   - verifiserer at journalposten tilhører saken
   - bygger et validert snapshot
8. Dekomponering gjør **ikke** arkiv-I/O.
9. Dekomponering:
   - resolver/oppretter stabile interne Skuffen-ID-er
   - materialiserer sak-, journalpost- og dokument-facts
   - oppretter eksisterende operasjoner
10. En ekstern journalpost og dens dokumenter har fortsatt
    `SkuffenJournalpostId`/`SkuffenDokumentId`. `arkiv_id` er en ekstern
    referanse ved boundary; `client_reference` er `NULL` når klienten ikke har
    opprettet entiteten.
11. Mappingen er:
    - I: `Journalfor` → `Avskriv(TE)`
    - X: `Journalfor`
    - U: normalt `SettEkspedert(E)` → `AvventJournalfort`; hvis observert status
      allerede er `F` eller `E`, skal Skuffen ikke regressere/overskrive den,
      men bare `AvventJournalfort`; `J` er idempotent ferdig
12. `E` betyr statusen som brukes for utgående journalposter uten SvarUt før
    roboten setter `J`; commanden påstår ikke at Skuffen selv har sendt brevet.
13. `AvsluttSak` skal aldri automatisk ferdigbehandle journalposter.
14. State og operasjonsplan skal skrives atomisk i samme
    dekomponeringstransaksjon.
15. Replay skal gjenbruke samme Skuffen-ID-er via unik
    `(entitet_type, arkiv_id)` og aldri lage parallelle entiteter.

---

## 3. Scope

### 3.1 Med i hovedleveransen

- Ny public `FerdigbehandleJournalpost`-command i intern `lib-schemas`.
- Public wire-mapping, routing, ingest og statuskontekst.
- Beriket, privat og versjonert ready-envelope mellom validator og
  dekomponering.
- Arkivoppslag og tilhørighetskontroll i validator.
- Stabil Skuffen-ID for ekstern sak/journalpost/dokument.
- Materialisering av observert journalpost- og dokumentstate.
- Dekomponering til eksisterende operasjoner etter type/status.
- Idempotent håndtering av allerede ferdig journalpost.
- Databaseconstraints/migrasjon og monotone conflict-regler.
- Fake arkivmodell og unit/repository/integration-tester.
- Nødvendig retting av terminal statuspublisering for
  `AlleredeUtfort`/`Ugyldig`.
- Nødvendig retting slik at en tidligere feilet `AvsluttSak` ikke permanent
  blokkerer en ny avslutning etter at journalposten er behandlet.
- Tydelig, terminal og klientvennlig klassifisering når `AvsluttSak` avvises av
  arkivet på grunn av uferdige journalposter.
- ADR og teknisk dokumentasjon.

### 3.2 Parallell klientreise som må planlegges sammen, men kan deles i egen PR

Et menneske må kunne lese journalposten før commanden sendes. Dagens production
query-path er ikke tilstrekkelig:

- `HentSak` hardkoder `inkluder_journalposter = false`
- production `HentJournalpost` bruker en `NotImplemented`-adapter
- public dokumentrespons dropper arkivets dokument-ID/URL/innhold

Minste komplette klientreise trenger derfor også:

1. `HentSak` med journalposter, eller en målrettet blocker/query-response.
2. Production `HentJournalpost` med permissiv metadata for en uferdig ekstern
   journalpost.
3. En size-safe måte å hente dokumentinnhold på; ikke legg store base64-felt i
   sak-/journalpostmetadata.

Dette kan leveres i en separat PR/plan dersom teamet vil holde write-commanden
avgrenset, men full brukerreise-akseptansen i §17.2 er ikke oppfylt før mennesket
faktisk kan lese innholdet gjennom den avtalte klientintegrasjonen.

### 3.3 Ikke med

- Bulk-ferdigbehandling.
- Automatisk gjenopptak av den opprinnelige `AvsluttSak`.
- Klientstyrt journalposttype, status, SvarUt-valg eller avskrivningskode.
- Automatisk ferdigbehandling av alle blockers på en sak.
- Endring av canonical arkivregler i `.agent/skills/arkivfag/`.
- Admin-write eller manuell databasekorrigering.

---

## 4. Avklar før produksjonsimplementasjon

Disse punktene krever ground truth, ikke antakelser. De blokkerer ikke schema-
og domainarbeid, men må være avklart før feature regnes som produksjonsklar.

### 4.1 Sikri-avskrivingsendepunkt

Koden bruker i dag:

```text
POST /api/Archive/AvskrivJournalpost
```

i `crates/sikri_client/src/api.rs`, mens checked-in canonical Swagger viser:

```text
PUT /api/Archive/SetAvskrivRestanseJournalpost
```

med `kildesystem`, `journalpostId`, `avskrivingsmaate` og `merknad`.

Verifiser mot faktisk Sikri-miljø/leverandør og legg en contract-test/fixture som
låser riktig endpoint og parametre. Ikke «fiks» dette bare ut fra navnet.

### 4.2 Hvordan avskrevet state observeres

Bekreft med ekte responsfixture om `avskrivningsmaate = TE` på
`HentJournalpost`/`HentArkivsak` er den autoritative indikatoren på at en
inngående journalpost allerede er avskrevet. `journalstatus = J` alene er ikke
nok.

### 4.3 Tilhørighet og kildesystem

`HentJournalpost`-responsen har ikke parent-saksnummer. Tilhørighet må derfor
bevises ved at journalpost-ID-en finnes i `HentArkivsak(...,
inkluderJournalposter=true)`, og direkte journalpostoppslag må returnere samme
ID. Verifiser at kildesystemfilteret ikke skjuler journalposter opprettet av
andre systemer.

### 4.4 Dokumentidentitet

Verifiser representative Sikri-responser for:

- `dokumenterRespons[].dokumentId`
- `hoveddokument`
- `hoveddokId`
- rekkefølge
- om alle dokumenter alltid har stabil numerisk arkiv-ID

Validatoren skal avvise ufullstendig/tvetydig identitet; den skal ikke bruke
`unwrap_or_default()` og opprette arkiv-ID `0`.

### 4.5 `AvsluttSak`-feilfixture

Fang et sanitert ekte HTTP-status/body-eksempel for «saken har journalposter som
ikke er journalført/avskrevet». Legg positiv Sikri-feilklassifisering fra den
fixture-en. Ukjent 400/409/422 skal fortsatt følge SKU-0017 og ikke gjøres
terminalt ved gjetting.

### 4.6 Race-sikker statusovergang

Et vanlig read-before-write rundt `SettEkspedert(E)` er ikke nok: en utgående
journalpost kan gå fra R til F mellom read og write. Avklar om Sikri tilbyr en
atomisk precondition/conditional overgang eller garanterer at E-kallet ikke kan
overskrive F/J. Hvis ikke, kan ikke kravet «F overskrives aldri med E» løses
race-fritt med dagens endpoint; dette må løftes til arkivintegrasjonseier før
produksjonssetting.

---

## 5. Public kontrakt i `landdyrtilsyn-libs`

Intern git-avhengighet er del av codebase og skal endres først i:

```text
/Users/andrefosvold/src/mattilsynet/apps/public/landdyrtilsyn-libs
```

Resolved baseline er `9b47b3291e513d51c6e555d0b2212c497b5c7f3c`.

### 5.1 DTO

Endre:

```text
lib-schemas/src/skuffen/command/journalpost.rs
lib-schemas/src/skuffen/command/commands.rs
lib-schemas/README.md
```

Legg til:

```rust
#[derive(PartialEq, Eq, Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct FerdigbehandleJournalpost {
    pub sak_key: SakKey,
    pub journalpost_id: JournalpostId,
}
```

og enum-varianten:

```rust
Command::FerdigbehandleJournalpost(FerdigbehandleJournalpost)
```

Behold default externally tagged command-shape. Eksempel:

```json
{
  "FerdigbehandleJournalpost": {
    "sak_key": {
      "type": "arkivId",
      "value": "2026/12345"
    },
    "journalpost_id": "987654"
  }
}
```

`JournalpostId(String)` kan gjenbrukes på wire. Skuffens mapping skal parse til
positiv `i32` og canonicalisere tilbake til desimalstreng (`"00042"` → `"42"`),
slik at samme Sikri-ID ikke kan få flere `entitet.arkiv_id`-verdier.

### 5.2 Contract-tester

- eksakt JSON-shape
- begge `SakKey`-varianter
- `journalpost_id`, `sak_key` og nested felter er obligatoriske
- ukjent `journalposttype`, `avskrivningsmaate` og andre top-level-felter
  avvises
- ugyldig command-variant avvises
- roundtrip bevarer envelope metadata

### 5.3 Dependency-rollout

1. Implementer, test, commit og push schema-endringen i
   `landdyrtilsyn-libs` først.
2. Skuffen følger latest git HEAD; ikke legg `rev` i `Cargo.toml` (SKU-0008).
3. Noter pushet SHA og bruk `cargo update -p lib-schemas --precise <SHA>`, slik
   at lockfile-reviewet ikke løper mot en senere HEAD. Inspiser at taggede
   `lib-nats`/`lib-sql` ikke flyttes utilsiktet, og avklar den tracked nested
   `crates/sikri_client/Cargo.lock`, som har en eldre schema-revisjon.

---

## 6. Intern command- og ready-modell

### 6.1 Application command

Endre:

```text
src/application/src/command/model.rs
src/application/src/command/mod.rs
src/infrastructure/src/command/wire_mapper.rs
```

Legg til intern command med kun public intent:

```rust
pub struct FerdigbehandleJournalpostCommand {
    pub sak_key: SakKey,
    pub journalpost_arkiv_id: JournalpostArkivId,
}
```

`JournalpostArkivId` og `DokumentArkivId` er positive, canonicaliserte
`i32`-newtypes i application eller domain; bruk ikke rå `i32` videre enn
Sikri-boundaryen.
`Command::client_reference()` returnerer `None` for denne varianten.
Domain/application må fortsatt ikke importere `lib-schemas` (SKU-0013).

### 6.2 Validert snapshot

Public command inneholder med vilje ikke nok data til dekomponering. Innfør en
egen application-type, for eksempel i:

```text
src/application/src/command/validated.rs
```

Forslag:

```rust
pub struct ValidatedCommandEnvelope {
    pub command_id: Uuid,
    pub correlation_id: Option<Uuid>,
    pub payload: ValidatedPayload,
}

pub enum ValidatedPayload {
    Standard(Command),
    FerdigbehandleJournalpost {
        intent: FerdigbehandleJournalpostCommand,
        snapshot: FerdigbehandleJournalpostSnapshot,
    },
}

pub struct FerdigbehandleJournalpostSnapshot {
    pub saksnummer: String,
    pub sak_tilstand: SakTilstand,
    pub journalpost_arkiv_id: JournalpostArkivId,
    pub journalposttype: JournalpostType,
    pub tilstand: JournalpostTilstand,
    pub dokumenter: Vec<ValidertDokumentSnapshot>,
}

pub struct ValidertDokumentSnapshot {
    pub arkiv_id: DokumentArkivId,
    pub rekkefolge: u16,
    pub er_hoveddokument: bool,
}
```

Den algebraiske typen gjør «ny command uten snapshot» og «snapshot på feil
command» urepresenterbart. Constructoren skal også kreve at intent-ID og
snapshot-ID er like.

Hold snapshotet minimalt. Ikke legg inn tittel, personnavn, parter, base64 eller
annen informasjon som dekomponering/guards ikke trenger. Type/status/dokument-ID
er arkivavledede facts, ikke klientinput.

Public wire-mapping blir nå fallible fordi ID-newtypen valideres. Command
listeneren skal validere/mape hele batchen før `IngestCommandService` kalles;
én ugyldig ID skal gi eksisterende `invalid payload format` og null partial
ingest.

### 6.3 Privat ready-wire

Dagens validator publiserer den opprinnelige public envelopen på ready-streamen.
Det er utilstrekkelig for denne commanden. Legg en infrastructure-private,
versjonert Serde-type, for eksempel:

```text
src/infrastructure/src/command/ready_wire.rs
```

Den mapper `ValidatedCommandEnvelope` til/fra JSON og skal ikke inn i
`lib-schemas`. Bruk en eksplisitt top-level `ready_version`; ikke en tvetydig
`#[serde(untagged)]`-fallback.

Ready-streamen beholder meldinger i 180 dager. Listeneren må derfor kunne lese:

1. legacy public `CommandEnvelope<Command>` for eksisterende command-varianter
2. ny versjonert ready-envelope

Ny `FerdigbehandleJournalpost` uten korrekt snapshot er en intern kontraktsfeil
og skal aldri dekomponeres ved å gjøre et nytt Sikri-oppslag. Ukjent version
eller malformed V1 skal NAK-es/parkeres og alarmere; den skal ikke ACK-droppes.

Oppdater:

```text
src/application/src/command/ports/validated_command_dispatcher_port.rs
src/infrastructure/src/command/adapter/nats_validated_publisher.rs
src/infrastructure/src/command/nats/dekomponering_listener.rs
src/infrastructure/src/command/mod.rs
```

Publiser fortsatt legacy-ready-shape for eksisterende commands og V1-ready bare
for den nye commanden. Rolling deploy må gates: gamle command listeners,
validatorer og dekomponeringslisteners kjenner ikke den nye varianten, og
dekomponeringskoden ACK-er i dag ukjente ready-payloads. Ikke åpne klienttrafikk
før alle gamle instanser er drenert, eller innfør en eksplisitt kompatibel
rollout i samme change.

---

## 7. Validator: arkivoppslag og tilhørighet

### 7.1 Ny port

Ikke gjenbruk legacy/query-modellene. Opprett en command-side read-port, for
eksempel:

```text
src/application/src/command/ports/arkivoppslag_port.rs
```

Porten skal returnere permissive, saniterte snapshots for:

- sak med `lukket`, canonical saksnummer og journalpost-ID-er
- journalpost med type, status, avskrivningsindikasjon og dokument-ID-er

Infrastructure implementerer porten med `sikri_client::hent_sak(..., true)` og
`sikri_client::hent_journalpost(...)`.

Relevant adapter kan ligge i:

```text
src/infrastructure/src/command/adapter/sikri_ferdigbehandling_oppslag.rs
```

Bruk samme saniterte Sikri-feilpolicy som resten av command-siden. Generaliser
duplisert mapping i `sikri_command_state_repo.rs` og
`sikri_arkiv_gateway.rs` fremfor å lage en tredje inkonsistent kopi.

### 7.2 Valideringsalgoritme

Utvid `ValidateCommandService`:

1. Resolve `sak_key`:
   - `ArkivId`: bruk requestverdien for oppslag, men canonical `saksnr` fra den
     verifiserte Sikri-responsen som persisted arkiv-ID
   - `ClientReference`: slå opp entitet, krev type `Sak` og eksisterende
     `arkiv_id`; uten arkiv-ID er kommandoen blokkert til saken er opprettet
2. Hent saken fra arkivet med journalposter.
3. Avvis dersom saken ikke finnes.
4. Hent journalposten direkte med dokumenter.
5. Krev at request-ID, direkte respons-ID og ID-en i sakens journalpostliste er
   identiske.
6. Avvis dersom journalposten ikke tilhører saken.
7. Map type:
   - `I` → `Inngaende`
   - `X` → `InterntNotat`
   - `U` → `Utgaaende`
   - kjent, eksplisitt unsupported kode → terminal invalid request
   - manglende/ukjent respons-shape → recoverable `sikri_response_unparsable`
8. Map observert state uten regresjon:
   - I/X: åpen status → `Opprettet`; `J` → `Journalfoert`; I med enhver
     autoritativ eksisterende avskrivning (`TE`, `TO`, `TLF`, eller annen
     verifisert gyldig kode) → `Avskrevet`
   - U: `R`/åpen → `Opprettet`; `F` → `KlarForEkspedering`; `E` →
     `Ekspedert`; `J` → `Journalfoert`
9. Krev unike positive dokument-ID-er og deterministisk rekkefølge.
10. Krev nøyaktig ett hoveddokument og konsistens mellom `hoveddok_id`,
    `hoveddokument` og dokument-ID. Bevar eksisterende lokal rekkefølge; for ren
    import brukes verifisert arkivrekkefølge, ellers stabil ID-sortering. Ikke
    bruk response-arrayets tilfeldige rekkefølge som identitet.
11. Bygg snapshotet og dispatch den berikede envelopen.

Validator skal ikke skrive `sak_tilstand`, `journalpost_tilstand`,
`dokument_tilstand` eller `operasjon`.

For en allerede terminal journalpost skal en retry kunne valideres og ende som
idempotent success, også om saken siden er avsluttet. Snapshotet må da bære
`SakTilstand::Avsluttet`, slik at dekomponering ikke seeder den lokalt som åpen.
Dersom mutasjon fortsatt gjenstår på en avsluttet sak, returner `Conflict`.

### 7.3 Tester

Utvid `validate_command_test.rs` med fakes for:

- begge sak-key-varianter
- client reference uten arkiv-ID
- ukjent/wrong-type client reference
- ukjent sak/journalpost
- journalpost på annen sak
- mismatch mellom requested og returned journalpost-ID
- I/X/U og alle relevante statuser
- I/J med og uten avskrivningsindikasjon
- manglende/ukjent type/status
- manglende, duplikate eller ugyldige dokument-ID-er
- `journalposter = None` etter eksplisitt include klassifiseres recoverable
- recoverable Sikri-feil
- lukket sak med terminalt no-op versus gjenstående mutasjon
- ingen lokale state-writes
- dispatcher mottar nøyaktig validert snapshot

---

## 8. Ingest, routing og statuskontekst

Oppdater exhaustive matches i:

```text
src/application/src/command/services/ingest_command.rs
src/infrastructure/src/command/wire_routing_token.rs
src/infrastructure/src/command/nats/command_listener.rs
src/infrastructure/src/command/wire_mapper.rs
src/domain/src/eksekvering/typer.rs
```

Regler:

- routing token er `journalpost`
- commanden har ingen mediareferanser ved ingest
- ingest skal ikke mint sak-/journalpost-/dokument-ID-er for denne commanden;
  canonical arkiv-ID-er adopteres i dekomponering
- `CommandTypeCode` får `FerdigbehandleJournalpost` med lagret kode
  `ferdigbehandle_journalpost`
- migrasjonens `command_type` CHECK utvides
- `kontekst()` fyller sak-kontekst og `journalpost_arkiv_id`
- legg canonical `intent_hash` på command-raden; samme `command_id` med annet
  target/payload er `Conflict` og gir null videre writes
- legg `validated_basis_hash` ved første dekomponering; divergent ready-replay
  er konflikt, ikke en ny merge
- command-/operasjonsstatus skal derfor vise hvilken journalpost klienten ba om

Hashene beregnes fra en eksplisitt versjonert canonical representation, ikke en
tilfeldig Rust-`Hash` eller JSON key-order. Persistér hash-version sammen med
verdien og pin testvektorer.

`SkuffenOperasjonstype` trenger ingen ny variant; commanden bruker bare
eksisterende operasjoner.

---

## 9. Identitet og materialisering i dekomponering

### 9.1 Identitetsregler

For hver ekstern ID:

```text
(Sak, saksnummer)              → stabil SkuffenSakId
(Journalpost, journalpost-id)  → stabil SkuffenJournalpostId
(Dokument, dokument-id)        → stabil SkuffenDokumentId
```

Canonicaliser numeriske ID-er før lookup. `entitet` beholder:

- `skuffen_id`: alltid satt
- `entitet_type`: riktig type
- `client_reference`: `NULL` for eksternt adopterte entiteter, med mindre en
  eksisterende lokal mapping allerede har den
- `arkiv_id`: canonical ekstern ID

Hvis samme arkiv-ID allerede er knyttet til en lokalt opprettet entitet, skal
den eksisterende Skuffen-ID-en gjenbrukes. Ikke opprett en ny entitet.

ID-oppslagene kan **ikke** utføres med dagens autocommittende
`EntitetRepository::hent_eller_opprett_for_arkiv_id` før
`lagre_dekomponering`; da er identitet skrevet selv om state/operasjoner senere
ruller tilbake. Innfør en transactional dekomponerings-unit-of-work/repository
som i én PostgreSQL-transaksjon resolver/oppretter alle ID-er, bygger typed
`Dekomponeringsinput`, kaller ren `domain::dekomponer`, merger state/target,
skriver operasjoner og setter `dekomponert_at`.

### 9.2 Materialiseringsmodell

Dagens typer forutsetter create-command:

```text
JournalpostRad.client_reference: Uuid
DokumentRad.client_reference: Uuid
JournalpostRad starter alltid ikke_opprettet
DokumentRad krever Bytes eller HtmlTemplate
```

Utvid `src/application/src/command/materialisering.rs` slik at rader kan bære:

- optional `client_reference`
- canonical `arkiv_id`
- initial observert tilstand
- type
- typed utsendingsopprinnelse (`Ukjent | MedSvarUt | UtenSvarUt`) fremfor å
  fabrikere `false`
- dokumentkilde `EksisterendeIArkiv`

Ikke fabriker object-store-referanse, template eller utsendingsintensjon.

### 9.3 Database og migrasjon

Lag neste forward/down migration. Minimum:

1. Utvid `command_command_type_check` med
   `ferdigbehandle_journalpost`.
2. Representer `med_utsending` som ukjent for adopterte journalposter, for
   eksempel `BOOLEAN NULL`, og map den til typed
   `Ukjent | MedSvarUt | UtenSvarUt`; eksisterende create-commands setter
   fortsatt eksplisitt true/false.
3. Tillat dokumentstate med arkiv-ID og uten lokal
   `dokument_referanse`/`mal_referanse`.
4. Behold unik `(entitet_type, arkiv_id)`, parent-FK-er, unik
   `(journalpost_id, rekkefolge)` og hoveddokumentinvarianten.

En egen DB-kolonne for dokumentkilde er bare nødvendig dersom null/null ellers
er tvetydig. Domain-facts må uansett ha en eksplisitt
`DokumentKildeTilstand::EksisterendeIArkiv`.

Skill create- og importmaterialisering eksplisitt (`Ny` versus
`EksisterendeIArkiv`) i stedet for å gjøre alle create-attributter tilfeldig
optional. Oppdater `postgres_fakta_repository.rs`: facts-lesing må forstå
importerte dokumenter, mens executorens create-attributt-oppslag skal returnere
`None` for dem.

### 9.4 Atomisk og monotont upsert

Endre `postgres_dekomponering.rs` og planmodellene slik at samme transaksjon:

1. sikrer entitetsmappingene
2. sikrer/seed-er saken med snapshotets observerte `Opprettet`/`Avsluttet`
3. skriver/merger journalpoststate
4. skriver/merger alle dokumenter som `Ok`
5. skriver operasjonsradene
6. setter `dekomponert_at`

`ON CONFLICT DO NOTHING` er ikke nok. Ved konflikt skal repositoryet:

- verifisere samme parent-sak/journalpost
- verifisere samme entitetstype og journalposttype
- aldri regressere lokal state fra `J`/avskrevet eller `F`/`E` til åpen
- bevare eksisterende client reference og opprinnelig provenance
- kunne fremme eldre lokal state til et nyere validert snapshot
- rulle hele dekomponeringen tilbake ved parent/type/identity-konflikt

Formaliser samme monotone merge i dekomponering og execution/faktaoppdatering:

```text
Sak: Opprettet < Avsluttet
I:   Opprettet < Journalfoert < Avskrevet
X:   Opprettet < Journalfoert
U:   Opprettet < F/E < J  (F og E må behandles etter verifisert arkivregel;
                           aldri blindt regressere/overskrive mellom dem)
Dokument: Ok degraderes aldri
```

### 9.5 Hoveddokument-ID for allerede lokale journalposter

Vanlig `OpprettJournalpost` lagrer i dag journalpostens arkiv-ID, men ikke
Sikris `hoveddok_id` på dokumententiteten. Senere adopsjon kan da lage en ny
dokumententitet og kollidere på `rekkefolge = 0`.

Utvid:

```text
OpprettJournalpostResultat
Faktaoppdatering::JournalpostOpprettet
eksekver_operasjon.rs
postgres_operasjon_repository.rs
```

slik at `hoveddok_id` lagres som dokumentets `entitet.arkiv_id` ved vanlige nye
opprettelser. Vedlegg lagrer allerede ID når Sikri returnerer den; bevar denne
stien. Definer følgende reconciliation:

1. Eksisterende archive-ID-mapping vinner, og parent/rekkefølge verifiseres.
2. Mangler mapping, kan en lokal slot med `arkiv_id = NULL` adopteres bare når
   den matcher entydig hoveddokument/stabil dokumentidentitet.
3. Ulike mapping- og slot-ID-er er konflikt og ruller tilbake.
4. Arrayposisjon alene er aldri nok uten verifisert stabil ordering.

---

## 10. Domain-dekomponering og guards

### 10.1 Ny dekomponeringsinput

Utvid `src/domain/src/command/mod.rs`:

```rust
Dekomponeringsinput::FerdigbehandleJournalpost {
    sak_id: SkuffenSakId,
    journalpost_id: SkuffenJournalpostId,
    journalposttype: JournalpostType,
    observert_tilstand: JournalpostTilstand,
}
```

`dekomponer()` er fortsatt ren: den er en funksjon av det validerte,
materialiserbare inputet. Oppdater SKU-0016s ordlyd fra «public command payload»
til «validert dekomponeringsinput».

### 10.2 Operasjonsmatrise

| Type | Observert state | Operasjoner |
| :-- | :-- | :-- |
| I | åpen | `Journalfor`, `Avskriv` |
| I | `Journalfoert` | `Journalfor`, `Avskriv` (`Journalfor` short-circuiter) |
| I | `Avskrevet` | samme stabile plan; begge short-circuiter |
| X | åpen | `Journalfor` |
| X | `Journalfoert` | `Journalfor` short-circuiter |
| U | åpen/R | `SettEkspedert`, `AvventJournalfort` |
| U | F | `AvventJournalfort` |
| U | E | `AvventJournalfort` |
| U | J | `AvventJournalfort` short-circuiter |

Opprett alltid minst én operasjon, også for terminal snapshot. Dagens
`CommandOutcome` blir ellers permanent `Uavklart` ved null operasjoner.

### 10.3 Typeguards

Styrk `vurder()` i `operasjon.rs`:

- `Journalfor` tillates bare for I/X
- `Avskriv` tillates bare for I og krever J
- `SettEkspedert`, `KlargjorForEkspedering` og `AvventJournalfort` tillates bare
  for U
- `SettEkspedert` skal anse F/E/J som allerede forbi steget; F må aldri
  overskrives med E
- allerede J/avskrevet blir `AlleredeUtfort`
- imported dokumenter i `Ok` tilfredsstiller dokument-prerequisite

### 10.4 Snapshot-staleness

Det kan skje arkivendringer mellom validation og execution. Det gjelder I/X som
kan ha blitt J, I som kan ha blitt avskrevet, og særlig U som kan ha gått fra R
til F/E/J.

Velg og dokumenter én sikker løsning før coding av executor-delen:

1. observer rett før mutasjon; hvis ønsket effekt allerede finnes, skriv det
   observerte fact-et og fullfør uten write
2. bruk verifisert conditional/atomisk Sikri-semantikk for selve write-steget

Et read rett før write alene lukker ikke TOCTOU-racet. Hvis Sikri ikke støtter
conditional overgang, må §4.6 løses med arkivintegrasjonseier før garantien kan
gis. Live-observasjon før et eventuelt write må dessuten skje før operasjonen
markeres `sendt`; observasjonen er idempotent og skal ikke skape ukjent utfall.

Dette er execution-I/O og bryter ikke beslutningen om at **dekomponering** ikke
gjør arkiv-I/O. Ikke stol blindt på et gammelt snapshot.

---

## 11. Execution/status-korrekthet som feature avhenger av

### 11.1 Terminal short-circuit må publiseres

`EvaluerOperasjonerService` markerer i dag `AlleredeUtfort` direkte som `ok` og
`Ugyldig` som `feilet`, uten operasjonsstatus eller terminal command outcome.
Dermed ville I/avskrevet, X/J og U/J bli ferdig i DB uten at klienten får
`Fullfort`.

Refaktorer slik at alle terminaloverganger går gjennom én felles tjeneste som:

- persisterer overgang
- publiserer operasjonshendelse
- folder og publiserer command outcome

Ikke dupliser privat publish-logikk mellom evaluator og executor. Gjør
publisering crash-sikker: persistér outward event/message/error code i en
transactional outbox eller bruk `status_published_at` med retry og stabil NATS
message-id. En terminal DB-rad skal ikke kunne bli permanent upublisert etter
crash. Command-status skal utlede/bevare den relevante terminalårsaken, ikke
bare telle `feilet` og miste den actionable meldingen.

### 11.2 Statusmetadata for adoptert journalpost

`hent_command_metadata` finner i dag journalpostkontekst via
`journalpost_tilstand.opprettet_av_command_id = command_id`. En
ferdigbehandlingscommand virker på en journalpost som kan være opprettet av en
annen command.

Hent journalpostkontekst fra commandens/operasjonens target-entitet, eller legg
en eksplisitt command-target-relasjon. Terminal status skal inneholde requested
`journalpost_id` uavhengig av hvem som først materialiserte journalposten.

### 11.3 Ny `AvsluttSak` etter reparasjon

Dagens `vurder_avslutt_sak` krever at **alle historiske** operasjoner på saken
er `Ok`. En tidligere terminalt feilet `AvsluttSak` gjør dermed en ny
`AvsluttSak` permanent blokkert, selv etter vellykket ferdigbehandling.

Avgrens første leveranse eksplisitt: ekskluder andre `AvsluttSak`-operasjoner fra
søskenkravet; alle andre non-`Ok`-operasjoner blokkerer fortsatt. Ikke hev at
generell «current relevance» er løst uten en separat modell. Dokumenter dette
som en revisjon av SKU-0016 R3 og execution-design D4, og pin med
domain/repositorytester.

Klienten sender ny `command_id` når `AvsluttSak` forsøkes igjen.

### 11.4 Actionable avslutningsfeil

Når fixture i §4.5 er verifisert:

- legg positiv Sikri-klassifisering med stabil intern kode
- klassifiser den terminalt, ikke retry for alltid
- map til klientvennlig `PrerequisitePending` eller en ny eksplisitt public kode
  dersom kontrakteier ønsker det
- melding skal si at saken har journalposter som må leses og ferdigbehandles,
  og at klienten deretter må sende `AvsluttSak` på nytt
- behold rå Sikri-body kun på `debug!`
- command-status må bevare den actionable årsaken; ikke erstatt den ubetinget
  med generisk `ProcessingFailed`

Hvis Sikri-responsen ikke gir blocker-ID-er pålitelig, skal statusen ikke gjette
dem. Klienten finner journalpostene gjennom read-flyten i §3.2.

---

## 12. Fake arkiv, bootstrap og observability

Dagens fake-state er for enkel. Innfør et delt `Arc<FakeArkivStore>` med:

- saker og lukket-status
- journalpost-parent
- type/status/avskrivningsstatus
- dokumenter og arkiv-ID-er

Bruk samme store i validatorens oppslagsadapter og executorens
`FakeArkivGateway`, slik at E2E faktisk observerer samme arkivtilstand.
Eksponer en test-handle/call-log fra `TestEnv`, slik at tester kan seede ekstern
I/X/U i R/F/E/J, styre robotens overgang til J og verifisere hvilke writes som
faktisk ble forsøkt.

Oppdater:

```text
src/infrastructure/src/command/adapter/fake_command_state_repo.rs
src/infrastructure/src/command/adapter/fake_arkiv_gateway.rs
src/infrastructure/src/bootstrap.rs
```

Legg tracing på valideringsoppslag uten å logge tittel, parter, dokumentinnhold
eller rå response-body. Trygge identifikatorer er command-id, correlation-id,
saksnummer/journalpost-ID og saniterte error-koder etter eksisterende policy.

---

## 13. Tester

### 13.1 `lib-schemas`

Se §5.2.

### 13.2 Domain

Utvid `src/domain/src/eksekvering/operasjon/tests.rs`:

- eksakt dekomponeringsliste for hele matrisen i §10.2
- minst én operasjon for terminal startstate
- I/X/U typeguards for alle relevante operasjoner
- U/F får aldri `SettEkspedert`
- `SettEkspedert` short-circuiter F/E/J
- imported dokumentkilde i `Ok` tilfredsstiller guards
- tidligere feilet `AvsluttSak` blokkerer ikke nytt avslutningsforsøk
- annet relevant uferdig arbeid blokkerer fortsatt avslutning
- type-spesifikke monotone mergefunksjoner dekker alle tillatte og avviste
  overganger

### 13.3 Application

- validator-testene i §7.3
- dekomponering bruker snapshot uten archive-port/I/O
- samme arkiv-ID gir samme Skuffen-ID ved replay
- snapshot blir korrekt `Dekomponeringsplan`
- statuskontekst inneholder sak og journalpostarkiv-ID
- evaluator publiserer terminal status ved `AlleredeUtfort` og `Ugyldig`
- stale U-snapshot kan ikke sette E over observert F/J
- stale I/X-snapshot short-circuiter når live state allerede er J/avskrevet
- same `command_id` med annet intent avvises uten partial ingest
- same `command_id` med divergent validert snapshot avvises uten state-merge
- canonical intent-/basis-hash matcher låste testvektorer
- terminal DB-commit etterfulgt av publish-crash blir etterpublisert eksakt én
  logisk gang

### 13.4 PostgreSQL repository

Utvid `src/infrastructure/tests/postgres_operasjon_repository_test.rs` eller
opprett målrettet dekomponeringstest med testcontainers:

1. Ekstern journalpost/dokumenter persisteres med Skuffen-ID og arkiv-ID, uten
   client reference.
2. State og operasjoner committes atomisk.
3. Replay samme command-id setter ikke inn duplikater.
4. Ny command-id mot samme journalpost gjenbruker samme entiteter.
5. Parent/type-konflikt ruller alt tilbake.
6. State merge er monoton for åpen/F/E/J/avskrevet.
7. Dokument-ID og rekkefølge er stabile.
8. Lokal hoveddokumentmapping reconciles med imported snapshot.
9. `hent_command_metadata` returnerer target journalpost-ID.
10. Ny `AvsluttSak` ignorerer gammelt feilet avslutningsforsøk, men ikke annet
    uferdig relevant arbeid.
11. To samtidige commands som adopterer samme sak/journalpost/dokument ender med
    én stabil mapping.
12. Feil injisert etter hvert delsteg (identity, state, target, operation,
    `dekomponert_at`) beviser full rollback.
13. Samtidig snapshot-merge J og execution-write E kan ikke regressere state.

### 13.5 NATS/integration

Minste command-E2E:

1. Public payload → inbox → validator enrichment → ready → state + operasjoner
   → terminal `Fullfort`.
2. Tilhørighetsmismatch gir terminal `Avvist` og null journalpost-/dokumentstate
   og operasjoner.
3. I åpen → J → TE.
4. I/J → bare effektiv TE.
5. I allerede avskrevet → terminal idempotent success uten Sikri-write.
6. X åpen/J.
7. U/R → E → poll J.
8. U/F → aldri E; poll J.
9. U/E → poll J.
10. U/J → terminal idempotent success.
11. Samme command-id replay.
12. Ny command-id mot allerede ferdig journalpost.
13. Status inneholder journalpostens arkiv-ID.
14. Gammel feilet `AvsluttSak` → ferdigbehandling → ny `AvsluttSak` lykkes.
15. Verifisert Sikri blocker-feil gir tydelig terminal client-status.
16. Legacy ready-envelope for eksisterende command-varianter kan fortsatt
    konsumeres.
17. Legacy-shape med ny command uten snapshot, ukjent ready-version og malformed
    V1 blir ikke ACK-droppet.
18. Canonical aliases (`"00042"`/`"42"`) gir samme journalpost- og
    dokumentmapping; canonical saksnummer fra Sikri gjenbrukes.
19. I allerede avskrevet med TE, TO eller TLF er no-op og overskrives ikke.
20. Lukket sak + terminal journalpost-no-op materialiserer saken som
    `Avsluttet`, ikke `Opprettet`.
21. Endret dokument-arrayrekkefølge påvirker ikke identitet/rekkefølge.
22. Ugyldig ID i én command-batch gir null partial ingest.

Komplett user-journey-E2E skal i tillegg hente metadata og dokumentinnhold før
ferdigbehandlingscommanden sendes.

---

## 14. ADR og dokumentasjon

Opprett neste ledige Skuffen-ADR. Ikke anta nummeret; `SKU-0018` kan bli tatt av
det eksisterende ucommittede admin-planarbeidet. Kjør først:

```bash
cargo run -p adr-fmt -- --guidelines
cargo run -p adr-fmt -- --context skuffen
cargo run -p adr-fmt -- --critique SKU-0016
```

ADR-en skal beslutte:

- human-in-the-loop-betydningen
- minimal public payload
- validation leser arkivet; dekomponering gjør det ikke
- privat beriket ready-envelope
- ArkivId → stabil SkuffenId-adopsjon
- atomisk/monoton state-materialisering
- I/X/U-operasjonsmatrisen
- staleness/idempotency-reglene
- retry av `AvsluttSak` etter tidligere terminal feil
- rollout/legacy-ready-strategien
- intent-/snapshot-hash og crash-sikker statuspublisering

Oppdater eller presiser:

```text
docs/adr/skuffen/SKU-0016-operasjonsbasert-eksekvering.md
.agent/guides/architecture/command/commands.md
.agent/guides/architecture/id_mapping_and_idempotency.md
.agent/guides/architecture/design_guidelines.md
.agent/guides/observability.md
docs/execution_v3_design.md
README.md
```

SKU-0016 R2 skal si at dekomponering er en ren funksjon av **validert
dekomponeringsinput**, ikke nødvendigvis bare public payload.

Canonical `.agent/skills/arkivfag/` beskriver allerede typeflytene. Ikke skriv
dem om eller reinterpreter dem. Endre bare canonical ressurs dersom feature-en
avdekker en faktisk, verifisert feil i fasiten.

Etter ADR-endringer:

```bash
cargo run -p adr-fmt -- --lint
```

---

## 15. Implementasjonsrekkefølge

1. Start fra committed HEAD. Bevar urelaterte untracked filer; current baseline
   har `.githooks/`, `deny.toml` og `docs/plan-admin-read-mvp.md` som urelatert
   arbeid.
2. Verifiser Sikri-punktene i §4 og lag saniterte fixtures.
3. Skriv/land ADR-en og presiser SKU-0016 R2/R3 og execution-design D4.
4. Implementer public DTO + contract-tester i `landdyrtilsyn-libs`.
5. Commit/push schema repo, noter eksakt SHA, og kjør
   `cargo update -p lib-schemas --precise <SHA>`. Inspiser root `Cargo.lock` og
   avgjør eksplisitt om tracked `crates/sikri_client/Cargo.lock` skal oppdateres;
   den peker på en eldre schema-revisjon. Flytt ikke taggede `lib-nats`/`lib-sql`.
6. Implementer application command, canonical ID-mapping og public wire-routing.
7. Innfør validert snapshot og legacy-kompatibel privat ready-envelope.
8. Implementer arkivoppslagsport, Sikri-adapter og fake store.
9. Implementer validatorens tilhørighetskontroll og enrichment.
10. Implementer domain-dekomponering og type/status-guards.
11. Lag database-migrasjon og utvid materialiseringsmodell.
12. Implementer atomisk, monotont PostgreSQL-upsert og identity-conflict checks.
13. Løs hoveddokument-/vedlegg-arkiv-ID ved vanlig opprettelse og reconciliation.
14. Rett evaluatorens terminalpublisering og command target-metadata.
15. Rett `AvsluttSak`-retry etter tidligere feilet avslutningsforsøk.
16. Sikre execution-time stalenessvern for U/F/E/J.
17. Legg actionable Sikri blocker-feil på operation- og command-status.
18. Kjør unit-, repository- og command-E2E-testene.
19. Ferdigstill write-command-akseptansen i §17.1.
20. Implementer eller koble på den separat spesifiserte read/content-leveransen
    i §3.2 og kjør komplett user-journey-E2E i §17.2.
21. Oppdater docs og kjør alle kvalitetsporter.

Hold commits små nok til å reviewe boundaryene separat: schema, ready/validation,
domain/persistence, execution/status, read journey/docs.

---

## 16. Kvalitetssjekker

I `landdyrtilsyn-libs`:

```bash
cargo fmt --check
cargo test -p lib-schemas --features skuffen
cargo clippy -p lib-schemas --all-targets --features skuffen -- -D warnings
cargo test --workspace --all-features
```

I Skuffen:

```bash
cargo fmt --check
cargo check
cargo test -p domain
cargo test -p application
cargo test -p infrastructure
cargo test --workspace --exclude skuffen-integration-tests
cargo clippy --all-targets --all-features -- -D warnings
cargo test -p skuffen-integration-tests -- --nocapture
cargo run -p adr-fmt -- --lint
```

Ved commit/push: la ordinære hooks kjøre. Ikke bruk `--no-verify`, `-n` eller
skip-variabler.

---

## 17. Akseptansekriterier

### 17.1 Write-commanden

Write-commanden er ferdig når:

- klienten kan sende én generic `FerdigbehandleJournalpost` med kun sak-key og
  journalpostens arkiv-ID
- type og avskrivningsmåte kan ikke sendes i public payload
- validator verifiserer journalpostens tilhørighet til saken mot arkivet
- validator gjør ingen lokale state-writes
- ready-meldingen bærer et validert minimalt snapshot
- dekomponering gjør ingen Sikri-I/O
- ekstern journalpost og dokumenter har stabile Skuffen-ID-er og materialisert
  state med arkiv-ID, uten fabrikkert client reference/mediareferanse
- state og operasjoner skrives atomisk og monotont
- I/X/U følger matrisen i §10.2
- U/F overskrives aldri med E
- allerede ferdig journalpost gir terminal idempotent `Fullfort`
- statusen peker på riktig journalpost selv om den ble materialisert av en annen
  command
- en tidligere feilet `AvsluttSak` forgifter ikke en ny avslutning etter
  vellykket ferdigbehandling
- en ekte arkivavvisning ved uferdige journalposter gir en tydelig terminal
  melding til klienten, ikke evig generic retry
- legacy ready-meldinger og eksisterende commands fungerer fortsatt
- ADR, docs, migrations, unit-, repository- og integration-tester er oppdatert
- fmt, check, clippy, workspace-tester, integration-tester og ADR-lint passerer

### 17.2 Komplett brukerreise

Den komplette brukerreisen er ferdig når §17.1 er oppfylt og den separat
spesifiserte read/content-leveransen lar et menneske:

- se den tydelige `AvsluttSak`-feilen
- identifisere aktuell journalpost på saken
- hente journalpostmetadata og lese alle relevante dokumenter
- sende `FerdigbehandleJournalpost`
- observere terminal `Fullfort`
- sende ny `AvsluttSak` med ny command-id og få den fullført

---

## 18. Verifisert grunnlag og ikke-verifisert

Verifisert direkte i current source, tests, migrations, ADR-er og intern
git-avhengighet:

- `entitet` støtter entiteter med kun arkiv-ID og har unik
  `(entitet_type, arkiv_id)`
- journalpost-/dokumentstate bruker interne Skuffen-ID-er
- eksisterende operasjoner dekker J, E, F, vent på J og TE
- dekomponering og state skrives i samme transaksjon i dagens design
- dagens materialisering forutsetter client reference og lokal dokumentkilde
- dagens ready-payload er bare public command-envelope
- dagens evaluator publiserer ikke terminalstatus for `AlleredeUtfort`
- dagens statusmetadata finner journalpost via opprettelses-command
- dagens `AvsluttSak` ser alle historiske operasjoner, også tidligere feilet
  avslutning
- production `HentJournalpost` er ikke implementert, og dokumentinnhold mangler
  en public read-path

Ikke live-verifisert:

- riktig avskrivingsendepunkt i nåværende Sikri-miljø
- nøyaktig avskrivningsindikator i runtime-respons
- om kildesystemfilteret viser journalposter fra andre systemer
- reell blocker-error body/status ved `AvsluttSak`
- dokument-ID-/hoveddokumentfeltenes komplette runtime-shape

Disse er eksplisitte avklaringer i §4 og skal ikke presenteres som etablerte
fakta under implementasjonen.
