//! Widths for the sidebar and the key column.
//!
//! The key column defaults to the longest key in the open file, clamped so a
//! single huge name cannot swallow the values.

pub const SIDEBAR_MIN_PX: f32 = 200.;
pub const SIDEBAR_MAX_PX: f32 = 480.;
pub const SIDEBAR_DEFAULT_PX: f32 = 256.;

pub const KEY_COLUMN_MIN_PX: f32 = 160.;
pub const KEY_COLUMN_MAX_PX: f32 = 560.;

/// Toggle, gap, and the padding around the key text.
const KEY_CHROME_PX: f32 = 56.;
/// Approximate advance of the 13px monospace face.
const KEY_CHAR_PX: f32 = 8.2;

pub fn clamp_sidebar_px(width: f32) -> f32 {
    width.clamp(SIDEBAR_MIN_PX, SIDEBAR_MAX_PX)
}

pub fn clamp_key_column_px(width: f32) -> f32 {
    width.clamp(KEY_COLUMN_MIN_PX, KEY_COLUMN_MAX_PX)
}

/// Width that fits every label, clamped to [`KEY_COLUMN_MIN_PX`] and
/// [`KEY_COLUMN_MAX_PX`]. An empty file uses the minimum.
pub fn fitted_key_column_px<I, S>(labels: I) -> f32
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let longest = labels
        .into_iter()
        .map(|label| label.as_ref().chars().count())
        .max()
        .unwrap_or(0);
    clamp_key_column_px(KEY_CHROME_PX + longest as f32 * KEY_CHAR_PX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_keys_use_the_minimum_and_long_keys_grow_until_the_max() {
        let short = fitted_key_column_px(["PORT", "HOST"]);
        assert_eq!(short, KEY_COLUMN_MIN_PX);

        let long_name = "PATTERN_DATAMODELPROCESSOR_PROCESS";
        let fitted = fitted_key_column_px([long_name, "PORT"]);
        assert!(fitted > KEY_COLUMN_MIN_PX);
        assert!(fitted < KEY_COLUMN_MAX_PX);
        assert!(fitted > short);

        let huge = "K".repeat(200);
        assert_eq!(fitted_key_column_px([huge.as_str()]), KEY_COLUMN_MAX_PX);
    }

    #[test]
    fn dragged_widths_stay_inside_the_limits() {
        assert_eq!(clamp_sidebar_px(40.), SIDEBAR_MIN_PX);
        assert_eq!(clamp_sidebar_px(900.), SIDEBAR_MAX_PX);
        assert_eq!(clamp_key_column_px(10.), KEY_COLUMN_MIN_PX);
        assert_eq!(clamp_key_column_px(2_000.), KEY_COLUMN_MAX_PX);
    }
}
