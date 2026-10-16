//! Undo/redo for local operations: a [`Snapshot`] of where HEAD, the local
//! branches and the tags point, taken before and after an operation, and
//! [`restore`] to bring the repository back to either one.

use std::collections::BTreeMap;

use crate::git::{Git, GitError, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeadState {
    /// Short branch name.
    Branch(String),
    Detached(String),
    Unborn,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub head: HeadState,
    /// Commit HEAD points at, if any.
    pub head_id: Option<String>,
    /// Full ref name (`refs/heads/x`, `refs/tags/y`) -> object id.
    pub refs: BTreeMap<String, String>,
}

/// How the checked-out branch is moved when its tip changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    /// `reset --soft`: the index keeps the content (undoing a commit puts its changes back in stage).
    Soft,
    /// `reset --keep`: refuses rather than overwrite local changes.
    Keep,
}

pub fn snapshot(git: &Git) -> Result<Snapshot> {
    let out = git.run(&["for-each-ref", "--format=%(refname)%00%(objectname)", "refs/heads", "refs/tags"])?;
    let refs = out
        .lines()
        .filter_map(|l| l.split_once('\0'))
        .map(|(name, id)| (name.to_owned(), id.to_owned()))
        .collect();
    let head_id = git.run(&["rev-parse", "-q", "--verify", "HEAD^{commit}"]).ok().map(|s| s.trim().to_owned());
    let head = match git.run(&["symbolic-ref", "-q", "HEAD"]) {
        Ok(name) => HeadState::Branch(name.trim().trim_start_matches("refs/heads/").to_owned()),
        Err(_) => match &head_id {
            Some(id) => HeadState::Detached(id.clone()),
            None => HeadState::Unborn,
        },
    };
    Ok(Snapshot { head, head_id, refs })
}

/// Brings the repository to `target`. The working tree is only touched by
/// `checkout` and `reset --soft/--keep`, which refuse to overwrite local changes.
pub fn restore(git: &Git, target: &Snapshot, mode: Move) -> Result<()> {
    let now = snapshot(git)?;
    let branch_ref = |b: &str| format!("refs/heads/{b}");

    // 1. HEAD on the right branch (or commit), with that branch's tip in place.
    match (&now.head, &target.head) {
        (HeadState::Branch(a), HeadState::Branch(b)) if a == b => {
            if now.head_id != target.head_id
                && let Some(id) = &target.head_id
            {
                let flag = if mode == Move::Soft { "--soft" } else { "--keep" };
                git.run(&["reset", flag, id])?;
            }
        }
        (_, HeadState::Branch(b)) => {
            if let Some(id) = target.refs.get(&branch_ref(b))
                && now.refs.get(&branch_ref(b)) != Some(id)
            {
                git.run(&["update-ref", &branch_ref(b), id])?;
            }
            git.run(&["checkout", "-q", b])?;
        }
        (_, HeadState::Detached(id)) => {
            if now.head != target.head {
                git.run(&["checkout", "-q", "--detach", id])?;
            }
        }
        (_, HeadState::Unborn) => {
            return Err(GitError::Failed {
                args: "undo".into(),
                stderr: "não dá para voltar a um repositório sem commits".into(),
            });
        }
    }

    // 2. Every other branch and tag, now that none of them is checked out.
    let current = match &target.head {
        HeadState::Branch(b) => Some(branch_ref(b)),
        _ => None,
    };
    let now = snapshot(git)?;
    for (name, id) in &target.refs {
        if Some(name) != current.as_ref() && now.refs.get(name) != Some(id) {
            git.run(&["update-ref", name, id])?;
        }
    }
    for name in now.refs.keys() {
        if !target.refs.contains_key(name) {
            git.run(&["update-ref", "-d", name])?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions;
    use std::fs;

    fn repo() -> (tempfile::TempDir, Git) {
        let dir = tempfile::tempdir().unwrap();
        let git = Git::new(dir.path());
        for args in [
            &["init", "-q", "-b", "main"][..],
            &["config", "user.name", "Ana"],
            &["config", "user.email", "ana@x.dev"],
            &["config", "commit.gpgsign", "false"],
            &["config", "tag.gpgsign", "false"],
        ] {
            git.run(args).unwrap();
        }
        fs::write(dir.path().join("a.txt"), "one\n").unwrap();
        git.run(&["add", "."]).unwrap();
        git.run(&["commit", "-qm", "first"]).unwrap();
        (dir, git)
    }

    fn commit(dir: &tempfile::TempDir, git: &Git, content: &str, message: &str) {
        fs::write(dir.path().join("a.txt"), content).unwrap();
        actions::stage(git, &["a.txt".into()]).unwrap();
        actions::commit(git, message, false).unwrap();
    }

    fn staged(git: &Git) -> String {
        git.run(&["diff", "--cached", "--name-only"]).unwrap().trim().to_owned()
    }

    #[test]
    fn undo_commit_keeps_changes_staged_and_redo_brings_it_back() {
        let (dir, git) = repo();
        let before = snapshot(&git).unwrap();
        commit(&dir, &git, "two\n", "second");
        let after = snapshot(&git).unwrap();

        restore(&git, &before, Move::Soft).unwrap();
        assert_eq!(snapshot(&git).unwrap(), before);
        assert_eq!(staged(&git), "a.txt", "the undone commit's changes are back in stage");

        restore(&git, &after, Move::Soft).unwrap();
        assert_eq!(snapshot(&git).unwrap(), after);
        assert_eq!(staged(&git), "");
    }

    #[test]
    fn undo_checkout_and_branch_creation() {
        let (_dir, git) = repo();
        let before = snapshot(&git).unwrap();
        actions::create_branch(&git, "feat", "HEAD", true).unwrap();
        let after = snapshot(&git).unwrap();
        assert_eq!(after.head, HeadState::Branch("feat".into()));

        restore(&git, &before, Move::Keep).unwrap();
        assert_eq!(snapshot(&git).unwrap(), before, "back on main and feat is gone");
        restore(&git, &after, Move::Keep).unwrap();
        assert_eq!(snapshot(&git).unwrap(), after);
    }

    #[test]
    fn undo_branch_and_tag_deletion() {
        let (_dir, git) = repo();
        git.run(&["branch", "old"]).unwrap();
        actions::create_tag(&git, "v1", "HEAD", Some("release")).unwrap();
        let before = snapshot(&git).unwrap();
        actions::delete_branch(&git, "old", true).unwrap();
        actions::delete_tag(&git, "v1").unwrap();

        restore(&git, &before, Move::Keep).unwrap();
        let now = snapshot(&git).unwrap();
        assert_eq!(now, before);
        assert_eq!(git.run(&["cat-file", "-t", "v1"]).unwrap().trim(), "tag", "annotated tag object restored");
    }

    #[test]
    fn undo_rebase_moves_the_branch_back() {
        let (dir, git) = repo();
        git.run(&["branch", "base"]).unwrap();
        commit(&dir, &git, "two\n", "on main");
        git.run(&["checkout", "-q", "base"]).unwrap();
        fs::write(dir.path().join("b.txt"), "b\n").unwrap();
        actions::stage(&git, &["b.txt".into()]).unwrap();
        actions::commit(&git, "on base", false).unwrap();

        let before = snapshot(&git).unwrap();
        actions::rebase(&git, "main").unwrap();
        assert_ne!(snapshot(&git).unwrap(), before);
        restore(&git, &before, Move::Keep).unwrap();
        assert_eq!(snapshot(&git).unwrap(), before);
        assert_eq!(fs::read_to_string(dir.path().join("a.txt")).unwrap(), "one\n", "main's commit is gone from base again");
        assert!(dir.path().join("b.txt").exists());
    }

    #[test]
    fn keep_refuses_to_overwrite_local_changes() {
        let (dir, git) = repo();
        let before = snapshot(&git).unwrap();
        commit(&dir, &git, "two\n", "second");
        fs::write(dir.path().join("a.txt"), "local edit\n").unwrap();
        assert!(restore(&git, &before, Move::Keep).is_err());
        assert_eq!(fs::read_to_string(dir.path().join("a.txt")).unwrap(), "local edit\n");
    }
}
