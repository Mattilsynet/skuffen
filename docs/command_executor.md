# Eksekvering av kommandoer mot Sikri

Se også ADR: `docs/adr/0001-execution-v2-og-opprydding-av-legacy-state-seams.md`.

Dette dokumentet beskriver execution v2 i Skuffen. Målet er robusthet og pålitelighet mot et upålitelig arkiv-API, men med en så tydelig modell at fremtidige endringer fortsatt er trygge.

## Mental modell

Execution skal kunne forklares slik:

1. En validert kommando registreres i execution-systemet.
2. Skuffen materialiserer lokal snapshot-state for sak, journalpost og dokument.
3. Skuffen vurderer om kommandoen er **klar**, **venter** eller terminal **feil**.
4. Én executor plukker neste kjørbare kommando og kjører den stegvis mot Sikri.
5. Etter hvert steg oppdateres snapshot-state og command execution state eksplisitt.
6. Kommandoen ender i `ok`, `retry_venter`, `venter` eller `feil`.

Det er ingen skjult workflow engine. Snapshot-state beskriver fakta. `command_execution` beskriver progresjon.

## Flyt

1. Lytt på `arkiv.command.ready.<entity>.<commandId>` (JetStream, retention 180 dager).
2. Registrer kommandoen i `command_execution` og seed snapshot-state i samme use case.
3. Bruk én eksplisitt readiness-vurderer for å avgjøre om kommandoen er:
   - `klar`
   - `venter`
   - `feil`
4. Executor starter, tar singleton-lock og resetter eventuelle hengende `kjorer`-rader tilbake til `klar`.
5. Executor plukker neste kjørbare kommando fra DB.
6. Executor kjører planen steg for steg:
   - guard leser snapshot-state
   - steg kaller Sikri
   - steg oppdaterer snapshot-state og command execution state
7. Status-eventer publiseres på `arkiv.status.<commandId>`.
8. Når execution-path publiserer en terminal status, publiseres også `arkiv.command.done.<entity>.<commandId>`.

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

## Eksekveringsplan (parse + typebasert validering)

Kommandoer blir oversatt til en plan av steg. Typebasert validering brukes for å sikre at steg har nødvendige felt før de kan kjøres (f.eks. at utgående har mottaker og hoveddokument).

### Opprett sak
Steg:
- `OpprettSak`

### Opprett journalpost (inngående/utgående/internt)
Steg:
- `OpprettJournalpost`
- `LeggTilDokument` (ett steg per dokument, ingen øvre grense)
- `Journalfør` (avhenger av type og flyt)
- `Avskriv` (kun inngående)

### Avslutt sak
Steg:
- `AvsluttSak`

## Lokal state i database

Execution v2 skiller bevisst mellom:

- **workflow-state**: hvor langt kommandoen har kommet
- **snapshot-state**: arkivfaglige fakta som guarder og regler leser

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
- `wait_kind`
- `wait_sak_id`, `wait_journalpost_id`
- `last_detail`
- `utfores_venter_publisert_at`
- `created_at`, `updated_at`, `started_at`, `finished_at`

Statusene er:
- `klar`
- `kjorer`
- `venter`
- `retry_venter`
- `ok`
- `feil`

Semantikk:
- `venter` = prerequisite mangler i Skuffen-state; ingen timer
- `retry_venter` = recoverable teknisk feil; styres av `retry_ready_at`
- `feil` = terminal, inkludert irrecoverable stegfeil

`utfores_venter_publisert_at` brukes for idempotent publisering av `utfores::venter` ved replay eller re-registrering av samme kommando.

### Historikk: `command_execution_attempt`

Historikk per forsøk brukes for audit/debug og startup recovery. Denne tabellen er ikke scheduler-state.

### Snapshot-state (arkivfaglige fakta)

### SakState
- `sak_id` (skuffen_id)
- `saksnummer` (nullable)
- `status` (B/F/A)
- `opprettet` bool

### JournalpostState
- `journalpost_id` (skuffen_id)
- `sak_id` (FK)
- `journalpostnummer` (nullable)
- `type` (I/U/X)
- `med_utsending` bool (kun U)
- `journalført` bool
- `avskrevet` bool (kun I)
- `ekspedert` bool (kun U, kun ved utsending)
- `har_feilede_dokumenter` bool

### DokumentState
- `dokument_id` (skuffen_id)
- `journalpost_id` (FK)
- `lagt_til` bool
- `irrecoverable_feil` bool

## Rekkefølge og blokkering

### Opprett journalpost
Krav:
- Sak finnes og er ikke avsluttet.
- Ved `SakKey::ClientReference` registreres journalpostkommandoen lokalt, men blir `venter` til saken er opprettet i snapshot-state.
- Journalpostkommando venter også på `saksnummer` hvis saken finnes, men fortsatt mangler dette.
- Ved `SakKey::ArkivId` seedes saken som opprettet med kjent `saksnummer`.

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

- `venter`, hvis prerequisite kan bli oppfylt av videre Skuffen-fremdrift
- `feil`, hvis tilstanden er irrecoverable for denne kommandoen

## Hva hvis prosessen dør midt i planen?

- Planen ligger i RAM, men **hvert steg skal være gated av lokal snapshot-state**.
- Ved startup resettes `kjorer` til `klar`, og executor kan kjøre kommandoen på nytt.
- Åpne rader i `command_execution_attempt` markeres samtidig som `avbrutt` før `kjorer` resettes.
- Allerede fullførte steg skal skippe basert på snapshot-state.
- `command_execution_attempt` brukes til å se hva som skjedde før restart.

## Timeout/feil fra arkivet

- Sikri-feil mappes til `Recoverable` eller `Irrecoverable`.
- Recoverable feil fører til `retry_venter` og planlagt `retry_ready_at` i DB.
- Irrecoverable stegfeil betyr at **hele kommandoen feiler**.
- NATS redelivery brukes kun hvis registrering i Skuffen feiler, ikke for execution retry.

## Feilsemantikk

- Hvis ett steg i en kommando feiler irrecoverably, feiler **hele kommandoen**.
- En kommando kan også bli terminal `feil` allerede ved registrering hvis readiness-vurdereren ser at prerequisites ikke kan bli oppfylt.
- `venter` brukes bare når kommandoen faktisk kan komme videre av senere state-endringer i Skuffen.
- `retry_venter` brukes bare for tekniske feil som kan forsøkes igjen senere.

## Wake-up av ventende kommandoer

`venter` er ikke timer-basert. Når relevant snapshot-state endres, reevaluerer Skuffen ventende kommandoer for samme sak eller journalpost.

Samme readiness-vurderer brukes:
- ved registrering
- ved wake-up

Wake-up kan flytte en kommando fra `venter` til `klar` eller terminal `feil`.
Ved `venter -> feil` publiseres outward error-status, og `done` publiseres bare hvis `utfores::venter` tidligere faktisk ble publisert for kommandoen.

Wake-up trigges også etter persistert irrecoverable dokumentfeil, fordi dette kan gjøre ventende kommandoer for samme journalpost eller sak terminalt umulige.

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

`<entity>` er `sak` eller `journalpost`.
