//! Snapshots of the working tree stored under `refs/lazygit/checkpoints/`.
//!
//! A checkpoint is a dangling commit whose tree is the full working tree
//! (tracked and untracked, respecting `.gitignore`) and whose parent is the
//! `HEAD` at that moment. Neither the branch, the index nor the stash is
//! touched: every step runs against a temporary index file.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::git::{Git, Result};

pub const REF_PREFIX: &str = "refs/lazygit/checkpoints";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkpoint {
    pub session: String,
    pub seq: u32,
    /// Commit id of the snapshot.
    pub id: String,
    /// Creation time, seconds since the Unix epoch.
    pub time: i64,
    pub label: String,
}

impl Checkpoint {
    pub fn ref_name(&self) -> String {
        ref_name(&self.session, self.seq)
    }
}

fn ref_name(session: &str, seq: u32) -> String {
    format!("{REF_PREFIX}/{session}/{seq:04}")
}

/// Takes a checkpoint unless the working tree is identical to the session's
/// latest one, in which case `Ok(None)` is returned.
pub fn create(git: &Git, session: &str, seq: u32, label: &str) -> Result<Option<Checkpoint>> {
    let tree = snapshot_tree(git)?;
    if let Some(last) = list(git, session)?.last() {
        let last_tree = git.run(&["rev-parse", &format!("{}^{{tree}}", last.id)])?;
        if last_tree.trim() == tree {
            return Ok(None);
        }
    }
    let id = commit_tree(git, &tree, &format!("checkpoint {session}#{seq}: {label}"))?;
    git.run(&["update-ref", &ref_name(session, seq), &id])?;
    Ok(list(git, session)?.into_iter().find(|c| c.seq == seq))
}

/// Checkpoints of one session, oldest first.
pub fn list(git: &Git, session: &str) -> Result<Vec<Checkpoint>> {
    let out = git.run(&[
        "for-each-ref",
        "--format=%(refname)%00%(objectname)%00%(creatordate:unix)%00%(contents:subject)",
        &format!("{REF_PREFIX}/{session}/"),
    ])?;
    let mut checkpoints: Vec<Checkpoint> = out.lines().filter_map(parse_line).collect();
    checkpoints.sort_by_key(|c| c.seq);
    Ok(checkpoints)
}

/// Session ids that have checkpoints, oldest first (ids sort chronologically).
pub fn sessions(git: &Git) -> Result<Vec<String>> {
    let out = git.run(&["for-each-ref", "--format=%(refname)", &format!("{REF_PREFIX}/")])?;
    let mut ids: Vec<String> = out
        .lines()
        .filter_map(|r| r.strip_prefix(&format!("{REF_PREFIX}/"))?.split('/').next().map(str::to_owned))
        .collect();
    ids.sort();
    ids.dedup();
    Ok(ids)
}

/// Makes the working tree match `target`. The current state is saved first as
/// checkpoint `safety_seq` of `target.session`, so the restore can be undone.
pub fn restore(git: &Git, target: &Checkpoint, safety_seq: u32) -> Result<Checkpoint> {
    let tree = snapshot_tree(git)?;
    let current = commit_tree(git, &tree, &format!("checkpoint {}#{safety_seq}: antes de restaurar #{}", target.session, target.seq))?;
    git.run(&["update-ref", &ref_name(&target.session, safety_seq), &current])?;

    // Files that exist now but not in the target must go away.
    let added = git.run(&["diff-tree", "-r", "-z", "--name-only", "--no-renames", "--diff-filter=A", &target.id, &current])?;
    for path in added.split('\0').filter(|p| !p.is_empty()) {
        let full = git.workdir().join(path);
        if full.is_file() || full.is_symlink() {
            std::fs::remove_file(&full)?;
            remove_empty_parents(git.workdir(), &full);
        }
    }

    let index = TempIndex::new(git)?;
    let env = [("GIT_INDEX_FILE", index.path.as_os_str())];
    git.run_with_env(&["read-tree", &target.id], &env)?;
    git.run_with_env(&["checkout-index", "-a", "-f"], &env)?;

    let saved = list(git, &target.session)?;
    Ok(saved.into_iter().find(|c| c.seq == safety_seq).expect("safety checkpoint was just written"))
}

/// Deletes every checkpoint of `session`.
pub fn delete_session(git: &Git, session: &str) -> Result<()> {
    for cp in list(git, session)? {
        git.run(&["update-ref", "-d", &cp.ref_name()])?;
    }
    Ok(())
}

fn parse_line(line: &str) -> Option<Checkpoint> {
    let mut parts = line.split('\0');
    let refname = parts.next()?;
    let id = parts.next()?.to_owned();
    let time = parts.next()?.parse().unwrap_or_default();
    let subject = parts.next().unwrap_or_default();
    let rest = refname.strip_prefix(REF_PREFIX)?.trim_start_matches('/');
    let (session, seq) = rest.split_once('/')?;
    let label = subject.split_once(": ").map_or(subject, |(_, l)| l).to_owned();
    Some(Checkpoint { session: session.to_owned(), seq: seq.parse().ok()?, id, time, label })
}

/// Writes the whole working tree as a tree object and returns its id.
fn snapshot_tree(git: &Git) -> Result<String> {
    let index = TempIndex::new(git)?;
    let env = [("GIT_INDEX_FILE", index.path.as_os_str())];
    if !index.seeded && has_head(git)? {
        git.run_with_env(&["read-tree", "HEAD"], &env)?;
    }
    git.run_with_env(&["add", "-A", "--", "."], &env)?;
    Ok(git.run_with_env(&["write-tree"], &env)?.trim().to_owned())
}

fn commit_tree(git: &Git, tree: &str, message: &str) -> Result<String> {
    let identity: [(&str, &OsStr); 4] = [
        ("GIT_AUTHOR_NAME", OsStr::new("lazy-git")),
        ("GIT_AUTHOR_EMAIL", OsStr::new("lazy-git@localhost")),
        ("GIT_COMMITTER_NAME", OsStr::new("lazy-git")),
        ("GIT_COMMITTER_EMAIL", OsStr::new("lazy-git@localhost")),
    ];
    let mut args = vec!["commit-tree", tree, "-m", message];
    if has_head(git)? {
        args.extend(["-p", "HEAD"]);
    }
    Ok(git.run_with_env(&args, &identity)?.trim().to_owned())
}

fn has_head(git: &Git) -> Result<bool> {
    Ok(git.try_run(&["rev-parse", "--verify", "-q", "HEAD"])?.is_some())
}

fn remove_empty_parents(root: &Path, file: &Path) {
    let mut dir = file.parent();
    while let Some(d) = dir {
        if d == root || std::fs::remove_dir(d).is_err() {
            break;
        }
        dir = d.parent();
    }
}

/// A private index file inside the git dir, deleted on drop.
struct TempIndex {
    path: PathBuf,
    /// Whether it starts as a copy of the real index (faster `git add`).
    seeded: bool,
}

impl TempIndex {
    fn new(git: &Git) -> Result<Self> {
        let git_dir = git.resolve(&git.run(&["rev-parse", "--git-dir"])?);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let path = git_dir.join(format!("lazygit-index-{}-{nanos}", std::process::id()));
        let real = git.resolve(&git.run(&["rev-parse", "--git-path", "index"])?);
        let seeded = std::fs::copy(&real, &path).is_ok();
        Ok(Self { path, seeded })
    }
}

impl Drop for TempIndex {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_file(self.path.with_extension("lock"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn repo() -> (tempfile::TempDir, Git) {
        let dir = tempfile::tempdir().unwrap();
        let git = Git::new(dir.path());
        git.run(&["init", "-q"]).unwrap();
        git.run(&["config", "user.name", "t"]).unwrap();
        git.run(&["config", "user.email", "t@t"]).unwrap();
        git.run(&["config", "commit.gpgsign", "false"]).unwrap();
        fs::write(dir.path().join("a.txt"), "one\n").unwrap();
        fs::write(dir.path().join(".gitignore"), "ignored/\n").unwrap();
        git.run(&["add", "."]).unwrap();
        git.run(&["commit", "-q", "-m", "init"]).unwrap();
        (dir, git)
    }

    fn read(dir: &tempfile::TempDir, p: &str) -> Option<String> {
        fs::read_to_string(dir.path().join(p)).ok()
    }

    #[test]
    fn create_captures_untracked_files_and_skips_duplicates() {
        let (dir, git) = repo();
        fs::write(dir.path().join("new.txt"), "fresh\n").unwrap();

        let cp = create(&git, "s1", 0, "início").unwrap().expect("first checkpoint");
        assert_eq!(cp.label, "início");
        let files = git.run(&["ls-tree", "-r", "--name-only", &cp.id]).unwrap();
        assert!(files.lines().any(|f| f == "new.txt"));

        assert!(create(&git, "s1", 1, "nada mudou").unwrap().is_none());
        assert_eq!(list(&git, "s1").unwrap().len(), 1);
        assert_eq!(sessions(&git).unwrap(), vec!["s1".to_owned()]);
    }

    #[test]
    fn create_leaves_index_branch_and_stash_alone() {
        let (dir, git) = repo();
        fs::write(dir.path().join("a.txt"), "two\n").unwrap();
        let head = git.run(&["rev-parse", "HEAD"]).unwrap();

        create(&git, "s1", 0, "x").unwrap();

        assert_eq!(git.run(&["rev-parse", "HEAD"]).unwrap(), head);
        assert_eq!(git.run(&["diff", "--cached", "--name-only"]).unwrap(), "");
        assert_eq!(git.run(&["stash", "list"]).unwrap(), "");
        assert!(
            !fs::read_dir(dir.path().join(".git")).unwrap().any(|e| e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("lazygit-index")),
            "temporary index must be cleaned up"
        );
    }

    #[test]
    fn restore_brings_back_old_state_and_can_be_undone() {
        let (dir, git) = repo();
        fs::create_dir(dir.path().join("ignored")).unwrap();
        fs::write(dir.path().join("ignored/cache.bin"), "keep me").unwrap();
        let start = create(&git, "s1", 0, "início").unwrap().unwrap();

        fs::write(dir.path().join("a.txt"), "changed by ai\n").unwrap();
        fs::create_dir_all(dir.path().join("src/deep")).unwrap();
        fs::write(dir.path().join("src/deep/b.txt"), "new file\n").unwrap();
        create(&git, "s1", 1, "ia editou").unwrap().unwrap();

        let safety = restore(&git, &start, 2).unwrap();
        assert_eq!(read(&dir, "a.txt").as_deref(), Some("one\n"));
        assert_eq!(read(&dir, "src/deep/b.txt"), None);
        assert!(!dir.path().join("src").exists(), "empty dirs are removed");
        assert_eq!(read(&dir, "ignored/cache.bin").as_deref(), Some("keep me"), "ignored files untouched");

        restore(&git, &safety, 3).unwrap();
        assert_eq!(read(&dir, "a.txt").as_deref(), Some("changed by ai\n"));
        assert_eq!(read(&dir, "src/deep/b.txt").as_deref(), Some("new file\n"));
    }

    #[test]
    fn works_in_repo_without_commits() {
        let dir = tempfile::tempdir().unwrap();
        let git = Git::new(dir.path());
        git.run(&["init", "-q"]).unwrap();
        fs::write(dir.path().join("x.txt"), "x\n").unwrap();
        let cp = create(&git, "s", 0, "x").unwrap().unwrap();
        assert_eq!(git.run(&["rev-list", "--count", &cp.id]).unwrap().trim(), "1");
    }

    #[test]
    fn delete_session_removes_refs() {
        let (dir, git) = repo();
        create(&git, "s1", 0, "a").unwrap();
        fs::write(dir.path().join("a.txt"), "z\n").unwrap();
        create(&git, "s1", 1, "b").unwrap();
        delete_session(&git, "s1").unwrap();
        assert!(sessions(&git).unwrap().is_empty());
    }
}
