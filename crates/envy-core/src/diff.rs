//! Key-level diff between two `.env` documents.
//!
//! Values are compared after quote decoding, so `KEY=foo` and `KEY="foo"` are
//! the same. Order follows the left file, then keys that exist only on the right.

use crate::document::EnvDocument;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffKind {
    /// Present in both files with the same decoded value and active/disabled state.
    Same,
    /// Present in both files (both active or both disabled) with different decoded values.
    Different,
    /// Present only in the left file (active).
    OnlyLeft,
    /// Present only in the right file (active). When the right file is
    /// `.env.example`, this is the "missing key" warning.
    OnlyRight,
    /// Present on at least one side as a commented-out (disabled) entry while
    /// the other side is active or absent ("present but off", not missing).
    Disabled,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffRow {
    pub key: String,
    pub kind: DiffKind,
    pub left: Option<String>,
    pub right: Option<String>,
    pub left_disabled: bool,
    pub right_disabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffReport {
    pub rows: Vec<DiffRow>,
}

impl DiffReport {
    pub fn missing_on_left(&self) -> impl Iterator<Item = &DiffRow> {
        self.rows
            .iter()
            .filter(|row| row.kind == DiffKind::OnlyRight)
    }

    pub fn different(&self) -> impl Iterator<Item = &DiffRow> {
        self.rows
            .iter()
            .filter(|row| row.kind == DiffKind::Different)
    }

    pub fn only_left(&self) -> impl Iterator<Item = &DiffRow> {
        self.rows
            .iter()
            .filter(|row| row.kind == DiffKind::OnlyLeft)
    }

    pub fn disabled(&self) -> impl Iterator<Item = &DiffRow> {
        self.rows
            .iter()
            .filter(|row| row.kind == DiffKind::Disabled)
    }

    pub fn has_warnings(&self) -> bool {
        self.rows
            .iter()
            .any(|row| matches!(row.kind, DiffKind::OnlyRight | DiffKind::Different))
    }
}

pub fn diff(left: &EnvDocument, right: &EnvDocument) -> DiffReport {
    let left_snaps = left.resolved_entries();
    let right_snaps = right.resolved_entries();
    let mut rows = Vec::new();
    let mut seen_right = vec![false; right_snaps.len()];

    for left_snap in &left_snaps {
        if let Some(pos) = right_snaps.iter().position(|r| r.key == left_snap.key) {
            seen_right[pos] = true;
            let right_snap = &right_snaps[pos];
            let kind = if left_snap.disabled != right_snap.disabled {
                DiffKind::Disabled
            } else if left_snap.value == right_snap.value {
                DiffKind::Same
            } else {
                DiffKind::Different
            };
            rows.push(DiffRow {
                key: left_snap.key.clone(),
                kind,
                left: Some(left_snap.value.clone()),
                right: Some(right_snap.value.clone()),
                left_disabled: left_snap.disabled,
                right_disabled: right_snap.disabled,
            });
        } else {
            let kind = if left_snap.disabled {
                DiffKind::Disabled
            } else {
                DiffKind::OnlyLeft
            };
            rows.push(DiffRow {
                key: left_snap.key.clone(),
                kind,
                left: Some(left_snap.value.clone()),
                right: None,
                left_disabled: left_snap.disabled,
                right_disabled: false,
            });
        }
    }

    for (pos, right_snap) in right_snaps.iter().enumerate() {
        if seen_right[pos] {
            continue;
        }
        let kind = if right_snap.disabled {
            DiffKind::Disabled
        } else {
            DiffKind::OnlyRight
        };
        rows.push(DiffRow {
            key: right_snap.key.clone(),
            kind,
            left: None,
            right: Some(right_snap.value.clone()),
            left_disabled: false,
            right_disabled: right_snap.disabled,
        });
    }

    DiffReport { rows }
}

/// Keys present in `example` and absent from `file`.
pub fn missing_vs_example(file: &EnvDocument, example: &EnvDocument) -> Vec<String> {
    diff(file, example)
        .missing_on_left()
        .map(|row| row.key.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_missing_different_and_extra() {
        let current = EnvDocument::parse("A=1\nB=changed\nC=only-here\n");
        let example = EnvDocument::parse("# sample\nA=1\nB=original\nD=missing\n");
        let report = diff(&current, &example);
        let kinds: Vec<_> = report
            .rows
            .iter()
            .map(|row| (row.key.as_str(), row.kind))
            .collect();
        assert_eq!(
            kinds,
            vec![
                ("A", DiffKind::Same),
                ("B", DiffKind::Different),
                ("C", DiffKind::OnlyLeft),
                ("D", DiffKind::OnlyRight),
            ]
        );
        assert_eq!(
            missing_vs_example(&current, &example),
            vec!["D".to_string()]
        );
    }

    #[test]
    fn quotes_do_not_count_as_a_difference() {
        let left = EnvDocument::parse("A=foo\n");
        let right = EnvDocument::parse("A=\"foo\"\n");
        let report = diff(&left, &right);
        assert_eq!(report.rows[0].kind, DiffKind::Same);
    }

    #[test]
    fn duplicate_keys_use_the_last_value() {
        let left = EnvDocument::parse("A=1\nA=2\n");
        let right = EnvDocument::parse("A=2\n");
        assert_eq!(diff(&left, &right).rows[0].kind, DiffKind::Same);
    }

    #[test]
    fn disabled_key_is_present_but_off_not_missing() {
        let current = EnvDocument::parse("A=1\n# DEBUG=true\n# BOTH_OFF=1\n");
        let example = EnvDocument::parse("A=1\nDEBUG=true\n# BOTH_OFF=1\n# OPT_ONLY=yes\nREQ=1\n");
        let report = diff(&current, &example);

        let by_key = |k: &str| report.rows.iter().find(|r| r.key == k).unwrap();
        assert_eq!(by_key("A").kind, DiffKind::Same);

        let debug = by_key("DEBUG");
        assert_eq!(debug.kind, DiffKind::Disabled);
        assert!(debug.left_disabled);
        assert!(!debug.right_disabled);
        assert_eq!(debug.left.as_deref(), Some("true"));
        assert_eq!(debug.right.as_deref(), Some("true"));

        let both_off = by_key("BOTH_OFF");
        assert_eq!(both_off.kind, DiffKind::Same);
        assert!(both_off.left_disabled);
        assert!(both_off.right_disabled);

        let opt_only = by_key("OPT_ONLY");
        assert_eq!(opt_only.kind, DiffKind::Disabled);
        assert!(!opt_only.left_disabled);
        assert!(opt_only.right_disabled);

        // Neither DEBUG (present but off) nor OPT_ONLY (off in example) counts as missing!
        assert_eq!(
            missing_vs_example(&current, &example),
            vec!["REQ".to_string()]
        );
    }

    #[test]
    fn copy_across_preserves_disabled_state() {
        let left = EnvDocument::parse("# FEATURE_X=beta\nPORT=3000\n");
        let mut right = EnvDocument::parse("PORT=3000\n");
        let report = diff(&left, &right);
        let row = report.rows.iter().find(|r| r.key == "FEATURE_X").unwrap();
        assert!(row.left_disabled);
        right.upsert_entry(
            row.key.clone(),
            row.left.clone().unwrap(),
            row.left_disabled,
        );
        assert_eq!(right.serialize(), "PORT=3000\n# FEATURE_X=beta\n");

        // Re-diffing now shows both sides disabled and equal (`Same`).
        let after = diff(&left, &right);
        let after_row = after.rows.iter().find(|r| r.key == "FEATURE_X").unwrap();
        assert_eq!(after_row.kind, DiffKind::Same);
        assert!(after_row.left_disabled && after_row.right_disabled);
    }
}
