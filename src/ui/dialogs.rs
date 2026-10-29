//! Diálogos modais da aba de repositório.

use egui::{Id, Key, Modal, RichText, TextEdit};

use crate::git::gitflow::FlowKind;
use crate::repo_tab::{Action, Dialog, RepoTab};
use crate::theme;

fn valid_ref_name(name: &str) -> bool {
    let n = name.trim();
    !n.is_empty()
        && !n.starts_with('-')
        && !n.ends_with('/')
        && !n.ends_with(".lock")
        && !n.contains("..")
        && !n.chars().any(|c| c.is_whitespace() || "~^:?*[\\".contains(c))
}

pub fn show(ctx: &egui::Context, tab: &mut RepoTab, actions: &mut Vec<Action>) {
    let remotes: Vec<String> = tab.snap.as_ref().map(|s| s.remotes.iter().map(|r| r.name.clone()).collect()).unwrap_or_default();
    // (provedor, host, página de token, branches do remoto) — lido antes de emprestar o diálogo.
    let hosting = tab.hosting.as_ref().map(|h| {
        let branches: Vec<String> = tab
            .snap
            .as_ref()
            .and_then(|s| s.remotes.iter().find(|r| r.name == h.repo.remote))
            .map(|r| r.branches.iter().map(|b| b.name.clone()).collect())
            .unwrap_or_default();
        (h.repo.kind, h.repo.host.clone(), h.repo.token_page(), branches)
    });
    let Some(dialog) = &mut tab.dialog else { return };
    let mut close = false;
    let enter = ctx.input(|i| i.key_pressed(Key::Enter) && !i.modifiers.shift);

    let resp = Modal::new(Id::unique("repo_dialog")).show(ctx, |ui| {
        ui.set_width(440.0);
        match dialog {
            Dialog::CreateBranch { at, name, checkout } => {
                ui.heading("Criar branch");
                ui.label(RichText::new(format!("a partir de {}", &at.to_string()[..7])).color(theme::p().text_secondary));
                let r = ui.add(TextEdit::singleline(name).hint_text("nome-do-branch").desired_width(f32::INFINITY));
                r.request_focus();
                ui.checkbox(checkout, "Fazer checkout após criar");
                let ok = valid_ref_name(name);
                ui.horizontal(|ui| {
                    if crate::ui::accent_button(ui, "Create branch", theme::p().success, ok).clicked() || (enter && ok) {
                        actions.push(Action::CreateBranch { name: name.trim().to_string(), at: *at, checkout: *checkout });
                        close = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            }
            Dialog::CreateTag { at, name, message, annotated } => {
                ui.heading(if *annotated { "Criar tag anotada" } else { "Criar tag" });
                ui.label(RichText::new(format!("em {}", &at.to_string()[..7])).color(theme::p().text_secondary));
                ui.add(TextEdit::singleline(name).hint_text("v1.0.0").desired_width(f32::INFINITY)).request_focus();
                if *annotated {
                    ui.add(TextEdit::multiline(message).hint_text("mensagem da tag").desired_rows(3).desired_width(f32::INFINITY));
                }
                let ok = valid_ref_name(name) && (!*annotated || !message.trim().is_empty());
                ui.horizontal(|ui| {
                    if crate::ui::accent_button(ui, "Create tag", theme::p().success, ok).clicked() || (enter && ok && !*annotated) {
                        let msg = annotated.then(|| message.clone());
                        actions.push(Action::CreateTag { name: name.trim().to_string(), at: *at, message: msg });
                        close = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            }
            Dialog::RenameBranch { old, new } => {
                ui.heading(format!("Renomear {old}"));
                ui.add(TextEdit::singleline(new).desired_width(f32::INFINITY)).request_focus();
                let ok = valid_ref_name(new) && new != old;
                ui.horizontal(|ui| {
                    if crate::ui::accent_button(ui, "Rename", theme::p().primary, ok).clicked() || (enter && ok) {
                        actions.push(Action::RenameBranch { old: old.clone(), new: new.trim().to_string() });
                        close = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            }
            Dialog::Confirm { title, body, confirm, danger, action } => {
                ui.heading(title.as_str());
                ui.add_space(4.0);
                ui.label(body.as_str());
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let color = if *danger { theme::p().danger } else { theme::p().primary };
                    if crate::ui::accent_button(ui, confirm, color, true).clicked() || enter {
                        actions.push((**action).clone());
                        close = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            }
            Dialog::Stash { message, include_untracked } => {
                ui.heading("Stash");
                ui.add(TextEdit::singleline(message).hint_text("mensagem (opcional)").desired_width(f32::INFINITY)).request_focus();
                ui.checkbox(include_untracked, "Incluir arquivos não rastreados");
                ui.horizontal(|ui| {
                    if crate::ui::accent_button(ui, "Stash", theme::p().primary, true).clicked() || enter {
                        actions.push(Action::StashSave { message: message.clone(), include_untracked: *include_untracked });
                        close = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            }
            Dialog::PushUpstream { branch, remote, remote_branch } => {
                ui.heading("Definir upstream e fazer push");
                ui.label(format!("O branch {branch} não tem upstream. Escolha o remoto:"));
                if remotes.is_empty() {
                    ui.colored_label(theme::p().warning, "Este repositório não tem remotos. Adicione um no painel REMOTE.");
                }
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("push_remote").selected_text(remote.as_str()).show_ui(ui, |ui| {
                        for r in &remotes {
                            ui.selectable_value(remote, r.clone(), r);
                        }
                    });
                    ui.label("/");
                    ui.add(TextEdit::singleline(remote_branch).desired_width(220.0));
                });
                let ok = !remotes.is_empty() && valid_ref_name(remote_branch);
                ui.horizontal(|ui| {
                    if crate::ui::accent_button(ui, "Submit", theme::p().success, ok).clicked() || (enter && ok) {
                        actions.push(Action::PushTo {
                            remote: remote.clone(),
                            branch: branch.clone(),
                            remote_branch: remote_branch.clone(),
                            set_upstream: true,
                            force: false,
                        });
                        close = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            }
            Dialog::AddRemote { name, url } => {
                ui.heading("Adicionar remoto");
                ui.add(TextEdit::singleline(name).hint_text("origin").desired_width(f32::INFINITY)).request_focus();
                ui.add(TextEdit::singleline(url).hint_text("git@github.com:usuario/repo.git").desired_width(f32::INFINITY));
                let ok = valid_ref_name(name) && !url.trim().is_empty();
                ui.horizontal(|ui| {
                    if crate::ui::accent_button(ui, "Add remote", theme::p().success, ok).clicked() || (enter && ok) {
                        actions.push(Action::AddRemote { name: name.trim().into(), url: url.trim().into() });
                        close = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            }
            Dialog::Reword { message } => {
                ui.heading("Reword commit");
                ui.add(TextEdit::multiline(message).desired_rows(6).desired_width(f32::INFINITY)).request_focus();
                ui.horizontal(|ui| {
                    let ok = !message.trim().is_empty();
                    if crate::ui::accent_button(ui, "Update Message", theme::p().success, ok).clicked() {
                        actions.push(Action::RewordHead(message.clone()));
                        close = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            }
            Dialog::Error { title, message } => {
                ui.horizontal(|ui| {
                    crate::icons::paint(ui.painter(), egui::Rect::from_min_size(ui.cursor().min, egui::vec2(22.0, 22.0)), crate::icons::Icon::Warning, theme::p().danger);
                    ui.add_space(26.0);
                    ui.heading(RichText::new(title.as_str()).color(theme::p().danger));
                });
                egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    ui.label(RichText::new(message.as_str()).monospace());
                });
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() || enter {
                        close = true;
                    }
                    if ui.button("Copy").clicked() {
                        ui.ctx().copy_text(message.clone());
                    }
                });
            }
            Dialog::GitflowInit(cfg) => {
                ui.heading("Initialize Gitflow");
                egui::Grid::new("gitflow_init").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                    for (label, value) in [
                        ("Main/Master", &mut cfg.master),
                        ("Develop", &mut cfg.develop),
                        ("Feature prefix", &mut cfg.feature),
                        ("Release prefix", &mut cfg.release),
                        ("Hotfix prefix", &mut cfg.hotfix),
                        ("Version tag prefix", &mut cfg.version_tag),
                    ] {
                        ui.label(label);
                        ui.add(TextEdit::singleline(value).desired_width(220.0));
                        ui.end_row();
                    }
                });
                let ok = valid_ref_name(&cfg.master) && valid_ref_name(&cfg.develop) && cfg.master != cfg.develop;
                ui.horizontal(|ui| {
                    if crate::ui::accent_button(ui, "Initialize Gitflow", theme::p().success, ok).clicked() {
                        actions.push(Action::GitflowInit(cfg.clone()));
                        close = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            }
            Dialog::GitflowStart { kind, name } => {
                ui.heading(format!("Start {}", kind.label()));
                ui.add(TextEdit::singleline(name).hint_text(if *kind == FlowKind::Feature { "nome-da-feature" } else { "1.2.0" }).desired_width(f32::INFINITY))
                    .request_focus();
                let ok = valid_ref_name(name);
                ui.horizontal(|ui| {
                    if crate::ui::accent_button(ui, &format!("Start {}", kind.label()), theme::p().success, ok).clicked() || (enter && ok) {
                        actions.push(Action::GitflowStart { kind: *kind, name: name.trim().into() });
                        close = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            }
            Dialog::GitflowFinish { kind, name, tag_message, keep_branch } => {
                ui.heading(format!("Finish {} {name}", kind.label()));
                if kind.tags() {
                    ui.label("Mensagem da tag");
                    ui.add(TextEdit::multiline(tag_message).desired_rows(2).desired_width(f32::INFINITY));
                }
                ui.checkbox(keep_branch, "Manter o branch após o finish");
                ui.horizontal(|ui| {
                    if crate::ui::accent_button(ui, &format!("Finish {}", kind.label()), theme::p().success, true).clicked() {
                        actions.push(Action::GitflowFinish { kind: *kind, name: name.clone(), tag_message: tag_message.clone(), keep_branch: *keep_branch });
                        close = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            }
            Dialog::WorktreeAdd { path, branch, new_branch } => {
                ui.heading("Add worktree");
                ui.horizontal(|ui| {
                    ui.add(TextEdit::singleline(path).hint_text("/caminho/do/worktree").desired_width(320.0));
                    if ui.button("Browse").clicked()
                        && let Some(p) = crate::ui::home::pick_folder()
                    {
                        *path = p.display().to_string();
                    }
                });
                ui.add(TextEdit::singleline(branch).hint_text("branch").desired_width(f32::INFINITY));
                ui.checkbox(new_branch, "Criar novo branch a partir do HEAD");
                let ok = !path.trim().is_empty() && valid_ref_name(branch);
                ui.horizontal(|ui| {
                    if crate::ui::accent_button(ui, "Add worktree", theme::p().success, ok).clicked() {
                        actions.push(Action::WorktreeAdd { path: path.trim().into(), branch: branch.trim().into(), new_branch: *new_branch });
                        close = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            }
            Dialog::SubmoduleAdd { url, path } => {
                ui.heading("Add submodule");
                ui.add(TextEdit::singleline(url).hint_text("https://github.com/org/lib.git").desired_width(f32::INFINITY)).request_focus();
                ui.add(TextEdit::singleline(path).hint_text("vendor/lib").desired_width(f32::INFINITY));
                let ok = !url.trim().is_empty() && !path.trim().is_empty();
                ui.horizontal(|ui| {
                    if crate::ui::accent_button(ui, "Add submodule", theme::p().success, ok).clicked() {
                        actions.push(Action::SubmoduleAdd { url: url.trim().into(), path: path.trim().into() });
                        close = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            }
            Dialog::CreatePr { source, target, title, body, draft, push_first } => {
                let Some((kind, host, _, branches)) = &hosting else {
                    close = true;
                    return;
                };
                ui.heading(format!("Create {}", kind.pr_noun()));
                ui.label(RichText::new(format!("em {host}")).color(theme::p().text_secondary));
                egui::Grid::new("create_pr").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                    ui.label("From");
                    ui.label(RichText::new(source.as_str()).monospace().strong());
                    ui.end_row();
                    ui.label("To");
                    egui::ComboBox::from_id_salt("pr_target").selected_text(target.as_str()).width(260.0).show_ui(ui, |ui| {
                        for b in branches.iter().filter(|b| *b != source) {
                            ui.selectable_value(target, b.clone(), b);
                        }
                    });
                    ui.end_row();
                });
                ui.add(TextEdit::singleline(title).hint_text("Título").desired_width(f32::INFINITY));
                ui.add(TextEdit::multiline(body).hint_text("Descrição (markdown)").desired_rows(6).desired_width(f32::INFINITY));
                ui.checkbox(draft, "Criar como draft");
                ui.checkbox(push_first, format!("Fazer push de {source} antes (com upstream)"));
                let ok = !title.trim().is_empty() && !target.is_empty() && source != target;
                ui.horizontal(|ui| {
                    if crate::ui::accent_button(ui, &format!("Create {}", kind.pr_noun()), theme::p().success, ok).clicked() {
                        let pr = crate::hosting::NewPr { source: source.clone(), target: target.clone(), title: title.trim().into(), body: body.clone(), draft: *draft };
                        actions.push(Action::Hosting(crate::repo_hosting::HostingAction::CreatePr { pr, push_first: *push_first }));
                        close = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            }
            Dialog::ConnectHosting { token } => {
                let Some((kind, host, page, _)) = &hosting else {
                    close = true;
                    return;
                };
                ui.heading(format!("Conectar ao {kind_label}", kind_label = kind.label()));
                ui.label(format!("Cole um token de acesso pessoal de {host}."));
                ui.label(
                    RichText::new(match kind {
                        crate::hosting::Kind::GitHub => "Escopos necessários: repo, read:org.",
                        crate::hosting::Kind::GitLab => "Escopo necessário: api.",
                    })
                    .small()
                    .color(theme::p().text_secondary),
                );
                if ui.link("Abrir a página de criação de token no navegador").clicked() {
                    actions.push(Action::Hosting(crate::repo_hosting::HostingAction::OpenUrl(page.clone())));
                }
                ui.add(TextEdit::singleline(&mut token.0).password(true).hint_text("token").desired_width(f32::INFINITY)).request_focus();
                ui.label(RichText::new("O token fica no keyring do sistema (Secret Service), nunca em arquivo.").small().color(theme::p().text_disabled));
                let ok = !token.0.trim().is_empty();
                ui.horizontal(|ui| {
                    if crate::ui::accent_button(ui, "Conectar", theme::p().success, ok).clicked() || (enter && ok) {
                        actions.push(Action::Hosting(crate::repo_hosting::HostingAction::Connect(std::mem::take(token))));
                        close = true;
                    }
                    if ui.button("Cancel").clicked() {
                        token.0.clear();
                        close = true;
                    }
                });
            }
            Dialog::CreateIssue { title, body } => {
                ui.heading("Create issue");
                ui.add(TextEdit::singleline(title).hint_text("Título").desired_width(f32::INFINITY)).request_focus();
                ui.add(TextEdit::multiline(body).hint_text("Descrição").desired_rows(6).desired_width(f32::INFINITY));
                let ok = !title.trim().is_empty();
                ui.horizontal(|ui| {
                    if crate::ui::accent_button(ui, "Create issue", theme::p().success, ok).clicked() {
                        actions.push(Action::Hosting(crate::repo_hosting::HostingAction::CreateIssue { title: title.trim().into(), body: body.clone() }));
                        close = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            }
            Dialog::LfsTrack { pattern } => {
                ui.heading("Track with Git LFS");
                ui.add(TextEdit::singleline(pattern).hint_text("*.psd").desired_width(f32::INFINITY)).request_focus();
                let ok = !pattern.trim().is_empty();
                ui.horizontal(|ui| {
                    if crate::ui::accent_button(ui, "Track", theme::p().success, ok).clicked() || (enter && ok) {
                        actions.push(Action::LfsTrack(pattern.trim().into()));
                        close = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            }
        }
    });
    if close || resp.should_close() {
        tab.dialog = None;
    }
}
