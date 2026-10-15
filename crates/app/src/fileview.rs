//! The file viewer: one file compared between two versions, its blame or its
//! history. Loading runs off the UI thread; views live in `view_file.rs`.

use std::sync::Arc;

use lazygit_core::textdiff::FileDiff;
use lazygit_ops::files::{self, BlameLine, CommitInfo, Content, HistoryEntry};
use lazygit_ops::{ChangeKind, FileChange, Git};

use crate::highlight::{self, Line};
use crate::style;

/// Which two versions of a file to compare.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileTarget {
    pub path: String,
    /// Path on the old side when the file was renamed.
    pub old_path: Option<String>,
    /// Old revision; `None` means the file did not exist.
    pub old_rev: Option<String>,
    /// New revision; `None` means the working tree.
    pub new_rev: Option<String>,
    pub kind: ChangeKind,
}

impl FileTarget {
    pub fn new(change: &FileChange, old_rev: Option<&str>, new_rev: Option<&str>) -> Self {
        let new_file = matches!(change.kind, ChangeKind::Added | ChangeKind::Untracked);
        Self {
            path: change.path.clone(),
            old_path: change.from.clone(),
            old_rev: if new_file { None } else { old_rev.map(str::to_owned) },
            new_rev: new_rev.map(str::to_owned),
            kind: change.kind,
        }
    }

    pub fn is_worktree(&self) -> bool {
        self.new_rev.is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Diff,
    Blame,
    History,
}

#[derive(Debug)]
pub struct DiffDoc {
    pub diff: FileDiff,
    pub old_hl: Vec<Line>,
    pub new_hl: Vec<Line>,
    /// Why the content is not shown, for binary or huge files.
    pub unavailable: Option<&'static str>,
}

#[derive(Debug)]
pub struct BlameDoc {
    pub lines: Vec<BlameLine>,
    pub hl: Vec<Line>,
}

#[derive(Debug, Clone)]
pub enum Loaded {
    Diff(Result<Arc<DiffDoc>, String>),
    Blame(Result<Arc<BlameDoc>, String>),
    History(Result<Arc<Vec<HistoryEntry>>, String>),
}

pub struct FileView {
    pub target: FileTarget,
    pub mode: Mode,
    pub diff: Option<Result<Arc<DiffDoc>, String>>,
    pub blame: Option<Result<Arc<BlameDoc>, String>>,
    pub history: Option<Result<Arc<Vec<HistoryEntry>>, String>>,
}

impl FileView {
    pub fn new(target: FileTarget) -> Self {
        Self { target, mode: Mode::Diff, diff: None, blame: None, history: None }
    }

    /// Forgets loaded content (the file changed or the theme did).
    pub fn invalidate(&mut self) {
        self.diff = None;
        self.blame = None;
        self.history = None;
    }

    pub fn needs_load(&self) -> bool {
        match self.mode {
            Mode::Diff => self.diff.is_none(),
            Mode::Blame => self.blame.is_none(),
            Mode::History => self.history.is_none(),
        }
    }

    pub fn accept(&mut self, loaded: Loaded) {
        match loaded {
            Loaded::Diff(r) => self.diff = Some(r),
            Loaded::Blame(r) => self.blame = Some(r),
            Loaded::History(r) => self.history = Some(r),
        }
    }

    /// The background job for the current mode.
    pub fn job(&self, git: Git) -> impl FnOnce() -> Loaded + Send + 'static {
        let target = self.target.clone();
        let mode = self.mode;
        let theme = highlight::code_theme(style::p().name);
        move || match mode {
            Mode::Diff => Loaded::Diff(load_diff(&git, &target, theme).map(Arc::new)),
            Mode::Blame => Loaded::Blame(load_blame(&git, &target, theme).map(Arc::new)),
            Mode::History => Loaded::History(
                files::history(&git, &target.path, 300).map(Arc::new).map_err(|e| e.to_string()),
            ),
        }
    }
}

fn load_diff(git: &Git, t: &FileTarget, theme: two_face::theme::EmbeddedThemeName) -> Result<DiffDoc, String> {
    let old_path = t.old_path.as_deref().unwrap_or(&t.path);
    let old = match &t.old_rev {
        Some(rev) => files::content(git, Some(rev), old_path),
        None => Content::Text(String::new()),
    };
    let new = if t.kind == ChangeKind::Deleted {
        Content::Text(String::new())
    } else {
        files::content(git, t.new_rev.as_deref(), &t.path)
    };
    let unavailable = [&old, &new].iter().find_map(|c| match c {
        Content::Binary => Some("Arquivo binário: não há diff de texto."),
        Content::TooLarge => Some("Arquivo grande demais para exibir."),
        Content::Text(_) => None,
    });
    if unavailable.is_some() {
        return Ok(DiffDoc { diff: FileDiff::default(), old_hl: Vec::new(), new_hl: Vec::new(), unavailable });
    }
    let (old, new) = (old.text(), new.text());
    Ok(DiffDoc {
        diff: FileDiff::compute(old, new),
        old_hl: highlight::highlight(old, old_path, theme),
        new_hl: highlight::highlight(new, &t.path, theme),
        unavailable: None,
    })
}

fn load_blame(git: &Git, t: &FileTarget, theme: two_face::theme::EmbeddedThemeName) -> Result<BlameDoc, String> {
    if t.kind == ChangeKind::Deleted {
        return Err("O arquivo foi removido; não há blame.".into());
    }
    if t.kind == ChangeKind::Untracked {
        return Err("Arquivo ainda não versionado; não há blame.".into());
    }
    let lines = files::blame(git, t.new_rev.as_deref(), &t.path).map_err(|e| e.to_string())?;
    let text: String = lines.iter().map(|l| format!("{}\n", l.text)).collect();
    let hl = highlight::highlight(&text, &t.path, theme);
    Ok(BlameDoc { lines, hl })
}

/// What the details panel shows for a commit or checkpoint.
#[derive(Debug)]
pub struct Details {
    pub info: Option<CommitInfo>,
    pub files: Vec<FileChange>,
    pub old_rev: Option<String>,
    pub new_rev: String,
}

/// Details of commit `id`, compared with its first parent.
pub fn commit_details(git: &Git, id: &str) -> Result<Details, String> {
    let info = files::commit_info(git, id).map_err(|e| e.to_string())?;
    let old_rev = info.parents.first().cloned();
    let files = files::changed(git, old_rev.as_deref(), id).map_err(|e| e.to_string())?;
    Ok(Details { info: Some(info), files, old_rev, new_rev: id.to_owned() })
}

/// Files a checkpoint changed since `prev` (or since the HEAD it was taken on).
pub fn checkpoint_details(git: &Git, id: &str, prev: Option<String>) -> Result<Details, String> {
    let old_rev = match prev {
        Some(p) => Some(p),
        None => files::commit_info(git, id).map_err(|e| e.to_string())?.parents.first().cloned(),
    };
    let files = files::changed(git, old_rev.as_deref(), id).map_err(|e| e.to_string())?;
    Ok(Details { info: None, files, old_rev, new_rev: id.to_owned() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_files_have_no_old_side() {
        let added = FileChange { path: "a".into(), from: None, kind: ChangeKind::Untracked, staged: false };
        let t = FileTarget::new(&added, Some("HEAD"), None);
        assert_eq!(t.old_rev, None);
        assert!(t.is_worktree());

        let renamed = FileChange { path: "b".into(), from: Some("a".into()), kind: ChangeKind::Renamed, staged: false };
        let t = FileTarget::new(&renamed, Some("p"), Some("c"));
        assert_eq!((t.old_rev.as_deref(), t.old_path.as_deref()), (Some("p"), Some("a")));
    }

    #[test]
    fn loads_diff_blame_and_details_from_a_repo() {
        let dir = tempfile::tempdir().unwrap();
        let git = Git::new(dir.path());
        for a in [&["init", "-q"][..], &["config", "user.name", "Ana"], &["config", "user.email", "a@x"], &["config", "commit.gpgsign", "false"]] {
            git.run(a).unwrap();
        }
        std::fs::write(dir.path().join("m.rs"), "fn a() {}\n").unwrap();
        git.run(&["add", "."]).unwrap();
        git.run(&["commit", "-qm", "one"]).unwrap();
        std::fs::write(dir.path().join("m.rs"), "fn a() {}\nfn b() {}\n").unwrap();
        git.run(&["commit", "-qam", "two"]).unwrap();

        let details = commit_details(&git, "HEAD").unwrap();
        assert_eq!(details.files.len(), 1);
        let target = FileTarget::new(&details.files[0], details.old_rev.as_deref(), Some(&details.new_rev));

        let mut view = FileView::new(target);
        let Loaded::Diff(Ok(doc)) = view.job(git.clone())() else { panic!("diff failed") };
        assert_eq!((doc.diff.added, doc.diff.removed), (1, 0));
        assert_eq!(doc.new_hl.len(), 2);

        view.mode = Mode::Blame;
        let Loaded::Blame(Ok(blame)) = view.job(git.clone())() else { panic!("blame failed") };
        assert_eq!(blame.lines.iter().map(|l| l.summary.as_str()).collect::<Vec<_>>(), ["one", "two"]);

        view.mode = Mode::History;
        let Loaded::History(Ok(h)) = view.job(git)() else { panic!("history failed") };
        assert_eq!(h.len(), 2);
    }
}
