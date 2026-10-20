//! Top-level state: tabs (one [`Workspace`] per repository), the window and
//! the panel layout shared by all tabs.

use std::cell::Cell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use iced::keyboard::{Key, Modifiers};
use iced::widget::{pane_grid, text_editor};
use iced::{Point, Size, Subscription, Task, Theme, event, keyboard, window};
use brio_session::Tool;

use crate::avatars::{self, Avatars};
use crate::layout::{Layout, PaneKind};
use crate::menu::{self, MenuItem};
use crate::persist::{self, Saved};
use crate::session::SessionId;
use crate::style;
use crate::toasts::{self, Toasts};
use crate::update::{self, Outcome};
use crate::util::blocking;
use crate::workspace::{Workspace, WsMessage};

pub type TabId = u64;

pub struct Tab {
    pub id: TabId,
    pub ws: Workspace,
}

pub struct App {
    pub tabs: Vec<Tab>,
    pub active: usize,
    next_id: TabId,
    tools: Vec<Tool>,

    pub layout: Layout,
    pub panes: Option<pane_grid::State<PaneKind>>,

    window: Option<window::Id>,
    pub maximized: bool,
    state_file: Option<PathBuf>,
    /// Tab waiting for a second click on ✕ because its AI is still running.
    pub confirm_close: Option<TabId>,
    pub error: Option<String>,
    pub theme: Theme,
    /// Open settings modal and the section it shows.
    pub settings: Option<Section>,
    /// Instruction sent to the AI before the staged diff; empty means [`brio_session::ai::PROMPT`].
    pub commit_prompt: String,
    pub prompt_editor: text_editor::Content,
    pub toasts: Toasts,
    pub avatars: Avatars,

    pub auto_update: bool,
    /// A newer release: installed and waiting for a restart, or only available.
    pub update: Option<Outcome>,
    pub checking_update: bool,
    /// Result of the last check, shown in the settings.
    pub update_note: Option<String>,
    /// "Restart" was clicked once while AI sessions were running.
    pub confirm_restart: bool,

    /// Open context menu, if any.
    pub menu: Option<ContextMenu>,
    /// List of every open AI session, opened from the status bar.
    pub session_list: bool,
    /// Last cursor position in window coordinates, written by an invisible
    /// layer without producing messages (see `view::CursorTracker`).
    pub cursor: Cell<Point>,
    pub window_size: Size,
    /// Pinned branch per repository work tree, as saved.
    pinned: HashMap<PathBuf, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Appearance,
    CommitAi,
    Updates,
}

pub struct ContextMenu {
    pub tab: TabId,
    pub items: Vec<MenuItem>,
    pub position: Point,
    /// Item whose submenu is open.
    pub open_child: Option<usize>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Tab(TabId, WsMessage),
    SelectTab(TabId),
    CloseTab(TabId),
    PickFolder,
    FolderPicked(Option<PathBuf>),

    PaneResized(pane_grid::ResizeEvent),
    PaneDragged(pane_grid::DragEvent),
    ResetLayout,
    TogglePane(PaneKind),
    MaximizePane(pane_grid::Pane),
    RestorePanes,

    WindowReady(Option<window::Id>),
    DragWindow,
    ResizeWindow(window::Direction),
    Minimize,
    ToggleMaximize,
    Close,

    OpenSettings,
    CloseSettings,
    ShowSection(Section),
    SetTheme(&'static str),
    SetLook(style::Look),
    EditPrompt(text_editor::Action),
    ResetPrompt,
    DismissToast(u64),
    Tick(Instant),
    MenuPick(WsMessage),
    MenuHover(Option<usize>),
    CloseMenu,
    ToggleSessionList,
    /// Shows a session: selects its tab and opens the session panel.
    GoToSession(TabId, SessionId),
    WindowResized(Size),
    AvatarLoaded(String, Option<avatars::Picture>),

    CheckUpdate { manual: bool },
    UpdateChecked { manual: bool, result: Result<Outcome, String> },
    /// Restarts into the installed update, or opens the release page when it could not be installed.
    ApplyUpdate,
    SetAutoUpdate(bool),

    Key(Key, Modifiers, Option<String>),
    EscapeInField,
}

impl App {
    /// Opens `paths` as tabs, or restores the last run when none are given.
    pub fn boot(paths: Vec<PathBuf>, state_file: Option<PathBuf>) -> (Self, Task<Message>) {
        let saved = state_file.as_deref().map(persist::load).unwrap_or_default();
        style::set_theme(&saved.theme);
        style::set_look(style::Look::from_key(&saved.look));
        let restoring = paths.is_empty();
        let paths: Vec<PathBuf> = if restoring {
            let mut tabs: Vec<PathBuf> = saved.tabs.iter().filter(|p| p.is_dir()).cloned().collect();
            if tabs.is_empty()
                && let Ok(cwd) = std::env::current_dir()
                && brio_core::Repo::open(&cwd).is_ok()
            {
                tabs.push(cwd);
            }
            tabs
        } else {
            paths
        };

        let mut app = Self {
            tabs: Vec::new(),
            active: 0,
            next_id: 0,
            tools: brio_session::tools::detect(),
            panes: saved.layout.build(),
            layout: saved.layout,
            window: None,
            maximized: false,
            state_file,
            confirm_close: None,
            error: None,
            theme: style::p().iced_theme(),
            settings: None,
            prompt_editor: text_editor::Content::with_text(effective_prompt(&saved.commit_prompt)),
            commit_prompt: saved.commit_prompt.clone(),
            toasts: Toasts::default(),
            avatars: Avatars::default(),
            auto_update: saved.auto_update,
            update: None,
            checking_update: false,
            update_note: None,
            confirm_restart: false,
            menu: None,
            session_list: false,
            cursor: Cell::new(Point::ORIGIN),
            window_size: Size::new(1440.0, 920.0),
            pinned: saved.pinned.iter().map(|(k, v)| (PathBuf::from(k), v.clone())).collect(),
        };
        let mut tasks: Vec<Task<Message>> = paths.into_iter().map(|p| app.open_tab(p)).collect();
        if restoring {
            app.active = saved.active.min(app.tabs.len().saturating_sub(1));
        }
        tasks.push(window::oldest().map(Message::WindowReady));
        update::cleanup();
        if app.auto_update && update::enabled() {
            let wait = blocking(|| {
                std::thread::sleep(update::FIRST_CHECK);
                Ok(())
            });
            tasks.push(Task::perform(wait, |_: Result<(), String>| Message::CheckUpdate { manual: false }));
        }
        (app, Task::batch(tasks))
    }

    pub fn title(&self) -> String {
        match self.current() {
            Some(ws) => format!("brio-git — {}", ws.name()),
            None => "brio-git".into(),
        }
    }

    pub fn theme(&self) -> Theme {
        self.theme.clone()
    }

    pub fn current(&self) -> Option<&Workspace> {
        self.tabs.get(self.active).map(|t| &t.ws)
    }

    pub fn active_id(&self) -> Option<TabId> {
        self.tabs.get(self.active).map(|t| t.id)
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let keys = event::listen_with(|event, status, _window| match (event, status) {
            (
                iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, text, .. }),
                event::Status::Ignored,
            ) => Some(Message::Key(key, modifiers, text.map(|t| t.to_string()))),
            // A focused text field eats Escape (it only drops focus); popups still close on it.
            (
                iced::Event::Keyboard(keyboard::Event::KeyPressed { key: Key::Named(keyboard::key::Named::Escape), .. }),
                event::Status::Captured,
            ) => Some(Message::EscapeInField),
            _ => None,
        });
        let close = window::close_requests().map(|_| Message::Close);
        // Also animates the "working" dots and lets them fade once the output stops.
        let now = Instant::now();
        let working = self.tabs.iter().any(|t| t.ws.working(now) > 0);
        let tick = if self.toasts.is_empty() && !working {
            Subscription::none()
        } else {
            iced::time::every(Duration::from_millis(250)).map(Message::Tick)
        };
        let resize = window::resize_events().map(|(_, size)| Message::WindowResized(size));
        let ready = matches!(self.update, Some(Outcome::Ready { .. }));
        let updates = if self.auto_update && update::enabled() && !ready {
            iced::time::every(update::CHECK_EVERY).map(|_| Message::CheckUpdate { manual: false })
        } else {
            Subscription::none()
        };
        Subscription::batch([keys, close, resize, tick, updates])
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        let task = self.handle(message);
        self.collect_messages();
        task
    }

    /// Moves errors and notices left by the tabs (and the app) into toasts.
    fn collect_messages(&mut self) {
        let now = Instant::now();
        for (i, tab) in self.tabs.iter_mut().enumerate() {
            let source = (i != self.active).then(|| tab.ws.name());
            if let Some(e) = tab.ws.error.take() {
                self.toasts.push(toasts::Kind::Error, e, source.clone(), now);
            }
            if let Some(n) = tab.ws.notice.take() {
                self.toasts.push(toasts::Kind::Info, n, source, now);
            }
        }
        if let Some(e) = self.error.take() {
            self.toasts.push(toasts::Kind::Error, e, None, now);
        }
    }

    fn handle(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Tab(id, WsMessage::GenerateMessage) => {
                self.forward(id, WsMessage::GenerateWith(effective_prompt(&self.commit_prompt).to_owned()))
            }
            Message::Tab(id, WsMessage::OpenMenu(target)) => {
                let Some(ws) = self.tabs.iter().find(|t| t.id == id).map(|t| &t.ws) else { return Task::none() };
                let Some(repo) = &ws.repo else { return Task::none() };
                let items = menu::build(repo, ws.pinned.as_deref(), &target);
                self.menu = Some(ContextMenu { tab: id, items, position: self.cursor.get(), open_child: None });
                Task::none()
            }
            Message::Tab(id, msg) => {
                let pin = matches!(msg, WsMessage::TogglePin(_));
                let avatars = self.fetch_avatars(&msg);
                let task = Task::batch([avatars, self.forward(id, msg)]);
                if pin {
                    self.save();
                }
                task
            }
            Message::MenuPick(msg) => match self.menu.take() {
                Some(menu) => self.update(Message::Tab(menu.tab, msg)),
                None => Task::none(),
            },
            Message::MenuHover(index) => {
                if let Some(menu) = self.menu.as_mut() {
                    menu.open_child = index;
                }
                Task::none()
            }
            Message::CloseMenu => {
                self.menu = None;
                Task::none()
            }
            Message::ToggleSessionList => {
                self.session_list = !self.session_list;
                Task::none()
            }
            Message::GoToSession(tab, session) => {
                self.session_list = false;
                let Some(i) = self.index_of(tab) else { return Task::none() };
                self.active = i;
                self.confirm_close = None;
                if !self.layout.is_visible(PaneKind::Session) {
                    self.layout.set_visible(PaneKind::Session, true);
                    self.panes = self.layout.build();
                }
                self.save();
                self.forward(tab, WsMessage::SelectSession(session))
            }
            Message::WindowResized(size) => {
                self.window_size = size;
                Task::none()
            }
            Message::SelectTab(id) => {
                if let Some(i) = self.index_of(id) {
                    self.active = i;
                    self.confirm_close = None;
                    self.save();
                }
                Task::none()
            }
            Message::CloseTab(id) => {
                let Some(i) = self.index_of(id) else { return Task::none() };
                if self.tabs[i].ws.session_running() && self.confirm_close != Some(id) {
                    self.confirm_close = Some(id);
                    return Task::none();
                }
                self.confirm_close = None;
                // Dropping the workspace kills its terminal and stops its watcher.
                self.tabs.remove(i);
                if self.active >= i && self.active > 0 {
                    self.active -= 1;
                }
                self.save();
                Task::none()
            }
            Message::PickFolder => Task::perform(
                async {
                    rfd::AsyncFileDialog::new()
                        .set_title("Abrir repositório")
                        .pick_folder()
                        .await
                        .map(|h| h.path().to_path_buf())
                },
                Message::FolderPicked,
            ),
            Message::FolderPicked(Some(path)) => {
                let task = self.open_tab(path);
                self.save();
                task
            }
            Message::FolderPicked(None) => Task::none(),

            Message::PaneResized(pane_grid::ResizeEvent { split, ratio }) => {
                if let Some(panes) = self.panes.as_mut() {
                    panes.resize(split, ratio);
                    self.layout.capture(panes);
                }
                Task::none()
            }
            Message::PaneDragged(pane_grid::DragEvent::Dropped { pane, target }) => {
                if let Some(panes) = self.panes.as_mut() {
                    panes.drop(pane, target);
                    self.layout.capture(panes);
                }
                self.save();
                Task::none()
            }
            Message::PaneDragged(_) => Task::none(),
            Message::ResetLayout => {
                self.layout.reset();
                self.panes = self.layout.build();
                self.save();
                Task::none()
            }
            Message::TogglePane(kind) => {
                let visible = self.layout.is_visible(kind);
                self.layout.set_visible(kind, !visible);
                self.panes = self.layout.build();
                self.save();
                Task::none()
            }
            Message::MaximizePane(pane) => {
                if let Some(panes) = self.panes.as_mut() {
                    panes.maximize(pane);
                }
                Task::none()
            }
            Message::RestorePanes => {
                if let Some(panes) = self.panes.as_mut() {
                    panes.restore();
                }
                Task::none()
            }

            Message::WindowReady(id) => {
                self.window = id;
                self.with_window(|id| window::size(id).map(Message::WindowResized))
            }
            Message::DragWindow => self.with_window(window::drag),
            Message::ResizeWindow(direction) => self.with_window(|id| window::drag_resize(id, direction)),
            Message::Minimize => self.with_window(|id| window::minimize(id, true)),
            Message::ToggleMaximize => {
                self.maximized = !self.maximized;
                self.with_window(window::toggle_maximize)
            }
            Message::Close => {
                self.save();
                iced::exit()
            }

            Message::OpenSettings => {
                self.settings = Some(Section::Appearance);
                Task::none()
            }
            Message::CloseSettings => {
                self.settings = None;
                self.save();
                Task::none()
            }
            Message::ShowSection(section) => {
                self.settings = Some(section);
                Task::none()
            }
            Message::EditPrompt(action) => {
                self.prompt_editor.perform(action);
                let text = self.prompt_editor.text();
                let text = text.trim();
                // Saving the default text verbatim would freeze it; keep "default" as empty.
                self.commit_prompt = if text == brio_session::ai::PROMPT { String::new() } else { text.to_owned() };
                Task::none()
            }
            Message::ResetPrompt => {
                self.commit_prompt.clear();
                self.prompt_editor = text_editor::Content::with_text(brio_session::ai::PROMPT);
                Task::none()
            }
            Message::DismissToast(id) => {
                self.toasts.dismiss(id);
                Task::none()
            }
            Message::Tick(now) => {
                self.toasts.expire(now);
                Task::none()
            }
            Message::SetTheme(name) => {
                if style::set_theme(name) {
                    self.theme = style::p().iced_theme();
                    self.save();
                    let ids: Vec<TabId> = self.tabs.iter().map(|t| t.id).collect();
                    return Task::batch(ids.into_iter().map(|id| self.forward(id, WsMessage::ThemeChanged)));
                }
                Task::none()
            }
            Message::SetLook(look) => {
                if style::look() == look {
                    return Task::none();
                }
                style::set_look(look);
                self.save();
                // Same as a theme change: every cached canvas must be redrawn.
                let ids: Vec<TabId> = self.tabs.iter().map(|t| t.id).collect();
                Task::batch(ids.into_iter().map(|id| self.forward(id, WsMessage::ThemeChanged)))
            }
            Message::AvatarLoaded(email, picture) => {
                let found = picture.is_some();
                self.avatars.loaded(email, picture);
                if found {
                    for tab in &self.tabs {
                        tab.ws.graph_cache.clear();
                    }
                }
                Task::none()
            }

            Message::CheckUpdate { manual } => {
                if !update::enabled() {
                    self.update_note = Some("Builds locais (cargo run, cargo install) não se atualizam.".into());
                    return Task::none();
                }
                if self.checking_update || matches!(self.update, Some(Outcome::Ready { .. })) {
                    return Task::none();
                }
                self.checking_update = true;
                Task::perform(blocking(update::check_and_install), move |result| Message::UpdateChecked { manual, result })
            }
            Message::UpdateChecked { manual, result } => {
                self.checking_update = false;
                let now = Instant::now();
                match result {
                    Ok(found) => {
                        let note = found.message();
                        let kind = if matches!(found, Outcome::Available { .. }) { toasts::Kind::Error } else { toasts::Kind::Info };
                        // Periodic checks find the same release again; tell only once.
                        let new = found != Outcome::UpToDate && self.update.as_ref() != Some(&found);
                        if manual || new {
                            self.toasts.push(kind, note.clone(), None, now);
                        }
                        self.update_note = Some(note);
                        if found != Outcome::UpToDate {
                            self.update = Some(found);
                        }
                    }
                    Err(e) => {
                        if manual {
                            self.toasts.push(toasts::Kind::Error, e.clone(), None, now);
                        }
                        self.update_note = Some(e);
                    }
                }
                Task::none()
            }
            Message::ApplyUpdate => match self.update.clone() {
                Some(Outcome::Ready { launch, .. }) => {
                    let running: usize = self.tabs.iter().map(|t| t.ws.sessions.iter().filter(|s| s.running).count()).sum();
                    if running > 0 && !self.confirm_restart {
                        self.confirm_restart = true;
                        return Task::none();
                    }
                    self.save();
                    match update::relaunch(&launch) {
                        Ok(()) => iced::exit(),
                        Err(e) => {
                            self.confirm_restart = false;
                            self.error = Some(format!("Não foi possível reiniciar: {e}"));
                            Task::none()
                        }
                    }
                }
                Some(Outcome::Available { page, .. }) => {
                    crate::util::open_url(&page);
                    Task::none()
                }
                _ => Task::none(),
            },
            Message::SetAutoUpdate(on) => {
                self.auto_update = on;
                self.save();
                Task::none()
            }

            Message::Key(key, _, _) if self.menu.is_some() => {
                if key == Key::Named(keyboard::key::Named::Escape) {
                    self.menu = None;
                }
                Task::none()
            }
            Message::Key(key, _, _) if self.session_list => {
                if key == Key::Named(keyboard::key::Named::Escape) {
                    self.session_list = false;
                }
                Task::none()
            }
            Message::Key(key, _, _) if self.settings.is_some() => {
                if key == Key::Named(keyboard::key::Named::Escape) {
                    return self.handle(Message::CloseSettings);
                }
                Task::none()
            }
            Message::EscapeInField if self.settings.is_some() => self.handle(Message::CloseSettings),
            Message::EscapeInField => match self.active_id() {
                Some(id) => self.forward(id, WsMessage::CloseOverlays),
                None => Task::none(),
            },
            Message::Key(key, mods, text) => match self.active_id() {
                Some(id) => self.forward(id, WsMessage::Key(key, mods, text)),
                None => Task::none(),
            },
        }
    }

    /// Starts fetching avatars for authors that just showed up.
    fn fetch_avatars(&mut self, msg: &WsMessage) -> Task<Message> {
        let emails: Vec<String> = match msg {
            WsMessage::Loaded(Ok(data)) => self.avatars.request(data.commits.iter().map(|c| c.email.as_str())),
            WsMessage::DetailsLoaded(_, Ok(d)) => self.avatars.request(d.info.iter().map(|i| i.email.as_str())),
            _ => return Task::none(),
        };
        Task::batch(emails.into_iter().map(|email| {
            let key = email.clone();
            Task::perform(blocking(move || Ok(avatars::fetch(&key).and_then(|b| avatars::circular(&b)))), move |r: Result<_, String>| {
                Message::AvatarLoaded(email.clone(), r.ok().flatten())
            })
        }))
    }

    fn forward(&mut self, id: TabId, msg: WsMessage) -> Task<Message> {
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) else { return Task::none() };
        tab.ws.update(msg).map(move |m| Message::Tab(id, m))
    }

    fn open_tab(&mut self, path: PathBuf) -> Task<Message> {
        if let Some(i) = self.tabs.iter().position(|t| same_dir(&t.ws.path, &path)) {
            self.active = i;
            return Task::none();
        }
        let id = self.next_id;
        self.next_id += 1;
        let pinned = self.pinned.iter().find(|(p, _)| same_dir(p, &path)).map(|(_, b)| b.clone());
        let (ws, task) = Workspace::new(path, self.tools.clone(), pinned);
        self.tabs.push(Tab { id, ws });
        self.active = self.tabs.len() - 1;
        task.map(move |m| Message::Tab(id, m))
    }

    fn index_of(&self, id: TabId) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }

    fn with_window(&self, f: impl FnOnce(window::Id) -> Task<Message>) -> Task<Message> {
        self.window.map_or_else(Task::none, f)
    }

    /// Saved pins merged with the current tabs' pins.
    fn pinned_map(&mut self) -> HashMap<String, String> {
        for tab in &self.tabs {
            match &tab.ws.pinned {
                Some(b) => {
                    self.pinned.insert(tab.ws.path.clone(), b.clone());
                }
                None => {
                    self.pinned.remove(&tab.ws.path);
                }
            }
        }
        self.pinned.iter().map(|(k, v)| (k.display().to_string(), v.clone())).collect()
    }

    fn save(&mut self) {
        let pinned = self.pinned_map();
        let Some(file) = &self.state_file else { return };
        let saved = Saved {
            tabs: self.tabs.iter().map(|t| t.ws.path.clone()).collect(),
            active: self.active,
            layout: self.layout.clone(),
            theme: style::p().name.to_owned(),
            look: style::look().key().to_owned(),
            pinned,
            commit_prompt: self.commit_prompt.clone(),
            auto_update: self.auto_update,
        };
        if let Err(e) = persist::save(file, &saved) {
            self.error = Some(format!("Não foi possível salvar o estado: {e}"));
        }
    }
}

/// The prompt in use: the saved one, or the built-in default when none was set.
pub fn effective_prompt(saved: &str) -> &str {
    if saved.trim().is_empty() { brio_session::ai::PROMPT } else { saved }
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}
