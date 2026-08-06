# SKU-0008. NATS wire contract for arkivering og queries

Date: 2026-05-22
Last-reviewed: 2026-08-05
Tier: B
Status: Accepted
Crates: skuffen, infrastructure, application, skuffen-integration-tests

## Related

References: SKU-0004, SKU-0007, SKU-0015

## Context

Skuffen bruker NATS request-reply for command intake og synkrone read/query-kall. `arkiv.arkiver` tar en atomisk `CommandSequence`; queries ligger under `arkiv.request.*`.

Kontraktene ligger delvis i `lib-schemas`. `lib-schemas` og `lib-nats` følger latest git HEAD; `lib-sql` bruker tag. `Cargo.lock` er resolved build boundary. Direct cutover ble godkjent av eier fordi ingen klienter finnes.

## Decision

R1 [5]: `arkiv.arkiver` skal svare med `ArkiveringKvittering`, ikke `NatsResponse`, og `Ok` betyr bare at hele command batchen er mottatt for prosessering.

R2 [5]: `ArkiveringKvittering` skal bruke default serde externally tagged enum-shape: `{ "Ok": { "command_ids": [...] } }` eller `{ "Error": { "message": "..." } }`.

R3 [5]: `Ok.command_ids` skal inneholde alle aksepterte command ids i innsendt rekkefølge, inkludert idempotent aksepterte kommandoer, og ingen partial acceptance rapporteres.

R4 [5]: Command intake error replies skal være statiske og sanitiserte; wire-meldinger er `invalid payload format`, `media validation failed`, `invalid command sequence` eller `internal error`.

R5 [5]: Synkrone read/query subjects er `arkiv.request.sak.hent`, `arkiv.request.journalpost.hent` og `arkiv.request.bruker.mt_enheter`, uten legacy aliases.

R6 [5]: Query replies bruker `NatsResponse<T>`; `arkiv.request.bruker.mt_enheter` er en live stub som returnerer `NatsResponse::Error { message: "Not implemented" }`.

R7 [5]: `arkiv.request.journalpost.hent` er koblet opp, men bruker fake/testdata repository inntil ekte backing integration erstatter denne adapteren.

R8 [5]: `lib-schemas` og `lib-nats` skal ikke bruke git `rev` i `Cargo.toml`; latest HEAD eller tag brukes, med `Cargo.lock` som resolved build boundary.

R9 [5]: `lib-sql` kan fortsatt bruke release tag; lockfile eller `cargo update` for `landdyrtilsyn-libs` krever schema- og kompatibilitetsreview.

R10 [6]: Hvis uventede klienter feiler etter cutover, mitigasjon er ny deploy med midlertidige aliases eller rask klientoppdatering etter eierbeslutning.

## Consequences

Dette er en direkte breaking wire-endring. Cutover bygger på eieropplysning i build-sesjonen om at ingen aktive klienter finnes; før produksjonscutover skal eier verifisere dette mot kjente consumers/operatører.

Klienter må behandle `arkiv.arkiver` som batch-kvittering og query subjects som `NatsResponse<T>`.

Latest-HEAD policy gjør lockfile-endringer til kontraktspunkter som må reviewes. `arkiv.request.journalpost.hent` returnerer syntetisk fixture-data inntil ekte repository finnes; `arkiv.request.bruker.mt_enheter` har ingen success-payload-kontrakt ennå.

### Skjermingssikker kontrakt-redesign (SKU-0015)

SKU-0015 endrer shapen på journalpost-kommandoene og query-responsene som denne
kontrakten dekker, som en koordinert breaking change uten live klienter (samme
cutover-grunnlag som over):

- Journalpost-kommandoene får `Tilgjengelighet { Offentlig | Skjermet { tilgangskode,
  tilgangshjemmel } }`, eksplisitt `Korrespondansepart` (parttype Person/Virksomhet)
  og `Utsendingsmottaker` (`MottakerId` + full `Postadresse`).
- Query-responsene får EGNE permissive respons-typer som kun rapporterer tilstand
  og aldri re-validerer, slik at historiske koder alltid kan deserialiseres.

R1-R10 formuleringene over står uendret; `ArkiveringKvittering`- og
`NatsResponse<T>`-rammene beholdes. Detaljerte regler for skjerming, merking,
gateway-utledning og audit eies av SKU-0015.
