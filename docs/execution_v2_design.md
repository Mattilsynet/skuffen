# Execution v2 design

Se også ADR: `docs/adr/skuffen/SKU-0001-execution-v2-og-opprydding-av-legacy-state-seams.md`.

Dette dokumentet beskriver den implementerte modellen for execution v2.

## Mål

- Høy robusthet og pålitelighet mot et upålitelig arkiv-API.
- Tydelig modell som er lett å forstå og endre riktig.
- Ingen skjult workflow engine.

## Kjerneprinsipper

1. `command_execution` eier workflow/progresjon for en kommando.
2. `sak_tilstand`, `journalpost_tilstand` og `dokument_tilstand` eier entity tilstand som persisterte tilstandsmaskiner.
3. `RegistrerIEksekveringssystemService` oppretter entity tilstand-rader og evaluerer klarhet via `evaluer_klarhet()` — eneste sted som vurderer om en kommando er `klar`, `blokkert_venter` eller terminal `feil` før execution.
4. Step handlers eier idempotency/skip og step-lokal sikkerhet, men ikke separat prerequisite-policy.
5. `retry_venter` er kun for recoverable tekniske feil.
6. `blokkert_venter` er kun for prerequisites som kan bli oppfylt av videre fremdrift i Skuffen.
7. Irrecoverable stegfeil betyr at hele kommandoen feiler.

## Execution state machine

Tillatte runtime-statuser:

- `klar`
- `kjorer`
- `blokkert_venter`
- `retry_venter`
- `ok`
- `feil`

Tillatte overganger:

- `klar -> kjorer`
- `retry_venter -> kjorer` når `retry_ready_at <= now()`
- `blokkert_venter -> klar` når prerequisite reevalueres som oppfylt
- `blokkert_venter -> feil` når wake-up reevaluerer prerequisite som terminalt umulig
- `kjorer -> ok`
- `kjorer -> blokkert_venter`
- `kjorer -> retry_venter`
- `kjorer -> feil`
- `kjorer -> klar` ved startup recovery etter prosessdød

Ikke tillatt:

- `blokkert_venter -> retry_venter` uten ny execution
- `retry_venter -> blokkert_venter` uten ny execution
- terminale statuser tilbake til kjørbare statuser

## Årsakstyper

Blokkering i `blokkert_venter` er implisitt — `planlegg_neste_handling` returnerer `CommandStateDecision::Blocked` når prerequisites ikke er oppfylt. Det finnes ikke lenger eksplisitte `wait_kind`-årsakstyper lagret i databasen. Klarhet vurderes ved å laste `SakMedBarn` og inspisere entity tilstand direkte.

## Guard vs readiness evaluator

### `evaluer_klarhet()` i `RegistrerIEksekveringssystemService`
Eier prerequisite-vurdering for om kommandoen er:

- `klar`
- `blokkert_venter`
- `feil`

Brukes ved:

- registrering
- wake-up/re-evaluering via `ReevaluerVentendeKommandoerService`

### Step handlers
Eier kun:

- step-lokal idempotency / `AlreadyCompleted`
- step-lokal sikkerhet før external side effect
- mapping av external feil til recoverable/irrecoverable

Step handlers skal ikke innføre en konkurrerende generell prerequisite-policy ved siden av vurdereren.

`evaluer_klarhet()` avgjør også om en observerbar prerequisite-mangel er `blokkert_venter` eller terminal `feil` ut fra lokale facts og domeneregler. Step handlers kan fortsatt produsere irrecoverable feil når et faktisk step-kall eller en step-lokal invariant bryter sammen under execution.

## Entity tilstand og partial progress

- Entity tilstand beskriver fakta som faktisk er observert eller fullført, som persisterte tilstandsmaskiner per domeneentitet.
- Hvis et tidlig steg lykkes og et senere steg feiler irrecoverably, skal allerede persisterte fakta bli stående.
- Kommandoen blir likevel terminal `feil`.
- Tilstandstabellene er keyed av stabile interne `skuffen_id`.
- Tilstandsoppdateringer er merge/upsert, ikke wholesale replace.

Dette er bevisst: facts og workflow-resultat er to forskjellige ting.

## Registrering og replay

- Registrering er idempotent per `command_id`.
- Hvis `command_execution` allerede finnes, men `utfores_venter_publisert_at` mangler, kan registrering publisere `utfores::venter` på nytt uten å materialisere ny entity tilstand.
- Registrering kan også ende direkte i terminal `feil` hvis `evaluer_klarhet()` ser at prerequisites ikke kan bli oppfylt.

## Wake-up contract

Ventende kommandoer skal reevalueres når relevant entity tilstand endres.

Per i dag reevalueres på scope:

- `sak_id`
- `journalpost_id`

Triggere i application flow:

- etter `anvend_sak_transition`
- etter `anvend_journalpost_opprettet`
- etter `anvend_journalpost_overgang_ved_journalforing`
- etter `anvend_journalpost_avskrevet` når dette kan påvirke `AvsluttSak`
- etter dokumentoverganger som påvirker journalpost-kompletthet

Wake-up skal bruke samme `evaluer_klarhet()` som registrering.

Hvis wake-up reevaluerer en ventende kommando til terminal `feil`, skal Skuffen også publisere terminal outward status. `done` publiseres bare hvis kommandoen allerede har observert outward `utfores::venter`.

Wake-up må også være retry-tolerant: hvis et step allerede er fullført i entity tilstand og `planlegg_neste_handling` derfor returnerer `Done`, skal relevante wake-up scopes fortsatt trigges på nytt.

## Startup recovery og singleton executor

- Kun én executor skal være aktiv.
- Executor tar en global Postgres advisory lock ved startup.
- Advisory lock holdes på en dedikert session/connection som lever like lenge som executor.
- Hvis lock ikke fås, skal executor ikke starte.
- Åpne `command_execution_attempt` markeres som `avbrutt` ved startup recovery.
- Ved startup resettes alle `kjorer` til `klar`.

Dette er tilstrekkelig fordi systemet har nøyaktig én executor.

## Step idempotency og reconcile-strategi

Execution v2 baserer seg på entity tilstand for replay/skip. Skip-logikken er basert på `planlegg_neste_handling`: `Done` returneres når entity allerede er i riktig tilstand.

Systemet bruker følgende skip-regler:

- `opprett_sak` skipes hvis `sak_tilstand.tilstand != IkkeRealisert`
- `opprett_journalpost` skipes hvis `journalpost_tilstand.tilstand != IkkeRealisert`
- `legg_til_dokument` skipes hvis `dokument_tilstand.tilstand == Ok`
- `journalfoer` skipes hvis `journalpost_tilstand.tilstand == Journalfoert`
- `avskriv` skipes hvis `journalpost_tilstand.tilstand == Avskrevet`
- `avslutt_sak` skipes hvis `sak_tilstand.tilstand == Avsluttet`

Mer avansert reconcile mot Sikri er foreløpig ikke implementert.

## Dokumentfeil

- Et dokument med `FeiletPermanent` tilstand gir `EksekveringFeil::irrecoverable(...)` — hele kommandoen feiler terminalt (`feil`), ikke `blokkert_venter`.
- Et permanent-feilet dokument kan aldri hentes; kommandoen skal feile umiddelbart fremfor å vente for alltid.
- `tilstand_historikk` materialiseres likevel for audit trail.

## Public status mapping

- Intern statusmodell kan være norsk og presis.
- Outward status events beholdes kompatible så langt mulig.
- `utfores::venter` beholdes som ikke-terminal queued/venter-status.
- Execution `venter` mapes fortsatt til outward blocked/pending semantics der det er riktig, men intern modell styres ikke av outward naming.

## Implementerte schema-beslutninger

- `command_execution_attempt` tas med fra start.
- `command_execution` har ikke lenger `wait_kind`/`wait_sak_id`/`wait_journalpost_id` — blokkering er implisitt via entity tilstand.
- `command_execution` skal ha streng nullability/checks for `retry_ready_at`, `finished_at`.
- `utfores_venter_publisert_at` lagres på `command_execution`-raden for idempotent replay av outward `utfores::venter`.
- Entity tilstand keyed av stabile interne `skuffen_id`.
- `tilstand_historikk` brukes for audit trail av alle tilstandsoverganger.
- `id_mapping.client_reference` beholdes globalt unik i denne iterasjonen.
- `arkiv_id` i `id_mapping` er unik per `entity_type` når den finnes.
