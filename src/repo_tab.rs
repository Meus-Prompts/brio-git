//! Estado e lógica de uma aba de repositório.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use git2::{Oid, Repository};
use notify::{RecursiveMode, Watcher};

use crate::git::conflict::{self, Segment};
use crate::git::diff::{self, BlameLine, DiffSource, FileDiff, HistoryEntry, PatchMode};
use crate::git::loader::{self, LoadOptions};
use crate::git::model::{CommitInfo, FileChange, OpState, Snapshot};
use crate::git::gitflow::{self, FlowKind};
use crate::git::ops::{self, PullMode, RebaseAction, RebaseStep, ResetMode};
use crate::jobs::{JobResult, Jobs};
use crate::settings::Settings;

#[derive(Clone, Debug, PartialEq)]
pub enum Selection {
    Wip,
    Commits(Vec<Oid>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffMode {
    Hunk,
    Inline,
    Split,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileTab {
    Diff,
    File,
    History,
    Blame,
}

pub struct FileView {
    pub source: DiffSource,
    pub path: String,
    pub diff: Result<FileDiff, String>,
    pub mode: DiffMode,
    pub tab: FileTab,
    /// Linhas selecionadas por hunk (para stage/unstage/discard de linhas).
    pub selected: HashMap<usize, HashSet<usize>>,
    pub content: Option<Result<String, String>>,
    pub blame: Option<Result<Vec<BlameLine>, String>>,
    pub history: Option<Result<Vec<HistoryEntry>, String>>,
}

pub struct MergeView {
    pub path: String,
    pub segments: Vec<Segment>,
    pub output: String,
    /// Usuário editou a saída manualmente; não recalcular a partir das escolhas.
    pub output_edited: bool,
    pub error: Option<String>,
}

pub struct RebasePlan {
    pub base: Oid,
    pub base_label: String,
    /// Do mais novo para o mais antigo (ordem de exibição, como no cliente de referência).
    pub steps: Vec<(CommitInfo, RebaseAction, String)>,
    /// Linha com foco para os atalhos de teclado (p/r/s/f/d, Alt+↑/↓).
    pub cursor: usize,
}

pub struct SearchState {
    pub query: String,
    pub matches: Vec<usize>,
    pub current: usize,
    pub focus: bool,
}

#[derive(Clone, Debug)]
pub enum Dialog {
    CreateBranch { at: Oid, name: String, checkout: bool },
    CreateTag { at: Oid, name: String, message: String, annotated: bool },
    RenameBranch { old: String, new: String },
    Confirm { title: String, body: String, confirm: String, danger: bool, action: Box<Action> },
    Stash { message: String, include_untracked: bool },
    PushUpstream { branch: String, remote: String, remote_branch: String },
    AddRemote { name: String, url: String },
    Reword { message: String },
    Error { title: String, message: String },
    GitflowInit(crate::git::gitflow::GitflowConfig),
    GitflowStart { kind: FlowKind, name: String },
    GitflowFinish { kind: FlowKind, name: String, tag_message: String, keep_branch: bool },
    WorktreeAdd { path: String, branch: String, new_branch: bool },
    SubmoduleAdd { url: String, path: String },
    LfsTrack { pattern: String },
    CreatePr { source: String, target: String, title: String, body: String, draft: bool, push_first: bool },
    ConnectHosting { token: crate::hosting::auth::Secret },
    CreateIssue { title: String, body: String },
}

#[derive(Clone, Debug)]
pub enum UndoEntry {
    Reset { to: Oid, mode: ResetMode, label: String },
    Checkout { to: String },
    CreateBranch { name: String, at: Oid },
    DeleteBranch { name: String },
}

impl UndoEntry {
    pub fn label(&self) -> String {
        match self {
            UndoEntry::Reset { label, .. } => label.clone(),
            UndoEntry::Checkout { to } => format!("checkout {to}"),
            UndoEntry::CreateBranch { name, .. } => format!("recriar branch {name}"),
            UndoEntry::DeleteBranch { name } => format!("remover branch {name}"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UndoStack {
    /// Operação normal: entra na pilha de undo e limpa o redo.
    New,
    FromUndo,
    FromRedo,
}

#[derive(Clone, Debug)]
pub enum Action {
    Refresh,
    SelectWip,
    SelectCommit { oid: Oid, add: bool },
    JumpTo(Oid),
    OpenDiff { source: DiffSource, path: String },
    OpenConflict(String),
    CloseCenter,
    Stage(Vec<String>),
    Unstage(Vec<String>),
    StageAll,
    UnstageAll,
    Discard(Vec<FileChange>),
    DiscardAll,
    ApplyPatch { patch: String, mode: PatchMode },
    Commit { stage_all: bool },
    /// Preenche summary/description com o CLI de IA local.
    GenerateMessage(crate::ai::Tool),
    Checkout(String),
    CheckoutCommit(Oid),
    CheckoutRemote(String),
    CreateBranch { name: String, at: Oid, checkout: bool },
    DeleteBranch { name: String, force: bool },
    DeleteRemoteBranch { remote: String, branch: String },
    RenameBranch { old: String, new: String },
    SetUpstream { branch: String, upstream: String },
    CreateTag { name: String, at: Oid, message: Option<String> },
    DeleteTag(String),
    PushTag(String),
    DeleteRemoteTag(String),
    Merge(String),
    FastForward { branch: String, to: Oid },
    /// Move um branch que não é o atual para `to` (`git branch -f`).
    ResetBranchTo { branch: String, to: Oid },
    Rebase(String),
    MergeInto { source: String, target: String },
    ShowDropMenu { source: DragRef, target: DragRef, pos: egui::Pos2 },
    RebaseBranch { branch: String, onto: String },
    CherryPick(Vec<Oid>),
    Revert(Oid),
    Reset { to: Oid, mode: ResetMode },
    StartInteractiveRebase(Oid),
    RunInteractiveRebase,
    CancelInteractiveRebase,
    RewordHead(String),
    Fetch,
    FetchRemote(String),
    Pull(PullMode),
    Push { force: bool },
    PushTo { remote: String, branch: String, remote_branch: String, set_upstream: bool, force: bool },
    StashSave { message: String, include_untracked: bool },
    StashApply(usize),
    StashPop(usize),
    StashDrop(usize),
    Abort,
    Continue,
    Skip,
    TakeSide { path: String, ours: bool },
    SaveMerge,
    Undo,
    Redo,
    OpenDialog(Dialog),
    ToggleHidden(String),
    Solo(String),
    ClearSolo,
    AddRemote { name: String, url: String },
    RemoveRemote(String),
    CopyText(String),
    ToggleTerminal,
    NewTerminal,
    CloseTerminal(usize),
    SelectTerminal(usize),
    OpenFolder,
    /// Tratada pelo app (abre outra aba).
    OpenRepoTab(PathBuf),
    Hosting(crate::repo_hosting::HostingAction),
    /// Abre o diálogo de PR/MR a partir de um branch local.
    StartPr(String),
    GitflowInit(crate::git::gitflow::GitflowConfig),
    GitflowStart { kind: FlowKind, name: String },
    GitflowFinish { kind: FlowKind, name: String, tag_message: String, keep_branch: bool },
    WorktreeAdd { path: String, branch: String, new_branch: bool },
    WorktreeRemove { path: String, force: bool },
    WorktreeLock { path: String, lock: bool },
    SubmoduleAdd { url: String, path: String },
    SubmoduleUpdate(Option<String>),
    LfsTrack(String),
    LfsUntrack(String),
    LfsPull,
    LfsInstall,
}

pub struct Notice {
    pub text: String,
    pub error: bool,
    pub at: Instant,
}

pub struct RepoTab {
    pub path: PathBuf,
    pub name: String,
    pub snap: Option<Arc<Snapshot>>,
    pub load_error: Option<String>,
    loader: Option<Receiver<anyhow::Result<Snapshot>>>,
    reload_again: bool,
    pub jobs: Jobs,
    pub selection: Selection,
    pub center: Option<CenterView>,
    pub commit_files: Option<(Selection, Result<Vec<FileChange>, String>)>,
    pub wip_summary: String,
    pub wip_body: String,
    pub amend: bool,
    pub push_after_commit: bool,
    pub skip_hooks: bool,
    /// Geração da mensagem de commit pela IA em andamento.
    pub ai_pending: Option<crate::ai::Pending>,
    pub dialog: Option<Dialog>,
    pub undo: Vec<UndoEntry>,
    pub redo: Vec<UndoEntry>,
    pub hidden: HashSet<String>,
    pub solo: HashSet<String>,
    pub left_filter: String,
    pub collapsed: HashSet<String>,
    pub search: Option<SearchState>,
    pub rebase: Option<RebasePlan>,
    pub notices: Vec<Notice>,
    pub scroll_to_row: Option<usize>,
    pub file_tree: bool,
    /// Abas de terminal do painel inferior.
    pub terminals: Vec<crate::ui::terminal::Terminal>,
    pub active_terminal: usize,
    next_terminal: u32,
    pub terminal_open: bool,
    pub focus_message: bool,
    /// Menu exibido ao soltar um ref arrastado sobre outro.
    pub drop_menu: Option<DropMenu>,
    /// Status de assinatura por commit (calculado sob demanda, `git log %G?`).
    pub signatures: HashMap<Oid, (ops::Signature, String)>,
    pub hosting: Option<crate::repo_hosting::HostingState>,
    _watcher: Option<notify::RecommendedWatcher>,
    fs_dirty: Arc<AtomicBool>,
    fs_dirty_since: Option<Instant>,
    /// Sem watcher (ex.: `\\wsl$` no Windows), o status é relido periodicamente.
    last_poll: Instant,
    last_fetch: Instant,
    ctx: egui::Context,
}

/// Ref sendo arrastado no grafo/painel esquerdo.
#[derive(Clone, Debug, PartialEq)]
pub struct DragRef {
    pub name: String,
    pub kind: crate::git::model::RefKind,
    pub oid: Oid,
}

pub struct DropMenu {
    pub source: DragRef,
    pub target: DragRef,
    pub pos: egui::Pos2,
}

pub enum CenterView {
    File(Box<FileView>),
    Merge(Box<MergeView>),
    PullRequest(Box<crate::repo_hosting::PrView>),
    Issue(Box<crate::repo_hosting::IssueView>),
}

impl RepoTab {
    pub fn open(path: PathBuf, ctx: &egui::Context) -> anyhow::Result<Self> {
        let repo = Repository::open(&path)?;
        let workdir = repo.workdir().map(Path::to_path_buf).unwrap_or_else(|| path.clone());
        let name = workdir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| workdir.display().to_string());

        let fs_dirty = Arc::new(AtomicBool::new(false));
        let watcher = {
            let flag = fs_dirty.clone();
            let ctx = ctx.clone();
            notify::recommended_watcher(move |ev: notify::Result<notify::Event>| {
                let Ok(ev) = ev else { return };
                let relevant = ev.paths.iter().any(|p| {
                    let s = p.to_string_lossy();
                    !(s.contains("/.git/objects/") || s.ends_with(".lock") || s.contains("/target/") || s.contains("/node_modules/"))
                });
                if relevant {
                    flag.store(true, Ordering::Relaxed);
                    ctx.request_repaint_after(Duration::from_millis(350));
                }
            })
            .ok()
            .and_then(|mut w| w.watch(&workdir, RecursiveMode::Recursive).ok().map(|_| w))
        };

        let mut tab = Self {
            path: workdir,
            name,
            snap: None,
            load_error: None,
            loader: None,
            reload_again: false,
            jobs: Jobs::new(ctx.clone()),
            selection: Selection::Wip,
            center: None,
            commit_files: None,
            wip_summary: String::new(),
            wip_body: String::new(),
            amend: false,
            push_after_commit: false,
            skip_hooks: false,
            ai_pending: None,
            dialog: None,
            undo: Vec::new(),
            redo: Vec::new(),
            hidden: HashSet::new(),
            solo: HashSet::new(),
            left_filter: String::new(),
            collapsed: ["TAGS", "SUBMODULES", "WORKTREES"].iter().map(|s| s.to_string()).collect(),
            search: None,
            rebase: None,
            notices: Vec::new(),
            scroll_to_row: None,
            file_tree: false,
            terminals: Vec::new(),
            active_terminal: 0,
            next_terminal: 1,
            terminal_open: false,
            focus_message: false,
            drop_menu: None,
            signatures: HashMap::new(),
            hosting: None,
            _watcher: watcher,
            fs_dirty,
            fs_dirty_since: None,
            last_poll: Instant::now(),
            last_fetch: Instant::now(),
            ctx: ctx.clone(),
        };
        tab.selection = Selection::Wip;
        Ok(tab)
    }

    pub fn ctx(&self) -> &egui::Context {
        &self.ctx
    }

    pub fn new_terminal(&mut self) {
        match crate::ui::terminal::Terminal::spawn(&self.path, &self.ctx, self.next_terminal) {
            Ok(t) => {
                self.next_terminal += 1;
                t.focus(&self.ctx);
                self.terminals.push(t);
                self.active_terminal = self.terminals.len() - 1;
            }
            Err(e) => self.notify(format!("Terminal: {e}"), true),
        }
    }

    /// Remove abas cujo shell terminou (`exit`, Ctrl+D).
    pub fn reap_terminals(&mut self) {
        let before = self.terminals.len();
        self.terminals.retain(|t| !t.exited());
        if self.terminals.len() != before {
            self.active_terminal = self.active_terminal.min(self.terminals.len().saturating_sub(1));
            if self.terminals.is_empty() {
                self.terminal_open = false;
            }
        }
    }

    #[cfg(test)]
    pub fn is_loading(&self) -> bool {
        self.loader.is_some()
    }

    pub fn repo(&self) -> anyhow::Result<Repository> {
        Ok(Repository::open(&self.path)?)
    }

    pub fn reload(&mut self, settings: &Settings) {
        if self.loader.is_some() {
            self.reload_again = true;
            return;
        }
        let (tx, rx) = channel();
        let path = self.path.clone();
        let opts = LoadOptions {
            max_commits: settings.max_commits,
            hidden: self.hidden.clone(),
            solo: self.solo.clone(),
            show_stashes: settings.show_stashes,
        };
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(loader::load(&path, &opts));
            ctx.request_repaint();
        });
        self.loader = Some(rx);
    }

    /// Chamado a cada frame: coleta resultados de loader/jobs e dispara
    /// refresh por watcher e autofetch.
    pub fn tick(&mut self, settings: &Settings) {
        if let Some(rx) = &self.loader
            && let Ok(res) = rx.try_recv()
        {
            self.loader = None;
            match res {
                Ok(snap) => {
                    self.load_error = None;
                    self.on_snapshot(snap);
                    self.detect_hosting(settings);
                }
                Err(e) => self.load_error = Some(format!("{e:#}")),
            }
            if self.reload_again {
                self.reload_again = false;
                self.reload(settings);
            }
        }

        self.hosting_tick();
        if let Some((tool, rx)) = &self.ai_pending
            && let Ok(res) = rx.try_recv()
        {
            let tool = *tool;
            self.ai_pending = None;
            match res {
                Ok((summary, body)) => {
                    self.wip_summary = summary;
                    self.wip_body = body;
                }
                Err(e) => self.dialog = Some(Dialog::Error { title: format!("{} falhou", tool.label()), message: format!("{e:#}") }),
            }
        }
        for done in self.jobs.poll() {
            self.on_job_done(done);
            self.reload(settings);
        }

        if self.fs_dirty.swap(false, Ordering::Relaxed) {
            self.fs_dirty_since.get_or_insert_with(Instant::now);
        }
        if let Some(since) = self.fs_dirty_since
            && since.elapsed() > Duration::from_millis(300)
            && !self.jobs.busy()
        {
            self.fs_dirty_since = None;
            self.reload(settings);
            self.refresh_center();
        }

        if self._watcher.is_none() && self.last_poll.elapsed() > Duration::from_secs(4) && !self.jobs.busy() && self.loader.is_none() {
            self.last_poll = Instant::now();
            self.reload(settings);
        }

        if settings.autofetch_minutes > 0
            && self.last_fetch.elapsed() > Duration::from_secs(settings.autofetch_minutes as u64 * 60)
            && !self.jobs.busy()
        {
            self.last_fetch = Instant::now();
            let has_remotes = self.snap.as_ref().is_some_and(|s| !s.remotes.is_empty());
            if has_remotes {
                let path = self.path.clone();
                self.jobs.spawn("Autofetch", None, move || ops::fetch_all(&path));
                self.hosting_action(crate::repo_hosting::HostingAction::Refresh);
            }
        }
        self.notices.retain(|n| n.at.elapsed() < Duration::from_secs(if n.error { 12 } else { 5 }));
    }

    fn on_snapshot(&mut self, snap: Snapshot) {
        // Seleção de commits que sumiram do grafo volta para o WIP.
        if let Selection::Commits(oids) = &self.selection
            && oids.iter().any(|o| !snap.index_of.contains_key(o))
        {
            self.selection = Selection::Wip;
        }
        if let Some(search) = &mut self.search {
            search.matches = search_matches(&snap, &search.query);
        }
        self.snap = Some(Arc::new(snap));
        self.commit_files = None;
        if self.snap.as_ref().is_some_and(|s| s.state == OpState::Clean) && self.rebase.is_none() {
            // Nada a fazer: mantido para clareza do fluxo.
        }
    }

    fn on_job_done(&mut self, done: JobResult) {
        match done.result {
            Ok(out) => {
                if done.label != "Autofetch" {
                    let first = out.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
                    let text = if first.is_empty() { format!("{} concluído", done.label) } else { format!("{}: {}", done.label, first) };
                    self.notify(text, false);
                }
                if let Some((entry, stack)) = done.undo {
                    match stack {
                        UndoStack::New => {
                            self.undo.push(entry);
                            self.redo.clear();
                        }
                        UndoStack::FromRedo => self.undo.push(entry),
                        UndoStack::FromUndo => self.redo.push(entry),
                    }
                }
            }
            Err(e) => {
                let msg = format!("{e:#}");
                if done.label == "Autofetch" {
                    self.notify("Autofetch falhou. Faça fetch manualmente para mais detalhes.".into(), true);
                } else {
                    self.dialog = Some(Dialog::Error { title: format!("{} falhou", done.label), message: msg });
                }
            }
        }
        self.refresh_center();
    }

    pub fn notify(&mut self, text: String, error: bool) {
        self.notices.push(Notice { text, error, at: Instant::now() });
    }

    /// Recarrega o diff aberto no centro (após stage/unstage etc.).
    fn refresh_center(&mut self) {
        let Ok(repo) = self.repo() else { return };
        match &mut self.center {
            Some(CenterView::File(fv)) => {
                if matches!(fv.source, DiffSource::Unstaged | DiffSource::Staged) {
                    fv.diff = diff::file_diff(&repo, fv.source, &fv.path).map_err(|e| e.to_string());
                    fv.selected.clear();
                    fv.content = None;
                    if fv.diff.is_err() {
                        // O arquivo saiu desta lista; tenta o outro lado.
                        let other = if fv.source == DiffSource::Unstaged { DiffSource::Staged } else { DiffSource::Unstaged };
                        if let Ok(d) = diff::file_diff(&repo, other, &fv.path) {
                            fv.source = other;
                            fv.diff = Ok(d);
                        }
                    }
                }
            }
            Some(CenterView::Merge(mv)) => {
                let still_conflicted = repo
                    .index()
                    .ok()
                    .is_some_and(|i| i.conflicts().map(|c| c.flatten().any(|c| conflict_path(&c) == mv.path)).unwrap_or(false));
                if !still_conflicted {
                    self.center = None;
                }
            }
            _ => {}
        }
    }

    pub fn head_oid(&self) -> Option<Oid> {
        self.snap.as_ref().and_then(|s| s.head.oid)
    }

    fn current_ref_name(&self) -> Option<String> {
        let s = self.snap.as_ref()?;
        s.head.branch.clone().filter(|_| !s.head.detached).or_else(|| s.head.oid.map(|o| o.to_string()))
    }

    pub fn commit(&self, oid: Oid) -> Option<&CommitInfo> {
        let s = self.snap.as_ref()?;
        s.index_of.get(&oid).map(|&i| &s.commits[i])
    }

    /// Arquivos alterados do commit/seleção atual (cacheado por seleção).
    pub fn ensure_commit_files(&mut self) {
        if self.commit_files.as_ref().is_some_and(|(s, _)| *s == self.selection) {
            return;
        }
        let res = match (&self.selection, self.repo()) {
            (Selection::Commits(oids), Ok(repo)) => {
                let source = self.selection_source(oids);
                diff::changed_files(&repo, source).map_err(|e| e.to_string())
            }
            (_, Err(e)) => Err(e.to_string()),
            _ => Ok(Vec::new()),
        };
        self.commit_files = Some((self.selection.clone(), res));
    }

    pub fn signature(&mut self, oid: Oid) -> (ops::Signature, String) {
        let path = self.path.clone();
        self.signatures.entry(oid).or_insert_with(|| ops::signature_status(&path, &oid.to_string())).clone()
    }

    /// Para 2+ commits selecionados, compara o mais antigo com o mais novo.
    pub fn selection_source(&self, oids: &[Oid]) -> DiffSource {
        if oids.len() >= 2 {
            let mut sorted: Vec<(usize, Oid)> = oids
                .iter()
                .filter_map(|o| self.snap.as_ref().and_then(|s| s.index_of.get(o)).map(|&i| (i, *o)))
                .collect();
            sorted.sort();
            if let (Some(newest), Some(oldest)) = (sorted.first(), sorted.last()) {
                return DiffSource::Range(oldest.1, newest.1);
            }
        }
        DiffSource::Commit(oids[0])
    }

    fn spawn(&mut self, label: &str, undo: Option<UndoEntry>, f: impl FnOnce(&Path) -> anyhow::Result<String> + Send + 'static) {
        let path = self.path.clone();
        self.jobs.spawn(label, undo.map(|u| (u, UndoStack::New)), move || f(&path));
    }

    fn spawn_undo(&mut self, label: &str, inverse: Option<UndoEntry>, stack: UndoStack, f: impl FnOnce(&Path) -> anyhow::Result<String> + Send + 'static) {
        let path = self.path.clone();
        self.jobs.spawn(label, inverse.map(|u| (u, stack)), move || f(&path));
    }

    fn has_head(&self) -> bool {
        self.snap.as_ref().is_some_and(|s| !s.head.unborn)
    }

    pub fn handle(&mut self, action: Action, settings: &Settings) {
        match action {
            Action::Refresh => {
                self.reload(settings);
                self.refresh_center();
            }
            Action::SelectWip => {
                self.selection = Selection::Wip;
                self.center = None;
            }
            Action::SelectCommit { oid, add } => {
                match (&mut self.selection, add) {
                    (Selection::Commits(list), true) => {
                        if let Some(i) = list.iter().position(|o| *o == oid) {
                            if list.len() > 1 {
                                list.remove(i);
                            }
                        } else {
                            list.push(oid);
                        }
                    }
                    _ => self.selection = Selection::Commits(vec![oid]),
                }
                if matches!(self.center, Some(CenterView::File(ref f)) if !matches!(f.source, DiffSource::Commit(_) | DiffSource::Range(..))) {
                    self.center = None;
                }
                if let Some(CenterView::File(fv)) = &self.center
                    && let Selection::Commits(oids) = &self.selection
                {
                    let source = self.selection_source(oids);
                    let path = fv.path.clone();
                    self.open_file(source, path);
                }
            }
            Action::JumpTo(oid) => {
                if let Some(&row) = self.snap.as_ref().and_then(|s| s.index_of.get(&oid)) {
                    self.scroll_to_row = Some(row);
                    self.selection = Selection::Commits(vec![oid]);
                } else {
                    self.notify("Não é possível ir até o commit no grafo: ele não está visível.".into(), true);
                }
            }
            Action::OpenDiff { source, path } => self.open_file(source, path),
            Action::OpenConflict(path) => self.open_conflict(path),
            Action::CloseCenter => self.center = None,
            Action::Stage(paths) => self.spawn("Stage", None, move |p| ops::stage(p, &paths)),
            Action::Unstage(paths) => {
                let has_head = self.has_head();
                self.spawn("Unstage", None, move |p| ops::unstage(p, &paths, has_head))
            }
            Action::StageAll => self.spawn("Stage all", None, ops::stage_all),
            Action::UnstageAll => {
                let has_head = self.has_head();
                self.spawn("Unstage all", None, move |p| ops::unstage_all(p, has_head))
            }
            Action::Discard(files) => {
                let (untracked, tracked): (Vec<_>, Vec<_>) =
                    files.into_iter().partition(|f| f.kind == crate::git::model::ChangeKind::Untracked);
                let tracked: Vec<String> = tracked.into_iter().map(|f| f.path).collect();
                let untracked: Vec<String> = untracked.into_iter().map(|f| f.path).collect();
                self.spawn("Discard", None, move |p| ops::discard(p, &tracked, &untracked));
            }
            Action::DiscardAll => self.spawn("Discard all", None, ops::discard_all),
            Action::ApplyPatch { patch, mode } => {
                let label = match mode {
                    PatchMode::Stage => "Stage",
                    PatchMode::Unstage => "Unstage",
                    PatchMode::Discard => "Discard",
                };
                self.spawn(label, None, move |p| ops::apply_patch(p, &patch, mode));
            }
            Action::Commit { stage_all } => self.do_commit(stage_all, settings.profile().map(|p| (p.name.clone(), p.email.clone()))),
            Action::GenerateMessage(tool) => {
                let (tx, rx) = std::sync::mpsc::channel();
                let path = self.path.clone();
                let ctx = self.ctx.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(crate::ai::generate(&path, tool));
                    ctx.request_repaint();
                });
                self.ai_pending = Some((tool, rx));
            }
            Action::Checkout(name) => {
                let undo = self.current_ref_name().map(|to| UndoEntry::Checkout { to });
                self.spawn(&format!("Checkout {name}"), undo, move |p| ops::checkout(p, &name));
            }
            Action::CheckoutCommit(oid) => {
                let undo = self.current_ref_name().map(|to| UndoEntry::Checkout { to });
                self.spawn("Checkout", undo, move |p| ops::checkout(p, &oid.to_string()));
            }
            Action::CheckoutRemote(name) => {
                let undo = self.current_ref_name().map(|to| UndoEntry::Checkout { to });
                self.spawn(&format!("Checkout {name}"), undo, move |p| ops::checkout_remote(p, &name));
            }
            Action::CreateBranch { name, at, checkout } => {
                let undo = (!checkout).then(|| UndoEntry::DeleteBranch { name: name.clone() });
                self.spawn(&format!("Branch {name}"), undo, move |p| ops::create_branch(p, &name, &at.to_string(), checkout));
            }
            Action::DeleteBranch { name, force } => {
                let at = self.snap.as_ref().and_then(|s| s.locals.iter().find(|b| b.name == name).map(|b| b.oid));
                let undo = at.map(|at| UndoEntry::CreateBranch { name: name.clone(), at });
                self.spawn(&format!("Delete {name}"), undo, move |p| ops::delete_branch(p, &name, force));
            }
            Action::DeleteRemoteBranch { remote, branch } => {
                self.spawn(&format!("Delete {remote}/{branch}"), None, move |p| ops::delete_remote_branch(p, &remote, &branch))
            }
            Action::RenameBranch { old, new } => self.spawn("Rename", None, move |p| ops::rename_branch(p, &old, &new)),
            Action::SetUpstream { branch, upstream } => {
                self.spawn("Set upstream", None, move |p| ops::set_upstream(p, &branch, &upstream))
            }
            Action::CreateTag { name, at, message } => {
                self.spawn(&format!("Tag {name}"), None, move |p| ops::create_tag(p, &name, &at.to_string(), message.as_deref()))
            }
            Action::DeleteTag(name) => self.spawn(&format!("Delete tag {name}"), None, move |p| ops::delete_tag(p, &name)),
            Action::PushTag(name) => {
                let remote = self.default_remote();
                self.spawn(&format!("Push tag {name}"), None, move |p| ops::push_tag(p, &remote, &name))
            }
            Action::DeleteRemoteTag(name) => {
                let remote = self.default_remote();
                self.spawn(&format!("Delete remote tag {name}"), None, move |p| ops::delete_remote_tag(p, &remote, &name))
            }
            Action::Merge(what) => {
                let undo = self.head_oid().map(|to| UndoEntry::Reset { to, mode: ResetMode::Hard, label: format!("merge {what}") });
                self.spawn(&format!("Merge {what}"), undo, move |p| ops::merge(p, &what, false))
            }
            Action::FastForward { branch, to } => {
                let is_head = self.snap.as_ref().and_then(|s| s.head.branch.clone()) == Some(branch.clone());
                if is_head {
                    self.spawn(&format!("Fast-forward {branch}"), None, move |p| ops::merge(p, &to.to_string(), true));
                } else {
                    self.spawn(&format!("Fast-forward {branch}"), None, move |p| ops::move_branch(p, &branch, &to.to_string()));
                }
            }
            Action::ResetBranchTo { branch, to } => {
                self.spawn(&format!("Reset {branch}"), None, move |p| ops::move_branch(p, &branch, &to.to_string()));
            }
            Action::ShowDropMenu { source, target, pos } => self.drop_menu = Some(DropMenu { source, target, pos }),
            Action::MergeInto { source, target } => {
                let undo = self.current_ref_name().map(|to| UndoEntry::Checkout { to });
                let current = self.snap.as_ref().and_then(|s| s.head.branch.clone());
                self.spawn(&format!("Merge {source} into {target}"), undo, move |p| ops::merge_into(p, &source, &target, current.as_deref()))
            }
            Action::RebaseBranch { branch, onto } => {
                let undo = self.current_ref_name().map(|to| UndoEntry::Checkout { to });
                self.spawn(&format!("Rebase {branch} onto {onto}"), undo, move |p| ops::rebase_branch(p, &branch, &onto))
            }
            Action::Rebase(onto) => {
                let undo = self.head_oid().map(|to| UndoEntry::Reset { to, mode: ResetMode::Hard, label: format!("rebase {onto}") });
                self.spawn(&format!("Rebase onto {onto}"), undo, move |p| ops::rebase(p, &onto))
            }
            Action::CherryPick(oids) => {
                let undo = self.head_oid().map(|to| UndoEntry::Reset { to, mode: ResetMode::Hard, label: "cherry-pick".into() });
                let shas: Vec<String> = oids.iter().rev().map(|o| o.to_string()).collect();
                self.spawn("Cherry pick", undo, move |p| ops::cherry_pick(p, &shas))
            }
            Action::Revert(oid) => {
                let undo = self.head_oid().map(|to| UndoEntry::Reset { to, mode: ResetMode::Hard, label: "revert".into() });
                self.spawn("Revert", undo, move |p| ops::revert(p, &oid.to_string()))
            }
            Action::Reset { to, mode } => {
                let undo = self.head_oid().map(|prev| UndoEntry::Reset { to: prev, mode, label: "reset".into() });
                self.spawn("Reset", undo, move |p| ops::reset(p, &to.to_string(), mode))
            }
            Action::StartInteractiveRebase(base) => self.start_rebase(base),
            Action::RunInteractiveRebase => self.run_rebase(),
            Action::CancelInteractiveRebase => self.rebase = None,
            Action::RewordHead(message) => {
                self.spawn("Reword", None, move |p| {
                    ops::commit(p, &ops::CommitRequest { message, amend: true, skip_hooks: true, identity: None })
                });
            }
            Action::Fetch => {
                self.last_fetch = Instant::now();
                self.hosting_action(crate::repo_hosting::HostingAction::Refresh);
                self.spawn("Fetch", None, ops::fetch_all)
            }
            Action::FetchRemote(r) => self.spawn(&format!("Fetch {r}"), None, move |p| ops::fetch_remote(p, &r)),
            Action::Pull(mode) => {
                let undo = self.head_oid().map(|to| UndoEntry::Reset { to, mode: ResetMode::Hard, label: "pull".into() });
                self.spawn("Pull", undo, move |p| ops::pull(p, mode))
            }
            Action::Push { force } => self.do_push(force),
            Action::PushTo { remote, branch, remote_branch, set_upstream, force } => {
                let refspec = if branch == remote_branch { branch.clone() } else { format!("{branch}:{remote_branch}") };
                self.spawn("Push", None, move |p| ops::push(p, &remote, &refspec, set_upstream, force))
            }
            Action::StashSave { message, include_untracked } => {
                self.spawn("Stash", None, move |p| ops::stash_save(p, &message, include_untracked))
            }
            Action::StashApply(i) => self.spawn("Apply stash", None, move |p| ops::stash_apply(p, i)),
            Action::StashPop(i) => self.spawn("Pop stash", None, move |p| ops::stash_pop(p, i)),
            Action::StashDrop(i) => self.spawn("Delete stash", None, move |p| ops::stash_drop(p, i)),
            Action::Abort => {
                let state = self.op_state();
                self.center = None;
                self.spawn(&format!("Abort {}", state.label()), None, move |p| ops::abort(p, state))
            }
            Action::Continue => {
                let state = self.op_state();
                self.spawn(&format!("Continue {}", state.label()), None, move |p| ops::continue_op(p, state))
            }
            Action::Skip => self.spawn("Skip", None, ops::skip_op),
            Action::TakeSide { path, ours } => {
                self.center = None;
                self.spawn("Resolve", None, move |p| ops::take_side(p, &path, ours))
            }
            Action::SaveMerge => self.save_merge(),
            Action::Undo => self.undo_redo(true),
            Action::Redo => self.undo_redo(false),
            Action::OpenDialog(d) => self.dialog = Some(d),
            Action::ToggleHidden(name) => {
                if !self.hidden.remove(&name) {
                    self.hidden.insert(name);
                }
                self.reload(settings);
            }
            Action::Solo(name) => {
                if !self.solo.remove(&name) {
                    self.solo.insert(name);
                }
                self.reload(settings);
            }
            Action::ClearSolo => {
                self.solo.clear();
                self.reload(settings);
            }
            Action::AddRemote { name, url } => self.spawn(&format!("Add remote {name}"), None, move |p| {
                ops::add_remote(p, &name, &url)?;
                ops::fetch_remote(p, &name)
            }),
            Action::RemoveRemote(name) => self.spawn(&format!("Remove remote {name}"), None, move |p| ops::remove_remote(p, &name)),
            Action::CopyText(text) => {
                self.ctx.copy_text(text);
                self.notify("Copiado para a área de transferência".into(), false);
            }
            Action::ToggleTerminal => {
                self.terminal_open = !self.terminal_open;
                if self.terminal_open && self.terminals.is_empty() {
                    self.new_terminal();
                }
            }
            Action::NewTerminal => {
                self.terminal_open = true;
                self.new_terminal();
            }
            Action::CloseTerminal(i) => {
                if i < self.terminals.len() {
                    self.terminals.remove(i);
                }
                self.active_terminal = self.active_terminal.min(self.terminals.len().saturating_sub(1));
                if self.terminals.is_empty() {
                    self.terminal_open = false;
                }
            }
            Action::SelectTerminal(i) => {
                if let Some(t) = self.terminals.get(i) {
                    self.active_terminal = i;
                    t.focus(&self.ctx);
                }
            }
            Action::OpenRepoTab(_) => {}
            Action::Hosting(a) => self.hosting_action(a),
            Action::StartPr(branch) => {
                if self.hosting.is_some() {
                    self.dialog = Some(crate::ui::hosting_view::create_pr_dialog(self, Some(&branch)));
                }
            }
            Action::GitflowInit(cfg) => self.spawn("Initialize Gitflow", None, move |p| gitflow::init(p, &cfg)),
            Action::GitflowStart { kind, name } => {
                let Some(cfg) = self.snap.as_ref().and_then(|s| s.gitflow.clone()) else { return };
                let undo = self.current_ref_name().map(|to| UndoEntry::Checkout { to });
                self.spawn(&format!("Start {}", kind.label()), undo, move |p| gitflow::start(p, &cfg, kind, &name));
            }
            Action::GitflowFinish { kind, name, tag_message, keep_branch } => {
                let Some(cfg) = self.snap.as_ref().and_then(|s| s.gitflow.clone()) else { return };
                self.spawn(&format!("Finish {}", kind.label()), None, move |p| {
                    gitflow::finish(p, &cfg, &gitflow::Finish { kind, name: &name, tag_message: &tag_message, keep_branch })
                });
            }
            Action::WorktreeAdd { path, branch, new_branch } => {
                let base = self.current_ref_name().unwrap_or_else(|| "HEAD".into());
                self.spawn("Add worktree", None, move |p| ops::worktree_add(p, &path, &branch, new_branch, &base));
            }
            Action::WorktreeRemove { path, force } => self.spawn("Remove worktree", None, move |p| ops::worktree_remove(p, &path, force)),
            Action::WorktreeLock { path, lock } => {
                self.spawn(if lock { "Lock worktree" } else { "Unlock worktree" }, None, move |p| ops::worktree_lock(p, &path, lock))
            }
            Action::SubmoduleAdd { url, path } => self.spawn("Add submodule", None, move |p| ops::submodule_add(p, &url, &path)),
            Action::SubmoduleUpdate(path) => self.spawn("Update submodules", None, move |p| ops::submodule_update(p, path.as_deref())),
            Action::LfsTrack(pat) => self.spawn("LFS track", None, move |p| ops::lfs_track(p, &pat)),
            Action::LfsUntrack(pat) => self.spawn("LFS untrack", None, move |p| ops::lfs_untrack(p, &pat)),
            Action::LfsPull => self.spawn("LFS pull", None, ops::lfs_pull),
            Action::LfsInstall => self.spawn("LFS install", None, ops::lfs_install),
            Action::OpenFolder => {
                crate::ui::open_path(self.path.as_os_str());
            }
        }
    }

    pub fn op_state(&self) -> OpState {
        self.snap.as_ref().map(|s| s.state).unwrap_or(OpState::Clean)
    }

    pub fn default_remote(&self) -> String {
        let Some(s) = &self.snap else { return "origin".into() };
        if let Some(up) = s.current_branch().and_then(|b| b.upstream.as_ref())
            && let Some((r, _)) = up.split_once('/')
        {
            return r.to_string();
        }
        s.remotes
            .iter()
            .find(|r| r.name == "origin")
            .or(s.remotes.first())
            .map(|r| r.name.clone())
            .unwrap_or_else(|| "origin".into())
    }

    fn do_commit(&mut self, stage_all: bool, identity: Option<(String, String)>) {
        let summary = self.wip_summary.trim().to_string();
        if summary.is_empty() {
            self.notify("Digite uma mensagem de commit".into(), true);
            return;
        }
        let message = if self.wip_body.trim().is_empty() { summary } else { format!("{summary}\n\n{}", self.wip_body.trim()) };
        let amend = self.amend;
        let skip_hooks = self.skip_hooks;
        let push = self.push_after_commit;
        let remote = self.default_remote();
        let branch = self.snap.as_ref().and_then(|s| s.head.branch.clone());
        let upstream_missing = self.snap.as_ref().and_then(|s| s.current_branch()).is_some_and(|b| b.upstream.is_none());
        let undo = self.head_oid().map(|to| UndoEntry::Reset { to, mode: ResetMode::Soft, label: "commit".into() });
        self.spawn(if amend { "Amend" } else { "Commit" }, undo, move |p| {
            if stage_all {
                ops::stage_all(p)?;
            }
            let out = ops::commit(p, &ops::CommitRequest { message, amend, skip_hooks, identity })?;
            if push && let Some(b) = branch {
                ops::push(p, &remote, &b, upstream_missing, amend)?;
            }
            Ok(out)
        });
        self.wip_summary.clear();
        self.wip_body.clear();
        self.amend = false;
    }

    fn do_push(&mut self, force: bool) {
        let Some(s) = &self.snap else { return };
        let Some(b) = s.current_branch() else {
            self.notify("Push não está disponível com HEAD destacado (detached)".into(), true);
            return;
        };
        match &b.upstream {
            Some(up) => {
                let (remote, rb) = up.split_once('/').map(|(r, b)| (r.to_string(), b.to_string())).unwrap_or(("origin".into(), b.name.clone()));
                let local = b.name.clone();
                let refspec = if local == rb { local } else { format!("{local}:{rb}") };
                self.spawn(if force { "Force push" } else { "Push" }, None, move |p| ops::push(p, &remote, &refspec, false, force));
            }
            None => {
                let branch = b.name.clone();
                self.dialog = Some(Dialog::PushUpstream { remote: self.default_remote(), remote_branch: branch.clone(), branch });
            }
        }
    }

    fn undo_redo(&mut self, is_undo: bool) {
        let entry = if is_undo { self.undo.pop() } else { self.redo.pop() };
        let Some(entry) = entry else {
            self.notify(if is_undo { "Nada para desfazer" } else { "Nada para refazer" }.into(), false);
            return;
        };
        let stack = if is_undo { UndoStack::FromUndo } else { UndoStack::FromRedo };
        let label = format!("{} {}", if is_undo { "Undo" } else { "Redo" }, entry.label());
        match entry {
            UndoEntry::Reset { to, mode, label: l } => {
                let inverse = self.head_oid().map(|h| UndoEntry::Reset { to: h, mode, label: l });
                self.spawn_undo(&label, inverse, stack, move |p| ops::reset(p, &to.to_string(), mode));
            }
            UndoEntry::Checkout { to } => {
                let inverse = self.current_ref_name().map(|t| UndoEntry::Checkout { to: t });
                self.spawn_undo(&label, inverse, stack, move |p| ops::checkout(p, &to));
            }
            UndoEntry::CreateBranch { name, at } => {
                let inverse = Some(UndoEntry::DeleteBranch { name: name.clone() });
                self.spawn_undo(&label, inverse, stack, move |p| ops::create_branch(p, &name, &at.to_string(), false));
            }
            UndoEntry::DeleteBranch { name } => {
                let at = self.snap.as_ref().and_then(|s| s.locals.iter().find(|b| b.name == name).map(|b| b.oid));
                let inverse = at.map(|at| UndoEntry::CreateBranch { name: name.clone(), at });
                self.spawn_undo(&label, inverse, stack, move |p| ops::delete_branch(p, &name, true));
            }
        }
    }

    pub fn open_file(&mut self, source: DiffSource, path: String) {
        let diff = self.repo().and_then(|r| diff::file_diff(&r, source, &path)).map_err(|e| e.to_string());
        let (mode, tab) = match &self.center {
            Some(CenterView::File(fv)) => (fv.mode, if fv.tab == FileTab::Diff || fv.path == path { fv.tab } else { FileTab::Diff }),
            _ => (DiffMode::Hunk, FileTab::Diff),
        };
        self.center = Some(CenterView::File(Box::new(FileView {
            source,
            path,
            diff,
            mode,
            tab,
            selected: HashMap::new(),
            content: None,
            blame: None,
            history: None,
        })));
    }

    fn open_conflict(&mut self, path: String) {
        let text = std::fs::read_to_string(self.path.join(&path));
        let (segments, error) = match text {
            Ok(t) => (conflict::parse(&t), None),
            Err(e) => (Vec::new(), Some(e.to_string())),
        };
        let output = conflict::render(&segments);
        self.center = Some(CenterView::Merge(Box::new(MergeView { path, segments, output, output_edited: false, error })));
    }

    fn save_merge(&mut self) {
        let Some(CenterView::Merge(mv)) = &self.center else { return };
        let path = mv.path.clone();
        let output = mv.output.clone();
        let full = self.path.join(&path);
        self.center = None;
        self.spawn("Resolve", None, move |p| {
            std::fs::write(&full, output)?;
            ops::stage(p, &[path])
        });
    }

    fn start_rebase(&mut self, base: Oid) {
        let Some(s) = self.snap.clone() else { return };
        let Some(head) = s.head.oid else { return };
        let Ok(repo) = self.repo() else { return };
        let mut walk = match repo.revwalk() {
            Ok(w) => w,
            Err(e) => return self.notify(e.to_string(), true),
        };
        let _ = walk.push(head);
        let _ = walk.hide(base);
        let _ = walk.set_sorting(git2::Sort::TOPOLOGICAL);
        let mut steps = Vec::new();
        for oid in walk.flatten() {
            let Ok(c) = repo.find_commit(oid) else { continue };
            if c.parent_count() > 1 {
                self.notify("Rebase interativo de merges não é suportado; use o terminal.".into(), true);
                return;
            }
            let info = loader::commit_info(&c);
            let msg = String::from_utf8_lossy(c.message_bytes()).to_string();
            steps.push((info, RebaseAction::Pick, msg));
        }
        if steps.is_empty() {
            self.notify("Nenhum commit para rebasear".into(), false);
            return;
        }
        let base_label = self.commit(base).map(|c| c.short.clone()).unwrap_or_else(|| base.to_string()[..7].to_string());
        self.rebase = Some(RebasePlan { base, base_label, steps, cursor: 0 });
    }

    fn run_rebase(&mut self) {
        let Some(plan) = self.rebase.take() else { return };
        let steps: Vec<RebaseStep> = plan
            .steps
            .iter()
            .rev()
            .map(|(c, a, m)| RebaseStep { sha: c.id.to_string(), action: *a, message: m.clone() })
            .collect();
        let base = plan.base.to_string();
        let undo = self.head_oid().map(|to| UndoEntry::Reset { to, mode: ResetMode::Hard, label: "rebase interativo".into() });
        self.spawn("Interactive rebase", undo, move |p| ops::interactive_rebase(p, &base, &steps));
    }
}

fn conflict_path(c: &git2::IndexConflict) -> String {
    c.our
        .as_ref()
        .or(c.their.as_ref())
        .or(c.ancestor.as_ref())
        .map(|e| String::from_utf8_lossy(&e.path).to_string())
        .unwrap_or_default()
}

pub fn search_matches(snap: &Snapshot, query: &str) -> Vec<usize> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    snap.commits
        .iter()
        .enumerate()
        .filter(|(_, c)| {
            c.summary.to_lowercase().contains(&q)
                || c.body.to_lowercase().contains(&q)
                || c.author.to_lowercase().contains(&q)
                || c.id.to_string().starts_with(&q)
        })
        .map(|(i, _)| i)
        .collect()
}
