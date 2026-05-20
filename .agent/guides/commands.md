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
- `SKUFFEN_HTML2PDF_RENDERER_ENDPOINT=<url>` enables HTML-template rendering through
  the external `html-to-pdf` service. When unset, Skuffen keeps a recoverable
  "renderer not configured" failure so local services can still start. Set this
  in any environment that must process `Dokumentform::HtmlTemplate` commands.
  The endpoint must be the renderer base URL without `/render`, and must use
  `https://` unless `APP_ENV=local`. Skuffen uses the base URL as Cloud Run
  ID-token audience and appends `/render` for the HTTP request.
- `SIKRI_SAKSBEHANDLER_ID` and `SIKRI_SAKSBEHANDLER_ENHET` are required for deployed
  test/dev environments to identify the case handler and unit.

## Deployed test/dev workflow

- `skuffen-manual send-sequence` against deployed test/dev includes both `Bytes`
  documents and one `HtmlTemplate` document by default.
- Environments must have `SKUFFEN_HTML2PDF_RENDERER_ENDPOINT` configured to process
  `HtmlTemplate` documents.
- The `watch-status` subcommand monitors template upload/rendering/status and fails
  hard (non-zero exit) if any tracked command reaches non-`Ok`, times out, or the
  status stream closes/errors before terminal status. Default timeout is 300s.
- Use `--timeout-seconds <SECONDS>` to override the default timeout.

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
