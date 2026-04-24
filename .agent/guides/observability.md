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

Log level rules:
- `info!`: nominal pipeline progress (message received, dispatched, acknowledged)
- `warn!`: retryable failures, blocked commands awaiting redelivery
- `error!`: irrecoverable failures, terminal command drops, infrastructure errors
- `debug!`: detailed diagnostics (Sikri response parsing, query replies)
- Never log full request/response payloads at `info!` or above
- Never log domain structs that may contain PII at any default level

## Error taxonomy

Use error classification consistently across layers:
- `blocked`: domain rule prevents progress (retry only if external state changes)
- `recoverable`: transient errors (network, 5xx, rate limit)
- `irrecoverable`: invalid input, missing resources, or rule violations

## Trace context propagation

All NATS listeners extract the incoming `traceparent` header into the OpenTelemetry
context using `set_parent_from_nats_headers()` in `telemetry.rs`. This must be
the first statement in each `#[instrument]`-annotated message handler, before any
nested spans or log statements.

The pattern:
1. Inbound: `crate::telemetry::set_parent_from_nats_headers(headers)` — extracts
   OTel context from NATS headers and reparents the current span.
2. Outbound: `crate::telemetry::current_trace_parent()` — injects the current
   trace context as a `traceparent` header on published NATS messages.

This enables end-to-end trace visibility in GCP Trace Explorer: a command
batch flows through `nats.command_batch` → `command.validate` →
`command.register_execution` → `sikri.*` as a single connected trace.

Listeners using this pattern:
- `command_listener` (`nats.command_batch`)
- `validation_listener` (`command.validate`)
- `eksekvering_listener` (`command.register_execution`)
- `media_listener` (`media.assemble`)
- `NatsReplier` (`query.handle`)

## Span coverage

Infrastructure spans (all use `#[tracing::instrument]`):
- NATS listeners: per-message spans with `command_id`, `correlation_id`, `subject`
- Sikri HTTP client: per-function spans (`sikri.get_sak`, `sikri.create_sak`, etc.)
  with safe fields only (`saksnr`, `journalpost_id`, `method`, `url`)
- NATS publishers: per-publish spans with `entity_type`, `subject`

Application layer does not use tracing (per repo rule: no I/O or tracing in
Domain/Application). Application service execution time is visible as the gap
between the listener span start and the first infrastructure child span.

## Error reply sanitization

NATS error replies to callers must not echo internal details:
- Deserialization failures: reply with "Invalid payload format" / "Invalid request format"
- Use case errors: reply with "Internal error"
- Sikri HTTP errors: the `ensure_success` function logs response metadata but does
  not echo response bodies to callers

## Test logging

Suggested environment:
- `RUST_LOG=info` for baseline logging
- Include `RUST_LOG=debug` only for local diagnostics

## Notes

When validating `SakKey::ArkivId`, do not persist state if the sak does not exist.
If ingestion created an ArkivId mapping and validation fails irrecoverably, delete the mapping.
