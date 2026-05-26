# Eksekvering av kommandoer mot Sikri

Se også ADR: `docs/adr/skuffen/SKU-0001-execution-v2-og-opprydding-av-legacy-state-seams.md`.

Dette dokumentet beskriver execution v2 i Skuffen. Målet er robusthet og pålitelighet mot et upålitelig arkiv-API, men med en så tydelig modell at fremtidige endringer fortsatt er trygge.

## Mental modell

Execution skal kunne forklares slik:

1. En validert kommando registreres i execution-systemet.
2. `RegistrerIEksekveringssystemService` oppretter entity tilstand-rader for sak, journalpost og dokument og materialiserer `CommandStateDecision` via `planlegg_neste_handling()`.
3. Ved registrering og wake-up materialiseres domenebeslutningen til `command_execution.status`: `Ready -> klar`, `Blocked -> blokkert_venter`, `Done -> ok`, `Invalid -> feil`.
4. Én executor plukker neste kjørbare kommando og utfører maksimalt én `ArkivOperasjon` per attempt.
5. Etter operasjonsutfallet oppdateres entity facts, kommandoen re-evalueres, og neste status materialiseres.
6. Recoverable tekniske feil ender i `retry_venter`; kommandoen ender ellers i `ok`, `blokkert_venter` eller `feil`.

Det er ingen skjult workflow engine. Entity tilstand beskriver fakta per domeneentitet. `command_execution` beskriver progresjon.

## Flyt

1. Lytt på `arkiv.command.ready.<entity>.<commandId>` (JetStream, retention 180 dager).
2. Registrer kommandoen i `command_execution` og seed entity tilstand-rader i samme use case via `RegistrerIEksekveringssystemService`.
3. Bruk `planlegg_neste_handling(command_type, target, facts)` for å materialisere kommandoen som:
   - `klar` for `Ready(_)`
   - `blokkert_venter` for `Blocked(_)`
   - `ok` for `Done`
   - `feil` for `Invalid(_)`
4. Executor starter, tar singleton-lock og resetter eventuelle hengende `kjorer`-rader tilbake til `klar`.
5. Executor plukker neste kjørbare kommando fra DB.
6. Executor kaller `planlegg_neste_handling(command_type, target, facts) -> CommandStateDecision` fra ferske facts:
   - `Ready(ArkivOperasjon)` → executor utfører **én** operasjon mot Sikri, oppdaterer entity facts, re-evaluerer og materialiserer ny beslutning
   - `Blocked(reason)` → status settes til `blokkert_venter` med safe reason-kategori
   - `Done` → status settes til `ok`
   - `Invalid(violation)` → status settes til `feil` med safe violation-kategori
   - recoverable Sikri-/teknisk feil → status settes til `retry_venter` med `retry_ready_at`
7. Status-eventer publiseres på `arkiv.status.<commandId>`.
8. Når execution-path publiserer en terminal status, publiseres også `arkiv.command.done.<entity>.<commandId>`.

JetStream-operasjonelt:
- Streamene `arkiv_command_inbox`, `arkiv_command_ready`, `arkiv_command_done` og `arkiv_status` konfigureres med `num_replicas = 3`.
- `arkiv_media` object store konfigureres med `num_replicas = 3`.
- Durable consumer `validator` og `executor` konfigureres med explicit ack og `num_replicas = 3`.
- Listenerne for validation og execution kjører i en supervisor-loop som reoppretter stream/consumer/messages etter NATS-avbrudd.

## Arkivfaglige regler (oppsummering)

Hent detaljer fra arkivfag‑ressurser:
- Sak: `.agent/skills/arkivfag/resources/sak.md`
- Journalpost: `.agent/skills/arkivfag/resources/journalpost/*`
- Dokument: `.agent/skills/arkivfag/resources/dokument.md`

Kjernekrav:
- Sak må finnes før journalpost kan opprettes.
- Saksstatus `B` eller `F` tillater journalposter. `A` (avsluttet) låser saken.
- Avslutt sak krever at alle journalposter er ferdige:
  - Inngående: journalført og avskrevet.
  - Utgående: journalført.
  - Internt notat: journalført.

 Utgående uten utsending:
- Opprett i `R`, sett til `J`.

## Eksekveringsplan (planlegg_neste_handling decision function)

Kommandoer drives av den rene domenefunksjonen `planlegg_neste_handling(command_type, target, facts) -> CommandStateDecision`. Den inspiserer entity tilstand (facts) og returnerer `Ready(ArkivOperasjon)`, `Done`, `Blocked` eller `Invalid`. Ved registrering og wake-up materialiserer application beslutningen til `command_execution.status`. Executor utfører maksimalt én `ArkivOperasjon` per attempt, deretter re-evalueres kommandoen.

### Opprett sak
Steg:
- `OpprettSak`

### Opprett journalpost (inngående/utgående/internt)
Steg:
- `RenderDokument` for HTML-template hoveddokument når render-facts er klare; dette skjer før `OpprettJournalpost` fordi journalpost normalt opprettes med hoveddokument. Hvis rendered PDF-referanse allerede finnes, men dokumentet fortsatt står i `avventer_rendring` etter et avbrutt forsøk, fullfører retryen `RenderDokument` idempotent ved å sette dokumentet til `ok` uten ny rendering eller lagring.
- `OpprettJournalpost`
- `LeggTilDokument` (ett steg per dokument, ingen øvre grense; gjelder bytes-vedlegg; rendered HTML-template vedlegg er ikke i v1-scope for attachment-arkivering)
- `Journalfør` (avhenger av type og flyt)
- `Avskriv` (kun inngående)

### Sett saksansvarlig
Steg:
- `SettSaksansvarlig` (én operasjon, idempotent)

Regler:
- Hvis `oensket_saksansvarlig == naavaerende_saksansvarlig`, returnerer `planlegg_neste_handling` `Done`.
- Hvis mismatch, returnerer `Ready(SettSaksansvarlig)` og executor utfører Sikri-kallet.
- `AvsluttSak` blokkerer på saksansvarlig mismatch, men utfører ikke saksansvarlig-arbeid selv.

### Avslutt sak
Steg:
- `AvsluttSak`

## Lokal state i database

Execution v2 skiller bevisst mellom:

- **workflow-state**: hvor langt kommandoen har kommet
- **entity tilstand**: tilstandsmaskiner per domeneentitet (sak, journalpost, dokument)

### Workflow-state: `command_execution`

`command_execution` er runtime source of truth for executor. Den beskriver bare progresjon og årsak til at en kommando eventuelt ikke kan kjøre nå.

Felt på høyt nivå:
- `command_id`
- `correlation_id`
- `payload`
- `command_type`
- `sak_id`, `journalpost_id` (når relevant)
- `status`
- `attempt_no`
- `retry_ready_at`
- `last_detail`
- `utfores_venter_publisert_at`
- `created_at`, `updated_at`, `started_at`, `finished_at`

Statusene er:
- `klar`
- `kjorer`
- `blokkert_venter`
- `retry_venter`
- `ok`
- `feil`

Semantikk:
- `blokkert_venter` = prerequisite mangler i Skuffen entity tilstand; ingen timer
- `retry_venter` = recoverable teknisk feil; styres av `retry_ready_at`
- `feil` = terminal, inkludert irrecoverable stegfeil

`utfores_venter_publisert_at` brukes for idempotent publisering av `utfores::venter` ved replay eller re-registrering av samme kommando.

### Historikk: `command_execution_attempt`

Historikk per forsøk brukes for audit/debug og startup recovery. Denne tabellen er ikke scheduler-state.

### Entity tilstand (arkivfaglige fakta)

Entity tilstand er persisterte tilstandsmaskiner per domeneentitet, keyed av stabile interne `skuffen_id`.

#### `sak_tilstand`
- `sak_id`
- `tilstand` (`ikke_realisert` | `opprettet` | `avsluttet` | `feilet_permanent`)
- `sikri_id` (nullable)
- `saksnummer` (nullable)
- `oensket_saksansvarlig_id` (nullable), `oensket_saksansvarlig_enhet` (nullable) — requested fact for `SettSaksansvarlig` og `AvsluttSak` guard
- `naavaerende_saksansvarlig_id` (nullable), `naavaerende_saksansvarlig_enhet` (nullable) — faktisk saksansvarlig fra Sikri

#### `journalpost_tilstand`
- `journalpost_id`
- `sak_id` (FK)
- `tilstand` (`ikke_realisert` | `opprettet` | `dokumenter_under_arbeid` | `klar_for_journalforing` | `venter_paa_utsending` | `journalfoert` | `avskrevet` | `feilet_permanent`)
- `journalposttype`
- `med_utsending` bool
- `sikri_id` (nullable)
- `journalpostnummer` (nullable)

#### `dokument_tilstand`
- `dokument_id`
- `journalpost_id` (FK)
- `tilstand` (`ikke_realisert` | `avventer_rendring` | `ok` | `feilet_permanent`)

#### `tilstand_historikk`

Audit trail for alle tilstandsoverganger på tvers av entitetstyper.

## Entity state og target

- Entity state (sak_tilstand, journalpost_tilstand, dokument_tilstand) lagrer kun fakta om domeneentiteter.
- Det finnes ingen `oensket_tilstand` i entity tilstand. `oensket_saksansvarlig` er et eksplisitt requested fact-unntak for saksansvarlig, ikke en scheduler-kolonne.
- For journalpost-kommandoer er `command_execution.journalpost_id` targeten som brukes i `planlegg_neste_handling`.
- Phase 2 kjøres som greenfield/clean migration state: det finnes ingen reelle klienter eller live produksjonsdata, så lokale databaser kan droppes/reopprettes og base-migrasjonen kan endres direkte. Det kreves derfor ingen forward-only kompatibilitetsmigrasjon for fjerning av gamle `oensket_tilstand`-kolonner.

## Rekkefølge og blokkering

### Opprett journalpost
Krav:
- Sak finnes og er ikke avsluttet.
- Ved `SakKey::ClientReference` registreres journalpostkommandoen lokalt, men blir `blokkert_venter` til saken er opprettet i entity tilstand.
- Journalpostkommando venter også på `saksnummer` hvis saken finnes, men fortsatt mangler dette.
- Ved `SakKey::ArkivId` verifiserer Sikri-/arkivvalidering at saken finnes og er åpen før lokal seeding; etter validert kommando seeder registrering `sak_tilstand` som `Opprettet` med `saksnummer` før FK-avhengige rader (journalpost/dokument) settes inn. Seeding er idempotent og må ikke overskrive eksisterende `tilstand`, `saksnummer` eller `opprettet_av_command_id`.

### Legg til dokument
Krav:
- Journalpost må være opprettet.
- Ingen øvre grense på antall vedlegg.

### Journalfør
Krav:
- Journalpost er opprettet.
- Alle dokumenter som skal inngå er lagt til.

### Avskriv
Krav:
- Kun inngående.
- Journalpost må være journalført.

### Avslutt sak
Krav:
- Alle journalposter på saken er ferdige iht. arkivfag (se over).
- Ingen journalposter har `har_feilede_dokumenter = true`.

Hvis krav ikke er oppfylt, blir kommandoen enten:

- `blokkert_venter`, hvis prerequisite kan bli oppfylt av videre Skuffen-fremdrift
- `feil`, hvis tilstanden er irrecoverable for denne kommandoen

## Hva hvis prosessen dør midt i planen?

- `planlegg_neste_handling` er en ren domenefunksjon som leser **entity tilstand** (facts) for å avgjøre neste operasjon.
- Ved startup resettes `kjorer` til `klar`, og executor kan kjøre kommandoen på nytt.
- Åpne rader i `command_execution_attempt` markeres samtidig som `avbrutt` før `kjorer` resettes.
- Allerede fullførte steg hoppes over fordi `planlegg_neste_handling` ser at entity tilstand allerede er i riktig tilstand.
- `command_execution_attempt` brukes til å se hva som skjedde før restart.

## Timeout/feil fra arkivet

- Sikri-feil mappes til `Recoverable` eller `Irrecoverable`.
- Recoverable feil fører til `retry_venter` og planlagt `retry_ready_at` i DB.
- Irrecoverable stegfeil betyr at **hele kommandoen feiler**.
- NATS redelivery brukes kun hvis registrering i Skuffen feiler, ikke for execution retry.
- Validation bruker explicit ack per melding: `Ok` og `Irrecoverable` ACKes, mens `Recoverable` og `Blocked` NAKes for redelivery.

## Feilsemantikk

- Hvis ett steg i en kommando feiler irrecoverably, feiler **hele kommandoen**.
- En kommando kan også bli terminal `feil` allerede ved registrering hvis `planlegg_neste_handling()` returnerer `Invalid`.
- `blokkert_venter` brukes bare når kommandoen faktisk kan komme videre av senere tilstandsendringer i Skuffen.
- `retry_venter` brukes bare for tekniske feil som kan forsøkes igjen senere.
- Et dokument med `feilet_permanent` tilstand gir `EksekveringFeil::irrecoverable(...)` — kommandoen avsluttes med terminal `feil`, ikke `blokkert_venter`. Et permanent-feilet dokument kan aldri hentes og skal ikke blokkere for alltid.

## Wake-up av ventende kommandoer

`blokkert_venter` er ikke timer-basert. Når relevant entity tilstand endres, reevaluerer `ReevaluerVentendeKommandoerService` ventende kommandoer for samme sak eller journalpost.

Samme `planlegg_neste_handling()` og mapping brukes ved registrering og wake-up.

Wake-up kan flytte en kommando fra `blokkert_venter` til `klar` eller terminal `feil`. I dagens repository-seam flyttes også `Done` til `klar`: executor eier terminal success-status og `arkiv.command.done`-publisering, så wake-up stille-finaliserer ikke en tidligere blokkert kommando.
Ved `blokkert_venter -> feil` publiseres outward error-status, og `done` publiseres bare hvis `utfores::venter` tidligere faktisk ble publisert for kommandoen.

Wake-up trigges også etter persistert irrecoverable dokumentfeil, fordi `feilet_permanent` på et dokument gir umiddelbar terminal `feil` for kommandoen — ikke videre blokkering.

Hvis et step allerede er fullfort og execution derfor skiper med `AlreadyCompleted`, skal relevante wake-up scopes fortsatt trigges, slik at tidligere tapte wake-ups kan hentes inn igjen.

Det skal være enkelt å lese hvorfor en kommando venter, og hva som må skje før den kan bli `klar`.

## Backoff-strategi

Backoff skal aldri være 0. Bruk eksponentiell backoff med øvre grense til ett forsøk per dag, og fortsett med ett forsøk per dag deretter.

Eksempel:
- 1m, 5m, 15m, 1h, 6h, 12h, 24h, 24h, 24h ...

## NATS-kanaler

- Input: `arkiv.command.ready.<entity>.<commandId>`
- Status: `arkiv.status.<commandId>` (stream `arkiv_status`)
- Done: `arkiv.command.done.<entity>.<commandId>`

## Runtime og prioritet

- Inntak (`command_listener`) og media-opplasting (`media_listener`) er kritiske for opptak og har restartbudsjett på 3 forsøk før prosessen avsluttes.
- Validation, execution registration og execution worker kan være midlertidig degradert. De restartes med backoff og skal hente igjen backlog når NATS/infra er tilbake.

`<entity>` er `sak` eller `journalpost`.

## Stabile domenefeilkoder

`planlegg_neste_handling` emits stable codes via `as_code()` and `as_detail()` on `BlockedReason` and `DomainViolation`. These appear in `last_detail` and structured logs and are safe for dashboards.

Key codes and their semantics:

| Code | Type | Meaning |
|---|---|---|
| `blocked_reason=journalpost_tilstand_uavklart` | `BlockedReason` | Journalpost tilstand is not yet in a state the planner can act on; command waits for entity facts to update. Triggers wake-up on `EntityFaktaEndret`. |
| `invalid_reason=sak_feilet_permanent` | `DomainViolation` | Sak has permanently failed; the command cannot proceed and is set to terminal `feil`. |
| `invalid_reason=journalpost_feilet_permanent` | `DomainViolation` | Journalpost has permanently failed; command fails terminally. |
| `invalid_reason=dokument_feilet_permanent` | `DomainViolation` | A required document has permanently failed; command fails terminally. |

For Sikri HTTP error codes (e.g. `sikri_unknown_user`, `sikri_rate_limited`) see `.agent/guides/observability.md`.

Arkivmapping contract failures (emitted by the Sikri adapter when entity facts are inconsistent at the point of Sikri API call construction) are always irrecoverable and appear as stable `last_detail` prefixes safe for dashboards:

| Code | Type | Meaning |
|---|---|---|
| `arkivmapping_dokument_fact_mangler` | Irrecoverable | Dokument facts expected by the Sikri adapter are missing at call time; indicates an invariant violation. Command fails terminally. |
| `arkivmapping_rendered_dokument_mangler` | Irrecoverable | HTML-template hoveddokument has no `rendered_dokument_referanse` in entity facts when `OpprettJournalpost` is attempted; indicates a planner sequencing bug. Command fails terminally. |
| `arkivmapping_dokumentform_mismatch` | Irrecoverable | Dokument form (bytes vs. rendered) does not match what the Sikri adapter expected; indicates a contract violation. Command fails terminally. |
