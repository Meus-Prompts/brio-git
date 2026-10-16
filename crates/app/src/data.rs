//! Everything the UI shows about a repository, loaded in one background job.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use brio_core::graph::{self, GraphRow};
use brio_core::{Commit, Head, Ref, RefKind, Repo};
use brio_ops::actions::{self, InProgress, Stash, Upstream};
use brio_ops::{FileChange, Git};

/// How many commits the graph shows.
pub const LOG_LIMIT: usize = 2000;

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
    /// A merge/rebase/cherry-pick/revert stopped by conflicts.
    pub in_progress: Option<InProgress>,
    pub remotes: Vec<String>,
}

impl RepoData {
    /// Loads everything; `pinned` names a local branch kept in the first lane.
    pub fn load(path: &Path, pinned: Option<&str>) -> Result<Self, String> {
        let repo = Repo::open(path).map_err(|e| e.to_string())?;
        let head = repo.head().map_err(|e| e.to_string())?;
        let refs = repo.refs().map_err(|e| e.to_string())?;
        let commits = repo.log(&refs, &head, LOG_LIMIT).map_err(|e| e.to_string())?;
        let pinned_tip = pinned.and_then(|name| {
            refs.iter().find(|r| r.kind == RefKind::LocalBranch && r.name == name).map(|r| r.target.as_str())
        });
        let rows = graph::layout_pinned(&commits, pinned_tip);

        let mut labels: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, r) in refs.iter().enumerate() {
            labels.entry(r.target.clone()).or_default().push(i);
        }

        let git = Git::new(repo.workdir());
        let changes = brio_ops::status::status(&git).map_err(|e| e.to_string())?;
        // Secondary data: a failure here should not hide the repository.
        let upstream = actions::upstream(&git).ok().flatten();
        let stashes = actions::stash_list(&git).unwrap_or_default();
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
            in_progress,
            remotes,
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
