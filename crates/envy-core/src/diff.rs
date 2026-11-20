//! Key-level diff between two `.env` documents.
//!
//! Values are compared after quote decoding, so `KEY=foo` and `KEY="foo"` are
//! the same. Order follows the left file, then keys that exist only on the right.

use crate::document::EnvDocument;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffKind {
    /// Present in both files with the same decoded value.
    Same,
    /// Present in both files with different decoded values.
    Different,
    /// Present only in the left file.
    OnlyLeft,
    /// Present only in the right file. When the right file is `.env.example`,
    /// this is the "missing key" warning.
    OnlyRight,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffRow {
    pub key: String,
    pub kind: DiffKind,
    pub left: Option<String>,
    pub right: Option<String>,
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

    pub fn has_warnings(&self) -> bool {
        self.rows
            .iter()
            .any(|row| matches!(row.kind, DiffKind::OnlyRight | DiffKind::Different))
    }
}

pub fn diff(left: &EnvDocument, right: &EnvDocument) -> DiffReport {
    let left_pairs = left.pairs();
    let right_pairs = right.pairs();
    let mut rows = Vec::new();
    let mut seen_right = vec![false; right_pairs.len()];

    for (key, left_value) in &left_pairs {
        if let Some(pos) = right_pairs.iter().position(|(k, _)| k == key) {
            seen_right[pos] = true;
            let right_value = &right_pairs[pos].1;
            let kind = if left_value == right_value {
                DiffKind::Same
            } else {
                DiffKind::Different
            };
            rows.push(DiffRow {
                key: key.clone(),
                kind,
                left: Some(left_value.clone()),
                right: Some(right_value.clone()),
            });
        } else {
            rows.push(DiffRow {
                key: key.clone(),
                kind: DiffKind::OnlyLeft,
                left: Some(left_value.clone()),
                right: None,
            });
        }
    }

    for (pos, (key, right_value)) in right_pairs.iter().enumerate() {
        if seen_right[pos] {
            continue;
        }
        rows.push(DiffRow {
            key: key.clone(),
            kind: DiffKind::OnlyRight,
            left: None,
            right: Some(right_value.clone()),
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
}
