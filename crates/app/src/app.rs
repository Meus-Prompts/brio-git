//! Top-level state: tabs (one [`Workspace`] per repository), the window and
//! the panel layout shared by all tabs.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use iced::keyboard::{Key, Modifiers};
use iced::widget::pane_grid;
use iced::{Subscription, Task, Theme, event, keyboard, window};
use lazygit_session::Tool;

use crate::avatars::{self, Avatars};
use crate::layout::{Layout, PaneKind};
use crate::persist::{self, Saved};
use crate::style;
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
    pub theme_picker: bool,
    pub avatars: Avatars,
}

#[derive(Debug, Clone)]
pub enum Message {
    Tab(TabId, WsMessage),
    SelectTab(TabId),
    CloseTab(TabId),
    PickFolder,
    FolderPicked(Option<PathBuf>),

    PaneResized(pane_grid::ResizeEvent),
    TogglePane(PaneKind),
    MaximizePane(pane_grid::Pane),
    RestorePanes,

    WindowReady(Option<window::Id>),
    DragWindow,
    ResizeWindow(window::Direction),
    Minimize,
    ToggleMaximize,
    Close,

    OpenThemes,
    CloseThemes,
    SetTheme(&'static str),
    AvatarLoaded(String, Option<Vec<u8>>),

    Key(Key, Modifiers, Option<String>),
    Tick(Instant),
    DismissError,
}

impl App {
    /// Opens `paths` as tabs, or restores the last run when none are given.
    pub fn boot(paths: Vec<PathBuf>, state_file: Option<PathBuf>) -> (Self, Task<Message>) {
        let saved = state_file.as_deref().map(persist::load).unwrap_or_default();
        style::set_theme(&saved.theme);
        let restoring = paths.is_empty();
        let paths: Vec<PathBuf> = if restoring {
            let mut tabs: Vec<PathBuf> = saved.tabs.iter().filter(|p| p.is_dir()).cloned().collect();
            if tabs.is_empty()
                && let Ok(cwd) = std::env::current_dir()
                && lazygit_core::Repo::open(&cwd).is_ok()
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
            tools: lazygit_session::tools::detect(),
            panes: saved.layout.build(),
            layout: saved.layout,
            window: None,
            maximized: false,
            state_file,
            confirm_close: None,
            error: None,
            theme: style::p().iced_theme(),
            theme_picker: false,
            avatars: Avatars::default(),
        };
        let mut tasks: Vec<Task<Message>> = paths.into_iter().map(|p| app.open_tab(p)).collect();
        if restoring {
            app.active = saved.active.min(app.tabs.len().saturating_sub(1));
        }
        tasks.push(window::oldest().map(Message::WindowReady));
        (app, Task::batch(tasks))
    }

    pub fn title(&self) -> String {
        match self.current() {
            Some(ws) => format!("lazy-git — {}", ws.name()),
            None => "lazy-git".into(),
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
            _ => None,
        });
        let tick = if self.tabs.iter().any(|t| t.ws.session_running()) {
            iced::time::every(Duration::from_secs(1)).map(Message::Tick)
        } else {
            Subscription::none()
        };
        let close = window::close_requests().map(|_| Message::Close);
        Subscription::batch([keys, tick, close])
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Tab(id, msg) => {
                let avatars = self.fetch_avatars(&msg);
                Task::batch([avatars, self.forward(id, msg)])
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
                Task::none()
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

            Message::OpenThemes => {
                self.theme_picker = true;
                Task::none()
            }
            Message::CloseThemes => {
                self.theme_picker = false;
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
            Message::AvatarLoaded(email, bytes) => {
                let found = bytes.is_some();
                self.avatars.loaded(email, bytes);
                if found {
                    for tab in &self.tabs {
                        tab.ws.graph_cache.clear();
                    }
                }
                Task::none()
            }

            Message::Key(key, _, _) if self.theme_picker => {
                if key == Key::Named(keyboard::key::Named::Escape) {
                    self.theme_picker = false;
                }
                Task::none()
            }
            Message::Key(key, mods, text) => match self.active_id() {
                Some(id) => self.forward(id, WsMessage::Key(key, mods, text)),
                None => Task::none(),
            },
            Message::Tick(now) => {
                let running: Vec<TabId> =
                    self.tabs.iter().filter(|t| t.ws.session_running()).map(|t| t.id).collect();
                Task::batch(running.into_iter().map(|id| self.forward(id, WsMessage::Tick(now))))
            }
            Message::DismissError => {
                self.error = None;
                Task::none()
            }
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
            Task::perform(blocking(move || Ok(avatars::fetch(&key))), move |r: Result<_, String>| {
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
        let (ws, task) = Workspace::new(path, self.tools.clone());
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

    fn save(&mut self) {
        let Some(file) = &self.state_file else { return };
        let saved = Saved {
            tabs: self.tabs.iter().map(|t| t.ws.path.clone()).collect(),
            active: self.active,
            layout: self.layout.clone(),
            theme: style::p().name.to_owned(),
        };
        if let Err(e) = persist::save(file, &saved) {
            self.error = Some(format!("Não foi possível salvar o estado: {e}"));
        }
    }
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}
