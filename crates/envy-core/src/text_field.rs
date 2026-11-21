//! Single-line text editing: caret, selection, word jumps, and undo.
//!
//! The window paints this and forwards platform text through GPUI's input
//! handler. Character input is not interpreted from key codes here, so a
//! Swedish layout and dead keys stay intact.

use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

/// The first line of pasted text. A value is one line, so the rest is dropped.
pub fn first_line(text: &str) -> &str {
    let line = text.split('\n').next().unwrap_or("");
    line.strip_suffix('\r').unwrap_or(line)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EditGroup {
    Insert,
    Backward,
    Forward,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Snap {
    text: String,
    anchor: usize,
    caret: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    Word,
    Space,
    Other,
}

/// A single-line buffer. Offsets are UTF-8 bytes on char boundaries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextField {
    text: String,
    anchor: usize,
    caret: usize,
    undo: Vec<Snap>,
    redo: Vec<Snap>,
    group: Option<EditGroup>,
}

impl Default for TextField {
    fn default() -> Self {
        Self::new()
    }
}

impl TextField {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            anchor: 0,
            caret: 0,
            undo: Vec::new(),
            redo: Vec::new(),
            group: None,
        }
    }

    pub fn from_text(text: impl Into<String>) -> Self {
        let mut field = Self::new();
        field.set_text(text);
        field
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn caret(&self) -> usize {
        self.caret
    }

    pub fn selection(&self) -> Range<usize> {
        if self.anchor <= self.caret {
            self.anchor..self.caret
        } else {
            self.caret..self.anchor
        }
    }

    pub fn selection_reversed(&self) -> bool {
        self.caret < self.anchor
    }

    pub fn selected_text(&self) -> &str {
        let range = self.selection();
        &self.text[range]
    }

    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        let end = self.text.len();
        self.anchor = end;
        self.caret = end;
        self.undo.clear();
        self.redo.clear();
        self.group = None;
    }

    pub fn move_left(&mut self, extend: bool) {
        let next = if !extend && self.anchor != self.caret {
            self.selection().start
        } else {
            prev_grapheme(&self.text, self.caret)
        };
        self.place(next, extend);
    }

    pub fn move_right(&mut self, extend: bool) {
        let next = if !extend && self.anchor != self.caret {
            self.selection().end
        } else {
            next_grapheme(&self.text, self.caret)
        };
        self.place(next, extend);
    }

    pub fn move_word_left(&mut self, extend: bool) {
        self.place(prev_word(&self.text, self.caret), extend);
    }

    pub fn move_word_right(&mut self, extend: bool) {
        self.place(next_word(&self.text, self.caret), extend);
    }

    pub fn home(&mut self, extend: bool) {
        self.place(0, extend);
    }

    pub fn end(&mut self, extend: bool) {
        self.place(self.text.len(), extend);
    }

    pub fn select_all(&mut self) {
        self.group = None;
        self.anchor = 0;
        self.caret = self.text.len();
    }

    pub fn select_word_at(&mut self, offset: usize) {
        let (start, end) = word_extent(&self.text, offset);
        self.group = None;
        self.anchor = start;
        self.caret = end;
    }

    /// Move the caret. `extend` keeps the anchor, which is how Shift-click and
    /// a drag grow the selection.
    pub fn move_to(&mut self, offset: usize, extend: bool) {
        self.place(snap(&self.text, offset), extend);
    }

    pub fn insert(&mut self, text: &str) {
        let text = first_line(text);
        let range = self.selection();
        let continuing = self.group == Some(EditGroup::Insert)
            && range.is_empty()
            && range.start == self.caret
            && self.anchor == self.caret;
        if !continuing {
            self.push_undo(EditGroup::Insert);
        }
        self.text.replace_range(range.start..range.end, text);
        let pos = range.start + text.len();
        self.anchor = pos;
        self.caret = pos;
    }

    pub fn replace_range(&mut self, range: Range<usize>, text: &str) {
        let text = first_line(text);
        let start = snap(&self.text, range.start);
        let end = snap(&self.text, range.end.max(start));
        let continuing = self.group == Some(EditGroup::Insert)
            && start == end
            && start == self.caret
            && self.anchor == self.caret;
        if !continuing {
            self.push_undo(EditGroup::Insert);
        }
        self.text.replace_range(start..end, text);
        let pos = start + text.len();
        self.anchor = pos;
        self.caret = pos;
    }

    pub fn backspace(&mut self) {
        if self.anchor != self.caret {
            self.delete_selection(EditGroup::Backward);
            return;
        }
        let prev = prev_grapheme(&self.text, self.caret);
        if prev == self.caret {
            return;
        }
        let continuing = self.group == Some(EditGroup::Backward);
        if !continuing {
            self.push_undo(EditGroup::Backward);
        }
        self.text.replace_range(prev..self.caret, "");
        self.anchor = prev;
        self.caret = prev;
    }

    pub fn delete(&mut self) {
        if self.anchor != self.caret {
            self.delete_selection(EditGroup::Forward);
            return;
        }
        let next = next_grapheme(&self.text, self.caret);
        if next == self.caret {
            return;
        }
        let continuing = self.group == Some(EditGroup::Forward);
        if !continuing {
            self.push_undo(EditGroup::Forward);
        }
        self.text.replace_range(self.caret..next, "");
    }

    pub fn delete_word_backward(&mut self) {
        if self.anchor != self.caret {
            self.delete_selection(EditGroup::Backward);
            return;
        }
        let prev = prev_word(&self.text, self.caret);
        if prev == self.caret {
            return;
        }
        self.push_undo(EditGroup::Backward);
        self.group = None;
        self.text.replace_range(prev..self.caret, "");
        self.anchor = prev;
        self.caret = prev;
    }

    pub fn delete_word_forward(&mut self) {
        if self.anchor != self.caret {
            self.delete_selection(EditGroup::Forward);
            return;
        }
        let next = next_word(&self.text, self.caret);
        if next == self.caret {
            return;
        }
        self.push_undo(EditGroup::Forward);
        self.group = None;
        self.text.replace_range(self.caret..next, "");
    }

    pub fn undo(&mut self) -> bool {
        self.group = None;
        let Some(prev) = self.undo.pop() else {
            return false;
        };
        self.redo.push(self.snap());
        self.restore(prev);
        true
    }

    pub fn redo(&mut self) -> bool {
        self.group = None;
        let Some(next) = self.redo.pop() else {
            return false;
        };
        self.undo.push(self.snap());
        self.restore(next);
        true
    }

    pub fn offset_to_utf16(&self, offset: usize) -> usize {
        let offset = snap(&self.text, offset);
        self.text[..offset].encode_utf16().count()
    }

    pub fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8 = 0;
        let mut utf16 = 0;
        for ch in self.text.chars() {
            if utf16 >= offset {
                break;
            }
            utf16 += ch.len_utf16();
            utf8 += ch.len_utf8();
        }
        utf8
    }

    fn delete_selection(&mut self, group: EditGroup) {
        let range = self.selection();
        if range.is_empty() {
            return;
        }
        self.push_undo(group);
        self.group = None;
        self.text.replace_range(range.clone(), "");
        self.anchor = range.start;
        self.caret = range.start;
    }

    fn place(&mut self, offset: usize, extend: bool) {
        let offset = snap(&self.text, offset);
        self.group = None;
        if extend {
            self.caret = offset;
        } else {
            self.anchor = offset;
            self.caret = offset;
        }
    }

    fn push_undo(&mut self, group: EditGroup) {
        self.undo.push(self.snap());
        self.redo.clear();
        self.group = Some(group);
    }

    fn snap(&self) -> Snap {
        Snap {
            text: self.text.clone(),
            anchor: self.anchor,
            caret: self.caret,
        }
    }

    fn restore(&mut self, snap: Snap) {
        self.text = snap.text;
        self.anchor = snap.anchor;
        self.caret = snap.caret;
    }
}

fn snap(text: &str, offset: usize) -> usize {
    let offset = offset.min(text.len());
    if text.is_char_boundary(offset) {
        return offset;
    }
    let mut index = offset;
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn prev_grapheme(text: &str, offset: usize) -> usize {
    let offset = snap(text, offset);
    text.grapheme_indices(true)
        .map(|(index, _)| index)
        .take_while(|index| *index < offset)
        .last()
        .unwrap_or(0)
}

fn next_grapheme(text: &str, offset: usize) -> usize {
    let offset = snap(text, offset);
    text.grapheme_indices(true)
        .map(|(index, _)| index)
        .find(|index| *index > offset)
        .unwrap_or(text.len())
}

fn class_of(grapheme: &str) -> Class {
    let Some(ch) = grapheme.chars().next() else {
        return Class::Other;
    };
    if ch.is_whitespace() {
        Class::Space
    } else if ch.is_alphanumeric() || ch == '_' {
        Class::Word
    } else {
        Class::Other
    }
}

fn class_at(text: &str, offset: usize) -> Option<Class> {
    let offset = snap(text, offset);
    if offset >= text.len() {
        return None;
    }
    let grapheme = text[offset..].graphemes(true).next().unwrap_or("");
    Some(class_of(grapheme))
}

/// Start of the previous word, after skipping spaces.
fn prev_word(text: &str, offset: usize) -> usize {
    let mut index = snap(text, offset);
    if index == 0 {
        return 0;
    }
    index = prev_grapheme(text, index);
    while index > 0 && class_at(text, index) == Some(Class::Space) {
        index = prev_grapheme(text, index);
    }
    let Some(class) = class_at(text, index) else {
        return 0;
    };
    while index > 0 {
        let prev = prev_grapheme(text, index);
        if class_at(text, prev) != Some(class) || class == Class::Space {
            break;
        }
        index = prev;
    }
    index
}

/// Start of the next word.
fn next_word(text: &str, offset: usize) -> usize {
    let mut index = snap(text, offset);
    if index >= text.len() {
        return text.len();
    }
    let Some(class) = class_at(text, index) else {
        return text.len();
    };
    if class == Class::Space {
        while class_at(text, index) == Some(Class::Space) {
            index = next_grapheme(text, index);
        }
        return index;
    }
    while class_at(text, index) == Some(class) {
        index = next_grapheme(text, index);
    }
    while class_at(text, index) == Some(Class::Space) {
        index = next_grapheme(text, index);
    }
    index
}

fn word_extent(text: &str, offset: usize) -> (usize, usize) {
    if text.is_empty() {
        return (0, 0);
    }
    let mut index = snap(text, offset).min(text.len());
    if index == text.len() {
        index = prev_grapheme(text, index);
    }
    let Some(class) = class_at(text, index) else {
        return (index, index);
    };
    let mut start = index;
    while start > 0 {
        let prev = prev_grapheme(text, start);
        if class_at(text, prev) != Some(class) {
            break;
        }
        start = prev;
    }
    let mut end = index;
    while class_at(text, end) == Some(class) {
        end = next_grapheme(text, end);
    }
    (start, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrows_move_by_grapheme_and_shift_extends() {
        let mut field = TextField::from_text("åäö");
        field.home(false);
        field.move_right(false);
        assert_eq!(field.caret(), "å".len());
        field.move_right(true);
        assert_eq!(field.selection(), "å".len().."åä".len());
        assert_eq!(field.selected_text(), "ä");
        field.move_left(true);
        assert!(field.selection().is_empty());
        assert_eq!(field.caret(), "å".len());

        let mut emoji = TextField::from_text("a🙂b");
        emoji.end(false);
        emoji.move_left(false);
        assert_eq!(emoji.caret(), "a🙂".len());
        emoji.move_left(false);
        assert_eq!(&emoji.text()[..emoji.caret()], "a");
    }

    #[test]
    fn dead_key_letter_is_one_caret_step() {
        let mut field = TextField::from_text("e\u{0301}");
        field.home(false);
        field.move_right(false);
        assert_eq!(field.caret(), field.text().len());
        assert_eq!(field.text(), "e\u{0301}");
    }

    #[test]
    fn word_jumps_skip_spaces_and_keep_underscores() {
        let mut field = TextField::from_text("hello world");
        field.home(false);
        field.move_word_right(false);
        assert_eq!(field.caret(), 6);
        field.move_word_right(false);
        assert_eq!(field.caret(), 11);
        field.move_word_left(false);
        assert_eq!(field.caret(), 6);
        field.move_word_left(false);
        assert_eq!(field.caret(), 0);

        let mut key = TextField::from_text("FOO_BAR=baz");
        key.home(false);
        key.move_word_right(false);
        assert_eq!(&key.text()[..key.caret()], "FOO_BAR");
        key.move_word_right(true);
        assert_eq!(key.selected_text(), "=");
        key.move_word_right(true);
        assert_eq!(key.selected_text(), "=baz");
    }

    #[test]
    fn home_end_and_word_selection() {
        let mut field = TextField::from_text("one two");
        field.home(false);
        field.end(true);
        assert_eq!(field.selected_text(), "one two");
        field.select_word_at(1);
        assert_eq!(field.selected_text(), "one");
        field.select_all();
        assert_eq!(field.selection(), 0..7);
    }

    #[test]
    fn typing_replaces_a_selection_and_deletes_by_character_or_word() {
        let mut field = TextField::from_text("hello");
        field.select_all();
        field.insert("y");
        assert_eq!(field.text(), "y");
        field.insert("es");
        assert_eq!(field.text(), "yes");
        field.backspace();
        assert_eq!(field.text(), "ye");
        field.end(false);
        field.insert(" no");
        field.delete_word_backward();
        assert_eq!(field.text(), "ye ");
        field.home(false);
        field.delete_word_forward();
        assert_eq!(field.text(), "");
    }

    #[test]
    fn undo_groups_typing_and_redo_restores_it() {
        let mut field = TextField::from_text("ab");
        field.end(false);
        field.insert("c");
        field.insert("d");
        assert_eq!(field.text(), "abcd");
        assert!(field.undo());
        assert_eq!(field.text(), "ab");
        assert_eq!(field.caret(), 2);
        assert!(field.redo());
        assert_eq!(field.text(), "abcd");
        assert!(!field.redo());

        field.move_left(false);
        field.backspace();
        field.backspace();
        assert_eq!(field.text(), "ad");
        assert!(field.undo());
        assert_eq!(field.text(), "abcd");
    }

    #[test]
    fn a_move_splits_the_undo_group() {
        let mut field = TextField::new();
        field.insert("ab");
        field.move_left(false);
        field.insert("X");
        assert_eq!(field.text(), "aXb");
        assert!(field.undo());
        assert_eq!(field.text(), "ab");
        assert!(field.undo());
        assert_eq!(field.text(), "");
    }

    #[test]
    fn multiline_paste_keeps_the_first_line() {
        assert_eq!(first_line("one\r\ntwo"), "one");
        assert_eq!(first_line("only"), "only");
        let mut field = TextField::from_text("key");
        field.end(false);
        field.insert("=a\nb");
        assert_eq!(field.text(), "key=a");
    }

    #[test]
    fn utf16_offsets_round_trip_for_swedish_letters_and_emoji() {
        let field = TextField::from_text("å🙂");
        assert_eq!(field.offset_to_utf16(0), 0);
        assert_eq!(field.offset_to_utf16("å".len()), 1);
        assert_eq!(field.offset_to_utf16(field.text().len()), 3);
        assert_eq!(field.offset_from_utf16(1), "å".len());
        assert_eq!(field.offset_from_utf16(3), field.text().len());
    }
}
