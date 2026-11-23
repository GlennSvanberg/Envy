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
}
