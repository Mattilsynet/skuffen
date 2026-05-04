//! Project-local adr-fmt vendoring support.
//!
//! `init` is the only write path in adr-fmt. It bootstraps a Rust target
//! project with a vendored copy of this crate and ADR governance scaffolding.

use std::error::Error;
use std::fmt::{self, Write as _};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const README_CONTENT: &str = "# Architecture Decision Records

This directory contains the project's Architecture Decision Records (ADRs).

## Tooling

This project uses a project-local vendored copy of `adr-fmt` in
`crates/adr-fmt/`. The vendored copy is development/documentation tooling only;
do not add it as an application runtime dependency.

Register `crates/adr-fmt` manually as a dev/docs workspace member if you want to
run it through Cargo workspace commands. Keep release builds app-targeted with
`cargo build -p <app>`, package allowlists, or `--exclude adr-fmt`.

Run ADR tooling with:

```bash
cargo run -p adr-fmt -- --guidelines
cargo run -p adr-fmt -- --lint docs/adr
```

The vendored copy is frozen at init time. Updates require a future update flow
or an explicit manual re-vendor.

## Troubleshooting

If `init` fails after creating some files, inspect the partial scaffold and retry
with `--force` when you intentionally want to replace existing adr-fmt files.
";

const VENDORED_CARGO_TOML: &str = r#"[package]
name = "adr-fmt"
version = "0.1.0"
description = "Project-local ADR governance CLI"
edition = "2024"
rust-version = "1.91"
license = "MIT"
publish = false

[dependencies]
clap = { version = "4", features = ["derive"] }
regex = "1"
serde = { version = "1", features = ["derive"] }
toml = "1"

[dev-dependencies]
assert_cmd = "2"
predicates = "3"
tempfile = "3"
"#;

const SEED_TEMPLATE: &str = r#"# PREFIX-NNNN. Title

Date: YYYY-MM-DD
Last-reviewed: YYYY-MM-DD
Tier: S|A|B|C|D
Status: Draft | Proposed | Accepted | Rejected | Deprecated | Superseded by PREFIX-NNNN

## Related

Root: PREFIX-NNNN | References: PREFIX-NNNN | Supersedes: PREFIX-NNNN

## Context

Describe the problem, relevant constraints, and alternatives considered.

## Decision

Summarize the chosen approach.

R1 [5]: State one concrete, self-contained rule for agents to follow.

## Consequences

Describe the trade-offs, impact, and what becomes easier or harder.
"#;

const SEED_CONFIG: &str = r#"# adr-fmt configuration
#
# Domain definitions, crate mappings, and stale directory config.
# Rules are hardcoded in the binary — only parameter overrides go here.

[stale]
directory = "stale"

[[domains]]
prefix = "COM"
name = "Common Domain"
directory = "common"
description = "Cross-cutting architecture and design principles."
crates = []
foundation = true

[[domains]]
prefix = "RST"
name = "Rust Domain"
directory = "rust"
description = "Rust language, toolchain, dependency, and release-build governance."
crates = []
foundation = true

[[domains]]
prefix = "AFM"
name = "ADR Tooling Domain"
directory = "adr-fmt"
description = "Project-local ADR governance tooling and agent documentation workflow decisions."
crates = ["adr-fmt"]

[[rules]]
id = "T015"
params = { min_words = 7, max_words = 100 }

[[rules]]
id = "T016"
params = { max_rules = 10, min_rule_words = 7, max_rule_words = 60 }
"#;

const SEED_GOVERNANCE: &str = "# ADR Governance

This directory is governed by the project-local `adr-fmt` binary vendored in
`crates/adr-fmt/`. The binary and `adr-fmt.toml` are the source of truth for
invariant ADR rules and configurable domain mappings.

Use `cargo run -p adr-fmt -- --guidelines` for the generated governance
reference. Keep this document focused on project-specific rationale, process,
and judgment that cannot be enforced mechanically.

## Development-only tooling

`adr-fmt` is documentation tooling only. Keep application release builds
targeted to runtime packages with `cargo build -p <app>`, package allowlists,
or `--exclude adr-fmt` in workspace/container build commands.
";

#[derive(Debug)]
pub(crate) enum InitError {
    NotFound {
        path: PathBuf,
    },
    NotDirectory {
        path: PathBuf,
    },
    MissingCargoToml {
        path: PathBuf,
    },
    ExistingConfig {
        path: PathBuf,
    },
    SourceUnavailable {
        path: PathBuf,
    },
    Io {
        action: &'static str,
        path: PathBuf,
        source: io::Error,
    },
}

impl fmt::Display for InitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { path } => {
                write!(f, "target directory does not exist: {}", path.display())
            }
            Self::NotDirectory { path } => {
                write!(f, "target is not a directory: {}", path.display())
            }
            Self::MissingCargoToml { path } => write!(
                f,
                "target is not a Rust project: missing {}",
                path.display()
            ),
            Self::ExistingConfig { path } => write!(
                f,
                "{} already exists; use --force to overwrite",
                path.display()
            ),
            Self::SourceUnavailable { path } => write!(
                f,
                "cannot locate adr-fmt source checkout at {}; this init implementation must run from the source checkout",
                path.display()
            ),
            Self::Io {
                action,
                path,
                source,
            } => {
                write!(f, "cannot {action} {}: {source}", path.display())
            }
        }
    }
}

impl Error for InitError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub(crate) struct InitReport {
    target_dir: PathBuf,
    written_paths: Vec<PathBuf>,
    replaced_existing_crate: bool,
}

impl InitReport {
    pub(crate) fn render(&self) -> String {
        let mut out = String::new();
        let _ = write!(
            out,
            "Initialized adr-fmt in {}\n\n",
            self.target_dir.display()
        );
        out.push_str("Created or updated:\n");
        for path in &self.written_paths {
            let _ = writeln!(out, "- {}", path.display());
        }
        if self.replaced_existing_crate {
            out.push_str("\nWarning: replaced existing `crates/adr-fmt` contents because `--force` was supplied.\n");
        }
        out.push_str("\nNext steps:\n");
        out.push_str(
            "1. Manually register `crates/adr-fmt` as a dev/docs workspace member in Cargo.toml.\n",
        );
        out.push_str("2. Keep release builds app-targeted: `cargo build -p <app>`, package allowlists, or `--exclude adr-fmt`.\n");
        out.push_str(
            "3. Run `cargo run -p adr-fmt -- --guidelines` to verify the vendored copy.\n\n",
        );
        out.push_str("Note: `adr-fmt` is project-local development/documentation tooling only, not a runtime dependency.\n");
        out.push_str("Note: this implementation copies from the current source checkout via CARGO_MANIFEST_DIR; cargo-install self-embedding is deferred.\n");
        out.push_str("Troubleshooting: if init fails after partial writes, inspect the scaffold and retry with `--force` only when replacement is intentional.\n");
        out
    }
}

pub(crate) fn init_project(target_dir: &Path, force: bool) -> Result<InitReport, InitError> {
    validate_target(target_dir)?;

    let adr_config = target_dir.join("docs/adr/adr-fmt.toml");
    if adr_config.exists() && !force {
        return Err(InitError::ExistingConfig { path: adr_config });
    }

    let source_crate = source_crate_dir()?;
    let vendored_crate = target_dir.join("crates/adr-fmt");
    let adr_root = target_dir.join("docs/adr");
    let replaced_existing_crate = force && vendored_crate.exists();

    let mut written_paths = Vec::new();
    if replaced_existing_crate {
        fs::remove_dir_all(&vendored_crate).map_err(|source_error| InitError::Io {
            action: "remove directory",
            path: vendored_crate.clone(),
            source: source_error,
        })?;
    }
    copy_vendored_crate(&source_crate, &vendored_crate, &mut written_paths)?;
    write_scaffold(&adr_root, &mut written_paths)?;

    Ok(InitReport {
        target_dir: target_dir.to_path_buf(),
        written_paths,
        replaced_existing_crate,
    })
}

fn validate_target(target_dir: &Path) -> Result<(), InitError> {
    if !target_dir.exists() {
        return Err(InitError::NotFound {
            path: target_dir.to_path_buf(),
        });
    }
    if !target_dir.is_dir() {
        return Err(InitError::NotDirectory {
            path: target_dir.to_path_buf(),
        });
    }

    let cargo_toml = target_dir.join("Cargo.toml");
    if !cargo_toml.is_file() {
        return Err(InitError::MissingCargoToml { path: cargo_toml });
    }

    Ok(())
}

fn source_crate_dir() -> Result<PathBuf, InitError> {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if source.join("src/main.rs").is_file() {
        Ok(source)
    } else {
        Err(InitError::SourceUnavailable { path: source })
    }
}

fn write_scaffold(adr_root: &Path, written_paths: &mut Vec<PathBuf>) -> Result<(), InitError> {
    create_dir_all(adr_root)?;
    write_file(
        &adr_root.join("GOVERNANCE.md"),
        SEED_GOVERNANCE.as_bytes(),
        written_paths,
    )?;
    write_file(
        &adr_root.join("TEMPLATE.md"),
        SEED_TEMPLATE.as_bytes(),
        written_paths,
    )?;
    write_file(
        &adr_root.join("adr-fmt.toml"),
        SEED_CONFIG.as_bytes(),
        written_paths,
    )?;
    write_file(
        &adr_root.join("README.md"),
        README_CONTENT.as_bytes(),
        written_paths,
    )?;
    for domain in ["common", "rust", "adr-fmt", "stale"] {
        create_dir_all(&adr_root.join(domain))?;
        written_paths.push(adr_root.join(domain));
    }
    Ok(())
}

fn copy_vendored_crate(
    source: &Path,
    dest: &Path,
    written_paths: &mut Vec<PathBuf>,
) -> Result<(), InitError> {
    create_dir_all(dest)?;
    written_paths.push(dest.to_path_buf());

    write_file(
        &dest.join("Cargo.toml"),
        VENDORED_CARGO_TOML.as_bytes(),
        written_paths,
    )?;
    copy_file(
        &source.join("README.md"),
        &dest.join("README.md"),
        written_paths,
    )?;
    copy_dir(&source.join("src"), &dest.join("src"), written_paths)?;

    Ok(())
}

fn copy_dir(source: &Path, dest: &Path, written_paths: &mut Vec<PathBuf>) -> Result<(), InitError> {
    create_dir_all(dest)?;
    written_paths.push(dest.to_path_buf());

    let entries = fs::read_dir(source).map_err(|source_error| InitError::Io {
        action: "read directory",
        path: source.to_path_buf(),
        source: source_error,
    })?;

    for entry in entries {
        let entry = entry.map_err(|source_error| InitError::Io {
            action: "read directory entry in",
            path: source.to_path_buf(),
            source: source_error,
        })?;
        let entry_path = entry.path();
        let file_name = entry.file_name();
        let target_path = dest.join(&file_name);
        let file_type = entry.file_type().map_err(|source_error| InitError::Io {
            action: "inspect",
            path: entry_path.clone(),
            source: source_error,
        })?;
        if file_type.is_dir() {
            copy_dir(&entry_path, &target_path, written_paths)?;
        } else if file_type.is_file() {
            copy_file(&entry_path, &target_path, written_paths)?;
        }
    }

    Ok(())
}

fn copy_file(
    source: &Path,
    dest: &Path,
    written_paths: &mut Vec<PathBuf>,
) -> Result<(), InitError> {
    let bytes = fs::read(source).map_err(|source_error| InitError::Io {
        action: "read",
        path: source.to_path_buf(),
        source: source_error,
    })?;
    write_file(dest, &bytes, written_paths)
}

fn write_file(
    dest: &Path,
    bytes: &[u8],
    written_paths: &mut Vec<PathBuf>,
) -> Result<(), InitError> {
    if let Some(parent) = dest.parent() {
        create_dir_all(parent)?;
    }
    fs::write(dest, bytes).map_err(|source_error| InitError::Io {
        action: "write",
        path: dest.to_path_buf(),
        source: source_error,
    })?;
    written_paths.push(dest.to_path_buf());
    Ok(())
}

fn create_dir_all(path: &Path) -> Result<(), InitError> {
    fs::create_dir_all(path).map_err(|source_error| InitError::Io {
        action: "create directory",
        path: path.to_path_buf(),
        source: source_error,
    })
}
