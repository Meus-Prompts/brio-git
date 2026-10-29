//! Toolbar do repositório: repo/branch | Undo Redo Pull Push Branch Stash Pop | Terminal Search.

use egui::{Align2, Rect, RichText, Sense, Ui, pos2, vec2};

use crate::git::ops::PullMode;
use crate::icons::{self, Icon};
use crate::repo_tab::{Action, Dialog, RepoTab, SearchState};
use crate::theme;

/// Ações de nível de app disparadas pela toolbar.
pub enum AppRequest {
    OpenRepo(std::path::PathBuf),
}

fn tool_button(ui: &mut Ui, icon: Icon, label: &str, enabled: bool, tooltip: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(52.0, theme::TOOLBAR_HEIGHT - 4.0), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    if enabled && resp.hovered() {
        p.rect_filled(rect.shrink2(vec2(2.0, 4.0)), 3.0, theme::p().toolbar_bg1);
    }
    let color = if !enabled {
        theme::p().text_disabled
    } else if resp.hovered() {
        theme::p().text_bright
    } else {
        theme::p().text
    };
    p.text(pos2(rect.center().x, rect.top() + 11.0), Align2::CENTER_CENTER, label, theme::prop(11.5), color);
    icons::paint(p, Rect::from_center_size(pos2(rect.center().x, rect.top() + 30.0), vec2(18.0, 18.0)), icon, color);
    if tooltip.is_empty() { resp } else { resp.on_hover_text(tooltip) }
}

pub fn show(ui: &mut Ui, tab: &mut RepoTab, recent: &[std::path::PathBuf], actions: &mut Vec<Action>, app: &mut Vec<AppRequest>) {
    let snap = tab.snap.clone();
    let detached = snap.as_ref().is_none_or(|s| s.head.detached || s.head.branch.is_none());
    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.add_space(10.0);

        // Seletor de repositório.
        ui.vertical(|ui| {
            ui.add_space(5.0);
            ui.label(RichText::new("repository").small().color(theme::p().text_secondary));
            let resp = ui.add(egui::Button::new(RichText::new(format!("{}  ▾", tab.name)).strong().color(theme::p().text_bright)).frame(false));
            egui::Popup::menu(&resp).show(|ui| {
                ui.set_min_width(260.0);
                ui.label(RichText::new("Repositórios recentes").small().color(theme::p().text_secondary));
                for p in recent.iter().take(15) {
                    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    if ui.button(name).on_hover_text(p.display().to_string()).clicked() {
                        app.push(AppRequest::OpenRepo(p.clone()));
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button("Open in file manager").clicked() {
                    actions.push(Action::OpenFolder);
                    ui.close();
                }
            });
        });
        ui.add_space(6.0);
        icons::paint(ui.painter(), Rect::from_center_size(ui.cursor().min + vec2(6.0, theme::TOOLBAR_HEIGHT / 2.0), vec2(10.0, 10.0)), Icon::ChevronRight, theme::p().text_secondary);
        ui.add_space(16.0);

        // Seletor de branch.
        ui.vertical(|ui| {
            ui.add_space(5.0);
            ui.label(RichText::new("branch").small().color(theme::p().text_secondary));
            let label = snap
                .as_ref()
                .map(|s| match (&s.head.branch, s.head.detached) {
                    (Some(b), false) => b.clone(),
                    _ => s.head.oid.map(|o| format!("HEAD ({})", &o.to_string()[..7])).unwrap_or_else(|| "—".into()),
                })
                .unwrap_or_default();
            let resp = ui.add(egui::Button::new(RichText::new(format!("{label}  ▾")).strong().color(theme::p().text_bright)).frame(false));
            egui::Popup::menu(&resp).show(|ui| {
                ui.set_min_width(240.0);
                egui::ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
                    if let Some(s) = &snap {
                        for b in &s.locals {
                            let text = if b.is_head { RichText::new(format!("✔ {}", b.name)).strong() } else { RichText::new(&b.name) };
                            if ui.button(text).clicked() {
                                if !b.is_head {
                                    actions.push(Action::Checkout(b.name.clone()));
                                }
                                ui.close();
                            }
                        }
                    }
                });
            });
        });

        // Botões centrais.
        let center_w = 7.0 * 54.0 + 16.0;
        let space = (ui.available_width() - center_w - 150.0) / 2.0;
        ui.add_space(space.max(10.0));
        let can_undo = !tab.undo.is_empty();
        let can_redo = !tab.redo.is_empty();
        let undo_tip = tab.undo.last().map(|u| format!("Undo {}", u.label())).unwrap_or_else(|| "Nada para desfazer".into());
        let redo_tip = tab.redo.last().map(|u| format!("Redo {}", u.label())).unwrap_or_else(|| "Nada para refazer".into());
        if tool_button(ui, Icon::Undo, "Undo", can_undo, &undo_tip).clicked() {
            actions.push(Action::Undo);
        }
        if tool_button(ui, Icon::Redo, "Redo", can_redo, &redo_tip).clicked() {
            actions.push(Action::Redo);
        }
        let pull = tool_button(ui, Icon::Pull, "Pull", !detached, if detached { "Pull não está disponível com HEAD destacado" } else { "Pull (fast-forward if possible)" });
        if pull.clicked() {
            actions.push(Action::Pull(PullMode::FastForwardIfPossible));
        }
        let (arrow_rect, arrow) = ui.allocate_exact_size(vec2(14.0, theme::TOOLBAR_HEIGHT - 4.0), Sense::click());
        icons::paint(ui.painter(), Rect::from_center_size(arrow_rect.center() + vec2(0.0, 8.0), vec2(10.0, 10.0)), Icon::ChevronDown, theme::p().text);
        egui::Popup::menu(&arrow).show(|ui| {
            ui.set_min_width(240.0);
            let mut item = |ui: &mut Ui, label: &str, a: Action| {
                if ui.button(label).clicked() {
                    actions.push(a);
                    ui.close();
                }
            };
            item(ui, "Fetch All", Action::Fetch);
            item(ui, "Pull (fast-forward if possible)", Action::Pull(PullMode::FastForwardIfPossible));
            item(ui, "Pull (fast-forward only)", Action::Pull(PullMode::FastForwardOnly));
            item(ui, "Pull (rebase)", Action::Pull(PullMode::Rebase));
            // Local divergente do remoto: oferece descartar o local, como no cliente de referência.
            if let Some(s) = snap.as_ref()
                && let Some(cur) = s.head.branch.as_deref().filter(|_| !s.head.detached)
                && let Some((up, target)) = crate::ui::graph_view::upstream_of(s, cur)
                && s.head.oid != Some(target)
            {
                ui.separator();
                crate::ui::graph_view::reset_to_upstream_menu(ui, s, cur, &up, target, actions);
            }
        });
        if tool_button(ui, Icon::Push, "Push", !detached, "Push").clicked() {
            actions.push(Action::Push { force: false });
        }
        if tool_button(ui, Icon::Branch, "Branch", tab.head_oid().is_some(), "Criar branch (Ctrl+B)").clicked()
            && let Some(h) = tab.head_oid()
        {
            actions.push(Action::OpenDialog(Dialog::CreateBranch { at: h, name: String::new(), checkout: true }));
        }
        let dirty = snap.as_ref().is_some_and(|s| !s.status.is_empty());
        if tool_button(ui, Icon::Stash, "Stash", dirty, "Stash").clicked() {
            actions.push(Action::StashSave { message: String::new(), include_untracked: true });
        }
        let has_stash = snap.as_ref().is_some_and(|s| !s.stashes.is_empty());
        if tool_button(ui, Icon::Pop, "Pop", has_stash, "Pop stash").clicked() {
            actions.push(Action::StashPop(0));
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(8.0);
            if tool_button(ui, Icon::Search, "Search", true, "Buscar commits (Ctrl+F)").clicked() {
                toggle_search(tab);
            }
            if tool_button(ui, Icon::Terminal, "Terminal", true, "Terminal integrado").clicked() {
                actions.push(Action::ToggleTerminal);
            }
            if let Some(s) = &tab.search
                && !s.query.is_empty()
            {
                ui.label(RichText::new(format!("{} resultados", s.matches.len())).small().color(theme::p().text_secondary));
            }
        });
    });
}

pub fn toggle_search(tab: &mut RepoTab) {
    if tab.search.is_some() {
        tab.search = None;
    } else {
        tab.search = Some(SearchState { query: String::new(), matches: Vec::new(), current: 0, focus: true });
    }
}

/// Barra de busca de commits mostrada acima do grafo.
pub fn search_bar(ui: &mut Ui, tab: &mut RepoTab, actions: &mut Vec<Action>) {
    let Some(snap) = tab.snap.clone() else { return };
    let mut close = false;
    if let Some(s) = &mut tab.search {
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            icons::paint(ui.painter(), Rect::from_center_size(ui.cursor().min + vec2(8.0, 12.0), vec2(14.0, 14.0)), Icon::Search, theme::p().text);
            ui.add_space(20.0);
            let resp = ui.add(egui::TextEdit::singleline(&mut s.query).hint_text("Buscar commits por mensagem, autor ou SHA").desired_width(360.0));
            if s.focus {
                resp.request_focus();
                s.focus = false;
            }
            if resp.changed() {
                s.matches = crate::repo_tab::search_matches(&snap, &s.query);
                s.current = 0;
                if let Some(&first) = s.matches.first() {
                    actions.push(Action::JumpTo(snap.commits[first].id));
                }
            }
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.label(format!("{} / {}", if s.matches.is_empty() { 0 } else { s.current + 1 }, s.matches.len()));
            let mut go = |delta: isize| {
                if s.matches.is_empty() {
                    return;
                }
                let n = s.matches.len() as isize;
                s.current = ((s.current as isize + delta).rem_euclid(n)) as usize;
                actions.push(Action::JumpTo(snap.commits[s.matches[s.current]].id));
            };
            if ui.button("▲").clicked() {
                go(-1);
            }
            if ui.button("▼").clicked() || enter {
                go(1);
                if enter {
                    resp.request_focus();
                }
            }
            if icons::button(ui, Icon::Close, 20.0, "Fechar busca (Esc)").clicked() {
                close = true;
            }
        });
    }
    if close {
        tab.search = None;
    }
}
