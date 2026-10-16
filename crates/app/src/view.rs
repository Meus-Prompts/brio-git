//! Layout of the main window: title bar with tabs, panel toolbar and the
//! resizable panel grid of the active tab.

use iced::mouse::Interaction;
use iced::widget::pane_grid::{self, PaneGrid};
use iced::widget::{
    button, canvas, center, column, container, mouse_area, opaque, pick_list, row, scrollable, space, stack, text,
    text_editor, tooltip,
};
use iced::window::Direction;
use iced::{Alignment, Color, Element, Fill, Font, Length, Padding};
use brio_core::RefKind;

use crate::app::{App, Message, Section, TabId};
use crate::data::RepoData;
use crate::icons;
use crate::layout::PaneKind;
use crate::style;
use crate::util;
use crate::widgets::graph::{GraphBase, GraphView};
use crate::widgets::terminal::TerminalView;
use crate::avatars::Avatars;
use crate::workspace::{GRAPH_SCROLL, Selection, Workspace, WsMessage};
use crate::actions::Op;
use crate::menu::Target;
use crate::{view_details, view_file, view_overlay};

const EDGE: f32 = 5.0;

pub fn view(app: &App) -> Element<'_, Message> {
    let mut layout = column![title_bar(app)];
    if let Some(tab) = app.tabs.get(app.active) {
        layout = layout.push(toolbar(tab.id, &tab.ws));
        if let Some(b) = conflict_banner(tab.id, &tab.ws) {
            layout = layout.push(b);
        }
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
    let mut tabs = row![].spacing(2).align_y(Alignment::Center);
    for (i, tab) in app.tabs.iter().enumerate() {
        let active = i == app.active;
        let running = tab.ws.session_running();
        let confirm = app.confirm_close == Some(tab.id);
        let label = row![
            text(if running { "● " } else { "" }).size(11).color(style::p().added),
            text(tab.ws.name()).size(13).color(if active { style::p().text } else { style::p().muted }),
        ];
        let close = button(text(if confirm { "fechar?" } else { "✕" }).size(11))
            .on_press(Message::CloseTab(tab.id))
            .style(if confirm { button::danger } else { button::text })
            .padding([2, 6]);
        tabs = tabs.push(
            container(
                row![button(label).on_press(Message::SelectTab(tab.id)).style(button::text).padding([4, 8]), close]
                    .align_y(Alignment::Center),
            )
            .style(if active { style::tab_active } else { style::tab }),
        );
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
    .height(38)
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
    .style(style::bar)
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

    container(row![toggles, space::horizontal(), status, upstream, version].spacing(14).align_y(Alignment::Center))
        .padding([1, 8])
        .height(26)
        .align_y(Alignment::Center)
        .style(style::status_bar)
        .into()
}

/// `content` with a small explanation shown on hover.
fn tip<'a>(content: impl Into<Element<'a, Message>>, label: impl Into<String>, position: tooltip::Position) -> Element<'a, Message> {
    let label = container(text(label.into()).size(12)).padding([5, 9]).max_width(340).style(style::popover);
    tooltip(content, label, position).gap(6).into()
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
        let control = |glyph: char, msg: Message| button(icons::icon(glyph, 12.0)).on_press(msg).style(style::ghost).padding([3, 5]);
        let controls = row![
            control(if maximized { icons::RESTORE } else { icons::MAXIMIZE }, toggle_max),
            control(icons::CLOSE, Message::TogglePane(*kind)),
        ]
        .spacing(2);
        let heading = row![
            icons::icon(icons::GRIP, 11.0).color(style::p().muted.scale_alpha(0.6)),
            icons::icon(kind.icon(), 12.0),
            text(kind.title()).size(12),
        ]
        .spacing(7)
        .align_y(Alignment::Center);
        let title = pane_grid::TitleBar::new(heading)
            .controls(pane_grid::Controls::new(controls))
            .always_show_controls()
            .padding([3, 8])
            .style(style::pane_title);
        pane_grid::Content::new(content.map(move |m| Message::Tab(id, m)))
            .title_bar(title)
            .style(style::panel)
    })
    .spacing(4)
    .on_resize(8, Message::PaneResized)
    .on_drag(Message::PaneDragged);

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
            button(text(format!("{} {title}  ({})", if collapsed { "▸" } else { "▾" }, refs.len())).size(11).color(style::p().muted))
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
            let item = button(label)
                .on_press(WsMessage::JumpToCommit(r.target.clone()))
                .padding([3, 14])
                .width(Fill);
            let item = if is_head { item.style(current_branch) } else { item.style(if selected { button::secondary } else { button::text }) };
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
            button(text(format!("{} STASHES  ({})", if collapsed { "▸" } else { "▾" }, repo.stashes.len())).size(11).color(style::p().muted))
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
        let head = row![
            text(entry.selector.clone()).size(11).font(Font::MONOSPACE).color(p.muted),
            text(crate::actions::short(&entry.id).to_owned()).size(11).font(Font::MONOSPACE).color(p.accent),
            tag,
            space::horizontal(),
            text(util::relative_time(entry.time, now)).size(11).color(p.muted),
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        let message = if entry.message.is_empty() { entry.action.clone() } else { entry.message.clone() };
        let item = button(column![head, text(message).size(12).wrapping(text::Wrapping::None)].spacing(3))
            .on_press(WsMessage::SelectReflog(entry.id.clone()))
            .style(if selected { button::secondary } else { button::text })
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
    let running = ws.session_running();

    let p = style::p();
    let labeled = |glyph: char, label: &'static str| row![icons::icon(glyph, 12.0), text(label).size(13)].spacing(7).align_y(Alignment::Center);
    let action = if running {
        button(labeled(icons::STOP, "Parar")).on_press(WsMessage::StopSession).style(style::soft(p.removed))
    } else {
        button(labeled(icons::PLAY, "Iniciar sessão"))
            .on_press_maybe(ws.repo.as_ref().map(|_| WsMessage::StartSession))
            .style(style::soft(p.accent))
    }
    .padding([5, 12]);

    let status = match &ws.session {
        Some(s) if s.running => {
            let branch = ws.repo.as_ref().and_then(|r| r.head.branch.clone()).unwrap_or_else(|| "HEAD".into());
            format!("{} rodando em ⎇ {branch} — clique no terminal para digitar", s.tool)
        }
        Some(s) => format!(
            "{} encerrado{}",
            s.tool,
            s.exit_code.map(|c| format!(" (código {c})")).unwrap_or_default()
        ),
        None => "A IA roda no diretório e na branch atuais.".into(),
    };

    let bar = row![
        pick_list(names, ws.tool.clone(), WsMessage::ToolSelected).text_size(13).padding([5, 10]).style(style::quiet_pick_list),
        action,
        text(status).size(12).color(p.muted).width(Fill),
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    let terminal = canvas(TerminalView {
        snapshot: ws.term_snapshot.as_ref(),
        cache: &ws.term_cache,
        focused: ws.term_focused,
        size: ws.term_size,
        placeholder: "Escolha uma IA (ou \"terminal\") e clique em Iniciar sessão.",
    })
    .width(Fill)
    .height(Fill);

    column![bar, terminal].spacing(8).padding(8).into()
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
            column![
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
            container(space()).width(Fill).height(1).style(style::divider),
            row![nav, container(space()).width(1).height(Fill).style(style::divider), content].spacing(16).height(Fill),
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
fn current_branch(_: &iced::Theme, status: button::Status) -> button::Style {
    let p = style::p();
    let alpha = if matches!(status, button::Status::Hovered) { 0.28 } else { 0.18 };
    button::Style {
        background: Some(p.accent.scale_alpha(alpha).into()),
        text_color: p.accent,
        border: iced::Border { color: p.accent, width: 1.0, radius: 4.0.into() },
        ..Default::default()
    }
}
