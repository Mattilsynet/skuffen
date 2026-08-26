# Observability

## Pipeline overview

Subjects and flows:
- `arkiv.arkiver` (NATS core): receives command batch, reply required as `ArkiveringKvittering` (`Ok.command_ids` or `Error.message`)
- `arkiv.request.sak.hent` (NATS core): read/query request-reply for sak
- `arkiv.request.journalpost.hent` (NATS core): read/query request-reply for journalpost
- `arkiv.request.bruker.mt_enheter` (NATS core): live read/query stub returning `Not implemented`
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
- Raw Sikri error-body logges KUN på `debug!`-nivå, aldri på `info!`/`error!`; ellers brukes safe code (`sikri_unknown_error` fallback fra `safe_detail_for_http_error`)
- Rå Sikri error-body skal aldri til NATS replies, public status events, `operasjon.siste_detalj`
- Bounded safe error messages (koder + Norwegian user messages) er greit i internal logs for debugging og monitoring
- Never log request payloads sent to external systems at `info!`/`error!`, because they may contain client-submitted sensitive data
- PII in structured domain/command types may be rendered via `Debug` at `debug!` level only. `debug!` is off by default in prod, so this is acceptable; the hard rule is that raw external response text and request payloads must never appear at `info!`/`error!`

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

## Cloud Logging command outcomes

Cloud Logging list rows use scan-friendly command outcome headlines while retaining structured `event="command_execution_outcome"`. This enables operators to quickly scan command execution results in logs while preserving structured filtering capability.

## Render diagnostics

Application-rendered document failures propagate stable safe detail codes:
- `render_dokument_mangler` — Target document missing from the command facts
- `render_journalpost_mangler` — Target journalpost missing from the command facts
- `render_utilgjengelig` — Renderer temporarily unavailable (recoverable)
- `render_avvist` — Renderer rejected the document (irrecoverable)
- `render_mal_mangler` — Referenced HTML template does not exist
- `render_mal_substitusjon_feilet` — Template token substitution error
- `intern_mal_utilgjengelig` — Template storage layer unavailable (recoverable)
- `intern_lagring_av_rendret_dokument_feilet` — Rendered document persist failed (recoverable)

Mapping failures in the Sikri adapter use the `arkivmapping_` prefix
(`arkivmapping_mottaker_mangler`, `arkivmapping_ufullstendig_skjerming`,
`arkivmapping_postadresse_mangler`). These are our own mapping errors, always
irrecoverable, and the client-facing message carries the `client_reference` so the
caller knows which document or correspondence party to fix.

Failures originating inside Skuffen use the `intern_` prefix, which makes them easy to
separate from `sikri_` codes in a log query.

These codes appear in `operasjon.siste_detalj` and structured logs. They are safe to
surface in dashboards and alerts.

## Media store logs

Media store operations emit structured events with `media_get_*` and `media_save_*` event types, including:
- `media_id` — unique media identifier
- `operation` — get or save action
- `byte_len` — content size in bytes (when available)
- `content_type` — MIME type of media content
- `origin/source ids` — upstream arkiv system identifiers

## Safety constraints

Never logged:
- HTML/PDF document contents or generated payloads
- Authorization headers, tokens, or secrets
- Request payloads sent to external systems
- Secrets, credentials, or PII

Acceptable and encouraged in internal logs:
- Safe error codes and bounded status details from external responses when useful for debugging and monitoring
- Raw Sikri error-body only at `debug!` level; never at `info!`/`error!`, and never echoed to NATS replies, public status events, `operasjon.siste_detalj`
- `command_id` and `correlation_id` wherever command context is available

The public outward status is safe but no longer static: it carries the mapped, sanitised message for the failure, so the client learns what went wrong. Internal detail — sqlx errors, raw Sikri bodies — stays in logs and `operasjon.siste_detalj`, which are for operators only.

## Error reply sanitization

NATS error replies to callers must not echo internal details:
- Deserialization failures: reply with "Invalid payload format" / "Invalid request format"
- Use case errors: reply with "Internal error"
- Sikri HTTP errors: the `ensure_success` function logs response metadata but does
  not echo response bodies to callers. Raw Sikri error-body is logged only at `debug!`
  level; safe codes (`safe_detail_for_http_error`, `sikri_unknown_error` fallback) are
  used everywhere else and are the only Sikri error detail allowed on NATS replies,
  public status events, `operasjon.siste_detalj`

This section governs what is echoed back to callers over NATS. Internal logs may include useful external response error messages as long as request payloads, HTML/PDF contents, secrets, and auth material are not logged.

## Status-event safety contract

Public status projections use `outward_message` as the external/client-facing message.
Dynamic `detail` is internal diagnostic context and must not be relied on as safe outward text.
Application lifecycle helpers that accept dynamic detail must set a static, safe `outward_message`,
especially validation blocked/retrying/error and execution outcomes. This prevents infrastructure
fallback from exposing diagnostics if outward messages are omitted.

## HTML2PDF renderer logging

Internal HTML2PDF renderer logs include:
- HTTP status code/class (2xx/4xx/5xx)
- Safe endpoint/audience labels
- Content-type and content-length headers
- Error category classification
- Bounded external response error messages for `text/plain` and `application/json` responses (truncated and redacted)

Renderer authentication uses a Cloud Run metadata-server OIDC identity token with the configured base endpoint URL as audience. The HTTP request appends `/render`. Logs may include the safe audience label and token acquisition category, but never the token value.

Never logged:
- HTML payloads or request bodies
- HTML/PDF response bodies from the renderer, because they may echo request content or generated content
- Authorization headers or tokens
- PDF bytes or generated content
- Secrets, credentials, or PII

The public outward status is safe but no longer static: it carries the mapped, sanitised message for the failure, so the client learns what went wrong. Internal detail — sqlx errors, raw Sikri bodies — stays in logs and `operasjon.siste_detalj`, which are for operators only.

## Test logging

Suggested environment:
- `RUST_LOG=info` for baseline logging
- Include `RUST_LOG=debug` only for local diagnostics

## Notes

When validating `SakKey::ArkivId`, do not persist state if the sak does not exist.
If ingestion created an ArkivId mapping and validation fails irrecoverably, delete the mapping.

## Sikri safe error detail codes

`safe_detail_for_http_error` in `crates/sikri_client/src/error_mapping.rs` maps Sikri HTTP responses to stable, safe `&'static str` codes with no PII, URLs, or raw response body text. `SikriFeil` pairs each code with its classification and a pre-mapped user-facing message. The codes appear in `operasjon.siste_detalj` and in structured logs, and are stable identifiers safe to surface in dashboards and alerts.

Terminal failure requires a positive match (SKU-0017): the classifier's floor is
`Recoverable`, so an unmapped error retries until someone adds a rule for it.

| Code | Meaning | Recoverability |
|---|---|---|
| `sikri_unknown_user` | Saksbehandler/systembruker not found in ePhorte | Irrecoverable |
| `sikri_access_control_rejected` | Tilgangskode/tilgangshjemmel rejected | Irrecoverable |
| `sikri_validation_failed` | Sikri rejected the content as invalid | Irrecoverable |
| `sikri_missing_document_content` | Journalpost document files have no content | Irrecoverable |
| `sikri_resource_not_found` | HTTP 404 from Sikri | Irrecoverable |
| `sikri_request_validation_failed` | Local pre-flight validation rejected the payload | Irrecoverable |
| `sikri_upstream_unavailable` | 502 Bad Gateway, or no response at all | Recoverable |
| `sikri_rate_limited` | HTTP 429 — Sikri throttling | Recoverable |
| `sikri_upstream_error` | Generic 5xx from Sikri | Recoverable |
| `sikri_invalid_request` | Generic 4xx client error | Recoverable |
| `sikri_secret_unavailable` | Credentials could not be read from Secret Manager | Recoverable |
| `sikri_response_unparsable` | 2xx in a shape we do not recognise | Recoverable |
| `sikri_unknown_error` | Unclassified error | Recoverable |

`sikri_invalid_request` is deliberately recoverable. `401`/`403` most often mean a rotated
credential rather than a bad request, and terminating is irreversible while retrying is not.
`ALLE_SIKRI_KODER` lists every producible code; the adapters have a coverage test that fails
if a code has no client-facing error code.

The companion `user_message_for_http_error` function returns a Norwegian human-readable message that is forwarded to the client on the status stream. It shares the same classification logic. Neither function echoes raw Sikri response bodies or user identifiers.

Logging policy: raw Sikri error-body is logged only at `debug!` level (never `error!`/`info!`). The same split applies to transport and parse failures, whose `reqwest::Error` renders the full URL including query parameters; the `error!` line carries `sikri_transport_arsak` instead. Everywhere else the safe code applies, with `sikri_unknown_error` as fallback. Raw bodies must never reach NATS replies, public status events, or `operasjon.siste_detalj`. Raw error bodies may contain sensitive archive details; the previous risk acceptance permitting full 4xx/5xx body logging on `error!`/`info!` is withdrawn.

## NATS server URL redaction

`safe_nats_server_label` in `src/infrastructure/src/nats/config.rs` strips credentials (user/password or token in the URL authority) before logging the NATS server address. Only `scheme://host:port` is logged; inline secrets are never emitted to logs or spans.

Example: `nats://user:secret@nats.example.invalid:4222` → logged as `nats://nats.example.invalid:4222`.
