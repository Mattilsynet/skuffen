# Observability

## Pipeline overview

Subjects and flows:
- `arkiv.arkiver` (NATS core): receives command batch, reply required as `ArkiveringKvittering` (`Ok.command_ids` or `Error.message`)
- `arkiv.arkiver.media.begin` (NATS core): starts a media upload session
- `arkiv.arkiver.media.receiver.<receiver_id>.session.<session_id>.chunk.<index>` and `.commit` (NATS core): receiver-bound media upload traffic
- `arkiv.request.sak.hent` (NATS core): read/query request-reply for sak
- `arkiv.request.journalpost.hent` (NATS core): read/query request-reply for journalpost
- `arkiv.request.bruker.mt_enheter` (NATS core): live read/query stub returning `Not implemented`
- `arkiv.admin.read.command.hent` (NATS core): admin read request-reply for one command and its current operasjon rows
- `arkiv.admin.read.sak.hent` (NATS core): admin read request-reply for one sak with materialised local state
- `arkiv.command.inbox.<entity>.<command_id>` (JetStream): ingested commands
- `arkiv.command.ready.<entity>.<command_id>` (JetStream): validated commands ready for execution
- `arkiv.command.done.<entity>.<command_id>` (JetStream): terminal execution result
- `arkiv.status.<commandId>` (JetStream): status events from validation + execution

Durability and availability:
- `arkiv_command_inbox`, `arkiv_command_ready`, `arkiv_command_done`, `arkiv_status` and `arkiv_media` are configured with `num_replicas = 3`.
- Durable consumers `validator` and `executor` use explicit ack and `num_replicas = 3`.
- `validation_listener` and `eksekvering_listener` run in restart loops that recreate stream/consumer state after NATS disruptions.
- `admin_listener` is degradable. It queue-subscribes to the two exact admin subjects with the stable queue groups `skuffen-admin-read-command-hent-v1` and `skuffen-admin-read-sak-hent-v1`, so only one instance answers during deploy overlap. Both subscriptions run under one `TaskSupervisor::background`; a subscription that ends returns `Err` so the supervisor restarts both.
- Shutdown is cancellation-aware end to end: supervisor backoff waits on the shutdown token, and the root runtime gives tasks eight seconds to finish before aborting the rest. That keeps us inside Cloud Run's ten seconds.
- `command_listener` and `media_listener` are intake-critical. The command listener has a bounded restart budget; the session-based media server remains critical without an outer restart loop, so a stopped server crashes the process and Cloud Run can replace the instance. During shutdown it stops begin intake and keeps receiver sessions available for a five-second grace period.

Database state:
- `entitet`: master for `skuffen_id` (client_reference / arkiv_id)
- `command`: mottaksjournal, holder `correlation_id` og dispatch-milepælene
- `sak_tilstand`, `journalpost_tilstand`, `dokument_tilstand`: materialiserte fakta
- `operasjon`, `operasjon_forsok`: eksekveringstilstand og forsøkshistorikk

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

Message lines must identify their subject. Cloud Logging's list view shows only
`message`, so a milestone that reads `operasjon utført` is unreadable when a
batch runs many operations at once. The discriminator — command type,
operasjonstype, status event, media id, count — goes into the message text as
well as into the structured field: `operasjon utført: opprett_journalpost
(forsøk 1)`. Only bounded enum codes and ids belong there; the payload rules
below are unchanged.

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

## Log–trace correlation

`telemetry/cloud_logging.rs` is our own Cloud Logging event formatter. It exists
because `tracing-stackdriver` pins `opentelemetry 0.22`, which cannot coexist
with the `0.32` series this workspace uses, so its `with_cloud_trace` support is
unavailable to us.

Every log line carries:
- `logging.googleapis.com/trace` — `projects/<project>/traces/<trace_id>`
- `logging.googleapis.com/spanId` and `logging.googleapis.com/trace_sampled`
- `severity`, `time`, `message`, `target`, `span`
- all span fields, flattened into the payload

Flattening is what makes `jsonPayload.command_id = "..."` find everything that
happened to one command, regardless of which layer emitted it.

The trace fields require `GOOGLE_CLOUD_PROJECT` (or `APP_APPLICATION__PROJECT_ID`).
Without it the log line still carries the raw trace id, but Cloud Logging cannot
resolve it to a trace.

Filters are per layer: `RUST_LOG` governs logs, `SKUFFEN_TRACE_FILTER` governs
spans. Otherwise `RUST_LOG=warn` would silently remove the spans the logs
correlate against.

## Identity fields

| Field | Scope | Use |
|---|---|---|
| `correlation_id` | The client's own key, e.g. a bekymringsmelding | The whole course of events, across commands and across traces |
| `command_id` | One command | Everything that happened to one request |
| `operasjon_id` | One operation | One unit of archive work and its attempts |

`correlation_id` is persisted on the `command` row and read back via
`hent_command_metadata`, so it is available during execution even though the
message is long gone. UUIDs are always logged as plain strings — never as
`Some(...)` — so a log search on the value matches.

## Trace context propagation

Trace context travels in NATS headers while a message exists:

```
arkiv.arkiver → arkiv_command_inbox → arkiv_command_ready → dekomponering (ack)
```

After decomposition the message is gone and execution is driven by polling
Postgres. There is no trace context to continue, so each operation attempt gets
its own trace, tagged with the identity fields above. This is deliberate: a
retry may happen a day later, and a trace that spans a day is not a useful
trace.

The pattern, inbound:

```rust
let span = tracing::info_span!("command.validate", ...);
crate::telemetry::set_parent_on_span_from_nats_headers(&span, message.headers.as_ref());
self.handle_message(message).instrument(span).await
```

The order matters. `tracing-opentelemetry` builds the OTel span when the span is
entered, and `set_parent` after that point returns `AlreadyStarted` and is
discarded. Calling it from inside an `#[instrument]` body therefore breaks the
trace silently. `telemetry/mod.rs` has a regression test for both directions.

Outbound: `crate::telemetry::trace_headers()` returns the headers for the
current span, or `None` when there is no span. Only trace context is
propagated. Baggage is deliberately excluded: nothing in Skuffen sets or reads
it, so carrying it would only make the service a relay for caller-controlled
key-value pairs into our own subjects. The business key is `correlation_id`,
which also survives ack, restart and a day of retries. `installer_propagator`
is shared with the tests so they lock the configuration production runs. It reads the context from the
span, not from `opentelemetry::Context::current()` — `tracing-opentelemetry`
never attaches to the global context, so that one is always empty.

Handlers using this pattern:
- `command_listener` (`nats.command_batch`)
- `validation_listener` (`nats.validate`)
- `dekomponering_listener` (`nats.dekomponer`)
- `admin_listener` (`admin.read`)
- `NatsReplier` (`query.handle`)

## Admin read attribution logging

Admin read requests carry a mandatory self-declared `utfort_av`. It is
attribution, not an authenticated audit log, and it is never stored.

One structured `info!` line is written per valid request, after the publish
result is known:
- `admin_action`: `read.command.hent` or `read.sak.hent`
- `utfort_av`: trimmed operator identifier
- `key_type` and `lookup`: UUID key values may be logged; a raw `ArkivId` is not,
  so `key_type = "arkiv_id"` is logged without its value
- `resultat`: `ok` | `not_found` | `error` | `response_too_large`

Request and response payloads, `siste_detalj`, titles, correspondence parties,
addresses and document metadata are never logged. `utfort_av` is the only
human-identifying field permitted at `info!`, and the listener rejects blank
values, control characters and anything over 128 UTF-8 bytes.

The `lib-nats` media server owns its own protocol handlers and is not included.

## Span coverage

Infrastructure owns the transport boundary; application owns the unit of work.

| Span | Layer | Boundary |
|---|---|---|
| `nats.command_batch`, `nats.validate`, `nats.dekomponer`, `admin.read`, `query.handle` | infrastructure | One inbound message |
| `command.ingest`, `command.validate`, `command.dekomponer` | application | One command |
| `operasjon.utfor` | application | One operation attempt |
| `operasjon.evaluer` | application | One evaluation pass |
| `sikri.*`, `nats.publish.*`, `sak.hent` | infrastructure | One outbound call |

`tracing` is permitted in application per SKU-0019. `domain` stays free of it —
`domain/Cargo.toml` has no tracing dependency, which makes that boundary
compile-enforced.

`#[instrument]` must always carry `skip_all` and name its fields explicitly
(SKU-0019 R2). Without it the macro records every argument via `Debug`, and in
application the arguments are command payloads. `scripts/sjekk-instrument-skip-all.sh`
enforces this in CI; it handles multi-line attributes and bare `#[instrument]`.

What you then record is governed by the logging policy below, unchanged: no
external request payloads at `info!`/`error!`, PII from domain and command types
at `debug!` only.

## Milestone logs

The nominal path is logged at `info!`, so a search on one id yields a readable
sequence without reading NATS. Each milestone is emitted where the fact is
known, and only once.

| Event | Emitted by |
|---|---|
| `kommandobatch mottatt og videresendt: <n> kommandoer` | `command_listener`, with `command_count` and `command_ids` |
| `kommando mottatt og dispatchet: <command_type>` / `allerede dispatchet` | `IngestCommandService` |
| `kommando validert: <command_type>` / `avvist` / `venter på ny levering` | `ValidateCommandService`, with `error_code` and `arsak` |
| `kommando dekomponert: <command_type> til <n> operasjoner` | `DekomponerCommandService`, with `nye_operasjoner` |
| `operasjon blokkert: <operasjonstype>` / `allerede utført` / `er ugyldig` | `EksekverOperasjonService`, with `grunn` |
| `operasjon utført: <operasjonstype> (forsøk <n>)` / `venter, poller igjen` / `feilet terminalt` | `EksekverOperasjonService`, with `attempt_no` and `kode` |
| `executor overtok lederskapet` | `OperasjonWorker`, with the recovery counts |
| `kommandostatus publisert: <command_type> <hendelse>` / `operasjonstatus publisert: <operasjonstype> <hendelse>` | `NatsStatusPublisher`, mirroring the status stream |

Each milestone keeps its old wording as a prefix, so a log filter written
against the bare label still matches by `contains` — but an exact-equality or
anchored-regex filter does not.


The blocked and polling paths matter most: they write to the database without
publishing status, so the log is the only place their reason becomes visible.

## Archive identifiers always belong in the log

SKU-0015 R11: `saksnummer`, `journalpost_id`, `client_reference`, `command_id`,
`operasjon_id` and `correlation_id` are what make a log line correlatable to a
case, and a log you cannot correlate has no operational value. All six are either
Skuffen's own UUIDs or the archive's own references — `client_reference` is a
`Uuid` in the wire contract, so none of them can carry client free text.

Log them as **structured fields** so Cloud Logging can query them under a
consistent name. The message text may echo one when it is what identifies the
line — `media get ok: <uuid> 89698 bytes` pairs with its own start line — but
the field is what makes it findable, and free text is never a substitute for it:

```rust
error!(saksnummer = %saksnummer, "fant ikke Skuffen-id for arkiv-id");   // yes
anyhow!("Skuffen ID ikke funnet for arkiv_id: {}", saksnummer)           // no
```

What stays forbidden on `info!`/`error!` is unchanged: personal data (names,
national identity numbers, addresses, correspondence parties, document content)
and raw external response text. The reason `sikri_client` avoids `reqwest::Error`
`Display` is that the URL may carry credentials in query parameters — not that it
carries a case number. Redact the credentials, log the case number.

## Shutdown

The OTLP batch exporter buffers. `vent_paa_nedstengingssignal` calls
`shutdown_telemetry()` after signalling shutdown, because Cloud Run tears the
container down shortly after SIGTERM and the last batch is usually the one that
explains why the service stopped.

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
(`arkivmapping_mottaker_mangler`, `arkivmapping_postadresse_mangler`). These are
our own mapping errors, always irrecoverable, and the client-facing message
carries the `client_reference` so the caller knows which document or
correspondence party to fix.

`arkivmapping_ufullstendig_skjerming` finnes ikke lenger: `Tilgang` er en enum
der halv skjerming ikke er representerbar (SKU-0015 R10), så feilen kan ikke
oppstå.

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

## URL redaction

`trygg_url_etikett` in `src/infrastructure/src/url_etikett.rs` keeps `scheme://host:port`
and discards everything else — user, password, token, path and query. Two call sites use it:

- `safe_nats_server_label` (`src/infrastructure/src/nats/config.rs`) for the NATS server address
- telemetry startup logging for `OTEL_EXPORTER_OTLP_ENDPOINT`

Both URLs come from configuration and may carry credentials in the authority. The
OTLP exporter's own build error renders the full URL, so it goes to `debug!` only —
the same split as for Sikri transport errors. Only `scheme://host:port` is logged; inline secrets are never emitted to logs or spans.

Example: `nats://user:secret@nats.example.invalid:4222` → logged as `nats://nats.example.invalid:4222`.
