//! Where a label's glyphs sit, in display and while that label is being edited.
//!
//! The window uses these insets for both modes. Editing adds a caret and a
//! ring around the glyphs. It does not add padding, so the text stays put.

/// 1px row border. The border is present in both modes, so it does not move the text.
pub const ROW_BORDER_PX: f32 = 1.;
/// Horizontal padding inside a key row.
pub const ROW_PAD_X_PX: f32 = 7.;
/// Key row height. Editing uses the same height.
pub const KEY_ROW_H_PX: f32 = 32.;
/// On/off control. It keeps its width while a key is being edited.
pub const KEY_TOGGLE_W_PX: f32 = 24.;
/// Gap between the toggle and the key name.
pub const KEY_CELL_GAP_PX: f32 = 6.;
/// Right padding of the key column.
pub const KEY_CELL_PAD_RIGHT_PX: f32 = 14.;
/// Extra left padding on a related (disabled twin) row.
pub const KEY_RELATED_PAD_PX: f32 = 10.;
/// Corner-down icon in front of a related row.
pub const KEY_RELATED_ICON_PX: f32 = 11.;
/// Key and value type size.
pub const KEY_TEXT_PX: f32 = 13.;
/// Key and value line height.
pub const KEY_LINE_PX: f32 = 18.;
/// Right padding of the value column.
pub const VALUE_PAD_RIGHT_PX: f32 = 8.;
/// File-tab height.
pub const TAB_H_PX: f32 = 31.;
/// Horizontal padding inside a file tab.
pub const TAB_PAD_X_PX: f32 = 12.;
/// Gap between a production dot and the file name.
pub const TAB_GAP_PX: f32 = 6.;
/// File-name type size.
pub const TAB_TEXT_PX: f32 = 12.5;
/// File-name line height.
pub const TAB_LINE_PX: f32 = 16.;
/// Production dot on a file tab.
pub const TAB_DOT_PX: f32 = 6.;

/// Glyph origin inside its row or tab, in pixels from the row's outer left and top.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextOrigin {
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub line: f32,
}

/// Key name while the row is showing the label.
pub fn display_key_origin(related: bool) -> TextOrigin {
    key_origin(related)
}

/// Key name while the row is showing the field. Same slots as the label.
pub fn edit_key_origin(related: bool) -> TextOrigin {
    key_origin(related)
}

/// Value while the row is showing the label. `key_column` is the key column width.
pub fn display_value_origin(key_column: f32) -> TextOrigin {
    value_origin(key_column)
}

/// Value while the row is showing the field.
pub fn edit_value_origin(key_column: f32) -> TextOrigin {
    value_origin(key_column)
}

/// File name on a tab. `prod_dot` is the amber mark in front of a production file.
pub fn display_filename_origin(prod_dot: bool) -> TextOrigin {
    filename_origin(prod_dot)
}

/// File name while that tab is being renamed.
pub fn edit_filename_origin(prod_dot: bool) -> TextOrigin {
    filename_origin(prod_dot)
}

fn key_origin(related: bool) -> TextOrigin {
    let mut x = ROW_BORDER_PX + ROW_PAD_X_PX;
    if related {
        x += KEY_RELATED_PAD_PX + KEY_RELATED_ICON_PX + KEY_CELL_GAP_PX;
    }
    x += KEY_TOGGLE_W_PX + KEY_CELL_GAP_PX;
    TextOrigin {
        x,
        y: centered_line(KEY_ROW_H_PX, KEY_LINE_PX),
        size: KEY_TEXT_PX,
        line: KEY_LINE_PX,
    }
}

fn value_origin(key_column: f32) -> TextOrigin {
    TextOrigin {
        x: ROW_BORDER_PX + ROW_PAD_X_PX + key_column,
        y: centered_line(KEY_ROW_H_PX, KEY_LINE_PX),
        size: KEY_TEXT_PX,
        line: KEY_LINE_PX,
    }
}

fn filename_origin(prod_dot: bool) -> TextOrigin {
    let mut x = TAB_PAD_X_PX;
    if prod_dot {
        x += TAB_DOT_PX + TAB_GAP_PX;
    }
    TextOrigin {
        x,
        y: (TAB_H_PX - TAB_LINE_PX) / 2.,
        size: TAB_TEXT_PX,
        line: TAB_LINE_PX,
    }
}

fn centered_line(row_h: f32, line: f32) -> f32 {
    let inner = row_h - (2. * ROW_BORDER_PX);
    ROW_BORDER_PX + (inner - line) / 2.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editing_does_not_move_the_glyphs() {
        for related in [false, true] {
            assert_eq!(display_key_origin(related), edit_key_origin(related));
        }
        for column in [160., 220., 560.] {
            assert_eq!(display_value_origin(column), edit_value_origin(column));
        }
        for prod in [false, true] {
            assert_eq!(display_filename_origin(prod), edit_filename_origin(prod));
        }
    }

    #[test]
    fn dropping_the_toggle_is_what_used_to_shift_the_key() {
        let shown = display_key_origin(false);
        let jumped = ROW_BORDER_PX + ROW_PAD_X_PX;
        let delta = shown.x - jumped;
        assert!((delta - (KEY_TOGGLE_W_PX + KEY_CELL_GAP_PX)).abs() < f32::EPSILON);
        assert_eq!(shown.size, edit_key_origin(false).size);
        assert_eq!(shown.line, edit_key_origin(false).line);
        assert_eq!(shown.y, edit_key_origin(false).y);
    }
}
