# SKU-0008. NATS wire contract for arkivering og queries

Date: 2026-05-22
Last-reviewed: 2026-11-16
Tier: B
Status: Accepted
Crates: skuffen, infrastructure, application, skuffen-integration-tests

## Related

References: SKU-0004, SKU-0016, SKU-0015

## Context

Skuffen bruker NATS request-reply for command intake og synkrone read/query-kall. `arkiv.arkiver` tar en atomisk `CommandSequence`; queries ligger under `arkiv.request.*`.

Kontraktene ligger delvis i `lib-schemas`. Alle tre bibliotekene fra `landdyrtilsyn-libs` bruker tag, med `Cargo.lock` som resolved build boundary. Opprinnelig cutover ble godkjent uten klienter; utvidelsen av `OpprettSak` må støtte eksisterende klienter.

## Decision

R1 [5]: `arkiv.arkiver` skal svare med `ArkiveringKvittering`, ikke `NatsResponse`, og `Ok` betyr bare at hele command batchen er mottatt for prosessering.

R2 [5]: `ArkiveringKvittering` skal bruke default serde externally tagged enum-shape: `{ "Ok": { "command_ids": [...] } }` eller `{ "Error": { "message": "..." } }`.

R3 [5]: `Ok.command_ids` skal inneholde alle aksepterte command ids i innsendt rekkefølge, inkludert idempotent aksepterte kommandoer, og ingen partial acceptance rapporteres.

R4 [5]: Synkrone read/query subjects er `arkiv.request.sak.hent`, `arkiv.request.sak.med_journalposter`, `arkiv.request.journalpost.hent` og `arkiv.request.bruker.mt_enheter`, uten legacy aliases.

R5 [5]: Query replies bruker `NatsResponse<T>`; `arkiv.request.bruker.mt_enheter` er en live stub som returnerer `NatsResponse::Error { message: "Not implemented" }`.

R6 [5]: `arkiv.request.journalpost.hent` er koblet opp, men produksjonsadapteren returnerer en tydelig feil inntil ekte backing finnes. Fake-data brukes kun når `SKUFFEN_FAKE_SIKRI` er aktiv; ekte klienter får aldri syntetiske svar som ser gyldige ut.

R7 [5]: `lib-schemas` og `lib-nats` skal ikke bruke git `rev` i `Cargo.toml`; tag foretrekkes, med `Cargo.lock` som resolved build boundary. Alle tre bibliotekene fra `landdyrtilsyn-libs` skal være tag-pinnet.

R8 [5]: `lib-sql` kan fortsatt bruke release tag; lockfile eller `cargo update` for `landdyrtilsyn-libs` krever schema- og kompatibilitetsreview.

R9 [6]: Hvis uventede klienter feiler etter cutover, mitigasjon er ny deploy med midlertidige aliases eller rask klientoppdatering etter eierbeslutning.

R10 [5]: `OpprettSak` støtter saksbehandler, enhet eller begge. Utelatt felt og `null` betyr fravær. Skuffen kontrollerer tildeling ved mottak, ikke i libs, og avviser manglende, tom eller blank tildeling med forståelig kvittering. Eksisterende meldinger med begge strenger støttes uten samtidig klientoppgradering.

## Consequences

Det opprinnelige kontraktskiftet var en direkte breaking wire-endring. Cutover bygget på eieropplysning i build-sesjonen om at ingen aktive klienter fantes; dette er ikke en generell tillatelse til senere breaking endringer.

Klienter må behandle `arkiv.arkiver` som batch-kvittering og query subjects som `NatsResponse<T>`.

Tag- og lockfile-endringer er kontraktspunkter som må reviewes. `arkiv.request.journalpost.hent` returnerer en eksplisitt feil inntil ekte repository finnes; `arkiv.request.bruker.mt_enheter` har ingen success-payload-kontrakt ennå.

### Skjermingssikker kontrakt-redesign (SKU-0015)

SKU-0015 endrer shapen på journalpost-kommandoene og query-responsene som denne
kontrakten dekker, som en koordinert breaking change uten live klienter (samme
cutover-grunnlag som over):

- Journalpost-kommandoene får `Tilgjengelighet { Offentlig | Skjermet { tilgangskode,
  tilgangshjemmel } }`, eksplisitt `Korrespondansepart` (parttype Person/Virksomhet)
  og `Utsendingsmottaker` (`MottakerId` + full `Postadresse`).
- Query-responsene får EGNE permissive respons-typer som kun rapporterer tilstand
  og aldri re-validerer, slik at historiske koder alltid kan deserialiseres.

R1-R9 formuleringene over står uendret; `ArkiveringKvittering`- og
`NatsResponse<T>`-rammene beholdes. Detaljerte regler for skjerming, merking,
gateway-utledning og audit eies av SKU-0015.

### Sak med journalposter

`arkiv.request.sak.med_journalposter` tar samme `key` som `arkiv.request.sak.hent`
og svarer med samme `SakResponse`, men ber alltid arkivet om journalposter.
Inkluderingen er bundet til subjectet, ikke til et request-felt, slik at en eldre
instans under utrullingsoverlapp aldri stille kan ignorere ønsket.

Svaret bærer dokumentmetadata, ikke filinnhold. Første dokument er hoveddokumentet.
`SakResponse` fikk arkivets `arkiv_sak_id` (`saksid` fra Sikri/Elements),
`DokumentResponse` fikk arkivets `dokument_id`, og `JournalpostResponse.dokument_dato`
er `NaiveDateTime` i stedet for `String`: arkivet oppgir dato og klokkeslett uten
tidssone, og Skuffen dikter ikke opp en. Alle tre er breaking for typed Rust-klienter
av de delte responstypene.

Saksoppslag på `arkivId` går direkte til arkivet uten krav om lokal identitet.

Ved innføringen ble `lib-schemas` pinnet til tag `1.7.5`, mens `lib-nats` og
`lib-sql` ble på tag `1.7.2`; `Cargo.toml` og `Cargo.lock` viser dagens boundary.

### Statusstrømmen (SKU-0020)

`arkiv.status.>` er ikke dekket av R1-R9, som gjelder request-reply. SKU-0020 R5
fastslår at strømmen er at-least-once uten deduplisering: en klient må tåle å se
samme hendelse flere ganger, særlig `Feilet` når flere operasjoner på samme
kommando feiler terminalt.

`SkuffenCommandEvent` fikk i tillegg varianten `krever_avklaring`. Uten den fikk
`arkiv.status.<cmd>.command` ingenting når en operasjon kom ut av recovery med
ukjent utfall, og en klient som følger `.command` — den anbefalte subscriptionen
for «bare utfallet» — så stillhet. Den publiseres med `terminal: false`, fordi
utfallet ikke er avgjort, og taper for `feilet`, som er monotont. Enumet har
ingen `#[serde(other)]`, så en klient som deserialiserer strengt må ta inn
taggen. Cutover-grunnlaget er det samme som over: ingen aktive klienter.

### Valgfri tildeling ved opprettelse av sak

R10 er en bakoverkompatibel wire-utvidelse, ikke et nytt command eller subject.
Saksbehandlerfeltene blir `Option<String>` i schemas og gjennom materialisering
til Sikri. Fravær erstattes ikke med tom streng, og enhet utledes av arkivet.
Query kan returnere en sak uten saksbehandler, slik responskontrakten allerede tillater.
Tildelingsregelen sjekkes bare i Skuffens batchkontroll ved mottak, før lagring
og dispatch. Feil inngår i batchens samlede, forståelige feilkvittering;
schemas deserialiserer vanlige valgfrie strenger uten egen domenesjekk.

Eier godkjenner en minor-release i libs og bekrefter at eksisterende klienter
ikke sender blanke verdier. Rust-klienter som tar inn den nye taggen må tilpasse
feltkonstruksjonen; klienter som beholder gammel tag trenger ingen endring.
Hele Skuffen-pipelinen må være oppgradert før nye klienter utelater et felt.
Gamle instanser tåler verken den nye meldingsformen eller uferdige saker med
fraværende tildelingsfelt; rollback krever derfor håndtering av begge deler.

`lib-schemas` og `lib-sql` er pinnet til publisert tag `1.8.1`
(`8f444c7`), mens `lib-nats` beholdes på `1.7.7`. `Cargo.lock` er resolved
boundary. `lib-sql` bruker `sqlx-core` og `sqlx-postgres` direkte, med samme
pool- og feiltype; Skuffen aktiverer `macros` eksplisitt for egne migrasjoner.
