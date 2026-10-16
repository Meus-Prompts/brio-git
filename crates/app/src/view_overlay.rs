//! Layers drawn above the window: context menu, dialogs and the invisible
//! cursor tracker that tells the menu where to open.

use std::cell::Cell;

use iced::mouse;
use iced::widget::canvas::{self, Geometry};
use iced::widget::{button, canvas as canvas_widget, center, checkbox, column, container, mouse_area, opaque, row, space, stack, text, text_input};
use iced::{Alignment, Element, Fill, Padding, Point, Rectangle, Renderer, Theme};

use crate::actions::CheckoutTarget;
use crate::app::{App, ContextMenu, Message, TabId};
use crate::menu::{MenuItem, rev_label};
use crate::style;
use crate::workspace::{DIALOG_INPUT, Dialog, DialogKind, WsMessage};

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
