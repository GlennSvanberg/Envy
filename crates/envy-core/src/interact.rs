//! Which part of a key row a click edits.
//!
//! The window calls this so a click on the key name never opens the value,
//! and a click on the value never renames the key.

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_and_value_clicks_edit_different_fields() {
        assert_eq!(row_click(RowTarget::KeyName), RowClick::EditKey);
        assert_eq!(row_click(RowTarget::Value), RowClick::EditValue);
    }
}
