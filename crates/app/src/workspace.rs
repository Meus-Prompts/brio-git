//! One open repository (a tab): its data, live watcher, AI session and
//! checkpoint timeline. Views live in `view.rs`.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use iced::keyboard::{Key, Modifiers};
use iced::widget::canvas::Cache;
use iced::widget::operation;
use iced::widget::scrollable::AbsoluteOffset;
use iced::Task;
use lazygit_core::RefKind;
use lazygit_ops::checkpoint::{self, Checkpoint};
use lazygit_session::{ChangeBatch, CheckpointScheduler, TermEvent, TermSnapshot, Terminal, Tool, WatchHandle};

use crate::data::RepoData;
use crate::fileview::{self, Details, FileTarget, FileView, Loaded, Mode};
use crate::util::{self, blocking};
use crate::widgets::graph::ROW_H;
use crate::keys;

pub const GRAPH_SCROLL: &str = "graph";

/// What the right-hand panel shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    /// Live changes in the working tree.
    Worktree,
    Commit(String),
    Checkpoint(u32),
}

/// The AI session running in the embedded terminal.
pub struct Session {
    pub tool: String,
    pub running: bool,
    pub scheduler: CheckpointScheduler,
    pub exit_code: Option<u32>,
}

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
    /// Commit or checkpoint shown in the details panel.
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

    pub tools: Vec<Tool>,
    pub tool: Option<String>,
    pub terminal: Option<Terminal>,
    pub term_snapshot: Option<TermSnapshot>,
    pub term_cache: Cache,
    pub term_focused: bool,
    pub term_size: (u16, u16),
    pub session: Option<Session>,

    /// Session whose checkpoints the timeline shows (current or most recent).
    pub timeline_session: Option<String>,
    pub checkpoints: Vec<Checkpoint>,
    checkpoint_busy: bool,
    queued_checkpoint: Option<String>,

    pub error: Option<String>,
    pub notice: Option<String>,
}

#[derive(Debug, Clone)]
pub enum WsMessage {
    ToggleSection(RefKind),
    Refresh,
    Loaded(Result<Arc<RepoData>, String>),
    FsChanged(ChangeBatch),

    ShowWorktree,
    SelectCommit(String),
    JumpToCommit(String),
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
    ThemeChanged,

    ToolSelected(String),
    StartSession,
    StopSession,
    Term(TermEvent),
    TermResize(u16, u16),
    TermScroll(i32),
    TermFocus(bool),
    Key(Key, Modifiers, Option<String>),
    Paste(Option<String>),
    Tick(Instant),

    CheckpointNow,
    CheckpointDone(Result<Option<Checkpoint>, String>),
    TimelineLoaded(Result<Option<(String, Vec<Checkpoint>)>, String>),
    SelectCheckpoint(u32),
    RestoreCheckpoint(u32),
    Restored(Result<Checkpoint, String>),

    DismissError,
}

impl Workspace {
    pub fn new(path: PathBuf, tools: Vec<Tool>) -> (Self, Task<WsMessage>) {
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
            tools,
            tool,
            terminal: None,
            term_snapshot: None,
            term_cache: Cache::new(),
            term_focused: false,
            term_size: (120, 20),
            session: None,
            timeline_session: None,
            checkpoints: Vec::new(),
            checkpoint_busy: false,
            queued_checkpoint: None,
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
        self.session.as_ref().is_some_and(|s| s.running)
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
            WsMessage::FsChanged(batch) => {
                if batch.worktree
                    && let Some(s) = self.session.as_mut().filter(|s| s.running)
                {
                    s.scheduler.on_change(Instant::now());
                }
                self.request_reload()
            }

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
                        let y = (row as f32 * ROW_H - 120.0).max(0.0);
                        Task::batch([task, operation::scroll_to(GRAPH_SCROLL, AbsoluteOffset { x: None, y: Some(y) })])
                    }
                    None => {
                        self.notice = Some("Esse commit está além do limite carregado no grafo.".into());
                        task
                    }
                }
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
            WsMessage::ThemeChanged => {
                self.graph_cache.clear();
                self.term_cache.clear();
                if let Some(fv) = self.file_view.as_mut() {
                    fv.invalidate();
                }
                self.load_file()
            }

            WsMessage::ToolSelected(name) => {
                self.tool = Some(name);
                Task::none()
            }
            WsMessage::StartSession => self.start_session(),
            WsMessage::StopSession => {
                if let Some(t) = self.terminal.as_mut() {
                    t.kill();
                }
                Task::none()
            }
            WsMessage::Term(TermEvent::Output) => {
                if let Some(t) = &self.terminal {
                    self.term_snapshot = Some(t.snapshot());
                    self.term_cache.clear();
                }
                Task::none()
            }
            WsMessage::Term(TermEvent::Exited(code)) => {
                if let Some(t) = self.terminal.take() {
                    self.term_snapshot = Some(t.snapshot());
                    self.term_cache.clear();
                }
                self.term_focused = false;
                if let Some(s) = self.session.as_mut() {
                    s.running = false;
                    s.exit_code = code;
                }
                self.checkpoint("fim da sessão".into())
            }
            WsMessage::TermResize(cols, rows) => {
                self.term_size = (cols, rows);
                if let Some(t) = self.terminal.as_mut() {
                    t.resize(cols, rows);
                    self.term_snapshot = Some(t.snapshot());
                    self.term_cache.clear();
                }
                Task::none()
            }
            WsMessage::TermScroll(lines) => {
                if let Some(t) = &self.terminal {
                    t.scroll(lines);
                    self.term_snapshot = Some(t.snapshot());
                    self.term_cache.clear();
                }
                Task::none()
            }
            WsMessage::TermFocus(focused) => {
                self.term_focused = focused && self.terminal.is_some();
                self.term_cache.clear();
                Task::none()
            }
            WsMessage::Key(key, mods, text) => {
                let Some(term) = self.terminal.as_ref().filter(|_| self.term_focused) else {
                    return Task::none();
                };
                if keys::is_paste(&key, mods) {
                    return iced::clipboard::read().map(WsMessage::Paste);
                }
                if let Some(bytes) = keys::encode(&key, mods, text.as_deref(), term.app_cursor()) {
                    term.write(&bytes);
                }
                Task::none()
            }
            WsMessage::Paste(text) => {
                if let (Some(term), Some(text)) = (&self.terminal, text) {
                    term.write(&keys::paste(&text, term.bracketed_paste()));
                }
                Task::none()
            }
            WsMessage::Tick(now) => {
                let due = self.session.as_ref().is_some_and(|s| s.running && s.scheduler.is_due(now));
                if due {
                    let n = self.repo.as_ref().map_or(0, |r| r.changes.len());
                    self.checkpoint(format!("{n} arquivo(s) alterado(s)"))
                } else {
                    Task::none()
                }
            }

            WsMessage::CheckpointNow => {
                // Without any session yet, a manual checkpoint starts a timeline of its own.
                if self.timeline_session.is_none() {
                    self.timeline_session = Some(util::session_id(util::now_secs()));
                }
                self.checkpoint("manual".into())
            }
            WsMessage::CheckpointDone(result) => {
                self.checkpoint_busy = false;
                match result {
                    Ok(Some(cp)) => {
                        if self.timeline_session.as_deref() == Some(cp.session.as_str()) {
                            self.checkpoints.push(cp);
                        }
                    }
                    Ok(None) => {}
                    Err(e) => self.error = Some(format!("Falha ao criar checkpoint: {e}")),
                }
                match self.queued_checkpoint.take() {
                    Some(label) => self.checkpoint(label),
                    None => Task::none(),
                }
            }
            WsMessage::TimelineLoaded(result) => {
                match result {
                    Ok(None) => {}
                    Ok(Some((session, list))) => {
                        if self.session.is_none() {
                            self.timeline_session = Some(session);
                            self.checkpoints = list;
                        }
                    }
                    Err(e) => self.error = Some(e),
                }
                Task::none()
            }
            WsMessage::SelectCheckpoint(seq) => {
                self.selection = Selection::Checkpoint(seq);
                self.graph_cache.clear();
                self.load_details()
            }
            WsMessage::RestoreCheckpoint(seq) => self.restore(seq),
            WsMessage::Restored(result) => {
                self.checkpoint_busy = false;
                match result {
                    Ok(safety) => {
                        self.notice = Some(format!(
                            "Checkpoint restaurado. O estado anterior foi salvo como #{}.",
                            safety.seq
                        ));
                        self.checkpoints.push(safety);
                        self.request_reload()
                    }
                    Err(e) => {
                        self.error = Some(format!("Falha ao restaurar: {e}"));
                        Task::none()
                    }
                }
            }

            WsMessage::DismissError => {
                self.error = None;
                self.notice = None;
                Task::none()
            }
        }
    }

    fn load(&mut self, path: PathBuf) -> Task<WsMessage> {
        self.loading = true;
        Task::perform(blocking(move || RepoData::load(&path).map(Arc::new)), WsMessage::Loaded)
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
            self.checkpoints.clear();
            self.timeline_session = None;
            self.error = None;
            match lazygit_session::watcher::watch(&data.workdir, &data.git_dir) {
                Ok((handle, rx)) => {
                    self.watch = Some(handle);
                    tasks.push(Task::run(rx, WsMessage::FsChanged));
                }
                Err(e) => self.error = Some(format!("Sem atualização automática: {e}")),
            }
            let git = data.git();
            tasks.push(Task::perform(
                blocking(move || {
                    let sessions = checkpoint::sessions(&git).map_err(|e| e.to_string())?;
                    let Some(last) = sessions.last() else { return Ok(None) };
                    let list = checkpoint::list(&git, last).map_err(|e| e.to_string())?;
                    Ok(Some((last.clone(), list)))
                }),
                WsMessage::TimelineLoaded,
            ));
        }

        // Keep the selection meaningful after a refresh.
        if let Selection::Commit(id) = &self.selection
            && data.commit_index(id).is_none()
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
            Selection::Checkpoint(seq) => {
                let Some(pos) = self.checkpoints.iter().position(|c| c.seq == *seq) else {
                    return Task::none();
                };
                let id = self.checkpoints[pos].id.clone();
                let prev = pos.checked_sub(1).map(|p| self.checkpoints[p].id.clone());
                Box::new(move || fileview::checkpoint_details(&git, &id, prev))
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

    fn start_session(&mut self) -> Task<WsMessage> {
        let Some(repo) = &self.repo else { return Task::none() };
        let Some(tool) = self.tools.iter().find(|t| Some(&t.name) == self.tool.as_ref()).cloned() else {
            self.error = Some("Escolha uma ferramenta.".into());
            return Task::none();
        };
        let (cols, rows) = self.term_size;
        match Terminal::spawn(&tool, &repo.workdir, cols, rows) {
            Ok((terminal, rx)) => {
                let id = util::session_id(util::now_secs());
                self.terminal = Some(terminal);
                self.term_snapshot = None;
                self.term_cache.clear();
                self.term_focused = true;
                self.session = Some(Session {
                    tool: tool.name.clone(),
                    running: true,
                    scheduler: CheckpointScheduler::default(),
                    exit_code: None,
                });
                self.timeline_session = Some(id);
                self.checkpoints.clear();
                let start = self.checkpoint("início da sessão".into());
                Task::batch([Task::run(rx, WsMessage::Term), start])
            }
            Err(e) => {
                self.error = Some(format!("Não foi possível iniciar {}: {e}", tool.name));
                Task::none()
            }
        }
    }

    fn next_seq(&self) -> u32 {
        self.checkpoints.iter().map(|c| c.seq + 1).max().unwrap_or(0)
    }

    /// Takes a checkpoint of the timeline session, one at a time.
    fn checkpoint(&mut self, label: String) -> Task<WsMessage> {
        let (Some(repo), Some(session)) = (&self.repo, self.timeline_session.clone()) else {
            return Task::none();
        };
        if self.checkpoint_busy {
            self.queued_checkpoint = Some(label);
            return Task::none();
        }
        self.checkpoint_busy = true;
        if let Some(s) = self.session.as_mut() {
            s.scheduler.checkpointed(Instant::now());
        }
        let git = repo.git();
        let seq = self.next_seq();
        Task::perform(
            blocking(move || checkpoint::create(&git, &session, seq, &label).map_err(|e| e.to_string())),
            WsMessage::CheckpointDone,
        )
    }

    fn restore(&mut self, seq: u32) -> Task<WsMessage> {
        let Some(repo) = &self.repo else { return Task::none() };
        let Some(target) = self.checkpoints.iter().find(|c| c.seq == seq).cloned() else {
            return Task::none();
        };
        if self.checkpoint_busy {
            self.notice = Some("Aguarde o checkpoint em andamento terminar.".into());
            return Task::none();
        }
        self.checkpoint_busy = true;
        let git = repo.git();
        let safety = self.next_seq();
        Task::perform(
            blocking(move || checkpoint::restore(&git, &target, safety).map_err(|e| e.to_string())),
            WsMessage::Restored,
        )
    }
}
