//! Top-level state: tabs (one [`Workspace`] per repository), the window and
//! the panel layout shared by all tabs.

use std::cell::Cell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use iced::keyboard::{Key, Modifiers};
use iced::widget::pane_grid;
use iced::{Point, Size, Subscription, Task, Theme, event, keyboard, window};
use brio_session::Tool;

use crate::avatars::{self, Avatars};
use crate::layout::{Layout, PaneKind};
use crate::menu::{self, MenuItem};
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

    /// Open context menu, if any.
    pub menu: Option<ContextMenu>,
    /// Last cursor position in window coordinates, written by an invisible
    /// layer without producing messages (see `view::CursorTracker`).
    pub cursor: Cell<Point>,
    pub window_size: Size,
    /// Pinned branch per repository work tree, as saved.
    pinned: HashMap<PathBuf, String>,
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

    OpenThemes,
    CloseThemes,
    SetTheme(&'static str),
    MenuPick(WsMessage),
    MenuHover(Option<usize>),
    CloseMenu,
    WindowResized(Size),
    AvatarLoaded(String, Option<avatars::Picture>),

    Key(Key, Modifiers, Option<String>),
    EscapeInField,
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
            theme_picker: false,
            avatars: Avatars::default(),
            menu: None,
            cursor: Cell::new(Point::ORIGIN),
            window_size: Size::new(1440.0, 920.0),
            pinned: saved.pinned.iter().map(|(k, v)| (PathBuf::from(k), v.clone())).collect(),
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
        let resize = window::resize_events().map(|(_, size)| Message::WindowResized(size));
        Subscription::batch([keys, close, resize])
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
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

            Message::Key(key, _, _) if self.menu.is_some() => {
                if key == Key::Named(keyboard::key::Named::Escape) {
                    self.menu = None;
                }
                Task::none()
            }
            Message::Key(key, _, _) if self.theme_picker => {
                if key == Key::Named(keyboard::key::Named::Escape) {
                    self.theme_picker = false;
                }
                Task::none()
            }
            Message::EscapeInField => match self.active_id() {
                Some(id) => self.forward(id, WsMessage::CloseOverlays),
                None => Task::none(),
            },
            Message::Key(key, mods, text) => match self.active_id() {
                Some(id) => self.forward(id, WsMessage::Key(key, mods, text)),
                None => Task::none(),
            },
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
            pinned,
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
