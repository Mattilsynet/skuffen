# Plan: `arkiv.admin.read` MVP

Status: implementert. Se ADR
[SKU-0018](adr/skuffen/SKU-0018-admin-read-lokal-reparasjonstilstand.md) for den
varige beslutningen; denne planen beholdes som gjennomføringsdokumentasjon.
Skrevet 2026-08-25, verifisert på nytt mot `c50d895`/`765a19d` 2026-08-27.

Avvik som ble avklart under implementasjonen:

- Sak-oppslaget har maksimalt fem SELECTs: fem når `sak_tilstand` finnes, tre ved
  identity-only.
- Integrasjonsmiljøet hadde allerede NATS-config-fil og `max_payload`; den ble
  parameterisert i stedet for innført.
- Queue group-testen starter en andre admin-listener direkte mot samme NATS og
  database, og beviser køemedlemskap via NATS monitoring `/subsz`.
- Trace-parenting krevde en reell fiks: `tracing-opentelemetry` bygger OTel-spanet
  ved `on_enter`, så `set_parent` inne i en `#[instrument]`-kropp blir ignorert.
  Admin-listeneren setter derfor parent på spanet før det aktiveres.

Planen er selvstendig. En ny agentsesjon skal kunne implementere den uten å
rekonstruere brainstormingen som førte hit.

---

## 1. Formål

Ansvarsdelingen er:

- **Status-streamen:** Klientvendte lifecycle- og avvisningshendelser — hva har
  skjedd?
- **Admin read:** Hvilken tilstand må forstås før en reparasjon?
- **Admin write:** Utfør en avgrenset reparasjon og la workeren fortsette
  naturlig.

Denne leveransen bygger bare **admin read**. Den skal gi en klient eller
operatør den lokale tilstanden Skuffen faktisk kommer til å bruke dersom en
operasjon kjøres på nytt. Den skal ikke rekonstruere en hendelsestidslinje,
etterligne status-streamen eller være en generell databasekonsoll.

Typisk bruk:

1. Status-streamen viser at en kommando eller operasjon har feilet eller blitt
   avvist.
2. Operatøren slår opp kommandoen og ser den feilende operasjonens nåtilstand.
3. Operatøren slår opp saken og ser materialiserte verdier, for eksempel
   hvilken saksbehandler/enhet som ligger på akkurat saken eller journalposten.
4. Et senere admin-write-kall kan korrigere riktig mål. Write er ikke del av
   denne planen.

---

## 2. Avklarte beslutninger

Disse er allerede avtalt og skal ikke gjenåpnes i implementasjonen.

1. Admin bruker NATS core request-reply.
2. Subjects er:
   - `arkiv.admin.read.command.hent`
   - `arkiv.admin.read.sak.hent`
3. Replies bruker eksisterende `NatsResponse<T>`:
   `{"status":"Ok","payload":...}` eller
   `{"status":"Error","payload":{"message":"..."}}`.
4. Requesten har obligatorisk `utfort_av`. Det er selvdeklarert attribusjon,
   ikke autentisering. Verdien logges på `info`, men lagres ikke. Rå arkiv-id
   logges ikke.
5. Tillitsmodellen er den eksisterende NATS-tilgangen. Ingen JWT-logikk,
   application-autorisering eller kill-switch legges til.
6. Kontrakten eies av nye admin-DTO-er i
   `landdyrtilsyn-libs/lib-schemas/src/skuffen/admin/`.
7. Skuffen får en egen `admin`-vertikal i `application` og `infrastructure`.
8. MVP-en har ingen lister, søk eller paginering.
9. Original command-payload, JetStream-meldinger, media-bytes og
   `operasjon_forsok` returneres ikke.
10. Command-oppslaget returnerer alle nåværende operasjonsrader for kommandoen,
    men ikke forsøkshistorikk.
11. Sak-oppslaget returnerer full materialisert lokal state og et lett
    sammendrag av alle operasjoner på saken. Detaljer hentes via command-oppslaget.
12. Saksbehandler ved opprettelse av sak, ønsket/nåværende saksansvarlig og
    saksbehandler på hver journalpost er forskjellige begreper og eksponeres
    separat. De skal ikke flates ut eller tvinges like.
13. `client_reference`, arkiv-id-er og intern kontekst skjules ikke. De er
    nødvendige for reparasjon.
14. Valideringsavvisning er en status-hendelse og persisteres ikke på
    `command`. Admin read returnerer derfor ikke `avvist` og utleder det aldri
    fra at en command mangler operasjoner.
15. Command-feltet `utfall` er et snapshot-sammendrag utledet bare fra
    nåværende operasjonsrader. `krever_avklaring` har egen verdi og skjules ikke
    som `uavklart`.
16. SKU-0013 gjelder også tester: `domain` og `application` skal ikke importere
    eller avhenge av `lib-schemas`, heller ikke gjennom `dev-dependencies` eller
    `#[cfg(test)]`.

---

## 3. Scope

### Med

- Stabil wire-kontrakt i `lib-schemas`.
- To punkt-oppslag over NATS.
- Read-only PostgreSQL-projection av `command`, `operasjon`, `entitet`,
  `sak_tilstand`, `journalpost_tilstand` og `dokument_tilstand`.
- Current command outcome utledet fra nåværende operasjonsstatuser.
- Queue groups, trace-parent-propagation, strukturert attribusjonslogg og
  stabil feilhåndtering.
- Opprydding av den eksisterende test-only `lib-schemas`-avhengigheten i
  `application`, slik at SKU-0013 igjen håndheves av crate-grensen.
- Unit-, repository- og ende-til-ende-tester.
- README- og beslutningsdokumentasjon.

### Ikke med

- `arkiv.admin.write.*`.
- Gjenåpning eller restart av operasjoner.
- Korrigering av saksbehandler/enhet eller andre state-felt.
- Oppgjør av `krever_avklaring`.
- Ny kommando «fullfør journalpost».
- Original payload fra `arkiv_command_inbox`.
- Persistens eller rekonstruksjon av valideringsavvisning.
- Lesing eller reparasjon av status-streamen.
- Live-oppslag mot arkivet, object store eller andre tjenester.
- Egen CLI-binær. Kontrakten skal være enkel å bruke med `nats request`; en
  CLI kan bygges oppå den senere.

---

## 4. Verifisert baseline og prerequisite

### 4.1 Avvisning er ikke lokal command-state

Feilklassifiseringsarbeidet er landet. Committed source er sannheten:

- `command` har `command_id`, `correlation_id`, `command_type`, `mottatt_at`,
  `dispatchet_at` og `dekomponert_at`
- tabellen har ikke `avvist_at` eller `avvist_kode`
- `ValidateCommandService` publiserer `Avvist`, men skriver ingen
  avvisningsmarkør til PostgreSQL

Dette er bevisst og skal ikke endres av admin read. En registrert command uten
operasjoner kan fra PostgreSQL alene være blant annet ikke dekomponert, under
redelivery eller valideringsavvist. Admin read returnerer command-raden og
`utfall: "uavklart"`; den gjetter aldri `avvist`. Hvorfor en command ble avvist
tilhører status-streamen.

En request som avvises ved wire-grensen før ingest har ingen `command`-rad og
gir `Command not found` ved senere admin-oppslag.

Admin-leveransen trenger ingen migrasjon og ingen nye indekser.

### 4.2 Eksisterende schema-lekkasje i application-tester

`application` har ingen production dependency på `lib-schemas`, men har i dag
en `dev-dependency` og `#[cfg(test)]`-importer i:

```text
src/application/src/command/model.rs
src/application/src/command/wire_test_support.rs
src/application/src/command/services/ingest_command_test.rs
src/application/src/command/services/validate_command_test.rs
```

Dette dupliserer infrastructure sin wire-mapping og bryter den absolutte
SKU-0013-grensen. Før admin-vertikalen bygges:

1. fjern `lib-schemas` fra `src/application/Cargo.toml`
2. slett application sin wire-test-mapping og tilhørende exports
3. skriv application-testfixtures med interne `application`-/`domain`-typer;
   kall services med `Vec<CommandEnvelope<Command>>` og
   `CommandEnvelope<Command>` direkte, og forenkle de generiske
   `IntoCommandBatch`/`IntoCommandEnvelope`-seamene dersom de da ikke lenger har
   flere production-brukere
4. behold wire-shape- og mappingtester i henholdsvis `lib-schemas` og
   `infrastructure`

Etter oppryddingen skal verken `domain` eller `application` inneholde
`lib_schemas`-importer eller `lib-schemas`-dependencies, heller ikke test-only.

### 4.3 Status-stream og PostgreSQL kan avvike

Status-streamen er den klientvendte hendelseskanalen, men admin read skal ikke
bruke den som database. Enkelte terminale DB-overganger committes før
status-publish, og evaluator kan materialisere `ok`/`feilet` direkte. Ved feil i
publish-pathen kan derfor PostgreSQL vise `fullfort`, `feilet` eller
`krever_avklaring` uten en tilsvarende stream-hendelse. Admin read viser den
nåværende persisterte PostgreSQL-staten; den rekonstruerer, etterpubliserer eller
hevder ingenting om stream-historikken.

---

## 5. Wire-kontrakt i `lib-schemas`

### 5.1 Filer

Opprett i `landdyrtilsyn-libs`:

```text
lib-schemas/src/skuffen/admin/mod.rs
lib-schemas/src/skuffen/admin/requests.rs
lib-schemas/src/skuffen/admin/responses.rs
```

Eksporter modulen fra `lib-schemas/src/skuffen/mod.rs`, og dokumenter den i
`lib-schemas/README.md`. `admin/mod.rs` re-eksporterer request- og
response-typene, slik at kontraktens importsti er
`lib_schemas::skuffen::admin::*`.

Request-typer skal bruke `#[serde(deny_unknown_fields)]`, slik at en skrivefeil
i et CLI-kall ikke ignoreres. Feltnavnene på request-structene følger Rust-navn
uendret (`utfort_av`, `command_id`); ikke legg `rename_all` på structene.
Response-typer skal være permissive og skal ikke bruke command-side newtypes som
revaliderer historiske data. Options serialiseres eksplisitt som JSON `null`;
ikke bruk `skip_serializing_if` i admin-responsene.

Alle tidsstempler i response-DTO-ene bruker `chrono::DateTime<Utc>` og
serialiseres som RFC 3339.

UUID-kolonner bruker `Uuid`/`Option<Uuid>`. PostgreSQL `INT` bruker `i32` uten
revalidering. Lagrede koder og fritekst bruker `String`/`Option<String>`.

### 5.2 Hent command

Rust-type:

```rust
pub struct HentAdminCommandRequestV1 {
    pub utfort_av: String,
    pub command_id: Uuid,
}
```

JSON:

```json
{
  "utfort_av": "test-operator",
  "command_id": "00000000-0000-0000-0000-000000000001"
}
```

Manglende eller blank `utfort_av` er `Invalid request format`. Blankhet
valideres ved transportgrensen; verdien sendes ikke inn i application-laget.
`lib-schemas` validerer shape, mens trim-/blankhetskontrollen skjer i listeneren.

`AdminCommandResponseV1` inneholder:

| Felt | Type/semantikk |
| :-- | :-- |
| `command_id` | UUID |
| `correlation_id` | optional UUID |
| `command_type` | lagret domenekode som string |
| `mottatt_at` | timestamp |
| `dispatchet_at` | optional timestamp |
| `dekomponert_at` | optional timestamp |
| `utfall` | `uavklart`, `krever_avklaring`, `fullfort` eller `feilet` |
| `operasjoner` | alle nåværende operasjoner for commanden |

`utfall` bruker en egen `AdminCommandUtfallV1` med snake_case Serde-navn og
foldes slik, i denne rekkefølgen:

1. minst én operasjon er `feilet` → `feilet`
2. minst én operasjon er `krever_avklaring` → `krever_avklaring`
3. minst én operasjon finnes og alle er `ok` → `fullfort`
4. ellers → `uavklart`

Prioriteten betyr at `feilet + krever_avklaring` blir `feilet`, mens
`ok + krever_avklaring` blir `krever_avklaring`. En tom operasjonsliste blir
alltid `uavklart`, aldri vacuous `fullfort`. `uavklart` dekker både command før
dekomponering og command med pågående operasjoner. Det finnes ingen vedvarende
`validert_at` eller avvisningsmarkør, og admin-responsen skal ikke late som den
kjenner en mer detaljert fase.

Hver `AdminOperasjonDetaljerV1` inneholder:

- `operasjon_id`
- `operasjonstype` som string
- `entitet`: `skuffen_id`, `entitet_type`, optional `client_reference`,
  optional `arkiv_id`
- `sak_id`
- `status` som string
- `attempt_no` som `i32`
- `neste_forsok_at` som optional timestamp
- `blokkert_av` som optional UUID
- `siste_detalj` som optional string
- `sendt_at`, `ferdig_at` og `varslet_at` som optional timestamps
- `created_at`
- `updated_at`

Ingen felt fra `operasjon_forsok` inngår.
`blokkert_av` returneres som lagret. Dagens production-skrivevei setter normalt
ikke feltet, så det vil som regel være `null`; admin read skal ikke utlede en
verdi.

### 5.3 Hent sak

Rust-type:

```rust
pub struct HentAdminSakRequestV1 {
    pub utfort_av: String,
    pub key: AdminSakKeyV1,
}

#[serde(
    tag = "type",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum AdminSakKeyV1 {
    SkuffenId(Uuid),
    ClientReference(Uuid),
    ArkivId(String),
}
```

Eksempler:

```json
{
  "utfort_av": "test-operator",
  "key": {"type": "clientReference", "value": "00000000-0000-0000-0000-000000000002"}
}
```

```json
{
  "utfort_av": "test-operator",
  "key": {"type": "arkivId", "value": "2026/12345"}
}
```

`AdminSakResponseV1` deles i:

```text
identitet     entitet-raden for saken
fakta         optional materialisert sak-state med journalposter og dokumenter
operasjoner   lette operasjonssammendrag for hele saken
```

`fakta` er optional med vilje. Skuffen minter identitet ved ingest før
validering. En kjent `client_reference` kan derfor ha en `entitet`-rad uten
`sak_tilstand`. Det er viktig reparasjonsinformasjon og skal returneres som
success med `fakta: null`, ikke skjules som «Sak not found».

JSON-layouten er nestet uten `flatten`:

```text
AdminSakResponseV1
  identitet
  fakta
    journalposter[]
      identitet
      dokumenter[]
        identitet
  operasjoner[]
```

Command-operasjonens kompakte entitet-identitet og sakens fulle
entitet-identitet er separate DTO-er.

`identitet` inneholder samtlige relevante `entitet`-felt:

- `skuffen_id`
- `entitet_type`
- `client_reference`
- `arkiv_id`
- `created_at`
- `updated_at`

Materialisert sak-fakta inneholder:

- `tilstand`
- `sakstittel`
- `arkivdel`
- `ordningsverdi`
- `opprettelse_saksbehandler_id`
- `opprettelse_saksbehandler_enhet`
- `tilgangskode`
- `tilgangshjemmel`
- `oensket_saksansvarlig_id`
- `oensket_saksansvarlig_enhet`
- `naavaerende_saksansvarlig_id`
- `naavaerende_saksansvarlig_enhet`
- `opprettet_av_command_id`
- `created_at`
- `updated_at`
- `journalposter`

Navneprefikset `opprettelse_` er bevisst. Disse feltene er input til
`OpprettSak`, ikke den nåværende saksansvarlige.

Følgende sak-felt er optional akkurat som i databasen: `sakstittel`, `arkivdel`,
`ordningsverdi`, begge `opprettelse_saksbehandler_*`, begge
`oensket_saksansvarlig_*`, begge `naavaerende_saksansvarlig_*`,
`tilgangskode` og `tilgangshjemmel`. `fakta: Some` bestemmes bare av at
`sak_tilstand` finnes, ikke av om disse attributtene er satt.

Hver journalpost inneholder:

- full entitet-identitet
- `sak_id`
- `tilstand`
- `journalposttype`
- `med_utsending`
- `tittel`
- `dokument_dato`
- journalpostens egne `saksbehandler_id` og `saksbehandler_enhet`
- `tilgangskode` og `tilgangshjemmel`
- strukturerte korrespondanseparter med `rolle`, navn og de rollespesifikke
  feltene (`parttype` eller id-/adressefelter)
- `kildesystem`
- `opprettet_av_command_id`
- `created_at` og `updated_at`
- `dokumenter`

`tittel`, `dokument_dato`, begge journalpost-saksbehandlerfeltene,
`tilgangskode`, `tilgangshjemmel`, `korrespondanseparter` og `kildesystem` er
optional. `korrespondanseparter` er
`Option<Vec<AdminKorrespondansepartV1>>`, så SQL `NULL` og JSON `[]` bevares
separat. Elementtypen er flat og permissiv:

- obligatorisk `rolle: String` og `navn: String`
- optional `parttype`, `id_type`, `id`, `adresse`, `postnummer`, `poststed`

Dette speiler lagret JSON uten å konstruere command-side part-, id- eller
adressetyper. Databasen garanterer bare at kolonnen er en JSON-array. Dersom et
lagret element ikke er et objekt med string-feltene over, er det en mappingfeil
og hele oppslaget gir `Internal error`; MVP-en er ikke en rå JSONB-konsoll.

Hvert dokument inneholder:

- full entitet-identitet
- `journalpost_id`
- `tilstand`
- `rekkefolge`
- `er_hoveddokument`
- `tittel`
- `filtype`
- `dokument_referanse`
- `mal_referanse`
- `felter` som optional liste med lagrede tokens
- `rendered_dokument_referanse`
- `opprettet_av_command_id`
- `created_at` og `updated_at`

`rekkefolge` er `i32`. `tittel`, `filtype`, `dokument_referanse`,
`mal_referanse`, `felter` og `rendered_dokument_referanse` er optional.
`felter` er `Option<Vec<String>>`, slik at SQL `NULL` og JSON `[]` ikke
kollapses. Databasen har ingen element-shape-constraint på `felter`; en lagret
ikke-string token er en mappingfeil og gir `Internal error` etter den samme
avgrensningen som correspondence-JSON.

Bruk strings/optional strings for lagrede koder og fritekst i admin-responsen.
Ikke konstruer `Sakstittel`, `Tilgangskode`, `Postnummer` eller andre
command-side validasjonstyper på nytt. Admin read skal kunne vise den lokale
tilstanden selv om den er historisk eller trenger reparasjon.

Ikke bruk command-side enums for `entitet_type`, `command_type`,
`operasjonstype`, `status`, `tilstand`, `arkivdel`, `journalposttype`,
`parttype`, `id_type` eller dokumentets lagrede `felter`; de returneres som
lagrede strings.

`AdminOperasjonSammendragV1` inneholder bare:

- `operasjon_id`
- `command_id`
- `operasjonstype`
- `entitet_id`
- `status`

Dette er nok til å velge command-oppslaget uten å duplisere detaljresponsen.

### 5.4 Stabil ordering

Kontrakten skal ha deterministisk ordering:

- command-operasjoner: `created_at`, deretter `operasjon_id`
- journalposter: `created_at`, deretter `journalpost_id`
- dokumenter: `rekkefolge`, deretter `dokument_id`
- sakens operasjonssammendrag: `created_at`, deretter `operasjon_id`

### 5.5 Feilrespons

MVP-en bruker eksisterende string-baserte `NatsResponse::Error`. Følgende
meldinger er kontrakt:

| Situasjon | `message` |
| :-- | :-- |
| ugyldig JSON, ukjent felt, manglende/blank `utfort_av` | `Invalid request format` |
| ukjent command-id | `Command not found` |
| ingen sak-entitet matcher key | `Sak not found` |
| serialisert svar overskrider NATS-grensen | `Response too large` |
| database-/mapping-/serialiseringsfeil | `Internal error` |

Interne feil eller SQL-detaljer skal ikke ekkoes i reply.

---

## 6. Application-vertikal

Opprett:

```text
src/application/src/admin/mod.rs
src/application/src/admin/model.rs
src/application/src/admin/ports/mod.rs
src/application/src/admin/ports/admin_read_repository.rs
src/application/src/admin/services/mod.rs
src/application/src/admin/services/admin_read_service.rs
```

Eksporter `pub mod admin;` fra `src/application/src/lib.rs`.

### 6.1 Interne typer

`application::admin::model` speiler projectionen uten Serde eller
wire-typer. `application` skal ikke avhenge av eller importere `lib-schemas`,
heller ikke i tester. Bruk eksisterende interne identitetstyper:

- `SkuffenSakId`
- `SkuffenJournalpostId`
- `SkuffenDokumentId`
- `EntitetId`
- `OperasjonId`

Wire-keyen mappes i infrastructure til en intern `AdminSakNokkel` med
`SkuffenId`, `ClientReference` eller `ArkivId`.

Admin-modellen må bevare optional-feltene slik de ligger i databasen. Ikke
gjenbruk `FaktaRepository::hent_sak_med_barn`: den modellen utelater blant annet
opprettelses-saksbehandler, journalpostattributter, mediareferanser, provenance
og tidsstempler — nettopp dataene admin trenger. Ikke gjenbruk
`PostgresFaktaRepository` sine mappinghelpers: de revaliderer lagrede
identifikatorer, postnummer og template-tokens. Ikke gjenbruk eksisterende
operasjonssammendrag eller command-outcome; de mangler admin-felter og den
avtalte `krever_avklaring`-folden.

`AdminCommand` eier en ren, deterministisk `utled_utfall`-funksjon etter §5.2.
Repositoryet leverer rader; application-modellen eier folderegelen.

### 6.2 Port og service

Én read-port er tilstrekkelig:

```rust
#[async_trait]
pub trait AdminReadRepository: Send + Sync {
    async fn hent_command(
        &self,
        command_id: Uuid,
    ) -> Result<Option<AdminCommand>, anyhow::Error>;

    async fn hent_sak(
        &self,
        key: AdminSakNokkel,
    ) -> Result<Option<AdminSak>, anyhow::Error>;
}
```

`AdminReadService` deler porten via `Arc<dyn AdminReadRepository>` og tilbyr ett
use case per subject. Service-laget mapper `None` til en typet
`CommandNotFound`/`SakNotFound`; repositoryfeil forblir en egen intern variant.
Infrastructure skal kunne skille not-found fra intern feil uten å sammenligne
feilstrenger.

`utfort_av` skal ikke være parameter til use caset. Det er transportattribusjon
og har ingen plass i application eller database.

---

## 7. PostgreSQL-adapter

Opprett:

```text
src/infrastructure/src/admin/mod.rs
src/infrastructure/src/admin/adapter/mod.rs
src/infrastructure/src/admin/adapter/postgres_admin_read_repository.rs
```

Eksporter `pub mod admin;` fra `src/infrastructure/src/lib.rs`.

### 7.1 Konsistent snapshot

Hvert oppslag bruker én `REPEATABLE READ READ ONLY`-transaksjon. Command og
operasjoner, eller sak og barn, skal komme fra samme snapshot. Dette er viktig
når workeren oppdaterer operasjoner samtidig som en operatør vurderer reparasjon.

Bruk SQLx 0.9 sin eksplisitte start:

```rust
pool.begin_with("BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
```

Transaksjonen starter før første lookup. Alle queries bruker `&mut *tx`, aldri
poolen, og alle `Some`-/`None`-veier committer eller ruller eksplisitt tilbake.
Private `#[derive(sqlx::FromRow)]` DB-row-structs eller eksplisitt
`Row::try_get` brukes for brede projections; PostgreSQL-enums selectes som
`::text`.

### 7.2 Command-query

1. Hent `command` med primary key.
2. Hvis ingen rad: returner `None`.
3. Hent alle `operasjon`-rader for `command_id`, join `entitet` for faktisk
   type/client-reference/arkiv-id.
4. Fold `utfall` i application-modellen etter reglene i 5.2.

Operasjon-queryen bruker eksplisitt
`ORDER BY o.created_at, o.operasjon_id`.

Eksisterende `command(command_id)` PK og `ix_operasjon_command_id` dekker
oppslaget. Ingen indeks skal legges til.

En command kan ha null operasjoner. Det er et gyldig success-svar med
`utfall: "uavklart"`; admin read utleder ikke om årsaken er ventende
validering, redelivery, avvisning eller dispatch-/state-problem. Kombinasjonen
`dekomponert_at IS NOT NULL` og null operasjoner kan ikke produseres av dagens
førstegangsdekomponering, men response skal fortsatt vise de rå feltene og
`uavklart` fremfor å gjøre read-pathen utilgjengelig for inkonsistent state.

### 7.3 Sak-query uten N+1

1. Resolve én sak-entitet fra key:
   - `skuffen_id` + `entitet_type = 'sak'`
   - `client_reference` + `entitet_type = 'sak'`
   - `arkiv_id` + `entitet_type = 'sak'`
2. Hvis ingen identitet: returner `None`.
3. Hent optional `sak_tilstand`.
4. Hvis state finnes, hent alle journalposter med entitet-identitet i én query.
5. Hent alle dokumenter for disse journalpostene i én query, og grupper dem i
   Rust. Ikke én query per journalpost.
6. Hent alle lette operasjonssammendrag for `sak_id` i én query.

Bruk eksplisitt ordering:

```sql
ORDER BY j.created_at, j.journalpost_id
ORDER BY d.journalpost_id, d.rekkefolge, d.dokument_id
ORDER BY o.created_at, o.operasjon_id
```

Dokument-queryen henter alle dokumenter for saken i ett statement, for eksempel
via join mot `journalpost_tilstand.sak_id`. Grupperingen skal bevare den sorterte
rekkefølgen; ikke la `HashMap`-iterasjon bestemme wire-order.

Eksisterende indekser dekker alle filter- og join-predikater; bounded ordering
kan fortsatt sortere:

- `entitet` PK, unik `client_reference` og `ix_entitet_arkiv_id`
- `sak_tilstand` PK
- `ix_journalpost_tilstand_sak_id`
- `ix_dokument_tilstand_journalpost_id`
- `ix_operasjon_sak_id`

Adapteren leser ikke:

- `operasjon_forsok`
- `arkiv_command_inbox`
- NATS object store
- status-streamen
- arkivet

---

## 8. Infrastructure mapping og NATS-listener

Opprett:

```text
src/infrastructure/src/admin/mapping.rs
src/infrastructure/src/admin/nats/mod.rs
src/infrastructure/src/admin/nats/listener.rs
```

`mapping.rs` eier all oversettelse mellom `lib_schemas::skuffen::admin::*` og
application-modellen. Verken `domain` eller `application` skal ha
`lib-schemas`-dependency eller import, heller ikke test-only. Contract-tester
ligger i `lib-schemas`; admin wire-mappingtester ligger i `infrastructure`.

### 8.1 Listener

Lag en liten admin-lokal request-reply-responder. Ikke koble admin-feilsemantikk
til den offentlige query-listenerens private `QueryHandlerError`.

Listeneren skal:

- subscribe eksakt på de to subjects; aldri wildcard `arkiv.admin.>`
- bruke de stabile queue groups `skuffen-admin-read-command-hent-v1` og
  `skuffen-admin-read-sak-hent-v1`, så bare én instans svarer under
  deploy-overlapp
- ha `#[tracing::instrument(skip_all, ...)]` på per-message handler og kalle
  `set_parent_from_nats_headers()` som første statement
- deserialisere request, trimme og validere `utfort_av`
- kalle riktig application-use case
- mappe typed not-found til de stabile feilresponsene i 5.5
- ikke logge request- eller response-payload
- ikke logge `siste_detalj`, titler, korrespondanseparter, adresser eller
  dokumentmetadata

For gyldige requests logges én strukturert `info!`-linje med:

- `admin_action = "read.command.hent"` eller `"read.sak.hent"`
- `utfort_av`
- lookup-id/key-type; UUID-key-values kan logges, men rå `ArkivId` logges ikke
- `resultat = "ok" | "not_found" | "error" | "response_too_large"`

Dette er attribusjonslogg, ikke en autentisert audit-logg.

`utfort_av` er forventet operatøridentifikator og det eneste
menneskeidentifiserende feltet som er tillatt i denne `info!`-loggen.
Listeneren trimmer før bruk, avviser blank verdi og control characters, og
setter en eksplisitt øvre grense på 128 UTF-8 bytes for å hindre loggmisbruk.
UUID-key-values kan logges. `ArkivId` er en fri string i admin-kontrakten og
kan inneholde historisk eller uventet innhold; attribution-loggen bruker derfor
`key_type = "arkiv_id"` uten rå value. Loggen skrives etter at
publish-resultatet er kjent; publish-feil gir `resultat = "error"`. Requests
uten reply subject kan ikke få reply og logges som transportfeil uten payload.
Det eksplisitte unntaket for begrenset `utfort_av` på `info!` dokumenteres også
i `.agent/rules/repo_rules.md`, slik at den normative no-PII-regelen og denne
kontrakten ikke motsier hverandre. Unntaket gjelder ikke andre request-felt.

### 8.2 Responsstørrelse

Sak-responsen kan bli stor fordi den inneholder alle materialiserte
journalposter, dokumenter og korrespondanseparter, mens MVP-en ikke har
paginering.

Serialiser hele `NatsResponse::Ok(...)` før publish og sammenlign byte-lengden
med `async_nats::Client::max_payload()`. Prosjektet er låst til async-nats
0.49.1. Eksakt grense er tillatt; bare `bytes.len() > max_payload` er for stort.
Publish-metodene sjekker også grensen klient-side, men uten en eksplisitt guard
ville caller bare fått timeout når success-responsen ikke kan sendes. Er
responsen for stor, serialiser og send den lille stabile
`Response too large`-feilen i stedet.

Ikke trunker en sak og ikke returner partial success. Et senere behov løses med
et nytt målrettet/paginert subject, ikke ved å endre denne responsen stille.

### 8.3 Supervisor og startup

I `src/infrastructure/src/bootstrap.rs`:

- legg til `build_admin_listener(nats, db_pool)`
- del samme `Arc<PostgresAdminReadRepository>` mellom use casene

I `src/lib.rs`:

- bygg admin-listeneren før `runtime.db_pool` flyttes til execution-wiring
- spawn `admin_listener` som `TaskCriticality::Degraded`

Admin-listenerens `run()` bruker
`TaskSupervisor::background("admin_listener")`. Inne i ett forsøk kjøres de to
subscriptions med `tokio::try_join!`. Hver subscription-loop skal returnere
`Err` dersom streamen avsluttes; returnerer den `Ok` mens den andre fortsetter,
vil `try_join!` ellers vente for alltid. Dermed restarter en avsluttet eller
feilende subscription hele admin-listeneren. Dette er core request-reply;
`jetstream_setup.rs` skal ikke endres. Koble supervisoren til eksisterende
shutdown-token og la subscription-loopene avslutte via `tokio::select!`, slik
at normal shutdown ikke utløser restart.

Dette krever at `TaskSupervisor` sin backoff også venter med `tokio::select!`
mot shutdown-token; dagens ukansellerbare sleep kan ellers vare 30 sekunder,
mens Cloud Run gir 10 sekunder. Root-runtime skal ved cancellation avslutte/
abortere resterende tasks kontrollert og ikke rapportere normal admin-shutdown
som degraded failure. Legg en målrettet supervisor/root-shutdown-test til
endringen; ikke gjør en bred runtime-refactor utover det som trengs for korrekt
10-sekunders shutdown.

---

## 9. Tester

### 9.1 `lib-schemas`

- Eksakt top-level snake_case JSON-shape for begge requestene; `key.type`
  bruker de avtalte camelCase-variantene `skuffenId`, `clientReference` og
  `arkivId`.
- Alle tre `AdminSakKeyV1`-varianter.
- Manglende `utfort_av` avvises ved deserialisering.
- Ukjent felt avvises både top-level og inni `key`.
- Response roundtrip med optional fields, korrespondanseparter og alle fire
  saksbehandler-kontekstene (opprettelse, ønsket, nåværende og journalpost).
- Eksakt `null`-shape, særlig `fakta: null`, og bevaring av `NULL` mot tom liste.
- Alle fire `AdminCommandUtfallV1`-verdier.
- Admin-responsen aksepterer lagrede fritekst-/kodeverdier uten å kjøre
  command-side validering, inkludert lowercase lagrede part-/feltkoder.

### 9.2 Application

Med fake `AdminReadRepository`:

- command success og `CommandNotFound`
- sak success, inkludert identity-only (`fakta: None`), og `SakNotFound`
- repositoryfeil beholdes som intern feil og blir ikke not-found
- command-folden dekker tom liste, pågående status, alle `ok`, minst én
  `krever_avklaring`, minst én `feilet`, samt prioriteten
  `feilet > krever_avklaring > fullfort > uavklart`
- application-testene bruker bare interne modeller; en grep/manifestsjekk viser
  ingen `use lib_schemas`-importer i Rust-koden og ingen `lib-schemas`-dependency
  i `src/domain/Cargo.toml` eller `src/application/Cargo.toml`

### 9.3 PostgreSQL repository

Opprett `src/infrastructure/tests/postgres_admin_read_repository_test.rs` med
testcontainers, samme setup-mønster som
`postgres_operasjon_repository_test.rs`.

Dekk minst:

1. Command med blandede operasjonsstatuser returnerer alle current-felter i
   deterministisk rekkefølge.
2. Command uten operasjoner returnerer success med `uavklart`; både
   `dekomponert_at = NULL` og en syntetisk inkonsistent rad med timestamp vises
   uten å gjette `avvist`.
3. `feilet + krever_avklaring` blir `feilet`, mens `ok + krever_avklaring` blir
   `krever_avklaring`.
4. Sak kan slås opp med skuffen-id, client-reference og arkiv-id, og alle tre
   gir samme sak.
5. En identity-only sak-entitet returnerer success med `fakta: None`.
6. En `sak_tilstand` med optional attributter `NULL` returnerer
   `fakta: Some`, ikke identity-only.
7. Full sak inkluderer alle journalposter/dokumenter, provenance,
   mediareferanser og tidsstempler uten N+1.
8. Saksbehandlerne holdes separat. Seed fire ulike syntetiske verdier:
   - opprettelses-saksbehandler på saken = A
   - ønsket saksansvarlig = B
   - nåværende saksansvarlig = C
   - journalpost-saksbehandler = D
   og verifiser at ingen verdi overskriver en annen i projectionen.
9. Sakens operasjonssammendrag inneholder alle operasjoner og deres
   `command_id`.
10. Ordering låses med lik `created_at` og ikke-sortert insert-rekkefølge, slik
    at UUID tie-breakerne faktisk testes.
11. `felter: NULL`, `felter: []`, correspondence-JSON og både bytes-/template-
    dokumentreferanser bevares eksakt.
12. En deterministisk concurrency-test pauser oppslaget etter første query,
    muterer fakta fra en annen connection og verifiserer at resten av svaret
    fortsatt kommer fra samme repeatable-read snapshot. I tillegg kan en liten
    test-only query i transaksjonen verifisere `repeatable read` og `read only`.

«Uten N+1» skal bevises strukturelt: sak-oppslaget har fem faste SELECTs
(identitet, sak-state, journalposter, dokumenter, operasjoner), uavhengig av
antall journalposter/dokumenter. Hold dem som fem navngitte query-funksjoner og
test med både én og flere children; ikke introduser en generell SQLx wrapper
bare for query-counting.

### 9.4 Infrastructure mapping og listener

- Wire-keyene mappes til alle tre interne `AdminSakNokkel`-varianter.
- Alle optional storage-felt og de fire outcome-variantene mappes uten
  command-side validering.
- Malformed/unknown/blank/control-character/for-lang `utfort_av` gir
  `Invalid request format`; application-use case kalles ikke.
- Typed not-found, repository-/mappingfeil og size-guard mappes til eksakt
  kontraktsmelding. Serialiseringsfeil håndteres i production-koden som
  `Internal error`, men trenger ingen kunstig failing-serializer-test fordi den
  konkrete DTO-grafen er infallible for gyldige Rust-verdier.
- Size-guard måler hele `NatsResponse::Ok`; lik `max_payload` er tillatt.
- Listenerens interne handler-/subscription-tester bruker en liten fake boundary
  rundt subscribe/publish der det trengs: avsluttet command- eller
  sak-subscription avslutter `run_once`, og supervisoren restarter begge.
- Traceparent blir parent på request-spanet. Gyldig request gir nøyaktig én
  attribusjonslogg etter kjent publish-resultat; bruk test-subscriber/layer,
  ikke assertions på menneskelig formattert loggtekst.

### 9.5 NATS/integration

Opprett `integration-tests/tests/admin_read_e2e.rs` og hjelpere i
`integration-tests/tests/support/nats.rs`.

Dekk:

- begge subjects har responder etter normal startup
- to samtidige Skuffen-runtimes eller to admin-listenere i samme queue groups
  gir nøyaktig ett reply på en rå inbox-subscription, ikke bare «minst ett» via
  `request()`. Testen må først bevise at begge queue-medlemmer er subscribet,
  og vente en kort quiet-window etter første reply for å avkrefte reply nummer
  to.
- normal command kan hentes etter command-flow og inneholder operasjoner
- tilhørende sak kan hentes via minst client-reference og skuffen-id
- ukjent command/sak gir eksakt `NatsResponse::Error`
- malformed request og blank `utfort_av` gir `Invalid request format`
- en oversize serialisert response gir `Response too large`, ikke caller-timeout

Integration-testen trenger ikke Sikri eller original payload; bruk eksisterende
fake-arkiv for å produsere materialisert state. Oppdater også
`integration-tests/tests/support/env.rs`: start en isolert NATS med en liten
`max_payload` via config-fil for oversize-testen (NATS Server 2.10.7 har ingen
CLI-flag for dette). Velg en grense som er større enn startup-/request-/error-
meldingene, og bygg den store success-responsen gjennom flere requests som hver
er under grensen. Ikke bygg en kunstig sak på over standardgrensen. Runtime-
readiness skal probe begge admin-subjects; forventet `Command not found`/
`Sak not found` teller som responder. `skuffen.ready` alene beviser ikke at
admin-listeneren har subscribet. Sak-oppslaget via skuffen-id bruker
`identitet.skuffen_id` fra admin-responsen, ikke en separat scenario-fixture.

---

## 10. Dokumentasjon og kontraktseierskap

1. Oppdater `README.md` med begge subjects, request-eksempler,
   `NatsResponse`-shape og den avtalte ansvarsdelingen mellom status/admin
   read/admin write.
2. Oppdater `.agent/guides/commands.md` med to konkrete `nats request`-eksempler.
3. Oppdater `.agent/guides/observability.md` med de to admin-subjectene,
   listener/supervisor, request-span og den avgrensede `utfort_av`-loggen.
4. Oppdater `.agent/guides/architecture/command/query.md` med skillet mellom
   offentlig live-query og lokal admin repair-state.
5. Oppdater `.agent/rules/repo_rules.md` med det smale `utfort_av`-unntaket fra
   no-PII-at-info-regelen.
6. Korriger README-påstanden om at `skuffen_id` alltid er skjult: admin read
   eksponerer intern id eksplisitt fordi den er nødvendig for reparasjon.
   Oppdater også runtime-listen med `admin_listener`, og presiser at operasjoner
   er skjult for normale klienter, men synlige gjennom admin read.
7. Kjør `adr-fmt --critique SKU-0008`, og la den nye beslutningsrecorden
   referere til/utvide SKU-0008 med de to nye eksakte NATS-subjectene og
   `NatsResponse`-kontrakten. Ikke omskriv accepted historikk med mindre
   `adr-fmt`-guidelines eksplisitt krever det.
8. Opprett neste ledige Skuffen-beslutningsrecord (`SKU-0018` ved baseline).
   Den skal kort registrere den varige beslutningen: status-streamen er den
   klientvendte lifecycle-/avvisningskanalen; admin read viser autoritativ
   nåværende PostgreSQL-reparasjonstilstand og leser ikke streamen; admin write
   skal senere utføre avgrensede reparasjoner; denne leveransen er read-only
   over to eksakte NATS-subjects. ADR-en skal også registrere at
   `krever_avklaring` har egen command-sammendragsverdi, og at den begrensede,
   selvdeklarerte `utfort_av`-attribusjonen er tillatt i `info!` uten å være en
   autentisert audit-logg. Bruk
   prosjektets `adr-fmt --guidelines`, `--context` og `--lint`-workflow.
9. NATS-permissions ligger utenfor dette repoet. Deploy-oppsettet må tillate
   subscribe på de to eksakte request-subjectene og publish til den avtalte
   response-inbox-prefixen. Ikke gi en bredere
   `arkiv.admin.>`-rettighet enn nødvendig dersom permissions kan være
   finkornede.

---

## 11. Implementasjonsrekkefølge

1. Start fra committed Skuffen HEAD og verifiser baselinen i §4: ingen
   avvisningspersistens og ingen admin-migrasjon.
2. Kjør `cargo run -p adr-fmt -- --context application` og
   `--context infrastructure` før coding mot crate-grensene.
3. Lukk SKU-0013-bruddet: fjern `lib-schemas` fra application sine
   dev-dependencies/tester og behold samme application-testdekning med interne
   fixtures.
4. Implementer admin-DTO-ene i `landdyrtilsyn-libs`, kjør libs-sjekkene i §12,
   og merge schema-committen til dependencyens default branch. En feature-
   branch-push er ikke nok fordi Skuffen følger default-branch HEAD.
5. Oppdater deretter bare `lib-schemas` i Skuffens `Cargo.lock` bevisst og
   inspiser diffen. Ikke legg `rev` i `Cargo.toml`.
6. Implementer application-modell, port, typed errors og service.
7. Implementer PostgreSQL-repository og repositorytester.
8. Implementer infrastructure-mapping, NATS-listener og størrelsesguard.
9. Wire listener i bootstrap/root runtime med shutdown-token.
10. Legg til integration-testene.
11. Oppdater README, repo-rule, command-/query-/observability-guide og
    beslutningsrecords.
12. Kjør Skuffen-sjekkene i §12.

---

## 12. Kvalitetssjekker

I `landdyrtilsyn-libs`:

```bash
cargo fmt --check
cargo test -p lib-schemas --features skuffen
cargo clippy -p lib-schemas --all-targets --features skuffen -- -D warnings
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Full `cargo test --workspace` i `landdyrtilsyn-libs` er ikke hard gate for
denne schema-endringen fordi workspace har en live Geonorge-test. Kjør den bare
når det miljøet er tilgjengelig; den målrettede `lib-schemas`-testen er den
obligatoriske testgaten.

I Skuffen:

```bash
cargo fmt --check
cargo check --workspace --all-targets
cargo test --workspace --exclude skuffen-integration-tests
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test -p skuffen-integration-tests --test admin_read_e2e -- --nocapture
cargo test -p skuffen-integration-tests -- --nocapture
```

Etter dokumentendringer:

```bash
cargo run -p adr-fmt -- --lint
```

---

## 13. Akseptansekriterier

Leveransen er ferdig når:

- et rått `nats request` kan hente command- og sak-state med obligatorisk
  `utfort_av`
- svarene bruker DTO-er fra `lib-schemas::skuffen::admin`
- command-svaret viser current operasjonsstate, men ingen hendelsestidslinje
  eller forsøkshistorikk
- en command uten operasjoner vises som `uavklart`, aldri som utledet `avvist`
- `krever_avklaring` er synlig både på operasjonsraden og i commandens
  oppsummerte `utfall`, med prioritet under `feilet` og over `fullfort`
- sak-svaret viser alle separate saksbehandlerkontekster og all materialisert
  state som en senere reparasjon kan måtte endre
- identity-only saker er synlige
- ingen admin read kaller arkivet, JetStream, object store eller skriver i DB
- ukjente ressurser, ugyldige requests, interne feil og for store svar gir de
  avtalte stabile feilresponsene
- valid `utfort_av` logges én gang per request uten at response-data logges
- flere Skuffen-instanser gir bare ett reply per request via queue groups
- `domain` og `application` har ingen `lib-schemas`-dependency eller import,
  heller ikke test-only
- shutdown under subscription eller supervisor-backoff avslutter kontrollert
  innen Cloud Runs 10-sekundersvindu
- alle relevante tester og kvalitetsporter passerer

---

## 14. Verifisert grunnlag og kjente begrensninger

Verifisert mot nåværende project source, migrations, tests, git history og den
interne git-avhengigheten `landdyrtilsyn-libs`:

- eksisterende indekser dekker alle planlagte oppslag
- `FaktaRepository` er for smal for admin og kan ikke gjenbrukes som response
- `operasjon_forsok` er unødvendig for MVP-en
- valideringsavvisning persisteres ikke i PostgreSQL og tilhører
  status-streamen; null operasjoner er ikke bevis på avvisning
- `krever_avklaring` er ikke terminal operasjonsstate, men er den viktigste
  manuelle reparasjonstilstanden og får derfor egen command-sammendragsverdi
- `application` har bare test-only `lib-schemas`-lekkasje i baseline; production
  dependency direction er allerede riktig, og implementasjonen rydder også
  testgrensen
- queue group er nødvendig for å unngå duplikate replies ved deploy-overlapp
- async-nats 0.49.1 eksponerer gjeldende payloadgrense og avviser oversized
  publish klient-side
- SQLx 0.9 støtter custom `BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ
  READ ONLY` via `Pool::begin_with`
- NATS Server 2.10.7 setter redusert `max_payload` via config-fil, ikke CLI-flag
- dagens supervisor-backoff og root shutdown er ikke fullt cancellation-aware;
  dette er eksplisitt del av listener-wiringen, ikke en skjult follow-up

Ikke verifisert:

- hvilke NATS ACL-er som er satt i deploymiljøene
- faktisk maksimal sakstørrelse i produksjon

Derfor er permission-oppfølging og størrelsesguard eksplisitte deler av
implementasjonen. Permission-endringen skjer utenfor dette repoet.
