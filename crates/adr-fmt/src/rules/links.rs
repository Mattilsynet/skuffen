//! Link and relationship rules (L001, L003, L007–L009).
//!
//! L001: Dangling link — target ADR file does not exist
//! L003: Supersedes-status consistency — if A supersedes B, B's
//!       status must be `Superseded by A`
//! L007: Stale reference — target ADR is in stale archive
//! L008: Root self-reference mismatch — Root target must match own ID
//! L009: Root + References coexistence — Root and References cannot
//!       appear in the same Related section

use std::collections::HashMap;

use crate::model::{AdrId, AdrRecord, RelVerb, Relationship};
use crate::report::Diagnostic;

pub fn check(records: &[AdrRecord], diags: &mut Vec<Diagnostic>) {
    // Build a lookup: AdrId → &AdrRecord
    let by_id: HashMap<&AdrId, &AdrRecord> = records.iter().map(|r| (&r.id, r)).collect();

    for record in records {
        // L009: Root + References coexistence check (per-record)
        check_root_references_coexistence(record, diags);

        // L008: Root self-reference mismatch (per-relationship)
        for rel in &record.relationships {
            if rel.verb == RelVerb::Root {
                check_root_self_reference(record, rel, diags);
            }
        }

        for rel in &record.relationships {
            check_single_link(record, rel, &by_id, diags);
        }
    }

    // L003: Supersedes-status consistency (cross-file)
    check_supersedes_consistency(records, &by_id, diags);
}

fn check_single_link(
    source: &AdrRecord,
    rel: &Relationship,
    by_id: &HashMap<&AdrId, &AdrRecord>,
    diags: &mut Vec<Diagnostic>,
) {
    let target_id = &rel.target;

    // Skip further link validation for Root self-references
    if rel.verb == RelVerb::Root && rel.target == source.id {
        return;
    }

    // L001: Dangling link — target file not found
    if !by_id.contains_key(target_id) {
        diags.push(Diagnostic::warning(
            "L001",
            &source.file_path,
            rel.line,
            format!(
                "{} → {target_id}: dangling link (target ADR not found)",
                source.id,
            ),
        ));
        return;
    }

    // L007: Stale reference — target is in stale archive
    // Exempt Supersedes relationships: they inherently target stale ADRs.
    if let Some(target_record) = by_id.get(target_id)
        && target_record.is_stale
        && !source.is_stale
        && rel.verb != RelVerb::Supersedes
    {
        diags.push(Diagnostic::warning(
            "L007",
            &source.file_path,
            rel.line,
            format!("{} → {target_id}: reference to stale ADR", source.id),
        ));
    }
}

/// L003: If A has `Supersedes: B`, then B's status must be
/// `Superseded by A`. Warns on inconsistency.
fn check_supersedes_consistency(
    records: &[AdrRecord],
    by_id: &HashMap<&AdrId, &AdrRecord>,
    diags: &mut Vec<Diagnostic>,
) {
    for record in records {
        for rel in &record.relationships {
            if rel.verb != RelVerb::Supersedes {
                continue;
            }

            let target_id = &rel.target;
            if let Some(target_record) = by_id.get(target_id) {
                let status_matches = matches!(
                    &target_record.status,
                    Some(crate::model::Status::SupersededBy(by_id)) if *by_id == record.id
                );

                if !status_matches {
                    diags.push(Diagnostic::warning(
                        "L003",
                        &record.file_path,
                        rel.line,
                        format!(
                            "{} supersedes {target_id}, but {target_id}'s status \
                             is not `Superseded by {}` — update the target's status",
                            record.id, record.id,
                        ),
                    ));
                }
            }
        }
    }
}

/// L008: Root self-reference mismatch.
fn check_root_self_reference(source: &AdrRecord, rel: &Relationship, diags: &mut Vec<Diagnostic>) {
    debug_assert_eq!(rel.verb, RelVerb::Root);
    if rel.target != source.id {
        diags.push(Diagnostic::warning(
            "L008",
            &source.file_path,
            rel.line,
            format!(
                "{}: Root target `{}` does not match own ID — \
                 Root must be a self-reference (`- Root: {}`)",
                source.id, rel.target, source.id,
            ),
        ));
    }
}

/// L009: Root and References cannot coexist in the same Related section.
fn check_root_references_coexistence(source: &AdrRecord, diags: &mut Vec<Diagnostic>) {
    let has_root = source.relationships.iter().any(|r| r.verb == RelVerb::Root);
    let has_references = source
        .relationships
        .iter()
        .any(|r| r.verb == RelVerb::References);

    if has_root && has_references {
        // Find line of first References entry for diagnostic location
        let ref_line = source
            .relationships
            .iter()
            .find(|r| r.verb == RelVerb::References)
            .map_or(0, |r| r.line);

        diags.push(Diagnostic::warning(
            "L009",
            &source.file_path,
            ref_line,
            format!(
                "{}: Root and References cannot coexist — \
                 a root ADR stands alone structurally",
                source.id,
            ),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AdrId, Status, Tier};
    use std::path::PathBuf;

    fn make_id(prefix: &str, num: u16) -> AdrId {
        AdrId {
            prefix: prefix.into(),
            number: num,
        }
    }

    fn make_record_with_rels(prefix: &str, num: u16, rels: Vec<(RelVerb, AdrId)>) -> AdrRecord {
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
            file_path: PathBuf::from(format!("docs/adr/cherry/{prefix}-{num:04}-test.md")),
            title: Some("Test".into()),
            title_line: 1,
            date: Some("2026-04-25".into()),
            last_reviewed: Some("2026-04-25".into()),
            tier: Some(Tier::B),
            status: Some(Status::Accepted),
            status_line: 8,
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
    fn forward_link_no_errors() {
        let records = vec![
            make_record_with_rels("CHE", 1, vec![(RelVerb::References, make_id("CHE", 2))]),
            make_record_with_rels("CHE", 2, vec![(RelVerb::Root, make_id("CHE", 2))]),
        ];
        let mut diags = Vec::new();
        check(&records, &mut diags);
        assert!(diags.is_empty(), "expected no diags, got: {diags:?}");
    }

    #[test]
    fn dangling_link_produces_l001() {
        let records = vec![make_record_with_rels(
            "CHE",
            1,
            vec![(RelVerb::References, make_id("CHE", 99))],
        )];
        let mut diags = Vec::new();
        check(&records, &mut diags);
        assert!(
            diags.iter().any(|d| d.rule == "L001"),
            "expected L001, got: {diags:?}"
        );
    }

    #[test]
    fn root_self_reference_match_no_l008() {
        let records = vec![make_record_with_rels(
            "CHE",
            1,
            vec![(RelVerb::Root, make_id("CHE", 1))],
        )];
        let mut diags = Vec::new();
        check(&records, &mut diags);
        assert!(
            !diags.iter().any(|d| d.rule == "L008"),
            "correct Root self-ref should not trigger L008"
        );
    }

    #[test]
    fn root_wrong_id_produces_l008() {
        let records = vec![make_record_with_rels(
            "CHE",
            1,
            vec![(RelVerb::Root, make_id("CHE", 2))],
        )];
        let mut diags = Vec::new();
        check(&records, &mut diags);
        assert!(
            diags.iter().any(|d| d.rule == "L008"),
            "Root pointing to wrong ID should trigger L008, got: {diags:?}"
        );
    }

    #[test]
    fn root_and_references_produces_l009() {
        let records = vec![
            make_record_with_rels(
                "CHE",
                1,
                vec![
                    (RelVerb::Root, make_id("CHE", 1)),
                    (RelVerb::References, make_id("CHE", 2)),
                ],
            ),
            make_record_with_rels("CHE", 2, vec![(RelVerb::Root, make_id("CHE", 2))]),
        ];
        let mut diags = Vec::new();
        check(&records, &mut diags);
        assert!(
            diags.iter().any(|d| d.rule == "L009"),
            "Root + References should trigger L009, got: {diags:?}"
        );
    }

    #[test]
    fn root_and_supersedes_no_l009() {
        let record = make_record_with_rels(
            "CHE",
            2,
            vec![
                (RelVerb::Root, make_id("CHE", 2)),
                (RelVerb::Supersedes, make_id("CHE", 1)),
            ],
        );
        let mut target = make_record_with_rels("CHE", 1, vec![(RelVerb::Root, make_id("CHE", 1))]);
        target.status = Some(Status::SupersededBy(make_id("CHE", 2)));
        target.status_raw = Some("Superseded by CHE-0002".into());

        let records = vec![record, target];
        let mut diags = Vec::new();
        check(&records, &mut diags);
        assert!(
            !diags.iter().any(|d| d.rule == "L009"),
            "Root + Supersedes should not trigger L009, got: {diags:?}"
        );
    }

    #[test]
    fn supersedes_without_target_status_produces_l003() {
        let records = vec![
            make_record_with_rels("CHE", 2, vec![(RelVerb::Supersedes, make_id("CHE", 1))]),
            make_record_with_rels("CHE", 1, vec![(RelVerb::Root, make_id("CHE", 1))]),
        ];
        let mut diags = Vec::new();
        check(&records, &mut diags);
        assert!(
            diags.iter().any(|d| d.rule == "L003"),
            "expected L003, got: {diags:?}"
        );
    }

    #[test]
    fn supersedes_with_correct_target_status_no_l003() {
        let mut target = make_record_with_rels("CHE", 1, vec![(RelVerb::Root, make_id("CHE", 1))]);
        target.status = Some(Status::SupersededBy(make_id("CHE", 2)));
        target.status_raw = Some("Superseded by CHE-0002".into());

        let records = vec![
            make_record_with_rels("CHE", 2, vec![(RelVerb::Supersedes, make_id("CHE", 1))]),
            target,
        ];
        let mut diags = Vec::new();
        check(&records, &mut diags);
        assert!(
            !diags.iter().any(|d| d.rule == "L003"),
            "correct supersedes-status should not trigger L003, got: {diags:?}"
        );
    }

    #[test]
    fn stale_reference_produces_l007() {
        let mut target = make_record_with_rels("CHE", 1, vec![(RelVerb::Root, make_id("CHE", 1))]);
        target.is_stale = true;

        let records = vec![
            make_record_with_rels("CHE", 2, vec![(RelVerb::References, make_id("CHE", 1))]),
            target,
        ];
        let mut diags = Vec::new();
        check(&records, &mut diags);
        assert!(
            diags.iter().any(|d| d.rule == "L007"),
            "expected L007, got: {diags:?}"
        );
    }

    #[test]
    fn supersedes_stale_no_l007() {
        let mut target = make_record_with_rels("CHE", 1, vec![(RelVerb::Root, make_id("CHE", 1))]);
        target.is_stale = true;
        target.status = Some(Status::SupersededBy(make_id("CHE", 2)));
        target.status_raw = Some("Superseded by CHE-0002".into());

        let records = vec![
            make_record_with_rels("CHE", 2, vec![(RelVerb::Supersedes, make_id("CHE", 1))]),
            target,
        ];
        let mut diags = Vec::new();
        check(&records, &mut diags);
        assert!(
            !diags.iter().any(|d| d.rule == "L007"),
            "Supersedes→stale should not trigger L007, got: {diags:?}"
        );
    }

    #[test]
    fn stale_source_references_stale_no_l007() {
        let mut source =
            make_record_with_rels("CHE", 2, vec![(RelVerb::References, make_id("CHE", 1))]);
        source.is_stale = true;

        let mut target = make_record_with_rels("CHE", 1, vec![(RelVerb::Root, make_id("CHE", 1))]);
        target.is_stale = true;

        let records = vec![source, target];
        let mut diags = Vec::new();
        check(&records, &mut diags);
        assert!(
            !diags.iter().any(|d| d.rule == "L007"),
            "stale→stale should not trigger L007, got: {diags:?}"
        );
    }
}
