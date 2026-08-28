---
description: Kjør vanlige utviklingskommandoer
---

Denne guiden beskriver trygge, vanlige bygg-, test- og kvalitetssjekker for `skuffen`.

## Fast local iteration

1. Sjekk kode
   `cargo check`
2. Kjør workspace-tester uten integration package
   `cargo test --workspace --exclude skuffen-integration-tests`
3. Sjekk formatting
   `cargo fmt --check`
4. Kjør clippy
   `cargo clippy --all-targets --all-features`

## CI-equivalent commands

- `RUSTFLAGS="-Dwarnings" cargo clippy --all-targets --all-features`
- `cargo fmt --check`
- `cargo test --workspace --exclude skuffen-integration-tests`
- `cargo build --bin skuffen`
- `cargo test -p skuffen-integration-tests -- --nocapture`

## Build and run

- Build workspace: `cargo build --workspace`
- Build service only: `cargo build --bin skuffen`
- Run service: `cargo run --bin skuffen`

## Runtime configuration

- `SKUFFEN_FAKE_SIKRI=1` uses fake Sikri adapters for local development.
- `SKUFFEN_FAKE_SIKRI_FEIL=irrecoverable|recoverable` makes the fake archive fail
  every call with that classification. Only read when `SKUFFEN_FAKE_SIKRI=1`, which
  is itself restricted to `APP_ENV` local/dev/test. Used by the integration test
  that pins terminal `feilet` versus `retry_venter` (SKU-0017).
- `SKUFFEN_HTML2PDF_RENDERER_ENDPOINT=<url>` enables HTML-template rendering through
  the external `html-to-pdf` service. When unset, Skuffen keeps a recoverable
  "renderer not configured" failure so local services can still start. Set this
  in any environment that must process `Dokumentform::HtmlTemplate` commands.
  The endpoint must be the renderer base URL without `/render`, and must use
  `https://` unless `APP_ENV=local`. Skuffen uses the base URL as Cloud Run
  ID-token audience and appends `/render` for the HTTP request.
- `SIKRI_SAKSBEHANDLER_ID` and `SIKRI_SAKSBEHANDLER_ENHET` are required for deployed
  test/dev environments to identify the case handler and unit.

## Sikri error diagnostics

- Skuffen does not log Sikri response bodies for successful 2xx responses.
- For Sikri 4xx/5xx responses, the raw Sikri error-body is logged ONLY at `debug!`
  level, never at `error!`/`info!`. Everywhere else Skuffen uses safe codes
  (`safe_detail_for_http_error`, with `sikri_unknown_error` as fallback) so operators
  can still classify upstream validation and code-set failures without exposing raw
  bodies at default log levels.
- Transport and parse failures follow the same split. `reqwest::Error` renders the
  full URL including query parameters — which carry saksnummer — so the raw error
  goes to `debug!` only. The `error!` line carries `sikri_transport_arsak`
  (`timeout`, `connect`, `decode`, …), which is the part you actually need to tell a
  dead Sikri from a slow one.
- Raw Sikri error-body must never reach NATS replies, public status events or
  `operasjon.siste_detalj`. `SikriFeil` enforces this by construction: `kode` is a
  static safe code and `melding` is a pre-mapped user-facing text.
- `operasjon.siste_detalj` holds the stable code, optionally followed by an internal
  detail for errors that originate inside Skuffen (sqlx and similar) where nothing
  else logs them. Archive errors carry no such detail — `sikri_client` has already
  logged status, endpoint and body.
- Skuffen still does not log request payloads, authorization headers, or secrets.
- Note: the previous risk acceptance permitting full 4xx/5xx error-body logging at
  `error!`/`info!` is withdrawn in favor of the `debug!`-only + safe-code policy above.

## Deployed test/dev workflow

- `skuffen-manual send-sequence` submits a 9-command sequence covering two complete
  sak lifecycles: one regular sak (with `Bytes` and `HtmlTemplate` journalposts) and
  one shielded sak (with a shielded internal-note journalpost). Shielded objects use
  title syntax `[|Ola Norrmann|]` with environment-specific default values
  `tilgangskode=UO` and `tilgangshjemmel=Offl. § 23 tredje ledd`. The tool does
  not validate those code-set values before sending; it prints a warning to stderr
  before side effects. Confirm the shielding values exist in the target Sikri
  environment before running against real archive data.
- Environments must have `SKUFFEN_HTML2PDF_RENDERER_ENDPOINT` configured to process
  `HtmlTemplate` documents.
- The `watch-status` subcommand monitors all tracked command IDs and fails hard
  (non-zero exit) if any tracked command reaches non-`Ok`, times out, or the
  status stream closes/errors before terminal status. Default timeout is 30s total
  for the full watch run, not per command ID.
- Use `--timeout-seconds <SECONDS>` to override the total timeout.

## Test suites

- All tests: `cargo test --workspace`
- Workspace tests without integration package:
  `cargo test --workspace --exclude skuffen-integration-tests`
- Integration package only:
  `cargo test -p skuffen-integration-tests -- --nocapture`

## Single-test commands

- Single test in `application`:
  `cargo test -p application test_ingest_command_dispatch_failure -- --nocapture`
- Single test in `domain`:
  `cargo test -p domain test_saksnummer -- --nocapture`
- Single integration test function:
  `cargo test -p skuffen-integration-tests --test command_sequence_e2e query_hent_sak_via_nats_uses_id_mapping -- --nocapture`
- By substring when exact name is unknown:
  `cargo test -p application ingest_command -- --nocapture`

## Admin read over NATS

Admin read er to eksakte core request-reply-subjects. `utfort_av` er obligatorisk
selvdeklarert attribusjon, ikke autentisering.

```bash
nats request arkiv.admin.read.command.hent \
  '{"utfort_av":"test-operator","command_id":"00000000-0000-0000-0000-000000000001"}'
```

```bash
nats request arkiv.admin.read.sak.hent \
  '{"utfort_av":"test-operator","key":{"type":"clientReference","value":"00000000-0000-0000-0000-000000000002"}}'
```

`key` støtter `skuffenId`, `clientReference` og `arkivId`. Svarene bruker
`NatsResponse<T>`; stabile feilmeldinger er `Invalid request format`,
`Command not found`, `Sak not found`, `Response too large` og `Internal error`.

## Useful targeted commands

- Check one crate: `cargo check -p infrastructure`
- Test one crate: `cargo test -p application`
- Build one crate: `cargo build -p sikri_client`

## Git hooks and safety checks

- Install hook: `scripts/git-hooks/install.sh`
- Pre-push hook can run:
  - `gitleaks` scan (if installed)
  - forbidden pattern scan (`scripts/git-hooks/forbidden-patterns.txt`)
  - `cargo fmt --check`
  - `cargo check`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo test`
  - `cargo test -p skuffen-integration-tests`
