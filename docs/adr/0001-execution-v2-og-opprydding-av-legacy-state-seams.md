# ADR 0001: execution v2 og opprydding av legacy state seams

- Status: Accepted
- Date: 2026-04-07

## Kontekst

Skuffen trenger en execution-modell som er robust mot prosessdød, replay og ustabilt arkiv-API, men som fortsatt er enkel å forstå og vedlikeholde.

Den tidligere løsningen hadde overgangsgjeld:

- legacy execution-port og adapter-bridge levde videre side om side med ny modell
- workflow/progresjon og arkivfaglige facts var ikke tydelig nok separert
- prerequisite-vurdering kunne bli implisitt fordelt mellom flere steder

Vi ønsket å rydde dette uten å redesigne command flow på nytt.

## Beslutning

### 1. `command_execution` er eneste workflow-state

`command_execution` er runtime source of truth for hvor en kommando er i execution-løpet.

Denne tabellen eier:

- status for kommandoen
- retry-tidspunkt
- wait-grunn og wait-scope
- attempt-teller
- siste detalj / feilbeskrivelse
- markering av om outward `utfores::venter` er publisert

Outward status events og step handlers er ikke workflow source of truth.

### 2. Snapshot-state er separat fra workflow-state

`sak_state`, `journalpost_state` og `dokument_state` eier observerte og materialiserte facts.

Disse facts brukes for:

- guards og readiness
- replay og idempotent skip
- vurdering av senere kommandoer

Dette betyr at en kommando kan bli terminal `feil` samtidig som tidligere fullførte steg fortsatt står igjen som gyldige facts i snapshot-state.

### 3. Readiness samles i `EksekveringsklarhetVurderer`

Det finnes ett sted som vurderer om en kommando er:

- `klar`
- `venter`
- terminal `feil`

Samme vurderer brukes:

- ved registrering i execution-systemet
- ved wake-up / re-evaluering av ventende kommandoer

Step handlers eier fortsatt step-lokal idempotency og mapping av external feil, men ikke en konkurrerende generell prerequisite-policy.

### 4. `venter` og `retry_venter` har ulik semantikk

- `venter` betyr at kommandoen mangler prerequisites i lokal snapshot-state
- `retry_venter` betyr recoverable teknisk feil med backoff

`venter` er derfor ikke timer-basert, mens `retry_venter` er det.

### 5. Single executor er et bevisst valg

Systemet kjører med én aktiv executor.

Dette håndheves med:

- Postgres advisory lock
- startup recovery som resetter `kjorer -> klar`
- marking av åpne `command_execution_attempt` som `avbrutt`

Vi velger dette fordi det gir en tydelig og robust modell uten behov for mer komplisert distribuert locking eller work partitioning.

### 6. Wake-up av ventende kommandoer er eventdrevet fra snapshot-endringer

Ventende kommandoer reevalueres når relevant snapshot-state endres for samme:

- `sak_id`
- `journalpost_id`

Wake-up kan flytte en kommando fra `venter` til:

- `klar`
- fortsatt `venter`
- terminal `feil`

Hvis wake-up ender i terminal `feil`, publiseres outward error-status. `done` publiseres bare hvis outward `utfores::venter` tidligere faktisk ble publisert.

### 7. Legacy execution seams skal ikke videreføres

Den gamle `EksekveringStateRepository`-seamen og legacy Postgres-bridge fjernes som del av execution v2 cleanup.

Videre utvikling skal bruke eksplisitte porter:

- `CommandExecutionRepository`
- `EksekveringSnapshotRepository`
- `VentendeKommandoWakeup`

Nye endringer skal ikke gjeninnføre blandede seams der workflow-state, snapshot-state eller readiness uttrykkes flere steder samtidig.

## Alternativer vurdert

### Beholde én stor legacy state-port

Forkastet fordi den blandet workflow-state, snapshot-facts og overgangslogikk i samme seam og gjorde boundaryene uklare.

### La step handlers eie mer prerequisite-logikk

Forkastet fordi prerequisite-policy da ville blitt distribuert og vanskeligere å holde konsistent mellom registrering, execution og wake-up.

### Bruke timers også for `venter`

Forkastet fordi `venter` er domenedrevet og bør reevalueres når facts faktisk endres, ikke på vilkårlige tidspunkter.

## Tradeoffs

Fordeler:

- tydeligere mental modell
- bedre replay- og recovery-egenskaper
- mindre overgangsgjeld
- lettere å teste readiness og wake-up isolert

Ulemper:

- flere eksplisitte porter og typer
- mer bevisst koordinering mellom workflow-state og snapshot-state
- single-executor-modellen begrenser parallellisering

## Konsekvenser

- schema og repositories må opprettholde streng semantikk for status, wait-grunn og attempts
- docs og tester må bruke v2-statusene `klar`, `kjorer`, `venter`, `retry_venter`, `ok`, `feil`
- wake-up og registrering må forbli koblet til samme readiness-vurderer
- integration tests må validere både workflow-state og observerbar lifecycle

## Åpne spørsmål

- Om `PostgresExecutionStore` senere bør splittes i to adapters er et vedlikeholdsvalg, ikke en låst arkitekturbeslutning nå.
- Mer avansert reconcile mot Sikri er fortsatt ikke implementert.

## Relaterte dokumenter

- `docs/command_executor.md`
- `docs/execution_v2_design.md`
