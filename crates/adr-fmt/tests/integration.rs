//! Integration tests for the adr-fmt binary.
//!
//! Each test creates a self-contained tempdir with the necessary file
//! structure (adr-fmt.toml, domain directories, ADR files) and runs the
//! binary against it.

use std::fs;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

// ── helpers ─────────────────────────────────────────────────────────

/// Minimal adr-fmt.toml for a single-domain test corpus (override-only format).
const MINIMAL_CONFIG: &str = r#"
[stale]
directory = "stale"

[[domains]]
prefix = "TST"
name = "Test Domain"
directory = "test"
description = "Integration test domain."
crates = ["test-core"]

[[rules]]
id = "T015"
params = { min_words = 10 }
"#;

/// Multi-domain config with foundation domain.
const MULTI_DOMAIN_CONFIG: &str = r#"
[stale]
directory = "stale"

[[domains]]
prefix = "COM"
name = "Common"
directory = "common"
description = "Cross-cutting principles."
crates = []
foundation = true

[[domains]]
prefix = "TST"
name = "Test Domain"
directory = "test"
description = "Integration test domain."
crates = ["test-core"]

[[rules]]
id = "T015"
params = { min_words = 10 }
"#;

/// A valid ADR file that passes all rules (root ADR).
const VALID_ADR: &str = "\
# TST-0001. Valid Test ADR

Date: 2026-04-27
Last-reviewed: 2026-04-27
Tier: B
Status: Accepted

## Related

Root: TST-0001

## Context

This ADR documents a valid test case for the integration test suite to verify.

## Decision

R1 [5]: We decided to create a minimal but complete ADR that satisfies all template rules.

## Consequences

The integration test can verify that a clean corpus produces zero diagnostics.
";

/// A second ADR that references TST-0001.
const REFERENCING_ADR: &str = "\
# TST-0002. Referencing ADR

Date: 2026-04-27
Last-reviewed: 2026-04-27
Tier: B
Status: Accepted

## Related

References: TST-0001

## Context

This ADR references TST-0001 to test transitive closure in critique mode.

## Decision

R1 [5]: We reference another ADR to verify critique mode connectivity.

## Consequences

Critique mode should include both TST-0001 and TST-0002 in the closure.
";

/// An ADR with a dangling link to trigger L001.
const DANGLING_LINK_ADR: &str = "\
# TST-0003. Dangling Link ADR

Date: 2026-04-27
Last-reviewed: 2026-04-27
Tier: B
Status: Accepted

## Related

References: TST-9999

## Context

This ADR has a dangling link that should trigger the L001 validation rule.

## Decision

R1 [5]: We reference a non-existent ADR to verify that dangling link detection works.

## Consequences

The linter should report a dangling link warning for TST-9999 in the output.
";

/// ADR without tagged rules (triggers T016).
const NO_TAGGED_RULES_ADR: &str = "\
# TST-0004. No Tagged Rules

Date: 2026-04-27
Last-reviewed: 2026-04-27
Tier: B
Status: Accepted

## Related

Root: TST-0004

## Context

This ADR has prose in the Decision section but no tagged rules.

## Decision

We decided to use plain prose without the required tagged rule format.

## Consequences

The linter should report a T016 warning for missing tagged rules.
";

/// Draft ADR without tagged rules (no longer exempt from T016).
const DRAFT_ADR: &str = "\
# TST-0005. Draft ADR

Date: 2026-04-27
Tier: B
Status: Draft

## Related

Root: TST-0005

## Context

This is a draft ADR that is no longer exempt from the tagged rules requirement.

## Decision

We are still drafting this decision and have not formalized rules yet.

## Consequences

Draft status no longer exempts this ADR from T016 checks.
";

/// ADR with per-ADR Crates field.
const ADR_WITH_CRATES: &str = "\
# TST-0006. ADR With Crates

Date: 2026-04-27
Last-reviewed: 2026-04-27
Tier: B
Crates: test-core, test-api
Status: Accepted

## Related

Root: TST-0006

## Context

This ADR specifies crate applicability via the Crates metadata field.

## Decision

R1 [5]: Crate-specific decisions are scoped to test-core and test-api.

## Consequences

Context mode should only include this ADR when querying test-core or test-api.
";

/// Foundation domain ADR (COM).
const FOUNDATION_ADR: &str = "\
# COM-0001. Foundation Principle

Date: 2026-04-27
Last-reviewed: 2026-04-27
Tier: S
Status: Accepted

## Related

Root: COM-0001

## Context

This is a foundation domain ADR that applies to all crates in the workspace.

## Decision

R1 [5]: Foundation rules apply universally across all domains and crates.

## Consequences

Context mode must always include foundation domain ADRs.
";

/// Stale ADR (superseded, in stale directory).
const STALE_ADR: &str = "\
# TST-0010. Stale ADR

Date: 2026-01-01
Last-reviewed: 2026-01-01
Tier: B
Status: Superseded by TST-0001

## Related

References: TST-0001

## Context

This ADR was superseded and moved to the stale directory.

## Decision

R1 [5]: This decision has been superseded by TST-0001.

## Consequences

This ADR should be included in critique closures (no stale filtering).

## Retirement

Superseded by TST-0001 on 2026-04-27. The newer ADR provides better guidance.
";

/// ADR with non-sequential tagged rule IDs (gap: R1, R3).
const GAP_RULES_ADR: &str = "\
# TST-0007. Gap Rules ADR

Date: 2026-04-27
Last-reviewed: 2026-04-27
Tier: B
Status: Accepted

## Related

Root: TST-0007

## Context

This ADR has tagged rules with a gap in numbering.

## Decision

R1 [5]: First rule is present.
R3 [5]: Third rule skips R2.

## Consequences

The linter should report a T016 warning for non-sequential rule IDs.
";

/// ADR using legacy `## Status` section format (triggers T005c).
const LEGACY_STATUS_ADR: &str = "\
# TST-0011. Legacy Status Format

Date: 2026-04-27
Last-reviewed: 2026-04-27
Tier: B

## Status

Accepted

## Related

Root: TST-0011

## Context

This ADR uses the legacy section format for status instead of the preamble field.

## Decision

R1 [5]: Legacy status section format should produce a T005c migration warning.

## Consequences

The linter should report a T005c warning suggesting migration to preamble format.
";

/// Create test corpus in a tempdir with optional multi-domain support.
///
/// `domains` is a slice of (`domain_directory`, &[(filename, content)]) tuples.
/// `stale_adrs` is an optional slice of (filename, content) for the stale directory.
fn setup_multi_corpus(
    config: &str,
    domains: &[(&str, &[(&str, &str)])],
    stale_adrs: &[(&str, &str)],
) -> TempDir {
    let dir = TempDir::new().expect("create tempdir");
    let adr_root = dir.path().join("docs/adr");

    fs::create_dir_all(&adr_root).expect("create adr root");
    fs::write(adr_root.join("GOVERNANCE.md"), "# Governance\n").expect("write governance");
    fs::write(adr_root.join("adr-fmt.toml"), config).expect("write config");

    for (domain_dir_name, adrs) in domains {
        let domain_dir = adr_root.join(domain_dir_name);
        fs::create_dir_all(&domain_dir).expect("create domain dir");
        for (filename, content) in *adrs {
            fs::write(domain_dir.join(filename), content).expect("write ADR");
        }
    }

    if !stale_adrs.is_empty() {
        let stale_dir = adr_root.join("stale");
        fs::create_dir_all(&stale_dir).expect("create stale dir");
        for (filename, content) in stale_adrs {
            fs::write(stale_dir.join(filename), content).expect("write stale ADR");
        }
    }

    dir
}

/// Create simple single-domain corpus.
fn setup_corpus(config: &str, adrs: &[(&str, &str)]) -> TempDir {
    setup_multi_corpus(config, &[("test", adrs)], &[])
}

fn adr_fmt() -> Command {
    Command::cargo_bin("adr-fmt").expect("binary exists")
}

fn adr_root(dir: &TempDir) -> String {
    dir.path()
        .join("docs/adr")
        .to_str()
        .expect("valid utf-8 path")
        .to_owned()
}

fn setup_rust_project() -> TempDir {
    let dir = TempDir::new().expect("create tempdir");
    fs::write(dir.path().join("Cargo.toml"), "[workspace]\nmembers = []\n")
        .expect("write Cargo.toml");
    dir
}

fn read_target_cargo_toml(dir: &TempDir) -> String {
    fs::read_to_string(dir.path().join("Cargo.toml")).expect("read Cargo.toml")
}

// ── init mode ──────────────────────────────────────────────────────

#[test]
fn init_success_creates_vendored_crate_and_scaffold() {
    let dir = setup_rust_project();

    adr_fmt()
        .args(["init", dir.path().to_str().unwrap()])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("Initialized adr-fmt")
                .and(predicate::str::contains("crates/adr-fmt"))
                .and(predicate::str::contains("docs/adr/adr-fmt.toml"))
                .and(predicate::str::contains(
                    "Manually register `crates/adr-fmt`",
                ))
                .and(predicate::str::contains("not a runtime dependency")),
        );

    assert!(dir.path().join("crates/adr-fmt/Cargo.toml").is_file());
    assert!(dir.path().join("crates/adr-fmt/src/main.rs").is_file());
    assert!(!dir.path().join("crates/adr-fmt/tests").exists());
    assert!(!dir.path().join("crates/adr-fmt/PLAN.md").exists());
    assert!(dir.path().join("docs/adr/GOVERNANCE.md").is_file());
    assert!(dir.path().join("docs/adr/TEMPLATE.md").is_file());
    assert!(dir.path().join("docs/adr/adr-fmt.toml").is_file());
    assert!(dir.path().join("docs/adr/README.md").is_file());

    let vendored_manifest = fs::read_to_string(dir.path().join("crates/adr-fmt/Cargo.toml"))
        .expect("read vendored manifest");
    assert!(vendored_manifest.contains("publish = false"));
    assert!(!vendored_manifest.contains("edition.workspace"));
    assert!(!vendored_manifest.contains("workspace = true"));
}

#[test]
fn second_init_fails_without_force() {
    let dir = setup_rust_project();

    adr_fmt()
        .args(["init", dir.path().to_str().unwrap()])
        .assert()
        .success();

    adr_fmt()
        .args(["init", dir.path().to_str().unwrap()])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("docs/adr/adr-fmt.toml")
                .and(predicate::str::contains("--force")),
        );
}

#[test]
fn init_force_overwrites_existing_config() {
    let dir = setup_rust_project();
    let config_path = dir.path().join("docs/adr/adr-fmt.toml");
    fs::create_dir_all(config_path.parent().unwrap()).expect("create adr dir");
    fs::write(&config_path, "custom = true\n").expect("write existing config");

    adr_fmt()
        .args(["init", dir.path().to_str().unwrap(), "--force"])
        .assert()
        .success();

    let config = fs::read_to_string(config_path).expect("read config");
    assert!(config.contains("adr-fmt configuration"));
    assert!(!config.contains("custom = true"));
}

#[test]
fn init_force_replaces_existing_vendored_crate_with_warning() {
    let dir = setup_rust_project();
    let stale_file = dir.path().join("crates/adr-fmt/obsolete.txt");
    fs::create_dir_all(stale_file.parent().unwrap()).expect("create stale vendored crate");
    fs::write(&stale_file, "stale").expect("write stale file");

    adr_fmt()
        .args(["init", dir.path().to_str().unwrap(), "--force"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "replaced existing `crates/adr-fmt`",
        ));

    assert!(!stale_file.exists());
    assert!(dir.path().join("crates/adr-fmt/src/main.rs").is_file());
}

#[test]
fn init_missing_cargo_toml_fails_before_writing() {
    let dir = TempDir::new().expect("create tempdir");

    adr_fmt()
        .args(["init", dir.path().to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicate::str::contains("missing").and(predicate::str::contains("Cargo.toml")));

    assert!(!dir.path().join("crates/adr-fmt").exists());
    assert!(!dir.path().join("docs/adr").exists());
}

#[test]
fn init_does_not_modify_target_cargo_toml() {
    let dir = setup_rust_project();
    let before = read_target_cargo_toml(&dir);

    adr_fmt()
        .args(["init", dir.path().to_str().unwrap()])
        .assert()
        .success();

    let after = read_target_cargo_toml(&dir);
    assert_eq!(before, after, "init must not edit target Cargo.toml");
}

// ── default mode (guidelines) ──────────────────────────────────────

#[test]
fn default_mode_with_config_shows_governance() {
    let dir = setup_corpus(MINIMAL_CONFIG, &[("TST-0001-valid-test-adr.md", VALID_ADR)]);

    adr_fmt().arg(adr_root(&dir)).assert().success().stdout(
        predicate::str::contains("ADR Governance Reference")
            .and(predicate::str::contains("MODES"))
            .and(predicate::str::contains("TAGGED RULES")),
    );
}

#[test]
fn guidelines_flag_with_config_shows_governance() {
    let dir = setup_corpus(MINIMAL_CONFIG, &[("TST-0001-valid-test-adr.md", VALID_ADR)]);

    adr_fmt()
        .args(["--guidelines", adr_root(&dir).as_str()])
        .assert()
        .success()
        .stdout(predicate::str::contains("ADR Governance Reference"));
}

#[test]
fn report_outputs_children_index() {
    let dir = setup_corpus(
        MINIMAL_CONFIG,
        &[
            ("TST-0001-valid-test-adr.md", VALID_ADR),
            ("TST-0002-referencing-adr.md", REFERENCING_ADR),
        ],
    );

    adr_fmt()
        .args(["--report", adr_root(&dir).as_str()])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("# ADR Children Report")
                .and(predicate::str::contains("TST-0001"))
                .and(predicate::str::contains("References ← TST-0002")),
        );
}

#[test]
fn default_mode_without_config_shows_setup_guide() {
    let dir = TempDir::new().expect("create tempdir");
    let root = dir.path().join("docs/adr");
    fs::create_dir_all(&root).expect("create dir");
    fs::write(root.join("GOVERNANCE.md"), "# Governance\n").expect("write governance");
    // No adr-fmt.toml

    adr_fmt()
        .arg(root.to_str().unwrap())
        .assert()
        .success()
        .stdout(predicate::str::contains("adr-fmt").and(predicate::str::contains("QUICK START")));
}

// ── lint mode ──────────────────────────────────────────────────────

#[test]
fn valid_corpus_clean_output() {
    let dir = setup_corpus(MINIMAL_CONFIG, &[("TST-0001-valid-test-adr.md", VALID_ADR)]);

    adr_fmt()
        .args(["--lint", &adr_root(&dir)])
        .assert()
        .success()
        .stdout(predicate::str::contains("0 error(s), 0 warning(s)"));
}

#[test]
fn dangling_link_produces_l001() {
    let dir = setup_corpus(
        MINIMAL_CONFIG,
        &[
            ("TST-0001-valid-test-adr.md", VALID_ADR),
            ("TST-0003-dangling-link-adr.md", DANGLING_LINK_ADR),
        ],
    );

    adr_fmt()
        .args(["--lint", &adr_root(&dir)])
        .assert()
        .success()
        .stdout(predicate::str::contains("L001"));
}

#[test]
fn empty_domain_directory_graceful() {
    let dir = setup_corpus(MINIMAL_CONFIG, &[]);

    adr_fmt()
        .args(["--lint", &adr_root(&dir)])
        .assert()
        .success()
        .stdout(predicate::str::contains("0 ADR(s)"));
}

#[test]
fn lint_output_on_stdout() {
    let dir = setup_corpus(MINIMAL_CONFIG, &[("TST-0001-valid-test-adr.md", VALID_ADR)]);

    // Verify diagnostics go to stdout
    adr_fmt()
        .args(["--lint", &adr_root(&dir)])
        .assert()
        .success()
        .stdout(predicate::str::contains("Diagnostics"));
}

// ── T016 tagged rules ──────────────────────────────────────────────

#[test]
fn t016_missing_tagged_rules() {
    let dir = setup_corpus(
        MINIMAL_CONFIG,
        &[("TST-0004-no-tagged-rules.md", NO_TAGGED_RULES_ADR)],
    );

    adr_fmt()
        .args(["--lint", &adr_root(&dir)])
        .assert()
        .success()
        .stdout(predicate::str::contains("T016"));
}

#[test]
fn t016_draft_not_exempt() {
    let dir = setup_corpus(MINIMAL_CONFIG, &[("TST-0005-draft-adr.md", DRAFT_ADR)]);

    // Draft ADRs are no longer exempt from T016 — should appear in lint output
    adr_fmt()
        .args(["--lint", &adr_root(&dir)])
        .assert()
        .success()
        .stdout(predicate::str::contains("T016"));
}

#[test]
fn t016_gap_in_rule_ids() {
    let dir = setup_corpus(
        MINIMAL_CONFIG,
        &[("TST-0007-gap-rules-adr.md", GAP_RULES_ADR)],
    );

    adr_fmt()
        .args(["--lint", &adr_root(&dir)])
        .assert()
        .success()
        .stdout(predicate::str::contains("T016"));
}

#[test]
fn t016_tagged_rules_present_no_warning() {
    let dir = setup_corpus(MINIMAL_CONFIG, &[("TST-0001-valid-test-adr.md", VALID_ADR)]);

    // VALID_ADR has tagged rules — no T016
    adr_fmt()
        .args(["--lint", &adr_root(&dir)])
        .assert()
        .success()
        .stdout(predicate::str::contains("T016").not());
}

// ── T005c legacy status section ────────────────────────────────────

#[test]
fn t005c_legacy_status_section() {
    let dir = setup_corpus(
        MINIMAL_CONFIG,
        &[("TST-0011-legacy-status-format.md", LEGACY_STATUS_ADR)],
    );

    adr_fmt()
        .args(["--lint", &adr_root(&dir)])
        .assert()
        .success()
        .stdout(predicate::str::contains("T005c"));
}

#[test]
fn t005c_preamble_status_field_no_warning() {
    let dir = setup_corpus(MINIMAL_CONFIG, &[("TST-0001-valid-test-adr.md", VALID_ADR)]);

    // VALID_ADR uses `Status: Accepted` preamble field — no T005c
    adr_fmt()
        .args(["--lint", &adr_root(&dir)])
        .assert()
        .success()
        .stdout(predicate::str::contains("T005c").not());
}

// ── critique mode ──────────────────────────────────────────────────

#[test]
fn critique_focal_with_connected() {
    let dir = setup_corpus(
        MINIMAL_CONFIG,
        &[
            ("TST-0001-valid-test-adr.md", VALID_ADR),
            ("TST-0002-referencing-adr.md", REFERENCING_ADR),
        ],
    );

    adr_fmt()
        .args(["--critique", "TST-0001", &adr_root(&dir)])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("◆ FOCAL")
                .and(predicate::str::contains("TST-0001"))
                .and(predicate::str::contains("◇ CONNECTED"))
                .and(predicate::str::contains("TST-0002")),
        );
}

#[test]
fn critique_isolated_adr() {
    let dir = setup_corpus(MINIMAL_CONFIG, &[("TST-0001-valid-test-adr.md", VALID_ADR)]);

    // Isolated ADR: focal only, no connected blocks
    adr_fmt()
        .args(["--critique", "TST-0001", &adr_root(&dir)])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("◆ FOCAL").and(predicate::str::contains("◇ CONNECTED").not()),
        );
}

#[test]
fn critique_invalid_id_exits_nonzero() {
    let dir = setup_corpus(MINIMAL_CONFIG, &[("TST-0001-valid-test-adr.md", VALID_ADR)]);

    adr_fmt()
        .args(["--critique", "INVALID", &adr_root(&dir)])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not a valid ADR ID"));
}

#[test]
fn critique_unknown_adr_exits_nonzero() {
    let dir = setup_corpus(MINIMAL_CONFIG, &[("TST-0001-valid-test-adr.md", VALID_ADR)]);

    adr_fmt()
        .args(["--critique", "TST-9999", &adr_root(&dir)])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn critique_includes_stale() {
    let dir = setup_multi_corpus(
        MINIMAL_CONFIG,
        &[("test", &[("TST-0001-valid-test-adr.md", VALID_ADR)])],
        &[("TST-0010-stale-adr.md", STALE_ADR)],
    );

    // Critique TST-0001 which is referenced by stale TST-0010
    // Stale ADRs are now included (no filtering)
    adr_fmt()
        .args(["--critique", "TST-0001", &adr_root(&dir)])
        .assert()
        .success()
        .stdout(predicate::str::contains("◆ FOCAL").and(predicate::str::contains("TST-0010")));
}

#[test]
fn critique_depth_limits_traversal() {
    let dir = setup_corpus(
        MINIMAL_CONFIG,
        &[
            ("TST-0001-valid-test-adr.md", VALID_ADR),
            ("TST-0002-referencing-adr.md", REFERENCING_ADR),
        ],
    );

    // Depth 0: focal only
    adr_fmt()
        .args(["--critique", "TST-0001", "--depth", "0", &adr_root(&dir)])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("◆ FOCAL").and(predicate::str::contains("◇ CONNECTED").not()),
        );
}

// ── context mode ───────────────────────────────────────────────────

#[test]
fn context_shows_crate_rules() {
    let dir = setup_corpus(MINIMAL_CONFIG, &[("TST-0001-valid-test-adr.md", VALID_ADR)]);

    adr_fmt()
        .args(["--context", "test-core", &adr_root(&dir)])
        .assert()
        .success()
        .stdout(predicate::str::contains("test-core").and(predicate::str::contains("TST-0001")));
}

#[test]
fn context_unknown_crate_exits_nonzero() {
    let dir = setup_corpus(MINIMAL_CONFIG, &[("TST-0001-valid-test-adr.md", VALID_ADR)]);

    adr_fmt()
        .args(["--context", "unknown-crate", &adr_root(&dir)])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn context_includes_foundation() {
    let dir = setup_multi_corpus(
        MULTI_DOMAIN_CONFIG,
        &[
            (
                "common",
                &[("COM-0001-foundation-principle.md", FOUNDATION_ADR)],
            ),
            ("test", &[("TST-0001-valid-test-adr.md", VALID_ADR)]),
        ],
        &[],
    );

    // Foundation domain ADRs should always be included
    adr_fmt()
        .args(["--context", "test-core", &adr_root(&dir)])
        .assert()
        .success()
        .stdout(predicate::str::contains("COM-0001").and(predicate::str::contains("TST-0001")));
}

#[test]
fn context_per_adr_crates_filtering() {
    let dir = setup_corpus(
        MINIMAL_CONFIG,
        &[
            ("TST-0001-valid-test-adr.md", VALID_ADR),
            ("TST-0006-adr-with-crates.md", ADR_WITH_CRATES),
        ],
    );

    // TST-0006 has Crates: test-core, test-api — should be included
    adr_fmt()
        .args(["--context", "test-core", &adr_root(&dir)])
        .assert()
        .success()
        .stdout(predicate::str::contains("TST-0006"));
}

// ── context output format (end-to-end) ─────────────────────────────

/// ADR with multi-line tagged rules for end-to-end context output test.
const MULTILINE_RULES_ADR: &str = "\
# TST-0008. Multi-line Rules ADR

Date: 2026-04-27
Last-reviewed: 2026-04-27
Tier: B
Status: Accepted

## Related

Root: TST-0008

## Context

This ADR tests multi-line tagged rule extraction through context mode output.

## Decision

R1 [5]: Use explicit versioning on every event payload
  so that consumers can deserialize historical events
  without schema ambiguity.
R2 [5]: Single-line rule stays on one line.

## Consequences

Multi-line rules should be joined and rendered correctly in context output.
";

/// Draft ADR with tagged rules — must NOT appear in context output.
const DRAFT_WITH_RULES_ADR: &str = "\
# TST-0009. Draft With Rules

Date: 2026-04-27
Tier: B
Status: Draft

## Related

Root: TST-0009

## Context

Draft ADR with tagged rules that should be excluded from context output.

## Decision

R1 [5]: This rule must not leak into context output.

## Consequences

Draft exclusion verified.
";

#[test]
fn context_end_to_end_output_format() {
    // Setup: foundation S-tier + domain B-tier (multi-line rules) + draft (excluded)
    let dir = setup_multi_corpus(
        MULTI_DOMAIN_CONFIG,
        &[
            (
                "common",
                &[("COM-0001-foundation-principle.md", FOUNDATION_ADR)],
            ),
            (
                "test",
                &[
                    ("TST-0008-multiline-rules.md", MULTILINE_RULES_ADR),
                    ("TST-0009-draft-with-rules.md", DRAFT_WITH_RULES_ADR),
                ],
            ),
        ],
        &[],
    );

    let output = adr_fmt()
        .args(["--context", "test-core", &adr_root(&dir)])
        .output()
        .expect("run adr-fmt");

    assert!(output.status.success(), "adr-fmt should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);

    // ── Preamble ──
    assert!(
        stdout.contains("# Architecture Rules"),
        "missing preamble title:\n{stdout}"
    );
    assert!(
        stdout.contains("crate `test-core`"),
        "missing crate name in preamble:\n{stdout}"
    );
    assert!(
        stdout.contains("Follow every rule without exception"),
        "missing mandate in preamble:\n{stdout}"
    );

    // ── Tier headers in correct order ──
    let s_pos = stdout.find("## S-tier").expect("S-tier header missing");
    let b_pos = stdout.find("## B-tier").expect("B-tier header missing");
    assert!(
        s_pos < b_pos,
        "S-tier ({s_pos}) must appear before B-tier ({b_pos})"
    );

    // ── Foundation rule with ID and layer at end ──
    assert!(
        stdout.contains("[COM-0001:R1:L5]"),
        "foundation rule should have ID:layer at end:\n{stdout}"
    );

    // ── Multi-line rule text joined on single line with ID ──
    let r1_line = stdout
        .lines()
        .find(|l| l.contains("[TST-0008:R1:L5]"))
        .expect("R1 line with ID missing");
    assert!(
        r1_line.contains("Use explicit versioning on every event payload"),
        "multi-line R1 start text missing on rule line:\n{r1_line}"
    );
    assert!(
        r1_line.contains("without schema ambiguity."),
        "multi-line R1 continuation text must be on same line as ID:\n{r1_line}"
    );

    // ── Single-line rule ──
    assert!(
        stdout.contains("- Single-line rule stays on one line. [TST-0008:R2:L5]"),
        "single-line R2 format wrong:\n{stdout}"
    );

    // ── Draft exclusion ──
    assert!(
        !stdout.contains("TST-0009"),
        "draft ADR ID must not appear in context output:\n{stdout}"
    );
    assert!(
        !stdout.contains("must not leak"),
        "draft ADR rule text must not appear in context output:\n{stdout}"
    );

    // ── No old metadata noise ──
    assert!(
        !stdout.contains("| Status:"),
        "old status metadata should not appear:\n{stdout}"
    );
    assert!(
        !stdout.contains("| Domain:"),
        "old domain metadata should not appear:\n{stdout}"
    );
}

// ── tree mode ──────────────────────────────────────────────────────

#[test]
fn tree_produces_output() {
    let dir = setup_corpus(MINIMAL_CONFIG, &[("TST-0001-valid-test-adr.md", VALID_ADR)]);

    // Use -- to separate --tree (no domain filter) from positional ADR_DIR
    adr_fmt()
        .args(["--tree", "--", &adr_root(&dir)])
        .assert()
        .success()
        .stdout(predicate::str::contains("TST-0001"));
}

#[test]
fn tree_filtered_by_domain() {
    let dir = setup_multi_corpus(
        MULTI_DOMAIN_CONFIG,
        &[
            (
                "common",
                &[("COM-0001-foundation-principle.md", FOUNDATION_ADR)],
            ),
            ("test", &[("TST-0001-valid-test-adr.md", VALID_ADR)]),
        ],
        &[],
    );

    // Filter to TST domain only
    adr_fmt()
        .args(["--tree", "TST", &adr_root(&dir)])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("TST-0001").and(predicate::str::contains("COM-0001").not()),
        );
}

#[test]
fn tree_unknown_domain_graceful() {
    let dir = setup_corpus(MINIMAL_CONFIG, &[("TST-0001-valid-test-adr.md", VALID_ADR)]);

    adr_fmt()
        .args(["--tree", "NONEXISTENT", &adr_root(&dir)])
        .assert()
        .success()
        .stdout(predicate::str::contains("No domain found"));
}

// ── mutual exclusion ───────────────────────────────────────────────

#[test]
fn critique_and_context_mutually_exclusive() {
    adr_fmt()
        .args(["--critique", "TST-0001", "--context", "test-core"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be used with"));
}

#[test]
fn lint_and_critique_mutually_exclusive() {
    adr_fmt()
        .args(["--lint", "--critique", "TST-0001"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be used with"));
}

#[test]
fn critique_and_tree_mutually_exclusive() {
    adr_fmt()
        .args(["--critique", "TST-0001", "--tree"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be used with"));
}

// ── infrastructure errors ──────────────────────────────────────────

#[test]
fn lint_missing_config_exits_nonzero() {
    let dir = TempDir::new().expect("create tempdir");
    let root = dir.path().join("docs/adr");
    fs::create_dir_all(&root).expect("create dir");

    adr_fmt()
        .args(["--lint", root.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot read"));
}

#[test]
fn help_flag_shows_usage() {
    adr_fmt()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("adr-fmt"));
}

#[test]
fn version_flag_shows_version() {
    adr_fmt()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("adr-fmt"));
}

#[test]
fn invalid_path_exits_nonzero() {
    adr_fmt()
        .arg("/nonexistent/path/to/adr")
        .assert()
        .failure()
        .stderr(predicate::str::contains("is not a directory"));
}

// ── read-only verification ─────────────────────────────────────────

#[test]
fn no_files_modified_after_lint() {
    let dir = setup_corpus(MINIMAL_CONFIG, &[("TST-0001-valid-test-adr.md", VALID_ADR)]);

    // Snapshot directory contents before
    let adr_dir = dir.path().join("docs/adr");
    let before: Vec<_> = walkdir(&adr_dir);

    adr_fmt()
        .args(["--lint", &adr_root(&dir)])
        .assert()
        .success();

    // Verify no new files or modifications
    let after: Vec<_> = walkdir(&adr_dir);
    assert_eq!(before, after, "lint mode should not create or modify files");
}

#[test]
fn no_files_modified_after_critique() {
    let dir = setup_corpus(MINIMAL_CONFIG, &[("TST-0001-valid-test-adr.md", VALID_ADR)]);

    let adr_dir = dir.path().join("docs/adr");
    let before: Vec<_> = walkdir(&adr_dir);

    adr_fmt()
        .args(["--critique", "TST-0001", &adr_root(&dir)])
        .assert()
        .success();

    let after: Vec<_> = walkdir(&adr_dir);
    assert_eq!(
        before, after,
        "critique mode should not create or modify files"
    );
}

/// Recursively list all files under a directory (sorted, relative paths).
fn walkdir(root: &std::path::Path) -> Vec<String> {
    let mut entries = Vec::new();
    walk_recursive(root, root, &mut entries);
    entries.sort();
    entries
}

fn walk_recursive(base: &std::path::Path, dir: &std::path::Path, out: &mut Vec<String>) {
    if let Ok(rd) = fs::read_dir(dir) {
        for entry in rd.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk_recursive(base, &path, out);
            } else {
                let rel = path.strip_prefix(base).unwrap().display().to_string();
                out.push(rel);
            }
        }
    }
}
