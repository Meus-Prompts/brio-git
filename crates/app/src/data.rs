//! Everything the UI shows about a repository, loaded in one background job.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use brio_core::graph::{self, GraphRow};
use brio_core::{Commit, Head, Ref, RefKind, Repo};
use brio_ops::actions::{self, InProgress, ReflogEntry, Stash, Upstream};
use brio_ops::{FileChange, Git};

/// How many commits the graph shows.
pub const LOG_LIMIT: usize = 2000;
/// How many HEAD movements the reflog panel shows.
pub const REFLOG_LIMIT: usize = 300;

#[derive(Debug)]
pub struct RepoData {
    pub workdir: PathBuf,
    pub git_dir: PathBuf,
    pub head: Head,
    pub refs: Vec<Ref>,
    pub commits: Vec<Commit>,
    pub rows: Vec<GraphRow>,
    /// Commit id -> indices into `refs` pointing at it.
    pub labels: HashMap<String, Vec<usize>>,
    pub changes: Vec<FileChange>,
    /// Upstream of the current branch, with ahead/behind counts.
    pub upstream: Option<Upstream>,
    pub stashes: Vec<Stash>,
    /// Recent movements of HEAD, newest first.
    pub reflog: Vec<ReflogEntry>,
    /// A merge/rebase/cherry-pick/revert stopped by conflicts.
    pub in_progress: Option<InProgress>,
    pub remotes: Vec<String>,
    /// Index into `refs` of the branch pinned to the first lane.
    pub pinned: Option<usize>,
}

impl RepoData {
    /// Loads everything; `pinned` names a branch kept in the first lane: a
    /// local one, or a remote one (`origin/main`) when no local has that name.
    pub fn load(path: &Path, pinned: Option<&str>) -> Result<Self, String> {
        let repo = Repo::open(path).map_err(|e| e.to_string())?;
        let head = repo.head().map_err(|e| e.to_string())?;
        let refs = repo.refs().map_err(|e| e.to_string())?;
        let commits = repo.log(&refs, &head, LOG_LIMIT).map_err(|e| e.to_string())?;
        let pinned = pinned.and_then(|name| {
            let find = |kind| refs.iter().position(|r| r.kind == kind && r.name == name);
            find(RefKind::LocalBranch).or_else(|| find(RefKind::RemoteBranch))
        });
        let rows = graph::layout_pinned(&commits, pinned.map(|i| refs[i].target.as_str()));

        let mut labels: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, r) in refs.iter().enumerate() {
            labels.entry(r.target.clone()).or_default().push(i);
        }

        let git = Git::new(repo.workdir());
        let changes = brio_ops::status::status(&git).map_err(|e| e.to_string())?;
        // Secondary data: a failure here should not hide the repository.
        let upstream = actions::upstream(&git).ok().flatten();
        let stashes = actions::stash_list(&git).unwrap_or_default();
        let reflog = actions::reflog(&git, REFLOG_LIMIT).unwrap_or_default();
        let remotes = actions::remotes(&git).unwrap_or_default();
        let in_progress = actions::in_progress(repo.git_dir());

        Ok(Self {
            workdir: repo.workdir().to_path_buf(),
            git_dir: repo.git_dir().to_path_buf(),
            head,
            refs,
            commits,
            rows,
            labels,
            changes,
            upstream,
            stashes,
            reflog,
            in_progress,
            remotes,
            pinned,
        })
    }

    pub fn git(&self) -> Git {
        Git::new(&self.workdir)
    }

    /// Refs pointing at commit `id`.
    pub fn refs_at(&self, id: &str) -> impl Iterator<Item = &Ref> {
        self.labels.get(id).into_iter().flatten().map(|&i| &self.refs[i])
    }

    pub fn current_branch(&self) -> Option<&str> {
        self.head.branch.as_deref()
    }

    /// Default remote for branch/tag deletions on the server.
    pub fn main_remote(&self) -> Option<&str> {
        self.remotes.iter().find(|r| *r == "origin").or(self.remotes.first()).map(String::as_str)
    }

    pub fn commit_index(&self, id: &str) -> Option<usize> {
        self.commits.iter().position(|c| c.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pins_a_local_branch_or_else_a_remote_one() {
        let dir = tempfile::tempdir().unwrap();
        let git = Git::new(dir.path());
        for args in [
            &["init", "-q", "-b", "main"][..],
            &["config", "user.name", "Ana"],
            &["config", "user.email", "ana@x.dev"],
            &["config", "commit.gpgsign", "false"],
            &["commit", "-q", "--allow-empty", "-m", "first"],
            // A remote branch without a local one, and a local branch also on the remote.
            &["update-ref", "refs/remotes/origin/feat", "HEAD"],
            &["update-ref", "refs/remotes/origin/main", "HEAD"],
        ] {
            git.run(args).unwrap();
        }
        let pinned = |name: &str| {
            let data = RepoData::load(dir.path(), Some(name)).unwrap();
            data.pinned.map(|i| (data.refs[i].kind, data.refs[i].name.clone()))
        };
        assert_eq!(pinned("main"), Some((RefKind::LocalBranch, "main".into())));
        assert_eq!(pinned("origin/feat"), Some((RefKind::RemoteBranch, "origin/feat".into())));
        assert_eq!(pinned("origin/main"), Some((RefKind::RemoteBranch, "origin/main".into())));
        assert_eq!(pinned("nope"), None);
    }
}
