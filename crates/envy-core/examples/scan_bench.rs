//! Time a tree shaped like a large checkout folder.
//!
//! ```text
//! cargo run -p envy-core --release --example scan_bench
//! ```
//!
//! Builds 50 repos and about 200k entries, then times the previous serial
//! walk against the current parallel walk. Not part of `cargo test`.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Instant;

use envy_core::{is_env_filename, scan_paths, ScanError};

const REPOS: usize = 50;
const SRC_FILES: usize = 40;
const NODE_MODULES_FILES: usize = 1_500;
const TARGET_FILES: usize = 800;
const BIN_FILES: usize = 500;
const OBJ_FILES: usize = 500;
const VS_FILES: usize = 400;
const PACKAGES_FILES: usize = 280;

const OLD_SKIP: &[&str] = &[
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

fn main() {
    let root = std::env::temp_dir().join(format!("envy-scan-bench-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let started = Instant::now();
    let entries = build_tree(&root);
    println!(
        "built_entries={entries} build_ms={}",
        started.elapsed().as_millis()
    );

    let first = Instant::now();
    let (tx, rx) = mpsc::channel();
    let root_for_scan = root.clone();
    let worker = std::thread::spawn(move || {
        scan_paths(&[root_for_scan], |found| {
            let _ = tx.send(found.path);
        })
    });
    let first_path = rx.recv().expect("at least one env file");
    let first_us = first.elapsed().as_micros();
    let mut found = 1usize;
    while rx.try_recv().is_ok() {
        found += 1;
    }
    let errors = worker.join().expect("scan");
    while rx.try_recv().is_ok() {
        found += 1;
    }
    let current_us = first.elapsed().as_micros();
    println!(
        "current_first_us={first_us} current_full_us={current_us} current_files={found} errors={} first={}",
        errors.len(),
        first_path.display()
    );

    let previous = Instant::now();
    let mut previous_files = Vec::new();
    let mut previous_errors = Vec::new();
    walk_previous(&root, 0, &mut previous_files, &mut previous_errors);
    println!(
        "previous_full_us={} previous_files={} previous_errors={}",
        previous.elapsed().as_micros(),
        previous_files.len(),
        previous_errors.len()
    );

    let _ = fs::remove_dir_all(&root);
}

fn build_tree(root: &Path) -> u64 {
    let count = Arc::new(AtomicU64::new(0));
    let mut handles = Vec::new();
    for index in 0..REPOS {
        let repo = root.join(format!("repo-{index:02}"));
        let count = Arc::clone(&count);
        handles.push(std::thread::spawn(move || {
            fs::create_dir_all(repo.join(".git")).unwrap();
            fs::write(repo.join(".git").join("HEAD"), "ref: refs/heads/main\n").unwrap();
            fs::write(repo.join(".gitignore"), "node_modules/\ntarget/\n.env\n").unwrap();
            fs::write(repo.join(".env"), "TOKEN=1\n").unwrap();
            fs::write(repo.join(".env.local"), "LOCAL=1\n").unwrap();
            count.fetch_add(4, Ordering::Relaxed);
            write_files(&repo.join("src"), "f", SRC_FILES, &count);
            write_files(&repo.join("node_modules"), "m", NODE_MODULES_FILES, &count);
            write_files(&repo.join("target"), "t", TARGET_FILES, &count);
            write_files(&repo.join("bin"), "b", BIN_FILES, &count);
            write_files(&repo.join("obj"), "o", OBJ_FILES, &count);
            write_files(&repo.join(".vs"), "v", VS_FILES, &count);
            write_files(&repo.join("packages"), "p", PACKAGES_FILES, &count);
        }));
    }
    for handle in handles {
        handle.join().unwrap();
    }
    count.load(Ordering::Relaxed)
}

fn write_files(dir: &Path, prefix: &str, n: usize, count: &AtomicU64) {
    fs::create_dir_all(dir).unwrap();
    for index in 0..n {
        File::create(dir.join(format!("{prefix}{index}.txt"))).unwrap();
    }
    count.fetch_add(n as u64, Ordering::Relaxed);
}

fn walk_previous(dir: &Path, depth: usize, found: &mut Vec<PathBuf>, errors: &mut Vec<ScanError>) {
    if depth > envy_core::MAX_SCAN_DEPTH {
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
                    path,
                    message: err.to_string(),
                });
                continue;
            }
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            if OLD_SKIP.iter().any(|skip| name == *skip) {
                continue;
            }
            walk_previous(&path, depth + 1, found, errors);
            continue;
        }
        if !meta.is_file() || !is_env_filename(&name) {
            continue;
        }
        let _ = fs::read(&path);
        found.push(path);
    }
}
