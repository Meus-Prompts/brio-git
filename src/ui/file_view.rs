//! Área central de arquivo: Diff (Hunk/Inline/Split), File, History e Blame.

use std::collections::HashSet;

use egui::{Align2, Color32, RichText, Sense, Stroke, Ui, pos2, vec2};

use crate::git::diff::{self, DiffLine, DiffSource, FileDiff, Hunk, PatchMode};
use crate::icons::{self, Icon};
use crate::repo_tab::{Action, CenterView, DiffMode, FileTab, FileView, RepoTab};
use crate::theme;

const LINE_H: f32 = 19.0;

pub fn show(ui: &mut Ui, tab: &mut RepoTab, actions: &mut Vec<Action>) {
    let path = tab.path.clone();
    let repo = tab.repo().ok();
    let Some(CenterView::File(fv)) = &mut tab.center else { return };

    header(ui, fv, actions);
    ui.separator();

    match fv.tab {
        FileTab::Diff => match &fv.diff {
            Ok(d) if d.binary => {
                ui.centered_and_justified(|ui| ui.label("Arquivo binário — diff não disponível"));
            }
            Ok(d) if d.hunks.is_empty() => {
                ui.centered_and_justified(|ui| ui.label("Sem alterações de conteúdo (modo/renomeação)"));
            }
            Ok(_) => diff_body(ui, fv, actions),
            Err(e) => {
                ui.colored_label(theme::p().danger, e.as_str());
            }
        },
        FileTab::File => {
            if fv.content.is_none()
                && let Some(r) = &repo
            {
                let src = fv.source;
                fv.content = Some(diff::file_content(r, src, &fv.path).map_err(|e| e.to_string()));
            }
            match fv.content.as_ref().unwrap() {
                Ok(text) => code_view(ui, text),
                Err(e) => {
                    ui.colored_label(theme::p().danger, e);
                }
            }
        }
        FileTab::History => {
            if fv.history.is_none() {
                fv.history = Some(diff::file_history(&path, &fv.path, 500).map_err(|e| e.to_string()));
            }
            history_view(ui, fv, actions);
        }
        FileTab::Blame => {
            if fv.blame.is_none()
                && let Some(r) = &repo
            {
                let at = match fv.source {
                    DiffSource::Commit(o) | DiffSource::Range(_, o) => Some(o),
                    _ => None,
                };
                fv.blame = Some(diff::blame(r, &fv.path, at).map_err(|e| e.to_string()));
            }
            blame_view(ui, fv, actions);
        }
    }
}

fn header(ui: &mut Ui, fv: &mut FileView, actions: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        if icons::button(ui, Icon::ArrowLeft, 24.0, "Voltar ao grafo (Esc)").clicked() {
            actions.push(Action::CloseCenter);
        }
        let (dir, name) = fv.path.rsplit_once('/').map(|(d, n)| (format!("{d}/"), n.to_string())).unwrap_or_default();
        let name = if name.is_empty() { fv.path.clone() } else { name };
        ui.label(RichText::new(dir).color(theme::p().text_secondary));
        ui.label(RichText::new(name).color(theme::p().text_bright).strong());
        if let Ok(d) = &fv.diff {
            ui.label(RichText::new(format!("+{}", d.additions)).color(theme::p().success));
            ui.label(RichText::new(format!("−{}", d.deletions)).color(theme::p().danger));
        }
        let src = match fv.source {
            DiffSource::Unstaged => "unstaged".to_string(),
            DiffSource::Staged => "staged".to_string(),
            DiffSource::Commit(o) => o.to_string()[..7].to_string(),
            DiffSource::Range(a, b) => format!("{}..{}", &a.to_string()[..7], &b.to_string()[..7]),
        };
        ui.label(RichText::new(src).small().monospace().color(theme::p().text_disabled));

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            for (t, label) in [(FileTab::Blame, "Blame"), (FileTab::History, "History"), (FileTab::File, "File View"), (FileTab::Diff, "Diff View")] {
                ui.selectable_value(&mut fv.tab, t, label);
            }
            ui.separator();
            if fv.tab == FileTab::Diff {
                for (m, label) in [(DiffMode::Split, "Split"), (DiffMode::Inline, "Inline"), (DiffMode::Hunk, "Hunk")] {
                    ui.selectable_value(&mut fv.mode, m, label);
                }
            }
        });
    });
}

fn diff_body(ui: &mut Ui, fv: &mut FileView, actions: &mut Vec<Action>) {
    let FileView { diff: Ok(d), mode, source, selected, .. } = fv else { return };
    let source = *source;
    let editable = matches!(source, DiffSource::Unstaged | DiffSource::Staged);
    let line_level = editable && !matches!(d.kind, crate::git::model::ChangeKind::Untracked | crate::git::model::ChangeKind::Added | crate::git::model::ChangeKind::Deleted);

    // Ação em massa para linhas selecionadas.
    let total_sel: usize = selected.values().map(HashSet::len).sum();
    if total_sel > 0 {
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.label(format!("{total_sel} linha(s) selecionada(s)"));
            match source {
                DiffSource::Unstaged => {
                    if crate::ui::accent_button(ui, "Stage selected lines", theme::p().success, true).clicked() {
                        push_selected(d, selected, PatchMode::Stage, actions);
                    }
                    if crate::ui::accent_button(ui, "Discard selected lines", theme::p().danger, true).clicked() {
                        push_selected(d, selected, PatchMode::Discard, actions);
                    }
                }
                DiffSource::Staged
                    if crate::ui::accent_button(ui, "Unstage selected lines", theme::p().danger, true).clicked() => {
                        push_selected(d, selected, PatchMode::Unstage, actions);
                    }
                _ => {}
            }
            if ui.button("Limpar seleção").clicked() {
                selected.clear();
            }
        });
    }

    egui::ScrollArea::both().id_salt(("diff", &d.path)).auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        match mode {
            DiffMode::Hunk => {
                for (hi, h) in d.hunks.iter().enumerate() {
                    hunk_header(ui, d, hi, h, source, actions);
                    let sel = selected.entry(hi).or_default();
                    for (li, l) in h.lines.iter().enumerate() {
                        line_row(ui, l, line_level && l.origin != ' ', sel.contains(&li)).then(|| toggle(sel, li));
                    }
                    ui.add_space(10.0);
                }
            }
            DiffMode::Inline => {
                for (hi, h) in d.hunks.iter().enumerate() {
                    let sel = selected.entry(hi).or_default();
                    for (li, l) in h.lines.iter().enumerate() {
                        line_row(ui, l, line_level && l.origin != ' ', sel.contains(&li)).then(|| toggle(sel, li));
                    }
                    if hi + 1 < d.hunks.len() {
                        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 14.0), Sense::hover());
                        ui.painter().text(r.left_center() + vec2(12.0, 0.0), Align2::LEFT_CENTER, "⋯", theme::prop(12.0), theme::p().text_disabled);
                    }
                }
            }
            DiffMode::Split => split_view(ui, d),
        }
    });
}

fn toggle(set: &mut HashSet<usize>, i: usize) {
    if !set.remove(&i) {
        set.insert(i);
    }
}

fn push_selected(d: &FileDiff, selected: &std::collections::HashMap<usize, HashSet<usize>>, mode: PatchMode, actions: &mut Vec<Action>) {
    // Um patch por hunk, aplicados do último para o primeiro para manter offsets.
    let mut hunks: Vec<_> = selected.iter().filter(|(_, s)| !s.is_empty()).collect();
    hunks.sort_by_key(|(h, _)| std::cmp::Reverse(**h));
    let mut combined = String::new();
    for (hi, sel) in hunks {
        if let Some(p) = diff::partial_patch(d, *hi, Some(sel), mode) {
            if combined.is_empty() {
                combined = p;
            } else if let Some(idx) = p.find("@@") {
                combined.push_str(&p[idx..]);
            }
        }
    }
    if !combined.is_empty() {
        actions.push(Action::ApplyPatch { patch: reorder_hunks(&combined), mode });
    }
}

/// `git apply` exige hunks em ordem crescente dentro do arquivo.
fn reorder_hunks(patch: &str) -> String {
    let Some(first) = patch.find("@@") else { return patch.to_string() };
    let (head, body) = patch.split_at(first);
    let mut hunks: Vec<String> = Vec::new();
    for chunk in body.split("\n@@").map(|s| s.trim_start_matches("@@")) {
        hunks.push(format!("@@{chunk}"));
    }
    let start = |h: &String| h.split(['-', ',', ' ']).filter_map(|t| t.parse::<u32>().ok()).next().unwrap_or(0);
    hunks.sort_by_key(start);
    let mut out = head.to_string();
    for h in hunks {
        out.push_str(&h);
        if !out.ends_with('\n') {
            out.push('\n');
        }
    }
    out
}

fn hunk_header(ui: &mut Ui, d: &FileDiff, hi: usize, h: &Hunk, source: DiffSource, actions: &mut Vec<Action>) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, theme::p().panel_bg1);
    ui.painter().text(rect.left_center() + vec2(10.0, 0.0), Align2::LEFT_CENTER, &h.header, theme::mono(11.5), theme::p().text_secondary);
    let mut buttons: Vec<(&str, PatchMode, Color32)> = Vec::new();
    match source {
        DiffSource::Unstaged => {
            buttons.push(("Stage Hunk", PatchMode::Stage, theme::p().success));
            buttons.push(("Discard Hunk", PatchMode::Discard, theme::p().danger));
        }
        DiffSource::Staged => buttons.push(("Unstage Hunk", PatchMode::Unstage, theme::p().danger)),
        _ => {}
    }
    let mut x = rect.right() - 8.0;
    for (label, mode, color) in buttons {
        let w = 96.0;
        let b = egui::Rect::from_min_size(pos2(x - w, rect.top() + 3.0), vec2(w, 20.0));
        x -= w + 6.0;
        let resp = ui.interact(b, ui.scope_id().with(("hunkbtn", hi, label)), Sense::click());
        ui.painter().rect_filled(b, 3.0, if resp.hovered() { color } else { color.gamma_multiply(0.7) });
        ui.painter().text(b.center(), Align2::CENTER_CENTER, label, theme::prop(11.5), theme::p().text_bright);
        if resp.clicked() {
            if mode == PatchMode::Discard {
                if let Some(patch) = diff::partial_patch(d, hi, None, mode) {
                    actions.push(Action::OpenDialog(crate::repo_tab::Dialog::Confirm {
                        title: "Descartar hunk".into(),
                        body: "Descartar este hunk do working directory?".into(),
                        confirm: "Discard".into(),
                        danger: true,
                        action: Box::new(Action::ApplyPatch { patch, mode }),
                    }));
                }
            } else if let Some(patch) = diff::partial_patch(d, hi, None, mode) {
                actions.push(Action::ApplyPatch { patch, mode });
            }
        }
    }
}

/// Desenha uma linha de diff; retorna `true` se o número da linha foi clicado.
fn line_row(ui: &mut Ui, l: &DiffLine, selectable: bool, selected: bool) -> bool {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().max(600.0), LINE_H), Sense::hover());
    let p = ui.painter();
    let (bg, sign_color) = match l.origin {
        '+' => (theme::p().added_line, theme::p().success),
        '-' => (theme::p().deleted_line, theme::p().danger),
        _ => (Color32::TRANSPARENT, theme::p().text_disabled),
    };
    p.rect_filled(rect, 0.0, bg);
    let gutter = egui::Rect::from_min_size(rect.min, vec2(84.0, LINE_H));
    let mut clicked = false;
    if selectable {
        let resp = ui.interact(gutter, ui.scope_id().with(("ln", rect.top() as i32, l.old_no, l.new_no)), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(gutter, 0.0, theme::p().hover_row);
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        clicked = resp.on_hover_text("Clique para selecionar a linha").clicked();
    }
    let p = ui.painter();
    if selected {
        p.rect_filled(gutter, 0.0, theme::p().primary.gamma_multiply(0.45));
        p.rect_stroke(rect, 0.0, Stroke::new(1.0, theme::p().primary), egui::StrokeKind::Inside);
    }
    let num = |n: Option<u32>| n.map(|n| n.to_string()).unwrap_or_default();
    p.text(pos2(rect.left() + 36.0, rect.center().y), Align2::RIGHT_CENTER, num(l.old_no), theme::mono(11.0), theme::p().text_disabled);
    p.text(pos2(rect.left() + 74.0, rect.center().y), Align2::RIGHT_CENTER, num(l.new_no), theme::mono(11.0), theme::p().text_disabled);
    p.text(pos2(rect.left() + 90.0, rect.center().y), Align2::LEFT_CENTER, l.origin.to_string(), theme::mono(12.0), sign_color);
    let content = l.content.replace('\t', "    ");
    p.text(pos2(rect.left() + 104.0, rect.center().y), Align2::LEFT_CENTER, content, theme::mono(12.0), theme::p().text);
    clicked
}

fn split_view(ui: &mut Ui, d: &FileDiff) {
    let width = ui.available_width().max(800.0);
    let half = width / 2.0;
    for h in &d.hunks {
        let (rect, _) = ui.allocate_exact_size(vec2(width, 22.0), Sense::hover());
        ui.painter().rect_filled(rect, 0.0, theme::p().panel_bg1);
        ui.painter().text(rect.left_center() + vec2(10.0, 0.0), Align2::LEFT_CENTER, &h.header, theme::mono(11.5), theme::p().text_secondary);

        // Emparelha blocos de remoções/adições lado a lado.
        let mut i = 0;
        let lines = &h.lines;
        while i < lines.len() {
            if lines[i].origin == ' ' {
                split_row(ui, width, half, Some(&lines[i]), Some(&lines[i]));
                i += 1;
                continue;
            }
            let dels: Vec<&DiffLine> = lines[i..].iter().take_while(|l| l.origin == '-').collect();
            let j = i + dels.len();
            let adds: Vec<&DiffLine> = lines[j..].iter().take_while(|l| l.origin == '+').collect();
            let n = dels.len().max(adds.len()).max(1);
            for k in 0..n {
                split_row(ui, width, half, dels.get(k).copied(), adds.get(k).copied());
            }
            i = j + adds.len();
            if dels.is_empty() && adds.is_empty() {
                i += 1;
            }
        }
    }
}

fn split_row(ui: &mut Ui, width: f32, half: f32, left: Option<&DiffLine>, right: Option<&DiffLine>) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, LINE_H), Sense::hover());
    let p = ui.painter();
    for (side, line, x0) in [(0, left, rect.left()), (1, right, rect.left() + half)] {
        let r = egui::Rect::from_min_size(pos2(x0, rect.top()), vec2(half - 2.0, LINE_H));
        let Some(l) = line else {
            p.rect_filled(r, 0.0, theme::p().panel_bg1.gamma_multiply(0.5));
            continue;
        };
        let bg = match l.origin {
            '+' => theme::p().added_line,
            '-' => theme::p().deleted_line,
            _ => Color32::TRANSPARENT,
        };
        p.rect_filled(r, 0.0, bg);
        let n = if side == 0 { l.old_no } else { l.new_no };
        p.text(pos2(x0 + 40.0, rect.center().y), Align2::RIGHT_CENTER, n.map(|n| n.to_string()).unwrap_or_default(), theme::mono(11.0), theme::p().text_disabled);
        p.text(pos2(x0 + 50.0, rect.center().y), Align2::LEFT_CENTER, l.content.replace('\t', "    "), theme::mono(12.0), theme::p().text);
    }
    p.line_segment([pos2(rect.left() + half - 1.0, rect.top()), pos2(rect.left() + half - 1.0, rect.bottom())], Stroke::new(1.0, theme::p().panel_bg2));
}

fn code_view(ui: &mut Ui, text: &str) {
    let lines: Vec<&str> = text.lines().collect();
    egui::ScrollArea::both().id_salt("file_content").auto_shrink([false, false]).show_rows(ui, LINE_H, lines.len(), |ui, range| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for i in range {
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().max(600.0), LINE_H), Sense::hover());
            let p = ui.painter();
            p.text(pos2(rect.left() + 44.0, rect.center().y), Align2::RIGHT_CENTER, (i + 1).to_string(), theme::mono(11.0), theme::p().text_disabled);
            p.text(pos2(rect.left() + 56.0, rect.center().y), Align2::LEFT_CENTER, lines[i].replace('\t', "    "), theme::mono(12.0), theme::p().text);
        }
    });
}

fn history_view(ui: &mut Ui, fv: &FileView, actions: &mut Vec<Action>) {
    match fv.history.as_ref().unwrap() {
        Err(e) => {
            ui.colored_label(theme::p().danger, e);
        }
        Ok(entries) => {
            egui::ScrollArea::vertical().id_salt("file_history").auto_shrink([false, false]).show(ui, |ui| {
                if entries.is_empty() {
                    ui.label("Sem histórico");
                }
                for e in entries {
                    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_filled(rect, 0.0, theme::p().hover_row);
                    }
                    crate::ui::avatar(ui, rect.left_center() + vec2(20.0, 0.0), 12.0, &initials(&e.author), &e.email, crate::ui::author_color(&e.author));
                    let p = ui.painter();
                    p.text(pos2(rect.left() + 42.0, rect.top() + 13.0), Align2::LEFT_CENTER, &e.summary, theme::prop(13.0), theme::p().text_bright);
                    p.text(
                        pos2(rect.left() + 42.0, rect.top() + 31.0),
                        Align2::LEFT_CENTER,
                        format!("{} · {} · {} · {}", e.short, e.author, crate::ui::format_time(e.time, "%d/%m/%Y %H:%M"), e.path),
                        theme::prop(11.5),
                        theme::p().text_secondary,
                    );
                    if resp.clicked() {
                        actions.push(Action::OpenDiff { source: DiffSource::Commit(e.oid), path: e.path.clone() });
                    }
                    resp.context_menu(|ui| {
                        if ui.button("View on graph").clicked() {
                            actions.push(Action::JumpTo(e.oid));
                            ui.close();
                        }
                        if ui.button("Copy commit sha").clicked() {
                            actions.push(Action::CopyText(e.oid.to_string()));
                            ui.close();
                        }
                    });
                }
            });
        }
    }
}

fn initials(name: &str) -> String {
    name.split_whitespace().take(2).filter_map(|w| w.chars().next()).flat_map(char::to_uppercase).collect()
}

fn blame_view(ui: &mut Ui, fv: &FileView, actions: &mut Vec<Action>) {
    match fv.blame.as_ref().unwrap() {
        Err(e) => {
            ui.colored_label(theme::p().danger, e);
        }
        Ok(lines) => {
            egui::ScrollArea::both().id_salt("blame").auto_shrink([false, false]).show_rows(ui, LINE_H, lines.len(), |ui, range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for i in range {
                    let l = &lines[i];
                    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().max(800.0), LINE_H), Sense::hover());
                    let color = crate::ui::author_color(&l.author);
                    let p = ui.painter();
                    p.rect_filled(egui::Rect::from_min_size(rect.min, vec2(3.0, LINE_H)), 0.0, color);
                    let info = egui::Rect::from_min_size(rect.min + vec2(4.0, 0.0), vec2(250.0, LINE_H));
                    if l.block_start {
                        p.line_segment([rect.left_top(), rect.right_top()], Stroke::new(1.0, theme::p().panel_bg2));
                        let txt = format!("{}  {}  {}", l.short, crate::ui::format_time(l.time, "%d/%m/%y"), l.author);
                        let txt = crate::ui::elide(ui, &txt, &theme::prop(11.5), 240.0);
                        ui.painter().text(info.left_center() + vec2(6.0, 0.0), Align2::LEFT_CENTER, txt, theme::prop(11.5), theme::p().text_secondary);
                        let resp = ui.interact(info, ui.scope_id().with(("blame", i)), Sense::click());
                        if resp.on_hover_text("Ir para o commit no grafo").clicked() {
                            actions.push(Action::JumpTo(l.oid));
                        }
                    }
                    let p = ui.painter();
                    p.text(pos2(rect.left() + 300.0, rect.center().y), Align2::RIGHT_CENTER, l.line_no.to_string(), theme::mono(11.0), theme::p().text_disabled);
                    p.text(pos2(rect.left() + 312.0, rect.center().y), Align2::LEFT_CENTER, l.content.replace('\t', "    "), theme::mono(12.0), theme::p().text);
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::reorder_hunks;

    #[test]
    fn hunks_are_sorted_by_old_start() {
        let p = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -20,1 +20,1 @@\n-b\n+B\n@@ -3,1 +3,1 @@\n-a\n+A\n";
        let out = reorder_hunks(p);
        assert!(out.find("@@ -3").unwrap() < out.find("@@ -20").unwrap());
        assert!(out.contains("-a\n+A\n"));
    }
}
