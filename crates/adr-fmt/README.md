# adr-fmt

ADR governance tool for cherry-pit. `init` bootstraps project-local vendored
copies; all analysis modes remain read-only. stdout = output, stderr = errors.

Part of the [cherry-pit](../../README.md) workspace.

## Usage

```text
adr-fmt init [TARGET_DIR] [--force]   # bootstrap ADR governance in a Rust project
adr-fmt [ADR_DIR]                     # generated governance reference
adr-fmt --guidelines [ADR_DIR]        # generated governance reference
adr-fmt --lint [ADR_DIR]              # lint all ADRs
adr-fmt --critique <ADR_ID> [ADR_DIR] # focal ADR + transitive closure
adr-fmt --context <CRATE> [ADR_DIR]   # decision rules for a crate
adr-fmt --tree [DOMAIN] [ADR_DIR]     # domain dependency tree
adr-fmt --report [ADR_DIR]            # computed children index
```

Run via `cargo run -p adr-fmt` or `cargo run -p adr-fmt -- <args>`.

Auto-discovers `docs/adr/` by walking up from CWD looking for
`docs/adr/GOVERNANCE.md`. Pass explicit `ADR_DIR` to override.

## Exit Codes

- `0` — Init or analysis complete. **Parse stdout for diagnostics** — exit 0 does not mean "no issues."
- `1` — Infrastructure error (missing config, unknown ADR ID, unknown crate, missing Cargo.toml, existing init config without `--force`).

## Modes

### Init

`adr-fmt init [TARGET_DIR] [--force]` bootstraps ADR governance in a target
Rust project. `TARGET_DIR` defaults to the current directory. It validates that
the target contains `Cargo.toml`, copies the `adr-fmt` source into
`TARGET_DIR/crates/adr-fmt/`, and creates `TARGET_DIR/docs/adr/` scaffold files:
`GOVERNANCE.md`, `TEMPLATE.md`, `adr-fmt.toml`, and `README.md`.

`init` refuses to overwrite an existing `docs/adr/adr-fmt.toml` unless
`--force` is supplied. It does not edit the target `Cargo.toml` or workspace
membership.

After init:

1. Manually register `crates/adr-fmt` as a dev/docs workspace member.
2. Keep release builds app-targeted with `cargo build -p <app>`, package
   allowlists, or `--exclude adr-fmt`.
3. Run `cargo run -p adr-fmt -- --guidelines` to verify the vendored copy.

The vendored copy is project-local development/documentation tooling only, not
a runtime dependency. It is frozen at init time; updates require a future update
flow or an explicit manual re-vendor. This first implementation copies from the
current source checkout via `CARGO_MANIFEST_DIR`; cargo-install self-embedding is
deferred.

If `init` fails after creating some files, inspect the partial scaffold and retry
with `--force` only when you intentionally want to replace existing adr-fmt
files. With `--force`, existing `crates/adr-fmt/` contents are replaced.

### Lint

Runs all rules (template, link, naming, structure) across every ADR.
Diagnostic output format:

```text
## Diagnostics: E error(s), W warning(s) across N ADR(s)

- **severity[RULE_ID]** file:line: message
```

### Critique

`--critique CHE-0042` — BFS transitive closure around focal ADR.
Follows fan-out (forward relationships) and fan-in (reverse/children).
Stale ADRs filtered with count note. Output uses Alternative 4 markdown:
`◆ FOCAL`, `◇ CONNECTED`, `◈ EXCLUDED` headers. Connected blocks sorted
by tier (S→D) then ID.

### Context

`--context cherry-pit-core` — tagged decision rules applicable to a crate.
Resolution: if any ADR in a domain has `Crates:` populated, ADRs without
`Crates:` are still included; only ADRs with a non-matching `Crates:` list
are excluded. If no domain ADR has `Crates:`, all domain ADRs included.
Foundation domains (e.g., COM) always included. Output sorted: foundation
first, then by tier, then ID.
Exits 1 if crate not found in any domain.

### Tree

`--tree [DOMAIN]` — domain tree with ADR listings. Optional domain prefix
filter (e.g., `--tree CHE`). Shows stale counts per domain.

### Report

`--report` — computed children index. Inverts forward relationships to
show `parent ← verb child` entries. Grouped by domain prefix.

### Guidelines

`--guidelines` — complete generated ADR governance reference from rule
catalog, tier/lifecycle definitions, and config. Replaces prescriptive
sections of GOVERNANCE.md.

## SSOT Architecture

| Source | Scope |
|--------|-------|
| `adr-fmt` (code) | Invariant rules: template, naming, vocabulary, lifecycle, links |
| `adr-fmt.toml` | Configurable: domains, crate mappings, rule params, stale directory |
| `--guidelines` output | Generated complete reference |
| `GOVERNANCE.md` | Rationale, process, judgment only |

## Rule Catalog

All rules currently emit warnings. `adr-fmt` is advisory.

### Template (T001–T016)

| Rule | Description |
|------|-------------|
| T001 | H1 title `# PREFIX-NNNN. Title` present |
| T002 | `Date:` field present |
| T003 | `Last-reviewed:` field present (all tiers) |
| T004 | `Tier:` field present |
| T005 | `## Status` section with status line present |
| T006 | Status value recognized; no parenthetical annotations |
| T007 | `## Related` has ≥1 relationship (no orphans, no placeholders) |
| T008 | `## Context` section present |
| T009 | `## Decision` section present |
| T010 | `## Consequences` section present |
| T011 | Code block exceeds 20 lines |
| T012 | Amendment date ≥ creation date; valid ISO 8601 |
| T013 | *(reserved)* |
| T014 | Section order: Status → Related → Context → Decision → Consequences (+ Retirement for stale) |
| T015 | Prose section below minimum word count (configurable `min_words`, default 10) |
| T016 | Decision section lacks tagged rules (`- **RN**: text`) or has non-sequential IDs. Exempt: Draft, Proposed |

### Link (L001–L009)

| Rule | Description |
|------|-------------|
| L001 | Dangling link — target ADR not found in any domain |
| L003 | Supersedes verb without matching `Superseded by` status on target |
| L004 | Cross-domain reference to unmigrated ADR |
| L006 | Legacy relationship verb used |
| L007 | Stale reference — target ADR is in stale archive |
| L008 | Root verb target does not match own ID |
| L009 | Root and References coexist — Root ADRs may not also use References |

### Naming (N001–N004)

| Rule | Description |
|------|-------------|
| N001 | Filename matches `PREFIX-NNNN-kebab-slug.md` (prefix 2–4 uppercase) |
| N002 | Number in filename matches H1 title ID |
| N003 | Slug is valid lowercase kebab-case |
| N004 | Domain prefix not found in configuration |

### Structure (S004–S006)

| Rule | Description |
|------|-------------|
| S004 | Stale ADR missing `## Retirement` section (≥ min_words) |
| S005 | Active ADR has `## Retirement` section (forbidden outside stale/) |
| S006 | Terminal-status ADR not in stale directory |

## ADR Metadata

### Tiers

| Tier | Name | Scope |
|------|------|-------|
| S | Foundational | Design philosophy — reverberates through every crate |
| A | Core | Core trait design — major refactoring across crates |
| B | Behavioural | API semantics — coordinated updates across call sites |
| C | Tooling | DX, build — localized to config or test infra |
| D | Detail | Implementation detail — one crate's internals |

### Lifecycle States

`Draft` → `Proposed` → `Accepted` → `Amended YYYY-MM-DD — note`

Terminal: `Rejected`, `Deprecated`, `Superseded by PREFIX-NNNN`
Terminal states require: move to stale directory + `## Retirement` section.

### Relationship Vocabulary

Three permitted verbs:

| Verb | Meaning |
|------|---------|
| Root | Self-reference (`- Root: OWN-ID`) marking a tree root |
| References | Soft citation — citing another ADR |
| Supersedes | Replaces target entirely; target becomes Superseded |

Constraints: Root + References cannot coexist. Root + Supersedes can.
Legacy verbs (Depends on, Extends, Illustrates, etc.) trigger L006.

### Tagged Rules

Decision sections must contain tagged rules:

```markdown
- **R1**: First rule or decision statement
- **R2**: Second rule or decision statement
```

Global identifier: `PREFIX-NNNN:RN` (e.g., `CHE-0042:R1`).
When no tagged rules found, entire Decision text is captured as R0 (triggers T016).

### Crates Metadata

```markdown
Crates: crate-a, crate-b
```

Placed after Date/Tier, before `## Status`. Used by `--context` mode.

## Configuration

`docs/adr/adr-fmt.toml` — required. Config errors are hard failures (exit 1).

```toml
[stale]
directory = "stale"

[[domains]]
prefix = "CHE"
name = "Cherry Domain"
directory = "cherry"
description = "Architecture decisions"
crates = ["cherry-pit-core"]
foundation = false            # true = included in every --context query

[[rules]]
id = "T015"
category = "template"
description = "Prose section below minimum word count"
internal = false              # true = hidden from user-facing output
[rules.params]
min_words = 10
```

## Workflow

After creating or editing any ADR:

1. `cargo run -p adr-fmt` — parse stdout for warnings
2. Fix reported issues
3. Re-run to confirm clean output
4. Commit
