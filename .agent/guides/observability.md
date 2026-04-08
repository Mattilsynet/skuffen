# Observability

## Pipeline overview

Subjects and flows:
- `arkiv.arkiver` (NATS core): receives command batch, reply required
- `arkiv.command.inbox.<entity>.<command_id>` (JetStream): ingested commands
- `arkiv.command.ready.<entity>.<command_id>` (JetStream): validated commands ready for execution
- `arkiv.command.done.<entity>.<command_id>` (JetStream): terminal execution result
- `arkiv.status.<commandId>` (JetStream): status events from validation + execution

Durability and availability:
- `arkiv_command_inbox`, `arkiv_command_ready`, `arkiv_command_done`, `arkiv_status` and `arkiv_media` are configured with `num_replicas = 3`.
- Durable consumers `validator` and `executor` use explicit ack and `num_replicas = 3`.
- `validation_listener` and `eksekvering_listener` run in restart loops that recreate stream/consumer state after NATS disruptions.
- `command_listener` and `media_listener` are intake-critical and will crash the process after exhausting a restart budget, so Cloud Run can replace the instance.

Database state:
- `id_mapping`: client_reference -> skuffen_id (+ optional arkiv_id)
- `sak_state`: skuffen_id keyed sak state
- `journalpost_state`: journalpost state (FK -> sak_state)
- `dokument_state`: dokument state (FK -> journalpost_state)
- `command_execution`: execution lifecycle state

## Logging principles

Infrastructure logs should include:
- `command_id` and `correlation_id` when present
- `subject` and `reply_subject` (if applicable)
- `entity_type` and `sak_key` for journalpost commands
- validation outcome + failure cause
- execution outcome + failure cause

Application/domain errors should provide:
- classification: `blocked` | `recoverable` | `irrecoverable`
- short error code (stable identifier)
- human-readable message

## Error taxonomy

Use error classification consistently across layers:
- `blocked`: domain rule prevents progress (retry only if external state changes)
- `recoverable`: transient errors (network, 5xx, rate limit)
- `irrecoverable`: invalid input, missing resources, or rule violations

## Test logging

Suggested environment:
- `RUST_LOG=info` for baseline logging
- Include `RUST_LOG=debug` only for local diagnostics

## Notes

When validating `SakKey::ArkivId`, do not persist state if the sak does not exist.
If ingestion created an ArkivId mapping and validation fails irrecoverably, delete the mapping.
