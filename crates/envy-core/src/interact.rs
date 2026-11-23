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
    /// Enter. Commit and close. The row stays selected.
    NextRow,
    /// Esc. Does not write.
    Cancel,
}

/// Where focus goes after a cell motion.
///
/// `focus_cell` is `None` when the editor closes. Cancel also clears the row.
/// Tab keeps a field open, staying on the current cell when there is no neighbor.
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
        (CellMotion::NextRow, _) => {
            return CellRoute {
                commit: true,
                focus_row: Some(row),
                focus_cell: None,
            };
        }
    };
    CellRoute {
        commit: true,
        focus_row: Some(next_row),
        focus_cell: Some(next_cell),
    }
}

/// What Enter does while a key list is open.
///
/// A click chooses a suggestion. An open list with nothing chosen does not
/// take Enter; Enter commits the typed text and closes the field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnterEffect {
    CommitAndClose,
    AcceptSuggestion(usize),
}

pub fn enter_while_suggesting(chosen: Option<usize>) -> EnterEffect {
    match chosen {
        Some(index) => EnterEffect::AcceptSuggestion(index),
        None => EnterEffect::CommitAndClose,
    }
}

/// Windows names both Return keys `"enter"`.
///
/// GPUI's Windows mapper (`parse_immutable`) sends `VK_RETURN` (0x0D) as
/// `"enter"`. That virtual key is the main Return key and the numpad Enter;
/// the extended-key bit is not part of the name. `"kp_enter"` is accepted for
/// platforms that label the numpad key on its own.
/// File-tab menu length: Rename, Duplicate, Reveal in Explorer, Delete file.
pub const TAB_MENU_LEN: usize = 4;

/// Move `current` by `delta` and stay inside `0..len`.
pub fn step_index(current: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    (current as isize + delta).clamp(0, len as isize - 1) as usize
}

/// Row to land on when Down leaves a search field.
///
/// The marked row wins when it is still visible. Otherwise the first hit.
/// An empty list has nowhere to land.
pub fn search_landing(selected: Option<usize>, visible: &[usize]) -> Option<usize> {
    selected
        .filter(|index| visible.contains(index))
        .or_else(|| visible.first().copied())
}

/// Row to mark while a search field is focused.
///
/// A query marks the first visible hit and does not change which row is open.
/// An empty query keeps the current selection when it is still in the list.
pub fn search_mark(
    query_active: bool,
    selected: Option<usize>,
    visible: &[usize],
) -> Option<usize> {
    if query_active {
        visible.first().copied()
    } else {
        search_landing(selected, visible)
    }
}

pub fn is_cell_commit_key(key: &str) -> bool {
    matches!(key, "enter" | "kp_enter" | "numpadenter")
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
    fn enter_and_numpad_enter_commit_and_close() {
        for key in ["enter", "kp_enter", "numpadenter"] {
            assert!(is_cell_commit_key(key), "{key}");
        }
        assert!(!is_cell_commit_key("tab"));
        assert!(!is_cell_commit_key("return"));

        let key = route_cell_edit(0, 3, EditCell::Key, CellMotion::NextRow);
        assert!(key.commit);
        assert_eq!(key.focus_row, Some(0));
        assert_eq!(key.focus_cell, None);

        let value = route_cell_edit(1, 3, EditCell::Value, CellMotion::NextRow);
        assert!(value.commit);
        assert_eq!(value.focus_row, Some(1));
        assert_eq!(value.focus_cell, None);

        let last = route_cell_edit(2, 3, EditCell::Key, CellMotion::NextRow);
        assert_eq!(last.focus_row, Some(2));
        assert_eq!(last.focus_cell, None);

        let cancel = route_cell_edit(1, 3, EditCell::Value, CellMotion::Cancel);
        assert!(!cancel.commit);
        assert_eq!(cancel.focus_row, None);
        assert_eq!(cancel.focus_cell, None);
    }

    #[test]
    fn an_open_list_does_not_take_enter_until_a_suggestion_is_chosen() {
        assert_eq!(enter_while_suggesting(None), EnterEffect::CommitAndClose);
        assert_eq!(
            enter_while_suggesting(Some(2)),
            EnterEffect::AcceptSuggestion(2)
        );
    }

    #[test]
    fn down_from_search_keeps_the_marked_row_when_it_is_visible() {
        assert_eq!(search_landing(Some(2), &[0, 1, 2]), Some(2));
        assert_eq!(search_landing(None, &[3, 4]), Some(3));
        assert_eq!(search_landing(Some(5), &[0, 1]), Some(0));
        assert_eq!(search_landing(Some(1), &[]), None);
        assert_eq!(search_mark(true, Some(7), &[4, 7, 9]), Some(4));
        assert_eq!(search_mark(false, Some(7), &[4, 7, 9]), Some(7));
        assert_eq!(search_mark(false, Some(1), &[4, 7]), Some(4));
        assert_eq!(search_mark(true, Some(4), &[]), None);
        assert_eq!(step_index(0, TAB_MENU_LEN, -1), 0);
        assert_eq!(step_index(0, TAB_MENU_LEN, 1), 1);
        assert_eq!(
            step_index(TAB_MENU_LEN - 1, TAB_MENU_LEN, 1),
            TAB_MENU_LEN - 1
        );
    }

    #[test]
    fn a_click_past_the_end_lands_on_the_last_value() {
        let route = route_value_edit(1, 5, ValueMotion::To(99));
        assert!(route.commit);
        assert_eq!(route.focus_row, Some(4));
    }
}
