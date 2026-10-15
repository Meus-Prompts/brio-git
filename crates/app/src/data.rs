//! Everything the UI shows about a repository, loaded in one background job.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use lazygit_core::graph::{self, GraphRow};
use lazygit_core::{Commit, Head, Ref, Repo};
use lazygit_ops::{FileChange, Git};

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
}

impl RepoData {
    pub fn load(path: &Path) -> Result<Self, String> {
        let repo = Repo::open(path).map_err(|e| e.to_string())?;
        let head = repo.head().map_err(|e| e.to_string())?;
        let refs = repo.refs().map_err(|e| e.to_string())?;
        let commits = repo.log(&refs, &head, LOG_LIMIT).map_err(|e| e.to_string())?;
        let rows = graph::layout(&commits);

        let mut labels: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, r) in refs.iter().enumerate() {
            labels.entry(r.target.clone()).or_default().push(i);
        }

        let git = Git::new(repo.workdir());
        let changes = lazygit_ops::status::status(&git).map_err(|e| e.to_string())?;

        Ok(Self {
            workdir: repo.workdir().to_path_buf(),
            git_dir: repo.git_dir().to_path_buf(),
            head,
            refs,
            commits,
            rows,
            labels,
            changes,
        })
    }

    pub fn git(&self) -> Git {
        Git::new(&self.workdir)
    }

    pub fn commit_index(&self, id: &str) -> Option<usize> {
        self.commits.iter().position(|c| c.id == id)
    }
}
