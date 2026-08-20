# SKU-0001. execution v2 og opprydding av legacy state seams

Date: 2026-04-07
Last-reviewed: 2026-08-18
Tier: A
Status: Superseded by SKU-0016
Crates: skuffen, domain, application, infrastructure, sikri_client, skuffen-integration-tests

## Related

Root: SKU-0001

## Context

Skuffen trenger en execution-modell som er robust mot prosessdød, replay og ustabilt arkiv-API, men som fortsatt er enkel å forstå og vedlikeholde.

Den tidligere løsningen hadde overgangsgjeld:

- legacy execution-port og adapter-bridge levde videre side om side med ny modell
- workflow/progresjon og arkivfaglige facts var ikke tydelig nok separert
- prerequisite-vurdering kunne bli implisitt fordelt mellom flere steder

Vi ønsket å rydde dette uten å redesigne command flow på nytt.

## Decision

R1 [4]: `command_execution` er runtime source of truth for hvor en kommando er i execution-løpet. Tabellen eier status, retry-tidspunkt, wait-grunn, wait-scope, attempt-teller, siste detalj/feilbeskrivelse, og markering av om outward `utfores::venter` er publisert. Outward status events og step handlers er ikke workflow source of truth.

R2 [4]: `sak_state`, `journalpost_state` og `dokument_state` eier observerte og materialiserte facts som brukes for guards, readiness, replay, idempotent skip, og vurdering av senere kommandoer. En kommando kan bli terminal `feil` samtidig som tidligere fullførte steg står igjen som gyldige facts i snapshot-state.

R3 [4]: Ett sted vurderer om en kommando er `klar`, `venter`, eller terminal `feil`. Samme vurdering brukes ved registrering i execution-systemet og ved wake-up/re-evaluering av ventende kommandoer. Step handlers eier fortsatt step-lokal idempotency og mapping av external feil, men ikke en konkurrerende generell prerequisite-policy.

R4 [4]: `venter` betyr at kommandoen mangler prerequisites i lokal snapshot-state, mens `retry_venter` betyr recoverable teknisk feil med backoff. `venter` er derfor ikke timer-basert, mens `retry_venter` er det.

R5 [4]: Systemet kjører med én aktiv executor håndhevet med Postgres advisory lock, startup recovery som resetter `kjorer -> klar`, og marking av åpne `command_execution_attempt` som `avbrutt`. Dette gir en tydelig og robust modell uten behov for mer komplisert distribuert locking eller work partitioning.

R6 [4]: Ventende kommandoer reevalueres når relevant snapshot-state endres for samme `sak_id` eller `journalpost_id`. Wake-up kan flytte en kommando fra `venter` til `klar`, fortsatt `venter`, eller terminal `feil`. Hvis wake-up ender i terminal `feil`, publiseres outward error-status. `done` publiseres bare hvis outward `utfores::venter` tidligere faktisk ble publisert.

R7 [4]: Den gamle `EksekveringStateRepository`-seamen og legacy Postgres-bridge fjernes som del av execution v2 cleanup. Videre utvikling skal bruke eksplisitte porter: `CommandExecutionRepository`, `EksekveringSnapshotRepository`, `VentendeKommandoWakeup`. Nye endringer skal ikke gjeninnføre blandede seams der workflow-state, snapshot-state eller readiness uttrykkes flere steder samtidig.

### Implementeringsnoter fra 2026-05-18

SKU-0007 superseder SKU-0002 og presiserer execution v2-modellen:

- Entity state-tabeller lagrer facts, ikke `oensket_tilstand` eller global desired state.
- Readiness, blocking, completion og invalidity beregnes som `CommandStateDecision` fra command execution state, entity facts og domeneregler.
- `command_execution` materialiserer lifecycle/scheduling-status for commanden, men lagrer ikke `next_operation`.
- `planlegg_neste_handling` erstatter `neste_handling` som command-aware domenefunksjon.
- One operation per command attempt er standarden for tydelig audit og retry.

### Implementeringsnoter fra 2026-04-20

Entity state machine execution-modellen (implementert 2026-04-20) er den realiserte implementasjonen av disse målene. Konkrete endringer fra ADR-teksten:

- **Tabellnavn:** Snapshot-tabellene (`sak_state`, `journalpost_state`, `dokument_state`) er erstattet av tilstandsmaskin-tabeller (`sak_tilstand`, `journalpost_tilstand`, `dokument_tilstand`) og tilhørende `tilstand_historikk`-tabell for audit trail. SKU-0007 fjerner senere `oensket_tilstand`-kolonnene.
- **Port:** `EksekveringSnapshotRepository` er erstattet av `EntityTilstandRepository`.
- **Klarhetsvurdering:** `EksekveringsklarhetVurderer` er erstattet av `evaluer_klarhet()` i `RegistrerIEksekveringssystemService`. Samme funksjon brukes ved registrering og ved wake-up via `ReevaluerVentendeKommandoerService`.
- **Status rename:** `venter` er omdøpt til `blokkert_venter` i `command_execution` for klarhet. `wait_kind`/`wait_sak_id`/`wait_journalpost_id`-kolonnene er fjernet — blokkering er nå implisitt via entity tilstand.
- **Dokumentfeil:** `FeiletPermanent` dokument gir terminal `feil` for kommandoen (ikke `blokkert_venter`). Et permanent-feilet dokument kan aldri hentes og skal ikke blokkere for alltid.
- **Eksekveringsmodell:** Ingen in-memory plan. SKU-0007 erstatter `neste_handling(command_type, &SakMedBarn)` med `planlegg_neste_handling`, som returnerer `CommandStateDecision`.

Se SKU-0007 for full beslutningsdokumentasjon om command executor-modellen.

## Consequences

- schema og repositories må opprettholde streng semantikk for status, wait-grunn og attempts
- docs og tester må bruke v2-statusene `klar`, `kjorer`, `blokkert_venter`, `retry_venter`, `ok`, `feil`
- wake-up og registrering må forbli koblet til samme readiness-vurderer
- integration tests må validere både workflow-state og observerbar lifecycle

### Alternatives vurdert

**Beholde én stor legacy state-port**

Forkastet fordi den blandet workflow-state, snapshot-facts og overgangslogikk i samme seam og gjorde boundaryene uklare.

**La step handlers eie mer prerequisite-logikk**

Forkastet fordi prerequisite-policy da ville blitt distribuert og vanskeligere å holde konsistent mellom registrering, execution og wake-up.

**Bruke timers også for `venter`**

Forkastet fordi `venter` er domenedrevet og bør reevalueres når facts faktisk endres, ikke på vilkårlige tidspunkter.

### Tradeoffs

**Fordeler:**

- tydeligere mental modell
- bedre replay- og recovery-egenskaper
- mindre overgangsgjeld
- lettere å teste readiness og wake-up isolert

**Ulemper:**

- flere eksplisitte porter og typer
- mer bevisst koordinering mellom workflow-state og snapshot-state
- single-executor-modellen begrenser parallellisering

### Åpne spørsmål

- Om `PostgresExecutionStore` senere bør splittes i to adapters er et vedlikeholdsvalg, ikke en låst arkitekturbeslutning nå.
- Mer avansert reconcile mot Sikri er fortsatt ikke implementert.

## Retirement

SKU-0001 er superseded by SKU-0016. R2 og R5 lever videre: entity state eier
facts, og én executor håndheves med advisory lock. R1, R3, R4, R6 og R7 er
forkastet — `command_execution` finnes ikke, readiness vurderes per operasjon, og
wake-up er erstattet av et periodisk evalueringspass.
