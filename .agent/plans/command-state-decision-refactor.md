# Plan: CommandStateDecision og command executor-modell

**Status:** Ready for implementation in clean context
**Scope:** `domain`, `application`, `infrastructure`, `skuffen-integration-tests`, ADR/docs
**Database posture:** Clean migration state is allowed. There are no real clients yet, so the implementation may drop/recreate the local database and remove old columns directly.

## Context

Skuffen is a **command executor**, not a desired-state reconciler. External clients send command DTOs, `arkiv.status` is keyed by `command_id`, and idempotency is command-based.

The current execution bug came from mixing two schedulers:

1. `command_execution` selects a runnable command envelope.
2. `neste_handling()` then selects the next global operation for the whole sak aggregate.

That made the selected command partly irrelevant and allowed wrong-envelope execution, e.g. `OpprettSak` executing `SettSaksansvarlig`, or `AvsluttSak` executing journalpost work. This must be impossible by construction.

## Architecture decision this plan implements

Core rule:

```text
execution state(command) + entity state(facts) + domain rules
  -> CommandStateDecision
```

Entity state records what is true now. It must not store desired end state or drive execution by closing `tilstand -> oensket_tilstand`. The command and domain rules provide intent and progress interpretation.

`CommandStateDecision` is the single domain output used by registration, execution, and wake-up:

```rust
enum CommandStateDecision {
    Ready(ArkivOperasjon),
    Blocked(BlockedReason),
    Done,
    Invalid(DomainViolation),
}
```

Persistence mapping:

| Domain decision | `command_execution.status` |
|---|---|
| `Ready(_)` | `klar` |
| `Blocked(_)` | `blokkert_venter` |
| `Done` | `ok` |
| `Invalid(_)` | `feil` |

Do **not** store `next_operation`. `Ready(_)` may be materialized as `klar`, but the next operation is recomputed from fresh facts when the worker executes.

One command attempt executes at most one archive operation. After every outcome, update entity facts and materialize the next `CommandStateDecision`.

There must be no implicit waiting state. If a command cannot progress, it must be in `blokkert_venter` with an explicit `BlockedReason`, in `retry_venter`, or terminal `ok`/`feil`.

## Durable domain rules

- `OpprettSak` may only create/realize the sak. When the sak has `saksnummer`, the command is `Done`.
- Journalpost commands may only advance their own journalpost and documents. They may not set saksansvarlig and may not close the sak.
- `SettSaksansvarlig` may only set saksansvarlig. It is `Done` when current saksansvarlig equals the requested value.
- `AvsluttSak` may only close the sak. It blocks until the sak is opprettet, all journalposter are terminal, and requested saksansvarlig is set.
- Saksansvarlig is a prerequisite only for `AvsluttSak`, not for journalpost work.
- Entity state should not hold permanent-error diagnostics. Error details belong in command execution attempts/status diagnostics. Entity state may represent durable facts only when the fact itself is part of the archive-domain model.

## Phase 0 — Documentation and ADR baseline

Implementation must begin from the ADRs updated with this plan. Before code changes, run:

```text
cargo run -p adr-fmt -- --context domain
cargo run -p adr-fmt -- --context application
cargo run -p adr-fmt -- --context infrastructure
```

Hard ADR constraints for the implementation:

- SKU-0007 supersedes SKU-0002 for execution driving semantics.
- SKU-0002 is stale and must not be treated as requiring `oensket_tilstand`.
- SKU-0003 states that saksansvarlig is relevant to `AvsluttSak`, not journalpost work.
- SKU-0001 still owns the broader execution v2 constraints: command_execution is runtime source of truth, one readiness policy, and wake-up re-evaluates blocked commands from facts.

## Phase 1 — Domain: introduce `CommandStateDecision`

### Files likely touched

- `src/domain/src/eksekvering/tilstand.rs`
- optionally new `src/domain/src/eksekvering/decision.rs`

### Work

1. Define:
   - `CommandStateDecision`
   - `BlockedReason`
   - `DomainViolation`
2. Replace `neste_handling(...) -> Result<Option<ArkivOperasjon>, EksekveringFeil>` with:

   ```rust
   planlegg_neste_handling(command_type: CommandTypeCode, sak: &SakMedBarn) -> CommandStateDecision
   ```

3. Make `command_type` load-bearing. Branch by command type first.
4. Remove external `er_ferdig` usage as a separate readiness/completion signal. `Done` is the authoritative completion signal.
5. Audit every old `Ok(None)` case and classify it as `Done` or `Blocked(reason)`. No unclassified no-op path may remain.

### Required rules in code

- `OpprettSak + sak without saksnummer` -> `Ready(OpprettSak)`.
- `OpprettSak + sak with saksnummer` -> `Done`.
- Journalpost command + missing sak/saksnummer -> `Blocked(SakMangler)` or equivalent.
- Journalpost command + own uncreated journalpost -> `Ready(OpprettJournalpost)`.
- Journalpost command + own HTML template not rendered -> `Ready(RenderDokument)` when required fields are available, otherwise `Blocked(...)`.
- Journalpost command + own document not added -> `Ready(LeggTilDokument)`.
- Journalpost command + own journalpost not terminal -> `Ready(Journalfoer)` or `Ready(Avskriv)` as appropriate.
- Journalpost command must not check or execute saksansvarlig.
- `SettSaksansvarlig + missing saksnummer` -> `Blocked(...)`.
- `SettSaksansvarlig + mismatch` -> `Ready(SettSaksansvarlig)`.
- `SettSaksansvarlig + match` -> `Done`.
- `AvsluttSak + unfinished journalposter` -> `Blocked(JournalposterIkkeFerdige)`.
- `AvsluttSak + saksansvarlig mismatch` -> `Blocked(SaksansvarligIkkeSatt)`.
- `AvsluttSak + all prerequisites met` -> `Ready(AvsluttSak)`.
- `AvsluttSak + sak already closed` -> `Done`.

### Tests

Write domain tests before or with implementation:

- `OpprettSak` never returns `SettSaksansvarlig`, journalpost operations, or `AvsluttSak`.
- Journalpost commands are not blocked by saksansvarlig mismatch.
- Journalpost commands never return `SettSaksansvarlig` or `AvsluttSak`.
- Journalpost commands only return operations for their own journalpost/documents.
- `SettSaksansvarlig` never returns journalpost or close operations.
- `AvsluttSak` blocks on unfinished journalposter.
- `AvsluttSak` blocks on saksansvarlig mismatch.
- Every non-ready/non-done path has explicit `BlockedReason` or `DomainViolation`.

## Phase 2 — Schema and entity facts: remove `oensket_tilstand`

### Files likely touched

- `migrations/*`
- `src/domain/src/eksekvering/tilstand.rs`
- `src/application/src/command/ports/entity_tilstand_port.rs`
- `src/infrastructure/src/command/adapter/*entity*` / Postgres tilstand adapter files

### Work

1. Remove `oensket_tilstand` from:
   - `sak_tilstand`
   - `journalpost_tilstand`
   - `dokument_tilstand`
2. Remove fields from domain structs:
   - `SakMedBarn.oensket_tilstand`
   - `JournalpostMedDokumenter.oensket_tilstand`
3. Remove port and adapter paths that update desired state, especially `oppdater_sak_oensket_tilstand`.
4. Update all SQL reads/writes and constructors.
5. Keep saksansvarlig columns for now:
   - requested saksansvarlig fields
   - current/confirmed saksansvarlig fields

### Migration guidance

Clean DB recreation is allowed. Prefer updating the base migration if that is the project convention for pre-client schema cleanup. Otherwise add a forward migration that drops the columns. Do not preserve compatibility for existing deployed test data.

### Tests

- Compile domain/application/infrastructure after removing fields.
- Repository tests must verify `hent_sak_med_barn` builds facts without desired state.
- No code may reference `oensket_tilstand` after this phase, except stale ADRs or migration history if intentionally retained.

## Phase 3 — Application: materialize `CommandStateDecision`

### Files likely touched

- `src/application/src/command/services/registrer_i_eksekveringssystem.rs`
- `src/application/src/command/services/eksekver_kommando.rs`
- `src/application/src/command/services/eksekver_kommando/lifecycle_publisher.rs`
- `src/application/src/command/services/reevaluer_ventende_kommandoer.rs`
- `src/application/src/command/services/eksekvering_worker.rs`
- `src/application/src/command/ports/command_execution_port.rs`

### Work

1. Registration:
   - create/ensure entity fact rows
   - load facts
   - call `planlegg_neste_handling`
   - insert/materialize command_execution status from the decision
2. Execution:
   - worker fetches `klar`
   - mark `kjorer`
   - load fresh facts
   - call `planlegg_neste_handling`
   - if `Ready(operation)`, execute exactly one archive operation
   - update entity facts from outcome
   - re-evaluate and materialize the next command_execution status
3. Wake-up:
   - find blocked commands affected by entity fact changes
   - re-evaluate each command from fresh facts
   - materialize `Ready`/`Blocked`/`Done`/`Invalid`

### Mapping details

- `Ready(_)` -> `klar`; publish/retain non-terminal status as appropriate.
- `Blocked(reason)` -> `blokkert_venter` with safe detail/reason.
- `Done` -> `ok`; publish terminal `arkiv.status` and `done` when required by existing lifecycle semantics.
- `Invalid(violation)` -> `feil`; publish terminal error status with sanitized detail.
- Recoverable technical failure during operation execution -> `retry_venter`, not `Blocked`.

### Do not

- Do not store the selected `ArkivOperasjon` in `command_execution`.
- Do not allow `kjorer` to silently become a generic waiting state.
- Do not call a separate `er_ferdig` from application code.

## Phase 4 — Wake-up and deadlock prevention

### Work

1. Ensure all entity fact changes trigger relevant re-evaluation:
   - sak changes
   - journalpost changes
   - dokument changes
2. Implement or replace the current no-op journalpost wake-up path. A blocked `AvsluttSak` must be woken when journalposter become terminal.
3. Ensure `BlockedReason` implies a trigger category:
   - sak created/saksnummer assigned
   - journalpost terminal
   - document rendered/added
   - utsending update
   - saksansvarlig updated
4. Add structured safe logging for decision materialization:
   - command_id
   - command_type
   - status transition
   - blocked_reason/domain_violation category
   - affected sak/journalpost/dokument IDs when safe
5. Consider a periodic safety rescan of `blokkert_venter` commands if wake-up triggers are not yet fully reliable. If deferred, document why.

### Tests

- `blokkert_venter -> klar` when sak is created.
- `blokkert_venter -> klar` when journalpost becomes terminal for `AvsluttSak`.
- `blokkert_venter -> klar` when saksansvarlig matches for `AvsluttSak`.
- blocked commands remain blocked with explicit reason when facts still do not satisfy rules.

## Phase 5 — Archive-operation handlers

### Work

1. Keep handler envelope guards as defense-in-depth, but the domain planner must make mismatches unreachable.
2. Ensure handlers update only factual entity state.
3. Ensure one operation maps to one attempt:
   - operation success -> entity fact transition + re-evaluation
   - recoverable external failure -> `retry_venter`
   - domain/contract violation -> `feil`
4. Move permanent-error details out of entity state if any are currently stored there. Keep diagnostics in command execution status/attempts/logs.

### Tests

- Wrong-envelope operation path is unreachable in domain tests.
- Handler guard still returns a safe internal error if violated.
- HTML-template render mismatch fails the owning command terminally and does not strand sibling commands.

## Phase 6 — Integration and smoke tests

### Required scenarios

- `send-sequence` completes with:
  - `OpprettSak`
  - bytes journalpost
  - HTML-template journalpost
  - `SettSaksansvarlig`
  - `AvsluttSak`
- Journalpost commands can complete before `SettSaksansvarlig`.
- `AvsluttSak` blocks until journalposts are terminal and saksansvarlig matches.
- Two journalpost commands on the same sak do not steal each other's operations.
- Retry of one command does not prevent unrelated ready commands from being selected later.
- No command stays in a non-terminal state without being `klar`, `kjorer`, `retry_venter`, or explicit `blokkert_venter`.

### Manual deployed-test verification

After local tests pass and test environment DB is recreated/migrated:

```text
cargo run -p skuffen-integration-tests --bin skuffen-manual -- ready
cargo run -p skuffen-integration-tests --bin skuffen-manual -- send-sequence
cargo run -p skuffen-integration-tests --bin skuffen-manual -- watch-status <ids...>
```

Expected result: all commands reach `Ok`; report `saksnummer` for archive validation.

## Phase 7 — Verification gates

Run at minimum:

```text
cargo fmt --check
cargo test -p domain
cargo test -p application
cargo test -p infrastructure
cargo test --workspace --exclude skuffen-integration-tests
cargo clippy --all-targets --all-features
cargo run -p adr-fmt -- --lint
```

Run integration tests when local NATS/Postgres are available:

```text
cargo test -p skuffen-integration-tests
```

## Clean-context implementation order

1. Read `AGENTS.md`, `.agent/rules/repo_rules.md`, relevant `.agent/guides/`, and ADR context for `domain`, `application`, `infrastructure`.
2. Read ADRs `SKU-0001`, `SKU-0003`, `SKU-0007`, and stale `SKU-0002` retirement note.
3. Start with failing domain tests for `CommandStateDecision` and no global reconciliation.
4. Implement domain decision API.
5. Remove `oensket_tilstand` from schema/domain/repositories.
6. Update application materialization paths.
7. Harden wake-up.
8. Run verification gates and deployed `send-sequence` smoke.

## Known pitfalls

- Reintroducing global aggregate planning in `planlegg_neste_handling`.
- Treating `Blocked` as generic waiting instead of explicit reason + trigger.
- Leaving `etter_journalpost_endret` as no-op while `AvsluttSak` waits for journalposter.
- Storing `next_operation` in DB and making it stale.
- Letting entity state carry command intent again through renamed desired-state columns.
- Making `AvsluttSak` finish journalpost work instead of waiting for journalpost commands.
- Making journalpost work wait on saksansvarlig.
