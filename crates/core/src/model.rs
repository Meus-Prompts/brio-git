//! Plain data types shared by every crate. No I/O, no git access.

/// A commit as shown in the graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    /// Full hex object id.
    pub id: String,
    /// Full hex ids of the parents, first parent first.
    pub parents: Vec<String>,
    pub summary: String,
    pub author: String,
    pub email: String,
    /// Commit time in seconds since the Unix epoch.
    pub time: i64,
}

impl Commit {
    pub fn short_id(&self) -> &str {
        &self.id[..self.id.len().min(8)]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RefKind {
    LocalBranch,
    RemoteBranch,
    Tag,
}

/// A named reference pointing at a commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ref {
    /// Full name, e.g. `refs/heads/main`.
    pub full_name: String,
    /// Short display name, e.g. `main` or `origin/main`.
    pub name: String,
    pub kind: RefKind,
    /// Id of the commit the reference points to (annotated tags are peeled).
    pub target: String,
}

/// Where `HEAD` points.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Head {
    /// Short branch name, or `None` when detached.
    pub branch: Option<String>,
    /// Commit id, or `None` on an unborn branch (no commits yet).
    pub id: Option<String>,
}
