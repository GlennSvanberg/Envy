//! One single-line field for every place Envy asks for text.
//!
//! Platform text arrives through [`EntityInputHandler`], the same path as
//! GPUI's input example. Key bindings only move the caret and edit the
//! selection. They do not decode characters, so a Swedish layout, dead keys,
//! and IME go through the system.

use std::ops::Range;
use std::time::Duration;

use envy_core::TextField;
use gpui_kit::{
    actions, div, fill, point, px, relative, rgb, size, App, Bounds, ClipboardItem, ContentMask,
    Context, CursorStyle, Element, ElementId, ElementInputHandler, Entity, EntityInputHandler,
    EventEmitter, FocusHandle, Focusable, GlobalElementId, Hsla, InteractiveElement, IntoElement,
    KeyBinding, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad,
    ParentElement, Pixels, Point, Render, ShapedLine, SharedString, Style, Styled, Subscription,
    TextAlign, TextRun, UTF16Selection, UnderlineStyle, Window,
};

actions!(
    envy_text,
    [
        CaretLeft,
        CaretRight,
        CaretWordLeft,
        CaretWordRight,
        CaretSelectLeft,
        CaretSelectRight,
        CaretSelectWordLeft,
        CaretSelectWordRight,
        CaretHome,
        CaretEnd,
        CaretSelectHome,
        CaretSelectEnd,
        CaretLineStart,
        CaretLineEnd,
        CaretSelectLineStart,
        CaretSelectLineEnd,
        CaretSelectAll,
        CaretBackspace,
        CaretDelete,
        CaretDeleteWordBackward,
        CaretDeleteWordForward,
        CaretCopy,
        CaretCut,
        CaretPaste,
        CaretUndo,
        CaretRedo,
        CaretSubmit,
        CaretExitDown,
    ]
);

pub fn bind(cx: &mut App) {
    let field = Some("EnvyText");
    cx.bind_keys([
        KeyBinding::new("left", CaretLeft, field),
        KeyBinding::new("right", CaretRight, field),
        KeyBinding::new("ctrl-left", CaretWordLeft, field),
        KeyBinding::new("ctrl-right", CaretWordRight, field),
        KeyBinding::new("shift-left", CaretSelectLeft, field),
        KeyBinding::new("shift-right", CaretSelectRight, field),
        KeyBinding::new("ctrl-shift-left", CaretSelectWordLeft, field),
        KeyBinding::new("ctrl-shift-right", CaretSelectWordRight, field),
        KeyBinding::new("home", CaretHome, field),
        KeyBinding::new("end", CaretEnd, field),
        KeyBinding::new("shift-home", CaretSelectHome, field),
        KeyBinding::new("shift-end", CaretSelectEnd, field),
        KeyBinding::new("up", CaretLineStart, field),
        // Search fields and the new-file name set `EnvyExitDown`, so Down
        // leaves the field instead of moving the caret.
        KeyBinding::new("down", CaretLineEnd, Some("EnvyText && !EnvyExitDown")),
        KeyBinding::new("down", CaretExitDown, Some("EnvyExitDown")),
        KeyBinding::new("shift-up", CaretSelectLineStart, field),
        KeyBinding::new("shift-down", CaretSelectLineEnd, field),
        KeyBinding::new("pageup", CaretLineStart, field),
        KeyBinding::new("pagedown", CaretLineEnd, field),
        KeyBinding::new("shift-pageup", CaretSelectLineStart, field),
        KeyBinding::new("shift-pagedown", CaretSelectLineEnd, field),
        KeyBinding::new("ctrl-a", CaretSelectAll, field),
        KeyBinding::new("backspace", CaretBackspace, field),
        KeyBinding::new("delete", CaretDelete, field),
        KeyBinding::new("ctrl-backspace", CaretDeleteWordBackward, field),
        KeyBinding::new("ctrl-delete", CaretDeleteWordForward, field),
        KeyBinding::new("ctrl-c", CaretCopy, field),
        KeyBinding::new("ctrl-x", CaretCut, field),
        KeyBinding::new("ctrl-v", CaretPaste, field),
        KeyBinding::new("ctrl-z", CaretUndo, field),
        KeyBinding::new("ctrl-y", CaretRedo, field),
        KeyBinding::new("ctrl-shift-z", CaretRedo, field),
        // Windows reports the main Return key and numpad Enter as the same
        // name, "enter" (both are VK_RETURN). Linux names Return "enter" and
        // strips the keypad prefix, so numpad Enter is "enter" there too.
        // The extra names cover a platform that keeps the keypad prefix.
        KeyBinding::new("enter", CaretSubmit, field),
        KeyBinding::new("kp_enter", CaretSubmit, field),
        KeyBinding::new("numpadenter", CaretSubmit, field),
    ]);
}

#[derive(Clone, Debug)]
pub enum TextInputEvent {
    Change,
    Submit,
    Focus,
    Blur,
    /// Down should leave this field. The caret stays where it is.
    ExitDown,
}

pub struct TextInput {
    focus_handle: FocusHandle,
    field: TextField,
    placeholder: SharedString,
    accent: Hsla,
    marked_range: Option<Range<usize>>,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    scroll: Pixels,
    selecting: bool,
    focused: bool,
    caret_on: bool,
    caret_epoch: u64,
    focus_wired: bool,
    /// Down emits [`TextInputEvent::ExitDown`] instead of moving to the line end.
    exit_down: bool,
    _subs: Vec<Subscription>,
}

impl TextInput {
    pub fn new(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            field: TextField::new(),
            placeholder: placeholder.into(),
            accent: rgb(0x34D399).into(),
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            scroll: px(0.),
            selecting: false,
            focused: false,
            caret_on: true,
            caret_epoch: 0,
            focus_wired: false,
            exit_down: false,
            _subs: Vec::new(),
        }
    }

    pub fn exit_down(mut self) -> Self {
        self.exit_down = true;
        self
    }

    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.field.set_text(text);
        self
    }

    pub fn value(&self) -> &str {
        self.field.text()
    }

    pub fn is_empty(&self) -> bool {
        self.field.text().is_empty()
    }

    pub fn set_accent(&mut self, accent: Hsla) {
        self.accent = accent;
    }

    pub fn set_value(
        &mut self,
        text: impl Into<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.field.set_text(text);
        self.marked_range = None;
        self.kick_caret(cx);
        window.notify_text_input_changed(&self.focus_handle);
        cx.emit(TextInputEvent::Change);
        cx.notify();
    }

    pub fn select_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.field.select_all();
        self.kick_caret(cx);
        window.notify_text_selection_changed(&self.focus_handle);
        cx.notify();
    }

    fn edited(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.marked_range = None;
        self.kick_caret(cx);
        window.notify_text_input_changed(&self.focus_handle);
        cx.emit(TextInputEvent::Change);
        cx.notify();
    }

    fn moved(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.kick_caret(cx);
        window.notify_text_selection_changed(&self.focus_handle);
        cx.notify();
    }

    fn kick_caret(&mut self, cx: &mut Context<Self>) {
        self.caret_on = true;
        self.caret_epoch = self.caret_epoch.wrapping_add(1);
        let epoch = self.caret_epoch;
        cx.spawn(async move |this, async_cx| loop {
            async_cx
                .background_executor()
                .timer(Duration::from_millis(530))
                .await;
            let go = this
                .update(async_cx, |input, cx| {
                    if input.caret_epoch != epoch || !input.focused {
                        return false;
                    }
                    input.caret_on = !input.caret_on;
                    cx.notify();
                    true
                })
                .unwrap_or(false);
            if !go {
                break;
            }
        })
        .detach();
    }

    fn ensure_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.focus_wired {
            return;
        }
        self.focus_wired = true;
        let handle = self.focus_handle.clone();
        self._subs.push(cx.on_focus(&handle, window, |this, _, cx| {
            if this.focused {
                return;
            }
            this.focused = true;
            this.kick_caret(cx);
            cx.emit(TextInputEvent::Focus);
            cx.notify();
        }));
        self._subs.push(cx.on_blur(&handle, window, |this, _, cx| {
            this.focused = false;
            this.selecting = false;
            this.caret_epoch = this.caret_epoch.wrapping_add(1);
            this.caret_on = false;
            cx.emit(TextInputEvent::Blur);
            cx.notify();
        }));
        if !self.focused && self.focus_handle.is_focused(window) {
            self.focused = true;
            self.kick_caret(cx);
        }
    }

    fn left(&mut self, _: &CaretLeft, window: &mut Window, cx: &mut Context<Self>) {
        self.field.move_left(false);
        self.moved(window, cx);
    }

    fn right(&mut self, _: &CaretRight, window: &mut Window, cx: &mut Context<Self>) {
        self.field.move_right(false);
        self.moved(window, cx);
    }

    fn word_left(&mut self, _: &CaretWordLeft, window: &mut Window, cx: &mut Context<Self>) {
        self.field.move_word_left(false);
        self.moved(window, cx);
    }

    fn word_right(&mut self, _: &CaretWordRight, window: &mut Window, cx: &mut Context<Self>) {
        self.field.move_word_right(false);
        self.moved(window, cx);
    }

    fn select_left(&mut self, _: &CaretSelectLeft, window: &mut Window, cx: &mut Context<Self>) {
        self.field.move_left(true);
        self.moved(window, cx);
    }

    fn select_right(&mut self, _: &CaretSelectRight, window: &mut Window, cx: &mut Context<Self>) {
        self.field.move_right(true);
        self.moved(window, cx);
    }

    fn select_word_left(
        &mut self,
        _: &CaretSelectWordLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.field.move_word_left(true);
        self.moved(window, cx);
    }

    fn select_word_right(
        &mut self,
        _: &CaretSelectWordRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.field.move_word_right(true);
        self.moved(window, cx);
    }

    fn home(&mut self, _: &CaretHome, window: &mut Window, cx: &mut Context<Self>) {
        self.field.home(false);
        self.moved(window, cx);
    }

    fn end(&mut self, _: &CaretEnd, window: &mut Window, cx: &mut Context<Self>) {
        self.field.end(false);
        self.moved(window, cx);
    }

    fn select_home(&mut self, _: &CaretSelectHome, window: &mut Window, cx: &mut Context<Self>) {
        self.field.home(true);
        self.moved(window, cx);
    }

    fn select_end(&mut self, _: &CaretSelectEnd, window: &mut Window, cx: &mut Context<Self>) {
        self.field.end(true);
        self.moved(window, cx);
    }

    fn line_start(&mut self, _: &CaretLineStart, window: &mut Window, cx: &mut Context<Self>) {
        self.field.home(false);
        self.moved(window, cx);
    }

    fn line_end(&mut self, _: &CaretLineEnd, window: &mut Window, cx: &mut Context<Self>) {
        self.field.end(false);
        self.moved(window, cx);
    }

    fn leave_on_down(&mut self, _: &CaretExitDown, _window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(TextInputEvent::ExitDown);
        cx.stop_propagation();
    }

    fn select_line_start(
        &mut self,
        _: &CaretSelectLineStart,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.field.home(true);
        self.moved(window, cx);
    }

    fn select_line_end(
        &mut self,
        _: &CaretSelectLineEnd,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.field.end(true);
        self.moved(window, cx);
    }

    fn select_all_action(
        &mut self,
        _: &CaretSelectAll,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_all(window, cx);
    }

    fn backspace(&mut self, _: &CaretBackspace, window: &mut Window, cx: &mut Context<Self>) {
        self.field.backspace();
        self.edited(window, cx);
    }

    fn delete(&mut self, _: &CaretDelete, window: &mut Window, cx: &mut Context<Self>) {
        self.field.delete();
        self.edited(window, cx);
    }

    fn delete_word_back(
        &mut self,
        _: &CaretDeleteWordBackward,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.field.delete_word_backward();
        self.edited(window, cx);
    }

    fn delete_word_forward(
        &mut self,
        _: &CaretDeleteWordForward,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.field.delete_word_forward();
        self.edited(window, cx);
    }

    fn copy(&mut self, _: &CaretCopy, _: &mut Window, cx: &mut Context<Self>) {
        let selected = self.field.selected_text();
        if !selected.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(selected.to_string()));
        }
    }

    fn cut(&mut self, _: &CaretCut, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.field.selected_text().to_string();
        if selected.is_empty() {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(selected));
        self.field.replace_range(self.field.selection(), "");
        self.edited(window, cx);
    }

    fn paste(&mut self, _: &CaretPaste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.insert_text(&text, window, cx);
        }
    }

    fn undo(&mut self, _: &CaretUndo, window: &mut Window, cx: &mut Context<Self>) {
        if self.field.undo() {
            self.edited(window, cx);
        }
    }

    fn redo(&mut self, _: &CaretRedo, window: &mut Window, cx: &mut Context<Self>) {
        if self.field.redo() {
            self.edited(window, cx);
        }
    }

    fn submit(&mut self, _: &CaretSubmit, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(TextInputEvent::Submit);
        cx.stop_propagation();
    }

    fn insert_text(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let before = self.field.text().to_string();
        self.field.insert(text);
        if self.field.text() != before {
            self.edited(window, cx);
        }
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle, cx);
        window.request_virtual_keyboard();
        cx.stop_propagation();
        let index = self.index_for_mouse(event.position);
        if event.click_count >= 3 {
            self.field.select_all();
            self.selecting = false;
        } else if event.click_count == 2 {
            self.field.select_word_at(index);
            self.selecting = false;
        } else if event.modifiers.shift {
            self.field.move_to(index, true);
            self.selecting = true;
        } else {
            self.field.move_to(index, false);
            self.selecting = true;
        }
        self.moved(window, cx);
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.selecting = false;
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selecting {
            self.field
                .move_to(self.index_for_mouse(event.position), true);
            self.moved(window, cx);
        }
    }

    fn index_for_mouse(&self, position: Point<Pixels>) -> usize {
        if self.field.text().is_empty() {
            return 0;
        }
        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return self.field.caret();
        };
        if line.text.as_ref() != self.field.text() {
            return self.field.caret();
        }
        let local = position.x - bounds.left() + self.scroll;
        line.closest_index_for_x(local)
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.field.offset_to_utf16(range.start)..self.field.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.field.offset_from_utf16(range.start)..self.field.offset_from_utf16(range.end)
    }
}

impl EventEmitter<TextInputEvent> for TextInput {}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.field.text()[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.field.selection()),
            reversed: self.field.selection_reversed(),
        })
    }

    fn set_selected_text_range(
        &mut self,
        range_utf16: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.range_from_utf16(&range_utf16);
        self.field.move_to(range.start, false);
        self.field.move_to(range.end, true);
        self.moved(window, cx);
    }

    fn text_length_utf16(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.field.text().encode_utf16().count())
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.marked_range = None;
        window.notify_text_input_changed(&self.focus_handle);
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or(self.marked_range.clone())
            .unwrap_or_else(|| self.field.selection());
        let before = self.field.text().to_string();
        self.field.replace_range(range, new_text);
        self.marked_range = None;
        if self.field.text() != before {
            self.edited(window, cx);
        } else {
            self.moved(window, cx);
        }
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or(self.marked_range.clone())
            .unwrap_or_else(|| self.field.selection());
        let line = envy_core::first_line(new_text);
        self.field.replace_range(range.clone(), line);
        if !line.is_empty() {
            self.marked_range = Some(range.start..range.start + line.len());
        } else {
            self.marked_range = None;
        }
        if let Some(selected) = new_selected_range_utf16 {
            let insertion = self.field.offset_to_utf16(range.start);
            let inserted = line.encode_utf16().count();
            let start = insertion + selected.start.min(inserted);
            let end = insertion + selected.end.min(inserted);
            let utf8 = self.range_from_utf16(&(start..end));
            self.field.move_to(utf8.start, false);
            self.field.move_to(utf8.end, true);
        }
        self.kick_caret(cx);
        window.notify_text_input_changed(&self.focus_handle);
        cx.emit(TextInputEvent::Change);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let line = self.last_layout.as_ref()?;
        if line.text.as_ref() != self.field.text() {
            return None;
        }
        let range = self.range_from_utf16(&range_utf16);
        let left = bounds.left() + line.x_for_index(range.start) - self.scroll + px(1.);
        let right = bounds.left() + line.x_for_index(range.end) - self.scroll + px(1.);
        Some(Bounds::from_corners(
            point(left, bounds.top()),
            point(right, bounds.bottom()),
        ))
    }

    fn selection_bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Bounds<Pixels>> {
        self.bounds_for_range(range_utf16, bounds, window, cx)
            .into_iter()
            .collect()
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.field.offset_to_utf16(self.index_for_mouse(point)))
    }

    fn paste(&mut self, item: ClipboardItem, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = item.text() {
            self.insert_text(&text, window, cx);
        }
    }
}

struct TextElement {
    input: Entity<TextInput>,
}

struct PrepaintState {
    line: Option<ShapedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
    scroll: Pixels,
    show_caret: bool,
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui_kit::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui_kit::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let content = input.field.text().to_string();
        let selected = input.field.selection();
        let cursor = input.field.caret();
        let placeholder = content.is_empty();
        let display: SharedString = if placeholder {
            input.placeholder.clone()
        } else {
            content.clone().into()
        };
        let style = window.text_style();
        let color = if placeholder {
            style.color.opacity(0.45)
        } else {
            style.color
        };
        let run = TextRun {
            len: display.len(),
            font: style.font(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let marked = input.marked_range.clone();
        let accent = input.accent;
        let focused = input.focused;
        let caret_on = input.caret_on;
        let scroll_now = input.scroll;
        let runs = if let Some(marked_range) = marked.as_ref().filter(|_| !placeholder) {
            vec![
                TextRun {
                    len: marked_range.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked_range.end.saturating_sub(marked_range.start),
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: display.len().saturating_sub(marked_range.end),
                    ..run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect()
        } else {
            vec![run]
        };
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(display.clone(), font_size, &runs, None);
        let caret_x = if placeholder {
            px(0.)
        } else {
            line.x_for_index(cursor)
        };
        let width = bounds.size.width;
        let mut scroll = scroll_now;
        if !placeholder {
            if caret_x - scroll > width - px(4.) {
                scroll = caret_x - width + px(12.);
            }
            if caret_x < scroll {
                scroll = caret_x;
            }
            if scroll < px(0.) {
                scroll = px(0.);
            }
        } else {
            scroll = px(0.);
        }
        let origin_x = bounds.left() - scroll;
        let selection = if focused && !placeholder && !selected.is_empty() {
            Some(fill(
                Bounds::from_corners(
                    point(origin_x + line.x_for_index(selected.start), bounds.top()),
                    point(origin_x + line.x_for_index(selected.end), bounds.bottom()),
                ),
                accent.opacity(0.35),
            ))
        } else {
            None
        };
        let cursor_quad = if focused && caret_on {
            Some(fill(
                Bounds::new(
                    point(origin_x + caret_x, bounds.top() + px(3.)),
                    size(
                        px(1.5),
                        (bounds.bottom() - bounds.top() - px(6.)).max(px(1.)),
                    ),
                ),
                accent,
            ))
        } else {
            None
        };
        // Keep hit testing on the real text, not the placeholder.
        let stored = if placeholder {
            window
                .text_system()
                .shape_line(SharedString::from(""), font_size, &[], None)
        } else {
            line.clone()
        };
        self.input.update(cx, |input, _cx| {
            input.last_layout = Some(stored);
            input.last_bounds = Some(bounds);
            input.scroll = scroll;
        });
        PrepaintState {
            line: Some(line),
            cursor: cursor_quad,
            selection,
            scroll,
            show_caret: focused && caret_on,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui_kit::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        let scroll = prepaint.scroll;
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            if let Some(selection) = prepaint.selection.take() {
                window.paint_quad(selection);
            }
            if let Some(line) = prepaint.line.take() {
                let _ = line.paint(
                    point(bounds.left() - scroll, bounds.top()),
                    bounds.size.height,
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                );
            }
            if prepaint.show_caret && focus_handle.is_focused(window) {
                if let Some(cursor) = prepaint.cursor.take() {
                    window.paint_quad(cursor);
                }
            }
        });
    }
}

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_focus(window, cx);
        let entity = cx.entity();
        let mut field = div().size_full().min_w(px(0.)).key_context("EnvyText");
        if self.exit_down {
            field = field.key_context("EnvyExitDown");
        }
        field
            .track_focus(&self.focus_handle)
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::word_left))
            .on_action(cx.listener(Self::word_right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::select_home))
            .on_action(cx.listener(Self::select_end))
            .on_action(cx.listener(Self::line_start))
            .on_action(cx.listener(Self::line_end))
            .on_action(cx.listener(Self::leave_on_down))
            .on_action(cx.listener(Self::select_line_start))
            .on_action(cx.listener(Self::select_line_end))
            .on_action(cx.listener(Self::select_all_action))
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::delete_word_back))
            .on_action(cx.listener(Self::delete_word_forward))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::submit))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .child(TextElement { input: entity })
    }
}
