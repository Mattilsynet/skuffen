# adr-fmt Refactor Plan — Idiomatic Rust & ADR Alignment

## Summary

Analysis of the `adr-fmt` crate against idiomatic Rust practices and the ADR corpus
(123 ADRs across 6 domains). Two high-risk ADR compliance violations, nine medium/low
idiomatic Rust deviations, and five test coverage gaps identified.

## High-Risk: ADR Compliance Violations

### A1. AFM-0002 violated — clap dependency

AFM-0002 ("Manual CLI argument parsing", Status: Accepted) explicitly decides:
*"Do not depend on clap or any argument parsing crate."*

Current code uses `clap::Parser` derive macro (`main.rs:37-44`, workspace dependency).
The ADR's reassessment trigger ("surface grows beyond five flags or introduces
subcommands") is arguably met (5 flags + 1 positional), but the ADR is not marked
superseded or amended. This is undocumented architectural drift.

**Resolution**: Supersede AFM-0002 with AFM-0011 documenting clap adoption. The
reassessment trigger was met during the critique/context/index mode additions.

### A2. AFM-0007 violated — diagnostic output destination

AFM-0007 ("Compiler-style diagnostics", Status: Accepted) states:
*"Format all diagnostics in compiler-style on **stderr**"* and
*"Stdout — only `--report` or `--guidelines`. Never diagnostics."*

Current code sends lint diagnostics to stdout via `print!("{}", output::render_diagnostics(...))`
(`main.rs:154-157`). Integration test `lint_output_on_stdout` (line 441) explicitly
asserts `.stderr(predicate::str::is_empty())` — the test enshrines the violation.

Meanwhile `guidelines.rs:37` states *"All output goes to stdout"* — consistent with
code but contradicting AFM-0007.

**Resolution**: Amend AFM-0007 to document the stdout-based Alternative 4 diagnostic
format. The PLAN.md redesign intentionally moved to agent-first stdout output
(all modes emit to stdout, stderr reserved for infrastructure errors only).

## Medium/Low: Idiomatic Rust Issues

### I1. `process::exit(1)` in library functions

`context.rs:31` and `critique.rs:23` call `process::exit(1)` on error paths. Leaks
CLI concerns into domain logic, prevents reuse, makes error paths untestable without
subprocess invocation.

**Fix**: Return `Result<T, CliError>` from both functions. Handle exit in `main()`.

### I2. Regex compiled per-call in `parser.rs`

- `parser.rs:425` — tagged-rule regex `r"^\s*-\s*\*\*R(\d+)\*\*:\s*(.+)"` compiled
  on every `extract_tagged_rules` call. Should be a `LazyLock` static.
- `parser.rs:23-27` and `parser.rs:57-70` — domain-specific regexes are parameterized
  (include prefix). Acceptable, but could be cached.

Inconsistent with `naming.rs:16-22` which correctly uses `LazyLock`.

**Fix**: Hoist the constant-pattern regex to a `LazyLock` static.

### I3. `String`-based errors in config

`config::load` returns `Result<Config, String>` (`config.rs:73`). A typed error enum
would be more idiomatic and composable. Low impact — only one call site.

**Fix**: Optional. Create `ConfigError` enum if error type grows, or accept as-is.

### I4. `guidelines.rs` uses `println!` directly

28 `println!` calls while every other output module returns `String`. The two unit
tests only verify no-panic, not content. Untestable output.

**Fix**: Change to `pub fn render(config: &Config) -> String` using `write!` to a
buffer. `main.rs` calls `print!("{}", guidelines::render(&config))`.

### I5. Domain types in `output.rs`

`CrateRule` (`output.rs:43`) and `HeaderMeta` (`output.rs:18`) are domain model types
co-located with rendering logic. Used by both `output.rs` and `context.rs`.

**Fix**: Move to `model.rs` for better cohesion.

### I6. Missing `Display` impl for `Status`

`Status::short_display()` returns `String` but doesn't implement `Display`.
Inconsistent with `AdrId` and `RelVerb` which both implement `Display`.

**Fix**: Implement `Display for Status` replacing `short_display()`.

### I7. No `TryFrom<&str>` for parse-from-string types

`RelVerb::parse()`, `Tier::parse()`, `Status::parse()` use custom methods.
Standard `TryFrom<&str>` would enable generic parsing patterns.

**Fix**: Optional. The custom methods work fine for the tool's scope.

### I8. Redundant HashMap in `build_relationship_path`

`critique.rs:138` reconstructs `by_id: HashMap<&AdrId, &AdrRecord>` already
constructed at `critique.rs:27`.

**Fix**: Pass existing `by_id` reference as parameter.

### I9. `#[allow(dead_code)]` proliferation

13 instances (11 in `model.rs`, 2 in `report.rs`). Fields marked "reserved for
future": `code_block_count`, `related_has_placeholder`, `decision_content`,
`date_line`, `last_reviewed_line`, `tier_line`, `has_rejection_rationale`,
`is_self_referencing`.

**Fix**: Audit each. Remove fields with no concrete near-term consumer.

## Test Coverage Gaps

| Gap | Location | Impact |
|-----|----------|--------|
| No error-path test for unknown crate | `context.rs:31` | Untested `process::exit` path |
| No error-path test for unknown ADR | `critique.rs:23` | Untested `process::exit` path |
| `guidelines.rs` output not asserted | `guidelines.rs` tests | Cannot detect regressions |
| Transitive path fallback untested | `critique.rs:159` | `"... →"` format never exercised |
| `rules/naming.rs` no integration test | `integration.rs` | N001-N004 only tested in unit |

## Idiomatic Rust — Strengths (Retained)

These patterns are exemplary and should be preserved:

- `#![forbid(unsafe_code)]` — appropriate for a non-FFI CLI tool
- Workspace clippy pedantic lints — enforced project-wide
- Rust 2024 let-chains and let-else — used appropriately throughout
- `LazyLock` for static regex (in `naming.rs`) — correct static init
- `Default` impl on `AdrRecord` — eliminates test helper duplication
- Slice parameters (`&[T]`) over `&Vec<T>` — consistent throughout
- `Display` on `AdrId`, `RelVerb`, `Severity` — enables ergonomic formatting
- Pipeline architecture: `config → parser → rules → output` — clean SRP
- Integration tests with `assert_cmd`/`predicates`/`tempfile`
- Read-only verification tests (`no_files_modified_after_lint/critique`)
- Struct update syntax `..AdrRecord::default()` in all test modules
- `#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]` — minimal derive sets

## Proposed Steps (Priority Order)

1. **Supersede AFM-0002** — Write AFM-0011 justifying clap adoption. Mark AFM-0002
   `Superseded by AFM-0011`.

2. **Amend AFM-0007** — Document stdout-based Alternative 4 diagnostics. All modes
   emit to stdout; stderr reserved for infrastructure errors only.

3. **Refactor `process::exit` out of library functions** — Change `context::context`
   and `critique::critique` to return `Result<T, CliError>`. Handle in `main.rs`.

4. **Hoist static regex in `parser.rs`** — Move tagged-rule regex to `LazyLock`.

5. **Return `String` from `guidelines`** — Align with crate output pattern. Enables
   content assertions in tests.

6. **Move `CrateRule`/`HeaderMeta` to `model.rs`** — Improve cohesion.

7. **Add `Display` for `Status`** — Replace `short_display()` method.

8. **Pass `by_id` to `build_relationship_path`** — Eliminate redundant HashMap.

9. **Audit `#[allow(dead_code)]`** — Remove genuinely unused fields.

10. **Add missing tests** — Error paths (after step 3 enables unit testing), guidelines
    content assertions, naming integration tests, transitive path coverage.

## Open Questions

### Q1. AFM-0002 resolution timing

Supersede now or defer to a separate session? The clap adoption is live and working —
this is documentation catch-up. Writing AFM-0011 requires deciding whether the
reassessment trigger was the right threshold or whether other factors (derive macro
ergonomics, mutual exclusion groups, future growth) better justify the change.

### Q2. AFM-0007 amend vs revert

Amend AFM-0007 to document stdout diagnostics, or revert code to stderr?

Arguments for amend (stdout):
- Agent-first design — stdout enables clean piping to LLM context
- PLAN.md explicitly designed all modes to emit stdout
- `guidelines.rs:37` already documents this pattern
- Integration tests enshrine the behaviour

Arguments for revert (stderr):
- Unix convention: diagnostics/warnings to stderr
- Enables `adr-fmt 2>/dev/null` to suppress warnings
- Consistent with clippy/rustc output model

### Q3. Error type scope

For step 3, options:
- (a) Simple `enum CliError` in `main.rs` — minimal, sufficient for 3 variants
- (b) New `error.rs` module — cleaner if error types grow
- (c) Use `Box<dyn Error>` / `anyhow` — adds dependency, overkill for this tool

Recommendation: (a) — keep in `main.rs` until complexity warrants extraction.

### Q4. Dynamic regex caching

The per-domain regexes in `parse_domain`/`parse_stale` are constructed dynamically
(include domain prefix). Options:
- (a) Accept per-call cost — tool processes ~6 domains, called once per run
- (b) Cache in `HashMap<String, Regex>` passed through parsing pipeline
- (c) Pre-build regexes in `discover_domains` and attach to `DomainDir`

Recommendation: (a) — cost is negligible for 6 domains called once per invocation.

### Q5. Dead code audit aggressiveness

Options:
- (a) Remove all `#[allow(dead_code)]` fields now (strict YAGNI)
- (b) Keep fields with documented justification, remove unjustified
- (c) Keep all — the tool is small and fields are cheap

Recommendation: (b) — remove `code_block_count`, `related_has_placeholder` (parsed
but consumed by no rule and no ADR documents future use). Keep `decision_content`
(used by R0 fallback path even though the field itself is `dead_code` annotated
because it's stored but read only via `decision_rules`).

### Q6. `TryFrom<&str>` adoption

Worth implementing `TryFrom<&str>` for `RelVerb`, `Tier`, `Status`?

Arguments for:
- Standard trait — enables generic parsing patterns
- Better error type than `Option<Self>`

Arguments against:
- Custom `parse()` methods work fine
- `Status::parse` returns `Status::Invalid(...)` rather than `Err` — intentional
  design where "invalid" is a valid variant
- Changing would require updating all call sites

Recommendation: Leave as-is. The `Status::Invalid` variant design intentionally
captures unrecognized input rather than rejecting it — this doesn't map cleanly to
`TryFrom` semantics.
