//! Painel direito: WIP (staging + commit) ou detalhes do(s) commit(s) selecionado(s).

use egui::{Align2, RichText, Sense, Stroke, TextEdit, Ui, pos2, vec2};

use crate::git::diff::DiffSource;
use crate::git::model::{FileChange, OpState, Snapshot};
use crate::icons::{self, Icon};
use crate::repo_tab::{Action, CenterView, Dialog, RepoTab, Selection};
use crate::theme;

pub fn show(ui: &mut Ui, tab: &mut RepoTab, actions: &mut Vec<Action>) {
    let Some(snap) = tab.snap.clone() else { return };
    match tab.selection.clone() {
        Selection::Wip => wip(ui, tab, &snap, actions),
        Selection::Commits(oids) => commits(ui, tab, &snap, &oids, actions),
    }
}

fn open_path(tab: &RepoTab) -> Option<(DiffSource, String)> {
    match &tab.center {
        Some(CenterView::File(f)) => Some((f.source, f.path.clone())),
        Some(CenterView::Merge(m)) => Some((DiffSource::Unstaged, m.path.clone())),
        _ => None,
    }
}

fn wip(ui: &mut Ui, tab: &mut RepoTab, snap: &Snapshot, actions: &mut Vec<Action>) {
    let st = &snap.status;
    let branch = snap.head.branch.clone().unwrap_or_else(|| "HEAD destacado".into());
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(RichText::new(format!("{} alterações em", st.total())).color(theme::p().text));
        ui.label(RichText::new(&branch).color(theme::p().text_bright).strong());
    });
    ui.add_space(4.0);

    if snap.state != OpState::Clean {
        op_banner(ui, snap, actions);
    }

    let opened = open_path(tab);

    // A caixa de commit fica ancorada no rodapé; as listas usam o resto.
    egui::Panel::bottom("commit_box").frame(egui::Frame::new().fill(theme::p().panel_bg0)).show(ui, |ui| {
        ui.separator();
        commit_box(ui, tab, snap, actions);
    });

    egui::ScrollArea::vertical().id_salt("wip_lists").auto_shrink([false, false]).show(ui, |ui| {
        if !st.conflicted.is_empty() {
            list_header(ui, &format!("Conflitos ({})", st.conflicted.len()), None);
            for f in &st.conflicted {
                let sel = opened.as_ref().is_some_and(|(_, p)| *p == f.path);
                let resp = file_row(ui, f, sel, None);
                if resp.row.clicked() {
                    actions.push(Action::OpenConflict(f.path.clone()));
                }
                resp.row.context_menu(|ui| {
                    if ui.button("Take current (ours)").clicked() {
                        actions.push(Action::TakeSide { path: f.path.clone(), ours: true });
                        ui.close();
                    }
                    if ui.button("Take incoming (theirs)").clicked() {
                        actions.push(Action::TakeSide { path: f.path.clone(), ours: false });
                        ui.close();
                    }
                    if ui.button("Mark as resolved").clicked() {
                        actions.push(Action::Stage(vec![f.path.clone()]));
                        ui.close();
                    }
                });
            }
            ui.add_space(6.0);
        }

        let stage_all = list_header(ui, &format!("Unstaged Files ({})", st.unstaged.len()), (!st.unstaged.is_empty()).then_some("Stage all changes"));
        if stage_all {
            actions.push(Action::StageAll);
        }
        for f in &st.unstaged {
            let sel = opened.as_ref().is_some_and(|(s, p)| *s == DiffSource::Unstaged && *p == f.path);
            let r = file_row(ui, f, sel, Some("Stage File"));
            if r.button_clicked {
                actions.push(Action::Stage(vec![f.path.clone()]));
            } else if r.row.clicked() {
                actions.push(Action::OpenDiff { source: DiffSource::Unstaged, path: f.path.clone() });
            }
            r.row.context_menu(|ui| {
                if ui.button("Stage").clicked() {
                    actions.push(Action::Stage(vec![f.path.clone()]));
                    ui.close();
                }
                if ui.button("Discard changes").clicked() {
                    actions.push(Action::OpenDialog(Dialog::Confirm {
                        title: "Descartar alterações".into(),
                        body: format!("Descartar as alterações em {}?", f.path),
                        confirm: "Discard".into(),
                        danger: true,
                        action: Box::new(Action::Discard(vec![f.clone()])),
                    }));
                    ui.close();
                }
                if ui.button("Stash file").clicked() {
                    actions.push(Action::StashSave { message: format!("stash {}", f.path), include_untracked: true });
                    ui.close();
                }
                if ui.button("Ignore").clicked() {
                    let gi = tab.path.join(".gitignore");
                    let mut cur = std::fs::read_to_string(&gi).unwrap_or_default();
                    if !cur.is_empty() && !cur.ends_with('\n') {
                        cur.push('\n');
                    }
                    cur.push_str(&f.path);
                    cur.push('\n');
                    let _ = std::fs::write(gi, cur);
                    actions.push(Action::Refresh);
                    ui.close();
                }
                if ui.button("Copy file path").clicked() {
                    actions.push(Action::CopyText(f.path.clone()));
                    ui.close();
                }
            });
        }
        ui.add_space(6.0);

        let unstage_all = list_header(ui, &format!("Staged Files ({})", st.staged.len()), (!st.staged.is_empty()).then_some("Unstage all changes"));
        if unstage_all {
            actions.push(Action::UnstageAll);
        }
        for f in &st.staged {
            let sel = opened.as_ref().is_some_and(|(s, p)| *s == DiffSource::Staged && *p == f.path);
            let r = file_row(ui, f, sel, Some("Unstage File"));
            if r.button_clicked {
                actions.push(Action::Unstage(vec![f.path.clone()]));
            } else if r.row.clicked() {
                actions.push(Action::OpenDiff { source: DiffSource::Staged, path: f.path.clone() });
            }
            r.row.context_menu(|ui| {
                if ui.button("Unstage").clicked() {
                    actions.push(Action::Unstage(vec![f.path.clone()]));
                    ui.close();
                }
                if ui.button("Copy file path").clicked() {
                    actions.push(Action::CopyText(f.path.clone()));
                    ui.close();
                }
            });
        }
    });
}

fn op_banner(ui: &mut Ui, snap: &Snapshot, actions: &mut Vec<Action>) {
    egui::Frame::new().fill(theme::lane_row(7)).inner_margin(8).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let conflicts = snap.status.conflicted.len();
        ui.label(RichText::new(format!("{} em andamento", snap.state.label())).color(theme::p().warning).strong());
        if conflicts > 0 {
            ui.label(format!("{conflicts} arquivo(s) com conflito. Resolva e faça stage para continuar."));
        }
        ui.horizontal(|ui| {
            if ui.button(format!("Abort {}", snap.state.label())).clicked() {
                actions.push(Action::Abort);
            }
            if conflicts == 0 && crate::ui::accent_button(ui, "Continue", theme::p().success, true).clicked() {
                actions.push(Action::Continue);
            }
            if snap.state == OpState::Rebase && ui.button("Skip").clicked() {
                actions.push(Action::Skip);
            }
        });
    });
}

fn list_header(ui: &mut Ui, title: &str, button: Option<&str>) -> bool {
    let mut clicked = false;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, theme::p().panel_bg1);
    ui.painter().text(rect.left_center() + vec2(10.0, 0.0), Align2::LEFT_CENTER, title, theme::prop(12.5), theme::p().text);
    if let Some(label) = button {
        let galley_w = ui.ctx().fonts_mut(|f| f.layout_no_wrap(label.into(), theme::prop(12.0), theme::p().text).size().x);
        let b = egui::Rect::from_center_size(rect.right_center() - vec2(galley_w / 2.0 + 16.0, 0.0), vec2(galley_w + 16.0, 22.0));
        let resp = ui.interact(b, ui.scope_id().with(title), Sense::click());
        let color = if label.starts_with("Stage") { theme::p().success } else { theme::p().danger };
        ui.painter().rect_filled(b, 3.0, if resp.hovered() { color } else { color.gamma_multiply(0.75) });
        ui.painter().text(b.center(), Align2::CENTER_CENTER, label, theme::prop(12.0), theme::p().text_bright);
        clicked = resp.clicked();
    }
    clicked
}

struct FileRow {
    row: egui::Response,
    button_clicked: bool,
}

fn file_row(ui: &mut Ui, f: &FileChange, selected: bool, button: Option<&str>) -> FileRow {
    let (rect, row) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, 0.0, theme::p().selected_row);
    } else if row.hovered() {
        p.rect_filled(rect, 0.0, theme::p().hover_row);
    }
    let color = crate::ui::change_color(f.kind);
    let badge = egui::Rect::from_center_size(rect.left_center() + vec2(18.0, 0.0), vec2(13.0, 13.0));
    p.rect_stroke(badge, 2.0, Stroke::new(1.2, color), egui::StrokeKind::Inside);
    p.text(badge.center(), Align2::CENTER_CENTER, f.kind.letter(), theme::prop(9.5), color);

    let (dir, name) = match f.path.rsplit_once('/') {
        Some((d, n)) => (format!("{d}/"), n.to_string()),
        None => (String::new(), f.path.clone()),
    };
    let button_w = if button.is_some() && row.hovered() { 90.0 } else { 0.0 };
    let avail = rect.width() - 40.0 - button_w;
    let x = rect.left() + 32.0;
    let name_w = ui.ctx().fonts_mut(|fo| fo.layout_no_wrap(name.clone(), theme::prop(12.5), theme::p().text).size().x);
    let dir_txt = crate::ui::elide(ui, &dir, &theme::prop(12.5), (avail - name_w).max(0.0));
    let g = ui.painter().text(pos2(x, rect.center().y), Align2::LEFT_CENTER, &dir_txt, theme::prop(12.5), theme::p().text_secondary);
    let name = crate::ui::elide(ui, &name, &theme::prop(12.5), avail - g.width());
    ui.painter().text(pos2(g.right(), rect.center().y), Align2::LEFT_CENTER, name, theme::prop(12.5), if selected { theme::p().text_bright } else { theme::p().text });

    let mut button_clicked = false;
    if let Some(label) = button
        && row.hovered()
    {
        let b = egui::Rect::from_min_size(pos2(rect.right() - 92.0, rect.top() + 2.0), vec2(86.0, 20.0));
        let resp = ui.interact(b, ui.scope_id().with(("filebtn", &f.path, label)), Sense::click());
        let color = if label.starts_with("Stage") { theme::p().success } else { theme::p().danger };
        ui.painter().rect_filled(b, 3.0, if resp.hovered() { color } else { color.gamma_multiply(0.7) });
        ui.painter().text(b.center(), Align2::CENTER_CENTER, label, theme::prop(11.5), theme::p().text_bright);
        button_clicked = resp.clicked();
    }
    let row = match &f.old_path {
        Some(old) => row.on_hover_text(format!("renomeado de {old}")),
        None => row,
    };
    FileRow { row, button_clicked }
}

fn commit_box(ui: &mut Ui, tab: &mut RepoTab, snap: &Snapshot, actions: &mut Vec<Action>) {
    ui.add_space(4.0);
    egui::Frame::new().inner_margin(egui::Margin::symmetric(10, 4)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Commit").strong().color(theme::p().text_bright));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let had = tab.amend;
                ui.checkbox(&mut tab.amend, "Amend");
                if tab.amend && !had && tab.wip_summary.is_empty()
                    && let Some(head) = snap.head.oid.and_then(|o| snap.index_of.get(&o)).map(|&i| &snap.commits[i])
                {
                    tab.wip_summary = head.summary.clone();
                    tab.wip_body = head.body.clone();
                }
                ai_button(ui, tab, snap, actions);
            });
        });
        let len = tab.wip_summary.chars().count();
        let summary = ui.add(
            TextEdit::singleline(&mut tab.wip_summary)
                .hint_text("Summary")
                .desired_width(f32::INFINITY)
                .char_limit(200)
                .id(ui.scope_id().with("commit_summary")),
        );
        if tab.focus_message {
            summary.request_focus();
            tab.focus_message = false;
        }
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let color = if len > 72 { theme::p().warning } else { theme::p().text_disabled };
                ui.label(RichText::new(format!("{len}")).small().color(color));
            });
        });
        ui.add(TextEdit::multiline(&mut tab.wip_body).hint_text("Description").desired_rows(3).desired_width(f32::INFINITY));
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut tab.push_after_commit, "Push after committing");
            ui.checkbox(&mut tab.skip_hooks, "Skip hooks");
        });

        let staged = snap.status.staged.len();
        let has_msg = !tab.wip_summary.trim().is_empty();
        let (label, enabled, stage_all) = if staged > 0 {
            let verb = if tab.amend { "Amend previous commit" } else { "Commit changes" };
            (format!("{verb} to {staged} file{}", if staged == 1 { "" } else { "s" }), has_msg, false)
        } else if tab.amend && has_msg {
            ("Amend previous commit".to_string(), true, false)
        } else if !snap.status.unstaged.is_empty() {
            (if has_msg { "Stage all changes & commit".to_string() } else { "Type a message to commit".to_string() }, has_msg, true)
        } else {
            ("Stage files/changes to commit".to_string(), false, false)
        };
        ui.add_space(4.0);
        let resp = ui.add_sized(
            [ui.available_width(), 32.0],
            egui::Button::new(RichText::new(label).color(if enabled { theme::p().text_bright } else { theme::p().text_disabled }).strong())
                .fill(if enabled { theme::p().success.gamma_multiply(0.8) } else { theme::p().panel_bg2 }),
        );
        if resp.clicked() && enabled {
            actions.push(Action::Commit { stage_all });
        }
    });
}

/// Botão "IA": só aparece com algum CLI de IA instalado; com mais de um, vira menu.
fn ai_button(ui: &mut Ui, tab: &RepoTab, snap: &Snapshot, actions: &mut Vec<Action>) {
    let tools = crate::ai::available();
    if tools.is_empty() {
        return;
    }
    if let Some((tool, _)) = &tab.ai_pending {
        ui.label(RichText::new(format!("Gerando com {}…", tool.label())).small().color(theme::p().text_secondary));
        ui.spinner();
        return;
    }
    let has_changes = !snap.status.staged.is_empty() || !snap.status.unstaged.is_empty();
    ui.add_enabled_ui(has_changes, |ui| {
        if let [(tool, _)] = tools {
            if ui.small_button("IA").on_hover_text(format!("Gerar summary e description com {}", tool.label())).clicked() {
                actions.push(Action::GenerateMessage(*tool));
            }
        } else {
            ui.menu_button("IA", |ui| {
                for (tool, _) in tools {
                    if ui.button(format!("Gerar com {}", tool.label())).clicked() {
                        actions.push(Action::GenerateMessage(*tool));
                    }
                }
            })
            .response
            .on_hover_text("Gerar summary e description com a IA local");
        }
    });
}

fn commits(ui: &mut Ui, tab: &mut RepoTab, snap: &Snapshot, oids: &[git2::Oid], actions: &mut Vec<Action>) {
    tab.ensure_commit_files();
    let opened = open_path(tab);
    let source = tab.selection_source(oids);

    egui::ScrollArea::vertical().id_salt("commit_detail").auto_shrink([false, false]).show(ui, |ui| {
        egui::Frame::new().inner_margin(10).show(ui, |ui| {
            ui.set_width(ui.available_width());
            if oids.len() > 1 {
                ui.label(RichText::new(format!("{} commits selecionados", oids.len())).strong().color(theme::p().text_bright));
                if let DiffSource::Range(a, b) = source {
                    ui.label(RichText::new(format!("Diff entre {} e {}", &a.to_string()[..7], &b.to_string()[..7])).color(theme::p().text_secondary));
                }
                if ui.button(format!("Cherry pick {} commits", oids.len())).clicked() {
                    actions.push(Action::CherryPick(oids.to_vec()));
                }
            } else if let Some(c) = snap.index_of.get(&oids[0]).map(|&i| &snap.commits[i]) {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("commit:").color(theme::p().text_secondary));
                    let sha = ui.add(egui::Label::new(RichText::new(&c.short).monospace().color(theme::p().link)).sense(Sense::click()));
                    if sha.on_hover_text("Copiar SHA").clicked() {
                        actions.push(Action::CopyText(c.id.to_string()));
                    }
                    if !c.parents.is_empty() {
                        ui.label(RichText::new(if c.parents.len() > 1 { "parents:" } else { "parent:" }).color(theme::p().text_secondary));
                        for p in &c.parents {
                            let l = ui.add(egui::Label::new(RichText::new(&p.to_string()[..7]).monospace().color(theme::p().link)).sense(Sense::click()));
                            if l.clicked() {
                                actions.push(Action::JumpTo(*p));
                            }
                        }
                    }
                });
                let (sig, signer) = tab.signature(c.id);
                let badge = match sig {
                    crate::git::ops::Signature::Good => Some(("✔ Assinatura verificada", theme::p().success)),
                    crate::git::ops::Signature::Unverified => Some(("Assinado (não verificado)", theme::p().warning)),
                    crate::git::ops::Signature::Bad => Some(("⚠ Assinatura inválida", theme::p().danger)),
                    crate::git::ops::Signature::None => None,
                };
                if let Some((text, color)) = badge {
                    let r = ui.label(RichText::new(text).small().color(color));
                    if !signer.is_empty() {
                        r.on_hover_text(format!("Assinado por {signer}"));
                    }
                }
                ui.add_space(6.0);
                ui.label(RichText::new(&c.summary).size(15.0).strong().color(theme::p().text_bright));
                if !c.body.is_empty() {
                    ui.add_space(4.0);
                    ui.label(RichText::new(&c.body).color(theme::p().text));
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::hover());
                    crate::ui::avatar(ui, r.center(), 14.0, &c.initials(), &c.email, crate::ui::author_color(&c.author));
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&c.author).color(theme::p().text_bright));
                        ui.label(
                            RichText::new(format!("authored {} ({})", crate::ui::format_time(c.time, "%d/%m/%Y %H:%M"), crate::ui::relative_time(c.time)))
                                .small()
                                .color(theme::p().text_secondary),
                        );
                        if c.committer != c.author {
                            ui.label(RichText::new(format!("committed by {}", c.committer)).small().color(theme::p().text_secondary));
                        }
                    });
                });
                if let Some(refs) = snap.refs_by_commit.get(&c.id) {
                    ui.horizontal_wrapped(|ui| {
                        for r in refs {
                            ui.label(RichText::new(&r.name).small().background_color(theme::lane_bg(1)).color(theme::p().text_bright));
                        }
                    });
                }
            }
        });
        ui.separator();

        let files = tab.commit_files.as_ref().map(|(_, f)| f.clone());
        match files {
            Some(Ok(files)) => {
                let adds: usize = files.iter().map(|f| f.additions).sum();
                let dels: usize = files.iter().map(|f| f.deletions).sum();
                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    ui.label(RichText::new(format!("{} arquivo(s)", files.len())).color(theme::p().text));
                    ui.label(RichText::new(format!("+{adds}")).color(theme::p().success));
                    ui.label(RichText::new(format!("−{dels}")).color(theme::p().danger));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icons::button(ui, Icon::Tree, 20.0, "Tree view").clicked() {
                            tab.file_tree = true;
                        }
                        if icons::button(ui, Icon::List, 20.0, "Path view").clicked() {
                            tab.file_tree = false;
                        }
                    });
                });
                if tab.file_tree {
                    tree(ui, &files, source, opened.as_ref(), actions);
                } else {
                    for f in &files {
                        let sel = opened.as_ref().is_some_and(|(s, p)| *s == source && *p == f.path);
                        let r = file_row(ui, f, sel, None);
                        stats_overlay(ui, &r.row, f);
                        if r.row.clicked() {
                            actions.push(Action::OpenDiff { source, path: f.path.clone() });
                        }
                        r.row.context_menu(|ui| file_menu(ui, f, actions));
                    }
                }
            }
            Some(Err(e)) => {
                ui.colored_label(theme::p().danger, e);
            }
            None => {
                ui.spinner();
            }
        }
    });
}

fn stats_overlay(ui: &Ui, row: &egui::Response, f: &FileChange) {
    if row.hovered() || f.additions + f.deletions == 0 {
        return;
    }
    let p = ui.painter();
    let x = row.rect.right() - 8.0;
    let g = p.text(pos2(x, row.rect.center().y), Align2::RIGHT_CENTER, format!("−{}", f.deletions), theme::prop(11.0), theme::p().danger);
    p.text(pos2(g.left() - 6.0, row.rect.center().y), Align2::RIGHT_CENTER, format!("+{}", f.additions), theme::prop(11.0), theme::p().success);
}

fn file_menu(ui: &mut Ui, f: &FileChange, actions: &mut Vec<Action>) {
    if ui.button("Copy file path").clicked() {
        actions.push(Action::CopyText(f.path.clone()));
        ui.close();
    }
}

fn tree(ui: &mut Ui, files: &[FileChange], source: DiffSource, opened: Option<&(DiffSource, String)>, actions: &mut Vec<Action>) {
    use std::collections::BTreeMap;
    let mut dirs: BTreeMap<String, Vec<&FileChange>> = BTreeMap::new();
    for f in files {
        let dir = f.path.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();
        dirs.entry(dir).or_default().push(f);
    }
    for (dir, files) in dirs {
        let render = |ui: &mut Ui, actions: &mut Vec<Action>| {
            for f in files.iter().copied() {
                let sel = opened.is_some_and(|(s, p)| *s == source && *p == f.path);
                let r = file_row(ui, f, sel, None);
                stats_overlay(ui, &r.row, f);
                if r.row.clicked() {
                    actions.push(Action::OpenDiff { source, path: f.path.clone() });
                }
            }
        };
        if dir.is_empty() {
            render(ui, actions);
        } else {
            egui::CollapsingHeader::new(RichText::new(&dir).color(theme::p().text_secondary)).default_open(true).show(ui, |ui| render(ui, actions));
        }
    }
}
