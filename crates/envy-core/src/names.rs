//! Env file names for creating and renaming a file in a project.

use std::fs;
use std::io;
use std::path::Path;

use crate::discover::is_env_filename;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvFileNameError {
    Empty,
    /// A slash or backslash would create a folder, not rename the file.
    Nested,
    NotEnv,
    Collision,
}

/// Trim and check that `name` is a single env file name.
pub fn normalize_env_filename(name: &str) -> Result<String, EnvFileNameError> {
    let name = name.trim();
    if name.is_empty() || name == "." || name == ".." {
        return Err(EnvFileNameError::Empty);
    }
    if name.contains('/') || name.contains('\\') || name.contains('\0') {
        return Err(EnvFileNameError::Nested);
    }
    if !is_env_filename(name) {
        return Err(EnvFileNameError::NotEnv);
    }
    Ok(name.to_string())
}

/// Case-insensitive collision. `ignoring` is the file being renamed, so a
/// case-only change of that name is allowed.
pub fn env_name_taken(name: &str, existing: &[impl AsRef<str>], ignoring: Option<&str>) -> bool {
    existing.iter().any(|other| {
        let other = other.as_ref();
        if ignoring.is_some_and(|current| current.eq_ignore_ascii_case(other)) {
            return false;
        }
        other.eq_ignore_ascii_case(name)
    })
}

const NEW_ENV_NAMES: &[&str] = &[".env", ".env.local", ".env.development", ".env.dev"];

/// The name a new file starts with. `.env` when it is free, then the next
/// common name, then `.env.2`, `.env.3`, and so on.
pub fn next_env_filename(existing: &[impl AsRef<str>]) -> String {
    for name in NEW_ENV_NAMES {
        if !env_name_taken(name, existing, None) {
            return (*name).to_string();
        }
    }
    let mut n = 2u32;
    loop {
        let name = format!(".env.{n}");
        if !env_name_taken(&name, existing, None) {
            return name;
        }
        n = n.saturating_add(1);
        if n == u32::MAX {
            return ".env.new".to_string();
        }
    }
}

pub fn check_env_filename(
    name: &str,
    existing: &[impl AsRef<str>],
    ignoring: Option<&str>,
) -> Result<String, EnvFileNameError> {
    let name = normalize_env_filename(name)?;
    if env_name_taken(&name, existing, ignoring) {
        return Err(EnvFileNameError::Collision);
    }
    Ok(name)
}

/// Rename `from` to `to`. A case-only change renames via a temporary name so
/// it succeeds on case-insensitive disks. Fails when `to` is a different file
/// that already exists.
pub fn rename_env_file(from: &Path, to: &Path) -> io::Result<()> {
    if from == to {
        return Ok(());
    }
    if to.exists() && !same_file_name_case(from, to) {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "already exists",
        ));
    }
    if to.exists() {
        let tmp = from.with_file_name(format!(".{}.envy-tmp-rename", std::process::id()));
        fs::rename(from, &tmp)?;
        match fs::rename(&tmp, to) {
            Ok(()) => Ok(()),
            Err(err) => {
                let _ = fs::rename(&tmp, from);
                Err(err)
            }
        }
    } else {
        fs::rename(from, to)
    }
}

fn same_file_name_case(from: &Path, to: &Path) -> bool {
    match (from.file_name(), to.file_name(), from.parent(), to.parent()) {
        (Some(left), Some(right), Some(left_dir), Some(right_dir)) => {
            left_dir == right_dir
                && left
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&right.to_string_lossy())
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn names_must_be_a_single_env_file_and_must_not_collide() {
        assert_eq!(
            check_env_filename("  .env.local  ", &[".env"], None).as_deref(),
            Ok(".env.local")
        );
        assert_eq!(
            check_env_filename("", &[] as &[&str], None),
            Err(EnvFileNameError::Empty)
        );
        assert_eq!(
            check_env_filename("notes.txt", &[] as &[&str], None),
            Err(EnvFileNameError::NotEnv)
        );
        assert_eq!(
            check_env_filename("dir/.env", &[] as &[&str], None),
            Err(EnvFileNameError::Nested)
        );
        assert_eq!(
            check_env_filename(".ENV", &[".env"], None),
            Err(EnvFileNameError::Collision)
        );
        assert_eq!(
            check_env_filename(".ENV", &[".env"], Some(".env")).as_deref(),
            Ok(".ENV")
        );
    }

    #[test]
    fn new_file_prefers_env_then_the_next_free_name() {
        assert_eq!(next_env_filename(&[] as &[&str]), ".env");
        assert_eq!(next_env_filename(&[".env"]), ".env.local");
        assert_eq!(
            next_env_filename(&[".env", ".env.local", ".env.development", ".env.dev"]),
            ".env.2"
        );
        assert_eq!(
            next_env_filename(&[
                ".ENV",
                ".env.local",
                ".env.development",
                ".env.dev",
                ".env.2"
            ]),
            ".env.3"
        );
    }

    #[test]
    fn rename_moves_the_file_and_refuses_a_collision() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("envy-rename-{stamp}"));
        fs::create_dir_all(&dir).unwrap();
        let from = dir.join(".env");
        let other = dir.join(".env.local");
        let to = dir.join(".env.development");
        fs::write(&from, "A=1\n").unwrap();
        fs::write(&other, "B=2\n").unwrap();

        let err = rename_env_file(&from, &other).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert!(from.exists());

        rename_env_file(&from, &to).unwrap();
        assert!(!from.exists());
        assert_eq!(fs::read_to_string(&to).unwrap(), "A=1\n");
        let _ = fs::remove_dir_all(&dir);
    }
}
