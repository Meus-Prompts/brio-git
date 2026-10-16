//! Debounced file system watcher for a repository.

use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use futures::channel::mpsc::{UnboundedReceiver, unbounded};
use notify::{EventKind, RecursiveMode, Watcher};

/// What changed during one debounce window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ChangeBatch {
    /// Files of the working tree changed.
    pub worktree: bool,
    /// `HEAD`, refs or the index changed (commits, checkouts, staging).
    pub git: bool,
}

impl ChangeBatch {
    fn is_empty(&self) -> bool {
        !self.worktree && !self.git
    }
}

/// Keeps the watcher alive; dropping it stops the events.
pub struct WatchHandle {
    _watcher: notify::RecommendedWatcher,
}

const QUIET: Duration = Duration::from_millis(250);
const MAX_DELAY: Duration = Duration::from_secs(1);

/// Watches `workdir` (and `git_dir` when it lives elsewhere) and emits batches
/// at most every [`MAX_DELAY`], after [`QUIET`] without new events.
pub fn watch(workdir: &Path, git_dir: &Path) -> anyhow::Result<(WatchHandle, UnboundedReceiver<ChangeBatch>)> {
    let (raw_tx, raw_rx) = mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher = notify::recommended_watcher(raw_tx)?;
    watcher.watch(workdir, RecursiveMode::Recursive)?;
    let workdir = workdir.canonicalize()?;
    let git_dir = git_dir.canonicalize()?;
    if !git_dir.starts_with(&workdir) {
        watcher.watch(&git_dir, RecursiveMode::Recursive)?;
    }

    let (tx, rx) = unbounded();
    std::thread::Builder::new().name("brio-watcher".into()).spawn(move || {
        let mut batch = ChangeBatch::default();
        let mut first: Option<Instant> = None;
        loop {
            let timeout = match first {
                Some(t) => QUIET.min(MAX_DELAY.saturating_sub(t.elapsed())),
                None => Duration::from_secs(3600),
            };
            match raw_rx.recv_timeout(timeout) {
                Ok(Ok(event)) => {
                    if matches!(event.kind, EventKind::Access(_)) {
                        continue;
                    }
                    for path in &event.paths {
                        classify(&workdir, &git_dir, path, &mut batch);
                    }
                    if !batch.is_empty() && first.is_none() {
                        first = Some(Instant::now());
                    }
                }
                Ok(Err(_)) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if !batch.is_empty() {
                        if tx.unbounded_send(std::mem::take(&mut batch)).is_err() {
                            return;
                        }
                        first = None;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
    })?;

    Ok((WatchHandle { _watcher: watcher }, rx))
}

fn classify(workdir: &Path, git_dir: &Path, path: &Path, batch: &mut ChangeBatch) {
    if let Ok(rel) = path.strip_prefix(git_dir) {
        if is_git_state(rel) {
            batch.git = true;
        }
    } else if path.starts_with(workdir) {
        batch.worktree = true;
    }
}

/// Files inside `.git` whose change means the visible state changed.
fn is_git_state(rel: &Path) -> bool {
    if rel.extension().is_some_and(|e| e == "lock") {
        return false;
    }
    let mut parts = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned());
    matches!(
        parts.next().as_deref(),
        Some("HEAD" | "index" | "packed-refs" | "refs" | "MERGE_HEAD" | "REBASE_HEAD" | "CHERRY_PICK_HEAD")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_git_internals() {
        assert!(is_git_state(Path::new("HEAD")));
        assert!(is_git_state(Path::new("refs/heads/main")));
        assert!(is_git_state(Path::new("index")));
        assert!(!is_git_state(Path::new("index.lock")));
        assert!(!is_git_state(Path::new("objects/ab/cdef")));
        assert!(!is_git_state(Path::new("brio-index-1-2")));
    }

    #[test]
    fn reports_worktree_changes() {
        let dir = tempfile::tempdir().unwrap();
        let git = dir.path().join(".git");
        std::fs::create_dir(&git).unwrap();
        let (_handle, mut rx) = watch(dir.path(), &git).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        std::fs::write(dir.path().join("file.txt"), "x").unwrap();
        std::fs::write(git.join("HEAD"), "ref: refs/heads/main").unwrap();

        let mut seen = ChangeBatch::default();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !(seen.worktree && seen.git) {
            assert!(Instant::now() < deadline, "no events, got {seen:?}");
            if let Ok(b) = rx.try_recv() {
                seen.worktree |= b.worktree;
                seen.git |= b.git;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
