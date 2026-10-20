//! Layout of the main window: title bar with tabs, panel toolbar and the
//! resizable panel grid of the active tab.

use std::time::Instant;

use iced::mouse::Interaction;
use iced::widget::pane_grid::{self, PaneGrid};
use iced::widget::{
    button, canvas, center, checkbox, column, container, mouse_area, opaque, pick_list, row, scrollable, space, stack, text,
    text_editor, text_input, tooltip,
};
use iced::window::Direction;
use iced::{Alignment, Color, Element, Fill, Font, Length, Padding};
use brio_core::RefKind;

use crate::app::{App, Message, Section, TabId};
use crate::data::RepoData;
use crate::icons;
use crate::layout::PaneKind;
use crate::update::{self, Outcome};
use crate::session::{self, Activity, Session};
use crate::style;
use crate::util;
use crate::widgets::graph::{GraphBase, GraphView};
use crate::widgets::terminal::TerminalView;
use crate::avatars::Avatars;
use crate::workspace::{GRAPH_SCROLL, SESSION_NAME, Selection, Workspace, WsMessage};
use crate::actions::Op;
use crate::menu::Target;
use crate::{view_details, view_file, view_overlay};

const EDGE: f32 = 5.0;

pub fn view(app: &App) -> Element<'_, Message> {
    let mut layout = column![title_bar(app)];
    if let Some(tab) = app.tabs.get(app.active)
        && let Some(b) = conflict_banner(tab.id, &tab.ws)
    {
        layout = layout.push(b);
    }
    layout = layout.push(body(app)).push(status_bar(app));

    let framed: Element<_> = container(layout).style(style::root).width(Fill).height(Fill).into();
    let mut layers = stack![framed];
    if let Some(tab) = app.tabs.get(app.active)
        && let Some(d) = &tab.ws.dialog
    {
        layers = layers.push(view_overlay::dialog(tab.id, d));
    }
    if let Some(tab) = app.tabs.get(app.active)
        && let Some(picker) = &tab.ws.branch_picker
    {
        layers = layers.push(view_overlay::branch_picker(tab.id, &tab.ws, picker));
    }
    if app.session_list {
        layers = layers.push(view_overlay::session_list(app));
    }
    if !app.toasts.is_empty() {
        layers = layers.push(view_overlay::toasts(&app.toasts));
    }
    if let Some(section) = app.settings {
        layers = layers.push(settings(app, section));
    }
    if let Some(menu) = &app.menu {
        layers = layers.push(view_overlay::context_menu(app, menu));
    }
    // Topmost and invisible: remembers where the cursor is for the menus.
    layers = layers.push(view_overlay::cursor_tracker(app));
    let framed: Element<_> = layers.into();
    if app.maximized { framed } else { with_resize_edges(framed) }
}

fn title_bar(app: &App) -> Element<'_, Message> {
    let now = Instant::now();
    let mut tabs = row![].spacing(2).align_y(Alignment::Center);
    for (i, tab) in app.tabs.iter().enumerate() {
        let active = i == app.active;
        let running = tab.ws.session_running();
        let dot = if tab.ws.working(now) > 0 { style::p().added.scale_alpha(session::pulse()) } else { style::p().warn };
        let confirm = app.confirm_close == Some(tab.id);
        let label = row![
            text(if running { "● " } else { "" }).size(11).color(dot),
            text(tab.ws.name()).size(13).color(if active { style::p().text } else { style::p().muted }),
        ];
        let close = button(text(if confirm { "fechar?" } else { "✕" }).size(11))
            .on_press(Message::CloseTab(tab.id))
            .style(if confirm { button::danger } else { button::text })
            .padding([2, 6]);
        tabs = tabs.push(tab_frame(
            row![button(label).on_press(Message::SelectTab(tab.id)).style(button::text).padding([4, 8]), close].align_y(Alignment::Center),
            active,
        ));
    }
    tabs = tabs.push(
        button(text("+").size(16))
            .on_press(Message::PickFolder)
            .style(button::text)
            .padding([2, 10]),
    );

    let drag = mouse_area(container(space()).width(Fill).height(Fill))
        .on_press(Message::DragWindow)
        .on_double_click(Message::ToggleMaximize);

    let window_button = |label: &'static str, msg: Message| {
        button(text(label).size(14).align_x(Alignment::Center))
            .on_press(msg)
            .style(button::text)
            .padding([4, 14])
    };

    let minimal = style::minimal();
    container(
        row![
            logo(),
            tabs,
            drag,
            tip(
                button(icons::icon(icons::SETTINGS, 15.0)).on_press(Message::OpenSettings).style(style::ghost).padding([5, 8]),
                "Configurações",
                tooltip::Position::Bottom,
            ),
            window_button("—", Message::Minimize),
            window_button(if app.maximized { "❐" } else { "▢" }, Message::ToggleMaximize),
            window_button("✕", Message::Close),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .height(if minimal { 34 } else { 38 })
    .padding([0, 10])
    .style(style::title_bar)
    .into()
}

/// The app mark: gradient badge with the graph icon, then "brio" and a quieter "git".
fn logo<'a>() -> Element<'a, Message> {
    let p = style::p();
    let badge = container(icons::icon(icons::GIT_GRAPH, 13.0).color(Color::WHITE))
        .width(22)
        .height(22)
        .center_x(22)
        .center_y(22)
        .style(style::logo);
    let semibold = Font { weight: iced::font::Weight::Semibold, ..Font::default() };
    row![badge, row![text("brio").size(15).font(semibold).color(p.text), text("git").size(15).color(p.muted)]]
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
}

fn toolbar<'a>(id: TabId, ws: &'a Workspace) -> Element<'a, Message> {
    let p = style::p();
    let branch = ws.repo.as_ref().map(|r| match (&r.head.branch, &r.head.id) {
        (Some(b), _) => b.clone(),
        (None, Some(id)) => format!("HEAD destacado em {}", &id[..8.min(id.len())]),
        (None, None) => "repositório vazio".into(),
    });
    let chip = button(
        row![
            icons::icon(icons::GIT_BRANCH, 13.0),
            text(branch.unwrap_or_default()).size(13),
            icons::icon(icons::CHEVRON_DOWN, 12.0),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .on_press_maybe(ws.repo.as_ref().map(|_| Message::Tab(id, WsMessage::OpenBranchPicker)))
    .padding([3, 9])
    .style(style::chip_button);

    let up = ws.repo.as_ref().and_then(|r| r.upstream.as_ref());
    let current = ws.repo.as_ref().and_then(|r| r.current_branch().map(str::to_owned));
    let has_changes = ws.repo.as_ref().is_some_and(|r| !r.changes.is_empty());
    let has_stash = ws.repo.as_ref().is_some_and(|r| !r.stashes.is_empty());
    let idle = ws.busy.is_none();
    let tool = |glyph: char, label: &'static str, count: usize, op: Option<Op>, help: String| {
        let mut content = row![icons::icon(glyph, 14.0), text(label).size(12)].spacing(6).align_y(Alignment::Center);
        if count > 0 {
            content = content.push(container(text(count.to_string()).size(10)).padding([0, 6]).style(style::pill(p.accent)));
        }
        let b = button(content)
            .on_press_maybe(op.filter(|_| idle).map(|op| Message::Tab(id, WsMessage::Request(op))))
            .style(style::ghost)
            .padding([4, 9]);
        tip(b, help, tooltip::Position::Bottom)
    };
    let branch_name = current.clone().unwrap_or_else(|| "HEAD".into());
    let commits = |n: usize| if n == 1 { "1 commit".to_owned() } else { format!("{n} commits") };
    let pull_help = match up {
        Some(u) if u.behind > 0 => format!("Pull: traz {} de {} para {branch_name}.", commits(u.behind), u.name),
        Some(u) => format!("Pull: traz as novidades de {} para {branch_name}.", u.name),
        None => "Pull: indisponível, a branch atual não acompanha nenhuma branch remota.".into(),
    };
    let push_help = match up {
        Some(u) if u.ahead > 0 => format!("Push: envia {} de {branch_name} para {}.", commits(u.ahead), u.name),
        Some(u) => format!("Push: envia {branch_name} para {}.", u.name),
        None => format!("Push: envia {branch_name} para o remoto."),
    };
    let stash_help = if has_changes {
        "Stash: guarda todas as alterações (inclusive arquivos novos) e limpa o working tree."
    } else {
        "Stash: não há alterações para guardar."
    };
    let pop_help = if has_stash {
        "Pop: reaplica o stash mais recente e o remove da lista."
    } else {
        "Pop: não há stash guardado."
    };
    let divider = || container(space()).width(1).height(16).style(style::divider);

    let refresh = button(icons::icon(icons::REFRESH, 13.0))
        .on_press(Message::Tab(id, WsMessage::Refresh))
        .style(style::ghost)
        .padding([5, 7]);
    let refresh = tip(
        refresh,
        "Recarregar: relê branches, commits e status do disco. Já acontece sozinho quando os arquivos mudam; use se algo parecer desatualizado.",
        tooltip::Position::Bottom,
    );
    let chip = tip(chip, "Trocar de branch ou criar uma nova", tooltip::Position::Bottom);

    let mac = cfg!(target_os = "macos");
    let history = |redo: bool| {
        let entry = if redo { ws.history.next_redo() } else { ws.history.next_undo() };
        let glyph = if redo { icons::REDO } else { icons::UNDO };
        let b = button(icons::icon(glyph, 14.0))
            .on_press_maybe(entry.filter(|_| idle).map(|_| Message::Tab(id, if redo { WsMessage::Redo } else { WsMessage::Undo })))
            .style(style::ghost)
            .padding([5, 7]);
        let keys = match (redo, mac) {
            (false, false) => "Ctrl+Z",
            (false, true) => "⌘Z",
            (true, false) => "Ctrl+Shift+Z",
            (true, true) => "⇧⌘Z",
        };
        let help = match entry {
            Some(e) => format!("{}: {} ({keys})", if redo { "Refazer" } else { "Desfazer" }, e.label),
            None if redo => "Nada para refazer".to_owned(),
            None => "Nada para desfazer. Commits, checkouts, branches, tags, merge, rebase, reset e pull podem ser desfeitos; push, fetch e stash não.".to_owned(),
        };
        tip(b, help, tooltip::Position::Bottom)
    };

    container(
        row![
            history(false),
            history(true),
            divider(),
            chip,
            refresh,
            divider(),
            tool(icons::FETCH, "Fetch", 0, Some(Op::Fetch), "Fetch: baixa as novidades de todos os remotos sem mexer nas suas branches.".into()),
            tool(icons::PULL, "Pull", up.map_or(0, |u| u.behind), up.map(|_| Op::Pull), pull_help),
            tool(icons::PUSH, "Push", up.map_or(0, |u| u.ahead), current.map(|branch| Op::Push { branch, force: false }), push_help),
            divider(),
            tool(icons::STASH, "Stash", 0, has_changes.then_some(Op::StashPush), stash_help.into()),
            tool(icons::POP, "Pop", 0, has_stash.then(|| Op::StashPop("stash@{0}".into())), pop_help.into()),
        ]
        .spacing(4)
        .align_y(Alignment::Center),
    )
    .padding([5, 10])
    .width(Fill)
    .align_x(Alignment::Center)
    .style(style::strip)
    .into()
}

/// Bottom bar: one icon per panel on the left (bright when open), status on the right.
fn status_bar(app: &App) -> Element<'_, Message> {
    let p = style::p();
    let mut toggles = row![].spacing(2).align_y(Alignment::Center);
    for kind in PaneKind::ALL {
        let open = app.layout.is_visible(kind);
        let toggle = button(icons::icon(kind.icon(), 14.0))
            .on_press(Message::TogglePane(kind))
            .style(style::pane_toggle(open))
            .padding([3, 7]);
        let action = if open { "fechar" } else { "abrir" };
        toggles = toggles.push(tip(toggle, format!("{} ({action})", kind.title()), tooltip::Position::Top));
    }
    let reset = button(icons::icon(icons::LAYOUT, 13.0))
        .on_press(Message::ResetLayout)
        .style(style::pane_toggle(false))
        .padding([3, 7]);
    toggles = toggles
        .push(container(space()).width(1).height(14).style(style::divider))
        .push(tip(reset, "Restaurar o layout padrão dos painéis", tooltip::Position::Top));

    let ws = app.tabs.get(app.active).map(|t| &t.ws);
    let up = ws.and_then(|ws| ws.repo.as_ref()).and_then(|r| r.upstream.as_ref());
    let status: Element<_> = match ws.map(|ws| (&ws.busy, ws.loading)) {
        Some((Some(b), _)) => row![icons::icon(icons::LOADER, 12.0).color(p.warn), text(b).size(12).color(p.warn)]
            .spacing(6)
            .align_y(Alignment::Center)
            .into(),
        Some((None, true)) => text("atualizando…").size(12).into(),
        _ => space().into(),
    };
    let upstream: Element<_> = match up {
        Some(u) => row![icons::icon(icons::UPSTREAM, 12.0), text(&u.name).size(12)].spacing(6).align_y(Alignment::Center).into(),
        None => space().into(),
    };

    let version = if util::VERSION == "dev" { "dev".to_owned() } else { format!("v{}", util::VERSION) };
    let build = match util::COMMIT {
        Some(c) => format!("brio-git {version} · commit {}", &c[..c.len().min(7)]),
        None => format!("brio-git {version} · build local"),
    };
    let version = tip(text(version).size(11).color(p.muted.scale_alpha(0.8)), build, tooltip::Position::Top);

    let total: usize = app.tabs.iter().map(|t| t.ws.sessions.len()).sum();
    let sessions: Element<_> = if total == 0 {
        space().into()
    } else {
        let now = Instant::now();
        let working: usize = app.tabs.iter().map(|t| t.ws.working(now)).sum();
        let mut label = format!("{total} {}", if total == 1 { "sessão" } else { "sessões" });
        if working > 0 {
            label.push_str(&format!(" · {working} trabalhando"));
        }
        let color = if working > 0 { p.added } else { p.muted };
        let counter = button(row![icons::icon(icons::SPARKLES, 12.0).color(color), text(label).size(12)].spacing(6).align_y(Alignment::Center))
            .on_press(Message::ToggleSessionList)
            .style(style::pane_toggle(app.session_list))
            .padding([2, 7]);
        tip(counter, "Sessões de IA abertas em todos os repositórios", tooltip::Position::Top)
    };

    let update: Element<_> = match &app.update {
        Some(Outcome::Ready { version, .. }) => {
            let running: usize = app.tabs.iter().map(|t| t.ws.sessions.iter().filter(|s| s.running).count()).sum();
            let label = match (app.confirm_restart, running) {
                (false, _) => format!("Reiniciar para v{version}"),
                (true, 1) => "Reiniciar? 1 sessão será encerrada".to_owned(),
                (true, n) => format!("Reiniciar? {n} sessões serão encerradas"),
            };
            let restart = button(row![icons::icon(icons::REFRESH, 12.0), text(label).size(12)].spacing(6).align_y(Alignment::Center))
                .on_press(Message::ApplyUpdate)
                .style(style::soft(if app.confirm_restart { p.removed } else { p.accent }))
                .padding([1, 8]);
            tip(restart, "A versão nova já está instalada; reinicie para usá-la. As abas voltam abertas.", tooltip::Position::Top)
        }
        Some(Outcome::Available { version, reason, .. }) => {
            let open = button(row![icons::icon(icons::PULL, 12.0), text(format!("v{version} disponível")).size(12)].spacing(6).align_y(Alignment::Center))
                .on_press(Message::ApplyUpdate)
                .style(style::soft(p.warn))
                .padding([1, 8]);
            tip(open, format!("Não foi possível atualizar sozinho: {reason}. Clique para baixar."), tooltip::Position::Top)
        }
        _ => space().into(),
    };

    container(row![toggles, space::horizontal(), status, upstream, sessions, update, version].spacing(14).align_y(Alignment::Center))
        .padding([1, 8])
        .height(26)
        .align_y(Alignment::Center)
        .style(style::status_bar)
        .into()
}

/// `content` with a small explanation shown on hover.
fn tip<'a, M: 'a>(content: impl Into<Element<'a, M>>, label: impl Into<String>, position: tooltip::Position) -> Element<'a, M> {
    let label = container(text(label.into()).size(12)).padding([5, 9]).max_width(340).style(style::popover);
    tooltip(content, label, position).gap(6).into()
}

/// A tab's frame: outlined when active, or underlined in the minimal look.
fn tab_frame<'a, M: 'a>(content: impl Into<Element<'a, M>>, active: bool) -> Element<'a, M> {
    let frame = container(content).style(if active { style::tab_active } else { style::tab });
    if !style::minimal() {
        return frame.into();
    }
    // A stack takes its first layer's size, so the line spans just the tab.
    let mark = container(space()).width(Fill).height(2).style(if active { style::underline } else { style::tab });
    stack![frame, container(mark).height(Fill).align_y(iced::alignment::Vertical::Bottom)].into()
}

/// Shown while a merge/rebase/cherry-pick/revert waits for conflict resolution.
fn conflict_banner(id: TabId, ws: &Workspace) -> Option<Element<'_, Message>> {
    let op = ws.repo.as_ref()?.in_progress?;
    let p = style::p();
    let conflicts = ws.repo.as_ref().map_or(0, |r| r.changes.iter().filter(|c| c.kind == brio_ops::ChangeKind::Conflicted).count());
    let msg = if conflicts > 0 {
        format!("{} em andamento: {conflicts} arquivo(s) com conflito. Resolva, faça stage e clique em Continuar.", op.label())
    } else {
        format!("{} em andamento, sem conflitos pendentes.", op.label())
    };
    Some(
        container(
            row![
                text(msg).size(13).color(p.warn).width(Fill),
                button(text("Continuar").size(12)).on_press(Message::Tab(id, WsMessage::Request(Op::Continue(op)))).style(button::primary),
                button(text("Abortar").size(12)).on_press(Message::Tab(id, WsMessage::Request(Op::Abort(op)))).style(button::danger),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .padding([6, 12])
        .style(move |_: &iced::Theme| container::Style {
            background: Some(p.warn.scale_alpha(0.12).into()),
            border: iced::Border { color: p.warn, width: 1.0, radius: 0.0.into() },
            ..Default::default()
        })
        .into(),
    )
}

fn body(app: &App) -> Element<'_, Message> {
    let Some(tab) = app.tabs.get(app.active) else {
        return center(
            column![
                text("Nenhum repositório aberto").size(18),
                button("Abrir pasta…").on_press(Message::PickFolder).style(button::primary),
            ]
            .spacing(14)
            .align_x(Alignment::Center),
        )
        .into();
    };
    let (id, ws) = (tab.id, &tab.ws);

    let Some(repo) = &ws.repo else {
        let msg = if ws.loading { "Carregando…" } else { "Não foi possível abrir este repositório." };
        return center(text(msg).size(16).color(style::p().muted)).into();
    };

    let Some(panes) = &app.panes else {
        return center(text("Todos os painéis estão fechados. Reabra pelos ícones do rodapé.").color(style::p().muted)).into();
    };

    let grid = PaneGrid::new(panes, move |pane, kind, maximized| {
        let content: Element<'_, WsMessage> = match kind {
            PaneKind::Sidebar => sidebar(ws, repo),
            PaneKind::Reflog => reflog(ws, repo),
            PaneKind::Graph => match &ws.file_view {
                Some(fv) => view_file::file_viewer(ws, fv),
                None => graph(ws, repo, &app.avatars),
            },
            PaneKind::Details => view_details::details(ws, repo, &app.avatars),
            PaneKind::Session => session_panel(ws),
        };
        let toggle_max = if maximized { Message::RestorePanes } else { Message::MaximizePane(pane) };
        let minimal = style::minimal();
        let control = |glyph: char, msg: Message| button(icons::icon(glyph, 12.0)).on_press(msg).style(style::ghost).padding([3, 5]);
        let controls = row![
            control(if maximized { icons::RESTORE } else { icons::MAXIMIZE }, toggle_max),
            control(icons::CLOSE, Message::TogglePane(*kind)),
        ]
        .spacing(2);
        let grip = || icons::icon(icons::GRIP, 11.0).color(style::p().muted.scale_alpha(0.6));
        let heading: Element<'_, Message> = if minimal {
            // Small caps-like label; the grid shows the controls on hover, and
            // the drag handle shows up in the room kept for it on the left.
            let label = row![
                space().width(11),
                icons::icon(kind.icon(), 11.0),
                text(kind.title().to_uppercase()).size(11),
            ]
            .spacing(6)
            .align_y(Alignment::Center);
            // Not `Fill`: the title bar hides a title that doesn't fit beside the controls.
            let handle = container(grip()).height(Fill).align_y(iced::alignment::Vertical::Center);
            iced::widget::hover(label, handle)
        } else {
            row![grip(), icons::icon(kind.icon(), 12.0), text(kind.title()).size(12)]
                .spacing(7)
                .align_y(Alignment::Center)
                .into()
        };
        let title = pane_grid::TitleBar::new(heading)
            .controls(pane_grid::Controls::new(controls))
            .padding(if minimal { [5, 12] } else { [3, 8] })
            .style(style::pane_title);
        let title = if minimal { title } else { title.always_show_controls() };
        let mut body: Element<'_, Message> = content.map(move |m| Message::Tab(id, m));
        if *kind == PaneKind::Graph {
            // The git actions live at the top of the graph panel.
            let rule = container(space()).width(Fill).height(1).style(style::hairline);
            body = column![toolbar(id, ws), rule, body].into();
        }
        pane_grid::Content::new(body)
            .title_bar(title)
            .style(style::panel)
    })
    .spacing(if style::minimal() { 1 } else { 4 })
    .on_resize(8, Message::PaneResized)
    .on_drag(Message::PaneDragged);

    if style::minimal() {
        // Panels sit edge to edge; the 1px gaps show this hairline color.
        return container(grid).width(Fill).height(Fill).style(style::hairline).into();
    }
    container(grid).padding(4).width(Fill).height(Fill).into()
}

/// Invisible strips around the window that start an OS resize when dragged.
fn with_resize_edges(content: Element<'_, Message>) -> Element<'_, Message> {
    let edge = |direction: Direction, cursor: Interaction, width: Length, height: Length| {
        mouse_area(space().width(width).height(height))
            .on_press(Message::ResizeWindow(direction))
            .interaction(cursor)
    };
    let e = Length::Fixed(EDGE);
    column![
        row![
            edge(Direction::NorthWest, Interaction::ResizingDiagonallyDown, e, e),
            edge(Direction::North, Interaction::ResizingVertically, Fill, e),
            edge(Direction::NorthEast, Interaction::ResizingDiagonallyUp, e, e),
        ],
        row![
            edge(Direction::West, Interaction::ResizingHorizontally, e, Fill),
            content,
            edge(Direction::East, Interaction::ResizingHorizontally, e, Fill),
        ]
        .height(Fill),
        row![
            edge(Direction::SouthWest, Interaction::ResizingDiagonallyUp, e, e),
            edge(Direction::South, Interaction::ResizingVertically, Fill, e),
            edge(Direction::SouthEast, Interaction::ResizingDiagonallyDown, e, e),
        ],
    ]
    .into()
}

fn sidebar<'a>(ws: &'a Workspace, repo: &'a RepoData) -> Element<'a, WsMessage> {
    let mut list = column![].spacing(2);
    for (kind, title) in [
        (RefKind::LocalBranch, "BRANCHES"),
        (RefKind::RemoteBranch, "REMOTOS"),
        (RefKind::Tag, "TAGS"),
    ] {
        let refs: Vec<_> = repo.refs.iter().filter(|r| r.kind == kind).collect();
        if refs.is_empty() {
            continue;
        }
        let collapsed = ws.collapsed.contains(&kind);
        list = list.push(
            button(text(format!("{} {title}  ({})", if collapsed { "▸" } else { "▾" }, refs.len())).size(11).color(section_color()))
                .on_press(WsMessage::ToggleSection(kind))
                .style(button::text)
                .padding([3, 2])
                .width(Fill),
        );
        if collapsed {
            continue;
        }
        for r in refs {
            let is_head = kind == RefKind::LocalBranch && repo.head.branch.as_deref() == Some(r.name.as_str());
            let selected = ws.selection == Selection::Commit(r.target.clone());
            let label = text(if is_head { format!("✔ {}", r.name) } else { r.name.clone() })
                .size(13)
                .font(if is_head { Font { weight: iced::font::Weight::Bold, ..Font::default() } } else { Font::default() })
                .color(if is_head { style::p().accent } else { style::p().text });
            let mut label = row![label].spacing(6).align_y(Alignment::Center);
            if repo.pinned.is_some_and(|i| std::ptr::eq(&repo.refs[i], r)) {
                label = label.push(tip(icons::icon(icons::PIN, 11.0).color(style::p().muted), "Fixada à esquerda no grafo", tooltip::Position::Right));
            }
            let item = button(label)
                .on_press(WsMessage::JumpToCommit(r.target.clone()))
                .padding([3, 14])
                .width(Fill);
            let item = if is_head { item.style(current_branch) } else { item.style(style::selectable(selected)) };
            let target = match kind {
                RefKind::Tag => Target::Tag(r.name.clone()),
                RefKind::LocalBranch => Target::Branch { name: r.name.clone(), remote: false },
                RefKind::RemoteBranch => Target::Branch { name: r.name.clone(), remote: true },
            };
            list = list.push(mouse_area(item).on_right_press(WsMessage::OpenMenu(target)));
        }
        list = list.push(space().height(8));
    }

    if !repo.stashes.is_empty() {
        let collapsed = ws.collapsed_stash;
        list = list.push(
            button(text(format!("{} STASHES  ({})", if collapsed { "▸" } else { "▾" }, repo.stashes.len())).size(11).color(section_color()))
                .on_press(WsMessage::ToggleStashes)
                .style(button::text)
                .padding([3, 2])
                .width(Fill),
        );
        if !collapsed {
            for st in &repo.stashes {
                let label = column![
                    text(st.message.trim_start_matches("On ").to_owned()).size(12),
                    text(format!("{} · {}", st.name, util::relative_time(st.time, util::now_secs()))).size(11).color(style::p().muted),
                ];
                let item = button(label)
                    .on_press(WsMessage::OpenMenu(Target::Stash(st.name.clone())))
                    .style(button::text)
                    .padding([3, 14])
                    .width(Fill);
                list = list.push(mouse_area(item).on_right_press(WsMessage::OpenMenu(Target::Stash(st.name.clone()))));
            }
        }
    }

    scrollable(list.padding(8)).height(Fill).into()
}

/// Recent movements of HEAD; a click shows the commit, right-click opens its menu.
fn reflog<'a>(ws: &'a Workspace, repo: &'a RepoData) -> Element<'a, WsMessage> {
    let p = style::p();
    if repo.reflog.is_empty() {
        return center(text("Reflog vazio.").size(13).color(p.muted)).into();
    }
    let now = util::now_secs();
    let mut list = column![].spacing(1);
    for entry in &repo.reflog {
        let selected = ws.selection == Selection::Commit(entry.id.clone());
        let kind = entry.action.split([' ', '(']).next().unwrap_or_default();
        let color = match kind {
            "commit" | "merge" | "cherry-pick" => p.added,
            "checkout" | "switch" => p.accent,
            "reset" => p.removed,
            "rebase" | "pull" => p.purple,
            _ => p.muted,
        };
        let tag = container(text(entry.action.clone()).size(10)).padding([1, 6]).style(style::pill(color));
        let mut head = row![].spacing(6).align_y(Alignment::Center);
        if !style::minimal() {
            head = head.push(text(entry.selector.clone()).size(11).font(Font::MONOSPACE).color(p.muted));
        }
        let head = head.extend([
            text(crate::actions::short(&entry.id).to_owned()).size(11).font(Font::MONOSPACE).color(p.accent).into(),
            tag.into(),
            space::horizontal().into(),
            text(util::relative_time(entry.time, now)).size(11).color(p.muted).into(),
        ]);
        let message = if entry.message.is_empty() { entry.action.clone() } else { entry.message.clone() };
        let item = button(column![head, text(message).size(12).wrapping(text::Wrapping::None)].spacing(3))
            .on_press(WsMessage::SelectReflog(entry.id.clone()))
            .style(style::selectable(selected))
            .padding([5, 8])
            .width(Fill);
        list = list.push(mouse_area(item).on_right_press(WsMessage::OpenMenu(Target::Commit(entry.id.clone()))));
    }
    scrollable(list.padding(6)).height(Fill).into()
}

fn graph<'a>(ws: &'a Workspace, repo: &'a RepoData, avatars: &'a Avatars) -> Element<'a, WsMessage> {
    let selected = match &ws.selection {
        Selection::Commit(id) => repo.commit_index(id),
        _ => None,
    };
    let view = GraphView {
        data: repo,
        avatars,
        selected,
        worktree_selected: ws.selection == Selection::Worktree,
        cache: &ws.graph_cache,
        now: util::now_secs(),
    };
    let height = view.height();
    // Two canvases so popups get their own layer; see `widgets::graph`.
    let layers = stack![canvas(GraphBase(view)).width(Fill).height(height), canvas(view).width(Fill).height(height)];
    scrollable(layers).id(GRAPH_SCROLL).height(Fill).into()
}

fn session_panel(ws: &Workspace) -> Element<'_, WsMessage> {
    let names: Vec<String> = ws.tools.iter().map(|t| t.name.clone()).collect();
    let p = style::p();
    let labeled = |glyph: char, label: &'static str| row![icons::icon(glyph, 12.0), text(label).size(13)].spacing(7).align_y(Alignment::Center);
    let tool = pick_list(names, ws.tool.clone(), WsMessage::ToolSelected).text_size(13).padding([5, 10]).style(style::quiet_pick_list);
    let new = |label: &'static str| {
        button(labeled(icons::PLAY, label)).on_press_maybe(ws.repo.as_ref().map(|_| WsMessage::NewSession)).style(style::soft(p.accent)).padding([5, 12])
    };

    let Some(current) = ws.session() else {
        let bar = row![
            tool,
            new("Iniciar sessão"),
            text("A IA roda no diretório e na branch atuais.").size(12).color(p.muted).width(Fill),
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        let terminal = canvas(TerminalView {
            snapshot: None,
            cache: &ws.idle_cache,
            focused: false,
            size: ws.term_size,
            placeholder: "Escolha uma IA (ou \"terminal\") e clique em Iniciar sessão.",
        })
        .width(Fill)
        .height(Fill);
        return column![bar, terminal].spacing(8).padding(8).into();
    };

    let now = Instant::now();
    let working = ws.working(now);
    let mut tabs = row![].spacing(4).align_y(Alignment::Center);
    for s in &ws.sessions {
        tabs = tabs.push(session_tab(ws, s, now, working));
    }

    let icon_button = |glyph: char, color: Color, msg: Option<WsMessage>| {
        button(icons::icon(glyph, 13.0)).on_press_maybe(msg).style(style::soft(color)).padding([6, 9])
    };
    let start = tip(
        icon_button(icons::PLAY, p.accent, ws.repo.as_ref().map(|_| WsMessage::NewSession)),
        "Nova sessão: abre outra aba com a IA escolhida, no diretório e na branch atuais",
        tooltip::Position::Bottom,
    );
    let stop = tip(
        icon_button(icons::STOP, p.removed, current.running.then_some(WsMessage::StopSession(current.id))),
        if current.running {
            format!("Parar: encerra {}; a saída continua visível para copiar", current.name)
        } else {
            "Sessão já encerrada".to_owned()
        },
        tooltip::Position::Bottom,
    );
    let bar = row![
        scrollable(tabs).direction(scrollable::Direction::Horizontal(scrollable::Scrollbar::new().width(3).scroller_width(3))).width(Fill),
        tip(tool, "IA usada nas próximas sessões", tooltip::Position::Bottom),
        row![start, stop].spacing(4),
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    let branch = ws.repo.as_ref().and_then(|r| r.head.branch.clone()).unwrap_or_else(|| "HEAD".into());
    let status = match current.activity(now) {
        Activity::Working => format!("{} trabalhando em ⎇ {branch}", current.tool),
        Activity::Waiting => format!("{} aguardando em ⎇ {branch} — clique no terminal para digitar", current.tool),
        Activity::Exited => format!(
            "{} encerrado{}",
            current.tool,
            current.exit_code.map(|c| format!(" (código {c})")).unwrap_or_default()
        ),
    };
    let info = text(status).size(12).color(if style::minimal() { p.faint() } else { p.muted });

    let terminal = canvas(TerminalView {
        snapshot: current.snapshot.as_ref(),
        cache: &current.cache,
        focused: ws.term_focused,
        size: ws.term_size,
        placeholder: "Iniciando…",
    })
    .width(Fill)
    .height(Fill);

    column![bar, info, terminal].spacing(6).padding(8).into()
}

/// One session in the panel's tab bar: state dot, name (double click renames) and ✕.
fn session_tab<'a>(ws: &'a Workspace, s: &'a Session, now: Instant, working: usize) -> Element<'a, WsMessage> {
    let p = style::p();
    let active = ws.active_session == Some(s.id);
    let activity = s.activity(now);
    let dot = match activity {
        Activity::Working => text("●").color(p.added.scale_alpha(session::pulse())),
        Activity::Waiting => text("●").color(p.warn),
        Activity::Exited => text("○").color(p.muted),
    }
    .size(11);

    let name: Element<'a, WsMessage> = match &ws.renaming {
        Some((id, draft)) if *id == s.id => text_input("Nome da sessão", draft)
            .id(SESSION_NAME)
            .on_input(WsMessage::SessionNameInput)
            .on_submit(WsMessage::SessionNameSubmit)
            .size(13)
            .padding([1, 4])
            .width(160)
            .into(),
        _ => mouse_area(text(s.name.clone()).size(13).color(if active { p.text } else { p.muted }))
            .on_press(WsMessage::SelectSession(s.id))
            .on_double_click(WsMessage::RenameSession(s.id))
            .interaction(Interaction::Pointer)
            .into(),
    };

    let mut label = row![dot, name].spacing(6).align_y(Alignment::Center);
    // Two AIs printing at once in the same folder may be editing the same files.
    if activity == Activity::Working && working > 1 {
        let warning = container(text("Outra sessão também está trabalhando nesta pasta.").size(12)).padding([5, 9]).style(style::popover);
        label = label.push(tooltip(icons::icon(icons::ALERT, 11.0).color(p.warn), warning, tooltip::Position::Bottom).gap(6));
    }

    let confirm = ws.confirm_close_session == Some(s.id);
    let close = button(text(if confirm { "fechar?" } else { "✕" }).size(11))
        .on_press(WsMessage::CloseSession(s.id))
        .style(if confirm { button::danger } else { button::text })
        .padding([1, 5]);

    let tab = row![mouse_area(container(label).padding([4, 8])).on_press(WsMessage::SelectSession(s.id)), close]
        .align_y(Alignment::Center);
    tab_frame(tab, active)
}

/// Settings modal: a section list on the left, the section on the right.
fn settings(app: &App, section: Section) -> Element<'_, Message> {
    let p = style::p();
    let entry = |s: Section, glyph: char, label: &'static str| {
        let selected = s == section;
        button(row![icons::icon(glyph, 14.0), text(label).size(13)].spacing(8).align_y(Alignment::Center))
            .on_press(Message::ShowSection(s))
            .style(move |t: &iced::Theme, st| style::list_item(t, st, selected))
            .padding([7, 10])
            .width(Fill)
    };
    let nav = column![
        entry(Section::Appearance, icons::PALETTE, "Aparência"),
        entry(Section::CommitAi, icons::SPARKLES, "Commit com IA"),
        entry(Section::Updates, icons::REFRESH, "Atualizações"),
    ]
    .spacing(2)
    .width(180);

    let content: Element<_> = match section {
        Section::Appearance => {
            let current = p.name;
            let mut grid = column![].spacing(12);
            for chunk in style::THEMES.chunks(2) {
                let mut line = row![].spacing(12);
                for t in chunk {
                    line = line.push(theme_card(t, t.name == current));
                }
                if chunk.len() == 1 {
                    line = line.push(space().width(Fill));
                }
                grid = grid.push(line);
            }
            // Thin bar against the modal edge, beside (not over) the cards.
            let bar = scrollable::Scrollbar::new().width(6).scroller_width(6).margin(0).spacing(10);
            let mut looks = row![].spacing(8);
            for look in style::Look::ALL {
                looks = looks.push(
                    button(text(look.label()).size(13))
                        .on_press(Message::SetLook(look))
                        .style(style::toggle(style::look() == look))
                        .padding([5, 14]),
                );
            }
            column![
                text("Estilo").size(15),
                looks,
                space().height(6),
                text("Tema").size(15),
                scrollable(grid).direction(scrollable::Direction::Vertical(bar)).height(Fill),
            ]
            .spacing(12)
            .into()
        }
        Section::CommitAi => {
            let custom = !app.commit_prompt.is_empty();
            let reset = button(row![icons::icon(icons::RESET, 12.0), text("Restaurar padrão").size(12)].spacing(6).align_y(Alignment::Center))
                .on_press_maybe(custom.then_some(Message::ResetPrompt))
                .style(style::ghost)
                .padding([4, 8]);
            column![
                row![text("Prompt da mensagem de commit").size(15), space::horizontal(), reset].align_y(Alignment::Center),
                text("Instrução enviada à IA escolhida na Sessão de IA ao clicar em gerar mensagem. O diff em stage é anexado depois dela automaticamente.")
                    .size(12)
                    .color(p.muted),
                text_editor(&app.prompt_editor)
                    .on_action(Message::EditPrompt)
                    .size(13)
                    .padding(10)
                    .height(Fill),
                text(if custom { "Prompt personalizado, salvo para todas as abas." } else { "Usando o prompt padrão." })
                    .size(11)
                    .color(p.muted),
            ]
            .spacing(10)
            .padding(Padding { right: 10.0, ..Padding::ZERO })
            .into()
        }
        Section::Updates => {
            let installed = if update::enabled() { format!("v{}", util::VERSION) } else { "dev (build local)".into() };
            let check = button(row![icons::icon(icons::REFRESH, 12.0), text(if app.checking_update { "Procurando…" } else { "Procurar agora" }).size(13)].spacing(7).align_y(Alignment::Center))
                .on_press_maybe((update::enabled() && !app.checking_update).then_some(Message::CheckUpdate { manual: true }))
                .style(style::soft(p.accent))
                .padding([5, 12]);
            let note = match (&app.update_note, update::enabled()) {
                (Some(n), _) => n.clone(),
                (None, false) => "Builds locais (cargo run, cargo install) não se atualizam: use os arquivos de Releases no GitHub.".into(),
                (None, true) => String::new(),
            };
            column![
                text("Atualizações").size(15),
                text(format!("Versão instalada: {installed}")).size(13),
                checkbox(app.auto_update).label("Atualizar automaticamente").on_toggle(Message::SetAutoUpdate).size(14).text_size(13),
                text("Ao abrir e a cada 6 horas, o brio-git procura uma versão nova no GitHub, baixa e instala em segundo plano. Ela passa a valer quando você reinicia o app.")
                    .size(12)
                    .color(p.muted),
                check,
                text(note).size(12).color(p.muted),
            ]
            .spacing(12)
            .padding(Padding { right: 10.0, ..Padding::ZERO })
            .into()
        }
    };

    let dialog = container(
        column![
            row![
                icons::icon(icons::SETTINGS, 16.0),
                text("Configurações").size(18).width(Fill),
                button(icons::icon(icons::CLOSE, 14.0)).on_press(Message::CloseSettings).style(style::ghost).padding([4, 6]),
            ]
            .spacing(10)
            .padding(Padding { right: 10.0, ..Padding::ZERO })
            .align_y(Alignment::Center),
            container(space()).width(Fill).height(1).style(style::hairline),
            row![nav, container(space()).width(1).height(Fill).style(style::hairline), content].spacing(16).height(Fill),
        ]
        .spacing(14),
    )
    // Narrow on the right so the theme list scrollbar sits near the edge.
    .padding(Padding { top: 18.0, right: 8.0, bottom: 18.0, left: 18.0 })
    .width(760)
    .height(520)
    .style(style::modal);

    let backdrop = mouse_area(container(space()).width(Fill).height(Fill).style(style::backdrop))
        .on_press(Message::CloseSettings);
    stack![backdrop, center(opaque(dialog))].into()
}

fn theme_card<'a>(t: &'static style::Palette, selected: bool) -> Element<'a, Message> {
    let bar = |width: f32, color: Color| container(space()).width(width).height(4).style(style::fill(color, 2.0));
    let preview = container(
        row![
            container(space()).width(26).height(Fill).style(style::fill(t.title, 0.0)),
            column![
                bar(72.0, t.text),
                bar(52.0, t.muted),
                bar(62.0, t.muted),
                space().height(4),
                bar(18.0, t.accent),
            ]
            .spacing(5)
            .padding([10, 12]),
        ],
    )
    .width(Fill)
    .height(80)
    .clip(true)
    .style(move |_: &iced::Theme| container::Style {
        background: Some(t.bg.into()),
        border: iced::Border { color: t.border, width: 1.0, radius: 4.0.into() },
        ..Default::default()
    });

    let p = style::p();
    let label = text(t.name).size(13).color(if selected { p.accent } else { p.text });
    button(column![preview, label].spacing(8).align_x(Alignment::Center))
        .on_press(Message::SetTheme(t.name))
        .width(Fill)
        .padding(8)
        .style(move |_: &iced::Theme, status| {
            let hovered = matches!(status, button::Status::Hovered);
            button::Style {
                background: Some(if hovered { p.panel_alt } else { p.panel }.into()),
                border: iced::Border {
                    color: if selected { p.accent } else { p.border },
                    width: if selected { 2.0 } else { 1.0 },
                    radius: 6.0.into(),
                },
                text_color: p.text,
                ..Default::default()
            }
        })
        .into()
}

/// The checked-out branch in the sidebar: tinted background and accent edge.
/// Color of the BRANCHES/REMOTOS/TAGS/STASHES headers.
fn section_color() -> Color {
    let p = style::p();
    if style::minimal() { p.faint() } else { p.muted }
}

fn current_branch(_: &iced::Theme, status: button::Status) -> button::Style {
    let p = style::p();
    if style::minimal() {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        return button::Style {
            background: hovered.then(|| p.wash().into()),
            text_color: p.accent,
            border: iced::Border { radius: 4.0.into(), ..Default::default() },
            ..Default::default()
        };
    }
    let alpha = if matches!(status, button::Status::Hovered) { 0.28 } else { 0.18 };
    button::Style {
        background: Some(p.accent.scale_alpha(alpha).into()),
        text_color: p.accent,
        border: iced::Border { color: p.accent, width: 1.0, radius: 4.0.into() },
        ..Default::default()
    }
}
