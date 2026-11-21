//! Design tokens for Envy, implementing `docs/design.md`.
//!
//! One brand accent (the logo green), clear chrome/canvas surface separation,
//! and explicit light and dark values for every token.

use gpui_kit::component::Theme;
use gpui_kit::{px, rgb, App, Hsla, Pixels};

pub const SIDEBAR_W: Pixels = px(256.);
pub const TITLE_BAR_H: Pixels = px(36.);
pub const SIDEBAR_HEADER_H: Pixels = px(36.);
pub const SIDEBAR_ROW_BRANCH_H: Pixels = px(40.);
pub const TAB_BAR_H: Pixels = px(36.);
pub const TOOLBAR_H: Pixels = px(40.);
pub const COL_HEADER_H: Pixels = px(28.);
pub const SECTION_ROW_H: Pixels = px(28.);
pub const KEY_ROW_H: Pixels = px(32.);
pub const KEY_ACTIONS_W: Pixels = px(64.);
pub const DIFF_HEADER_H: Pixels = px(36.);
pub const DIFF_ROW_H: Pixels = px(32.);
pub const DIFF_KEY_COL_W: Pixels = px(256.);
pub const DIFF_GUTTER_W: Pixels = px(56.);
pub const FOOTER_H: Pixels = px(28.);
pub const MASK_MAX_DOTS: usize = 12;

#[derive(Clone, Copy)]
pub struct Palette {
    pub bg_canvas: Hsla,
    pub bg_chrome: Hsla,
    pub bg_surface: Hsla,
    pub bg_kbd: Hsla,
    pub bg_hover: Hsla,
    pub bg_selected_muted: Hsla,
    pub bg_selected_accent: Hsla,
    pub backdrop: Hsla,

    pub border_hairline: Hsla,
    pub border_control: Hsla,

    pub fg_primary: Hsla,
    pub fg_secondary: Hsla,
    pub fg_tertiary: Hsla,

    pub accent: Hsla,
    pub accent_hover: Hsla,
    pub accent_fg: Hsla,
    pub accent_subtle: Hsla,
    pub accent_ring: Hsla,

    pub prod_dot: Hsla,
    pub warning: Hsla,
    pub warning_dot: Hsla,
    pub warning_bg: Hsla,
    pub warning_border: Hsla,
    pub danger: Hsla,
    pub danger_bg: Hsla,
    pub danger_border: Hsla,

    pub diff_different_bg: Hsla,
    pub diff_only_left_bg: Hsla,
    pub diff_only_right_bg: Hsla,
}

impl Palette {
    pub fn for_mode(dark: bool) -> Self {
        if dark {
            let accent: Hsla = rgb(0x34D399).into();
            let warning: Hsla = rgb(0xF59E0B).into();
            let danger_base: Hsla = rgb(0xEF4444).into();
            Self {
                bg_canvas: rgb(0x111214).into(),
                bg_chrome: rgb(0x16171A).into(),
                bg_surface: rgb(0x1C1E22).into(),
                bg_kbd: rgb(0x202228).into(),
                bg_hover: rgb(0x1E2127).into(),
                bg_selected_muted: rgb(0x1E2127).into(),
                bg_selected_accent: accent.opacity(0.09),
                backdrop: Hsla::from(rgb(0x090A0C)).opacity(0.64),

                border_hairline: rgb(0x23262C).into(),
                border_control: rgb(0x2F333C).into(),

                fg_primary: rgb(0xECEEF2).into(),
                fg_secondary: rgb(0x9AA0AA).into(),
                fg_tertiary: rgb(0x7B8394).into(),

                accent,
                accent_hover: rgb(0x4ADEA8).into(),
                accent_fg: rgb(0x06281E).into(),
                accent_subtle: accent.opacity(0.11),
                accent_ring: accent.opacity(0.26),

                prod_dot: rgb(0xE5484D).into(),
                warning,
                warning_dot: warning,
                warning_bg: warning.opacity(0.09),
                warning_border: warning.opacity(0.22),
                danger: rgb(0xF87171).into(),
                danger_bg: danger_base.opacity(0.10),
                danger_border: danger_base.opacity(0.24),

                diff_different_bg: warning.opacity(0.045),
                diff_only_left_bg: accent.opacity(0.035),
                diff_only_right_bg: danger_base.opacity(0.035),
            }
        } else {
            let accent: Hsla = rgb(0x047857).into();
            let warning_dot: Hsla = rgb(0xD97706).into();
            let danger: Hsla = rgb(0xDC2626).into();
            Self {
                bg_canvas: rgb(0xFFFFFF).into(),
                bg_chrome: rgb(0xF6F7F9).into(),
                bg_surface: rgb(0xFFFFFF).into(),
                bg_kbd: rgb(0xECEEF2).into(),
                bg_hover: rgb(0xEEF0F4).into(),
                bg_selected_muted: rgb(0xE8EBF0).into(),
                bg_selected_accent: accent.opacity(0.08),
                backdrop: Hsla::from(rgb(0x111318)).opacity(0.32),

                border_hairline: rgb(0xE4E7EC).into(),
                border_control: rgb(0xCFD5DF).into(),

                fg_primary: rgb(0x111318).into(),
                fg_secondary: rgb(0x4B5362).into(),
                fg_tertiary: rgb(0x687080).into(),

                accent,
                accent_hover: rgb(0x065F46).into(),
                accent_fg: rgb(0xFFFFFF).into(),
                accent_subtle: accent.opacity(0.09),
                accent_ring: accent.opacity(0.28),

                prod_dot: danger,
                warning: rgb(0xB45309).into(),
                warning_dot,
                warning_bg: warning_dot.opacity(0.09),
                warning_border: warning_dot.opacity(0.22),
                danger,
                danger_bg: danger.opacity(0.08),
                danger_border: danger.opacity(0.22),

                diff_different_bg: warning_dot.opacity(0.05),
                diff_only_left_bg: accent.opacity(0.035),
                diff_only_right_bg: danger.opacity(0.035),
            }
        }
    }
}

/// Sync the active `gpui-component` theme with Envy's palette so built-in
/// widgets (title bar, inputs, buttons, tooltips, scrollbars) match our tokens.
pub fn apply_palette(cx: &mut App) {
    Theme::update(cx, |theme| {
        let p = Palette::for_mode(theme.mode.is_dark());
        theme.focus_ring = false;
        theme.radius = px(6.);
        theme.radius_lg = px(10.);

        theme.background = p.bg_canvas;
        theme.foreground = p.fg_primary;
        theme.muted = p.bg_kbd;
        theme.muted_foreground = p.fg_secondary;
        theme.border = p.border_hairline;
        theme.input = p.border_control;
        theme.ring = p.accent_ring;
        theme.caret = p.accent;
        theme.link = p.accent;
        theme.link_hover = p.accent_hover;

        theme.primary = p.accent;
        theme.primary_hover = p.accent_hover;
        theme.primary_active = p.accent_hover;
        theme.primary_foreground = p.accent_fg;
        theme.button_primary = p.accent;
        theme.button_primary_hover = p.accent_hover;
        theme.button_primary_active = p.accent_hover;
        theme.button_primary_foreground = p.accent_fg;

        theme.secondary = p.bg_kbd;
        theme.secondary_hover = p.bg_hover;
        theme.secondary_active = p.bg_selected_muted;
        theme.secondary_foreground = p.fg_primary;

        theme.accent = p.bg_hover;
        theme.accent_foreground = p.fg_primary;
        theme.list_hover = p.bg_hover;
        theme.list_active = p.bg_selected_accent;
        theme.list_highlight_foreground = p.accent;

        theme.sidebar = p.bg_chrome;
        theme.sidebar_foreground = p.fg_primary;
        theme.sidebar_border = p.border_hairline;
        theme.sidebar_accent = p.bg_selected_accent;
        theme.sidebar_accent_foreground = p.accent;

        theme.title_bar = p.bg_chrome;
        theme.title_bar_border = p.border_hairline;
        theme.status_bar = p.bg_chrome;
        theme.status_bar_border = p.border_hairline;

        theme.popover = p.bg_surface;
        theme.popover_foreground = p.fg_primary;
        theme.warning = p.warning_dot;
        theme.danger = p.danger;
        theme.selection = p.accent.opacity(0.24);
    });
}
