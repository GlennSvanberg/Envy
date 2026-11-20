//! One open `.env` file: undo, atomic save, and external-change detection.
//!
//! A save writes a sibling `*.envy-tmp` file, syncs it, then renames it over
//! the real file. A crash before the rename leaves the original intact. The
//! sibling is removed on failure and is not an env file Envy will open.
//! Undo and redo change the in-memory document only. The file is written when
//! [`OpenFile::save`] runs.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::document::EnvDocument;
use crate::paste::{self, PastePlan};

const UNDO_LIMIT: usize = 100;

#[derive(Clone, Debug)]
pub struct OpenFile {
    pub path: PathBuf,
    pub document: EnvDocument,
    undo: Vec<EnvDocument>,
    redo: Vec<EnvDocument>,
    pub mtime: Option<SystemTime>,
    /// Serialization last written to disk, or the text we loaded.
    clean: String,
    /// Disk differs from the document we have. The editor should ask before
    /// replacing in-memory edits.
    pub external_change: bool,
    /// mtime the user chose to keep editing over. Later polls ignore it until
    /// the file changes again.
    ignored_mtime: Option<SystemTime>,
}

impl OpenFile {
    pub fn load(path: impl Into<PathBuf>) -> io::Result<Self> {
        let path = path.into();
        let bytes = fs::read(&path)?;
        let text = String::from_utf8(bytes)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "file is not valid utf-8"))?;
        let mtime = fs::metadata(&path).and_then(|meta| meta.modified()).ok();
        let document = EnvDocument::parse(&text);
        Ok(Self {
            path,
            clean: document.serialize(),
            document,
            undo: Vec::new(),
            redo: Vec::new(),
            mtime,
            external_change: false,
            ignored_mtime: None,
        })
    }

    pub fn create(path: impl Into<PathBuf>, document: EnvDocument) -> io::Result<Self> {
        let path = path.into();
        if path.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "file already exists",
            ));
        }
        let mut open = Self {
            path,
            clean: String::new(),
            document,
            undo: Vec::new(),
            redo: Vec::new(),
            mtime: None,
            external_change: false,
            ignored_mtime: None,
        };
        open.save()?;
        Ok(open)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// True when the document differs from the last successful load or save.
    pub fn is_dirty(&self) -> bool {
        self.document.serialize() != self.clean
    }

    /// Snapshot, run `edit`, and remember the inverse for undo.
    pub fn mutate(&mut self, edit: impl FnOnce(&mut EnvDocument)) {
        self.push_undo();
        edit(&mut self.document);
        self.redo.clear();
    }

    pub fn apply_paste(&mut self, plan: &PastePlan) {
        self.mutate(|document| paste::apply_paste(document, plan));
    }

    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.undo.pop() else {
            return false;
        };
        self.redo.push(self.document.clone());
        self.document = previous;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        self.undo.push(self.document.clone());
        self.document = next;
        true
    }

    /// Write the document over the real file by renaming a synced sibling.
    pub fn save(&mut self) -> io::Result<()> {
        let text = self.document.serialize();
        write_atomic(&self.path, text.as_bytes())?;
        self.clean = text;
        self.mtime = fs::metadata(&self.path)
            .and_then(|meta| meta.modified())
            .ok();
        self.external_change = false;
        self.ignored_mtime = None;
        Ok(())
    }

    /// Replace the in-memory document with whatever is on disk now.
    pub fn reload(&mut self) -> io::Result<()> {
        let fresh = Self::load(&self.path)?;
        self.push_undo();
        self.document = fresh.document;
        self.mtime = fresh.mtime;
        self.external_change = false;
        self.ignored_mtime = None;
        self.redo.clear();
        Ok(())
    }

    /// Keep the in-memory document. The current disk mtime is ignored until
    /// the file changes again.
    pub fn dismiss_external(&mut self) {
        self.external_change = false;
        self.ignored_mtime = self.mtime;
    }

    /// Returns true when the file on disk changed and is not what we have.
    pub fn poll_disk(&mut self) -> io::Result<bool> {
        let meta = match fs::metadata(&self.path) {
            Ok(meta) => meta,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                self.external_change = true;
                return Ok(true);
            }
            Err(err) => return Err(err),
        };
        let mtime = meta.modified().ok();
        if mtime == self.mtime || mtime == self.ignored_mtime {
            return Ok(false);
        }
        let bytes = fs::read(&self.path)?;
        let text = match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(_) => {
                self.external_change = true;
                self.mtime = mtime;
                return Ok(true);
            }
        };
        self.mtime = mtime;
        if text == self.document.serialize() {
            self.external_change = false;
            self.ignored_mtime = None;
            return Ok(false);
        }
        self.external_change = true;
        Ok(true)
    }

    fn push_undo(&mut self) {
        self.undo.push(self.document.clone());
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
    }
}

/// Sibling used for one in-flight write. Same directory as `path`, so the
/// rename stays on one filesystem. The name ends in `.envy-tmp`, which
/// discovery ignores.
pub fn temp_sibling(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| "env".into());
    name.push(".envy-tmp");
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(name),
        _ => PathBuf::from(name),
    }
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let tmp = temp_sibling(path);
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        replace_file(&tmp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn replace_file(from: &Path, to: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        replace_file_windows(from, to)
    }
    #[cfg(not(windows))]
    {
        fs::rename(from, to)
    }
}

/// `MoveFileExW` replaces an existing file. `std::fs::rename` does not, on Windows.
#[cfg(windows)]
fn replace_file_windows(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }
    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }
    let from = wide(from);
    let to = wide(to);
    let ok = unsafe {
        MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn scratch() -> PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "envy-session-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn save_replaces_the_file_and_leaves_no_temp() {
        let dir = scratch();
        let path = dir.join(".env");
        fs::write(&path, "A=1\nB=2\n").unwrap();
        let mut open = OpenFile::load(&path).unwrap();
        assert!(!open.is_dirty());
        open.mutate(|document| {
            document.set_value("A", "9");
        });
        assert!(open.is_dirty());
        assert_eq!(fs::read_to_string(&path).unwrap(), "A=1\nB=2\n");
        open.save().unwrap();
        assert!(!open.is_dirty());
        assert_eq!(fs::read_to_string(&path).unwrap(), "A=9\nB=2\n");
        assert!(!temp_sibling(&path).exists());
        let names: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from(".env")]);
        assert!(open.undo());
        assert!(open.is_dirty());
        open.save().unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "A=1\nB=2\n");
        assert!(open.redo());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn external_change_is_noticed() {
        let dir = scratch();
        let path = dir.join(".env");
        fs::write(&path, "A=1\n").unwrap();
        let mut open = OpenFile::load(&path).unwrap();
        assert!(!open.poll_disk().unwrap());
        // Guarantee the mtime moves even on coarse filesystems.
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(&path, "A=2\n").unwrap();
        // Some filesystems round mtime to 1s. If the stamp did not move, poke it.
        if fs::metadata(&path).unwrap().modified().ok() == open.mtime {
            let later = open.mtime.unwrap() + std::time::Duration::from_secs(2);
            let _ = filetime_set(&path, later);
        }
        assert!(open.poll_disk().unwrap());
        assert!(open.external_change);
        open.reload().unwrap();
        assert_eq!(open.document.value_of("A"), Some("2"));
        assert!(!open.external_change);
        let _ = fs::remove_dir_all(&dir);
    }

    fn filetime_set(path: &Path, time: SystemTime) {
        let std_time = fs::File::options().write(true).open(path).ok();
        if let Some(file) = std_time {
            let _ = file.set_modified(time);
        }
    }
}
