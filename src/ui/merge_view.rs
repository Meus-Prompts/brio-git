//! Ferramenta de resolução de conflitos: A (current) | B (incoming) | Output.

use egui::{Color32, RichText, Stroke, Ui};

use crate::git::conflict::{self, Segment};
use crate::repo_tab::{Action, CenterView, RepoTab};
use crate::theme;

pub fn show(ui: &mut Ui, tab: &mut RepoTab, actions: &mut Vec<Action>) {
    let Some(CenterView::Merge(mv)) = &mut tab.center else { return };
    let total = conflict::conflict_count(&mv.segments);
    let open = conflict::unresolved(&mv.segments);

    ui.horizontal(|ui| {
        ui.add_space(4.0);
        if crate::icons::button(ui, crate::icons::Icon::ArrowLeft, 24.0, "Voltar ao grafo").clicked() {
            actions.push(Action::CloseCenter);
        }
        ui.label(RichText::new(&mv.path).strong().color(theme::p().text_bright));
        ui.label(RichText::new(format!("conflito {} de {total}", total - open)).color(theme::p().warning));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let ready = open == 0 || mv.output_edited;
            if crate::ui::accent_button(ui, "Save & mark resolved", theme::p().success, ready).clicked() {
                actions.push(Action::SaveMerge);
            }
            if ui.button("Take all incoming (B)").clicked() {
                actions.push(Action::TakeSide { path: mv.path.clone(), ours: false });
            }
            if ui.button("Take all current (A)").clicked() {
                actions.push(Action::TakeSide { path: mv.path.clone(), ours: true });
            }
            if ui.button("Reset").clicked() {
                for s in &mut mv.segments {
                    if let Segment::Conflict(c) = s {
                        c.take_ours = false;
                        c.take_theirs = false;
                    }
                }
                mv.output_edited = false;
                mv.output = conflict::render(&mv.segments);
            }
        });
    });
    if let Some(e) = &mv.error {
        ui.colored_label(theme::p().danger, e);
        return;
    }
    ui.separator();

    let avail = ui.available_height();
    let top_h = (avail * 0.55).max(160.0);
    let mut changed = false;
    ui.allocate_ui(egui::vec2(ui.available_width(), top_h), |ui| {
        ui.columns(2, |cols| {
            changed |= side(&mut cols[0], &mut mv.segments, true);
            changed |= side(&mut cols[1], &mut mv.segments, false);
        });
    });
    if changed && !mv.output_edited {
        mv.output = conflict::render(&mv.segments);
    }

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(egui::vec2(4.0, 16.0), egui::Sense::hover());
        ui.painter().rect_filled(r, 0.0, theme::conflict_output());
        ui.label(RichText::new("Output").strong());
        if mv.output_edited {
            ui.label(RichText::new("(editado manualmente)").small().color(theme::p().text_secondary));
        }
    });
    egui::ScrollArea::vertical().id_salt("merge_output").auto_shrink([false, false]).show(ui, |ui| {
        let resp = ui.add(egui::TextEdit::multiline(&mut mv.output).code_editor().desired_width(f32::INFINITY).desired_rows(12));
        if resp.changed() {
            mv.output_edited = true;
        }
    });
}

fn side(ui: &mut Ui, segments: &mut [Segment], ours: bool) -> bool {
    let color = if ours { theme::conflict_ours() } else { theme::conflict_theirs() };
    let mut changed = false;
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(egui::vec2(4.0, 16.0), egui::Sense::hover());
        ui.painter().rect_filled(r, 0.0, color);
        let label = segments
            .iter()
            .find_map(|s| match s {
                Segment::Conflict(c) => Some(if ours { c.ours_label.clone() } else { c.theirs_label.clone() }),
                _ => None,
            })
            .unwrap_or_default();
        ui.label(RichText::new(format!("{} {label}", if ours { "A" } else { "B" })).strong().color(color));
    });
    egui::ScrollArea::vertical().id_salt(("merge_side", ours)).auto_shrink([false, false]).show(ui, |ui| {
        for (i, s) in segments.iter_mut().enumerate() {
            match s {
                Segment::Common(lines) => {
                    for l in lines.iter() {
                        ui.label(RichText::new(l).monospace().color(theme::p().text_secondary));
                    }
                }
                Segment::Conflict(c) => {
                    let (lines, take) = if ours { (&c.ours, &mut c.take_ours) } else { (&c.theirs, &mut c.take_theirs) };
                    let fill = if *take { color.gamma_multiply(0.18) } else { Color32::from_rgba_premultiplied(0, 0, 0, 40) };
                    egui::Frame::new().fill(fill).stroke(Stroke::new(1.0, color.gamma_multiply(0.7))).inner_margin(4).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        if ui.checkbox(take, "Take").changed() {
                            changed = true;
                        }
                        ui.push_id(i, |ui| {
                            for l in lines.iter() {
                                ui.label(RichText::new(l).monospace().color(theme::p().text_bright));
                            }
                            if lines.is_empty() {
                                ui.label(RichText::new("(vazio)").italics().color(theme::p().text_disabled));
                            }
                        });
                    });
                }
            }
        }
    });
    changed
}
