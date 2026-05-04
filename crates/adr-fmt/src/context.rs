//! Context mode — decision rules applicable to a specific crate.
//!
//! `--context cherry-pit-core` resolves which ADRs apply to a crate
//! and extracts their tagged decision rules.

use crate::config::Config;
use crate::model::{AdrRecord, Status, Tier};
use crate::output::CrateRule;

/// Resolve decision rules applicable to a crate.
///
/// Resolution chain:
/// 1. Find domains where `crate_name` ∈ `domain.crates` → candidate domains
/// 2. Within candidates: if any ADR has `crates` field populated, filter
///    to ADRs where `crate_name` ∈ `adr.crates`; else include all domain ADRs
/// 3. Always include all ADRs from `foundation = true` domains
///
/// Returns `CrateRule` entries ordered by tier (S→D), then by ADR ID.
/// Only Accepted ADRs are included.
///
/// # Errors
///
/// Returns an error if `crate_name` is not found in any domain's crate list.
pub fn context(
    crate_name: &str,
    records: &[AdrRecord],
    config: &Config,
) -> Result<Vec<CrateRule>, String> {
    // Find candidate domains
    let candidate_domains: Vec<&str> = config
        .domains
        .iter()
        .filter(|d| d.crates.iter().any(|c| c == crate_name))
        .map(|d| d.prefix.as_str())
        .collect();

    if candidate_domains.is_empty() {
        return Err(format!(
            "crate '{crate_name}' not found in any domain's crate list"
        ));
    }

    // Foundation domain prefixes
    let foundation_prefixes: Vec<&str> = config
        .domains
        .iter()
        .filter(|d| d.foundation)
        .map(|d| d.prefix.as_str())
        .collect();

    let mut rules = Vec::new();

    // Collect foundation domain ADRs (Accepted only)
    for record in records {
        if record.is_stale {
            continue;
        }
        if record.status.as_ref() != Some(&Status::Accepted) {
            continue;
        }
        if !foundation_prefixes.contains(&record.id.prefix.as_str()) {
            continue;
        }
        if record.decision_rules.is_empty() {
            continue;
        }

        rules.push(CrateRule {
            adr_id: record.id.clone(),
            tier: record.tier,
            rules: record.decision_rules.clone(),
        });
    }

    // Collect candidate domain ADRs (Accepted only)
    for prefix in &candidate_domains {
        let domain_records: Vec<&AdrRecord> = records
            .iter()
            .filter(|r| {
                !r.is_stale
                    && r.id.prefix == *prefix
                    && r.status.as_ref() == Some(&Status::Accepted)
            })
            .collect();

        // Check if any ADR in this domain has a populated `crates` field
        let any_has_crates = domain_records.iter().any(|r| !r.crates.is_empty());

        for record in &domain_records {
            // If per-ADR crate annotations exist, filter to matching ADRs
            if any_has_crates
                && !record.crates.is_empty()
                && !record.crates.iter().any(|c| c == crate_name)
            {
                continue;
            }

            if record.decision_rules.is_empty() {
                continue;
            }

            rules.push(CrateRule {
                adr_id: record.id.clone(),
                tier: record.tier,
                rules: record.decision_rules.clone(),
            });
        }
    }

    // Sort by tier (S→D), then by prefix, then by ID number
    rules.sort_by(|a, b| {
        let ta = a.tier.unwrap_or(Tier::D).rank();
        let tb = b.tier.unwrap_or(Tier::D).rank();
        ta.cmp(&tb)
            .then(a.adr_id.prefix.cmp(&b.adr_id.prefix))
            .then(a.adr_id.number.cmp(&b.adr_id.number))
    });

    Ok(rules)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AdrId, AdrRecord, Status, TaggedRule, Tier};
    use std::path::PathBuf;

    fn make_config() -> Config {
        toml::from_str(
            r#"
[stale]
directory = "stale"

[[domains]]
prefix = "COM"
name = "Common"
directory = "common"
description = "Cross-cutting"
crates = []
foundation = true

[[domains]]
prefix = "CHE"
name = "Cherry"
directory = "cherry"
description = "Architecture"
crates = ["cherry-pit-core", "cherry-pit-gateway"]

[[rules]]
id = "T001"
category = "template"
description = "test"
"#,
        )
        .unwrap()
    }

    fn make_record_with_rules(
        prefix: &str,
        num: u16,
        crates: Vec<&str>,
        rules: Vec<(&str, &str)>,
    ) -> AdrRecord {
        AdrRecord {
            id: AdrId {
                prefix: prefix.into(),
                number: num,
            },
            file_path: PathBuf::from(format!("{prefix}-{num:04}-test.md")),
            title: Some(format!("Test {prefix}-{num:04}")),
            title_line: 1,
            tier: Some(Tier::A),
            status: Some(Status::Accepted),
            status_raw: Some("Accepted".into()),
            has_related: true,
            has_context: true,
            has_decision: true,
            has_consequences: true,
            crates: crates
                .into_iter()
                .map(std::borrow::ToOwned::to_owned)
                .collect(),
            decision_rules: rules
                .into_iter()
                .map(|(id, text)| TaggedRule {
                    id: id.into(),
                    text: text.into(),
                    line: 0,
                    layer: 5,
                })
                .collect(),
            ..AdrRecord::default()
        }
    }

    #[test]
    fn context_includes_foundation() {
        let records = vec![
            make_record_with_rules("COM", 1, vec![], vec![("R1", "Foundation rule")]),
            make_record_with_rules("CHE", 1, vec![], vec![("R1", "Cherry rule")]),
        ];
        let config = make_config();
        let rules = context("cherry-pit-core", &records, &config).unwrap();

        // Should include COM (foundation) and CHE
        let prefixes: Vec<&str> = rules.iter().map(|r| r.adr_id.prefix.as_str()).collect();
        assert!(prefixes.contains(&"COM"), "should include foundation");
        assert!(prefixes.contains(&"CHE"), "should include domain");
    }

    #[test]
    fn context_sorts_by_tier() {
        let mut com_d = make_record_with_rules("COM", 1, vec![], vec![("R1", "Foundation rule")]);
        com_d.tier = Some(Tier::D);
        let mut che_s = make_record_with_rules("CHE", 1, vec![], vec![("R1", "Cherry rule")]);
        che_s.tier = Some(Tier::S);

        let records = vec![com_d, che_s];
        let config = make_config();
        let rules = context("cherry-pit-core", &records, &config).unwrap();

        // S-tier CHE should come before D-tier COM (tier wins over foundation)
        assert_eq!(rules[0].adr_id.prefix, "CHE", "S-tier should be first");
        assert_eq!(rules[1].adr_id.prefix, "COM", "D-tier should be second");
    }

    #[test]
    fn context_excludes_draft() {
        let mut draft = make_record_with_rules("CHE", 2, vec![], vec![("R1", "Draft rule")]);
        draft.status = Some(Status::Draft);
        draft.status_raw = Some("Draft".into());

        let records = vec![
            make_record_with_rules("CHE", 1, vec![], vec![("R1", "Active rule")]),
            draft,
        ];
        let config = make_config();
        let rules = context("cherry-pit-core", &records, &config).unwrap();

        let ids: Vec<u16> = rules.iter().map(|r| r.adr_id.number).collect();
        assert!(ids.contains(&1), "accepted should be included");
        assert!(!ids.contains(&2), "draft should be excluded");
    }

    #[test]
    fn context_excludes_rejected() {
        let mut rejected = make_record_with_rules("CHE", 2, vec![], vec![("R1", "Rejected rule")]);
        rejected.status = Some(Status::Rejected);
        rejected.status_raw = Some("Rejected".into());

        let records = vec![
            make_record_with_rules("CHE", 1, vec![], vec![("R1", "Active rule")]),
            rejected,
        ];
        let config = make_config();
        let rules = context("cherry-pit-core", &records, &config).unwrap();

        let ids: Vec<u16> = rules.iter().map(|r| r.adr_id.number).collect();
        assert!(ids.contains(&1), "accepted should be included");
        assert!(!ids.contains(&2), "rejected should be excluded");
    }

    #[test]
    fn context_excludes_proposed_foundation() {
        let mut proposed =
            make_record_with_rules("COM", 1, vec![], vec![("R1", "Proposed foundation rule")]);
        proposed.status = Some(Status::Proposed);
        proposed.status_raw = Some("Proposed".into());

        let records = vec![
            proposed,
            make_record_with_rules("CHE", 1, vec![], vec![("R1", "Active rule")]),
        ];
        let config = make_config();
        let rules = context("cherry-pit-core", &records, &config).unwrap();

        let prefixes: Vec<&str> = rules.iter().map(|r| r.adr_id.prefix.as_str()).collect();
        assert!(
            !prefixes.contains(&"COM"),
            "proposed foundation should be excluded"
        );
        assert!(prefixes.contains(&"CHE"), "accepted should be included");
    }

    #[test]
    fn context_filters_by_per_adr_crates() {
        let records = vec![
            make_record_with_rules("CHE", 1, vec!["cherry-pit-core"], vec![("R1", "Core rule")]),
            make_record_with_rules(
                "CHE",
                2,
                vec!["cherry-pit-gateway"],
                vec![("R1", "Gateway rule")],
            ),
        ];
        let config = make_config();
        let rules = context("cherry-pit-core", &records, &config).unwrap();

        // Only CHE-0001 should be included (crate-level filter)
        let ids: Vec<u16> = rules.iter().map(|r| r.adr_id.number).collect();
        assert!(ids.contains(&1), "should include CHE-0001");
        assert!(!ids.contains(&2), "should exclude CHE-0002");
    }

    #[test]
    fn context_fallback_all_domain_adrs() {
        // No per-ADR crate annotations → include all domain ADRs
        let records = vec![
            make_record_with_rules("CHE", 1, vec![], vec![("R1", "Rule 1")]),
            make_record_with_rules("CHE", 2, vec![], vec![("R1", "Rule 2")]),
        ];
        let config = make_config();
        let rules = context("cherry-pit-core", &records, &config).unwrap();

        let ids: Vec<u16> = rules.iter().map(|r| r.adr_id.number).collect();
        assert!(ids.contains(&1));
        assert!(ids.contains(&2));
    }

    #[test]
    fn context_excludes_stale() {
        let mut stale = make_record_with_rules("CHE", 3, vec![], vec![("R1", "Stale rule")]);
        stale.is_stale = true;

        let records = vec![
            make_record_with_rules("CHE", 1, vec![], vec![("R1", "Active rule")]),
            stale,
        ];
        let config = make_config();
        let rules = context("cherry-pit-core", &records, &config).unwrap();

        let ids: Vec<u16> = rules.iter().map(|r| r.adr_id.number).collect();
        assert!(ids.contains(&1));
        assert!(!ids.contains(&3), "stale should be excluded");
    }

    #[test]
    fn context_unknown_crate_returns_error() {
        let records = vec![make_record_with_rules(
            "CHE",
            1,
            vec![],
            vec![("R1", "Rule")],
        )];
        let config = make_config();
        let result = context("nonexistent-crate", &records, &config);
        match result {
            Err(e) => assert!(e.contains("not found in any domain"), "error: {e}"),
            Ok(_) => panic!("expected error for unknown crate"),
        }
    }
}
