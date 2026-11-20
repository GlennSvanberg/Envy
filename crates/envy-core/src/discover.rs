//! Find `.env*` files under folders the user added and group them by git repo.
//!
//! This never walks a whole disk. It skips dependency and build directories,
//! and it does not follow directory symlinks. Key *names* are kept so search
//! can run without re-reading files; values are dropped before a project is
//! returned.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::diff::missing_vs_example;
use crate::document::EnvDocument;

const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "vendor",
    ".next",
    "build",
    "venv",
    ".venv",
    "__pycache__",
    "out",
    "coverage",
    ".turbo",
    ".cache",
    ".idea",
    ".vscode",
];

const MAX_DEPTH: usize = 8;
const MAX_FILE_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvFile {
    pub path: PathBuf,
    pub name: String,
    /// Key names only, last-wins order. Values are not retained.
    pub keys: Vec<String>,
    pub prod: bool,
    pub missing_vs_example: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Project {
    pub root: PathBuf,
    pub name: String,
    pub branch: String,
    pub files: Vec<EnvFile>,
    pub missing_vs_example: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanError {
    pub path: PathBuf,
    pub message: String,
}

struct Found {
    path: PathBuf,
    name: String,
    keys: Vec<String>,
    /// Held only while the scan computes "missing vs example", then dropped.
    document: EnvDocument,
}

/// Scan `folders` and group env files by repository.
pub fn discover(folders: &[PathBuf], config: &Config) -> (Vec<Project>, Vec<ScanError>) {
    let mut found: Vec<Found> = Vec::new();
    let mut errors = Vec::new();
    for folder in folders {
        walk(folder, 0, &mut found, &mut errors);
    }

    let mut groups: BTreeMap<PathBuf, Vec<Found>> = BTreeMap::new();
    for file in found {
        let root = project_root(&file.path);
        groups.entry(root).or_default().push(file);
    }

    let mut projects = Vec::new();
    for (root, files) in groups {
        projects.push(build_project(root, files, config));
    }
    projects.sort_by(|a, b| {
        a.name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase())
            .then(a.root.cmp(&b.root))
    });
    (projects, errors)
}

pub fn is_prod_filename(name: &str) -> bool {
    name.split(['.', '_', '-'])
        .any(|part| part.eq_ignore_ascii_case("prod") || part.eq_ignore_ascii_case("production"))
}

pub fn is_env_filename(name: &str) -> bool {
    if !name.starts_with(".env") {
        return false;
    }
    if name.ends_with('~') || name.ends_with(".swp") || name.ends_with(".swo") {
        return false;
    }
    true
}

/// Preferred file to open when a project is selected and the link didn't name one.
pub fn default_file_index(project: &Project) -> Option<usize> {
    let prefer = [".env.local", ".env", ".env.development", ".env.dev"];
    for name in prefer {
        if let Some(index) = project.files.iter().position(|file| file.name == name) {
            return Some(index);
        }
    }
    project
        .files
        .iter()
        .position(|file| !file.prod && !file.name.contains("example"))
        .or_else(|| project.files.first().map(|_| 0))
}

pub fn resolve_project<'a>(
    projects: &'a [Project],
    config: &Config,
    repo_name: &str,
) -> Option<usize> {
    if let Some(mapped) = config.projects.get(repo_name) {
        if let Some(index) = projects.iter().position(|project| &project.root == mapped) {
            return Some(index);
        }
    }
    if let Some(index) = projects
        .iter()
        .position(|project| project.name == repo_name)
    {
        return Some(index);
    }
    projects
        .iter()
        .position(|project| project.name.eq_ignore_ascii_case(repo_name))
}

pub fn resolve_file(project: &Project, file: &str) -> Option<usize> {
    if let Some(index) = project.files.iter().position(|item| item.name == file) {
        return Some(index);
    }
    project.files.iter().position(|item| {
        item.path
            .to_string_lossy()
            .replace('\\', "/")
            .ends_with(&file.replace('\\', "/"))
    })
}

fn walk(dir: &Path, depth: usize, found: &mut Vec<Found>, errors: &mut Vec<ScanError>) {
    if depth > MAX_DEPTH {
        return;
    }
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) => {
            errors.push(ScanError {
                path: dir.to_path_buf(),
                message: err.to_string(),
            });
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let meta = match fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(err) => {
                errors.push(ScanError {
                    path: path.clone(),
                    message: err.to_string(),
                });
                continue;
            }
        };
        // Directory links can escape the folder the user added. File links are
        // skipped too: the scan reports the path it can see, not a second copy.
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            if SKIP_DIRS.iter().any(|skip| name == *skip) {
                continue;
            }
            walk(&path, depth + 1, found, errors);
            continue;
        }
        if !meta.is_file() || !is_env_filename(&name) {
            continue;
        }
        if meta.len() > MAX_FILE_BYTES {
            errors.push(ScanError {
                path: path.clone(),
                message: "skipped; larger than 1 MiB".into(),
            });
            continue;
        }
        match fs::read(&path) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(text) => {
                    let document = EnvDocument::parse(&text);
                    let keys = document.key_names();
                    found.push(Found {
                        path,
                        name,
                        keys,
                        document,
                    });
                }
                Err(_) => errors.push(ScanError {
                    path,
                    message: "skipped; not valid utf-8".into(),
                }),
            },
            Err(err) => errors.push(ScanError {
                path,
                message: err.to_string(),
            }),
        }
    }
}

fn build_project(root: PathBuf, files: Vec<Found>, config: &Config) -> Project {
    let example_indexes: Vec<usize> = files
        .iter()
        .enumerate()
        .filter(|(_, file)| is_example_filename(&file.name))
        .map(|(index, _)| index)
        .collect();

    let mut env_files = Vec::with_capacity(files.len());
    for (index, file) in files.iter().enumerate() {
        let missing = if is_example_filename(&file.name) {
            false
        } else {
            example_for(&files, &example_indexes, &file.path)
                .map(|example| !missing_vs_example(&file.document, &example.document).is_empty())
                .unwrap_or(false)
        };
        let _ = index;
        env_files.push(EnvFile {
            path: file.path.clone(),
            name: file.name.clone(),
            keys: file.keys.clone(),
            prod: is_prod_filename(&file.name),
            missing_vs_example: missing,
        });
    }
    // Documents are dropped with `files` at the end of this function.
    drop(files);

    env_files.sort_by(|a, b| {
        file_rank(&a.name)
            .cmp(&file_rank(&b.name))
            .then(a.name.cmp(&b.name))
    });
    let missing_vs_example = env_files.iter().any(|file| file.missing_vs_example);
    let (name, branch) = identity(&root, config);
    Project {
        root,
        name,
        branch,
        files: env_files,
        missing_vs_example,
    }
}

pub fn is_example_filename(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower == ".env.example" || lower.ends_with(".example") || lower.contains(".example.")
}

fn example_for<'a>(
    files: &'a [Found],
    example_indexes: &[usize],
    path: &Path,
) -> Option<&'a Found> {
    let parent = path.parent();
    let same_dir = example_indexes.iter().copied().find(|index| {
        files[*index].path.parent() == parent
            && files[*index].name.eq_ignore_ascii_case(".env.example")
    });
    if let Some(index) = same_dir {
        return Some(&files[index]);
    }
    let any_example = example_indexes
        .iter()
        .copied()
        .find(|index| files[*index].name.eq_ignore_ascii_case(".env.example"));
    if let Some(index) = any_example {
        return Some(&files[index]);
    }
    example_indexes.first().map(|index| &files[*index])
}

fn file_rank(name: &str) -> u8 {
    let lower = name.to_ascii_lowercase();
    if is_example_filename(name) {
        0
    } else if name == ".env" {
        1
    } else if lower.contains("local") {
        2
    } else if is_prod_filename(name) {
        5
    } else {
        3
    }
}

fn project_root(file: &Path) -> PathBuf {
    if let Some(root) = git_root(file) {
        return canonicalize(&root);
    }
    file.parent()
        .map(canonicalize)
        .unwrap_or_else(|| file.to_path_buf())
}

fn git_root(start: &Path) -> Option<PathBuf> {
    let mut cur = start.parent()?.to_path_buf();
    loop {
        if cur.join(".git").exists() {
            return Some(cur);
        }
        if !cur.pop() {
            return None;
        }
    }
}

fn canonicalize(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn identity(root: &Path, config: &Config) -> (String, String) {
    let root_key = root.to_string_lossy();
    if let Some(name) = config.project_names.get(root_key.as_ref()) {
        let branch = read_branch(root).unwrap_or_else(|| "—".into());
        return (name.clone(), branch);
    }
    match resolve_git_dir(root) {
        Some(git_dir) => {
            let name = read_remote_name(&git_dir)
                .or_else(|| root.file_name().map(|n| n.to_string_lossy().into_owned()))
                .unwrap_or_else(|| "project".into());
            let branch = read_branch_from_git_dir(&git_dir).unwrap_or_else(|| "unknown".into());
            (name, branch)
        }
        None => {
            let name = root
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "project".into());
            (name, "—".into())
        }
    }
}

fn read_branch(root: &Path) -> Option<String> {
    resolve_git_dir(root).and_then(|git_dir| read_branch_from_git_dir(&git_dir))
}

fn resolve_git_dir(repo: &Path) -> Option<PathBuf> {
    let git = repo.join(".git");
    if git.is_dir() {
        return Some(git);
    }
    if git.is_file() {
        let text = fs::read_to_string(&git).ok()?;
        let path = text.lines().next()?.trim().strip_prefix("gitdir:")?.trim();
        let path = PathBuf::from(path);
        let resolved = if path.is_absolute() {
            path
        } else {
            repo.join(path)
        };
        return Some(resolved);
    }
    None
}

fn read_branch_from_git_dir(git_dir: &Path) -> Option<String> {
    let head = fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head = head.trim();
    if let Some(reference) = head.strip_prefix("ref: ") {
        let name = reference
            .strip_prefix("refs/heads/")
            .or_else(|| reference.strip_prefix("refs/tags/"))
            .unwrap_or(reference);
        return Some(name.to_string());
    }
    if head.len() >= 7 {
        Some(format!("detached {}", &head[..7]))
    } else if head.is_empty() {
        None
    } else {
        Some(head.to_string())
    }
}

fn read_remote_name(git_dir: &Path) -> Option<String> {
    let text = fs::read_to_string(git_dir.join("config")).ok()?;
    repo_name_from_git_config(&text)
}

pub fn repo_name_from_git_config(config_text: &str) -> Option<String> {
    let mut section_is_origin = false;
    let mut first_remote_url = None;
    for line in config_text.lines() {
        let trimmed = line.trim();
        if let Some(section) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            section_is_origin = section.trim().eq_ignore_ascii_case("remote \"origin\"");
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        if !key.trim().eq_ignore_ascii_case("url") {
            continue;
        }
        let url = value.trim();
        if url.is_empty() {
            continue;
        }
        if section_is_origin {
            return Some(repo_name_from_url(url));
        }
        if first_remote_url.is_none() {
            first_remote_url = Some(repo_name_from_url(url));
        }
    }
    first_remote_url
}

pub fn repo_name_from_url(url: &str) -> String {
    let url = url.trim().trim_end_matches('/');
    let url = url.strip_suffix(".git").unwrap_or(url);
    let path = if let Some(rest) = url.strip_prefix("git@") {
        rest.split_once(':').map(|(_, path)| path).unwrap_or(rest)
    } else if let Some((_, after_scheme)) = url.split_once("://") {
        after_scheme
            .split_once('/')
            .map(|(_, path)| path)
            .unwrap_or(after_scheme)
    } else {
        url
    };
    path.rsplit('/')
        .find(|segment| !segment.is_empty())
        .unwrap_or("project")
        .to_string()
}

/// Key sets compared without retaining values. Used by tests and the dot.
pub fn key_set(keys: &[String]) -> BTreeSet<&str> {
    keys.iter().map(String::as_str).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn scratch() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "envy-discover-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn remote_names() {
        assert_eq!(
            repo_name_from_url("git@github.com:acme/billing.git"),
            "billing"
        );
        assert_eq!(
            repo_name_from_url("https://github.com/acme/billing.git"),
            "billing"
        );
        assert_eq!(
            repo_name_from_url("ssh://git@github.com/acme/billing"),
            "billing"
        );
        let config = "\
[core]\n\
\tbare = false\n\
[remote \"upstream\"]\n\
\turl = https://example.com/other.git\n\
[remote \"origin\"]\n\
\turl = git@github.com:acme/billing.git\n\
";
        assert_eq!(
            repo_name_from_git_config(config).as_deref(),
            Some("billing")
        );
    }

    #[test]
    fn groups_by_repo_and_flags_missing_keys() {
        let root = scratch();
        let repo = root.join("checkout");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::write(
            repo.join(".git").join("HEAD"),
            "ref: refs/heads/feature/env\n",
        )
        .unwrap();
        fs::write(
            repo.join(".git").join("config"),
            "[remote \"origin\"]\n\turl = https://github.com/acme/billing.git\n",
        )
        .unwrap();
        fs::write(repo.join(".env.example"), "A=1\nB=2\n# note\n").unwrap();
        fs::write(repo.join(".env.local"), "A=1\n").unwrap();
        fs::write(repo.join(".env.production"), "A=1\nB=2\n").unwrap();
        // Ignored noise.
        fs::create_dir_all(repo.join("node_modules")).unwrap();
        fs::write(repo.join("node_modules").join(".env"), "SECRET=1\n").unwrap();

        let (projects, errors) = discover(&[root.clone()], &Config::default());
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(projects.len(), 1);
        let project = &projects[0];
        assert_eq!(project.name, "billing");
        assert_eq!(project.branch, "feature/env");
        assert!(project.missing_vs_example);
        let names: Vec<_> = project
            .files
            .iter()
            .map(|file| file.name.as_str())
            .collect();
        assert_eq!(names, vec![".env.example", ".env.local", ".env.production"]);
        assert!(project
            .files
            .iter()
            .any(|file| file.name == ".env.local" && file.missing_vs_example));
        assert!(project
            .files
            .iter()
            .any(|file| file.name == ".env.production" && file.prod && !file.missing_vs_example));
        assert!(!project
            .files
            .iter()
            .any(|file| file.keys.iter().any(|key| key == "SECRET")));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn prod_filename() {
        assert!(is_prod_filename(".env.production"));
        assert!(is_prod_filename(".env.prod"));
        assert!(is_prod_filename(".env.production.local"));
        assert!(!is_prod_filename(".env"));
        assert!(!is_prod_filename(".env.product"));
        assert!(!is_prod_filename(".env.example"));
    }

    #[test]
    fn gitdir_file_is_followed() {
        let root = scratch();
        let real_git = root.join("real.git");
        fs::create_dir_all(&real_git).unwrap();
        fs::write(real_git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        let mut config = fs::File::create(real_git.join("config")).unwrap();
        writeln!(
            config,
            "[remote \"origin\"]\n\turl = git@github.com:acme/web.git"
        )
        .unwrap();
        let repo = root.join("worktree");
        fs::create_dir_all(&repo).unwrap();
        fs::write(
            repo.join(".git"),
            format!("gitdir: {}\n", real_git.display()),
        )
        .unwrap();
        fs::write(repo.join(".env"), "A=1\n").unwrap();
        let (projects, _) = discover(&[root.clone()], &Config::default());
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "web");
        assert_eq!(projects[0].branch, "main");
        let _ = fs::remove_dir_all(&root);
    }
}
