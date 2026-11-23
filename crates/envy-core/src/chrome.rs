//! Shared geometry for the two search fields.
//!
//! Find a project sits under the sidebar header. Find a key sits under the
//! file tabs, in the toolbar. The headers are the same height, each band
//! centers the same field, and both fields paint their line into that box,
//! so the two inputs share a top, a height, and a baseline.

/// Sidebar header above Find a project.
pub const PROJECT_HEADER_H_PX: f32 = 36.;
/// File tab row above Find a key.
pub const TAB_BAR_H_PX: f32 = 36.;
/// Band that centers Find a project. Matches the workspace toolbar.
pub const SEARCH_BAND_H_PX: f32 = 40.;
/// Workspace toolbar that centers Find a key.
pub const TOOLBAR_H_PX: f32 = 40.;
/// Height of both search fields, border included.
pub const SEARCH_FIELD_H_PX: f32 = 28.;
/// Hairline drawn inside the bottom of the toolbar band. The sidebar band
/// uses the same inset so the hairline does not lift Find a key.
pub const SEARCH_BAND_BORDER_PX: f32 = 1.;
/// Shared text size. Both fields paint into the full field height.
pub const SEARCH_TEXT_PX: f32 = 13.;
/// Shared line box. The paint uses the field height, so this stays in step.
pub const SEARCH_LINE_PX: f32 = 16.;
/// Find a key prefers this width and only shrinks after the notice has.
pub const KEY_SEARCH_PREFERRED_PX: f32 = 220.;
/// Narrowest Find a key, once the notice is down to its icon.
pub const KEY_SEARCH_MIN_PX: f32 = 120.;
/// Icon and padding left when a notice label has ellipsized away.
pub const TOOLBAR_NOTICE_MIN_PX: f32 = 32.;
/// The notice gives up width before the search field does.
pub const TOOLBAR_NOTICE_SHRINK: f32 = 100.;
/// The search field shrinks only after the notice is at its minimum.
pub const TOOLBAR_SEARCH_SHRINK: f32 = 1.;

/// Widths for the search field, the notice, and the icon buttons.
///
/// `available` is the space for those three plus the key count, after the
/// toolbar's padding and the gaps between them. Buttons stay at `buttons`.
/// A long notice ellipsizes before the search field moves off its preferred
/// width.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ToolbarFit {
    pub search: f32,
    pub notice: f32,
    pub buttons: f32,
}

pub fn fit_toolbar(
    available: f32,
    buttons: f32,
    search_preferred: f32,
    search_min: f32,
    count: f32,
    notice: f32,
    notice_min: f32,
) -> ToolbarFit {
    let buttons = buttons.max(0.);
    let search_preferred = search_preferred.max(0.);
    let search_min = search_min.clamp(0., search_preferred);
    let notice = notice.max(0.);
    let notice_min = notice_min.clamp(0., notice);
    let mut search = search_preferred;
    let mut notice_w = notice;
    let mut deficit = (buttons + count.max(0.) + search + notice_w - available).max(0.);
    let from_notice = deficit.min((notice_w - notice_min).max(0.));
    notice_w -= from_notice;
    deficit -= from_notice;
    let from_search = deficit.min((search - search_min).max(0.));
    search -= from_search;
    ToolbarFit {
        search,
        notice: notice_w,
        buttons,
    }
}

/// Top, height, and text center of one search field, measured from the
/// bottom of the title bar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SearchFrame {
    pub top: f32,
    pub height: f32,
    /// Vertical center of the field. Both fields paint their line into the
    /// full height, so a shared center is a shared baseline.
    pub baseline: f32,
}

/// Top padding that places the field in a band whose bottom hairline is
/// inside the band. The leftover half pixel stays below the field.
pub fn search_field_inset(band_h: f32, field_h: f32, border_h: f32) -> f32 {
    ((band_h - border_h - field_h) / 2.).floor()
}

/// Place a field under `header_h`, using [`search_field_inset`].
pub fn centered_search_frame(
    header_h: f32,
    band_h: f32,
    field_h: f32,
    border_h: f32,
) -> SearchFrame {
    let top = header_h + search_field_inset(band_h, field_h, border_h);
    SearchFrame {
        top,
        height: field_h,
        baseline: top + field_h / 2.,
    }
}

pub fn project_search_frame() -> SearchFrame {
    centered_search_frame(
        PROJECT_HEADER_H_PX,
        SEARCH_BAND_H_PX,
        SEARCH_FIELD_H_PX,
        SEARCH_BAND_BORDER_PX,
    )
}

pub fn key_search_frame() -> SearchFrame {
    centered_search_frame(
        TAB_BAR_H_PX,
        TOOLBAR_H_PX,
        SEARCH_FIELD_H_PX,
        SEARCH_BAND_BORDER_PX,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_and_key_search_share_top_height_and_baseline() {
        let project = project_search_frame();
        let key = key_search_frame();
        assert_eq!(PROJECT_HEADER_H_PX, TAB_BAR_H_PX);
        assert_eq!(SEARCH_BAND_H_PX, TOOLBAR_H_PX);
        assert_eq!(project, key);
        assert_eq!(project.height, SEARCH_FIELD_H_PX);
        let inset = search_field_inset(SEARCH_BAND_H_PX, SEARCH_FIELD_H_PX, SEARCH_BAND_BORDER_PX);
        assert_eq!(inset, 5.);
        assert_eq!(project.top, PROJECT_HEADER_H_PX + inset);
        assert_eq!(project.baseline, project.top + project.height / 2.);
    }

    #[test]
    fn buttons_keep_their_width_and_the_notice_truncates_first() {
        let buttons = 280.;
        let count = 36.;
        let notice = 180.;
        let wide = fit_toolbar(
            800.,
            buttons,
            KEY_SEARCH_PREFERRED_PX,
            KEY_SEARCH_MIN_PX,
            count,
            notice,
            TOOLBAR_NOTICE_MIN_PX,
        );
        assert_eq!(wide.buttons, buttons);
        assert_eq!(wide.search, KEY_SEARCH_PREFERRED_PX);
        assert_eq!(wide.notice, notice);

        // 220 + 36 + 180 + 280 = 716. A 600px row takes the extra 116 from the notice.
        let medium = fit_toolbar(
            600.,
            buttons,
            KEY_SEARCH_PREFERRED_PX,
            KEY_SEARCH_MIN_PX,
            count,
            notice,
            TOOLBAR_NOTICE_MIN_PX,
        );
        assert_eq!(medium.buttons, buttons);
        assert_eq!(medium.search, KEY_SEARCH_PREFERRED_PX);
        assert_eq!(medium.notice, notice - 116.);

        let narrow = fit_toolbar(
            400.,
            buttons,
            KEY_SEARCH_PREFERRED_PX,
            KEY_SEARCH_MIN_PX,
            count,
            notice,
            TOOLBAR_NOTICE_MIN_PX,
        );
        assert_eq!(narrow.buttons, buttons);
        assert_eq!(narrow.notice, TOOLBAR_NOTICE_MIN_PX);
        assert_eq!(narrow.search, KEY_SEARCH_MIN_PX);

        let compare_chips = fit_toolbar(
            560.,
            buttons,
            KEY_SEARCH_PREFERRED_PX,
            KEY_SEARCH_MIN_PX,
            0.,
            420.,
            TOOLBAR_NOTICE_MIN_PX,
        );
        assert_eq!(compare_chips.buttons, buttons);
        assert_eq!(compare_chips.notice, 60.);
        assert_eq!(compare_chips.search, KEY_SEARCH_PREFERRED_PX);

        let clear = fit_toolbar(
            500.,
            buttons,
            KEY_SEARCH_PREFERRED_PX,
            KEY_SEARCH_MIN_PX,
            count,
            0.,
            0.,
        );
        assert_eq!(clear.notice, 0.);
        assert_eq!(clear.buttons, buttons);
    }
}
