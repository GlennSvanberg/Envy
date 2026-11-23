//! Short labels for files that share a name.
//!
//! The tab keeps the filename. When two files in the project have that name,
//! each one also gets the shortest trailing run of directories that no other
//! file with that name ends with. A file in the project root gets `./` only
//! when another file has the same name.

use std::collections::HashMap;
use std::path::{Component, Path};

/// Filename, the directory suffix that tells it apart, and the project-relative path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabLabel {
    pub name: String,
    /// Empty when the filename is unique. `./` for the project root when it
    /// collides. Otherwise a suffix such as `backend` or `api/src`.
    pub suffix: String,
    /// `backend/.env`, `./.env` when the root collides, or `.env`.
    pub relative: String,
}

/// Path inside the project, with `/` between parts. A path outside `root`
/// falls back to the file name so the suffix cannot become an absolute path.
pub fn relative_slash_path(root: &Path, path: &Path) -> String {
    let rel = match path.strip_prefix(root) {
        Ok(rel) => rel,
        Err(_) => {
            return path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
        }
    };
    rel.components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// One suffix per relative path, in the same order.
///
/// `relative_paths` uses `/` (`backend/.env`, or `.env` for the project root).
/// Backslashes are accepted and treated as separators.
pub fn unique_dir_suffixes(relative_paths: &[&str]) -> Vec<String> {
    let parsed: Vec<(String, Vec<String>)> =
        relative_paths.iter().map(|path| split_rel(path)).collect();
    let mut suffixes = vec![String::new(); parsed.len()];
    let mut groups: HashMap<&str, Vec<usize>> = HashMap::new();
    for (index, (name, _)) in parsed.iter().enumerate() {
        groups.entry(name.as_str()).or_default().push(index);
    }
    for indices in groups.values() {
        if indices.len() < 2 {
            continue;
        }
        for &index in indices {
            let dir = &parsed[index].1;
            if dir.is_empty() {
                suffixes[index] = "./".to_string();
                continue;
            }
            let mut len = 1usize;
            loop {
                let suffix = &dir[dir.len() - len..];
                let unique = indices.iter().all(|&other_index| {
                    if other_index == index {
                        return true;
                    }
                    let other = &parsed[other_index].1;
                    if other.len() < suffix.len() {
                        return true;
                    }
                    &other[other.len() - suffix.len()..] != suffix
                });
                if unique || len == dir.len() {
                    suffixes[index] = suffix.join("/");
                    break;
                }
                len += 1;
            }
        }
    }
    suffixes
}

pub fn tab_labels(relative_paths: &[&str]) -> Vec<TabLabel> {
    let suffixes = unique_dir_suffixes(relative_paths);
    relative_paths
        .iter()
        .zip(suffixes)
        .map(|(path, suffix)| {
            let (name, dir) = split_rel(path);
            let relative = if suffix == "./" {
                format!("./{name}")
            } else if dir.is_empty() {
                name.clone()
            } else {
                format!("{}/{name}", dir.join("/"))
            };
            TabLabel {
                name,
                suffix,
                relative,
            }
        })
        .collect()
}

/// `name · suffix` when a suffix is needed, otherwise the filename.
pub fn labeled_name(name: &str, suffix: &str) -> String {
    if suffix.is_empty() {
        name.to_string()
    } else {
        format!("{name} · {suffix}")
    }
}

fn split_rel(path: &str) -> (String, Vec<String>) {
    let mut parts: Vec<String> = path
        .split(['/', '\\'])
        .filter(|part| !part.is_empty() && *part != ".")
        .map(str::to_string)
        .collect();
    let name = parts.pop().unwrap_or_default();
    (name, parts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn root_and_a_subfolder_are_told_apart() {
        let suffixes = unique_dir_suffixes(&[".env", "backend/.env"]);
        assert_eq!(suffixes, vec!["./".to_string(), "backend".to_string()]);
        let labels = tab_labels(&[".env", "backend/.env"]);
        assert_eq!(labels[0].relative, "./.env");
        assert_eq!(labels[1].relative, "backend/.env");
        assert_eq!(labeled_name(".env", "./"), ".env · ./");
    }

    #[test]
    fn deep_folders_drop_the_shared_parent() {
        let suffixes = unique_dir_suffixes(&["services/api/src/.env", "services/web/src/.env"]);
        assert_eq!(suffixes, vec!["api/src".to_string(), "web/src".to_string()]);
    }

    #[test]
    fn three_files_keep_only_the_part_that_differs() {
        let suffixes = unique_dir_suffixes(&["backend/.env", "frontend/.env", "services/api/.env"]);
        assert_eq!(
            suffixes,
            vec![
                "backend".to_string(),
                "frontend".to_string(),
                "api".to_string()
            ]
        );
        let shared_tail = unique_dir_suffixes(&["a/b/.env", "c/b/.env", "d/.env"]);
        assert_eq!(
            shared_tail,
            vec!["a/b".to_string(), "c/b".to_string(), "d".to_string()]
        );
    }

    #[test]
    fn a_unique_filename_has_no_suffix() {
        let suffixes = unique_dir_suffixes(&[".env", "backend/.env.local", ".env.example"]);
        assert_eq!(suffixes, vec!["", "", ""]);
        let outside = relative_slash_path(Path::new("/proj"), Path::new("/elsewhere/.env"));
        assert_eq!(outside, ".env");
    }
}
