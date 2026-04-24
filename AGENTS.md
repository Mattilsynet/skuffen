# AGENTS.md
This file is the start point for coding agents working in `skuffen`.
Treat `.agent/` as the deeper source of truth for repo rules, technical guidance, and domain knowledge.

## Read Order
1. Read `AGENTS.md`.
2. Read `.agent/rules/repo_rules.md` for normative repo rules.
3. Read the relevant technical guides in `.agent/guides/`.
4. Read `.agent/skills/arkivfag/SKILL.md` for canonical archive-domain knowledge.
5. Use `.agent/assets/diagram_text/INDEX.md` when SVG diagrams are hard to read in CLI.

## Source Of Truth
- Entry point: `AGENTS.md`
- Normative repo rules: `.agent/rules/repo_rules.md`
- Technical guidance: `.agent/guides/`
- Canonical archive-domain knowledge: `.agent/skills/arkivfag/`
- Machine-readable index: `.agent/manifest.yaml`
- CI reference: `.github/workflows/validate.yaml`

## Important Constraint
- `.agent/skills/arkivfag/` is the canonical source of truth for archive-domain integration rules.
- Do not rewrite or reinterpret that content when editing agent docs.

## Workspace Scope
- root binary: `skuffen`
- `src/domain`
- `src/application`
- `src/infrastructure`
- `crates/sikri_client`
- `integration-tests`

## Toolchain
- Rust toolchain: stable
- Formatting: `rustfmt`
- Linting: `clippy`
- Runtime: Tokio
- Integration stack: NATS + Postgres + Sikri adapters

## Command Reference
- Full command guide: `.agent/guides/commands.md`
- Fast local iteration:
  - `cargo check`
  - `cargo test --workspace --exclude skuffen-integration-tests`
  - `cargo fmt --check`
  - `cargo clippy --all-targets --all-features`

## Finalizing Expectations
- Run formatting and linting for the touched scope.
- Run at least one targeted crate test.
- Run integration tests when touching command flow or infrastructure:
  - `cargo test -p skuffen-integration-tests` (requires local NATS + Postgres).
  - See `.agent/guides/commands.md` for full setup.
- Update relevant documentation (decision docs, guides, AGENTS.md, domain skill resources) when changes affect behavior, contracts, architecture, or conventions. Documentation is part of done, not follow-up polish.

## Cursor / Copilot Rules
- `.cursorrules`: not present
- `.cursor/rules/`: not present
- `.github/copilot-instructions.md`: not present
