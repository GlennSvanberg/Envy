//! Which part of a key row a click edits, and how a value field moves.
//!
//! The window calls this so a click on the key name never opens the value,
//! and a click on the value never renames the key. Value motions stay inside
//! a field: the only motion that leaves edit mode is cancel.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowTarget {
    KeyName,
    Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowClick {
    EditKey,
    EditValue,
}

pub fn row_click(target: RowTarget) -> RowClick {
    match target {
        RowTarget::KeyName => RowClick::EditKey,
        RowTarget::Value => RowClick::EditValue,
    }
}

/// What the user did while a value field was open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueMotion {
    /// Tab, Enter, or Down.
    Next,
    /// Shift+Tab or Up.
    Previous,
    /// A click on a row's value.
    To(usize),
    /// Esc. Does not write.
    Cancel,
}

/// Where focus goes after a value motion.
///
/// `focus_row` is `None` only for cancel. Every other motion keeps a value
/// field open, staying on the current row when there is no neighbor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValueRoute {
    pub commit: bool,
    pub focus_row: Option<usize>,
}

pub fn route_value_edit(cursor: usize, len: usize, motion: ValueMotion) -> ValueRoute {
    if len == 0 || matches!(motion, ValueMotion::Cancel) {
        return ValueRoute {
            commit: !matches!(motion, ValueMotion::Cancel),
            focus_row: None,
        };
    }
    let cursor = cursor.min(len - 1);
    let row = match motion {
        ValueMotion::Next => (cursor + 1).min(len - 1),
        ValueMotion::Previous => cursor.saturating_sub(1),
        ValueMotion::To(row) => row.min(len - 1),
        ValueMotion::Cancel => cursor,
    };
    ValueRoute {
        commit: true,
        focus_row: Some(row),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_and_value_clicks_edit_different_fields() {
        assert_eq!(row_click(RowTarget::KeyName), RowClick::EditKey);
        assert_eq!(row_click(RowTarget::Value), RowClick::EditValue);
    }

    #[test]
    fn filling_five_values_never_drops_the_field() {
        let len = 5;
        let mut row = 0;
        for expected in 1..len {
            let route = route_value_edit(row, len, ValueMotion::Next);
            assert!(route.commit);
            assert_eq!(route.focus_row, Some(expected));
            row = route.focus_row.unwrap();
        }
        let stay = route_value_edit(row, len, ValueMotion::Next);
        assert!(stay.commit);
        assert_eq!(stay.focus_row, Some(4));

        assert_eq!(
            route_value_edit(4, len, ValueMotion::Previous).focus_row,
            Some(3)
        );
        assert_eq!(
            route_value_edit(0, len, ValueMotion::Previous).focus_row,
            Some(0)
        );

        let click = route_value_edit(0, len, ValueMotion::To(3));
        assert!(click.commit);
        assert_eq!(click.focus_row, Some(3));

        let cancel = route_value_edit(2, len, ValueMotion::Cancel);
        assert!(!cancel.commit);
        assert_eq!(cancel.focus_row, None);
    }

    #[test]
    fn a_click_past_the_end_lands_on_the_last_value() {
        let route = route_value_edit(1, 5, ValueMotion::To(99));
        assert!(route.commit);
        assert_eq!(route.focus_row, Some(4));
    }
}
