//! Brief confirmation on a copy button.
//!
//! A successful copy crossfades the copy icon into a check, holds, then
//! crossfades back. Each button keeps its own acknowledgement, so two copies
//! do not share one timer.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{Disableable, Icon, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::{
    div, ease_in_out, ease_out_quint, px, size, Animation, AnimationExt, AnyElement, ElementId,
    Hsla, SharedString, Transformation,
};

use crate::theme::Palette;

/// How long the copy icon and the check trade places.
pub const COPY_MORPH: Duration = Duration::from_millis(160);
/// How long the check stays fully visible.
pub const COPY_HOLD: Duration = Duration::from_millis(1500);
/// A short tail so the fade-out can finish before the static icon replaces it.
const COPY_SETTLE: Duration = Duration::from_millis(40);
/// Scale of a glyph at the start of its fade-in. Full size is 1.
const COPY_SCALE_MIN: f32 = 0.82;

/// Toolbar icon button, matching the other 24×24 controls (14px glyph).
const TOOLBAR_BUTTON_PX: f32 = 24.;
const TOOLBAR_ICON_PX: f32 = 14.;
/// Row action, matching the other 20×20 controls (12px glyph).
const ROW_BUTTON_PX: f32 = 20.;
const ROW_ICON_PX: f32 = 12.;

pub const TOOLBAR_COPY_ID: &str = "tb-copy-all";

pub fn row_copy_id(node: usize) -> String {
    format!("copy-{node}")
}

pub fn copy_ack_lifetime() -> Duration {
    COPY_MORPH + COPY_HOLD + COPY_MORPH + COPY_SETTLE
}

/// How visible the check is for one step of the fade / hold / fade sequence.
/// `delta` is already eased, in `0..=1`.
pub fn check_visibility(index: usize, delta: f32) -> f32 {
    let delta = delta.clamp(0.0, 1.0);
    match index {
        0 => delta,
        1 => 1.0,
        _ => 1.0 - delta,
    }
}

pub fn icon_scale(visibility: f32) -> f32 {
    let visibility = visibility.clamp(0.0, 1.0);
    COPY_SCALE_MIN + (1.0 - COPY_SCALE_MIN) * visibility
}

fn glyph_visibility(appear: bool, index: usize, delta: f32) -> f32 {
    let check = check_visibility(index, delta);
    if appear {
        check
    } else {
        1.0 - check
    }
}

fn copy_animations() -> Vec<Animation> {
    vec![
        Animation::new(COPY_MORPH).with_easing(ease_out_quint()),
        Animation::new(COPY_HOLD),
        Animation::new(COPY_MORPH).with_easing(ease_in_out),
    ]
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CopyButtonKind {
    Toolbar,
    Row,
}

impl CopyButtonKind {
    fn button_px(self) -> f32 {
        match self {
            Self::Toolbar => TOOLBAR_BUTTON_PX,
            Self::Row => ROW_BUTTON_PX,
        }
    }

    fn icon_px(self) -> f32 {
        match self {
            Self::Toolbar => TOOLBAR_ICON_PX,
            Self::Row => ROW_ICON_PX,
        }
    }

    fn radius(self) -> f32 {
        match self {
            Self::Toolbar => 6.,
            Self::Row => 4.,
        }
    }
}

/// One successful copy, still on screen.
pub struct CopyAck {
    pub generation: u64,
    file: PathBuf,
}

#[derive(Default)]
pub struct CopyAckBook {
    entries: HashMap<String, CopyAck>,
    next_generation: u64,
}

impl CopyAckBook {
    /// Starts (or restarts) the confirmation for `id`. Returns the generation
    /// a later [`Self::expire`] must match, so an older timer cannot clear a
    /// newer copy.
    pub fn acknowledge(&mut self, id: &str, file: PathBuf) -> u64 {
        self.next_generation = self.next_generation.wrapping_add(1);
        let generation = self.next_generation;
        self.entries
            .insert(id.to_string(), CopyAck { generation, file });
        generation
    }

    pub fn visible(&self, id: &str, file: &Path) -> Option<&CopyAck> {
        let ack = self.entries.get(id)?;
        (ack.file.as_path() == file).then_some(ack)
    }

    /// Drops the acknowledgement when `generation` is still current.
    pub fn expire(&mut self, id: &str, generation: u64) -> bool {
        match self.entries.get(id) {
            Some(ack) if ack.generation == generation => {
                self.entries.remove(id);
                true
            }
            _ => false,
        }
    }
}

/// Icon button that shows a check after its own copy succeeds.
///
/// The glyph is a child, which would otherwise pad the control like a text
/// button. The square size is pinned so it stays an icon button.
pub fn copy_icon_button(
    id: impl Into<SharedString>,
    tip: impl Into<SharedString>,
    kind: CopyButtonKind,
    ack: Option<&CopyAck>,
    disabled: bool,
    reduce_motion: bool,
    p: Palette,
) -> Button {
    let id = id.into();
    let button_px = kind.button_px();
    let mut button = Button::new(id.clone())
        .ghost()
        .compact()
        .tooltip(tip)
        .disabled(disabled)
        .rounded(px(kind.radius()))
        .size(px(button_px))
        .min_w(px(button_px))
        .p(px(0.))
        .child(copy_glyph(
            &id,
            kind.icon_px(),
            ack,
            reduce_motion,
            p.success,
        ));
    button = match kind {
        CopyButtonKind::Toolbar => button.small(),
        CopyButtonKind::Row => button.xsmall(),
    };
    if disabled {
        button = button
            .text_color(p.fg_tertiary.opacity(0.38))
            .cursor_default();
    } else {
        button = button.text_color(p.fg_secondary);
    }
    button
}

fn copy_glyph(
    id: &str,
    icon_px: f32,
    ack: Option<&CopyAck>,
    reduce_motion: bool,
    success: Hsla,
) -> gpui_kit::Div {
    let mut stack = div().relative().size(px(icon_px)).flex_shrink_0();
    if let Some(ack) = ack {
        if reduce_motion {
            stack = stack.child(static_slot(icon_px, IconName::Check, Some(success)));
        } else {
            let generation = ack.generation;
            stack = stack
                .child(fading_slot(
                    SharedString::from(format!("{id}-copy")),
                    generation,
                    icon_px,
                    IconName::Copy,
                    None,
                    false,
                ))
                .child(fading_slot(
                    SharedString::from(format!("{id}-check")),
                    generation,
                    icon_px,
                    IconName::Check,
                    Some(success),
                    true,
                ));
        }
    } else {
        stack = stack.child(static_slot(icon_px, IconName::Copy, None));
    }
    stack
}

fn static_slot(icon_px: f32, icon: IconName, color: Option<Hsla>) -> AnyElement {
    slot(icon_px, glyph(icon_px, icon, color)).into_any_element()
}

fn fading_slot(
    name: SharedString,
    generation: u64,
    icon_px: f32,
    icon: IconName,
    color: Option<Hsla>,
    appear: bool,
) -> AnyElement {
    let fade_id = ElementId::NamedInteger(SharedString::from(format!("{name}-fade")), generation);
    let scale_id = ElementId::NamedInteger(SharedString::from(format!("{name}-scale")), generation);
    // Opacity fades the slot. Scale lives on the icon, which is what GPUI can
    // transform around its center. Both timelines start together.
    slot(
        icon_px,
        glyph(icon_px, icon, color).with_animations(
            scale_id,
            copy_animations(),
            move |icon, index, delta| {
                let scale = icon_scale(glyph_visibility(appear, index, delta));
                icon.transform(Transformation::scale(size(scale, scale)))
            },
        ),
    )
    .with_animations(fade_id, copy_animations(), move |slot, index, delta| {
        slot.opacity(glyph_visibility(appear, index, delta))
    })
    .into_any_element()
}

fn glyph(icon_px: f32, icon: IconName, color: Option<Hsla>) -> Icon {
    let mut glyph = Icon::new(icon).size(px(icon_px));
    if let Some(color) = color {
        glyph = glyph.text_color(color);
    }
    glyph
}

fn slot(icon_px: f32, icon: impl IntoElement) -> gpui_kit::Div {
    div()
        .absolute()
        .top(px(0.))
        .left(px(0.))
        .size(px(icon_px))
        .flex()
        .items_center()
        .justify_center()
        .child(icon)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_fades_in_holds_then_fades_out() {
        assert_eq!(check_visibility(0, 0.0), 0.0);
        assert_eq!(check_visibility(0, 1.0), 1.0);
        assert_eq!(check_visibility(1, 0.4), 1.0);
        assert_eq!(check_visibility(2, 0.0), 1.0);
        assert_eq!(check_visibility(2, 1.0), 0.0);
        assert_eq!(COPY_HOLD, Duration::from_millis(1500));
        assert!(COPY_MORPH < Duration::from_millis(250));
        assert!(copy_ack_lifetime() > COPY_HOLD);
    }

    #[test]
    fn the_glyph_grows_as_it_appears() {
        assert!(icon_scale(0.0) < icon_scale(0.5));
        assert_eq!(icon_scale(1.0), 1.0);
        assert!(icon_scale(0.0) > 0.7);
    }

    #[test]
    fn each_button_keeps_its_own_acknowledgement() {
        let mut book = CopyAckBook::default();
        let file = PathBuf::from("/proj/.env");
        let row = book.acknowledge("copy-1", file.clone());
        let toolbar = book.acknowledge(TOOLBAR_COPY_ID, file.clone());
        assert_ne!(row, toolbar);
        assert!(book.visible("copy-1", &file).is_some());
        assert!(book.visible(TOOLBAR_COPY_ID, &file).is_some());
        assert!(book.expire("copy-1", row));
        assert!(book.visible("copy-1", &file).is_none());
        assert!(book.visible(TOOLBAR_COPY_ID, &file).is_some());
    }

    #[test]
    fn a_newer_copy_is_not_cleared_by_the_previous_timer() {
        let mut book = CopyAckBook::default();
        let file = PathBuf::from("/proj/.env");
        let first = book.acknowledge("copy-1", file.clone());
        let second = book.acknowledge("copy-1", file.clone());
        assert!(!book.expire("copy-1", first));
        assert_eq!(book.visible("copy-1", &file).unwrap().generation, second);
        assert!(book.expire("copy-1", second));
        assert!(book.visible("copy-1", &file).is_none());
    }

    #[test]
    fn an_acknowledgement_belongs_to_the_file_that_was_copied() {
        let mut book = CopyAckBook::default();
        let copied = PathBuf::from("/a/.env");
        let other = PathBuf::from("/b/.env");
        book.acknowledge("copy-0", copied.clone());
        assert!(book.visible("copy-0", &other).is_none());
        assert!(book.visible("copy-0", &copied).is_some());
    }
}
