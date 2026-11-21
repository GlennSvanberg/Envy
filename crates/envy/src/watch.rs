//! Watch added folders for new or removed env files.
//!
//! The watch is not recursive. A recursive watch on a tree the size of
//! `C:\git` spends its time inside `node_modules`. The scan already lists
//! project roots; we watch those, plus each folder the user added.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::Duration;

use envy_core::is_env_filename;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};

pub struct FolderWatch {
    ping: Receiver<()>,
    stop: Sender<()>,
}

impl FolderWatch {
    pub fn pinged(&self) -> bool {
        match self.ping.try_recv() {
            Ok(()) => {
                while self.ping.try_recv().is_ok() {}
                true
            }
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => false,
        }
    }
}

impl Drop for FolderWatch {
    fn drop(&mut self) {
        let _ = self.stop.send(());
    }
}

pub fn spawn(paths: Vec<PathBuf>) -> FolderWatch {
    let (ping_tx, ping_rx) = mpsc::channel();
    let (stop_tx, stop_rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("envy-watch".into())
        .spawn(move || run(paths, ping_tx, stop_rx))
        .ok();
    FolderWatch {
        ping: ping_rx,
        stop: stop_tx,
    }
}

fn run(paths: Vec<PathBuf>, ping: Sender<()>, stop: Receiver<()>) {
    let (tx, rx) = mpsc::channel();
    let mut watcher = match RecommendedWatcher::new(
        tx,
        notify::Config::default().with_poll_interval(Duration::from_secs(4)),
    ) {
        Ok(watcher) => watcher,
        Err(_) => return,
    };
    for path in &paths {
        if path.is_dir() {
            let _ = watcher.watch(path, RecursiveMode::NonRecursive);
        }
    }
    let mut due = false;
    loop {
        if matches!(stop.try_recv(), Ok(()) | Err(TryRecvError::Disconnected)) {
            break;
        }
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(Ok(event)) if is_env_event(&event) => due = true,
            Ok(_) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if due {
            let _ = ping.send(());
            due = false;
            // Swallow the burst that follows a single save.
            let _ = rx.recv_timeout(Duration::from_millis(400));
            while rx.try_recv().is_ok() {}
        }
    }
}

fn is_env_event(event: &Event) -> bool {
    event.paths.iter().any(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(is_env_filename)
            || path
                .parent()
                .and_then(Path::file_name)
                .and_then(|name| name.to_str())
                .is_some_and(is_env_filename)
    })
}
