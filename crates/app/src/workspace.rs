//! One open repository (a tab): its data, live watcher and AI sessions.
//! Views live in `view.rs`.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use iced::keyboard::{Key, Modifiers};
use iced::widget::canvas::Cache;
use iced::widget::operation;
use iced::widget::scrollable::AbsoluteOffset;
use iced::Task;
use iced::widget::text_editor;
use brio_core::RefKind;
use brio_ops::actions::ResetMode;
use brio_ops::undo::{self, Snapshot};
use brio_session::{TermEvent, Terminal, Tool, WatchHandle};

use crate::actions::{CheckoutTarget, Op};
use crate::branch_picker::{self, BranchPicker, Choice};
use crate::data::RepoData;
use crate::menu::Target;
use crate::history::{self, History};
use crate::fileview::{self, Details, FileTarget, FileView, Loaded, Mode};
use crate::session::{self, Activity, Session, SessionId};
use crate::util::{self, blocking};
use crate::widgets::graph;
use crate::keys;

pub const GRAPH_SCROLL: &str = "graph";

/// What the right-hand panel shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    /// Live changes in the working tree.
    Worktree,
    Commit(String),
    /// Two revisions side by side; `to: None` is the working tree.
    Compare { from: String, to: Option<String> },
}

/// A modal asking for a name or a confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogKind {
    CreateBranch { at: String },
    CreateTag { at: String },
    RenameBranch { old: String },
    Confirm(Op),
    /// Checkout blocked by local changes: stash them or cancel.
    DirtyCheckout(CheckoutTarget),
}

pub struct Dialog {
    pub kind: DialogKind,
    pub input: String,
    /// Tag message.
    pub input2: String,
    /// "Checkout after creating".
    pub check: bool,
    pub error: Option<String>,
}

pub const DIALOG_INPUT: &str = "dialog-input";
pub const COMMIT_SUMMARY: &str = "commit-summary";
pub const COMMIT_BODY: &str = "commit-body";
pub const SESSION_NAME: &str = "session-name";

pub struct Workspace {
    /// Folder the tab was opened with (the work tree once loaded).
    pub path: PathBuf,
    /// Collapsed sections of the branch list.
    pub collapsed: HashSet<RefKind>,
    pub repo: Option<Arc<RepoData>>,
    pub loading: bool,
    reload_pending: bool,
    watch: Option<WatchHandle>,
    pub graph_cache: Cache,

    pub selection: Selection,
    /// Commit shown in the details panel.
    pub details: Option<Result<Arc<Details>, String>>,
    details_token: u64,
    /// File open in the viewer (replaces the graph while open).
    pub file_view: Option<FileView>,
    file_token: u64,

    // Viewer preferences.
    pub split: bool,
    pub changes_only: bool,
    pub tree_view: bool,
    pub collapsed_dirs: HashSet<String>,
    pub collapsed_stash: bool,

    /// Local branch kept in the first lane of the graph.
    pub pinned: Option<String>,
    pub dialog: Option<Dialog>,
    /// Branch switcher opened from the toolbar.
    pub branch_picker: Option<BranchPicker>,
    /// Label of the git operation running in the background.
    pub busy: Option<String>,
    /// Local operations that can be undone and redone.
    pub history: History,

    // Commit box.
    pub commit_summary: String,
    pub commit_body: text_editor::Content,
    pub amend: bool,
    pub generating: bool,

    pub tools: Vec<Tool>,
    pub tool: Option<String>,
    /// AI sessions, in tab order; the panel shows `active_session`.
    pub sessions: Vec<Session>,
    pub active_session: Option<SessionId>,
    next_session: SessionId,
    /// Session whose name is being edited, with the text typed so far.
    pub renaming: Option<(SessionId, String)>,
    /// Session waiting for a second click on ✕ because it is still running.
    pub confirm_close_session: Option<SessionId>,
    /// Canvas cache of the empty terminal shown before any session starts.
    pub idle_cache: Cache,
    pub term_focused: bool,
    pub term_size: (u16, u16),

    pub error: Option<String>,
    pub notice: Option<String>,
}

#[derive(Debug, Clone)]
pub enum WsMessage {
    ToggleSection(RefKind),
    Refresh,
    Loaded(Result<Arc<RepoData>, String>),
    FsChanged,

    ShowWorktree,
    SelectCommit(String),
    JumpToCommit(String),
    /// A reflog entry: jumps to it in the graph, or just shows it when unreachable.
    SelectReflog(String),
    CopyHash(String),
    DetailsLoaded(u64, Result<Arc<Details>, String>),

    OpenFile(FileTarget),
    CloseFile,
    SetFileMode(Mode),
    FileLoaded(u64, Loaded),
    ToggleSplit,
    ToggleChangesOnly,
    SetTreeView(bool),
    ToggleDir(String),
    ToggleStashes,
    ThemeChanged,

    /// Right-click; handled by the app, which owns the menu overlay.
    OpenMenu(Target),
    Request(Op),
    /// The refs before and after are there when the operation can be undone.
    OpDone(Op, Result<Option<String>, String>, Option<Box<(Snapshot, Snapshot)>>),
    Undo,
    Redo,
    /// Ctrl+Z / Ctrl+Shift+Z, once it is known whether a commit field had the focus.
    HistoryShortcut { redo: bool, typing: bool },
    HistoryDone { redo: bool, result: Result<(), String> },
    Checkout(CheckoutTarget),
    CheckoutCommit(String),
    CopyText(String),
    CopyCommitMessage(String),
    CommitMessageCopied(Result<String, String>),
    Compare { from: String, to: Option<String> },
    TogglePin(String),

    OpenDialog(DialogKind),
    DialogInput(String),
    DialogInput2(String),
    DialogCheck(bool),
    DialogConfirm,
    DialogCancel,

    CommitSummary(String),
    CommitBody(text_editor::Action),
    ToggleAmend(bool),
    /// The app answers this with [`WsMessage::GenerateWith`] and the configured prompt.
    GenerateMessage,
    GenerateWith(String),
    MessageGenerated(Result<String, String>),
    DoCommit,
    StageAll,
    UnstageAll,

    ToolSelected(String),
    /// Starts a session with the selected tool.
    NewSession,
    SelectSession(SessionId),
    StopSession(SessionId),
    CloseSession(SessionId),
    /// Double click on a session tab: edit its name.
    RenameSession(SessionId),
    SessionNameInput(String),
    SessionNameSubmit,
    Term(SessionId, TermEvent),
    TermResize(u16, u16),
    TermScroll(i32),
    /// Jumps the terminal view to this many lines back into history.
    TermScrollTo(usize),
    TermFocus(bool),
    Key(Key, Modifiers, Option<String>),
    Paste(Option<String>),

    OpenBranchPicker,
    CloseBranchPicker,
    /// Escape pressed inside a text field: closes the branch picker or dialog.
    CloseOverlays,
    BranchQuery(String),
    /// Picks the highlighted entry (Enter in the filter).
    BranchSubmit,
    PickBranch(Choice),

}

impl Workspace {
    pub fn new(path: PathBuf, tools: Vec<Tool>, pinned: Option<String>) -> (Self, Task<WsMessage>) {
        let tool = tools.first().map(|t| t.name.clone());
        let mut ws = Self {
            path: path.clone(),
            collapsed: HashSet::new(),
            repo: None,
            loading: false,
            reload_pending: false,
            watch: None,
            graph_cache: Cache::new(),
            selection: Selection::Worktree,
            details: None,
            details_token: 0,
            file_view: None,
            file_token: 0,
            split: true,
            changes_only: true,
            tree_view: true,
            collapsed_dirs: HashSet::new(),
            collapsed_stash: false,
            pinned,
            dialog: None,
            branch_picker: None,
            history: History::default(),
            busy: None,
            commit_summary: String::new(),
            commit_body: text_editor::Content::new(),
            amend: false,
            generating: false,
            tools,
            tool,
            sessions: Vec::new(),
            active_session: None,
            next_session: 0,
            renaming: None,
            confirm_close_session: None,
            idle_cache: Cache::new(),
            term_focused: false,
            term_size: (120, 20),
            error: None,
            notice: None,
        };
        let task = ws.load(path);
        (ws, task)
    }

    /// Tab label: the repository folder name.
    pub fn name(&self) -> String {
        let path = self.repo.as_ref().map_or(&self.path, |r| &r.workdir);
        path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
    }

    pub fn session_running(&self) -> bool {
        self.sessions.iter().any(|s| s.running)
    }

    /// The session shown in the panel.
    pub fn session(&self) -> Option<&Session> {
        self.sessions.iter().find(|s| Some(s.id) == self.active_session)
    }

    fn session_mut(&mut self, id: SessionId) -> Option<&mut Session> {
        self.sessions.iter_mut().find(|s| s.id == id)
    }

    fn active_session_mut(&mut self) -> Option<&mut Session> {
        let id = self.active_session?;
        self.session_mut(id)
    }

    /// Sessions printing output right now.
    pub fn working(&self, now: Instant) -> usize {
        self.sessions.iter().filter(|s| s.activity(now) == Activity::Working).count()
    }

    pub fn update(&mut self, message: WsMessage) -> Task<WsMessage> {
        match message {
            WsMessage::ToggleSection(kind) => {
                if !self.collapsed.remove(&kind) {
                    self.collapsed.insert(kind);
                }
                Task::none()
            }
            WsMessage::Refresh => self.request_reload(),
            WsMessage::Loaded(result) => {
                self.loading = false;
                match result {
                    Ok(data) => self.apply(data),
                    Err(e) => {
                        self.error = Some(e);
                        Task::none()
                    }
                }
            }
            WsMessage::FsChanged => self.request_reload(),

            WsMessage::ShowWorktree => {
                self.selection = Selection::Worktree;
                self.graph_cache.clear();
                self.load_details()
            }
            WsMessage::SelectCommit(id) => {
                self.term_focused = false;
                self.selection = Selection::Commit(id);
                self.graph_cache.clear();
                self.load_details()
            }
            WsMessage::JumpToCommit(id) => {
                self.file_view = None;
                let row = self.repo.as_ref().and_then(|r| r.commit_index(&id));
                let task = self.update(WsMessage::SelectCommit(id));
                match row {
                    Some(row) => {
                        let top = self.repo.as_ref().map_or(0.0, |r| graph::row_top(r, row));
                        let y = (top - 120.0).max(0.0);
                        Task::batch([task, operation::scroll_to(GRAPH_SCROLL, AbsoluteOffset { x: None, y: Some(y) })])
                    }
                    None => {
                        self.notice = Some("Esse commit está além do limite carregado no grafo.".into());
                        task
                    }
                }
            }
            WsMessage::SelectReflog(id) => {
                if self.repo.as_ref().is_some_and(|r| r.commit_index(&id).is_some()) {
                    return self.update(WsMessage::JumpToCommit(id));
                }
                self.file_view = None;
                self.notice = Some("Commit fora do grafo: nenhuma branch ou tag chega até ele. Crie uma branch aqui para recuperá-lo.".into());
                self.update(WsMessage::SelectCommit(id))
            }
            WsMessage::CopyHash(id) => {
                self.notice = Some(format!("Hash {} copiado.", &id[..id.len().min(10)]));
                iced::clipboard::write(id)
            }
            WsMessage::DetailsLoaded(token, result) => {
                if token == self.details_token {
                    self.details = Some(result);
                }
                Task::none()
            }

            WsMessage::OpenFile(target) => {
                self.file_view = Some(FileView::new(target));
                self.load_file()
            }
            WsMessage::CloseFile => {
                self.file_view = None;
                Task::none()
            }
            WsMessage::SetFileMode(mode) => {
                if let Some(fv) = self.file_view.as_mut() {
                    fv.mode = mode;
                }
                self.load_file()
            }
            WsMessage::FileLoaded(token, loaded) => {
                if token == self.file_token
                    && let Some(fv) = self.file_view.as_mut()
                {
                    fv.accept(loaded);
                }
                Task::none()
            }
            WsMessage::ToggleSplit => {
                self.split = !self.split;
                Task::none()
            }
            WsMessage::ToggleChangesOnly => {
                self.changes_only = !self.changes_only;
                Task::none()
            }
            WsMessage::SetTreeView(tree) => {
                self.tree_view = tree;
                Task::none()
            }
            WsMessage::ToggleDir(dir) => {
                if !self.collapsed_dirs.remove(&dir) {
                    self.collapsed_dirs.insert(dir);
                }
                Task::none()
            }
            WsMessage::ToggleStashes => {
                self.collapsed_stash = !self.collapsed_stash;
                Task::none()
            }
            WsMessage::ThemeChanged => {
                self.graph_cache.clear();
                for s in &self.sessions {
                    s.cache.clear();
                }
                if let Some(fv) = self.file_view.as_mut() {
                    fv.invalidate();
                }
                self.load_file()
            }

            WsMessage::ToolSelected(name) => {
                self.tool = Some(name);
                Task::none()
            }
            WsMessage::NewSession => self.start_session(),
            WsMessage::SelectSession(id) => {
                if self.renaming.as_ref().is_some_and(|(r, _)| *r != id) {
                    self.finish_rename();
                }
                self.confirm_close_session = None;
                self.active_session = Some(id);
                self.term_focused = self.session().is_some_and(|s| s.running);
                Task::none()
            }
            WsMessage::StopSession(id) => {
                if let Some(t) = self.session_mut(id).and_then(|s| s.terminal.as_mut()) {
                    t.kill();
                }
                Task::none()
            }
            WsMessage::CloseSession(id) => {
                let Some(i) = self.sessions.iter().position(|s| s.id == id) else { return Task::none() };
                if self.sessions[i].running && self.confirm_close_session != Some(id) {
                    self.confirm_close_session = Some(id);
                    return Task::none();
                }
                self.confirm_close_session = None;
                // Dropping the session kills its terminal.
                self.sessions.remove(i);
                if self.active_session == Some(id) {
                    self.active_session = self.sessions.get(i.min(self.sessions.len().saturating_sub(1))).map(|s| s.id);
                    self.term_focused = false;
                }
                if self.renaming.as_ref().is_some_and(|(r, _)| *r == id) {
                    self.renaming = None;
                }
                Task::none()
            }
            WsMessage::RenameSession(id) => {
                let Some(name) = self.session_mut(id).map(|s| s.name.clone()) else { return Task::none() };
                self.term_focused = false;
                self.renaming = Some((id, name));
                Task::batch([operation::focus(SESSION_NAME), operation::select_all(SESSION_NAME)])
            }
            WsMessage::SessionNameInput(text) => {
                if let Some((_, name)) = self.renaming.as_mut() {
                    *name = text;
                }
                Task::none()
            }
            WsMessage::SessionNameSubmit => {
                self.finish_rename();
                Task::none()
            }
            WsMessage::Term(id, TermEvent::Output) => {
                if let Some(s) = self.session_mut(id) {
                    s.refresh();
                    s.mark_output(Instant::now());
                }
                Task::none()
            }
            WsMessage::Term(id, TermEvent::Exited(code)) => {
                if let Some(s) = self.session_mut(id) {
                    s.exited(code);
                }
                if self.active_session == Some(id) {
                    self.term_focused = false;
                }
                Task::none()
            }
            WsMessage::TermResize(cols, rows) => {
                self.term_size = (cols, rows);
                // Every session shares the panel, so they all take its size.
                for s in &mut self.sessions {
                    if let Some(t) = s.terminal.as_mut() {
                        t.resize(cols, rows);
                    }
                    s.refresh();
                }
                Task::none()
            }
            WsMessage::TermScroll(lines) => {
                if let Some(s) = self.active_session_mut() {
                    if let Some(t) = &s.terminal {
                        t.scroll(lines);
                    }
                    s.refresh();
                }
                Task::none()
            }
            WsMessage::TermScrollTo(offset) => {
                if let Some(s) = self.active_session_mut() {
                    if let Some(t) = &s.terminal {
                        t.scroll_to(offset);
                    }
                    s.refresh();
                }
                Task::none()
            }
            WsMessage::TermFocus(focused) => {
                self.term_focused = focused && self.session().is_some_and(|s| s.running);
                if let Some(s) = self.session() {
                    s.cache.clear();
                }
                Task::none()
            }
            WsMessage::Key(key, _, _) if self.branch_picker.is_some() => {
                use iced::keyboard::key::Named;
                let count = self.branch_choices().len();
                match (key, self.branch_picker.as_mut()) {
                    (Key::Named(Named::Escape), _) => self.branch_picker = None,
                    (Key::Named(Named::ArrowDown), Some(p)) => p.step(1, count),
                    (Key::Named(Named::ArrowUp), Some(p)) => p.step(-1, count),
                    _ => {}
                }
                Task::none()
            }
            WsMessage::OpenBranchPicker => {
                self.term_focused = false;
                self.branch_picker = Some(BranchPicker::default());
                iced::widget::operation::focus(branch_picker::INPUT)
            }
            WsMessage::CloseOverlays => {
                if self.renaming.take().is_none() && self.branch_picker.take().is_none() {
                    self.dialog = None;
                }
                Task::none()
            }
            WsMessage::CloseBranchPicker => {
                self.branch_picker = None;
                Task::none()
            }
            WsMessage::BranchQuery(query) => {
                if let Some(p) = self.branch_picker.as_mut() {
                    p.query = query;
                    p.highlighted = 0;
                }
                Task::none()
            }
            WsMessage::BranchSubmit => {
                let index = self.branch_picker.as_ref().map_or(0, |p| p.highlighted);
                match self.branch_choices().into_iter().nth(index) {
                    Some(choice) => self.update(WsMessage::PickBranch(choice)),
                    None => Task::none(),
                }
            }
            WsMessage::PickBranch(choice) => {
                self.branch_picker = None;
                let current = self.repo.as_ref().and_then(|r| r.current_branch().map(str::to_owned));
                match choice {
                    Choice::Local(name) if Some(&name) == current.as_ref() => Task::none(),
                    Choice::Local(name) => self.request(Op::Checkout(CheckoutTarget::Branch(name))),
                    Choice::Remote(name) => self.request(Op::Checkout(CheckoutTarget::Remote(name))),
                    Choice::Create(name) => self.request(Op::CreateBranch { name, at: "HEAD".into(), checkout: true }),
                }
            }
            WsMessage::Key(key, _, _) if self.dialog.is_some() => {
                if key == Key::Named(iced::keyboard::key::Named::Escape) {
                    self.dialog = None;
                }
                Task::none()
            }
            WsMessage::Key(key, mods, text) => {
                let focused = self.term_focused;
                let Some(session) = self.active_session_mut().filter(|s| focused && s.terminal.is_some()) else {
                    return match keys::history_shortcut(&key, mods) {
                        // Only when no commit field is being typed in.
                        Some(redo) => operation::is_focused(COMMIT_SUMMARY).then(move |summary| {
                            operation::is_focused(COMMIT_BODY)
                                .map(move |body| WsMessage::HistoryShortcut { redo, typing: summary || body })
                        }),
                        None => Task::none(),
                    };
                };
                if keys::is_paste(&key, mods) {
                    return iced::clipboard::read().map(WsMessage::Paste);
                }
                if let Some(term) = &session.terminal
                    && let Some(bytes) = keys::encode(&key, mods, text.as_deref(), term.app_cursor())
                {
                    term.write(&bytes);
                    session.mark_input(Instant::now());
                }
                Task::none()
            }
            WsMessage::Paste(text) => {
                if let (Some(session), Some(text)) = (self.active_session_mut(), text)
                    && let Some(term) = &session.terminal
                {
                    term.write(&keys::paste(&text, term.bracketed_paste()));
                    session.mark_input(Instant::now());
                }
                Task::none()
            }
            // The app intercepts this to show the menu.
            WsMessage::OpenMenu(_) => Task::none(),
            WsMessage::Request(op) => self.request(op),
            WsMessage::OpDone(op, result, change) => {
                self.busy = None;
                if let (Some(change), Some(mode)) = (change, op.undo_move()) {
                    let (before, after) = *change;
                    self.history.record(history::Entry {
                        label: op.history_label(),
                        before,
                        after,
                        mode,
                        message: match &op {
                            Op::Commit { message, .. } => Some(message.clone()),
                            _ => None,
                        },
                        hard_reset: matches!(op, Op::Reset { mode: ResetMode::Hard, .. }),
                    });
                }
                match result {
                    Ok(url) => {
                        if let Some(url) = url {
                            util::open_url(&url);
                        }
                        let done = op.done();
                        if !done.is_empty() {
                            self.error = None;
                            self.notice = Some(done);
                        }
                        if matches!(op, Op::Commit { .. }) {
                            self.commit_summary.clear();
                            self.commit_body = text_editor::Content::new();
                            self.amend = false;
                        }
                    }
                    Err(e) => self.error = Some(e),
                }
                self.request_reload()
            }
            WsMessage::Undo => self.run_history(false),
            WsMessage::Redo => self.run_history(true),
            WsMessage::HistoryShortcut { redo, typing } => {
                if typing { Task::none() } else { self.run_history(redo) }
            }
            WsMessage::HistoryDone { redo, result } => {
                self.busy = None;
                match result {
                    Ok(()) => {
                        let entry = if redo { self.history.next_redo() } else { self.history.next_undo() }.cloned();
                        if redo { self.history.redone() } else { self.history.undone() }
                        if let Some(e) = entry {
                            let mut msg = format!("{}: {}.", if redo { "Refeito" } else { "Desfeito" }, e.label);
                            if !redo && e.hard_reset {
                                msg.push_str(" As alterações descartadas pelo reset --hard não voltam.");
                            }
                            if !redo && let Some(m) = &e.message {
                                // The undone commit's changes are back in stage; so is its message.
                                self.set_commit_message(m);
                            }
                            self.notice = Some(msg);
                        }
                    }
                    Err(e) => self.error = Some(e),
                }
                self.request_reload()
            }
            WsMessage::Checkout(target) => self.request(Op::Checkout(target)),
            WsMessage::CheckoutCommit(id) => self.checkout_commit(&id),
            WsMessage::CopyText(text) => {
                self.notice = Some(format!("Copiado: {}", crate::menu::rev_label(&text)));
                iced::clipboard::write(text)
            }
            WsMessage::CopyCommitMessage(id) => {
                let Some(repo) = &self.repo else { return Task::none() };
                let git = repo.git();
                Task::perform(
                    blocking(move || {
                        let info = brio_ops::files::commit_info(&git, &id).map_err(|e| e.to_string())?;
                        Ok(if info.body.is_empty() { info.summary } else { format!("{}\n\n{}", info.summary, info.body) })
                    }),
                    WsMessage::CommitMessageCopied,
                )
            }
            WsMessage::CommitMessageCopied(result) => match result {
                Ok(text) => {
                    self.notice = Some("Mensagem do commit copiada.".into());
                    iced::clipboard::write(text)
                }
                Err(e) => {
                    self.error = Some(e);
                    Task::none()
                }
            },
            WsMessage::Compare { from, to } => {
                self.selection = Selection::Compare { from, to };
                self.graph_cache.clear();
                self.load_details()
            }
            WsMessage::TogglePin(name) => {
                self.pinned = if self.pinned.as_deref() == Some(name.as_str()) { None } else { Some(name) };
                self.request_reload()
            }

            WsMessage::OpenDialog(kind) => {
                let check = matches!(kind, DialogKind::CreateBranch { .. });
                self.dialog = Some(Dialog { kind, input: String::new(), input2: String::new(), check, error: None });
                iced::widget::operation::focus(DIALOG_INPUT)
            }
            WsMessage::DialogInput(v) => {
                if let Some(d) = self.dialog.as_mut() {
                    d.input = v;
                    d.error = None;
                }
                Task::none()
            }
            WsMessage::DialogInput2(v) => {
                if let Some(d) = self.dialog.as_mut() {
                    d.input2 = v;
                }
                Task::none()
            }
            WsMessage::DialogCheck(v) => {
                if let Some(d) = self.dialog.as_mut() {
                    d.check = v;
                }
                Task::none()
            }
            WsMessage::DialogCancel => {
                self.dialog = None;
                Task::none()
            }
            WsMessage::DialogConfirm => self.confirm_dialog(),

            WsMessage::CommitSummary(v) => {
                self.commit_summary = v;
                Task::none()
            }
            WsMessage::CommitBody(action) => {
                self.commit_body.perform(action);
                Task::none()
            }
            WsMessage::ToggleAmend(amend) => {
                self.amend = amend;
                // Start from the message being amended.
                if amend
                    && self.commit_summary.trim().is_empty()
                    && let Some(repo) = &self.repo
                    && let Ok(message) = brio_ops::actions::last_commit_message(&repo.git())
                {
                    self.set_commit_message(&message);
                }
                Task::none()
            }
            WsMessage::GenerateMessage => self.generate_message(brio_session::ai::PROMPT.to_owned()),
            WsMessage::GenerateWith(prompt) => self.generate_message(prompt),
            WsMessage::MessageGenerated(result) => {
                self.generating = false;
                match result {
                    Ok(message) => self.set_commit_message(&message),
                    Err(e) => self.error = Some(e),
                }
                Task::none()
            }
            WsMessage::DoCommit => {
                let summary = self.commit_summary.trim();
                if summary.is_empty() {
                    self.error = Some("Escreva o resumo do commit.".into());
                    return Task::none();
                }
                let body = self.commit_body.text();
                let message = if body.trim().is_empty() { summary.to_owned() } else { format!("{summary}\n\n{}", body.trim()) };
                self.request(Op::Commit { message, amend: self.amend })
            }
            WsMessage::StageAll => {
                let paths = self.paths(|c| c.unstaged);
                if paths.is_empty() { Task::none() } else { self.request(Op::Stage(paths)) }
            }
            WsMessage::UnstageAll => {
                let paths = self.paths(|c| c.staged);
                if paths.is_empty() { Task::none() } else { self.request(Op::Unstage(paths)) }
            }
        }
    }

    fn load(&mut self, path: PathBuf) -> Task<WsMessage> {
        self.loading = true;
        let pinned = self.pinned.clone();
        Task::perform(blocking(move || RepoData::load(&path, pinned.as_deref()).map(Arc::new)), WsMessage::Loaded)
    }

    fn request_reload(&mut self) -> Task<WsMessage> {
        let Some(repo) = &self.repo else { return Task::none() };
        if self.loading {
            self.reload_pending = true;
            return Task::none();
        }
        let path = repo.workdir.clone();
        self.load(path)
    }

    fn apply(&mut self, data: Arc<RepoData>) -> Task<WsMessage> {
        let fresh = self.repo.as_ref().is_none_or(|r| r.workdir != data.workdir);
        let mut tasks = Vec::new();

        if fresh {
            self.path = data.workdir.clone();
            self.selection = Selection::Worktree;
            self.details = None;
            self.file_view = None;
            self.error = None;
            match brio_session::watcher::watch(&data.workdir, &data.git_dir) {
                Ok((handle, rx)) => {
                    self.watch = Some(handle);
                    tasks.push(Task::run(rx, |_| WsMessage::FsChanged));
                }
                Err(e) => self.error = Some(format!("Sem atualização automática: {e}")),
            }
        }

        // Keep the selection meaningful after a refresh.
        // Commits picked from the reflog may be unreachable, yet still exist.
        if let Selection::Commit(id) = &self.selection
            && data.commit_index(id).is_none()
            && !data.reflog.iter().any(|e| &e.id == id)
        {
            self.selection = Selection::Worktree;
        }

        self.repo = Some(data);
        self.graph_cache.clear();
        // A file open from the working tree follows its edits live.
        if let Some(fv) = self.file_view.as_mut()
            && fv.target.is_worktree()
        {
            fv.invalidate();
            tasks.push(self.load_file());
        }
        if std::mem::take(&mut self.reload_pending) {
            tasks.push(self.request_reload());
        }
        Task::batch(tasks)
    }

    fn load_details(&mut self) -> Task<WsMessage> {
        self.details_token += 1;
        self.details = None;
        let token = self.details_token;
        let Some(repo) = &self.repo else { return Task::none() };
        let git = repo.git();
        let job: Box<dyn FnOnce() -> Result<Details, String> + Send> = match &self.selection {
            Selection::Worktree => return Task::none(),
            Selection::Commit(id) => {
                let id = id.clone();
                Box::new(move || fileview::commit_details(&git, &id))
            }
            Selection::Compare { from, to } => {
                let (from, to) = (from.clone(), to.clone());
                Box::new(move || fileview::compare_details(&git, &from, to.as_deref()))
            }
        };
        Task::perform(blocking(move || job().map(Arc::new)), move |r| WsMessage::DetailsLoaded(token, r))
    }

    /// Loads what the file viewer's current mode needs, if not loaded yet.
    fn load_file(&mut self) -> Task<WsMessage> {
        let (Some(repo), Some(fv)) = (&self.repo, &self.file_view) else { return Task::none() };
        if !fv.needs_load() {
            return Task::none();
        }
        self.file_token += 1;
        let token = self.file_token;
        let job = fv.job(repo.git());
        Task::perform(blocking(move || Ok(job())), move |r: Result<Loaded, String>| {
            WsMessage::FileLoaded(token, r.unwrap_or_else(|e| Loaded::Diff(Err(e))))
        })
    }

    /// Entries of the open branch picker for its current query.
    pub fn branch_choices(&self) -> Vec<Choice> {
        match (&self.repo, &self.branch_picker) {
            (Some(repo), Some(p)) => branch_picker::choices(repo, &p.query),
            _ => Vec::new(),
        }
    }

    fn is_dirty(&self) -> bool {
        self.repo.as_ref().is_some_and(|r| r.changes.iter().any(|c| c.kind != brio_ops::ChangeKind::Untracked))
    }

    fn paths(&self, pick: impl Fn(&brio_ops::FileChange) -> bool) -> Vec<String> {
        self.repo.as_ref().map(|r| r.changes.iter().filter(|c| pick(c)).map(|c| c.path.clone()).collect()).unwrap_or_default()
    }

    fn set_commit_message(&mut self, message: &str) {
        let message = message.trim();
        let (summary, body) = message.split_once('\n').unwrap_or((message, ""));
        self.commit_summary = summary.trim().to_owned();
        self.commit_body = text_editor::Content::with_text(body.trim());
    }

    /// Asks before checkouts over local changes and before destructive ops.
    fn request(&mut self, op: Op) -> Task<WsMessage> {
        if let Op::Checkout(target) = &op
            && self.is_dirty()
        {
            self.dialog = Some(Dialog { kind: DialogKind::DirtyCheckout(target.clone()), input: String::new(), input2: String::new(), check: false, error: None });
            return Task::none();
        }
        if op.confirmation().is_some() {
            self.dialog = Some(Dialog { kind: DialogKind::Confirm(op), input: String::new(), input2: String::new(), check: false, error: None });
            return Task::none();
        }
        self.run_op(op)
    }

    fn run_op(&mut self, op: Op) -> Task<WsMessage> {
        let Some(repo) = &self.repo else { return Task::none() };
        if let Some(busy) = &self.busy {
            self.notice = Some(format!("Aguarde: {busy}"));
            return Task::none();
        }
        self.busy = Some(op.progress());
        let git = repo.git();
        let job = op.clone();
        Task::perform(blocking(move || Ok(job.run_recorded(&git))), move |r: Result<_, String>| match r {
            Ok((result, change)) => WsMessage::OpDone(op.clone(), result, change.map(Box::new)),
            Err(e) => WsMessage::OpDone(op.clone(), Err(e), None),
        })
    }

    /// Undoes the last operation, or redoes the last undone one, if the
    /// repository is still exactly as that operation left it.
    fn run_history(&mut self, redo: bool) -> Task<WsMessage> {
        let Some(repo) = &self.repo else { return Task::none() };
        if let Some(busy) = &self.busy {
            self.notice = Some(format!("Aguarde: {busy}"));
            return Task::none();
        }
        let entry = if redo { self.history.next_redo() } else { self.history.next_undo() };
        let Some(entry) = entry.cloned() else {
            self.notice = Some(if redo { "Nada para refazer." } else { "Nada para desfazer." }.into());
            return Task::none();
        };
        let (expected, target) = if redo { (entry.before, entry.after) } else { (entry.after, entry.before) };
        self.busy = Some(format!("{} {}…", if redo { "Refazendo" } else { "Desfazendo" }, entry.label));
        let git = repo.git();
        let label = entry.label.clone();
        let mode = entry.mode;
        Task::perform(
            blocking(move || {
                let now = undo::snapshot(&git).map_err(|e| e.to_string())?;
                if now != expected {
                    return Err(format!(
                        "Não dá para {} \"{label}\": o repositório mudou depois disso (outro commit, checkout ou branch).",
                        if redo { "refazer" } else { "desfazer" }
                    ));
                }
                undo::restore(&git, &target, mode).map_err(|e| e.to_string())
            }),
            move |result| WsMessage::HistoryDone { redo, result },
        )
    }

    fn confirm_dialog(&mut self) -> Task<WsMessage> {
        let Some(dialog) = self.dialog.as_mut() else { return Task::none() };
        let name = dialog.input.trim().to_owned();
        let needs_name = matches!(dialog.kind, DialogKind::CreateBranch { .. } | DialogKind::CreateTag { .. } | DialogKind::RenameBranch { .. });
        if needs_name && let Err(e) = validate_ref_name(&name) {
            dialog.error = Some(e);
            return Task::none();
        }
        let op = match dialog.kind.clone() {
            DialogKind::CreateBranch { at } => Op::CreateBranch { name, at, checkout: dialog.check },
            DialogKind::CreateTag { at } => {
                let message = Some(dialog.input2.trim().to_owned()).filter(|m| !m.is_empty());
                Op::CreateTag { name, at, message }
            }
            DialogKind::RenameBranch { old } => Op::RenameBranch { old, new: name },
            DialogKind::Confirm(op) => op,
            DialogKind::DirtyCheckout(target) => Op::StashAndCheckout(target),
        };
        self.dialog = None;
        self.run_op(op)
    }

    /// Double-click: check out a branch at the commit, else track a remote
    /// branch, else detach.
    fn checkout_commit(&mut self, id: &str) -> Task<WsMessage> {
        let Some(repo) = &self.repo else { return Task::none() };
        let current = repo.current_branch();
        let refs: Vec<_> = repo.refs_at(id).collect();
        if let Some(cur) = current
            && refs.iter().any(|r| r.kind == RefKind::LocalBranch && r.name == cur)
        {
            self.notice = Some(format!("Você já está em {cur}."));
            return Task::none();
        }
        let target = if let Some(local) = refs.iter().find(|r| r.kind == RefKind::LocalBranch) {
            CheckoutTarget::Branch(local.name.clone())
        } else if let Some(remote) = refs.iter().find(|r| r.kind == RefKind::RemoteBranch) {
            CheckoutTarget::Remote(remote.name.clone())
        } else {
            CheckoutTarget::Detached(id.to_owned())
        };
        self.request(Op::Checkout(target))
    }

    fn generate_message(&mut self, prompt: String) -> Task<WsMessage> {
        let Some(repo) = &self.repo else { return Task::none() };
        let Some(tool) = self.tools.iter().find(|t| Some(&t.name) == self.tool.as_ref()).cloned() else {
            self.error = Some("Escolha uma IA na Sessão de IA.".into());
            return Task::none();
        };
        if !brio_session::ai::supports(&tool) {
            self.error = Some(format!("{} não gera mensagens sem interface; escolha claude, codex ou gemini.", tool.name));
            return Task::none();
        }
        if !repo.changes.iter().any(|c| c.staged) {
            self.error = Some("Coloque arquivos em stage antes de gerar a mensagem.".into());
            return Task::none();
        }
        self.generating = true;
        let git = repo.git();
        let cwd = repo.workdir.clone();
        Task::perform(
            blocking(move || {
                let diff = brio_ops::actions::staged_diff(&git).map_err(|e| e.to_string())?;
                brio_session::ai::commit_message(&tool, &cwd, &diff, &prompt)
            }),
            WsMessage::MessageGenerated,
        )
    }

    fn start_session(&mut self) -> Task<WsMessage> {
        let Some(repo) = &self.repo else { return Task::none() };
        let Some(tool) = self.tools.iter().find(|t| Some(&t.name) == self.tool.as_ref()).cloned() else {
            self.error = Some("Escolha uma ferramenta.".into());
            return Task::none();
        };
        let (cols, rows) = self.term_size;
        match Terminal::spawn(&tool, &repo.workdir, cols, rows) {
            Ok((terminal, rx)) => {
                self.finish_rename();
                let id = self.next_session;
                self.next_session += 1;
                let name = session::default_name(&tool.name, self.sessions.iter().map(|s| s.name.as_str()));
                self.sessions.push(Session::new(id, name, tool.name.clone(), Some(terminal)));
                self.active_session = Some(id);
                self.term_focused = true;
                Task::run(rx, move |e| WsMessage::Term(id, e))
            }
            Err(e) => {
                self.error = Some(format!("Não foi possível iniciar {}: {e}", tool.name));
                Task::none()
            }
        }
    }

    /// Saves the name being edited; an empty one keeps the old name.
    fn finish_rename(&mut self) {
        let Some((id, name)) = self.renaming.take() else { return };
        let name = name.trim();
        if let Some(s) = self.session_mut(id)
            && !name.is_empty()
        {
            s.name = name.to_owned();
        }
    }
}

/// Rejects names git would refuse, with a readable reason.
fn validate_ref_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Digite um nome.".into());
    }
    let bad = name.chars().any(|c| c.is_whitespace() || "~^:?*[\\".contains(c))
        || name.contains("..")
        || name.contains("@{")
        || name.starts_with(['-', '/'])
        || name.ends_with(['/', '.'])
        || name.ends_with(".lock");
    if bad { Err("Nome inválido: sem espaços nem ~ ^ : ? * [ \\ ..".into()) } else { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ref_names_are_validated() {
        assert!(validate_ref_name("feat/login-jwt").is_ok());
        assert!(validate_ref_name("v1.2.0").is_ok());
        for bad in ["", "com espaço", "a..b", "x~1", "fim/", "-x", "a.lock", "a@{b"] {
            assert!(validate_ref_name(bad).is_err(), "{bad}");
        }
    }
}
