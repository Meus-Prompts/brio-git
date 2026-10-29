//! Janela principal: abas, painéis, atalhos globais e preferências.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use egui::{Align2, Key, KeyboardShortcut, Modifiers, Panel, Rect, RichText, Sense, Ui, pos2, vec2};

use crate::git::ops;
use crate::icons::{self, Icon};
use crate::repo_tab::{Action, CenterView, Dialog, RepoTab, Selection};
use crate::settings::Settings;
use crate::theme;
use crate::ui::home::{Home, HomeRequest};
use crate::ui::palette::{Command, Palette};
use crate::ui::toolbar::AppRequest;

fn is_maximized(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.viewport().maximized.unwrap_or(false))
}

fn is_fullscreen(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.viewport().fullscreen.unwrap_or(false))
}

pub fn toggle_maximize(ctx: &egui::Context) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!is_maximized(ctx)));
}

pub fn toggle_fullscreen(ctx: &egui::Context) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!is_fullscreen(ctx)));
}

/// Compositores tiling que não têm "minimizar" e ignoram o pedido do protocolo
/// (verificado no niri 26.04: maximizar/tela cheia funcionam, minimizar não).
pub fn compositor_supports_minimize() -> bool {
    if std::env::var_os("NIRI_SOCKET").is_some() || std::env::var_os("SWAYSOCK").is_some() || std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
        return false;
    }
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_lowercase();
    !["niri", "sway", "river", "hyprland"].iter().any(|d| desktop.split(':').any(|x| x == *d))
}

/// Botões minimizar / maximizar-restaurar / fechar (layout da direita para a esquerda).
fn window_controls(ui: &mut Ui, ctx: &egui::Context) {
    ui.spacing_mut().item_spacing.x = 0.0;
    let maximized = is_maximized(ctx);
    let buttons = [
        (Icon::Close, "Fechar", egui::ViewportCommand::Close),
        (
            if maximized { Icon::Restore } else { Icon::Maximize },
            if maximized { "Restaurar" } else { "Maximizar" },
            egui::ViewportCommand::Maximized(!maximized),
        ),
        (Icon::Minimize, "Minimizar", egui::ViewportCommand::Minimized(true)),
    ];
    for (icon, tip, cmd) in buttons {
        if icon == Icon::Minimize && !compositor_supports_minimize() {
            continue;
        }
        let (rect, resp) = ui.allocate_exact_size(vec2(42.0, theme::TABBAR_HEIGHT), Sense::click());
        let hovered = resp.hovered();
        if hovered {
            let fill = if icon == Icon::Close { theme::p().danger } else { theme::p().toolbar_bg1 };
            ui.painter().rect_filled(rect, 0.0, fill);
        }
        let color = if hovered && icon == Icon::Close { egui::Color32::WHITE } else { theme::p().text };
        icons::paint(ui.painter(), Rect::from_center_size(rect.center(), vec2(14.0, 14.0)), icon, color);
        if resp.on_hover_text(tip).clicked() {
            ctx.send_viewport_cmd(cmd);
        }
    }
}

/// Amostra do tema: fundo + 5 cores de lane.
/// Aba "Temas": um tema fixo, ou um par claro/escuro que segue o sistema.
fn themes_page(ui: &mut Ui, s: &mut Settings) {
    ui.checkbox(&mut s.follow_system_theme, "Seguir o tema do sistema (claro e escuro)")
        .on_hover_text("Usa o tema claro ou o escuro escolhidos abaixo conforme a preferência de aparência do sistema operacional.");
    ui.add_space(8.0);
    let all = theme::themes();
    let (light, dark): (Vec<_>, Vec<_>) = all.iter().partition(|(id, _)| theme::is_light_theme(id));
    if s.follow_system_theme {
        ui.columns(2, |cols| {
            cols[0].label(RichText::new("Tema claro").strong());
            for (id, name) in &light {
                theme_choice(&mut cols[0], id, name, &mut s.theme_light);
            }
            cols[1].label(RichText::new("Tema escuro").strong());
            for (id, name) in &dark {
                theme_choice(&mut cols[1], id, name, &mut s.theme_dark);
            }
        });
    } else {
        ui.columns(2, |cols| {
            cols[0].label(RichText::new("Escuros").strong());
            for (id, name) in &dark {
                theme_choice(&mut cols[0], id, name, &mut s.theme);
            }
            cols[1].label(RichText::new("Claros").strong());
            for (id, name) in &light {
                theme_choice(&mut cols[1], id, name, &mut s.theme);
            }
        });
    }
}

fn theme_choice(ui: &mut Ui, id: &str, name: &str, selected: &mut String) {
    ui.horizontal(|ui| {
        theme_swatch(ui, id);
        if ui.selectable_label(selected == id, name).clicked() {
            *selected = id.to_string();
        }
    });
}

/// Guarda o tamanho de um painel quando o usuário o redimensiona.
fn remember_size(slot: &mut Option<f32>, size: f32) {
    if size > 1.0 && slot.is_none_or(|s| (s - size).abs() > 0.5) {
        *slot = Some(size.round());
    }
}

fn theme_swatch(ui: &mut Ui, id: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(54.0, 14.0), Sense::hover());
    let current = theme::p().id;
    theme::set_current(id);
    let pal = theme::p();
    ui.painter().rect_filled(rect, 3.0, pal.app_bg);
    for i in 0..5 {
        let c = Rect::from_center_size(pos2(rect.left() + 7.0 + i as f32 * 10.0, rect.center().y), vec2(7.0, 7.0));
        ui.painter().circle_filled(c.center(), 3.5, pal.lanes[i * 2]);
    }
    theme::set_current(current);
}

/// Barra de abas do painel de terminais: [>_ zsh ×][>_ Terminal 2 ×][+] ... [⌄]
fn terminal_tabs(ui: &mut Ui, tab: &RepoTab, actions: &mut Vec<Action>) {
    let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::hover());
    ui.painter().rect_filled(bar, 0.0, theme::p().toolbar_bg);
    let mut x = bar.left();
    for (i, t) in tab.terminals.iter().enumerate() {
        let active = i == tab.active_terminal;
        let label = crate::ui::elide(ui, &t.tab_label(), &theme::prop(12.0), 170.0);
        let w = ui.ctx().fonts_mut(|f| f.layout_no_wrap(label.clone(), theme::prop(12.0), theme::p().text).size().x) + 56.0;
        let rect = Rect::from_min_size(pos2(x, bar.top()), vec2(w, bar.height()));
        let resp = ui.interact(rect, egui::Id::unique(("term_tab", i, t.seq)), Sense::click());
        let p = ui.painter();
        if active {
            p.rect_filled(rect, 0.0, theme::p().app_bg);
            p.line_segment([rect.left_top(), rect.right_top()], egui::Stroke::new(2.0, theme::p().primary));
        } else if resp.hovered() {
            p.rect_filled(rect, 0.0, theme::p().toolbar_bg1);
        }
        icons::paint(p, Rect::from_center_size(rect.left_center() + vec2(14.0, 0.0), vec2(12.0, 12.0)), Icon::Terminal, theme::p().text_secondary);
        p.text(rect.left_center() + vec2(26.0, 0.0), Align2::LEFT_CENTER, label, theme::prop(12.0), if active { theme::p().text_bright } else { theme::p().text });
        let x_rect = Rect::from_center_size(rect.right_center() - vec2(13.0, 0.0), vec2(16.0, 16.0));
        let xr = ui.interact(x_rect, egui::Id::unique(("term_close", i, t.seq)), Sense::click());
        if resp.hovered() || active || xr.hovered() {
            icons::paint(ui.painter(), x_rect.shrink(3.0), Icon::Close, if xr.hovered() { theme::p().text_bright } else { theme::p().text_secondary });
        }
        if xr.clicked() {
            actions.push(Action::CloseTerminal(i));
        } else if resp.clicked() {
            actions.push(Action::SelectTerminal(i));
        } else if resp.middle_clicked() {
            actions.push(Action::CloseTerminal(i));
        }
        x += w;
    }
    let plus = Rect::from_min_size(pos2(x + 2.0, bar.top() + 2.0), vec2(24.0, 24.0));
    let pr = ui.interact(plus, egui::Id::unique("term_new"), Sense::click());
    if pr.hovered() {
        ui.painter().rect_filled(plus, 3.0, theme::p().toolbar_bg1);
    }
    icons::paint(ui.painter(), plus.shrink(6.0), Icon::Plus, theme::p().text);
    if pr.on_hover_text("Novo terminal (Ctrl+Shift+`)").clicked() {
        actions.push(Action::NewTerminal);
    }
    let hide = Rect::from_min_size(pos2(bar.right() - 28.0, bar.top() + 2.0), vec2(24.0, 24.0));
    let hr = ui.interact(hide, egui::Id::unique("term_hide"), Sense::click());
    if hr.hovered() {
        ui.painter().rect_filled(hide, 3.0, theme::p().toolbar_bg1);
    }
    icons::paint(ui.painter(), hide.shrink(6.0), Icon::ChevronDown, theme::p().text);
    if hr.on_hover_text("Ocultar painel (os terminais continuam rodando)").clicked() {
        actions.push(Action::ToggleTerminal);
    }
}

/// Sem decorações do sistema, as bordas da janela viram alças de redimensionamento.
fn edge_resize(ctx: &egui::Context) {
    use egui::{CursorIcon, ResizeDirection as D};
    if is_maximized(ctx) || is_fullscreen(ctx) {
        return;
    }
    let screen = ctx.content_rect();
    let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) else { return };
    const EDGE: f32 = 5.0;
    let (l, r) = (pos.x - screen.left() < EDGE, screen.right() - pos.x < EDGE);
    let (t, b) = (pos.y - screen.top() < EDGE, screen.bottom() - pos.y < EDGE);
    let dir = match (l, r, t, b) {
        (true, _, true, _) => Some((D::NorthWest, CursorIcon::ResizeNorthWest)),
        (_, true, true, _) => Some((D::NorthEast, CursorIcon::ResizeNorthEast)),
        (true, _, _, true) => Some((D::SouthWest, CursorIcon::ResizeSouthWest)),
        (_, true, _, true) => Some((D::SouthEast, CursorIcon::ResizeSouthEast)),
        (true, ..) => Some((D::West, CursorIcon::ResizeWest)),
        (_, true, ..) => Some((D::East, CursorIcon::ResizeEast)),
        (_, _, true, _) => Some((D::North, CursorIcon::ResizeNorth)),
        (.., true) => Some((D::South, CursorIcon::ResizeSouth)),
        _ => None,
    };
    if let Some((dir, cursor)) = dir {
        ctx.set_cursor_icon(cursor);
        if ctx.input(|i| i.pointer.primary_pressed()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(dir));
        }
    }
}

pub enum Tab {
    Home(Box<Home>),
    Repo(Box<RepoTab>),
}

pub struct BrioApp {
    settings: Settings,
    tabs: Vec<Tab>,
    active: usize,
    palette: Palette,
    show_left: bool,
    show_right: bool,
    closed: Vec<PathBuf>,
    prefs: Option<Prefs>,
    error: Option<String>,
    last_save: Instant,
    askpass: Option<std::sync::mpsc::Receiver<crate::askpass::Request>>,
    askpass_queue: std::collections::VecDeque<crate::askpass::Request>,
    askpass_input: String,
    /// Só a instância primária grava abas e faz autosave (evita clobber entre janelas).
    primary: bool,
    update_rx: Option<std::sync::mpsc::Receiver<Result<crate::update::Outcome, String>>>,
    update_state: Option<crate::update::Outcome>,
    update_note: Option<String>,
    last_update_check: Option<Instant>,
    /// Tema aplicado ao egui (o fixo, ou o claro/escuro do sistema).
    applied_theme: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PrefsSection {
    General,
    Graph,
    Themes,
    GitProfile,
    AppProfiles,
    Integrations,
    Updates,
}

impl PrefsSection {
    const ALL: [PrefsSection; 7] = [Self::General, Self::Graph, Self::Themes, Self::GitProfile, Self::AppProfiles, Self::Integrations, Self::Updates];

    fn label(self) -> &'static str {
        match self {
            Self::General => "Geral",
            Self::Graph => "Grafo",
            Self::Themes => "Temas",
            Self::GitProfile => "Perfil Git",
            Self::AppProfiles => "Perfis do app",
            Self::Integrations => "Integrações",
            Self::Updates => "Atualizações",
        }
    }
}

struct Prefs {
    section: PrefsSection,
    name: String,
    email: String,
    sign: bool,
    sign_format: String,
    signing_key: String,
}

const SC_PALETTE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::P);
const SC_OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::O);
const SC_NEW_TAB: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::T);
const SC_CLOSE_TAB: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::W);
const SC_REOPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::T);
const SC_FETCH: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::L);
const SC_BRANCH: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::B);
const SC_SEARCH: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::F);
const SC_STAGE_ALL: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::S);
const SC_UNSTAGE_ALL: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::U);
const SC_FOCUS_MSG: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::M);
const SC_COMMIT_ALL: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Enter);
const SC_COMMIT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Enter);
const SC_LEFT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::J);
const SC_RIGHT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::K);
const SC_TERMINAL: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Backtick);
const SC_NEW_TERMINAL: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Backtick);
const SC_UNDO: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);
const SC_REDO: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Y);
const SC_REDO2: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Z);
const SC_ZOOM_IN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Equals);
const SC_ZOOM_OUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Minus);
const SC_ZOOM_RESET: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Num0);
const SC_FILTER: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::ALT), Key::F);

impl BrioApp {
    pub fn new(cc: &eframe::CreationContext<'_>, args: Vec<PathBuf>) -> Self {
        theme::install_fonts(&cc.egui_ctx);
        let settings = Settings::load();
        let applied_theme = settings.effective_theme(cc.egui_ctx.system_theme()).to_string();
        theme::apply(&cc.egui_ctx, &applied_theme);
        crate::avatars::init(&cc.egui_ctx, settings.show_avatars);
        cc.egui_ctx.set_zoom_factor(settings.zoom);
        let mut app = Self {
            tabs: Vec::new(),
            active: 0,
            palette: Palette::default(),
            show_left: true,
            show_right: true,
            closed: Vec::new(),
            prefs: None,
            error: None,
            last_save: Instant::now(),
            askpass: crate::askpass::serve(cc.egui_ctx.clone()).ok(),
            askpass_queue: Default::default(),
            askpass_input: String::new(),
            primary: crate::settings::acquire_primary(),
            update_rx: None,
            update_state: None,
            update_note: None,
            last_update_check: None,
            applied_theme,
            settings,
        };
        let restore: Vec<PathBuf> = if args.is_empty() { app.settings.open_tabs.clone() } else { args };
        for p in restore {
            app.open_repo(p, &cc.egui_ctx);
        }
        if app.tabs.is_empty() {
            app.tabs.push(Tab::Home(Box::default()));
        }
        app.active = app.settings.active_tab.min(app.tabs.len() - 1);
        app
    }

    fn open_repo(&mut self, path: PathBuf, ctx: &egui::Context) {
        let root = git2::Repository::discover(&path).ok().and_then(|r| r.workdir().map(|p| p.to_path_buf())).unwrap_or(path.clone());
        if let Some(i) = self.tabs.iter().position(|t| matches!(t, Tab::Repo(r) if r.path == root)) {
            self.active = i;
            return;
        }
        match RepoTab::open(root.clone(), ctx) {
            Ok(mut tab) => {
                tab.terminal_open = self.settings.terminal_open;
                tab.reload(&self.settings);
                self.settings.touch_recent(&root);
                // Substitui a aba "New Tab" ativa, como no cliente de referência.
                if matches!(self.tabs.get(self.active), Some(Tab::Home(_))) {
                    self.tabs[self.active] = Tab::Repo(Box::new(tab));
                } else {
                    self.tabs.push(Tab::Repo(Box::new(tab)));
                    self.active = self.tabs.len() - 1;
                }
                self.persist_tabs();
            }
            Err(e) => self.error = Some(format!("Não foi possível abrir {}: {e}", path.display())),
        }
    }

    fn close_tab(&mut self, i: usize) {
        if let Tab::Repo(r) = self.tabs.remove(i) {
            self.closed.push(r.path.clone());
        }
        if self.tabs.is_empty() {
            self.tabs.push(Tab::Home(Box::default()));
        }
        self.active = self.active.min(self.tabs.len() - 1);
        self.persist_tabs();
    }

    fn persist_tabs(&mut self) {
        if !self.primary {
            return;
        }
        self.settings.open_tabs = self
            .tabs
            .iter()
            .filter_map(|t| match t {
                Tab::Repo(r) => Some(r.path.clone()),
                _ => None,
            })
            .collect();
        self.settings.active_tab = self.active;
        self.settings.save();
    }

    fn active_repo(&mut self) -> Option<&mut RepoTab> {
        match self.tabs.get_mut(self.active) {
            Some(Tab::Repo(r)) => Some(r),
            _ => None,
        }
    }

    /// Reaplica o tema quando a escolha ou o tema do sistema muda.
    fn sync_theme(&mut self, ctx: &egui::Context) {
        let want = self.settings.effective_theme(ctx.system_theme());
        if want != self.applied_theme {
            self.applied_theme = want.to_string();
            theme::apply(ctx, want);
        }
    }

    /// Guarda tamanho e estado maximizado da janela para a próxima abertura.
    fn remember_window(&mut self, ctx: &egui::Context) {
        let (inner, maximized, fullscreen) = ctx.input(|i| {
            let v = i.viewport();
            (v.inner_rect, v.maximized, v.fullscreen)
        });
        if let Some(m) = maximized {
            self.settings.layout.maximized = m;
        }
        if maximized == Some(true) || fullscreen == Some(true) {
            return;
        }
        if let Some(r) = inner {
            // Rects do egui vêm em pontos (já com o zoom do app); a janela usa pixels lógicos.
            let z = ctx.zoom_factor();
            self.settings.layout.window = Some([(r.width() * z).round(), (r.height() * z).round()]);
        }
    }

    fn zoom(&mut self, ctx: &egui::Context, z: f32) {
        self.settings.zoom = z.clamp(0.6, 2.0);
        ctx.set_zoom_factor(self.settings.zoom);
        self.settings.save();
    }

    fn shortcuts(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let typing = ctx.egui_wants_keyboard_input();
        let pressed = |sc: &KeyboardShortcut| ctx.input_mut(|i| i.consume_shortcut(sc));
        if pressed(&SC_PALETTE) {
            self.palette.toggle();
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F11)) {
            toggle_fullscreen(ctx);
        }
        if pressed(&SC_OPEN)
            && let Some(p) = crate::ui::home::pick_folder()
        {
            self.open_repo(p, ctx);
        }
        if pressed(&SC_NEW_TAB) {
            self.tabs.push(Tab::Home(Box::default()));
            self.active = self.tabs.len() - 1;
        }
        if pressed(&SC_CLOSE_TAB) {
            self.close_tab(self.active);
        }
        if pressed(&SC_REOPEN)
            && let Some(p) = self.closed.pop()
        {
            self.open_repo(p, ctx);
        }
        if pressed(&SC_ZOOM_IN) {
            self.zoom(ctx, self.settings.zoom + 0.1);
        }
        if pressed(&SC_ZOOM_OUT) {
            self.zoom(ctx, self.settings.zoom - 0.1);
        }
        if pressed(&SC_ZOOM_RESET) {
            self.zoom(ctx, 1.0);
        }
        for i in 0..9 {
            let key = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9][i];
            if pressed(&KeyboardShortcut::new(Modifiers::COMMAND, key)) && i < self.tabs.len() {
                self.active = i;
            }
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::Tab)) {
            self.active = (self.active + 1) % self.tabs.len();
        }

        let Some(tab) = self.active_repo() else { return };
        if pressed(&SC_FETCH) {
            actions.push(Action::Fetch);
        }
        if pressed(&SC_BRANCH)
            && let Some(h) = tab.head_oid()
        {
            actions.push(Action::OpenDialog(Dialog::CreateBranch { at: h, name: String::new(), checkout: true }));
        }
        if pressed(&SC_SEARCH) {
            crate::ui::toolbar::toggle_search(tab);
        }
        if pressed(&SC_STAGE_ALL) {
            actions.push(Action::StageAll);
        }
        if pressed(&SC_UNSTAGE_ALL) {
            actions.push(Action::UnstageAll);
        }
        if pressed(&SC_FOCUS_MSG) {
            tab.selection = Selection::Wip;
            tab.focus_message = true;
        }
        if pressed(&SC_COMMIT_ALL) && tab.selection == Selection::Wip {
            actions.push(Action::Commit { stage_all: true });
        }
        if pressed(&SC_COMMIT) && tab.selection == Selection::Wip {
            let staged = tab.snap.as_ref().is_some_and(|s| !s.status.staged.is_empty());
            actions.push(Action::Commit { stage_all: !staged });
        }
        if pressed(&SC_NEW_TERMINAL) {
            actions.push(Action::NewTerminal);
        } else if pressed(&SC_TERMINAL) {
            actions.push(Action::ToggleTerminal);
        }
        if pressed(&SC_FILTER) {
            ctx.memory_mut(|m| m.request_focus(egui::Id::unique(("left_filter", &tab.path))));
        }
        if !typing {
            if pressed(&SC_UNDO) {
                actions.push(Action::Undo);
            }
            if pressed(&SC_REDO) || pressed(&SC_REDO2) {
                actions.push(Action::Redo);
            }
        }
        let toggle_left = pressed(&SC_LEFT);
        let toggle_right = pressed(&SC_RIGHT);
        if toggle_left {
            self.show_left = !self.show_left;
        }
        if toggle_right {
            self.show_right = !self.show_right;
        }
        let Some(tab) = self.active_repo() else { return };

        if ctx.input(|i| i.key_pressed(Key::Escape)) && tab.dialog.is_none() {
            if tab.search.is_some() {
                tab.search = None;
            } else if tab.center.is_some() {
                actions.push(Action::CloseCenter);
            } else if tab.rebase.is_some() {
                actions.push(Action::CancelInteractiveRebase);
            }
        }

        // Navegação no grafo (↑/↓, j/k).
        if typing || tab.dialog.is_some() || tab.rebase.is_some() || ctx.memory(|m| m.focused().is_some()) {
            return;
        }
        let (up, down) = ctx.input(|i| {
            (i.key_pressed(Key::ArrowUp) || i.key_pressed(Key::K), i.key_pressed(Key::ArrowDown) || i.key_pressed(Key::J))
        });
        if (up || down)
            && let Some(snap) = tab.snap.clone()
        {
            let has_wip = !snap.status.is_empty();
            let cur: isize = match &tab.selection {
                Selection::Wip => -1,
                Selection::Commits(v) => v.last().and_then(|o| snap.index_of.get(o)).map(|&i| i as isize).unwrap_or(-1),
            };
            let next = (cur + if down { 1 } else { -1 }).clamp(if has_wip { -1 } else { 0 }, snap.commits.len() as isize - 1);
            if next < 0 {
                actions.push(Action::SelectWip);
            } else {
                let oid = snap.commits[next as usize].id;
                actions.push(Action::SelectCommit { oid, add: false });
                tab.scroll_to_row = Some(next as usize);
            }
        }
        if ctx.input(|i| i.key_pressed(Key::S)) && tab.selection == Selection::Wip
            && let Some(CenterView::File(f)) = &tab.center
                && f.source == crate::git::diff::DiffSource::Unstaged
            {
                actions.push(Action::Stage(vec![f.path.clone()]));
            }
        if ctx.input(|i| i.key_pressed(Key::U)) && tab.selection == Selection::Wip
            && let Some(CenterView::File(f)) = &tab.center
                && f.source == crate::git::diff::DiffSource::Staged
            {
                actions.push(Action::Unstage(vec![f.path.clone()]));
            }
    }

    fn tab_bar(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let mut open_new = false;
        let mut open_prefs = false;
        let mut close: Option<usize> = None;
        let mut activate: Option<usize> = None;
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            if icons::button(ui, Icon::Folder, theme::TABBAR_HEIGHT, "Open Repo (Ctrl+Shift+O)").clicked()
                && let Some(p) = crate::ui::home::pick_folder()
            {
                self.open_repo(p, ctx);
            }
            let avail = ui.available_width() - 60.0;
            let tab_w = (avail / self.tabs.len().max(1) as f32).clamp(70.0, 200.0);
            for (i, t) in self.tabs.iter().enumerate() {
                let (title, busy) = match t {
                    Tab::Home(_) => ("New Tab".to_string(), false),
                    Tab::Repo(r) => (r.name.clone(), r.jobs.busy()),
                };
                let (rect, resp) = ui.allocate_exact_size(vec2(tab_w, theme::TABBAR_HEIGHT), Sense::click());
                let p = ui.painter();
                let active = i == self.active;
                if active {
                    p.rect_filled(rect, 0.0, theme::p().app_bg);
                    p.line_segment([rect.left_top(), rect.right_top()], egui::Stroke::new(2.0, theme::p().primary));
                } else if resp.hovered() {
                    p.rect_filled(rect, 0.0, theme::p().toolbar_bg1);
                }
                p.line_segment([rect.right_top() + vec2(0.0, 8.0), rect.right_bottom() - vec2(0.0, 8.0)], egui::Stroke::new(1.0, theme::p().panel_bg2));
                if matches!(t, Tab::Repo(_)) {
                    icons::paint(p, Rect::from_center_size(rect.left_center() + vec2(14.0, 0.0), vec2(12.0, 12.0)), Icon::Branch, theme::p().text_secondary);
                }
                let text = crate::ui::elide(ui, &title, &theme::prop(13.0), tab_w - 52.0);
                let color = if active { theme::p().text_bright } else { theme::p().text };
                ui.painter().text(rect.left_center() + vec2(26.0, 0.0), Align2::LEFT_CENTER, text, theme::prop(13.0), color);
                if busy {
                    let t = ctx.input(|i| i.time) as f32;
                    let c = rect.right_center() - vec2(30.0, 0.0);
                    ui.painter().circle_filled(c + vec2((t * 6.0).cos(), (t * 6.0).sin()) * 4.0, 2.0, theme::p().primary);
                    ctx.request_repaint_after(Duration::from_millis(50));
                }
                let x_rect = Rect::from_center_size(rect.right_center() - vec2(13.0, 0.0), vec2(16.0, 16.0));
                if resp.hovered() || active {
                    let xr = ui.interact(x_rect, ui.scope_id().with(("close_tab", i)), Sense::click());
                    icons::paint(ui.painter(), x_rect.shrink(3.0), Icon::Close, if xr.hovered() { theme::p().text_bright } else { theme::p().text_secondary });
                    if xr.clicked() {
                        close = Some(i);
                        continue;
                    }
                }
                if resp.clicked() {
                    activate = Some(i);
                }
                if resp.middle_clicked() {
                    close = Some(i);
                }
                if let Tab::Repo(r) = t {
                    let path = r.path.display().to_string();
                    resp.on_hover_text(path);
                }
            }
            if icons::button(ui, Icon::Plus, theme::TABBAR_HEIGHT, "New Tab (Ctrl+T)").clicked() {
                open_new = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                window_controls(ui, ctx);
                ui.add_space(6.0);
                let label = self.settings.profile().map(|p| p.label.clone()).unwrap_or_else(|| "Default Profile".into());
                let resp = ui.add(egui::Button::new(RichText::new(format!("{label}  ▾")).color(theme::p().text)).frame(false));
                egui::Popup::menu(&resp).show(|ui| {
                    ui.set_min_width(220.0);
                    if ui.selectable_label(self.settings.active_profile.is_none(), "Default Profile (gitconfig)").clicked() {
                        self.settings.active_profile = None;
                        self.settings.save();
                        ui.close();
                    }
                    for (i, p) in self.settings.profiles.iter().enumerate() {
                        let text = format!("{} — {}", p.label, p.email);
                        if ui.selectable_label(self.settings.active_profile == Some(i), text).clicked() {
                            self.settings.active_profile = Some(i);
                            self.settings.save();
                            ui.close();
                        }
                    }
                    ui.separator();
                    if ui.button("Gerenciar perfis…").clicked() {
                        open_prefs = true;
                        ui.close();
                    }
                });
                ui.add_space(6.0);
                if icons::button(ui, Icon::Gear, 26.0, "Preferences").clicked() {
                    open_prefs = true;
                }
                // Espaço vazio da barra: arrastar move a janela, duplo clique maximiza.
                let free = ui.available_rect_before_wrap();

                if free.width() > 4.0 {
                    let r = ui.interact(free, egui::Id::unique("titlebar_drag"), Sense::click_and_drag());
                    if r.double_clicked() {
                        toggle_maximize(ctx);
                    } else if r.drag_started_by(egui::PointerButton::Primary) {
                        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                    }
                }
            });
        });
        if let Some(i) = activate {
            self.active = i;
            self.settings.active_tab = i;
        }
        if let Some(i) = close {
            self.close_tab(i);
        }
        if open_new {
            self.tabs.push(Tab::Home(Box::default()));
            self.active = self.tabs.len() - 1;
        }
        if open_prefs {
            self.open_prefs();
        }
    }

    fn open_prefs(&mut self) {
        let dir = self.active_repo().map(|r| r.path.clone()).unwrap_or_else(|| PathBuf::from("."));
        let fmt = ops::config_get(&dir, "gpg.format");
        self.prefs = Some(Prefs {
            section: PrefsSection::General,
            name: ops::config_get(&dir, "user.name"),
            email: ops::config_get(&dir, "user.email"),
            sign: ops::config_get(&dir, "commit.gpgsign") == "true",
            sign_format: if fmt.is_empty() { "openpgp".into() } else { fmt },
            signing_key: ops::config_get(&dir, "user.signingkey"),
        });
    }

    fn preferences(&mut self, ctx: &egui::Context) {
        let Some(p) = &mut self.prefs else { return };
        let mut open = true;
        let mut save_profile = false;
        let mut check_now = false;
        // Sempre centralizada e com tamanho fixo: grupos em abas à esquerda.
        let size = vec2(760.0, 500.0).min(ctx.content_rect().size() - vec2(40.0, 80.0));
        egui::Window::new("Preferences")
            .id(egui::Id::new("prefs_window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .movable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .fixed_size(size)
            .show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(150.0);
                        for sec in PrefsSection::ALL {
                            let selected = p.section == sec;
                            let (rect, resp) = ui.allocate_exact_size(vec2(150.0, 30.0), Sense::click());
                            let fill = if selected { theme::p().selected_row } else if resp.hovered() { theme::p().hover_row } else { egui::Color32::TRANSPARENT };
                            ui.painter().rect_filled(rect, 4.0, fill);
                            let color = if selected { theme::p().text_bright } else { theme::p().text };
                            ui.painter().text(rect.left_center() + vec2(12.0, 0.0), Align2::LEFT_CENTER, sec.label(), theme::prop(13.0), color);
                            resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, true, selected, sec.label()));
                            if resp.clicked() {
                                p.section = sec;
                            }
                        }
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width());
                        ui.heading(p.section.label());
                        ui.add_space(6.0);
                        egui::ScrollArea::vertical().id_salt(("prefs", p.section.label())).auto_shrink([false, false]).show(ui, |ui| match p.section {
                            PrefsSection::General => {
                                let s = &mut self.settings;
                                egui::Grid::new("prefs_general").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                                    ui.label("Autofetch (minutos, 0 = off)");
                                    ui.add(egui::DragValue::new(&mut s.autofetch_minutes).range(0..=60));
                                    ui.end_row();
                                    ui.label("Branch padrão no init");
                                    ui.text_edit_singleline(&mut s.default_branch);
                                    ui.end_row();
                                    ui.label("Formato de data");
                                    ui.text_edit_singleline(&mut s.date_format);
                                    ui.end_row();
                                    ui.label("Pasta padrão de clone");
                                    let mut dir = s.clone_dir.as_ref().map(|d| d.display().to_string()).unwrap_or_default();
                                    if ui.text_edit_singleline(&mut dir).changed() {
                                        s.clone_dir = Some(PathBuf::from(dir));
                                    }
                                    ui.end_row();
                                    ui.label("Zoom");
                                    let mut z = s.zoom;
                                    if ui.add(egui::Slider::new(&mut z, 0.6..=2.0)).changed() {
                                        s.zoom = z;
                                        ctx.set_zoom_factor(z);
                                    }
                                    ui.end_row();
                                });
                                ui.add_space(6.0);
                                ui.checkbox(&mut s.custom_titlebar, "Barra de título própria (sem decorações do sistema) — vale ao reiniciar");
                                ui.add_space(6.0);
                                if ui.button("Restaurar tamanhos dos painéis").on_hover_text("Volta as larguras do painel esquerdo, do detalhe e a altura do terminal ao padrão (vale ao reiniciar).").clicked() {
                                    s.layout = crate::settings::Layout { window: s.layout.window, maximized: s.layout.maximized, ..Default::default() };
                                }
                            }
                            PrefsSection::Graph => {
                                let s = &mut self.settings;
                                egui::Grid::new("prefs_graph").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                                    ui.label("Commits exibidos no grafo");
                                    ui.add(egui::DragValue::new(&mut s.max_commits).range(100..=200_000).speed(100));
                                    ui.end_row();
                                });
                                ui.add_space(6.0);
                                if ui
                                    .checkbox(&mut s.show_avatars, "Fotos dos autores no grafo")
                                    .on_hover_text("Busca a foto pelo e-mail do commit: avatar do GitHub (e-mails noreply), GitLab conectado e Gravatar (envia o hash MD5 do e-mail).")
                                    .changed()
                                {
                                    crate::avatars::set_enabled(s.show_avatars);
                                }
                                ui.checkbox(&mut s.show_stashes, "Mostrar stashes no grafo");
                                ui.checkbox(&mut s.show_body_in_graph, "Mostrar descrição do commit no grafo");
                            }
                            PrefsSection::Themes => themes_page(ui, &mut self.settings),
                            PrefsSection::GitProfile => {
                                ui.label(RichText::new("Grava no ~/.gitconfig (vale para todos os clientes git).").small().color(theme::p().text_secondary));
                                ui.add_space(4.0);
                                egui::Grid::new("prefs_profile").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                                    ui.label("Nome");
                                    ui.text_edit_singleline(&mut p.name);
                                    ui.end_row();
                                    ui.label("Email");
                                    ui.text_edit_singleline(&mut p.email);
                                    ui.end_row();
                                });
                                ui.add_space(10.0);
                                ui.label(RichText::new("Assinatura de commits").strong());
                                ui.checkbox(&mut p.sign, "Assinar commits por padrão (commit.gpgsign)");
                                egui::Grid::new("prefs_sign").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                                    ui.label("Formato");
                                    egui::ComboBox::from_id_salt("sign_format").selected_text(p.sign_format.as_str()).show_ui(ui, |ui| {
                                        for f in ["openpgp", "ssh", "x509"] {
                                            ui.selectable_value(&mut p.sign_format, f.to_string(), f);
                                        }
                                    });
                                    ui.end_row();
                                    ui.label("Chave");
                                    ui.add(egui::TextEdit::singleline(&mut p.signing_key).hint_text(if p.sign_format == "ssh" { "~/.ssh/id_ed25519.pub" } else { "ID da chave GPG" }));
                                    ui.end_row();
                                });
                                ui.add_space(8.0);
                                if ui.button("Salvar perfil e assinatura").clicked() {
                                    save_profile = true;
                                }
                            }
                            PrefsSection::AppProfiles => {
                                ui.label(RichText::new("Identidades usadas nos commits feitos pelo app, sem alterar o gitconfig.").small().color(theme::p().text_secondary));
                                ui.add_space(4.0);
                                let mut remove = None;
                                for (i, prof) in self.settings.profiles.iter_mut().enumerate() {
                                    ui.horizontal(|ui| {
                                        ui.add(egui::TextEdit::singleline(&mut prof.label).hint_text("Trabalho").desired_width(100.0));
                                        ui.add(egui::TextEdit::singleline(&mut prof.name).hint_text("Nome").desired_width(140.0));
                                        ui.add(egui::TextEdit::singleline(&mut prof.email).hint_text("email").desired_width(170.0));
                                        if icons::button(ui, Icon::Close, 18.0, "Remover perfil").clicked() {
                                            remove = Some(i);
                                        }
                                    });
                                }
                                if let Some(i) = remove {
                                    self.settings.profiles.remove(i);
                                    self.settings.active_profile = match self.settings.active_profile {
                                        Some(a) if a == i => None,
                                        Some(a) if a > i => Some(a - 1),
                                        other => other,
                                    };
                                }
                                if ui.button("+ Adicionar perfil").clicked() {
                                    self.settings.profiles.push(crate::settings::Profile { label: format!("Perfil {}", self.settings.profiles.len() + 1), ..Default::default() });
                                }
                            }
                            PrefsSection::Integrations => {
                                ui.label(
                                    RichText::new("github.com, gitlab.com e hosts com \"github\"/\"gitlab\" no nome são detectados sozinhos. Cadastre aqui outros hosts Enterprise/self-hosted.")
                                        .small()
                                        .color(theme::p().text_secondary),
                                );
                                ui.add_space(4.0);
                                let mut remove_host = None;
                                for (i, h) in self.settings.hosting_hosts.iter_mut().enumerate() {
                                    ui.horizontal(|ui| {
                                        ui.add(egui::TextEdit::singleline(&mut h.host).hint_text("git.empresa.com").desired_width(150.0));
                                        egui::ComboBox::from_id_salt(("host_kind", i)).selected_text(h.kind.label()).width(80.0).show_ui(ui, |ui| {
                                            ui.selectable_value(&mut h.kind, crate::hosting::Kind::GitHub, "GitHub");
                                            ui.selectable_value(&mut h.kind, crate::hosting::Kind::GitLab, "GitLab");
                                        });
                                        ui.add(egui::TextEdit::singleline(&mut h.api).hint_text("API (opcional)").desired_width(160.0));
                                        if icons::button(ui, Icon::Close, 18.0, "Remover host").clicked() {
                                            remove_host = Some(i);
                                        }
                                    });
                                }
                                if let Some(i) = remove_host {
                                    self.settings.hosting_hosts.remove(i);
                                }
                                if ui.button("+ Adicionar host").clicked() {
                                    self.settings.hosting_hosts.push(crate::hosting::HostConfig { host: String::new(), kind: crate::hosting::Kind::GitLab, api: String::new() });
                                }
                            }
                            PrefsSection::Updates => {
                                ui.horizontal(|ui| {
                                    let installed = if crate::update::enabled() { format!("v{}", crate::update::VERSION) } else { "dev (build local, não se atualiza)".into() };
                                    ui.label(format!("Versão instalada: {installed}"));
                                    if ui.add_enabled(crate::update::enabled() && self.update_rx.is_none(), egui::Button::new("Verificar agora")).clicked() {
                                        check_now = true;
                                    }
                                });
                                ui.checkbox(&mut self.settings.auto_update, "Atualizar automaticamente (ao abrir e a cada 6 h)");
                                if let Some(note) = &self.update_note {
                                    ui.label(RichText::new(note).small().color(theme::p().text_secondary));
                                }
                            }
                        });
                    });
                });
            });
        self.sync_theme(ctx);
        let Some(p) = &mut self.prefs else { return };
        if save_profile {
            let dir = std::env::current_dir().unwrap_or_default();
            let res = ops::config_set(&dir, "user.name", p.name.trim(), true)
                .and_then(|_| ops::config_set(&dir, "user.email", p.email.trim(), true))
                .and_then(|_| ops::config_set(&dir, "commit.gpgsign", if p.sign { "true" } else { "false" }, true))
                .and_then(|_| ops::config_set(&dir, "gpg.format", &p.sign_format, true))
                .and_then(|_| if p.signing_key.trim().is_empty() { Ok(String::new()) } else { ops::config_set(&dir, "user.signingkey", p.signing_key.trim(), true) });
            if let Err(e) = res {
                self.error = Some(e.to_string());
            }
        }
        if check_now {
            self.check_update(ctx, true);
        }
        if !open {
            self.prefs = None;
            self.settings.save();
            let settings = self.settings.clone();
            for t in &mut self.tabs {
                if let Tab::Repo(r) = t {
                    r.reload(&settings);
                }
            }
        }
    }

    /// Modal de usuário/senha/passphrase pedido pelo git ou ssh.
    fn credentials(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.askpass {
            self.askpass_queue.extend(rx.try_iter());
        }
        let Some(req) = self.askpass_queue.front() else { return };
        let secret = req.is_secret();
        let prompt = req.prompt.clone();
        let mut answer: Option<Option<String>> = None;
        let enter = ctx.input(|i| i.key_pressed(Key::Enter));
        egui::Modal::new(egui::Id::unique("askpass")).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.heading("Autenticação necessária");
            ui.label(RichText::new(&prompt).color(theme::p().text_secondary));
            ui.add_space(6.0);
            let r = ui.add(egui::TextEdit::singleline(&mut self.askpass_input).password(secret).desired_width(f32::INFINITY));
            r.request_focus();
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if crate::ui::accent_button(ui, "OK", theme::p().success, true).clicked() || enter {
                    answer = Some(Some(std::mem::take(&mut self.askpass_input)));
                }
                if ui.button("Cancel").clicked() {
                    answer = Some(None);
                }
            });
            ui.label(RichText::new("A resposta vai direto para o git; o brio-git não a armazena.").small().color(theme::p().text_disabled));
        });
        if let Some(a) = answer {
            self.askpass_input.clear();
            if let Some(req) = self.askpass_queue.pop_front() {
                let _ = req.reply.send(a);
            }
        }
    }

    /// Dispara a verificação de atualização em background.
    fn check_update(&mut self, ctx: &egui::Context, manual: bool) {
        if !crate::update::enabled() || self.update_rx.is_some() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(crate::update::check_and_install());
            ctx.request_repaint();
        });
        self.update_rx = Some(rx);
        self.last_update_check = Some(Instant::now());
        if manual {
            self.update_note = Some("Verificando…".into());
        }
    }

    fn poll_update(&mut self, ctx: &egui::Context) {
        let due = match self.last_update_check {
            None => self.last_save.elapsed() >= crate::update::FIRST_CHECK || cfg!(test),
            Some(t) => t.elapsed() >= crate::update::CHECK_EVERY,
        };
        let ready = matches!(self.update_state, Some(crate::update::Outcome::Ready { .. }));
        if self.settings.auto_update && due && !ready && !cfg!(test) {
            self.check_update(ctx, false);
        }
        if let Some(rx) = &self.update_rx
            && let Ok(result) = rx.try_recv()
        {
            self.update_rx = None;
            match result {
                Ok(outcome) => {
                    self.update_note = Some(outcome.message());
                    if outcome != crate::update::Outcome::UpToDate {
                        self.update_state = Some(outcome);
                    }
                }
                Err(e) => self.update_note = Some(e),
            }
        }
    }

    fn status_bar(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        ui.horizontal_centered(|ui| {
            ui.add_space(4.0);
            if let Some(Tab::Repo(r)) = self.tabs.get(self.active) {
                let n = r.terminals.len();
                let text = if n > 1 { format!("Terminal ({n})") } else { "Terminal".to_string() };
                let color = if r.terminal_open { theme::p().text_bright } else { theme::p().text };
                let resp = ui.add(egui::Button::new(RichText::new(format!("     {text}")).color(color)).frame(r.terminal_open).min_size(vec2(0.0, 20.0)));
                icons::paint(ui.painter(), Rect::from_center_size(resp.rect.left_center() + vec2(12.0, 0.0), vec2(12.0, 12.0)), Icon::Terminal, color);
                if resp.on_hover_text("Mostrar/ocultar terminais (Ctrl+`)").clicked() {
                    actions.push(Action::ToggleTerminal);
                }
                ui.separator();
            }
            if let Some(Tab::Repo(r)) = self.tabs.get(self.active) {
                let net = r.hosting.as_ref().map(|h| h.pending).unwrap_or(0);
                if net > 0 && r.jobs.running.is_empty() {
                    ui.spinner();
                    let host = r.hosting.as_ref().map(|h| h.repo.host.clone()).unwrap_or_default();
                    ui.label(RichText::new(format!("Comunicando com {host}…")).color(theme::p().text));
                } else if let Some((label, started)) = r.jobs.running.first() {
                    ui.spinner();
                    ui.label(RichText::new(format!("{label}… {}s", started.elapsed().as_secs())).color(theme::p().text));
                } else if let Some(n) = r.notices.last() {
                    ui.label(RichText::new(&n.text).color(if n.error { theme::p().danger } else { theme::p().text }));
                } else if let Some(s) = &r.snap {
                    let mut txt = format!("{} commits", s.commits.len());
                    if s.truncated {
                        txt.push_str(&format!(" (limite de {} — ajuste em Preferences)", self.settings.max_commits));
                    }
                    txt.push_str(&format!(" · carregado em {} ms", s.load_ms));
                    ui.label(RichText::new(txt).color(theme::p().text_secondary));
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(8.0);
                let version = if crate::update::enabled() { format!("brio-git v{}", crate::update::VERSION) } else { "brio-git dev".into() };
                ui.label(RichText::new(version).monospace().color(theme::p().text_secondary));
                match self.update_state.clone() {
                    Some(crate::update::Outcome::Ready { version, launch }) => {
                        if crate::ui::accent_button(ui, &format!("Reiniciar para v{version}"), theme::p().success, true).clicked() {
                            self.persist_tabs();
                            if crate::update::relaunch(&launch).is_ok() {
                                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        }
                    }
                    Some(crate::update::Outcome::Available { version, page, .. })
                        if ui.link(format!("v{version} disponível")).clicked() => {
                            crate::ui::open_path(std::ffi::OsStr::new(&page));
                        }
                    _ => {}
                }
                ui.label(RichText::new(format!("{:.0}%", self.settings.zoom * 100.0)).color(theme::p().text));
            });
        });
    }

    fn run_command(&mut self, cmd: Command, ctx: &egui::Context, actions: &mut Vec<Action>) {
        match cmd {
            Command::Repo(a) => actions.push(a),
            Command::NewTab => {
                self.tabs.push(Tab::Home(Box::default()));
                self.active = self.tabs.len() - 1;
            }
            Command::OpenRepo => {
                if let Some(p) = crate::ui::home::pick_folder() {
                    self.open_repo(p, ctx);
                }
            }
            Command::CloseTab => self.close_tab(self.active),
            Command::ToggleLeft => self.show_left = !self.show_left,
            Command::ToggleRight => self.show_right = !self.show_right,
            Command::ToggleTerminal => actions.push(Action::ToggleTerminal),
            Command::Search => {
                if let Some(t) = self.active_repo() {
                    crate::ui::toolbar::toggle_search(t);
                }
            }
            Command::Preferences => self.open_prefs(),
            Command::ZoomIn => self.zoom(ctx, self.settings.zoom + 0.1),
            Command::ZoomOut => self.zoom(ctx, self.settings.zoom - 0.1),
            Command::ZoomReset => self.zoom(ctx, 1.0),
            Command::Minimize => ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
            Command::ToggleMaximize => toggle_maximize(ctx),
            Command::ToggleFullscreen => toggle_fullscreen(ctx),
            Command::SetTheme(id) => {
                // Seguindo o sistema, a escolha vira o tema da variante correspondente.
                match (self.settings.follow_system_theme, theme::is_light_theme(id)) {
                    (true, true) => self.settings.theme_light = id.to_string(),
                    (true, false) => self.settings.theme_dark = id.to_string(),
                    (false, _) => self.settings.theme = id.to_string(),
                }
                self.sync_theme(ctx);
                self.settings.save();
            }
        }
    }

    fn repo_ui(&mut self, ui: &mut Ui, ctx: &egui::Context, actions: &mut Vec<Action>, app_req: &mut Vec<AppRequest>) {
        let recent = self.settings.recent.clone();
        let (show_left, show_right) = (self.show_left, self.show_right);
        let settings = &mut self.settings;
        let Some(Tab::Repo(tab)) = self.tabs.get_mut(self.active) else { return };

        Panel::top("toolbar").exact_size(theme::TOOLBAR_HEIGHT).frame(egui::Frame::new().fill(theme::p().toolbar_bg)).show(ui, |ui| {
            crate::ui::toolbar::show(ui, tab, &recent, actions, app_req);
        });

        // Larguras padrão proporcionais (em janelas estreitas o grafo não some);
        // depois de arrastar, vale o tamanho salvo.
        let width = ui.available_width();
        if show_left {
            let r = Panel::left("left_panel")
                .resizable(true)
                .default_size(settings.layout.left.unwrap_or((width * 0.18).clamp(170.0, 260.0)))
                .size_range(150.0..=500.0)
                .frame(egui::Frame::new().fill(theme::p().panel_bg0))
                .show(ui, |ui| crate::ui::left_panel::show(ui, tab, actions));
            remember_size(&mut settings.layout.left, r.response.rect.width());
        }
        if show_right && tab.rebase.is_none() {
            let r = Panel::right("detail_panel")
                .resizable(true)
                .default_size(settings.layout.right.unwrap_or((width * 0.28).clamp(260.0, 420.0)))
                .size_range(240.0..=800.0)
                .frame(egui::Frame::new().fill(theme::p().panel_bg0))
                .show(ui, |ui| crate::ui::detail_panel::show(ui, tab, actions));
            remember_size(&mut settings.layout.right, r.response.rect.width());
        }

        // Terminais só sob a área central, como no cliente de referência.
        tab.reap_terminals();
        if tab.terminal_open && tab.terminals.is_empty() {
            tab.new_terminal();
        }
        if tab.terminal_open && !tab.terminals.is_empty() {
            let r = Panel::bottom("terminal")
                .resizable(true)
                .default_size(settings.layout.terminal.unwrap_or(260.0))
                .min_size(80.0)
                .frame(egui::Frame::new().fill(theme::p().app_bg))
                .show(ui, |ui| {
                    terminal_tabs(ui, tab, actions);
                    let active = tab.active_terminal.min(tab.terminals.len() - 1);
                    tab.terminals[active].show(ui);
                });
            remember_size(&mut settings.layout.terminal, r.response.rect.height());
        }

        egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::p().app_bg)).show(ui, |ui| {
            if tab.rebase.is_some() {
                crate::ui::rebase_view::show(ui, tab, actions);
                return;
            }
            if tab.search.is_some() {
                crate::ui::toolbar::search_bar(ui, tab, actions);
            }
            match &tab.center {
                Some(CenterView::File(_)) => crate::ui::file_view::show(ui, tab, actions),
                Some(CenterView::Merge(_)) => crate::ui::merge_view::show(ui, tab, actions),
                Some(CenterView::PullRequest(_)) => crate::ui::hosting_view::pr_view(ui, tab, actions),
                Some(CenterView::Issue(_)) => crate::ui::hosting_view::issue_view(ui, tab, actions),
                None => crate::ui::graph_view::show(ui, tab, settings, actions),
            }
        });

        crate::ui::dialogs::show(ctx, tab, actions);
    }
}

impl eframe::App for BrioApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let settings = self.settings.clone();
        for (i, t) in self.tabs.iter_mut().enumerate() {
            if let Tab::Repo(r) = t {
                // Abas em segundo plano só coletam resultados; o watcher age na ativa.
                if i == self.active || r.jobs.busy() {
                    r.tick(&settings);
                }
            }
        }

        self.sync_theme(&ctx);
        self.remember_window(&ctx);
        let mut actions = Vec::new();
        let mut app_req = Vec::new();
        if self.settings.custom_titlebar {
            edge_resize(&ctx);
        }
        self.shortcuts(&ctx, &mut actions);

        Panel::top("tabs").exact_size(theme::TABBAR_HEIGHT).frame(egui::Frame::new().fill(theme::p().toolbar_bg)).show(ui, |ui| self.tab_bar(ui, &ctx));
        Panel::bottom("status").exact_size(theme::STATUSBAR_HEIGHT).frame(egui::Frame::new().fill(theme::p().toolbar_bg)).show(ui, |ui| self.status_bar(ui, &mut actions));

        match self.tabs.get_mut(self.active) {
            Some(Tab::Home(home)) => {
                let mut reqs = Vec::new();
                egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::p().app_bg)).show(ui, |ui| home.show(ui, &settings, &mut reqs));
                for HomeRequest::Open(p) in reqs {
                    self.open_repo(p, &ctx);
                }
            }
            Some(Tab::Repo(_)) => self.repo_ui(ui, &ctx, &mut actions, &mut app_req),
            None => {}
        }

        let has_repo = matches!(self.tabs.get(self.active), Some(Tab::Repo(_)));
        if let Some(cmd) = self.palette.show(&ctx, has_repo) {
            self.run_command(cmd, &ctx, &mut actions);
        }
        self.preferences(&ctx);
        self.credentials(&ctx);
        self.poll_update(&ctx);

        if let Some(e) = self.error.clone() {
            let mut close = false;
            egui::Modal::new(egui::Id::unique("app_error")).show(&ctx, |ui| {
                ui.heading(RichText::new("Erro").color(theme::p().danger));
                ui.label(e);
                if ui.button("OK").clicked() {
                    close = true;
                }
            });
            if close {
                self.error = None;
            }
        }

        for req in app_req {
            match req {
                AppRequest::OpenRepo(p) => self.open_repo(p, &ctx),
            }
        }
        let settings = self.settings.clone();
        let (open_tabs, actions): (Vec<Action>, Vec<Action>) = actions.into_iter().partition(|a| matches!(a, Action::OpenRepoTab(_)));
        for a in open_tabs {
            if let Action::OpenRepoTab(p) = a {
                let already = self.tabs.iter().any(|t| matches!(t, Tab::Repo(r) if r.path == p));
                if !already {
                    // Abre ao lado, sem substituir a aba atual.
                    self.tabs.push(Tab::Home(Box::default()));
                    self.active = self.tabs.len() - 1;
                }
                self.open_repo(p, &ctx);
            }
        }
        if let Some(tab) = self.active_repo() {
            for a in actions {
                tab.handle(a, &settings);
            }
            if tab.terminal_open != settings.terminal_open {
                self.settings.terminal_open = tab.terminal_open;
            }
        }

        if self.primary && self.last_save.elapsed() > Duration::from_secs(10) {
            self.last_save = Instant::now();
            self.settings.save();
        }
        // Mantém autofetch/relógios vivos mesmo sem input.
        ctx.request_repaint_after(Duration::from_secs(5));
        let _ = pos2;
    }

    fn on_exit(&mut self) {
        self.persist_tabs();
        if self.primary {
            crate::settings::release_primary();
        }
        crate::askpass::cleanup();
    }
}

#[cfg(test)]
#[path = "e2e_tests.rs"]
mod e2e_tests;
