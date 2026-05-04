//! Computed children index — inverts forward links to produce a
//! reverse-link index on demand.
//!
//! Used by critique mode for fan-in resolution.

use std::collections::HashMap;

use crate::model::{AdrId, AdrRecord, RelVerb};

/// A child entry: the verb that the child uses (forward direction) and
/// the child's ADR ID.
#[derive(Debug, Clone)]
pub struct ChildEntry {
    /// The forward verb the child used (e.g., `References`).
    pub verb: RelVerb,
    /// The child ADR ID.
    pub child: AdrId,
}

/// Compute a children index by inverting all forward relationships.
///
/// For each forward link `A -[verb]→ B`, inserts `B → (verb, A)`.
/// Root self-references are skipped (they are tree markers, not edges).
#[must_use]
pub fn compute_children(records: &[AdrRecord]) -> HashMap<AdrId, Vec<ChildEntry>> {
    let mut children: HashMap<AdrId, Vec<ChildEntry>> = HashMap::new();

    for record in records {
        for rel in &record.relationships {
            // Skip legacy reverse verbs and Root self-references
            if rel.verb.is_reverse() {
                continue;
            }
            if rel.verb == RelVerb::Root && rel.target == record.id {
                continue;
            }

            children
                .entry(rel.target.clone())
                .or_default()
                .push(ChildEntry {
                    verb: rel.verb,
                    child: record.id.clone(),
                });
        }
    }

    // Sort children by ID for stable output
    for entries in children.values_mut() {
        entries.sort_by(|a, b| {
            a.child
                .prefix
                .cmp(&b.child.prefix)
                .then(a.child.number.cmp(&b.child.number))
        });
    }

    children
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AdrId, Relationship, Status, Tier};
    use std::path::PathBuf;

    fn make_id(prefix: &str, num: u16) -> AdrId {
        AdrId {
            prefix: prefix.into(),
            number: num,
        }
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
            file_path: PathBuf::from(format!("docs/adr/test/{prefix}-{num:04}-test.md")),
            title: Some(format!("Test {prefix}-{num:04}")),
            title_line: 1,
            date: Some("2026-04-25".into()),
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
    fn references_produces_child_entry() {
        let records = vec![
            make_record("CHE", 2, vec![(RelVerb::References, make_id("CHE", 1))]),
            make_record("CHE", 1, vec![(RelVerb::Root, make_id("CHE", 1))]),
        ];
        let children = compute_children(&records);

        let che1_children = children.get(&make_id("CHE", 1)).unwrap();
        assert_eq!(che1_children.len(), 1);
        assert_eq!(che1_children[0].child, make_id("CHE", 2));
        assert_eq!(che1_children[0].verb, RelVerb::References);
    }

    #[test]
    fn root_self_reference_not_child() {
        let records = vec![make_record(
            "CHE",
            1,
            vec![(RelVerb::Root, make_id("CHE", 1))],
        )];
        let children = compute_children(&records);
        // Root self-reference should NOT create a child entry
        assert!(
            children.is_empty(),
            "Root self-ref should not produce children"
        );
    }

    #[test]
    fn reverse_verbs_are_skipped() {
        let records = vec![
            make_record("CHE", 1, vec![(RelVerb::Informs, make_id("CHE", 2))]),
            make_record("CHE", 2, vec![]),
        ];
        let children = compute_children(&records);
        assert!(
            children.is_empty(),
            "reverse verb should not produce children"
        );
    }

    #[test]
    fn empty_records_produce_empty_children() {
        let records: Vec<AdrRecord> = vec![];
        let children = compute_children(&records);
        assert!(children.is_empty());
    }

    #[test]
    fn multiple_children_sorted_by_id() {
        let records = vec![
            make_record("CHE", 3, vec![(RelVerb::References, make_id("CHE", 1))]),
            make_record("CHE", 2, vec![(RelVerb::References, make_id("CHE", 1))]),
            make_record("CHE", 5, vec![(RelVerb::References, make_id("CHE", 1))]),
            make_record("CHE", 1, vec![(RelVerb::Root, make_id("CHE", 1))]),
        ];
        let children = compute_children(&records);
        let che1 = children.get(&make_id("CHE", 1)).unwrap();
        assert_eq!(che1.len(), 3);
        assert_eq!(che1[0].child.number, 2);
        assert_eq!(che1[1].child.number, 3);
        assert_eq!(che1[2].child.number, 5);
    }

    #[test]
    fn supersedes_produces_child_entry() {
        let records = vec![
            make_record("CHE", 2, vec![(RelVerb::Supersedes, make_id("CHE", 1))]),
            make_record("CHE", 1, vec![(RelVerb::Root, make_id("CHE", 1))]),
        ];
        let children = compute_children(&records);
        let che1 = children.get(&make_id("CHE", 1)).unwrap();
        assert_eq!(che1.len(), 1);
        assert_eq!(che1[0].verb, RelVerb::Supersedes);
    }

    #[test]
    fn non_self_root_produces_child_entry() {
        // Root: CHE-0005 in CHE-0002's file — semantically invalid (L008
        // will warn) but compute_children should still produce the edge.
        let records = vec![
            make_record("CHE", 2, vec![(RelVerb::Root, make_id("CHE", 5))]),
            make_record("CHE", 5, vec![(RelVerb::Root, make_id("CHE", 5))]),
        ];
        let children = compute_children(&records);
        let che5 = children.get(&make_id("CHE", 5)).unwrap();
        assert_eq!(che5.len(), 1);
        assert_eq!(che5[0].child, make_id("CHE", 2));
        assert_eq!(che5[0].verb, RelVerb::Root);
    }
}
