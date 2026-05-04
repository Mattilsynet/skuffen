//! Critique mode — bounded transitive closure analysis of a focal ADR.
//!
//! `--critique CHE-0042` builds the transitive closure of all ADRs
//! reachable from the focal ADR (both fan-out and fan-in), bounded by
//! `--depth N` (default 1). Stale ADRs are included without filtering.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;

use crate::config::Config;
use crate::model::{AdrId, AdrRecord, RelVerb, Tier, layer_to_tier};
use crate::nav::{self, ChildEntry};
use crate::output::{self, OutputBlock};

/// Run critique mode for a focal ADR with depth bounding.
///
/// Returns output blocks: focal first, connected sorted by tier then ID.
///
/// # Errors
///
/// Returns an error if the focal ADR ID is not found in the parsed records.
pub fn critique(
    focal_id: &AdrId,
    records: &[AdrRecord],
    config: &Config,
    max_depth: usize,
) -> Result<Vec<OutputBlock>, String> {
    // Find focal record
    let Some(focal) = records.iter().find(|r| r.id == *focal_id) else {
        return Err(format!("ADR {focal_id} not found"));
    };

    // Build indexes
    let by_id: HashMap<&AdrId, &AdrRecord> = records.iter().map(|r| (&r.id, r)).collect();
    let children = nav::compute_children(records);

    // BFS transitive closure — both directions, bounded by depth
    let mut visited: HashSet<&AdrId> = HashSet::new();
    let mut queue: VecDeque<(&AdrId, usize)> = VecDeque::new();

    visited.insert(&focal.id);
    queue.push_back((&focal.id, 0));

    while let Some((current_id, depth)) = queue.pop_front() {
        if depth >= max_depth {
            continue;
        }

        let Some(current) = by_id.get(current_id) else {
            continue;
        };

        // Fan-out: follow forward relationships
        for rel in &current.relationships {
            if rel.verb.is_reverse() {
                continue;
            }
            if rel.verb == RelVerb::Root && rel.target == current.id {
                continue;
            }
            if visited.insert(&rel.target) {
                queue.push_back((&rel.target, depth + 1));
            }
        }

        // Fan-in: follow reverse links (children)
        if let Some(child_entries) = children.get(current_id) {
            for entry in child_entries {
                if visited.insert(&entry.child) {
                    queue.push_back((&entry.child, depth + 1));
                }
            }
        }
    }

    // Collect connected records (no stale filtering)
    let mut connected: Vec<&AdrRecord> = Vec::new();

    for id in &visited {
        if **id == focal.id {
            continue;
        }
        if let Some(record) = by_id.get(id) {
            connected.push(record);
        }
    }

    // Sort connected: by tier (S→D), then by ID
    connected.sort_by(|a, b| {
        let ta = a.tier.map_or(255, Tier::rank);
        let tb = b.tier.map_or(255, Tier::rank);
        ta.cmp(&tb)
            .then(a.id.prefix.cmp(&b.id.prefix))
            .then(a.id.number.cmp(&b.id.number))
    });

    // Build output blocks
    let mut blocks = Vec::new();

    // Focal block
    let focal_content = read_file_content(&focal.file_path);
    let focal_meta = output::build_header_meta(focal, config, &children);

    // Compute tension summary for focal rules
    let tension_suffix = compute_tension_summary(focal);
    let focal_content_with_tension = if tension_suffix.is_empty() {
        focal_content
    } else {
        format!("{focal_content}\n{tension_suffix}")
    };

    blocks.push(OutputBlock::Focal {
        meta: focal_meta,
        content: focal_content_with_tension,
    });

    // Connected blocks
    for record in &connected {
        let content = read_file_content(&record.file_path);
        let meta = output::build_header_meta(record, config, &children);
        let path = build_relationship_path(focal_id, &record.id, records, &children);
        blocks.push(OutputBlock::Connected {
            meta,
            content,
            path,
        });
    }

    Ok(blocks)
}

/// Compute tension summary for an ADR's rules.
///
/// Tension = `abs_diff(adr_tier.rank(), layer_derived_tier.rank())`.
/// Only rules with non-zero tension are reported.
/// Returns empty string if no tension or no tier/rules.
fn compute_tension_summary(record: &AdrRecord) -> String {
    let Some(adr_tier) = record.tier else {
        return String::new();
    };

    let mut lines: Vec<String> = Vec::new();
    let adr_rank = adr_tier.rank();

    for rule in &record.decision_rules {
        let Some(rule_tier) = layer_to_tier(rule.layer) else {
            continue;
        };
        let rule_rank = rule_tier.rank();
        let distance = adr_rank.abs_diff(rule_rank);
        if distance > 0 {
            lines.push(format!(
                "Tension: {} (+{distance} from {adr_tier:?}→{rule_tier:?})",
                rule.id,
            ));
        }
    }

    lines.join("\n")
}

/// Read file content, returning empty string on failure.
fn read_file_content(path: &std::path::Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

/// Build a human-readable relationship path from focal to target.
fn build_relationship_path(
    focal_id: &AdrId,
    target_id: &AdrId,
    records: &[AdrRecord],
    children: &HashMap<AdrId, Vec<ChildEntry>>,
) -> String {
    let by_id: HashMap<&AdrId, &AdrRecord> = records.iter().map(|r| (&r.id, r)).collect();

    // Check fan-out from focal
    if let Some(focal) = by_id.get(focal_id) {
        for rel in &focal.relationships {
            if rel.target == *target_id && !rel.verb.is_reverse() {
                return format!("{focal_id} → {} → {target_id}", rel.verb);
            }
        }
    }

    // Check fan-in to focal
    if let Some(entries) = children.get(focal_id) {
        for entry in entries {
            if entry.child == *target_id {
                return format!("{target_id} → {} → {focal_id}", entry.verb);
            }
        }
    }

    // Transitive — show general path
    format!("{focal_id} → ... → {target_id}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AdrId, AdrRecord, RelVerb, Relationship, Status, TaggedRule, Tier};
    use std::path::PathBuf;

    fn make_id(prefix: &str, num: u16) -> AdrId {
        AdrId {
            prefix: prefix.into(),
            number: num,
        }
    }

    fn make_config() -> Config {
        toml::from_str(
            r#"
[stale]
directory = "stale"

[[domains]]
prefix = "CHE"
name = "Cherry"
directory = "cherry"
description = "Test"
crates = []
"#,
        )
        .unwrap()
    }

    fn make_record(prefix: &str, num: u16, rels: Vec<(RelVerb, AdrId)>) -> AdrRecord {
        let id = make_id(prefix, num);
        let relationships: Vec<Relationship> = rels
            .into_iter()
            .enumerate()
            .map(|(i, (verb, target))| Relationship {
                verb,
                target,
                line: 10 + i,
            })
            .collect();

        AdrRecord {
            id,
            file_path: PathBuf::from(format!("nonexistent/{prefix}-{num:04}-test.md")),
            title: Some(format!("Test {prefix}-{num:04}")),
            title_line: 1,
            tier: Some(Tier::B),
            status: Some(Status::Accepted),
            status_raw: Some("Accepted".into()),
            relationships,
            has_related: true,
            has_context: true,
            has_decision: true,
            has_consequences: true,
            ..AdrRecord::default()
        }
    }

    #[test]
    fn critique_isolated_adr() {
        let records = vec![make_record(
            "CHE",
            1,
            vec![(RelVerb::Root, make_id("CHE", 1))],
        )];
        let config = make_config();
        let blocks = critique(&make_id("CHE", 1), &records, &config, 1).unwrap();
        assert_eq!(blocks.len(), 1);
        assert!(matches!(blocks[0], OutputBlock::Focal { .. }));
    }

    #[test]
    fn critique_with_connected() {
        let records = vec![
            make_record("CHE", 1, vec![(RelVerb::Root, make_id("CHE", 1))]),
            make_record("CHE", 2, vec![(RelVerb::References, make_id("CHE", 1))]),
        ];
        let config = make_config();
        let blocks = critique(&make_id("CHE", 1), &records, &config, 1).unwrap();
        // Focal + 1 connected (fan-in from CHE-0002)
        assert_eq!(blocks.len(), 2);
        assert!(matches!(blocks[0], OutputBlock::Focal { .. }));
        assert!(matches!(blocks[1], OutputBlock::Connected { .. }));
    }

    #[test]
    fn critique_includes_stale() {
        let mut stale = make_record("CHE", 3, vec![(RelVerb::References, make_id("CHE", 1))]);
        stale.is_stale = true;

        let records = vec![
            make_record("CHE", 1, vec![(RelVerb::Root, make_id("CHE", 1))]),
            make_record("CHE", 2, vec![(RelVerb::References, make_id("CHE", 1))]),
            stale,
        ];
        let config = make_config();
        let blocks = critique(&make_id("CHE", 1), &records, &config, 1).unwrap();
        // Focal + 2 connected (stale NOT filtered)
        assert_eq!(blocks.len(), 3);
        assert!(matches!(blocks[0], OutputBlock::Focal { .. }));
        assert!(matches!(blocks[1], OutputBlock::Connected { .. }));
        assert!(matches!(blocks[2], OutputBlock::Connected { .. }));
    }

    #[test]
    fn critique_respects_depth_bound() {
        // Chain: CHE-0001 ← CHE-0002 ← CHE-0003
        let records = vec![
            make_record("CHE", 1, vec![(RelVerb::Root, make_id("CHE", 1))]),
            make_record("CHE", 2, vec![(RelVerb::References, make_id("CHE", 1))]),
            make_record("CHE", 3, vec![(RelVerb::References, make_id("CHE", 2))]),
        ];
        let config = make_config();

        // Depth 1: focal CHE-0001 sees CHE-0002 (fan-in), NOT CHE-0003
        let blocks = critique(&make_id("CHE", 1), &records, &config, 1).unwrap();
        assert_eq!(blocks.len(), 2, "depth=1 should reach 1 neighbor");

        // Depth 2: focal CHE-0001 sees CHE-0002 and CHE-0003
        let blocks = critique(&make_id("CHE", 1), &records, &config, 2).unwrap();
        assert_eq!(blocks.len(), 3, "depth=2 should reach 2 neighbors");
    }

    #[test]
    fn critique_handles_cycle() {
        let records = vec![
            make_record("CHE", 1, vec![(RelVerb::References, make_id("CHE", 2))]),
            make_record("CHE", 2, vec![(RelVerb::References, make_id("CHE", 1))]),
        ];
        let config = make_config();
        let blocks = critique(&make_id("CHE", 1), &records, &config, 5).unwrap();
        // Should terminate and include both
        assert!(blocks.len() >= 2);
    }

    #[test]
    fn critique_depth_zero_returns_focal_only() {
        let records = vec![
            make_record("CHE", 1, vec![(RelVerb::Root, make_id("CHE", 1))]),
            make_record("CHE", 2, vec![(RelVerb::References, make_id("CHE", 1))]),
        ];
        let config = make_config();
        let blocks = critique(&make_id("CHE", 1), &records, &config, 0).unwrap();
        assert_eq!(blocks.len(), 1, "depth=0 should return focal only");
    }

    #[test]
    fn tension_summary_no_tier() {
        let mut record = make_record("CHE", 1, vec![(RelVerb::Root, make_id("CHE", 1))]);
        record.tier = None;
        record.decision_rules = vec![TaggedRule {
            id: "R1".into(),
            text: "Some rule".into(),
            line: 10,
            layer: 5,
        }];
        let summary = compute_tension_summary(&record);
        assert!(summary.is_empty(), "no tier means no tension");
    }

    #[test]
    fn tension_summary_no_tension() {
        // B-tier ADR with layer 5 rules (layer 5 → B-tier) → no tension
        let mut record = make_record("CHE", 1, vec![(RelVerb::Root, make_id("CHE", 1))]);
        record.tier = Some(Tier::B);
        record.decision_rules = vec![TaggedRule {
            id: "R1".into(),
            text: "Some rule".into(),
            line: 10,
            layer: 5,
        }];
        let summary = compute_tension_summary(&record);
        assert!(summary.is_empty(), "same tier means no tension");
    }

    #[test]
    fn tension_summary_with_distance() {
        // S-tier ADR (rank=0) with layer 5 rules (→ B-tier, rank=2) → distance=2
        let mut record = make_record("CHE", 1, vec![(RelVerb::Root, make_id("CHE", 1))]);
        record.tier = Some(Tier::S);
        record.decision_rules = vec![
            TaggedRule {
                id: "R1".into(),
                text: "Rule one".into(),
                line: 10,
                layer: 5,
            },
            TaggedRule {
                id: "R2".into(),
                text: "Rule two".into(),
                line: 11,
                layer: 9,
            },
        ];
        let summary = compute_tension_summary(&record);
        assert!(
            summary.contains("Tension: R1 (+2 from S→B)"),
            "expected S→B tension:\n{summary}"
        );
        assert!(
            summary.contains("Tension: R2 (+4 from S→D)"),
            "expected S→D tension:\n{summary}"
        );
    }

    #[test]
    fn tension_summary_skips_zero_distance() {
        // A-tier ADR (rank=1) with layer 4 rules (→ A-tier, rank=1) → distance=0
        let mut record = make_record("CHE", 1, vec![(RelVerb::Root, make_id("CHE", 1))]);
        record.tier = Some(Tier::A);
        record.decision_rules = vec![
            TaggedRule {
                id: "R1".into(),
                text: "Aligned rule".into(),
                line: 10,
                layer: 4,
            },
            TaggedRule {
                id: "R2".into(),
                text: "Misaligned rule".into(),
                line: 11,
                layer: 9,
            },
        ];
        let summary = compute_tension_summary(&record);
        assert!(
            !summary.contains("R1"),
            "R1 has zero tension, should be skipped"
        );
        assert!(
            summary.contains("Tension: R2 (+3 from A→D)"),
            "expected A→D tension:\n{summary}"
        );
    }

    #[test]
    fn critique_unknown_adr_returns_error() {
        let records = vec![make_record(
            "CHE",
            1,
            vec![(RelVerb::Root, make_id("CHE", 1))],
        )];
        let config = make_config();
        let result = critique(&make_id("CHE", 99), &records, &config, 1);
        match result {
            Err(e) => assert!(e.contains("not found"), "error: {e}"),
            Ok(_) => panic!("expected error for unknown ADR"),
        }
    }
}
