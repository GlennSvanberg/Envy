//! Find `.env` files under folders the user added and group them by git repo.
//!
//! The walk is parallel, skips dependency and build directories, and does not
//! follow directory symlinks. It records paths only. Key names are filled
//! later by [`fill_key_index`]; values are never kept.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ignore::{WalkBuilder, WalkState};
use serde::{Deserialize, Serialize};

use crate::config::{config_dir, Config};
use crate::diff::missing_vs_example;
use crate::document::EnvDocument;

/// How many directory levels under an added folder are walked.
/// `0` is the folder itself. Deeper directories are left alone.
pub const MAX_SCAN_DEPTH: usize = 8;

const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "vendor",
    "packages",
    ".next",
    "build",
    "bin",
    "obj",
    "venv",
    ".venv",
    "__pycache__",
    "out",
    "coverage",
    ".turbo",
    ".cache",
    ".gradle",
    ".idea",
    ".vs",
    ".vscode",
];

const MAX_FILE_BYTES: u64 = 1024 * 1024;

/// Dependency and VCS directories the walk never enters.
pub fn is_skipped_dir(name: &str) -> bool {
    SKIP_DIRS.iter().any(|skip| name.eq_ignore_ascii_case(skip))
}

/// One env file found by the walk. Paths only: the file is not read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoundEnv {
    pub path: PathBuf,
    pub root: PathBuf,
    pub name: String,
}

/// On-disk list of env paths for the folders the user added. No values.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexCache {
    #[serde(default)]
    pub folders: Vec<CachedFolder>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedFolder {
    pub path: PathBuf,
    pub files: Vec<CachedFile>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedFile {
    pub path: PathBuf,
    pub root: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvFile {
    pub path: PathBuf,
    pub name: String,
    /// Key names only (active and disabled). Values are not retained.
    pub keys: Vec<String>,
    /// Active (non-disabled) key names only. Used to compute missing keys vs
    /// `.env.example` without treating commented-out example keys as required.
    pub active_keys: Vec<String>,
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
    active_keys: Vec<String>,
    /// Held only while the scan computes "missing vs example", then dropped.
    document: EnvDocument,
}

/// Scan `folders`, index key names, and attach git names.
///
/// The window should call [`scan_paths`] (paths stream in as they are found),
/// then [`fill_key_index`] and [`attach_git_identity`]. All three do disk IO
/// and belong on a background task. Git config and `HEAD` are not read until
/// `attach_git_identity`. File contents are not read until `fill_key_index`.
pub fn discover(folders: &[PathBuf], config: &Config) -> (Vec<Project>, Vec<ScanError>) {
    let (mut projects, errors) = discover_files(folders, config);
    fill_key_index(&mut projects);
    attach_git_identity(&mut projects, config);
    (projects, errors)
}

/// Walk env files and group them. Project names are the folder names.
/// Branch is `"…"` until [`attach_git_identity`] runs. Keys are empty until
/// [`fill_key_index`].
///
/// The walk skips [`is_skipped_dir`] directories and ignored directories, does
/// not follow symlinks, and stops at [`MAX_SCAN_DEPTH`]. It does not read file
/// contents, git config, or `HEAD`.
pub fn discover_files(folders: &[PathBuf], config: &Config) -> (Vec<Project>, Vec<ScanError>) {
    let _ = config;
    let found = Mutex::new(Vec::new());
    let errors = scan_paths(folders, |file| {
        found.lock().expect("found").push(file);
    });
    let found = found.into_inner().expect("found");
    (projects_from_found(found), errors)
}

/// Walk `folders` in parallel and call `on_file` as soon as each env file is found.
///
/// `on_file` runs on the walker threads. It must not read the file. Ignored
/// directories are not entered. Ignored *files* whose names match `.env`,
/// `.env.*`, or `*.env` are still reported.
pub fn scan_paths(folders: &[PathBuf], on_file: impl Fn(FoundEnv) + Send + Sync) -> Vec<ScanError> {
    let errors = Mutex::new(Vec::new());
    let roots = Mutex::new(HashMap::<PathBuf, PathBuf>::new());
    for folder in folders {
        if !folder.is_dir() {
            errors.lock().expect("errors").push(ScanError {
                path: folder.clone(),
                message: "folder is missing".into(),
            });
            continue;
        }
        let mut overrides = ignore::overrides::OverrideBuilder::new(folder);
        let _ = overrides.case_insensitive(true);
        for pattern in [".env", ".env.*", "*.env"] {
            if overrides.add(pattern).is_err() {
                errors.lock().expect("errors").push(ScanError {
                    path: folder.clone(),
                    message: "could not build the file name filter".into(),
                });
            }
        }
        let mut builder = WalkBuilder::new(folder);
        builder
            .follow_links(false)
            .hidden(false)
            .parents(true)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .ignore(true)
            .max_depth(Some(MAX_SCAN_DEPTH + 1))
            .threads(walk_threads())
            .filter_entry(|entry| {
                let name = entry.file_name().to_string_lossy();
                if is_skipped_dir(&name) {
                    return false;
                }
                !entry.file_type().is_some_and(|kind| kind.is_symlink())
            });
        if let Ok(overrides) = overrides.build() {
            builder.overrides(overrides);
        }
        let on_file = &on_file;
        let errors = &errors;
        let roots = &roots;
        builder.build_parallel().run(|| {
            Box::new(|result| {
                let entry = match result {
                    Ok(entry) => entry,
                    Err(err) => {
                        errors.lock().expect("errors").push(ScanError {
                            path: PathBuf::new(),
                            message: err.to_string(),
                        });
                        return WalkState::Continue;
                    }
                };
                let name = entry.file_name().to_string_lossy();
                if !is_env_filename(&name) {
                    return WalkState::Continue;
                }
                let is_file = match entry.file_type() {
                    Some(kind) => kind.is_file(),
                    None => entry.metadata().is_ok_and(|meta| meta.is_file()),
                };
                if !is_file {
                    return WalkState::Continue;
                }
                match entry.metadata() {
                    Ok(meta) if meta.len() > MAX_FILE_BYTES => {
                        errors.lock().expect("errors").push(ScanError {
                            path: entry.path().to_path_buf(),
                            message: "skipped; larger than 1 MiB".into(),
                        });
                        return WalkState::Continue;
                    }
                    Ok(_) => {}
                    Err(err) => {
                        errors.lock().expect("errors").push(ScanError {
                            path: entry.path().to_path_buf(),
                            message: err.to_string(),
                        });
                        return WalkState::Continue;
                    }
                }
                let path = entry.path().to_path_buf();
                let root = project_root_cached(&path, roots);
                on_file(FoundEnv {
                    name: name.into_owned(),
                    path,
                    root,
                });
                WalkState::Continue
            })
        });
    }
    errors.into_inner().expect("errors")
}

fn walk_threads() -> usize {
    std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(4)
        .clamp(2, 8)
}

/// Read key names for files already found. Values are dropped before return.
pub fn fill_key_index(projects: &mut [Project]) {
    for project in projects {
        let mut found = Vec::with_capacity(project.files.len());
        let mut unread = Vec::new();
        for file in &project.files {
            match fs::read(&file.path) {
                Ok(bytes) => match String::from_utf8(bytes) {
                    Ok(text) => {
                        let document = EnvDocument::parse(&text);
                        let keys = document.key_names();
                        let active_keys = document.active_key_names();
                        found.push(Found {
                            path: file.path.clone(),
                            name: file.name.clone(),
                            keys,
                            active_keys,
                            document,
                        });
                    }
                    Err(_) => unread.push(file.clone()),
                },
                Err(_) => unread.push(file.clone()),
            }
        }
        let rebuilt = build_project(project.root.clone(), found);
        let mut files = rebuilt.files;
        files.extend(unread);
        files.sort_by(|a, b| {
            file_rank(&a.name)
                .cmp(&file_rank(&b.name))
                .then(a.name.cmp(&b.name))
                .then(a.path.cmp(&b.path))
        });
        project.missing_vs_example = files.iter().any(|file| file.missing_vs_example);
        project.files = files;
    }
}

pub fn index_cache_path() -> PathBuf {
    config_dir().join("index-cache.toml")
}

pub fn load_index_cache() -> IndexCache {
    load_index_cache_from(&index_cache_path())
}

pub fn load_index_cache_from(path: &Path) -> IndexCache {
    match fs::read_to_string(path) {
        Ok(text) => toml::from_str(&text).unwrap_or_default(),
        Err(_) => IndexCache::default(),
    }
}

pub fn save_index_cache(cache: &IndexCache) -> io::Result<()> {
    save_index_cache_to(cache, &index_cache_path())
}

pub fn save_index_cache_to(cache: &IndexCache, path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = toml::to_string_pretty(cache)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    fs::write(path, text)
}

/// Rebuild the sidebar list from a path cache. No disk access.
pub fn projects_from_cache(folders: &[PathBuf], cache: &IndexCache) -> Vec<Project> {
    let mut found = Vec::new();
    for folder in folders {
        let Some(cached) = cache.folders.iter().find(|item| item.path == *folder) else {
            continue;
        };
        for file in &cached.files {
            let Some(name) = file
                .path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
            else {
                continue;
            };
            if !is_env_filename(&name) {
                continue;
            }
            found.push(FoundEnv {
                path: file.path.clone(),
                root: file.root.clone(),
                name,
            });
        }
    }
    projects_from_found(found)
}

pub fn cache_from_projects(folders: &[PathBuf], projects: &[Project]) -> IndexCache {
    let mut cache = IndexCache {
        folders: Vec::with_capacity(folders.len()),
    };
    for folder in folders {
        let mut files = Vec::new();
        for project in projects {
            if !project.root.starts_with(folder) && project.root != *folder {
                let in_folder = project
                    .files
                    .iter()
                    .any(|file| file.path.starts_with(folder));
                if !in_folder {
                    continue;
                }
            }
            for file in &project.files {
                if file.path.starts_with(folder) || project.root.starts_with(folder) {
                    files.push(CachedFile {
                        path: file.path.clone(),
                        root: project.root.clone(),
                    });
                }
            }
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        files.dedup();
        cache.folders.push(CachedFolder {
            path: folder.clone(),
            files,
        });
    }
    cache
}

fn projects_from_found(found: Vec<FoundEnv>) -> Vec<Project> {
    let mut projects: Vec<Project> = Vec::new();
    for file in found {
        upsert_file(
            &mut projects,
            file.root,
            EnvFile {
                path: file.path,
                name: file.name.clone(),
                keys: Vec::new(),
                active_keys: Vec::new(),
                prod: is_prod_filename(&file.name),
                missing_vs_example: false,
            },
        );
    }
    sort_projects(&mut projects);
    projects
}

pub fn upsert_file(projects: &mut Vec<Project>, root: PathBuf, file: EnvFile) {
    if let Some(project) = projects.iter_mut().find(|project| project.root == root) {
        if let Some(existing) = project.files.iter_mut().find(|item| item.path == file.path) {
            if existing.keys.is_empty() {
                existing.keys = file.keys;
                existing.active_keys = file.active_keys;
                existing.missing_vs_example = file.missing_vs_example;
                existing.prod = file.prod;
            }
            return;
        }
        project.files.push(file);
        project.files.sort_by(|a, b| {
            file_rank(&a.name)
                .cmp(&file_rank(&b.name))
                .then(a.name.cmp(&b.name))
                .then(a.path.cmp(&b.path))
        });
        project.missing_vs_example = project.files.iter().any(|item| item.missing_vs_example);
        return;
    }
    let name = root
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "project".into());
    let missing_vs_example = file.missing_vs_example;
    projects.push(Project {
        root,
        name,
        branch: "…".into(),
        files: vec![file],
        missing_vs_example,
    });
    sort_projects(projects);
}

fn sort_projects(projects: &mut [Project]) {
    projects.sort_by(|a, b| {
        a.name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase())
            .then(a.root.cmp(&b.root))
    });
}

/// Fill `name` and `branch` from git config and `HEAD`. Disk IO.
pub fn attach_git_identity(projects: &mut [Project], config: &Config) {
    for project in projects {
        let (name, branch) = identity(&project.root, config);
        project.name = name;
        project.branch = branch;
    }
}

pub fn is_prod_filename(name: &str) -> bool {
    name.split(['.', '_', '-'])
        .any(|part| part.eq_ignore_ascii_case("prod") || part.eq_ignore_ascii_case("production"))
}

pub fn is_env_filename(name: &str) -> bool {
    if name.ends_with('~')
        || name.ends_with(".swp")
        || name.ends_with(".swo")
        || name.ends_with(".envy-tmp")
    {
        return false;
    }
    let lower = name.to_ascii_lowercase();
    lower == ".env" || lower.starts_with(".env.") || lower.ends_with(".env")
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

fn project_root_cached(file: &Path, cache: &Mutex<HashMap<PathBuf, PathBuf>>) -> PathBuf {
    let start = file.parent().unwrap_or(file).to_path_buf();
    if let Some(root) = cache.lock().expect("roots").get(&start).cloned() {
        return root;
    }
    let root = project_root(file);
    let mut guard = cache.lock().expect("roots");
    guard.insert(start, root.clone());
    root
}

fn build_project(root: PathBuf, files: Vec<Found>) -> Project {
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
            active_keys: file.active_keys.clone(),
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
    let name = root
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "project".into());
    Project {
        root,
        name,
        branch: "…".into(),
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
    fn disabled_keys_do_not_count_as_missing_vs_example() {
        let root = scratch();
        let repo = root.join("app");
        fs::create_dir_all(&repo).unwrap();
        fs::write(
            repo.join(".env.example"),
            "A=1\nB=2\n# OPTIONAL_FLAG=true\n",
        )
        .unwrap();
        fs::write(repo.join(".env.local"), "A=1\n# B=2\n").unwrap();

        let (projects, errors) = discover(&[root.clone()], &Config::default());
        assert!(errors.is_empty(), "{errors:?}");
        let project = &projects[0];
        let example = project
            .files
            .iter()
            .find(|f| f.name == ".env.example")
            .unwrap();
        let local = project
            .files
            .iter()
            .find(|f| f.name == ".env.local")
            .unwrap();

        assert_eq!(example.active_keys, vec!["A", "B"]);
        assert_eq!(local.keys, vec!["A", "B"]);
        assert!(!local.missing_vs_example);
        assert!(!project.missing_vs_example);
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

    #[test]
    fn files_are_listed_before_git_identity() {
        let root = scratch();
        let repo = root.join("checkout");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::write(repo.join(".git").join("HEAD"), "ref: refs/heads/main\n").unwrap();
        fs::write(
            repo.join(".git").join("config"),
            "[remote \"origin\"]\n\turl = https://github.com/acme/billing.git\n",
        )
        .unwrap();
        fs::write(repo.join(".env"), "A=1\n").unwrap();

        let (mut projects, _) = discover_files(&[root.clone()], &Config::default());
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "checkout");
        assert_eq!(projects[0].branch, "…");
        assert_eq!(projects[0].files.len(), 1);

        attach_git_identity(&mut projects, &Config::default());
        assert_eq!(projects[0].name, "billing");
        assert_eq!(projects[0].branch, "main");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn scan_skips_heavy_directories_and_stops_at_max_depth() {
        let root = scratch();
        fs::write(root.join(".env"), "ROOT=1\n").unwrap();
        for skip in ["node_modules", "target", ".git", "dist", ".next"] {
            let dir = root.join(skip);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join(".env"), "SKIP=1\n").unwrap();
        }
        let mut deep = root.clone();
        for index in 0..=MAX_SCAN_DEPTH {
            deep.push(format!("d{index}"));
        }
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join(".env"), "DEEP=1\n").unwrap();

        let (projects, _) = discover_files(&[root.clone()], &Config::default());
        let paths: Vec<_> = projects
            .iter()
            .flat_map(|project| project.files.iter().map(|file| file.path.clone()))
            .collect();
        assert_eq!(paths, vec![root.join(".env")]);
        assert!(is_skipped_dir("node_modules"));
        assert!(is_skipped_dir("Node_Modules"));
        assert!(!is_skipped_dir("src"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn env_filenames() {
        assert!(is_env_filename(".env"));
        assert!(is_env_filename(".env.local"));
        assert!(is_env_filename(".env.production"));
        assert!(is_env_filename("production.env"));
        assert!(is_env_filename(".ENV"));
        assert!(!is_env_filename(".envrc"));
        assert!(!is_env_filename(".environment"));
        assert!(!is_env_filename(".env.local.swp"));
        assert!(!is_env_filename(".env.envy-tmp"));
        assert!(!is_env_filename("readme.md"));
    }

    #[test]
    fn gitignored_env_files_are_kept_and_ignored_directories_are_not() {
        let root = scratch();
        let repo = root.join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::write(repo.join(".gitignore"), ".env\n*.env\nscratch-out/\n").unwrap();
        fs::write(repo.join(".ignore"), "skipme/\n").unwrap();
        fs::write(repo.join(".env"), "VISIBLE=1\n").unwrap();
        fs::write(repo.join("local.env"), "ALSO=1\n").unwrap();
        fs::create_dir_all(repo.join("scratch-out")).unwrap();
        fs::write(repo.join("scratch-out").join(".env"), "HIDDEN=1\n").unwrap();
        fs::create_dir_all(repo.join("skipme")).unwrap();
        fs::write(repo.join("skipme").join(".env"), "IGNORED=1\n").unwrap();
        fs::create_dir_all(repo.join("node_modules").join("pkg")).unwrap();
        fs::write(
            repo.join("node_modules").join("pkg").join(".env"),
            "NOPE=1\n",
        )
        .unwrap();

        let (mut projects, errors) = discover_files(&[root.clone()], &Config::default());
        assert!(errors.is_empty(), "{errors:?}");
        let paths: Vec<_> = projects
            .iter()
            .flat_map(|project| project.files.iter().map(|file| file.name.clone()))
            .collect();
        assert!(paths.contains(&".env".to_string()), "{paths:?}");
        assert!(paths.contains(&"local.env".to_string()), "{paths:?}");
        assert!(!paths
            .iter()
            .any(|name| name == "HIDDEN" || name.contains("skipme")));
        assert_eq!(projects[0].files.len(), 2, "{paths:?}");
        assert!(projects[0].files.iter().all(|file| file.keys.is_empty()));

        fill_key_index(&mut projects);
        let keys: Vec<_> = projects
            .iter()
            .flat_map(|project| project.files.iter().flat_map(|file| file.keys.clone()))
            .collect();
        assert!(keys.iter().any(|key| key == "VISIBLE"), "{keys:?}");
        assert!(keys.iter().any(|key| key == "ALSO"), "{keys:?}");
        assert!(!keys
            .iter()
            .any(|key| key == "HIDDEN" || key == "NOPE" || key == "IGNORED"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn index_cache_stores_paths_and_not_values() {
        let root = scratch();
        let repo = root.join("billing");
        fs::create_dir_all(&repo).unwrap();
        let secret = "SUPER_SECRET_VALUE_SHOULD_NOT_BE_CACHED";
        fs::write(repo.join(".env"), format!("TOKEN={secret}\n")).unwrap();
        let (projects, _) = discover_files(&[root.clone()], &Config::default());
        let cache = cache_from_projects(&[root.clone()], &projects);
        let cache_path = root.join("index-cache.toml");
        save_index_cache_to(&cache, &cache_path).unwrap();
        let text = fs::read_to_string(&cache_path).unwrap();
        assert!(!text.contains(secret), "{text}");
        assert!(text.contains(".env"), "{text}");
        let loaded = load_index_cache_from(&cache_path);
        let restored = projects_from_cache(&[root.clone()], &loaded);
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].files.len(), 1);
        assert!(restored[0].files[0].keys.is_empty());
        assert_eq!(restored[0].files[0].path, projects[0].files[0].path);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn scan_reports_the_first_file_before_the_walk_finishes() {
        let root = scratch();
        fs::write(root.join(".env"), "A=1\n").unwrap();
        fs::create_dir_all(root.join("later")).unwrap();
        fs::write(root.join("later").join(".env.local"), "B=1\n").unwrap();
        let seen = Mutex::new(Vec::new());
        let errors = scan_paths(&[root.clone()], |found| {
            seen.lock().expect("seen").push(found.name);
        });
        assert!(errors.is_empty(), "{errors:?}");
        let seen = seen.into_inner().expect("seen");
        assert!(seen.iter().any(|name| name == ".env"), "{seen:?}");
        assert!(seen.iter().any(|name| name == ".env.local"), "{seen:?}");
        let _ = fs::remove_dir_all(&root);
    }
}
