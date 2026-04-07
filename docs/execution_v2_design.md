# Execution v2 design

Se også ADR: `docs/adr/0001-execution-v2-og-opprydding-av-legacy-state-seams.md`.

Dette dokumentet beskriver den implementerte modellen for execution v2.

## Mål

- Høy robusthet og pålitelighet mot et upålitelig arkiv-API.
- Tydelig modell som er lett å forstå og endre riktig.
- Ingen skjult workflow engine.

## Kjerneprinsipper

1. `command_execution` eier workflow/progresjon for en kommando.
2. `sak_state`, `journalpost_state` og `dokument_state` eier snapshot-facts.
3. `EksekveringsklarhetVurderer` er eneste sted som vurderer om en kommando er `klar`, `venter` eller terminal `feil` før execution.
4. Step handlers eier idempotency/skip og step-lokal sikkerhet, men ikke separat prerequisite-policy.
5. `retry_venter` er kun for recoverable tekniske feil.
6. `venter` er kun for prerequisites som kan bli oppfylt av videre fremdrift i Skuffen.
7. Irrecoverable stegfeil betyr at hele kommandoen feiler.

## Execution state machine

Tillatte runtime-statuser:

- `klar`
- `kjorer`
- `venter`
- `retry_venter`
- `ok`
- `feil`

Tillatte overganger:

- `klar -> kjorer`
- `retry_venter -> kjorer` når `retry_ready_at <= now()`
- `venter -> klar` når prerequisite reevalueres som oppfylt
- `venter -> feil` når wake-up reevaluerer prerequisite som terminalt umulig
- `kjorer -> ok`
- `kjorer -> venter`
- `kjorer -> retry_venter`
- `kjorer -> feil`
- `kjorer -> klar` ved startup recovery etter prosessdød

Ikke tillatt:

- `venter -> retry_venter` uten ny execution
- `retry_venter -> venter` uten ny execution
- terminale statuser tilbake til kjørbare statuser

## Årsakstyper

`venter` må ha eksplisitt ventegrunn:

- `sak_opprettet`
- `saksnummer_tildelt`
- `journalpost_opprettet`
- `journalpostnummer_tildelt`
- `journalpost_journalfoert`
- `sak_har_uferdige_journalposter`

Det skal alltid være tydelig hva kommandoen venter på og hvilken entity det gjelder.

## Guard vs readiness evaluator

### `EksekveringsklarhetVurderer`
Eier prerequisite-vurdering for om kommandoen er:

- `klar`
- `venter`
- `feil`

Brukes ved:

- registrering
- wake-up/re-evaluering

### Step handlers
Eier kun:

- step-lokal idempotency / `AlreadyCompleted`
- step-lokal sikkerhet før external side effect
- mapping av external feil til recoverable/irrecoverable

Step handlers skal ikke innføre en konkurrerende generell prerequisite-policy ved siden av vurdereren.

`EksekveringsklarhetVurderer` avgjør også om en observerbar prerequisite-mangel er `venter` eller terminal `feil` ut fra lokale facts og domeneregler. Step handlers kan fortsatt produsere irrecoverable feil når et faktisk step-kall eller en step-lokal invariant bryter sammen under execution.

## Snapshot-facts og partial progress

- Snapshot-state beskriver fakta som faktisk er observert eller fullført.
- Hvis et tidlig steg lykkes og et senere steg feiler irrecoverably, skal allerede persisterte fakta bli stående.
- Kommandoen blir likevel terminal `feil`.
- Snapshot-tabellene er keyed av stabile interne `skuffen_id`.
- Snapshot-oppdateringer er merge/upsert, ikke wholesale replace.

Dette er bevisst: facts og workflow-resultat er to forskjellige ting.

## Registrering og replay

- Registrering er idempotent per `command_id`.
- Hvis `command_execution` allerede finnes, men `utfores_venter_publisert_at` mangler, kan registrering publisere `utfores::venter` på nytt uten å materialisere ny snapshot-state.
- Registrering kan også ende direkte i terminal `feil` hvis readiness-vurdereren ser at prerequisites ikke kan bli oppfylt.

## Wake-up contract

Ventende kommandoer skal reevalueres når relevant snapshot-state endres.

Per i dag reevalueres på scope:

- `sak_id`
- `journalpost_id`

Triggere i application flow:

- etter `anvend_sak_transition`
- etter `anvend_journalpost_opprettet`
- etter `anvend_journalpost_overgang_ved_journalfoering`
- etter `anvend_journalpost_avskrevet` når dette kan påvirke `AvsluttSak`
- etter dokumentoverganger som påvirker journalpost-kompletthet

Wake-up skal bruke samme `EksekveringsklarhetVurderer` som registrering.

Hvis wake-up reevaluerer en ventende kommando til terminal `feil`, skal Skuffen også publisere terminal outward status. `done` publiseres bare hvis kommandoen allerede har observert outward `utfores::venter`.

Wake-up må også være retry-tolerant: hvis et step allerede er fullfort i snapshot-state og senere execution derfor returnerer `AlreadyCompleted`, skal relevante wake-up scopes fortsatt trigges pa nytt.

## Startup recovery og singleton executor

- Kun én executor skal være aktiv.
- Executor tar en global Postgres advisory lock ved startup.
- Advisory lock holdes på en dedikert session/connection som lever like lenge som executor.
- Hvis lock ikke fås, skal executor ikke starte.
- Åpne `command_execution_attempt` markeres som `avbrutt` ved startup recovery.
- Ved startup resettes alle `kjorer` til `klar`.

Dette er tilstrekkelig fordi systemet har nøyaktig én executor.

## Step idempotency og reconcile-strategi

Execution v2 baserer seg per i dag på lokal snapshot-state for replay/skip.

Systemet bruker følgende skip-regler:

- `opprett_sak` skiper hvis `sak_state.opprettet` allerede er sann
- `opprett_journalpost` skiper hvis `journalpostnummer` allerede finnes
- `legg_til_dokument` skiper hvis `dokument_state.lagt_til` allerede er sann
- `journalfoer` skiper hvis `journalfoert` allerede er sann
- `avskriv` skiper hvis `avskrevet` allerede er sann
- `avslutt_sak` skiper hvis sak allerede er avsluttet

Mer avansert reconcile mot Sikri er foreløpig ikke implementert.

## Dokumentfeil

- Irrecoverable dokumentfeil betyr at hele kommandoen feiler.
- `dokument_state.irrecoverable_feil` og eventuell aggregert journalpost-state må likevel materialiseres når det er nødvendig for senere kommandoer.

## Public status mapping

- Intern statusmodell kan være norsk og presis.
- Outward status events beholdes kompatible så langt mulig.
- `utfores::venter` beholdes som ikke-terminal queued/venter-status.
- Execution `venter` mapes fortsatt til outward blocked/pending semantics der det er riktig, men intern modell styres ikke av outward naming.

## Implementerte schema-beslutninger

- `command_execution_attempt` tas med fra start.
- `wait_kind` lagres på `command_execution`-raden, ikke i egen tabell.
- `command_execution` skal ha streng nullability/checks for `retry_ready_at`, `wait_kind`, `finished_at`.
- `utfores_venter_publisert_at` lagres på `command_execution`-raden for idempotent replay av outward `utfores::venter`.
- Snapshot-state keyed av stabile interne `skuffen_id`.
- `har_feilede_dokumenter` beholdes foreløpig som lagret felt, men må oppdateres eksplisitt og konsekvent.
- `id_mapping.client_reference` beholdes globalt unik i denne iterasjonen.
- `arkiv_id` i `id_mapping` er unik per `entity_type` når den finnes.
