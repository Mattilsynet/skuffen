//! Configuration loading from `adr-fmt.toml`.
//!
//! The config file defines domain mappings, stale directory, and optional
//! rule parameter overrides. Rules themselves are hardcoded in the binary.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

/// Top-level configuration.
#[derive(Debug, Deserialize)]
pub struct Config {
    pub stale: StaleConfig,
    pub domains: Vec<DomainConfig>,
    /// Optional rule overrides. If present with full declarations (legacy
    /// format), a deprecation warning is emitted to stderr.
    #[serde(default)]
    pub rules: Vec<RuleConfig>,
}

/// Stale archive configuration.
#[derive(Debug, Deserialize)]
pub struct StaleConfig {
    pub directory: String,
}

/// Domain definition.
#[derive(Debug, Deserialize)]
pub struct DomainConfig {
    pub prefix: String,
    pub name: String,
    pub directory: String,
    pub description: String,
    pub crates: Vec<String>,
    /// Foundation domains are included with every domain query.
    #[serde(default)]
    pub foundation: bool,
}

/// Rule override entry. Only `id` is required; other fields are optional
/// and used only for parameter overrides or disabling rules.
#[derive(Debug, Deserialize)]
pub struct RuleConfig {
    pub id: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub description: String,
    /// Optional rule parameters (e.g., `min_words = 7`).
    #[serde(default)]
    pub params: HashMap<String, toml::Value>,
}

impl Config {
    /// Look up a rule parameter by rule ID and key.
    ///
    /// Returns `None` if the rule or key does not exist.
    pub fn rule_param_u64(&self, rule_id: &str, key: &str) -> Option<u64> {
        self.rules
            .iter()
            .find(|r| r.id == rule_id)
            .and_then(|r| r.params.get(key))
            .and_then(toml::Value::as_integer)
            .and_then(|v| u64::try_from(v).ok())
    }
}

/// Load configuration from `adr-fmt.toml` in the ADR root directory.
///
/// Returns an error string if the file is missing or malformed.
pub fn load(adr_root: &Path) -> Result<Config, String> {
    let config_path = adr_root.join("adr-fmt.toml");

    let content = std::fs::read_to_string(&config_path).map_err(|e| {
        format!(
            "cannot read {}: {e}\n       adr-fmt.toml is required",
            config_path.display()
        )
    })?;

    let config: Config = toml::from_str(&content)
        .map_err(|e| format!("failed to parse {}: {e}", config_path.display()))?;

    // Deprecation warning for legacy full rule declarations
    emit_legacy_rule_warnings(&config);

    Ok(config)
}

/// Try to load configuration, returning None if the file does not exist.
/// Used by guidelines mode to distinguish "no config" from "bad config".
pub fn try_load(adr_root: &Path) -> Result<Option<Config>, String> {
    let config_path = adr_root.join("adr-fmt.toml");

    if !config_path.is_file() {
        return Ok(None);
    }

    load(adr_root).map(Some)
}

/// Emit deprecation warnings if config contains legacy full rule declarations.
///
/// Legacy format: rules with `category` and `description` fields populated.
/// New format: only `id` and optional `params` for overrides.
fn emit_legacy_rule_warnings(config: &Config) {
    let legacy_count = config
        .rules
        .iter()
        .filter(|r| !r.category.is_empty() && !r.description.is_empty())
        .count();

    if legacy_count > 0 {
        eprintln!("warning: adr-fmt.toml contains {legacy_count} legacy rule declaration(s)");
        eprintln!("         Rules are now hardcoded in the binary. Only parameter overrides");
        eprintln!("         are needed in config. Remove `category` and `description` fields.");
        eprintln!("         Example override: [[rules]]");
        eprintln!("         id = \"T015\"");
        eprintln!("         params = {{ min_words = 7, max_words = 50 }}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_config_no_rules() {
        let toml_str = r#"
[stale]
directory = "stale"

[[domains]]
prefix = "CHE"
name = "Cherry"
directory = "cherry"
description = "Test domain"
crates = ["cherry-pit-core"]
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.stale.directory, "stale");
        assert_eq!(config.domains.len(), 1);
        assert_eq!(config.domains[0].prefix, "CHE");
        assert_eq!(config.domains[0].crates, vec!["cherry-pit-core"]);
        assert!(config.rules.is_empty());
    }

    #[test]
    fn parse_config_with_overrides() {
        let toml_str = r#"
[stale]
directory = "stale"

[[domains]]
prefix = "CHE"
name = "Cherry"
directory = "cherry"
description = "Test"
crates = []

[[rules]]
id = "T015"
params = { min_words = 7, max_words = 50 }
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.rules.len(), 1);
        assert_eq!(config.rules[0].id, "T015");
        assert_eq!(config.rule_param_u64("T015", "min_words"), Some(7));
        assert_eq!(config.rule_param_u64("T015", "max_words"), Some(50));
    }

    #[test]
    fn parse_multi_domain_config() {
        let toml_str = r#"
[stale]
directory = "stale"

[[domains]]
prefix = "COM"
name = "Common"
directory = "common"
description = "Cross-cutting"
crates = []

[[domains]]
prefix = "CHE"
name = "Cherry"
directory = "cherry"
description = "Architecture"
crates = ["cherry-pit-core", "cherry-pit-gateway"]
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.domains.len(), 2);
        assert_eq!(config.domains[0].prefix, "COM");
        assert!(config.domains[0].crates.is_empty());
        assert_eq!(config.domains[1].crates.len(), 2);
    }

    #[test]
    fn parse_rule_with_params() {
        let toml_str = r#"
[stale]
directory = "stale"

[[domains]]
prefix = "CHE"
name = "Cherry"
directory = "cherry"
description = "Test"
crates = []

[[rules]]
id = "T015"
params = { min_words = 10 }
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.rules[0].id, "T015");
        let min_words = config.rule_param_u64("T015", "min_words");
        assert_eq!(min_words, Some(10));
    }

    #[test]
    fn rule_param_missing_returns_none() {
        let toml_str = r#"
[stale]
directory = "stale"

[[domains]]
prefix = "CHE"
name = "Cherry"
directory = "cherry"
description = "Test"
crates = []
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.rule_param_u64("T001", "min_words"), None);
        assert_eq!(config.rule_param_u64("MISSING", "key"), None);
    }

    #[test]
    fn missing_required_field_fails() {
        let toml_str = r#"
[stale]
directory = "stale"

[[domains]]
prefix = "CHE"
name = "Cherry"
# missing directory and description
"#;
        let result: Result<Config, _> = toml::from_str(toml_str);
        assert!(result.is_err());
    }

    #[test]
    fn foundation_flag_defaults_to_false() {
        let toml_str = r#"
[stale]
directory = "stale"

[[domains]]
prefix = "CHE"
name = "Cherry"
directory = "cherry"
description = "Test"
crates = []
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(!config.domains[0].foundation);
    }

    #[test]
    fn foundation_flag_true_deserializes() {
        let toml_str = r#"
[stale]
directory = "stale"

[[domains]]
prefix = "COM"
name = "Common"
directory = "common"
description = "Cross-cutting"
crates = []
foundation = true
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(config.domains[0].foundation);
    }

    #[test]
    fn legacy_format_still_parses() {
        // Old-style TOML with full rule declarations still works
        let toml_str = r#"
[stale]
directory = "stale"

[[domains]]
prefix = "CHE"
name = "Cherry"
directory = "cherry"
description = "Test"
crates = []

[[rules]]
id = "T001"
category = "template"
description = "H1 title present"

[[rules]]
id = "T002"
category = "template"
description = "Date field present"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.rules.len(), 2);
    }
}
