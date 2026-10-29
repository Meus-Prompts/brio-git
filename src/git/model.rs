//! Tipos de dados imutáveis produzidos pelo loader e consumidos pela UI.

use std::collections::HashMap;

use git2::Oid;

use super::graph::GraphLayout;

#[derive(Clone, Debug)]
pub struct CommitInfo {
    pub id: Oid,
    pub short: String,
    pub summary: String,
    pub body: String,
    pub author: String,
    pub email: String,
    pub time: i64,
    pub committer: String,
    pub parents: Vec<Oid>,
    /// Índice do stash quando a linha representa um stash (`stash@{n}`).
    pub stash_index: Option<usize>,
}

impl CommitInfo {
    pub fn initials(&self) -> String {
        let mut out = String::new();
        for word in self.author.split_whitespace().take(2) {
            if let Some(c) = word.chars().next() {
                out.extend(c.to_uppercase());
            }
        }
        if out.is_empty() {
            out.push('?');
        }
        out
    }

    pub fn is_merge(&self) -> bool {
        self.parents.len() > 1 && self.stash_index.is_none()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RefKind {
    Local,
    Remote,
    Tag,
}

#[derive(Clone, Debug)]
pub struct RefLabel {
    /// Nome curto exibido (`main`, `origin/main`, `v1.0`).
    pub name: String,
    pub kind: RefKind,
    pub is_head: bool,
}

#[derive(Clone, Debug)]
pub struct BranchInfo {
    pub name: String,
    pub oid: Oid,
    pub is_head: bool,
    pub upstream: Option<String>,
    pub ahead: usize,
    pub behind: usize,
}

#[derive(Clone, Debug)]
pub struct RemoteInfo {
    pub name: String,
    pub url: String,
    pub branches: Vec<BranchInfo>,
}

#[derive(Clone, Debug)]
pub struct TagInfo {
    pub name: String,
    pub oid: Oid,
}

#[derive(Clone, Debug)]
pub struct StashInfo {
    pub index: usize,
    pub message: String,
    pub oid: Oid,
}

#[derive(Clone, Debug)]
pub struct SubmoduleInfo {
    pub name: String,
    pub path: String,
    pub head: Option<Oid>,
}

#[derive(Clone, Debug)]
pub struct WorktreeInfo {
    pub name: String,
    pub path: String,
    pub locked: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    TypeChange,
    Untracked,
    Conflicted,
}

impl ChangeKind {
    pub fn letter(self) -> &'static str {
        match self {
            ChangeKind::Added | ChangeKind::Untracked => "+",
            ChangeKind::Modified | ChangeKind::TypeChange => "✏",
            ChangeKind::Deleted => "−",
            ChangeKind::Renamed => "→",
            ChangeKind::Conflicted => "!",
        }
    }
}

#[derive(Clone, Debug)]
pub struct FileChange {
    pub path: String,
    pub old_path: Option<String>,
    pub kind: ChangeKind,
    pub additions: usize,
    pub deletions: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpState {
    Clean,
    Merge,
    Rebase,
    CherryPick,
    Revert,
    Bisect,
    Other,
}

impl OpState {
    pub fn label(self) -> &'static str {
        match self {
            OpState::Clean => "",
            OpState::Merge => "merge",
            OpState::Rebase => "rebase",
            OpState::CherryPick => "cherry-pick",
            OpState::Revert => "revert",
            OpState::Bisect => "bisect",
            OpState::Other => "operação",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct HeadInfo {
    pub branch: Option<String>,
    pub oid: Option<Oid>,
    pub detached: bool,
    pub unborn: bool,
}

#[derive(Clone, Debug, Default)]
pub struct WorkStatus {
    pub staged: Vec<FileChange>,
    pub unstaged: Vec<FileChange>,
    pub conflicted: Vec<FileChange>,
}

impl WorkStatus {
    pub fn is_empty(&self) -> bool {
        self.staged.is_empty() && self.unstaged.is_empty() && self.conflicted.is_empty()
    }

    pub fn total(&self) -> usize {
        self.staged.len() + self.unstaged.len() + self.conflicted.len()
    }
}

/// Tudo o que a UI precisa para desenhar um repositório. Recriado inteiro
/// pelo loader em background a cada refresh.
pub struct Snapshot {
    pub commits: Vec<CommitInfo>,
    pub graph: GraphLayout,
    pub index_of: HashMap<Oid, usize>,
    pub refs_by_commit: HashMap<Oid, Vec<RefLabel>>,
    pub locals: Vec<BranchInfo>,
    pub remotes: Vec<RemoteInfo>,
    pub tags: Vec<TagInfo>,
    pub stashes: Vec<StashInfo>,
    pub submodules: Vec<SubmoduleInfo>,
    pub worktrees: Vec<WorktreeInfo>,
    pub head: HeadInfo,
    pub state: OpState,
    pub status: WorkStatus,
    pub truncated: bool,
    pub load_ms: u128,
    pub gitflow: Option<super::gitflow::GitflowConfig>,
    /// Padrões do Git LFS; `None` quando o `git lfs` não está instalado.
    pub lfs: Option<Vec<String>>,
}

impl Snapshot {
    pub fn current_branch(&self) -> Option<&BranchInfo> {
        self.locals.iter().find(|b| b.is_head)
    }
}
