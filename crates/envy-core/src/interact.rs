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

/// Which cell of a key row is being edited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditCell {
    Key,
    Value,
}

/// How the user left a single-line cell.
///
/// Arrows stay inside the field. These motions are the ones that leave it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellMotion {
    /// Tab. Key then value, then the next row's key.
    NextCell,
    /// Shift+Tab. Value then key, then the previous row's value.
    PrevCell,
    /// Enter. Same column, next row.
    NextRow,
    /// Esc. Does not write.
    Cancel,
}

/// Where focus goes after a cell motion.
///
/// `focus_row` is `None` only for cancel. Every other motion keeps a field
/// open, staying on the current cell when there is no neighbor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellRoute {
    pub commit: bool,
    pub focus_row: Option<usize>,
    pub focus_cell: Option<EditCell>,
}

pub fn route_cell_edit(row: usize, len: usize, cell: EditCell, motion: CellMotion) -> CellRoute {
    if len == 0 || matches!(motion, CellMotion::Cancel) {
        return CellRoute {
            commit: !matches!(motion, CellMotion::Cancel),
            focus_row: None,
            focus_cell: None,
        };
    }
    let row = row.min(len - 1);
    let (next_row, next_cell) = match (motion, cell) {
        (CellMotion::Cancel, _) => (row, cell),
        (CellMotion::NextCell, EditCell::Key) => (row, EditCell::Value),
        (CellMotion::NextCell, EditCell::Value) => {
            if row + 1 < len {
                (row + 1, EditCell::Key)
            } else {
                (row, EditCell::Value)
            }
        }
        (CellMotion::PrevCell, EditCell::Value) => (row, EditCell::Key),
        (CellMotion::PrevCell, EditCell::Key) => {
            if row > 0 {
                (row - 1, EditCell::Value)
            } else {
                (row, EditCell::Key)
            }
        }
        (CellMotion::NextRow, cell) => {
            if row + 1 < len {
                (row + 1, cell)
            } else {
                (row, cell)
            }
        }
    };
    CellRoute {
        commit: true,
        focus_row: Some(next_row),
        focus_cell: Some(next_cell),
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
    fn tab_walks_key_then_value_then_the_next_key() {
        let len = 3;
        let key = route_cell_edit(0, len, EditCell::Key, CellMotion::NextCell);
        assert!(key.commit);
        assert_eq!(key.focus_row, Some(0));
        assert_eq!(key.focus_cell, Some(EditCell::Value));

        let value = route_cell_edit(0, len, EditCell::Value, CellMotion::NextCell);
        assert_eq!(value.focus_row, Some(1));
        assert_eq!(value.focus_cell, Some(EditCell::Key));

        let back = route_cell_edit(1, len, EditCell::Key, CellMotion::PrevCell);
        assert_eq!(back.focus_row, Some(0));
        assert_eq!(back.focus_cell, Some(EditCell::Value));

        let stay = route_cell_edit(2, len, EditCell::Value, CellMotion::NextCell);
        assert_eq!(stay.focus_row, Some(2));
        assert_eq!(stay.focus_cell, Some(EditCell::Value));

        let first = route_cell_edit(0, len, EditCell::Key, CellMotion::PrevCell);
        assert_eq!(first.focus_cell, Some(EditCell::Key));
        assert_eq!(first.focus_row, Some(0));
    }

    #[test]
    fn enter_stays_in_the_same_column_and_esc_drops_the_field() {
        let next = route_cell_edit(0, 3, EditCell::Value, CellMotion::NextRow);
        assert!(next.commit);
        assert_eq!(next.focus_row, Some(1));
        assert_eq!(next.focus_cell, Some(EditCell::Value));

        let key_next = route_cell_edit(1, 3, EditCell::Key, CellMotion::NextRow);
        assert_eq!(key_next.focus_row, Some(2));
        assert_eq!(key_next.focus_cell, Some(EditCell::Key));

        let stay = route_cell_edit(2, 3, EditCell::Key, CellMotion::NextRow);
        assert_eq!(stay.focus_row, Some(2));
        assert_eq!(stay.focus_cell, Some(EditCell::Key));

        let cancel = route_cell_edit(1, 3, EditCell::Value, CellMotion::Cancel);
        assert!(!cancel.commit);
        assert_eq!(cancel.focus_row, None);
        assert_eq!(cancel.focus_cell, None);
    }

    #[test]
    fn a_click_past_the_end_lands_on_the_last_value() {
        let route = route_value_edit(1, 5, ValueMotion::To(99));
        assert!(route.commit);
        assert_eq!(route.focus_row, Some(4));
    }
}
