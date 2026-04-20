# Plan: Black-Box Integration Tests

**Status**: Ready for implementation  
**Prerequisite**: Entity state machine execution plan (all 5 phases complete)

## Goal

Rewrite integration tests to be pure black-box: spin up Skuffen + Postgres + NATS via testcontainers, send commands via NATS, and assert results by reading JetStream status events and NATS query replies. No direct SQL access from test code.

## Decisions

- **Blocking test** (`avslutt_sak_blokkeres_nar_journalpost_ikke_er_ok`): **Drop** — can't create blocking scenario through external API with instant-success FakeArkivGateway. Blocking behavior is well-tested at domain level (tilstand.rs).
- **Unused fakes**: **Clean up** — remove dead `_command_state_repo`, `_arkiv_gateway`, `_query_repos` parameters from `start_runtime`. Simplify or remove `fakes.rs`.
- **ON CONFLICT bug**: **Fix in same changeset** — fix `id_mapping_postgres.rs` line 328 partial index predicate.
- **System is NOT in production** — no migration concerns.

### Architect Review Findings

- **Entity tilstand not in client contract**: Status events carry command outcome (Ok/Error/Blocked) + context (saksnummer, journalpost_id). Query endpoints return Sikri data (or fake data in test mode). Neither exposes internal tilstand. This is correct — tilstand is an internal implementation detail, not part of the client-facing contract. Domain unit tests cover tilstand transitions exhaustively.
- **Status event reliability**: `wait_for_status_events` uses `DeliverPolicy::All` which reads from stream start — safe even if events arrive before consumer creation.
- **Two-batch race conditions**: None — wait-for-terminal guarantees full persistence before second batch references the entity.
- **Verify during implementation**: Confirm `resolve_execution_context` populates `saksnummer` in OpprettSak terminal Ok events.

## NATS Observable Surface

| Channel | Type | Test Usage |
|---|---|---|
| `arkiv.arkiver` | request/reply | Send command batches |
| `arkiv.arkiver.media` | request/reply | Upload media |
| `arkiv.status.*` (JetStream `arkiv_status`) | events | Wait for status events, assert phases/status/saksnummer |
| `arkiv.command.done.{type}.{id}` (JetStream) | events | Terminal done events |
| `sak.hent` | request/reply | Query sak by ArkivId |
| `journalpost.hent` | request/reply | Query journalpost |

### Status Event Fields (`SkuffenStatusEventV1`)

command_id, correlation_id, phase, status, terminal, error_code, message, attempt, **saksnummer**, **journalpost_id**, **dokument_id**, timestamp.

### Terminal Rules

- `Execution + Ok` → terminal
- `Execution + Error` → terminal
- `Execution + Blocked` → NOT terminal (command can be re-evaluated)
- `Execution + Retrying` → NOT terminal

## Phase 1: Fix ON CONFLICT Bug + Enhance NATS Test Helpers

### 1a. Fix ON CONFLICT bug in production code

File: `src/infrastructure/src/command/adapter/id_mapping_postgres.rs` line 328  
Change: `ON CONFLICT (entity_type, arkiv_id) DO NOTHING` → `ON CONFLICT (entity_type, arkiv_id) WHERE arkiv_id IS NOT NULL DO NOTHING`

### 1b. Add NATS test helpers

In `integration-tests/tests/support/nats.rs`:

- Add `extract_saksnummer(events, command_id)` helper — finds terminal Ok event for a command_id and returns saksnummer
- Add `hent_sak_via_nats_by_arkiv_id(nats_url, saksnummer)` — queries `sak.hent` with `ArkivId`
- Keep existing `wait_for_status_events`, `send_command_batch`, `publish_media`, `hent_sak_via_nats`, `hent_journalpost_via_nats`

### Checkpoint

`cargo check -p skuffen-integration-tests` passes.

## Phase 2: Rewrite Tests to Pure Black-Box

> **Assertion principle**: All assertions target **client-facing data only** — status event phases, status values, terminal flags, saksnummer presence in OpprettSak terminal events, and query response status. Internal entity tilstand is NOT asserted here; it is covered exhaustively by domain unit tests in `tilstand.rs`.

### Test: `command_sequence_opprett_internt_notat_avslutt_sak`

Before:
1. send batch → `wait_for_command_execution_all` (DB) → `wait_for_status_events` → assert events + `fetch_*_state` (DB)

After:
1. send batch → `wait_for_status_events` (terminal) → assert events (all phases present, all terminal Ok)
2. Assert saksnummer present in OpprettSak terminal Ok event
3. No DB state checks — status events carry all client-facing outcome data

### Test: `command_sequence_inngaende_journalpost_flow`

Before:
1. `insert_arkiv_id_mapping` (DB) → send batch → `wait_for_command_execution_all` (DB) → assert events + `fetch_journalpost_state` (DB)

After:
1. Send OpprettSak batch → `wait_for_status_events` (terminal) → assert terminal Ok + extract saksnummer from event
2. Send OpprettInngaaende batch with `ArkivId(saksnummer)` → `wait_for_status_events` (terminal)
3. Assert terminal Ok events for journalpost command (phase, status, terminal flag)

### Test: `command_sequence_utgaaende_journalpost_flow`

Same pattern as inngående. Assert on status event phases/status/terminal only.

### Test: `query_hent_sak_via_nats_uses_id_mapping`

Before:
1. `insert_id_mapping` (DB) → `hent_sak_via_nats(skuffen_id)`

After:
1. Send OpprettSak → `wait_for_status_events` → extract saksnummer
2. `hent_sak_via_nats_by_arkiv_id(saksnummer)` → assert Ok query response status

### Test: `query_hent_journalpost_via_nats`

No DB access currently. No changes needed. Already asserts on query response status only.

### Test: `avslutt_sak_uten_journalposter_er_tillatt`

Before:
1. send batch → `wait_for_command_execution_all` (DB) → `fetch_sak_state` (DB)

After:
1. send batch → `wait_for_status_events` (terminal) → assert both OpprettSak and AvsluttSak get terminal Ok (phase, status, terminal flag)

### Test: `avslutt_sak_med_arkiv_id_fullfoerer_gjennom_hele_flyten`

Before:
1. `insert_arkiv_id_mapping` (DB) → send AvsluttSak → `wait_for_command_execution_all` (DB) → check DB status + DB sak state

After:
1. Send OpprettSak → `wait_for_status_events` → extract saksnummer
2. Send AvsluttSak with `ArkivId(saksnummer)` → `wait_for_status_events` → assert terminal Ok (phase, status, terminal flag)

### Test: `avslutt_sak_blokkeres_nar_journalpost_ikke_er_ok`

**DROP** — remove this test entirely.

### Checkpoint

All tests compile. `cargo check -p skuffen-integration-tests` passes.

## Phase 3: Cleanup

1. **Delete** `integration-tests/tests/support/db.rs` entirely
2. **Remove** `pool: PgPool` field from `TestEnv`
3. **Remove** pool creation from `start_runtime` (keep Postgres container for Skuffen, just don't create test pool)
4. **Simplify** `start_runtime` signature: remove unused `_command_state_repo`, `_arkiv_gateway`, `_query_repos` parameters
5. **Remove or simplify** `fakes.rs` (dead code)
6. **Update** `support/mod.rs` — remove db exports, update start_runtime imports
7. **Update** all test call sites for simplified `start_runtime()`

### Checkpoint

`cargo check -p skuffen-integration-tests` passes. No references to `sqlx`, `PgPool`, or `db::` in test code.

## Phase 4: Run and Fix

1. `cargo test -p skuffen-integration-tests` (requires Docker)
2. Fix any runtime failures
3. `cargo clippy --all-targets --all-features`
4. `cargo fmt --check`

## Files Changed

### Production code
- `src/infrastructure/src/command/adapter/id_mapping_postgres.rs` — ON CONFLICT fix

### Integration tests
- `integration-tests/tests/command_sequence_e2e.rs` — rewrite all tests
- `integration-tests/tests/support/nats.rs` — add helpers
- `integration-tests/tests/support/env.rs` — simplify TestEnv + start_runtime
- `integration-tests/tests/support/mod.rs` — update exports
- `integration-tests/tests/support/db.rs` — DELETE
- `integration-tests/tests/support/fakes.rs` — DELETE or simplify
- `integration-tests/Cargo.toml` — possibly remove sqlx dependency
