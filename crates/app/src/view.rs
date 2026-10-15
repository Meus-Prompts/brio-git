//! Layout of the main window: title bar with tabs, panel toolbar and the
//! resizable panel grid of the active tab.

use iced::mouse::Interaction;
use iced::widget::pane_grid::{self, PaneGrid};
use iced::widget::{
    button, canvas, center, column, container, mouse_area, opaque, pick_list, row, scrollable, space, stack, text,
};
use iced::window::Direction;
use iced::{Alignment, Color, Element, Fill, Font, Length};
use lazygit_core::RefKind;

use crate::app::{App, Message, TabId};
use crate::data::RepoData;
use crate::layout::PaneKind;
use crate::style;
use crate::util;
use crate::widgets::graph::GraphView;
use crate::widgets::terminal::TerminalView;
use crate::avatars::Avatars;
use crate::workspace::{GRAPH_SCROLL, Selection, Workspace, WsMessage};
use crate::{view_details, view_file};

const EDGE: f32 = 5.0;

pub fn view(app: &App) -> Element<'_, Message> {
    let mut layout = column![title_bar(app)];
    if let Some(tab) = app.tabs.get(app.active) {
        layout = layout.push(toolbar(app, tab.id, &tab.ws));
        if let Some(b) = banner(tab.ws.error.as_deref(), tab.ws.notice.as_deref()) {
            layout = layout.push(b.map(move |m| Message::Tab(tab.id, m)));
        }
    }
    if let Some(e) = &app.error {
        layout = layout.push(
            container(row![text(e).color(style::p().removed).width(Fill), button("✕").on_press(Message::DismissError).style(button::text)])
                .padding([6, 12])
                .style(style::error),
        );
    }
    layout = layout.push(body(app));

    let framed: Element<_> = container(layout).style(style::root).width(Fill).height(Fill).into();
    let framed = if app.theme_picker { stack![framed, theme_picker()].into() } else { framed };
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
            text("lazy-git").size(15).color(style::p().accent),
            tabs,
            drag,
            button(text("◐ Tema").size(13)).on_press(Message::OpenThemes).style(button::text).padding([4, 10]),
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

fn toolbar<'a>(app: &'a App, id: TabId, ws: &'a Workspace) -> Element<'a, Message> {
    let branch = ws.repo.as_ref().map(|r| match (&r.head.branch, &r.head.id) {
        (Some(b), _) => format!("⎇ {b}"),
        (None, Some(id)) => format!("HEAD destacado em {}", &id[..8.min(id.len())]),
        (None, None) => "repositório vazio".into(),
    });

    let mut toggles = row![text("Painéis").size(12).color(style::p().muted)].spacing(4).align_y(Alignment::Center);
    for kind in PaneKind::ALL {
        let visible = app.layout.is_visible(kind);
        toggles = toggles.push(
            button(text(kind.title()).size(12))
                .on_press(Message::TogglePane(kind))
                .style(if visible { button::secondary } else { button::text })
                .padding([3, 8]),
        );
    }

    container(
        row![
            text(branch.unwrap_or_default()).size(13),
            button(text("↻").size(13)).on_press(Message::Tab(id, WsMessage::Refresh)).style(button::text),
            text(if ws.loading { "atualizando…" } else { "" }).size(12).color(style::p().muted),
            space::horizontal(),
            toggles,
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .padding([4, 10])
    .style(style::bar)
    .into()
}

fn banner<'a>(error: Option<&'a str>, notice: Option<&'a str>) -> Option<Element<'a, WsMessage>> {
    let (msg, is_error) = match (error, notice) {
        (Some(e), _) => (e, true),
        (None, Some(n)) => (n, false),
        (None, None) => return None,
    };
    let content = row![
        text(msg).color(if is_error { style::p().removed } else { style::p().text }).width(Fill),
        button("✕").on_press(WsMessage::DismissError).style(button::text),
    ]
    .align_y(Alignment::Center);
    let c = container(content).padding([6, 12]).width(Fill);
    Some(if is_error { c.style(style::error) } else { c.style(style::bar) }.into())
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
        return center(text("Todos os painéis estão fechados. Reabra pela barra acima.").color(style::p().muted)).into();
    };

    let grid = PaneGrid::new(panes, move |pane, kind, maximized| {
        let content: Element<'_, WsMessage> = match kind {
            PaneKind::Sidebar => sidebar(ws, repo),
            PaneKind::Graph => match &ws.file_view {
                Some(fv) => view_file::file_viewer(ws, fv),
                None => graph(ws, repo, &app.avatars),
            },
            PaneKind::Details => view_details::details(ws, repo, &app.avatars),
            PaneKind::Session => session_panel(ws),
        };
        let toggle_max = if maximized { Message::RestorePanes } else { Message::MaximizePane(pane) };
        let controls = row![
            button(text(if maximized { "❐" } else { "⤢" }).size(12))
                .on_press(toggle_max)
                .style(button::text)
                .padding([1, 6]),
            button(text("✕").size(12))
                .on_press(Message::TogglePane(*kind))
                .style(button::text)
                .padding([1, 6]),
        ];
        let title = pane_grid::TitleBar::new(text(kind.title()).size(12).color(style::p().muted))
            .controls(pane_grid::Controls::new(controls))
            .always_show_controls()
            .padding([3, 8])
            .style(style::pane_title);
        pane_grid::Content::new(content.map(move |m| Message::Tab(id, m)))
            .title_bar(title)
            .style(style::panel)
    })
    .spacing(4)
    .on_resize(8, Message::PaneResized);

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
            list = list.push(if is_head {
                item.style(current_branch)
            } else {
                item.style(if selected { button::secondary } else { button::text })
            });
        }
        list = list.push(space().height(8));
    }

    scrollable(list.padding(8)).height(Fill).into()
}

fn graph<'a>(ws: &'a Workspace, repo: &'a RepoData, avatars: &'a Avatars) -> Element<'a, WsMessage> {
    let selected = match &ws.selection {
        Selection::Commit(id) => repo.commit_index(id),
        _ => None,
    };
    let view = GraphView { data: repo, avatars, selected, cache: &ws.graph_cache, now: util::now_secs() };
    let height = view.height();
    scrollable(canvas(view).width(Fill).height(height)).id(GRAPH_SCROLL).height(Fill).into()
}

fn session_panel(ws: &Workspace) -> Element<'_, WsMessage> {
    let names: Vec<String> = ws.tools.iter().map(|t| t.name.clone()).collect();
    let running = ws.session_running();

    let action = if running {
        button("■ Parar").on_press(WsMessage::StopSession).style(button::danger)
    } else {
        button("▶ Iniciar sessão")
            .on_press_maybe(ws.repo.as_ref().map(|_| WsMessage::StartSession))
            .style(button::success)
    };

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
        None => "A IA roda no diretório e na branch atuais; checkpoints são salvos automaticamente.".into(),
    };

    let bar = row![
        pick_list(names, ws.tool.clone(), WsMessage::ToolSelected).text_size(13),
        action,
        text(status).size(12).color(style::p().muted).width(Fill),
        button(text("◉ Checkpoint agora").size(13))
            .on_press_maybe(ws.repo.as_ref().map(|_| WsMessage::CheckpointNow))
            .style(button::secondary),
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

    column![bar, timeline(ws), terminal].spacing(8).padding(8).into()
}

fn timeline(ws: &Workspace) -> Element<'_, WsMessage> {
    if ws.checkpoints.is_empty() {
        return text("Timeline: nenhum checkpoint ainda.").size(12).color(style::p().muted).into();
    }
    let mut items = row![text("Timeline").size(12).color(style::p().muted)].spacing(6).align_y(Alignment::Center);
    for (i, cp) in ws.checkpoints.iter().enumerate() {
        if i > 0 {
            items = items.push(text("─").color(style::p().border));
        }
        let selected = ws.selection == Selection::Checkpoint(cp.seq);
        items = items.push(
            button(text(format!("#{} {} · {}", cp.seq, util::clock(cp.time), cp.label)).size(12))
                .on_press(WsMessage::SelectCheckpoint(cp.seq))
                .style(if selected { button::primary } else { button::secondary })
                .padding([3, 8]),
        );
    }
    scrollable(items)
        .direction(scrollable::Direction::Horizontal(scrollable::Scrollbar::default().width(4).scroller_width(4)))
        .anchor_right()
        .width(Fill)
        .into()
}

/// Modal grid of theme cards, each with a miniature of the app in its colors.
fn theme_picker<'a>() -> Element<'a, Message> {
    let current = style::p().name;
    let mut grid = column![].spacing(12);
    for chunk in style::THEMES.chunks(4) {
        let mut line = row![].spacing(12);
        for t in chunk {
            line = line.push(theme_card(t, t.name == current));
        }
        grid = grid.push(line);
    }

    let dialog = container(
        column![
            row![
                text("Tema").size(18).width(Fill),
                button(text("✕").size(14)).on_press(Message::CloseThemes).style(button::text),
            ]
            .align_y(Alignment::Center),
            scrollable(grid),
        ]
        .spacing(14),
    )
    .padding(18)
    .max_width(640)
    .max_height(560)
    .style(style::modal);

    let backdrop = mouse_area(container(space()).width(Fill).height(Fill).style(style::backdrop))
        .on_press(Message::CloseThemes);
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
    .width(124)
    .height(66)
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
