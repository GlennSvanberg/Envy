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
use std::time::{Duration, SystemTime};

use crate::document::EnvDocument;
use crate::paste::{self, PastePlan};

const UNDO_LIMIT: usize = 100;

/// How long typing waits before the file is written. Enter, Tab, blur,
/// a click outside, a file or project change, and quitting do not wait.
pub const SAVE_PAUSE: Duration = Duration::from_millis(300);

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

    /// Update the in-memory document directly from plain-text edits without
    /// pushing an undo snapshot per keystroke (used while the text editor owns
    /// fine-grained undo/redo). Returns true when the text actually changed.
    pub fn set_text_live(&mut self, text: &str) -> bool {
        if self.document.serialize() == text {
            return false;
        }
        self.document = EnvDocument::parse(text);
        true
    }

    /// Commit a completed plain-text editing session into the document's undo
    /// stack as one step. Returns true if the text differs from `before`.
    pub fn commit_text_snapshot(&mut self, before: EnvDocument, after_text: &str) -> bool {
        if before.serialize() == after_text {
            self.document = before;
            return false;
        }
        self.undo.push(before);
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.document = EnvDocument::parse(after_text);
        true
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

    /// Pieces a background task needs to notice an external edit.
    pub fn disk_parts(&self) -> (PathBuf, Option<SystemTime>, Option<SystemTime>, EnvDocument) {
        (
            self.path.clone(),
            self.mtime,
            self.ignored_mtime,
            self.document.clone(),
        )
    }

    /// Remember a save that already landed on disk. Does not touch the disk.
    pub fn note_saved(&mut self, text: String, mtime: Option<SystemTime>) {
        self.clean = text;
        self.mtime = mtime;
        self.external_change = false;
        self.ignored_mtime = None;
    }

    /// Swap in a document that was loaded on a background task.
    pub fn replace_from_disk(&mut self, fresh: OpenFile) {
        self.push_undo();
        self.document = fresh.document;
        self.clean = fresh.clean;
        self.mtime = fresh.mtime;
        self.external_change = false;
        self.ignored_mtime = None;
        self.redo.clear();
    }

    /// Returns true when the file on disk changed and is not what we have.
    pub fn poll_disk(&mut self) -> io::Result<bool> {
        let notice = inspect_disk(
            &self.path,
            self.mtime,
            self.ignored_mtime,
            &self.document.serialize(),
        )?;
        Ok(self.apply_disk_notice(notice))
    }

    /// Apply a notice produced by [`inspect_disk`] on another thread.
    /// Returns true when the editor should tell the user.
    pub fn apply_disk_notice(&mut self, notice: DiskNotice) -> bool {
        match notice {
            DiskNotice::Quiet => false,
            DiskNotice::Adopted(mtime) => {
                self.mtime = mtime;
                self.external_change = false;
                self.ignored_mtime = None;
                false
            }
            DiskNotice::External(mtime) => {
                let tell = !self.external_change;
                self.mtime = mtime.or(self.mtime);
                self.external_change = true;
                tell
            }
        }
    }

    fn push_undo(&mut self) {
        self.undo.push(self.document.clone());
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
    }

    /// Remember the document once, at the start of a cell edit.
    pub fn arm_undo(&mut self) {
        self.push_undo();
        self.redo.clear();
    }

    /// Drop an in-progress cell edit. `armed` means [`Self::arm_undo`] ran.
    pub fn restore_baseline(&mut self, baseline: EnvDocument, armed: bool) {
        if armed
            && self
                .undo
                .last()
                .is_some_and(|doc| doc.serialize() == baseline.serialize())
        {
            self.undo.pop();
        }
        self.document = baseline;
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

/// What [`inspect_disk`] found. Produced off the UI thread and applied later.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiskNotice {
    /// The mtime is one we already accounted for.
    Quiet,
    /// The bytes match the open document. Keep the new mtime.
    Adopted(Option<SystemTime>),
    /// The file differs, or it is gone.
    External(Option<SystemTime>),
}

/// Read `path` and compare it with `current` without mutating editor state.
pub fn inspect_disk(
    path: &Path,
    known: Option<SystemTime>,
    ignored: Option<SystemTime>,
    current: &str,
) -> io::Result<DiskNotice> {
    let meta = match fs::metadata(path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            return Ok(DiskNotice::External(None));
        }
        Err(err) => return Err(err),
    };
    let mtime = meta.modified().ok();
    if mtime == known || mtime == ignored {
        return Ok(DiskNotice::Quiet);
    }
    let bytes = fs::read(path)?;
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(_) => return Ok(DiskNotice::External(mtime)),
    };
    if text == current {
        Ok(DiskNotice::Adopted(mtime))
    } else {
        Ok(DiskNotice::External(mtime))
    }
}

/// Atomic save of `text`. Returns the mtime after the rename.
/// Call this from a background task; it syncs a sibling file and renames it.
pub fn save_text(path: &Path, text: &str) -> io::Result<Option<SystemTime>> {
    write_atomic(path, text.as_bytes())?;
    Ok(fs::metadata(path).and_then(|meta| meta.modified()).ok())
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

    #[test]
    fn text_and_document_sync_keeps_lossless_state_unreadable_lines_and_undo() {
        let dir = scratch();
        let path = dir.join(".env");
        let original = "# --- DB ---\r\nDB_PORT=5432\r\n# DEBUG=true\r\n";
        fs::write(&path, original).unwrap();
        let mut open = OpenFile::load(&path).unwrap();
        assert!(!open.is_dirty());

        // Table edit -> text view serialization preserves CRLF and untouched lines.
        open.mutate(|doc| {
            doc.set_value("DB_PORT", "5433");
        });
        let before_text_mode = open.document.clone();
        assert_eq!(
            before_text_mode.serialize(),
            "# --- DB ---\r\nDB_PORT=5433\r\n# DEBUG=true\r\n"
        );
        assert!(open.is_dirty());

        // No-op live text sync returns false and leaves document untouched.
        assert!(!open.set_text_live(&before_text_mode.serialize()));

        // Edit in text mode, including an unreadable line and toggling DEBUG on.
        let edited_text = "# --- DB ---\r\nDB_PORT=5433\r\nDEBUG=true\r\nbroken line\r\n";
        assert!(open.set_text_live(edited_text));
        assert!(open.is_dirty());
        assert_eq!(open.document.value_of("DEBUG"), Some("true"));
        assert_eq!(open.document.unreadable_lines().len(), 1);
        assert_eq!(open.document.unreadable_lines()[0].line_number, 4);
        assert_eq!(open.document.serialize(), edited_text);

        // Switching back to table commits one undo step for the text session.
        assert!(open.commit_text_snapshot(before_text_mode.clone(), edited_text));
        assert_eq!(open.document.serialize(), edited_text);

        // Undo rolls back the text session cleanly to the pre-text table edit.
        assert!(open.undo());
        assert_eq!(open.document.serialize(), before_text_mode.serialize());
        assert!(open.document.unreadable_lines().is_empty());

        // Second undo restores the exact original clean file bytes.
        assert!(open.undo());
        assert_eq!(open.document.serialize(), original);
        assert!(!open.is_dirty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn typing_waits_three_hundred_milliseconds() {
        assert_eq!(SAVE_PAUSE, Duration::from_millis(300));
    }

    #[test]
    fn editing_a_value_cell_changes_the_file_on_disk() {
        use crate::document::Node;

        let dir = scratch();
        let path = dir.join(".env");
        fs::write(&path, "TOKEN=old\nKEEP=1\n").unwrap();
        let mut open = OpenFile::load(&path).unwrap();
        open.mutate(|document| {
            document.set_value("TOKEN", "new");
        });
        open.save().unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "TOKEN=new\nKEEP=1\n");
        assert!(!open.is_dirty());
        assert!(!temp_sibling(&path).exists());

        open.mutate(|document| {
            if let Some(Node::Entry(entry)) = document
                .nodes
                .iter_mut()
                .find(|node| matches!(node, Node::Entry(entry) if entry.key == "TOKEN"))
            {
                entry.set_key("TOKEN2");
            }
        });
        open.save().unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "TOKEN2=new\nKEEP=1\n");

        open.mutate(|document| {
            document.push_entry("ADDED", "yes");
        });
        open.save().unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "TOKEN2=new\nKEEP=1\nADDED=yes\n"
        );
        assert!(!temp_sibling(&path).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn editing_a_crlf_value_keeps_crlf_on_disk() {
        let dir = scratch();
        let path = dir.join(".env");
        fs::write(&path, "TOKEN=old\r\nKEEP=1\r\n").unwrap();
        let mut open = OpenFile::load(&path).unwrap();
        open.mutate(|document| {
            document.set_value("TOKEN", "new");
        });
        open.save().unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "TOKEN=new\r\nKEEP=1\r\n"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cancel_after_a_preview_writes_the_original_back() {
        let dir = scratch();
        let path = dir.join(".env");
        fs::write(&path, "TOKEN=old\n").unwrap();
        let mut open = OpenFile::load(&path).unwrap();
        let baseline = open.document.clone();
        open.arm_undo();
        let mut preview = baseline.clone();
        preview.set_value("TOKEN", "typing");
        open.document = preview;
        open.save().unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "TOKEN=typing\n");
        open.restore_baseline(baseline, true);
        assert!(open.is_dirty());
        open.save().unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "TOKEN=old\n");
        assert!(!open.is_dirty());
        assert!(!open.can_undo());
        assert!(!open.can_redo());
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_save_keeps_the_original_and_returns_the_error() {
        let dir = scratch();
        let path = dir.join(".env");
        fs::write(&path, "TOKEN=old\n").unwrap();
        let mut open = OpenFile::load(&path).unwrap();
        open.mutate(|document| {
            document.set_value("TOKEN", "new");
        });
        let _guard = DirMode::readonly(&dir);
        let err = open.save().unwrap_err();
        drop(_guard);
        assert_eq!(fs::read_to_string(&path).unwrap(), "TOKEN=old\n");
        assert!(!temp_sibling(&path).exists());
        assert!(open.is_dirty());
        let _ = err;
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn verbatim_prefix_value_edit_lands_on_disk() {
        let dir = scratch();
        let path = dir.join(".env");
        fs::write(&path, "TOKEN=old\r\n").unwrap();
        let canon = fs::canonicalize(&path).unwrap();
        assert!(
            canon.to_string_lossy().starts_with(r"\\?\"),
            "canonical path was {}",
            canon.display()
        );
        let mut open = OpenFile::load(&canon).unwrap();
        open.mutate(|document| {
            document.set_value("TOKEN", "new");
        });
        open.save().unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "TOKEN=new\r\n");
        assert!(!temp_sibling(&canon).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn a_locked_file_reports_the_error_and_keeps_the_original() {
        use std::os::windows::fs::OpenOptionsExt;

        const FILE_SHARE_READ: u32 = 1;
        let dir = scratch();
        let path = dir.join(".env");
        fs::write(&path, "TOKEN=old\r\n").unwrap();
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(&path)
            .unwrap();
        let mut open = OpenFile::load(&path).unwrap();
        open.mutate(|document| {
            document.set_value("TOKEN", "new");
        });
        let err = open.save().unwrap_err();
        drop(lock);
        assert_eq!(fs::read_to_string(&path).unwrap(), "TOKEN=old\r\n");
        assert!(!temp_sibling(&path).exists());
        assert!(open.is_dirty());
        let _ = err;
        let _ = fs::remove_dir_all(&dir);
    }
}

/// Restores a directory mode even when the test fails.
#[cfg(all(test, unix))]
struct DirMode {
    path: PathBuf,
    mode: u32,
}

#[cfg(all(test, unix))]
impl DirMode {
    fn readonly(path: &Path) -> Self {
        let mode = mode_of(path);
        set_mode(path, 0o555);
        Self {
            path: path.to_path_buf(),
            mode,
        }
    }
}

#[cfg(all(test, unix))]
impl Drop for DirMode {
    fn drop(&mut self) {
        set_mode(&self.path, self.mode);
    }
}

#[cfg(all(test, unix))]
fn mode_of(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).unwrap().permissions().mode()
}

#[cfg(all(test, unix))]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(mode);
    fs::set_permissions(path, perms).unwrap();
}
