//! Vercel-style paste.
//!
//! The Vercel env UI copies a block of `KEY=value` lines. Dropping that block
//! on an empty row should become several assignments, not one giant value.
//! Nothing is written until the caller applies the plan.

use crate::document::EnvDocument;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PasteChange {
    Add {
        key: String,
        value: String,
    },
    Overwrite {
        key: String,
        previous: String,
        value: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PastePlan {
    pub changes: Vec<PasteChange>,
}

impl PastePlan {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    pub fn added(&self) -> usize {
        self.changes
            .iter()
            .filter(|change| matches!(change, PasteChange::Add { .. }))
            .count()
    }

    pub fn overwritten(&self) -> usize {
        self.changes.len() - self.added()
    }
}

/// Build a plan when `pasted` looks like a block of assignments.
///
/// A single line is left for the normal value editor (returns `None`), so
/// pasting one token into a field does not pop a confirmation. A block is one
/// or more parsed assignments that contain a newline.
pub fn plan_paste(document: &EnvDocument, pasted: &str) -> Option<PastePlan> {
    if !pasted.contains('\n') && !pasted.contains('\r') {
        return None;
    }
    let parsed = EnvDocument::parse(pasted);
    let pairs = parsed.pairs();
    if pairs.is_empty() {
        return None;
    }
    let changes = pairs
        .into_iter()
        .map(|(key, value)| match document.value_of(&key) {
            Some(previous) => PasteChange::Overwrite {
                key,
                previous: previous.to_string(),
                value,
            },
            None => PasteChange::Add { key, value },
        })
        .collect();
    Some(PastePlan { changes })
}

/// Apply a confirmed plan. Existing keys keep their position; new keys are
/// appended in paste order. Duplicate keys in the file are all updated.
pub fn apply_paste(document: &mut EnvDocument, plan: &PastePlan) {
    for change in &plan.changes {
        match change {
            PasteChange::Add { key, value } => document.push_entry(key.clone(), value.clone()),
            PasteChange::Overwrite { key, value, .. } => {
                document.set_value(key, value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vercel_block_splits_into_add_and_overwrite() {
        let doc = EnvDocument::parse("DB_PORT=5432\nKEEP=1\n");
        let pasted = "FOO=bar\nBAZ=qux\nDB_PORT=1\n";
        let plan = plan_paste(&doc, pasted).unwrap();
        assert_eq!(
            plan.changes,
            vec![
                PasteChange::Add {
                    key: "FOO".into(),
                    value: "bar".into(),
                },
                PasteChange::Add {
                    key: "BAZ".into(),
                    value: "qux".into(),
                },
                PasteChange::Overwrite {
                    key: "DB_PORT".into(),
                    previous: "5432".into(),
                    value: "1".into(),
                },
            ]
        );
        let mut doc = doc;
        apply_paste(&mut doc, &plan);
        assert_eq!(doc.serialize(), "DB_PORT=1\nKEEP=1\nFOO=bar\nBAZ=qux\n");
    }

    #[test]
    fn single_line_is_not_a_block() {
        let doc = EnvDocument::parse("A=1\n");
        assert!(plan_paste(&doc, "B=2").is_none());
    }

    #[test]
    fn prose_with_newlines_is_not_a_block() {
        let doc = EnvDocument::parse("A=1\n");
        assert!(plan_paste(&doc, "hello\nthere\n").is_none());
    }

    #[test]
    fn quoted_and_export_lines_in_a_paste() {
        let doc = EnvDocument::parse("# head\nA=1\n");
        let pasted = "export TOKEN=\"a b\"\n# skip me\nA=9\n";
        let plan = plan_paste(&doc, pasted).unwrap();
        assert_eq!(plan.added(), 1);
        assert_eq!(plan.overwritten(), 1);
        let mut doc = doc;
        apply_paste(&mut doc, &plan);
        let out = doc.serialize();
        assert!(out.starts_with("# head\nA=9\n"));
        assert!(out.contains("TOKEN=\"a b\"\n"));
    }

    #[test]
    fn last_duplicate_in_the_paste_wins() {
        let doc = EnvDocument::parse("A=1\n");
        let plan = plan_paste(&doc, "B=1\nB=2\n").unwrap();
        assert_eq!(
            plan.changes,
            vec![PasteChange::Add {
                key: "B".into(),
                value: "2".into(),
            }]
        );
    }
}
