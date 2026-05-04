//! Diagnostic reporting for ADR validation.

use std::fmt;
use std::path::Path;

/// Diagnostic severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Error variant is matched in output rendering but never constructed yet
pub enum Severity {
    Error,
    Warning,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Error => write!(f, "error"),
            Self::Warning => write!(f, "warning"),
        }
    }
}

/// A single diagnostic message attached to a file and optional line.
#[derive(Debug)]
pub struct Diagnostic {
    pub severity: Severity,
    pub rule: &'static str,
    pub file: String,
    pub line: usize,
    pub message: String,
    /// Internal diagnostics are not shown to users.
    pub internal: bool,
}

impl Diagnostic {
    pub fn warning(rule: &'static str, file: &Path, line: usize, message: String) -> Self {
        Self {
            severity: Severity::Warning,
            rule,
            file: file.display().to_string(),
            line,
            message,
            internal: false,
        }
    }
}
