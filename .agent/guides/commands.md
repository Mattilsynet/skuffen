---
description: Kjor vanlige utviklingskommandoer
---

Denne guiden beskriver trygge, vanlige bygg-, test- og kvalitetssjekker for `skuffen`.

## Fast local iteration

1. Sjekk kode
   `cargo check`
2. Kjor workspace-tester uten integration package
   `cargo test --workspace --exclude skuffen-integration-tests`
3. Sjekk formatting
   `cargo fmt --check`
4. Kjor clippy
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
