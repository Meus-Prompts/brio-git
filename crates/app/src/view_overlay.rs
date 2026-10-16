//! Layers drawn above the window: context menu, dialogs and the invisible
//! cursor tracker that tells the menu where to open.

use std::cell::Cell;

use iced::mouse;
use iced::widget::canvas::{self, Geometry};
use iced::widget::{
    button, canvas as canvas_widget, center, checkbox, column, container, mouse_area, opaque, row, scrollable, space, stack, text,
    text_input,
};
use iced::{Alignment, Element, Fill, Font, Length, Padding, Point, Rectangle, Renderer, Theme};

use crate::actions::CheckoutTarget;
use crate::branch_picker::{self, BranchPicker, Choice};
use crate::icons;
use crate::app::{App, ContextMenu, Message, TabId};
use crate::menu::{MenuItem, rev_label};
use crate::style;
use crate::toasts::{self, Toasts};
use crate::workspace::{DIALOG_INPUT, Dialog, DialogKind, Workspace, WsMessage};

const MENU_W: f32 = 300.0;
const SUBMENU_W: f32 = 320.0;
const ITEM_H: f32 = 30.0;

/// Records the cursor position (window coordinates) on every move without
/// emitting messages, so the context menu can open under the mouse.
pub struct CursorTracker<'a> {
    pub cell: &'a Cell<Point>,
}

impl canvas::Program<Message> for CursorTracker<'_> {
    type State = ();

    fn update(&self, _: &mut (), event: &canvas::Event, _: Rectangle, cursor: mouse::Cursor) -> Option<canvas::Action<Message>> {
        if let canvas::Event::Mouse(mouse::Event::CursorMoved { position }) = event {
            self.cell.set(*position);
        } else if let Some(p) = cursor.position() {
            self.cell.set(p);
        }
        None
    }

    fn draw(&self, _: &(), _: &Renderer, _: &Theme, _: Rectangle, _: mouse::Cursor) -> Vec<Geometry> {
        Vec::new()
    }
}

pub fn cursor_tracker(app: &App) -> Element<'_, Message> {
    canvas_widget(CursorTracker { cell: &app.cursor }).width(Fill).height(Fill).into()
}

pub fn context_menu<'a>(app: &'a App, menu: &'a ContextMenu) -> Element<'a, Message> {
    let p = style::p();
    let height = menu.items.len() as f32 * ITEM_H + 12.0;
    // Keep the menu inside the window.
    let x = menu.position.x.min(app.window_size.width - MENU_W - 8.0).max(4.0);
    let y = menu.position.y.min(app.window_size.height - height - 8.0).max(4.0);

    let mut list = column![].width(MENU_W);
    for (i, item) in menu.items.iter().enumerate() {
        if item.divider {
            list = list.push(container(space()).width(Fill).height(1).style(style::fill(p.border, 0.0)));
        }
        list = list.push(menu_row(item, Some(i), menu.open_child == Some(i)));
    }
    let main = container(list).padding(6).style(style::modal);

    let mut layers = row![main].spacing(2);
    if let Some(child) = menu.open_child.and_then(|i| menu.items.get(i)).filter(|i| !i.children.is_empty()) {
        let offset = menu.open_child.unwrap_or(0) as f32 * ITEM_H;
        let mut sub = column![].width(SUBMENU_W);
        for item in &child.children {
            sub = sub.push(menu_row(item, None, false));
        }
        layers = layers.push(column![space().height(offset), container(sub).padding(6).style(style::modal)]);
    }

    let backdrop = mouse_area(container(space()).width(Fill).height(Fill))
        .on_press(Message::CloseMenu)
        .on_right_press(Message::CloseMenu);
    stack![
        backdrop,
        container(opaque(layers)).padding(Padding { top: y, left: x, right: 0.0, bottom: 0.0 }),
    ]
    .into()
}

fn menu_row(item: &MenuItem, index: Option<usize>, open: bool) -> Element<'_, Message> {
    let p = style::p();
    let color = match (&item.action, item.danger, item.children.is_empty()) {
        (None, _, true) => p.muted,
        (_, true, _) => p.removed,
        _ => p.text,
    };
    let mut label = row![text(&item.label).size(13).color(color).width(Fill)].align_y(Alignment::Center);
    if !item.children.is_empty() {
        label = label.push(text("›").size(14).color(p.muted));
    }
    let mut b = button(label).padding([6, 10]).width(Fill).style(move |theme, status| {
        let mut s = button::text(theme, status);
        if open || matches!(status, button::Status::Hovered) {
            s.background = Some(p.selected.into());
        }
        s
    });
    if let Some(action) = &item.action {
        b = b.on_press(Message::MenuPick(action.clone()));
    }
    let area = mouse_area(b);
    match index {
        // Hovering an entry of the main list opens (or closes) its submenu.
        Some(i) => area.on_enter(Message::MenuHover((!item.children.is_empty()).then_some(i))).into(),
        None => area.into(),
    }
}

/// The active tab's dialog, centered over a dimmed backdrop.
pub fn dialog<'a>(id: TabId, d: &'a Dialog) -> Element<'a, Message> {
    let p = style::p();
    let (title, body, confirm, danger): (String, String, String, bool) = match &d.kind {
        DialogKind::CreateBranch { at } => (format!("Nova branch em {}", rev_label(at)), String::new(), "Criar branch".into(), false),
        DialogKind::CreateTag { at } => (format!("Nova tag em {}", rev_label(at)), String::new(), "Criar tag".into(), false),
        DialogKind::RenameBranch { old } => (format!("Renomear {old}"), String::new(), "Renomear".into(), false),
        DialogKind::Confirm(op) => {
            let (t, b, c) = op.confirmation().unwrap_or_default();
            (t, b, c, true)
        }
        DialogKind::DirtyCheckout(target) => (
            "Há alterações não commitadas".to_owned(),
            format!(
                "Para ir para {} as alterações locais precisam sair do caminho. Guardá-las no stash? Você pode reaplicá-las depois pela seção Stashes.",
                match target {
                    CheckoutTarget::Detached(id) => rev_label(id),
                    other => other.label(),
                }
            ),
            "Guardar no stash e trocar".into(),
            false,
        ),
    };

    let mut content = column![text(title).size(17)].spacing(12);
    if !body.is_empty() {
        content = content.push(text(body).size(13).color(p.muted));
    }
    let wants_name = matches!(d.kind, DialogKind::CreateBranch { .. } | DialogKind::CreateTag { .. } | DialogKind::RenameBranch { .. });
    if wants_name {
        let placeholder = if matches!(d.kind, DialogKind::CreateTag { .. }) { "v1.2.0" } else { "feat/minha-branch" };
        content = content.push(
            text_input(placeholder, &d.input)
                .id(DIALOG_INPUT)
                .on_input(move |v| Message::Tab(id, WsMessage::DialogInput(v)))
                .on_submit(Message::Tab(id, WsMessage::DialogConfirm))
                .padding(8),
        );
    }
    if matches!(d.kind, DialogKind::CreateTag { .. }) {
        content = content.push(
            text_input("Mensagem (opcional: cria tag anotada)", &d.input2)
                .on_input(move |v| Message::Tab(id, WsMessage::DialogInput2(v)))
                .on_submit(Message::Tab(id, WsMessage::DialogConfirm))
                .padding(8),
        );
    }
    if matches!(d.kind, DialogKind::CreateBranch { .. }) {
        content = content.push(
            checkbox(d.check)
                .label("Fazer checkout da nova branch")
                .on_toggle(move |v| Message::Tab(id, WsMessage::DialogCheck(v)))
                .text_size(13),
        );
    }
    if let Some(e) = &d.error {
        content = content.push(text(e).size(12).color(p.removed));
    }
    content = content.push(
        row![
            space::horizontal(),
            button(text("Cancelar").size(13)).on_press(Message::Tab(id, WsMessage::DialogCancel)).style(button::secondary),
            button(text(confirm).size(13))
                .on_press(Message::Tab(id, WsMessage::DialogConfirm))
                .style(if danger { button::danger } else { button::primary }),
        ]
        .spacing(8),
    );

    let card = container(content).padding(20).width(460).style(style::modal);
    let backdrop = mouse_area(container(space()).width(Fill).height(Fill).style(style::backdrop))
        .on_press(Message::Tab(id, WsMessage::DialogCancel));
    stack![backdrop, center(opaque(card))].into()
}

/// The branch switcher, opened under the branch chip of the toolbar.
pub fn branch_picker<'a>(id: TabId, ws: &'a Workspace, picker: &'a BranchPicker) -> Element<'a, Message> {
    let p = style::p();
    let current = ws.repo.as_ref().and_then(|r| r.current_branch());
    let choices = ws.branch_choices();

    let input = text_input("Trocar para… ou digite um nome novo", &picker.query)
        .id(branch_picker::INPUT)
        .on_input(WsMessage::BranchQuery)
        .on_submit(WsMessage::BranchSubmit)
        .size(13)
        .padding([6, 8])
        .style(style::bare_input);
    let search = row![icons::icon(icons::SEARCH, 13.0).color(p.muted), input].spacing(4).align_y(Alignment::Center);

    let mut list = column![].spacing(1);
    let mut section = None;
    for (i, choice) in choices.iter().enumerate() {
        let heading = match choice {
            Choice::Local(_) => "BRANCHES LOCAIS",
            Choice::Remote(_) => "REMOTAS",
            Choice::Create(_) => "",
        };
        if section != Some(heading) && !heading.is_empty() {
            list = list.push(container(text(heading).size(10).color(p.muted)).padding([6, 8]));
        }
        section = Some(heading);
        let (glyph, label): (char, Element<'a, WsMessage>) = match choice {
            Choice::Local(name) => (icons::GIT_BRANCH, text(name.clone()).size(13).into()),
            Choice::Remote(name) => (icons::CLOUD, text(name.clone()).size(13).into()),
            Choice::Create(name) => (
                icons::BRANCH_PLUS,
                row![text("Criar branch ").size(13), text(name.clone()).size(13).font(Font::MONOSPACE).color(p.accent), text(" a partir de HEAD").size(13).color(p.muted)].into(),
            ),
        };
        let mut line = row![icons::icon(glyph, 13.0), label, space::horizontal()].spacing(8).align_y(Alignment::Center);
        if let Choice::Local(name) = choice
            && Some(name.as_str()) == current
        {
            line = line.push(icons::icon(icons::CHECK, 13.0).color(p.accent));
        }
        let highlighted = i == picker.highlighted;
        list = list.push(
            button(line)
                .on_press(WsMessage::PickBranch(choice.clone()))
                .style(move |t: &Theme, s| style::list_item(t, s, highlighted))
                .padding([5, 8])
                .width(Fill),
        );
    }
    if choices.is_empty() {
        list = list.push(container(text("Nenhuma branch encontrada.").size(12).color(p.muted)).padding(8));
    }

    let hint = text("↑↓ navegar · Enter escolher · Esc fechar").size(11).color(p.muted);
    let panel = container(
        column![
            search,
            container(space()).width(Fill).height(1).style(style::fill(p.border, 0.0)),
            scrollable(list).height(Length::Shrink),
            hint,
        ]
        .spacing(6),
    )
    .padding(8)
    .width(380)
    .max_height(420)
    .style(style::popover);

    let backdrop = mouse_area(container(space()).width(Fill).height(Fill)).on_press(WsMessage::CloseBranchPicker);
    let placed = container(opaque(panel)).padding(Padding { top: 76.0, left: 14.0, ..Padding::ZERO });
    let layer: Element<'a, WsMessage> = stack![backdrop, placed].into();
    layer.map(move |m| Message::Tab(id, m))
}

/// Toasts stacked in the bottom-right corner, above the status bar; newest last.
pub fn toasts(toasts: &Toasts) -> Element<'_, Message> {
    let p = style::p();
    let mut stack_col = column![].spacing(8).width(360);
    for t in toasts.items() {
        let (glyph, color) = match t.kind {
            toasts::Kind::Info => (icons::SUCCESS, p.added),
            toasts::Kind::Error => (icons::ALERT, p.removed),
        };
        let mut body = column![].spacing(2).width(Fill);
        if let Some(source) = &t.source {
            body = body.push(text(source.clone()).size(11).color(p.muted));
        }
        body = body.push(text(t.text.clone()).size(13));
        let content = row![
            icons::icon(glyph, 15.0).color(color),
            body,
            button(icons::icon(icons::CLOSE, 12.0)).on_press(Message::DismissToast(t.id)).style(style::ghost).padding([3, 5]),
        ]
        .spacing(10)
        .align_y(Alignment::Start);
        stack_col = stack_col.push(opaque(container(content).padding([10, 12]).width(Fill).style(style::toast(color))));
    }
    container(stack_col)
        .width(Fill)
        .height(Fill)
        .align_x(Alignment::End)
        .align_y(Alignment::End)
        .padding(Padding { right: 16.0, bottom: 38.0, ..Padding::ZERO })
        .into()
}
