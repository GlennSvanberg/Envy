//! Project paths as the sidebar shows them.
//!
//! Windows keeps a drive letter and backslashes, and drops the `\\?\` prefix.
//! Unix uses forward slashes and may shorten the home directory to `~`.
//! When the path does not fit, characters are dropped from the start so the
//! end of the path stays visible.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathStyle {
    Windows,
    Unix,
}

pub fn host_path_style() -> PathStyle {
    if cfg!(windows) {
        PathStyle::Windows
    } else {
        PathStyle::Unix
    }
}

/// Native absolute path: drive letter and backslashes on Windows, slashes on Unix.
/// A `\\?\` or `\\?\UNC\` prefix is removed.
pub fn native_project_path(path: &str, style: PathStyle) -> String {
    let trimmed = path.trim();
    match style {
        PathStyle::Windows => {
            let stripped = if let Some(rest) = trimmed.strip_prefix(r"\\?\UNC\") {
                format!(r"\\{rest}")
            } else if let Some(rest) = trimmed.strip_prefix(r"\\?\") {
                rest.to_string()
            } else if let Some(rest) = trimmed.strip_prefix("//?/UNC/") {
                format!(r"\\{rest}")
            } else if let Some(rest) = trimmed.strip_prefix("//?/") {
                rest.to_string()
            } else {
                trimmed.to_string()
            };
            stripped.replace('/', "\\")
        }
        PathStyle::Unix => trimmed.replace('\\', "/"),
    }
}

/// Replace a home-directory prefix with `~`. `home` is only applied when it is
/// a real directory prefix, so `/home/glenn2` is left unchanged for home `/home/glenn`.
pub fn abbreviate_home(path: &str, home: Option<&str>) -> String {
    let Some(home) = home.map(str::trim).filter(|home| !home.is_empty()) else {
        return path.to_string();
    };
    let home = home.trim_end_matches(['/', '\\']);
    if path == home {
        return "~".to_string();
    }
    let prefix = format!("{home}/");
    match path.strip_prefix(&prefix) {
        Some(rest) if rest.is_empty() => "~".to_string(),
        Some(rest) => format!("~/{rest}"),
        None => path.to_string(),
    }
}

/// Drop characters from the start when `path` is longer than `max_chars`.
/// The cut prefers a `\` or `/` so the visible tail begins on a segment,
/// and the result starts with `…`.
pub fn truncate_path_head(path: &str, max_chars: usize) -> String {
    let chars: Vec<char> = path.chars().collect();
    if max_chars == 0 {
        return String::new();
    }
    if chars.len() <= max_chars {
        return path.to_string();
    }
    let budget = max_chars - 1;
    let mut cut = None;
    for (index, ch) in chars.iter().enumerate() {
        if (*ch == '\\' || *ch == '/') && chars.len() - index <= budget {
            cut = Some(index);
            break;
        }
    }
    let tail: String = match cut {
        Some(index) => chars[index..].iter().collect(),
        None => chars[chars.len() - budget..].iter().collect(),
    };
    format!("…{tail}")
}

/// How many characters of a project path fit in a sidebar of `sidebar_px`.
pub fn sidebar_path_chars(sidebar_px: u32) -> usize {
    let text_px = sidebar_px.saturating_sub(96);
    (text_px as usize / 6).clamp(12, 96)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_paths_use_backslashes_and_drop_verbatim_prefix() {
        assert_eq!(
            native_project_path(r"\\?\C:\git\glenn\braian", PathStyle::Windows),
            r"C:\git\glenn\braian"
        );
        assert_eq!(
            native_project_path(r"\\?\UNC\server\share\repo", PathStyle::Windows),
            r"\\server\share\repo"
        );
        assert_eq!(
            native_project_path("C:/git/glenn/braian", PathStyle::Windows),
            r"C:\git\glenn\braian"
        );
        assert_eq!(
            abbreviate_home(r"C:\git\glenn\braian", Some(r"C:\Users\glenn")),
            r"C:\git\glenn\braian"
        );
    }

    #[test]
    fn unix_paths_abbreviate_home_without_eating_a_longer_name() {
        let native = native_project_path("/home/glenn/src/braian", PathStyle::Unix);
        assert_eq!(
            abbreviate_home(&native, Some("/home/glenn")),
            "~/src/braian"
        );
        assert_eq!(
            abbreviate_home("/home/glenn2/src", Some("/home/glenn")),
            "/home/glenn2/src"
        );
        assert_eq!(abbreviate_home("/home/glenn", Some("/home/glenn")), "~");
    }

    #[test]
    fn truncation_keeps_the_end_and_starts_on_a_separator() {
        let path = r"C:\git\glenn\braian";
        assert_eq!(truncate_path_head(path, 80), path);
        assert_eq!(truncate_path_head(path, 14), r"…\glenn\braian");
        assert_eq!(truncate_path_head(path, 8), r"…\braian");
        assert_eq!(
            truncate_path_head("/home/glenn/src/braian", 12),
            "…/src/braian"
        );
    }

    #[test]
    fn wider_sidebar_fits_more_of_the_path() {
        assert!(sidebar_path_chars(256) < sidebar_path_chars(420));
        assert!(sidebar_path_chars(180) >= 12);
    }
}
