# SKU-0007. CommandStateDecision og command executor-modell

Date: 2026-05-18
Last-reviewed: 2026-05-21
Tier: A
Status: Accepted
Crates: skuffen, domain, application, infrastructure, skuffen-integration-tests

## Related

Root: SKU-0007 | Supersedes: SKU-0002

## Context

Skuffen er et command executor-system. Klienter sender kommando-DTOer, `arkiv.status` er keyed på `command_id`, og idempotency er command-basert.

SKU-0002 introduserte per-entitet `oensket_tilstand` og en modell der executoren lukket gapet mellom nåtilstand og ønsket tilstand. Den modellen blandet command intent og entity facts. Feilen ble synlig da `command_execution` hentet én runnable command, mens `neste_handling()` valgte neste globale aggregate-operasjon for hele saken. En command kunne derfor utføre arbeid som tilhørte en annen command-type.

Vi beholder state-machine-tanken, men flytter ønsket/intensjon ut av entity-tabellene. Entity state skal beskrive hva som er sant nå. Domeneregler over command execution state og entity facts beregner neste command state.

## Decision

R1 [4]: Skuffen er en command executor, ikke en desired-state reconciler. Kommandoer eksekverer klientinstruksjoner én gang; systemet rekonsilierer ikke kontinuerlig mot entity-lagret ønsket tilstand.

R2 [5]: Execution drives av regelen `execution state(command) + entity state(facts) + domain rules -> CommandStateDecision`. Denne regelen er eneste autoritet for command readiness, blocking, completion og domain-invalidity.

R3 [5]: `CommandStateDecision` er domenets beslutningstype med variantene `Ready(ArkivOperasjon)`, `Blocked(BlockedReason)`, `Done` og `Invalid(DomainViolation)`. Application materialiserer beslutningen til `command_execution.status`.

R4 [5]: Entity state-tabeller (`sak_tilstand`, `journalpost_tilstand`, `dokument_tilstand`) lagrer facts om hva som er sant nå. Scheduler-kolonner av typen `oensket_tilstand` fjernes fra alle entity-tabeller; dette gjelder ikke det eksplisitte requested fact-feltet `oensket_saksansvarlig`.

R5 [5]: `command_execution` lagrer lifecycle, scheduling, attempts, retry, blocking og terminal status for commanden. Den lagrer ikke `next_operation`; neste operasjon beregnes på nytt fra ferske facts.

R6 [5]: Én command attempt utfører maksimalt én `ArkivOperasjon`. Etter utfallet oppdateres entity facts, og commandens neste `CommandStateDecision` materialiseres før videre arbeid.

R7 [5]: `Blocked` er eksplisitt og må ha `BlockedReason` med re-evalueringsgrunn. En command kan aldri stå i implisitt ventende, ikke-terminal state uten å være `klar`, `kjorer`, `retry_venter` eller `blokkert_venter`.

R8 [5]: `planlegg_neste_handling` er en ren domenefunksjon uten IO. Den skal bruke command type og entity facts, ikke entity-lagret desired state, til å beregne `CommandStateDecision`.

R9 [5]: Saksansvarlig er bare prerequisite for `AvsluttSak`. Journalpost-kommandoer skal ikke blokkere på saksansvarlig og skal aldri returnere `SettSaksansvarlig` som neste operasjon.

R10 [5]: `AvsluttSak` kan bare utføre `AvsluttSak`. Den blokkerer på manglende sak, uferdige journalposter og saksansvarlig mismatch; den skal ikke utføre journalpost- eller dokumentarbeid.

R11 [5]: Entity state skal ikke lagre permanent error-diagnostikk som command-progress. Feildetaljer hører hjemme i command execution attempts/status/logging; entity state kan bare lagre durable arkivfacts.

R12 [5]: Wake-up reevaluerer berørte `blokkert_venter` commands fra ferske entity facts. Sak-, journalpost- og dokumentendringer må kunne føre til `klar`, fortsatt `blokkert_venter`, `ok` eller `feil` via samme domeneregel.

## Consequences

### Positive

- Commanden som `command_execution` velger er autoritativ for hva som kan utføres.
- Entity-tabeller blir rene fact/provenance-tabeller uten global desired-state scheduler.
- `CommandStateDecision` fjerner tvetydigheten i `Option<ArkivOperasjon>` og gjør blocked/done/invalid eksplisitt.
- Wrong-envelope execution blir en domenetestbar invariant i stedet for en runtime overraskelse.
- One-operation-per-attempt gir enklere audit, retry og feilsøking.

### Negative

- `oensket_tilstand` scheduler-kolonner fjernes fra schema, domain structs, repository queries og tests.
- SKU-0002 er superseded, og ADRer som refererte den må forstå SKU-0007 som ny execution-driver.
- Application må materialisere `CommandStateDecision` i registrering, execution og wake-up.
- Wake-up må dekke journalpost- og dokumentendringer; en no-op path kan strande blocked commands.
- Existing tests rundt global `neste_handling` må omskrives til command-aware decisions.

### Tradeoffs

- Vi beholder `command_execution` som scheduling/retry-lager fremfor å introdusere `operation_execution`. Det gir mindre redesign, men krever tydelig mapping fra `CommandStateDecision` til command status.
- Vi dropper `next_operation` persistence for å unngå stale plans, men worker må beregne neste operasjon på nytt ved execution.
- `oensket_saksansvarlig` beholdes som et snevert requested fact-unntak fordi `AvsluttSak` trenger en persisted verdi å sammenligne mot; feltet driver ikke global desired-state scheduling.
