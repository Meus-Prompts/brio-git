//! Painel de rebase interativo: pick / reword / squash / fixup / drop e reordenação.

use egui::{Key, RichText, Ui};

use crate::git::ops::RebaseAction;
use crate::repo_tab::{Action, RepoTab};
use crate::theme;

pub fn show(ui: &mut Ui, tab: &mut RepoTab, actions: &mut Vec<Action>) {
    let branch = tab.snap.as_ref().and_then(|s| s.head.branch.clone()).unwrap_or_else(|| "HEAD".into());
    let Some(plan) = &mut tab.rebase else { return };

    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.heading(RichText::new("Interactive Rebase").color(theme::p().text_bright));
        ui.label(RichText::new(format!("Rebasing {} commits de {branch} onto {}", plan.steps.len(), plan.base_label)).color(theme::p().text_secondary));
    });
    ui.label(RichText::new("Atalhos: ↑/↓ seleciona · p pick · r reword · s squash · f fixup · d drop · Alt+↑/↓ move").small().color(theme::p().text_disabled));
    ui.separator();

    let n = plan.steps.len();
    let mut swap: Option<(usize, usize)> = None;
    let mut clicked: Option<usize> = None;
    plan.cursor = plan.cursor.min(n.saturating_sub(1));
    if !ui.ctx().egui_wants_keyboard_input() {
        let (up, down, alt, set) = ui.input(|i| {
            let set = [(Key::P, RebaseAction::Pick), (Key::R, RebaseAction::Reword), (Key::S, RebaseAction::Squash), (Key::F, RebaseAction::Fixup), (Key::D, RebaseAction::Drop)]
                .into_iter()
                .find(|(k, _)| i.key_pressed(*k))
                .map(|(_, a)| a);
            (i.key_pressed(Key::ArrowUp), i.key_pressed(Key::ArrowDown), i.modifiers.alt, set)
        });
        let c = plan.cursor;
        match (up, down, alt) {
            (true, _, true) if c > 0 => swap = Some((c, c - 1)),
            (_, true, true) if c + 1 < n => swap = Some((c, c + 1)),
            (true, _, false) => plan.cursor = c.saturating_sub(1),
            (_, true, false) => plan.cursor = (c + 1).min(n.saturating_sub(1)),
            _ => {}
        }
        if let Some(a) = set
            && let Some(step) = plan.steps.get_mut(c)
        {
            step.1 = a;
        }
    }
    egui::ScrollArea::vertical().id_salt("rebase").max_height(ui.available_height() - 50.0).show(ui, |ui| {
        for i in 0..n {
            let (info, action, message) = &mut plan.steps[i];
            let dropped = *action == RebaseAction::Drop;
            let focused = i == plan.cursor;
            let frame = egui::Frame::new()
                .fill(if focused { theme::p().selected_row } else if i % 2 == 0 { theme::p().panel_bg0 } else { theme::p().panel_bg1 })
                .inner_margin(egui::Margin::symmetric(8, 4))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt(("rb_action", i)).width(80.0).selected_text(action.label()).show_ui(ui, |ui| {
                            for a in RebaseAction::ALL {
                                ui.selectable_value(action, a, a.label());
                            }
                        });
                        if ui.add_enabled(i > 0, egui::Button::new("▲")).on_hover_text("Mover para cima").clicked() {
                            swap = Some((i, i - 1));
                        }
                        if ui.add_enabled(i + 1 < n, egui::Button::new("▼")).on_hover_text("Mover para baixo").clicked() {
                            swap = Some((i, i + 1));
                        }
                        ui.label(RichText::new(&info.short).monospace().color(theme::p().link));
                        let mut text = RichText::new(&info.summary).color(if dropped { theme::p().text_disabled } else { theme::p().text_bright });
                        if dropped {
                            text = text.strikethrough();
                        }
                        ui.label(text);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(RichText::new(&info.author).small().color(theme::p().text_secondary));
                        });
                    });
                    if *action == RebaseAction::Reword {
                        ui.add(egui::TextEdit::multiline(message).desired_rows(2).desired_width(f32::INFINITY));
                    }
                });
            if frame.response.interact(egui::Sense::click()).clicked() {
                clicked = Some(i);
            }
        }
    });
    if let Some(i) = clicked {
        plan.cursor = i;
    }
    if let Some((a, b)) = swap {
        plan.steps.swap(a, b);
        if plan.cursor == a {
            plan.cursor = b;
        }
    }
    // O commit mais antigo (último da lista) não pode ser squash/fixup.
    if let Some(last) = plan.steps.last_mut()
        && matches!(last.1, RebaseAction::Squash | RebaseAction::Fixup)
    {
        last.1 = RebaseAction::Pick;
    }

    ui.separator();
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        if crate::ui::accent_button(ui, "Start Rebase", theme::p().success, true).clicked() {
            actions.push(Action::RunInteractiveRebase);
        }
        if ui.button("Cancel Rebase").clicked() {
            actions.push(Action::CancelInteractiveRebase);
        }
        if ui.button("Reset").clicked() {
            for s in &mut plan.steps {
                s.1 = RebaseAction::Pick;
            }
        }
    });
}
