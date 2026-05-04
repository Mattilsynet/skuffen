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
- Architecture decision records: `docs/adr/` (governed by `adr-fmt`)
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

## Architecture Decision Records (ADRs)

All durable architecture decisions live in `docs/adr/` and are governed by the project-local `adr-fmt` tool.
`.agent/decisions/` is no longer used — decisions belong in `docs/adr/`.

**Key files:**
- `docs/adr/GOVERNANCE.md` — Skuffen-specific process and judgment (read first)
- `docs/adr/TEMPLATE.md` — canonical ADR template
- `docs/adr/common/` — COM domain: cross-cutting foundation decisions
- `docs/adr/skuffen/` — SKU domain: Skuffen-specific decisions
- `docs/adr/stale/` — superseded ADRs (retired, not deleted)

**adr-fmt commands (always use project-local binary):**
```
cargo run -p adr-fmt -- --guidelines          # authoritative authoring rules
cargo run -p adr-fmt -- --lint                # diagnostics (advisory; warnings do not block)
cargo run -p adr-fmt -- --context <crate>     # applicable rules for a given crate
cargo run -p adr-fmt -- --critique <ADR_ID>   # focal ADR + direct neighbors
cargo run -p adr-fmt -- --tree [DOMAIN]       # domain tree overview
cargo run -p adr-fmt -- --report              # reverse-link / children index
```

**Lint is advisory.** `--lint` warnings (e.g. T015 word-count) inform quality but do not block commits or CI.
Errors (exit 1) from `--lint` indicate infrastructure failure and should be resolved before finalizing.

**When to run adr-fmt:**
- Before editing or superseding an ADR: `--critique <ADR_ID>`
- After writing or editing any ADR: `--lint`
- When planning work against a crate: `--context <crate>`

## Cursor / Copilot Rules
- `.cursorrules`: not present
- `.cursor/rules/`: not present
- `.github/copilot-instructions.md`: not present
