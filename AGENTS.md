# AGENTS.md
This file is for coding agents working in `skuffen`.
It captures build/test/lint commands and coding conventions for this repository.

## Scope and Source of Truth
- Rust workspace crates:
  - root binary: `skuffen`
  - `src/domain`
  - `src/application`
  - `src/infrastructure`
  - `crates/sikri_client`
  - `integration-tests`
- CI workflow reference: `.github/workflows/validate.yaml`
- Agent rules reference:
  - `.agent/manifest.yaml`
  - `.agent/rules/rust_and_language.md`
  - `.agent/rules/process.md`
  - `.agent/workflows/commands.md`

## Cursor / Copilot Rules
- `.cursorrules`: not present
- `.cursor/rules/`: not present
- `.github/copilot-instructions.md`: not present
- Use this file and `.agent/*` as the operational baseline.

## Toolchain
- Rust toolchain: stable (CI uses stable)
- Formatting: `rustfmt`
- Linting: `clippy`
- Runtime: Tokio
- Integration stack: NATS + Postgres + Sikri adapters

## Build, Lint, and Test Commands
Run from repo root unless noted.

### Fast local iteration
1. `cargo check`
2. `cargo test --workspace --exclude skuffen-integration-tests`
3. `cargo fmt --check`
4. `cargo clippy --all-targets --all-features`

### CI-equivalent commands
- `RUSTFLAGS="-Dwarnings" cargo clippy --all-targets --all-features`
- `cargo fmt --check`
- `cargo test --workspace --exclude skuffen-integration-tests`
- `cargo build --bin skuffen`
- `cargo test -p skuffen-integration-tests -- --nocapture`

### Build and run
- Build workspace: `cargo build --workspace`
- Build service only: `cargo build --bin skuffen`
- Run service: `cargo run --bin skuffen`

### Formatting and linting
- Format in place: `cargo fmt`
- Check formatting: `cargo fmt --check`
- Clippy: `cargo clippy --all-targets --all-features`
- Clippy with warnings denied:
  `RUSTFLAGS="-Dwarnings" cargo clippy --all-targets --all-features`

### Test suites
- All tests: `cargo test --workspace`
- Workspace tests without integration package:
  `cargo test --workspace --exclude skuffen-integration-tests`
- Integration package only:
  `cargo test -p skuffen-integration-tests -- --nocapture`

### Single-test commands (important)
- Single test in `application`:
  `cargo test -p application test_ingest_command_dispatch_failure -- --nocapture`
- Single test in `domain`:
  `cargo test -p domain test_saksnummer -- --nocapture`
- Single integration test function:
  `cargo test -p skuffen-integration-tests --test command_sequence_e2e query_hent_sak_via_nats_uses_id_mapping -- --nocapture`
- By substring when exact name is unknown:
  `cargo test -p application ingest_command -- --nocapture`

### Useful targeted commands
- Check one crate: `cargo check -p infrastructure`
- Test one crate: `cargo test -p application`
- Build one crate: `cargo build -p sikri_client`

## Git Hooks and Safety Checks
- Install hook: `scripts/git-hooks/install.sh`
- Pre-push hook can run:
  - `gitleaks` scan (if installed)
  - forbidden pattern scan (`scripts/git-hooks/forbidden-patterns.txt`)
  - `cargo fmt --check`
  - `cargo check`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo test`
  - `cargo test -p skuffen-integration-tests`

## Code Style Guidelines
Guidelines below align with existing code + `.agent/rules/rust_and_language.md`.

### Architecture and boundaries
- Keep strict layer boundaries:
  - Domain: business rules and invariants; no I/O
  - Application: use-cases + ports/traits; orchestration
  - Infrastructure: NATS/DB/HTTP adapters and external clients
- Do not leak transport, storage, or tracing concerns into Domain/Application.
- Keep domain logic deterministic and side-effect free.

### Naming and language
- Follow Rust naming:
  - `snake_case` for modules/functions/variables
  - `PascalCase` for types/traits/enums
  - `SCREAMING_SNAKE_CASE` for constants
- Follow repo Norwenglish convention:
  - Norwegian names for domain language (`Sak`, `Journalpost`, `Saksnummer`)
  - English names for technical constructs and standard Rust APIs
- Keep public domain vocabulary consistent with existing DTO/domain terms.

### Imports
- Prefer explicit imports; avoid wildcard imports.
- Keep import groups readable:
  1. std/core
  2. third-party crates
  3. local crate imports (`crate::...`)
- Remove unused imports quickly.

### Types and domain modeling
- Prefer strong value objects over raw primitives when invariants exist.
- Validate at construction boundaries (`new`, `TryFrom`, `FromStr`).
- Model state with enums instead of string status fields.
- Use UUIDs consistently for command ids, client references, and internal ids.

### Error handling
- Use `Result<T, E>` and propagate with `?`.
- Use `anyhow` at orchestration boundaries (application/infrastructure).
- Use `thiserror` for reusable typed domain/application errors.
- Add context via `anyhow::Context` around adapter/service failures.
- Preserve command failure semantics:
  - Recoverable -> retrying flow
  - Irrecoverable -> terminal error flow
  - Blocked -> unmet domain preconditions
- Avoid `unwrap()` in production paths; use only in tests or proven invariants.

### Async and side effects
- Use Tokio idioms consistently (`async fn`, `#[tokio::test]`, `tokio::join!`).
- Keep side effects in infrastructure listeners/adapters.
- Design command ingestion/execution to be idempotent.

### Formatting and readability
- Run `cargo fmt` before finalizing.
- Keep functions focused; extract helpers when logic grows.
- Prefer self-explanatory names over comments.
- Add comments only for non-obvious invariants or behavior.

### Logging and observability
- Use structured tracing in infrastructure boundaries.
- Include correlation ids / command ids where useful.
- Do not log secrets, credentials, or sensitive payloads.

### Testing conventions
- Use `#[tokio::test]` for async tests.
- Name tests by behavior/outcome.
- Follow Arrange / Act / Assert pattern used in current tests.
- Prefer fakes/mocks in application tests; keep end-to-end behavior in integration tests.

### Security and config hygiene
- Never hardcode secrets, tokens, or internal project ids.
- Use env vars and `.env` for local config; keep real values out of git.
- Keep examples/docs on placeholders.

## Agent Workflow Recommendations
- Before coding:
  1. read this file
  2. scan `.agent/rules/*`
  3. inspect neighboring modules for local patterns
- Before finalizing:
  1. run formatting + linting for touched scope
  2. run at least one targeted crate test
  3. run integration tests when touching command flow or infrastructure
