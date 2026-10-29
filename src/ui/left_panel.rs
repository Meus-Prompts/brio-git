//! Painel esquerdo: LOCAL, REMOTE, TAGS, STASHES, SUBMODULES, WORKTREES.
//! Fora LOCAL e REMOTE, uma seção só aparece quando tem itens; as ações de
//! criação ficam no botão "+" do topo.

use std::collections::BTreeMap;

use egui::{Align2, RichText, Sense, Ui, pos2, vec2};
use git2::Oid;

use crate::git::model::{BranchInfo, Snapshot};
use crate::icons::{self, Icon};
use crate::repo_tab::{Action, Dialog, RepoTab};
use crate::theme;

pub fn show(ui: &mut Ui, tab: &mut RepoTab, actions: &mut Vec<Action>) {
    let Some(snap) = tab.snap.clone() else { return };

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        ui.label(RichText::new("Viewing").color(theme::p().text));
        let visible = (snap.locals.len() + snap.remotes.iter().map(|r| r.branches.len()).sum::<usize>()).saturating_sub(tab.hidden.len());
        ui.label(RichText::new(visible.to_string()).color(theme::p().text_accent));
        if !tab.solo.is_empty() {
            ui.label(RichText::new(format!("· soloing {}", tab.solo.len())).color(theme::p().warning));
            if ui.small_button("Unsolo all").clicked() {
                actions.push(Action::ClearSolo);
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(8.0);
            let plus = icons::button(ui, Icon::Plus, 20.0, "Adicionar branch, tag, remote, submodule…");
            plus.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Add"));
            egui::Popup::menu(&plus).show(|ui| {
                ui.set_min_width(200.0);
                add_menu(ui, tab, &snap, actions);
            });
        });
    });
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.add(
            egui::TextEdit::singleline(&mut tab.left_filter)
                .hint_text("Filter (Ctrl + Alt + f)")
                .desired_width(ui.available_width() - 10.0)
                .id(egui::Id::unique(("left_filter", &tab.path))),
        );
    });
    ui.add_space(6.0);
    let filter = tab.left_filter.to_lowercase();
    let matches = |s: &str| filter.is_empty() || s.to_lowercase().contains(&filter);

    egui::ScrollArea::vertical().id_salt("left_panel").auto_shrink([false, false]).show(ui, |ui| {
        // LOCAL
        let locals: Vec<&BranchInfo> = snap.locals.iter().filter(|b| matches(&b.name)).collect();
        if section(ui, tab, "LOCAL", Icon::Computer, locals.len(), actions, Some(|ui: &mut Ui, actions: &mut Vec<Action>| {
            if let Some(h) = snap.head.oid
                && ui.button("Create branch").clicked()
            {
                actions.push(Action::OpenDialog(Dialog::CreateBranch { at: h, name: String::new(), checkout: true }));
                ui.close();
            }
        })) {
            branch_tree(ui, tab, &snap, locals.iter().map(|b| (b.name.as_str(), *b)).collect(), false, actions);
        }

        // REMOTE
        let remote_count: usize = snap.remotes.iter().map(|r| r.branches.iter().filter(|b| matches(&format!("{}/{}", r.name, b.name))).count()).sum();
        let hosting_menu = crate::ui::hosting_view::remote_menu(tab);
        if section(ui, tab, "REMOTE", Icon::Cloud, remote_count, actions, Some(|ui: &mut Ui, actions: &mut Vec<Action>| {
            if ui.button("Add remote").clicked() {
                actions.push(Action::OpenDialog(Dialog::AddRemote { name: String::new(), url: String::new() }));
                ui.close();
            }
            if ui.button("Fetch all").clicked() {
                actions.push(Action::Fetch);
                ui.close();
            }
            if !hosting_menu.is_empty() {
                ui.separator();
                crate::ui::hosting_view::menu_items(ui, &hosting_menu, actions);
            }
        })) {
            for remote in &snap.remotes {
                let branches: Vec<&BranchInfo> = remote.branches.iter().filter(|b| matches(&format!("{}/{}", remote.name, b.name))).collect();
                if branches.is_empty() && !filter.is_empty() {
                    continue;
                }
                let key = format!("remote:{}", remote.name);
                let open = !tab.collapsed.contains(&key);
                let resp = tree_row(ui, 1, &remote.name, Some(if open { Icon::ChevronDown } else { Icon::ChevronRight }), false, None);
                if resp.clicked() {
                    toggle(tab, &key);
                }
                let remote_name = remote.name.clone();
                let url = remote.url.clone();
                resp.on_hover_text(&url).context_menu(|ui| {
                    if ui.button(format!("Fetch {remote_name}")).clicked() {
                        actions.push(Action::FetchRemote(remote_name.clone()));
                        ui.close();
                    }
                    if ui.button("Copy remote url").clicked() {
                        actions.push(Action::CopyText(url.clone()));
                        ui.close();
                    }
                    if ui.button(format!("Remove {remote_name}")).clicked() {
                        actions.push(Action::OpenDialog(Dialog::Confirm {
                            title: "Remove remote".into(),
                            body: format!("Remover o remoto {remote_name}?"),
                            confirm: "Remove".into(),
                            danger: true,
                            action: Box::new(Action::RemoveRemote(remote_name.clone())),
                        }));
                        ui.close();
                    }
                });
                if open {
                    let named: Vec<(String, &BranchInfo)> = branches.iter().map(|b| (format!("{}/{}", remote.name, b.name), *b)).collect();
                    branch_tree_owned(ui, tab, &snap, named, actions);
                }
            }
        }

        // PULL/MERGE REQUESTS e ISSUES
        crate::ui::hosting_view::left_sections(ui, tab, actions);

        // TAGS
        let tags: Vec<_> = snap.tags.iter().filter(|t| matches(&t.name)).collect();
        if !tags.is_empty() && section(ui, tab, "TAGS", Icon::Tag, tags.len(), actions, None::<fn(&mut Ui, &mut Vec<Action>)>) {
            for t in tags {
                let hidden = tab.hidden.contains(&t.name);
                let resp = tree_row(ui, 1, &t.name, Some(Icon::Tag), false, Some(hidden));
                if resp.clicked() {
                    actions.push(Action::JumpTo(t.oid));
                }
                if resp.double_clicked() {
                    actions.push(Action::CheckoutCommit(t.oid));
                }
                resp.context_menu(|ui| crate::ui::graph_view::tag_menu(ui, &t.name, t.oid, actions));
            }
        }

        // STASHES
        let stashes: Vec<_> = snap.stashes.iter().filter(|s| matches(&s.message)).collect();
        if !stashes.is_empty() && section(ui, tab, "STASHES", Icon::Stash, stashes.len(), actions, None::<fn(&mut Ui, &mut Vec<Action>)>) {
            for s in stashes {
                let resp = tree_row(ui, 1, &s.message, Some(Icon::Stash), false, None);
                if resp.clicked() {
                    actions.push(Action::JumpTo(s.oid));
                }
                if resp.double_clicked() {
                    actions.push(Action::StashApply(s.index));
                }
                resp.context_menu(|ui| {
                    if ui.button("Apply Stash").clicked() {
                        actions.push(Action::StashApply(s.index));
                        ui.close();
                    }
                    if ui.button("Pop Stash").clicked() {
                        actions.push(Action::StashPop(s.index));
                        ui.close();
                    }
                    if ui.button("Delete Stash").clicked() {
                        actions.push(Action::OpenDialog(Dialog::Confirm {
                            title: "Delete stash".into(),
                            body: format!("Apagar stash@{{{}}}: {}?", s.index, s.message),
                            confirm: "Delete".into(),
                            danger: true,
                            action: Box::new(Action::StashDrop(s.index)),
                        }));
                        ui.close();
                    }
                });
            }
        }

        // SUBMODULES
        if !snap.submodules.is_empty()
            && section(ui, tab, "SUBMODULES", Icon::Folder, snap.submodules.len(), actions, Some(|ui: &mut Ui, actions: &mut Vec<Action>| {
            if ui.button("Add a new submodule").clicked() {
                actions.push(Action::OpenDialog(Dialog::SubmoduleAdd { url: String::new(), path: String::new() }));
                ui.close();
            }
            if ui.button("Update all (init + recursive)").clicked() {
                actions.push(Action::SubmoduleUpdate(None));
                ui.close();
            }
        })) {
            for s in &snap.submodules {
                let resp = tree_row(ui, 1, &s.name, Some(Icon::Folder), false, None);
                let full = tab.path.join(&s.path);
                if resp.double_clicked() {
                    actions.push(Action::OpenRepoTab(full.clone()));
                }
                resp.on_hover_text(match s.head {
                    Some(h) => format!("{} @ {}", s.path, &h.to_string()[..7]),
                    None => format!("{} (não inicializado)", s.path),
                })
                .context_menu(|ui| {
                    if ui.button("Open submodule").clicked() {
                        actions.push(Action::OpenRepoTab(full.clone()));
                        ui.close();
                    }
                    if ui.button("Update submodule").clicked() {
                        actions.push(Action::SubmoduleUpdate(Some(s.path.clone())));
                        ui.close();
                    }
                    if ui.button("Copy path").clicked() {
                        actions.push(Action::CopyText(s.path.clone()));
                        ui.close();
                    }
                });
            }
        }

        // WORKTREES
        if !snap.worktrees.is_empty()
            && section(ui, tab, "WORKTREES", Icon::Folder, snap.worktrees.len(), actions, Some(|ui: &mut Ui, actions: &mut Vec<Action>| {
            if ui.button("Add a new worktree").clicked() {
                actions.push(Action::OpenDialog(Dialog::WorktreeAdd { path: String::new(), branch: String::new(), new_branch: true }));
                ui.close();
            }
        })) {
            for w in &snap.worktrees {
                let label = if w.locked { format!("{} (locked)", w.name) } else { w.name.clone() };
                let resp = tree_row(ui, 1, &label, Some(Icon::Folder), false, None);
                let path = std::path::PathBuf::from(&w.path);
                if resp.double_clicked() {
                    actions.push(Action::OpenRepoTab(path.clone()));
                }
                resp.on_hover_text(&w.path).context_menu(|ui| {
                    if ui.button("Open worktree").clicked() {
                        actions.push(Action::OpenRepoTab(path.clone()));
                        ui.close();
                    }
                    let (label, lock) = if w.locked { ("Unlock worktree", false) } else { ("Lock worktree", true) };
                    if ui.button(label).clicked() {
                        actions.push(Action::WorktreeLock { path: w.path.clone(), lock });
                        ui.close();
                    }
                    if ui.button("Remove worktree").clicked() {
                        actions.push(Action::OpenDialog(Dialog::Confirm {
                            title: "Remove worktree".into(),
                            body: format!("Remover o worktree em {}? Mudanças não commitadas nele serão perdidas.", w.path),
                            confirm: "Remove".into(),
                            danger: true,
                            action: Box::new(Action::WorktreeRemove { path: w.path.clone(), force: true }),
                        }));
                        ui.close();
                    }
                });
            }
        }

        // GITFLOW
        gitflow_section(ui, tab, &snap, actions);

        // GIT LFS
        if let Some(patterns) = &snap.lfs
            && !patterns.is_empty()
            && section(ui, tab, "GIT LFS", Icon::File, patterns.len(), actions, Some(|ui: &mut Ui, actions: &mut Vec<Action>| {
                if ui.button("Track pattern…").clicked() {
                    actions.push(Action::OpenDialog(Dialog::LfsTrack { pattern: String::new() }));
                    ui.close();
                }
                if ui.button("LFS pull").clicked() {
                    actions.push(Action::LfsPull);
                    ui.close();
                }
                if ui.button("Initialize LFS (install hooks)").clicked() {
                    actions.push(Action::LfsInstall);
                    ui.close();
                }
            })) {
                for pat in patterns {
                    let resp = tree_row(ui, 1, pat, Some(Icon::File), false, None);
                    resp.context_menu(|ui| {
                        if ui.button(format!("Untrack {pat}")).clicked() {
                            actions.push(Action::LfsUntrack(pat.clone()));
                            ui.close();
                        }
                    });
                }
            }
    });
}

fn gitflow_section(ui: &mut Ui, tab: &mut RepoTab, snap: &Snapshot, actions: &mut Vec<Action>) {
    use crate::git::gitflow::FlowKind;
    // Sem Gitflow inicializado a seção some; "Initialize Gitflow" fica no "+".
    let Some(cfg) = &snap.gitflow else { return };
    let flows: Vec<(FlowKind, Vec<&str>)> = FlowKind::ALL
        .iter()
        .map(|k| {
            let prefix = k.prefix(cfg);
            (*k, snap.locals.iter().filter_map(|b| b.name.strip_prefix(prefix)).collect())
        })
        .collect();
    let count: usize = flows.iter().map(|(_, v)| v.len()).sum();
    let open = section(ui, tab, "GITFLOW", Icon::Branch, count, actions, Some(|ui: &mut Ui, actions: &mut Vec<Action>| {
        for k in FlowKind::ALL {
            if ui.button(format!("Start {}", k.label())).clicked() {
                actions.push(Action::OpenDialog(Dialog::GitflowStart { kind: k, name: String::new() }));
                ui.close();
            }
        }
    }));
    if !open {
        return;
    }
    for (kind, names) in flows {
        ui.horizontal(|ui| {
            ui.add_space(26.0);
            ui.label(RichText::new(kind.label()).color(theme::p().text_secondary));
            if ui.small_button("Start").on_hover_text(format!("Start {}", kind.label())).clicked() {
                actions.push(Action::OpenDialog(Dialog::GitflowStart { kind, name: String::new() }));
            }
        });
        for name in names {
            let resp = tree_row(ui, 2, name, Some(Icon::Branch), false, None);
            let full = format!("{}{name}", kind.prefix(cfg));
            if resp.double_clicked() {
                actions.push(Action::Checkout(full.clone()));
            }
            let finish = Action::OpenDialog(Dialog::GitflowFinish {
                kind,
                name: name.to_string(),
                tag_message: if kind.tags() { format!("{}{name}", cfg.version_tag) } else { String::new() },
                keep_branch: false,
            });
            resp.context_menu(|ui| {
                if ui.button(format!("Finish {} {name}", kind.label().to_lowercase())).clicked() {
                    actions.push(finish.clone());
                    ui.close();
                }
                if ui.button(format!("Checkout {full}")).clicked() {
                    actions.push(Action::Checkout(full.clone()));
                    ui.close();
                }
            });
        }
    }
}

/// Menu do "+": cria o que as seções ocultas (sem itens) ofereceriam.
fn add_menu(ui: &mut Ui, tab: &RepoTab, snap: &Snapshot, actions: &mut Vec<Action>) {
    let mut item = |ui: &mut Ui, label: &str, a: Action| {
        if ui.button(label).clicked() {
            actions.push(a);
            ui.close();
        }
    };
    if let Some(h) = snap.head.oid {
        item(ui, "Create branch", Action::OpenDialog(Dialog::CreateBranch { at: h, name: String::new(), checkout: true }));
        item(ui, "Create tag", Action::OpenDialog(Dialog::CreateTag { at: h, name: String::new(), message: String::new(), annotated: false }));
    }
    item(ui, "Add remote", Action::OpenDialog(Dialog::AddRemote { name: String::new(), url: String::new() }));
    item(ui, "Add a new submodule", Action::OpenDialog(Dialog::SubmoduleAdd { url: String::new(), path: String::new() }));
    item(ui, "Add a new worktree", Action::OpenDialog(Dialog::WorktreeAdd { path: String::new(), branch: String::new(), new_branch: true }));
    if snap.gitflow.is_none() {
        // Sugere o branch principal que existe no repo.
        let master = ["main", "master"]
            .iter()
            .find(|m| snap.locals.iter().any(|b| b.name == **m))
            .map(|m| m.to_string())
            .unwrap_or_else(|| snap.head.branch.clone().unwrap_or_else(|| "main".into()));
        item(ui, "Initialize Gitflow", Action::OpenDialog(Dialog::GitflowInit(crate::git::gitflow::GitflowConfig { master, ..Default::default() })));
    }
    if snap.lfs.is_some() {
        item(ui, "Track with Git LFS…", Action::OpenDialog(Dialog::LfsTrack { pattern: String::new() }));
    }
    let hosting = crate::ui::hosting_view::remote_menu(tab);
    if !hosting.is_empty() {
        ui.separator();
        crate::ui::hosting_view::menu_items(ui, &hosting, actions);
    }
}

fn toggle(tab: &mut RepoTab, key: &str) {
    if !tab.collapsed.remove(key) {
        tab.collapsed.insert(key.to_string());
    }
}

fn section<F: FnOnce(&mut Ui, &mut Vec<Action>)>(
    ui: &mut Ui,
    tab: &mut RepoTab,
    title: &str,
    _icon: Icon,
    count: usize,
    actions: &mut Vec<Action>,
    menu: Option<F>,
) -> bool {
    let open = !tab.collapsed.contains(title);
    let resp = crate::ui::section_header(ui, title, open, Some(count));
    if resp.clicked() {
        toggle(tab, title);
    }
    if let Some(menu) = menu {
        resp.context_menu(|ui| menu(ui, actions));
    }
    open
}

/// Linha da árvore. `hidden == Some(_)` desenha o olho de mostrar/ocultar.
fn tree_row(ui: &mut Ui, depth: usize, label: &str, icon: Option<Icon>, bold: bool, hidden: Option<bool>) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 0.0, theme::p().hover_row);
    }
    let x = rect.left() + 14.0 + depth as f32 * 12.0;
    let mut tx = x;
    if let Some(i) = icon {
        icons::paint(ui.painter(), egui::Rect::from_center_size(pos2(x + 6.0, rect.center().y), vec2(12.0, 12.0)), i, theme::p().text_secondary);
        tx += 18.0;
    }
    let color = if hidden == Some(true) { theme::p().text_disabled } else if bold { theme::p().text_bright } else { theme::p().text };
    let text = crate::ui::elide(ui, label, &theme::prop(12.5), rect.right() - tx - 30.0);
    ui.painter().text(pos2(tx, rect.center().y), Align2::LEFT_CENTER, text, theme::prop(12.5), color);
    if let Some(h) = hidden
        && (resp.hovered() || h)
    {
        icons::paint(ui.painter(), egui::Rect::from_center_size(pos2(rect.right() - 14.0, rect.center().y), vec2(13.0, 13.0)), if h { Icon::EyeOff } else { Icon::Eye }, theme::p().text_secondary);
    }
    resp
}

fn branch_tree(ui: &mut Ui, tab: &mut RepoTab, snap: &Snapshot, branches: Vec<(&str, &BranchInfo)>, _remote: bool, actions: &mut Vec<Action>) {
    let owned: Vec<(String, &BranchInfo)> = branches.into_iter().map(|(n, b)| (n.to_string(), b)).collect();
    branch_tree_owned(ui, tab, snap, owned, actions);
}

/// Agrupa branches por pasta (`feat/x`, `feat/y` → `feat/`).
fn branch_tree_owned(ui: &mut Ui, tab: &mut RepoTab, snap: &Snapshot, branches: Vec<(String, &BranchInfo)>, actions: &mut Vec<Action>) {
    let mut folders: BTreeMap<String, Vec<(String, &BranchInfo)>> = BTreeMap::new();
    let mut roots = Vec::new();
    for (full, b) in branches {
        let local_part = b.name.clone();
        match local_part.split_once('/') {
            Some((folder, _)) => folders.entry(folder.to_string()).or_default().push((full, b)),
            None => roots.push((full, b)),
        }
    }
    let base_depth = if roots.first().is_some_and(|(f, b)| *f != b.name) || folders.values().flatten().any(|(f, b)| *f != b.name) { 2 } else { 1 };
    for (full, b) in &roots {
        branch_row(ui, tab, snap, full, b, &b.name, base_depth, actions);
    }
    for (folder, items) in folders {
        let key = format!("folder:{}:{folder}", items[0].0.split('/').next().unwrap_or(""));
        let open = !tab.collapsed.contains(&key);
        let resp = tree_row(ui, base_depth, &folder, Some(if open { Icon::ChevronDown } else { Icon::ChevronRight }), false, None);
        if resp.clicked() {
            toggle(tab, &key);
        }
        if open {
            for (full, b) in &items {
                let leaf = b.name.split_once('/').map(|(_, l)| l).unwrap_or(&b.name);
                branch_row(ui, tab, snap, full, b, leaf, base_depth + 1, actions);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn branch_row(ui: &mut Ui, tab: &RepoTab, snap: &Snapshot, full: &str, b: &BranchInfo, label: &str, depth: usize, actions: &mut Vec<Action>) {
    let is_remote = full != b.name;
    let hidden = tab.hidden.contains(full);
    let icon = if b.is_head { Some(Icon::Check) } else { None };
    let resp = tree_row(ui, depth, label, icon, b.is_head, Some(hidden));

    // Indicadores ahead/behind (↑n ↓n) à direita.
    if !is_remote && (b.ahead > 0 || b.behind > 0) {
        let mut txt = String::new();
        if b.ahead > 0 {
            txt.push_str(&format!("↑{} ", b.ahead));
        }
        if b.behind > 0 {
            txt.push_str(&format!("↓{}", b.behind));
        }
        ui.painter().text(resp.rect.right_center() - vec2(30.0, 0.0), Align2::RIGHT_CENTER, txt.trim(), theme::prop(11.0), theme::p().text_accent);
    }

    // Clique no olho alterna visibilidade no grafo.
    let eye = egui::Rect::from_center_size(pos2(resp.rect.right() - 14.0, resp.rect.center().y), vec2(18.0, 18.0));
    if resp.clicked() && ui.input(|i| i.pointer.interact_pos().is_some_and(|p| eye.contains(p))) {
        actions.push(Action::ToggleHidden(full.to_string()));
        return;
    }
    if resp.clicked() {
        actions.push(Action::JumpTo(b.oid));
    }
    if resp.double_clicked() {
        if is_remote {
            actions.push(Action::CheckoutRemote(full.to_string()));
        } else if !b.is_head {
            actions.push(Action::Checkout(b.name.clone()));
        }
    }
    let oid: Oid = b.oid;
    resp.on_hover_text(match &b.upstream {
        Some(u) => format!("{full} → {u}"),
        None => full.to_string(),
    })
    .context_menu(|ui| {
        if is_remote {
            crate::ui::graph_view::ref_menu_remote(ui, snap, full, oid, actions);
        } else {
            crate::ui::graph_view::ref_menu_local(ui, snap, &b.name, oid, tab.hosting.as_ref().map(|h| h.repo.kind), actions);
        }
    });
}
