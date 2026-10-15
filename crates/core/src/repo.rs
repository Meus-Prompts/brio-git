//! Read-only repository access through gitoxide.

use std::path::{Path, PathBuf};

use gix::revision::walk::Sorting;
use gix::traverse::commit::simple::CommitTimeOrder;

use crate::model::{Commit, Head, Ref, RefKind};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("não é um repositório git: {0}")]
    NotARepository(String),
    #[error("repositório sem working tree (bare)")]
    Bare,
    #[error("erro do git: {0}")]
    Git(String),
}

pub type Result<T> = std::result::Result<T, Error>;

fn git_err(e: impl std::fmt::Display) -> Error {
    Error::Git(e.to_string())
}

/// Namespace used by lazy-git for its own refs (checkpoints). They are hidden
/// from the branch list and from the graph.
pub const INTERNAL_REF_PREFIX: &str = "refs/lazygit/";

pub struct Repo {
    inner: gix::Repository,
    workdir: PathBuf,
}

impl Repo {
    /// Opens the repository containing `path` (searching parent directories).
    pub fn open(path: &Path) -> Result<Self> {
        let inner = gix::discover(path).map_err(|e| Error::NotARepository(e.to_string()))?;
        let workdir = inner.workdir().ok_or(Error::Bare)?.to_path_buf();
        Ok(Self { inner, workdir })
    }

    pub fn workdir(&self) -> &Path {
        &self.workdir
    }

    /// The `.git` directory (may live outside the work tree for worktrees).
    pub fn git_dir(&self) -> &Path {
        self.inner.git_dir()
    }

    pub fn head(&self) -> Result<Head> {
        let branch = self
            .inner
            .head_name()
            .map_err(git_err)?
            .map(|n| n.shorten().to_string());
        let id = self.inner.head_id().ok().map(|id| id.to_string());
        Ok(Head { branch, id })
    }

    /// Local branches, remote branches and tags, sorted by kind then name.
    pub fn refs(&self) -> Result<Vec<Ref>> {
        let platform = self.inner.references().map_err(git_err)?;
        let mut refs = Vec::new();
        for reference in platform.all().map_err(git_err)? {
            let Ok(mut reference) = reference else { continue };
            let full_name = reference.name().as_bstr().to_string();
            let kind = if full_name.starts_with("refs/heads/") {
                RefKind::LocalBranch
            } else if full_name.starts_with("refs/remotes/") {
                if full_name.ends_with("/HEAD") {
                    continue;
                }
                RefKind::RemoteBranch
            } else if full_name.starts_with("refs/tags/") {
                RefKind::Tag
            } else {
                continue;
            };
            let Ok(target) = reference.peel_to_id() else { continue };
            refs.push(Ref {
                name: reference.name().shorten().to_string(),
                full_name,
                kind,
                target: target.to_string(),
            });
        }
        refs.sort_by(|a, b| (a.kind, &a.name).cmp(&(b.kind, &b.name)));
        Ok(refs)
    }

    /// Commits reachable from `HEAD` and all `refs`, newest first.
    pub fn log(&self, refs: &[Ref], head: &Head, limit: usize) -> Result<Vec<Commit>> {
        let mut tips: Vec<gix::ObjectId> = Vec::new();
        for id in head.id.iter().chain(refs.iter().map(|r| &r.target)) {
            let Ok(oid) = gix::ObjectId::from_hex(id.as_bytes()) else { continue };
            if !tips.contains(&oid) {
                tips.push(oid);
            }
        }
        if tips.is_empty() {
            return Ok(Vec::new());
        }

        let walk = self
            .inner
            .rev_walk(tips)
            .sorting(Sorting::ByCommitTime(CommitTimeOrder::NewestFirst))
            .all()
            .map_err(git_err)?;

        let mut commits = Vec::new();
        for info in walk.take(limit) {
            let info = info.map_err(git_err)?;
            // Tags may point at trees or blobs; skip anything that is not a commit.
            let Ok(commit) = info.object() else { continue };
            let summary = commit
                .message()
                .map(|m| m.summary().to_string())
                .unwrap_or_default();
            let (author, email) = commit
                .author()
                .map(|a| (a.name.to_string(), a.email.to_string()))
                .unwrap_or_default();
            let time = commit.time().map(|t| t.seconds).unwrap_or_default();
            commits.push(Commit {
                id: info.id.to_string(),
                parents: info.parent_ids().map(|p| p.to_string()).collect(),
                summary,
                author,
                email,
                time,
            });
        }
        Ok(commits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        git(p, &["init", "-q", "-b", "main"]);
        git(p, &["commit", "-q", "--allow-empty", "-m", "first"]);
        git(p, &["checkout", "-q", "-b", "feature"]);
        git(p, &["commit", "-q", "--allow-empty", "-m", "on feature"]);
        git(p, &["checkout", "-q", "main"]);
        git(p, &["merge", "-q", "--no-ff", "feature", "-m", "merge feature"]);
        git(p, &["tag", "-a", "v1", "-m", "v1"]);
        git(p, &["update-ref", "refs/lazygit/checkpoints/s/0001", "HEAD"]);
        dir
    }

    #[test]
    fn reads_head_refs_and_log() {
        let dir = fixture();
        let repo = Repo::open(dir.path()).unwrap();

        let head = repo.head().unwrap();
        assert_eq!(head.branch.as_deref(), Some("main"));

        let refs = repo.refs().unwrap();
        let names: Vec<_> = refs.iter().map(|r| (r.kind, r.name.as_str())).collect();
        assert_eq!(
            names,
            vec![(RefKind::LocalBranch, "feature"), (RefKind::LocalBranch, "main"), (RefKind::Tag, "v1")]
        );
        // The annotated tag is peeled to the merge commit.
        assert_eq!(refs[2].target, head.id.clone().unwrap());

        let log = repo.log(&refs, &head, 100).unwrap();
        let summaries: Vec<_> = log.iter().map(|c| c.summary.as_str()).collect();
        assert_eq!(summaries.len(), 3);
        assert_eq!(summaries[0], "merge feature");
        assert_eq!(log[0].parents.len(), 2);
    }

    #[test]
    fn opening_a_subdirectory_finds_the_repo() {
        let dir = fixture();
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        let repo = Repo::open(&sub).unwrap();
        assert_eq!(repo.workdir().canonicalize().unwrap(), dir.path().canonicalize().unwrap());
    }

    #[test]
    fn empty_repo_has_no_commits() {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q"]);
        let repo = Repo::open(dir.path()).unwrap();
        let head = repo.head().unwrap();
        assert!(head.id.is_none());
        assert!(repo.log(&[], &head, 10).unwrap().is_empty());
    }
}
