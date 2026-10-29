//! UI de GitHub/GitLab: seções do painel esquerdo e views de PR/MR e issue.

use egui::{Align2, RichText, Sense, Ui, pos2, vec2};

use crate::hosting::{CiState, CiStatus, Comment, LineAnchor, MergeMethod, PrReview, PrState, PullRequest};
use crate::icons::{self, Icon};
use crate::repo_hosting::{HostingAction as H, PrTab};
use crate::repo_tab::{Action, CenterView, Dialog, RepoTab};
use crate::theme;

fn act(a: H) -> Action {
    Action::Hosting(a)
}

fn state_badge(ui: &mut Ui, pr: &PullRequest) {
    let (text, color) = match (pr.state, pr.draft) {
        (PrState::Open, true) => ("Draft", theme::p().text_secondary),
        (PrState::Open, false) => ("Open", theme::p().success),
        (PrState::Merged, _) => ("Merged", crate::theme::p().lanes[2]),
        (PrState::Closed, _) => ("Closed", theme::p().danger),
    };
    egui::Frame::new().fill(color.gamma_multiply(0.85)).corner_radius(10).inner_margin(egui::Margin::symmetric(8, 2)).show(ui, |ui| {
        ui.label(RichText::new(text).small().strong().color(egui::Color32::WHITE));
    });
}

/// Seções "PULL REQUESTS"/"MERGE REQUESTS" e "GITHUB ISSUES"/"GITLAB ISSUES".
pub fn left_sections(ui: &mut Ui, tab: &mut RepoTab, actions: &mut Vec<Action>) {
    let Some(h) = &tab.hosting else { return };
    let kind = h.repo.kind;
    let pr_title = match kind {
        crate::hosting::Kind::GitHub => "PULL REQUESTS",
        crate::hosting::Kind::GitLab => "MERGE REQUESTS",
    };
    let issues_title = format!("{} ISSUES", kind.label().to_uppercase());
    let pr_count = h.prs.as_ref().and_then(|r| r.as_ref().ok()).map(Vec::len).unwrap_or(0);
    let issue_count = h.issues.as_ref().and_then(|r| r.as_ref().ok()).map(Vec::len).unwrap_or(0);
    let connected = h.connected.is_some();
    let prefix = kind.pr_prefix();
    let repo = h.repo.clone();
    let prs = h.prs.clone();
    let issues = h.issues.clone();
    let loading = h.loading;

    let open = !tab.collapsed.contains(pr_title);
    let resp = crate::ui::section_header(ui, pr_title, open, Some(pr_count));
    if resp.clicked() {
        toggle(tab, pr_title);
    }
    resp.context_menu(|ui| {
        if ui.button(format!("Create {}", kind.pr_noun())).clicked() {
            actions.push(Action::OpenDialog(create_pr_dialog(tab, None)));
            ui.close();
        }
        if ui.button("Refresh").clicked() {
            actions.push(act(H::Refresh));
            ui.close();
        }
        if ui.button(format!("Open {} in browser", repo.path)).clicked() {
            actions.push(act(H::OpenUrl(format!("{}/{}", repo.web, repo.path))));
            ui.close();
        }
        if connected && ui.button(format!("Fork {} on {}", repo.path, kind.label())).clicked() {
            actions.push(act(H::Fork));
            ui.close();
        }
        if connected && ui.button(format!("Disconnect from {}", repo.host)).clicked() {
            actions.push(act(H::Disconnect));
            ui.close();
        }
    });
    if open {
        if !connected {
            connect_row(ui, &repo, actions);
        } else if loading && prs.is_none() {
            ui.horizontal(|ui| {
                ui.add_space(26.0);
                ui.spinner();
            });
        } else {
            match &prs {
                Some(Err(e)) => error_row(ui, e, actions),
                Some(Ok(list)) if list.is_empty() => hint_row(ui, &format!("Nenhum {} aberto", kind.pr_noun().to_lowercase())),
                Some(Ok(list)) => {
                    for pr in list {
                        pr_row(ui, tab, pr, prefix, actions);
                    }
                }
                None => {}
            }
        }
    }

    let open = !tab.collapsed.contains(&issues_title);
    let resp = crate::ui::section_header(ui, &issues_title, open, Some(issue_count));
    if resp.clicked() {
        toggle(tab, &issues_title);
    }
    resp.context_menu(|ui| {
        if ui.button("Create issue").clicked() {
            actions.push(Action::OpenDialog(Dialog::CreateIssue { title: String::new(), body: String::new() }));
            ui.close();
        }
        if ui.button("Refresh").clicked() {
            actions.push(act(H::Refresh));
            ui.close();
        }
    });
    if open && connected {
        match &issues {
            Some(Err(e)) => error_row(ui, e, actions),
            Some(Ok(list)) if list.is_empty() => hint_row(ui, "Nenhuma issue aberta"),
            Some(Ok(list)) => {
                for issue in list {
                    let selected = matches!(&tab.center, Some(CenterView::Issue(v)) if v.number == issue.number);
                    let resp = row(ui, &format!("#{} {}", issue.number, issue.title), Icon::Warning, selected, false);
                    if resp.clicked() {
                        actions.push(act(H::OpenIssue(issue.number)));
                    }
                    let num = issue.number;
                    let url = issue.url.clone();
                    resp.on_hover_text(format!("por {}{}", issue.author, if issue.labels.is_empty() { String::new() } else { format!(" · {}", issue.labels.join(", ")) }))
                        .context_menu(|ui| {
                            if ui.button("Create branch from issue").clicked() {
                                actions.push(act(H::BranchFromIssue(num)));
                                ui.close();
                            }
                            if ui.button("View issue in browser").clicked() {
                                actions.push(act(H::OpenUrl(url.clone())));
                                ui.close();
                            }
                            if ui.button("Copy issue link").clicked() {
                                actions.push(Action::CopyText(url.clone()));
                                ui.close();
                            }
                        });
                }
            }
            None => {}
        }
    }
}

fn toggle(tab: &mut RepoTab, key: &str) {
    if !tab.collapsed.remove(key) {
        tab.collapsed.insert(key.to_string());
    }
}

fn row(ui: &mut Ui, label: &str, icon: Icon, selected: bool, dim: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
    if selected {
        ui.painter().rect_filled(rect, 0.0, theme::p().selected_row);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 0.0, theme::p().hover_row);
    }
    icons::paint(ui.painter(), egui::Rect::from_center_size(pos2(rect.left() + 32.0, rect.center().y), vec2(12.0, 12.0)), icon, theme::p().text_secondary);
    let color = if dim { theme::p().text_secondary } else { theme::p().text };
    let text = crate::ui::elide(ui, label, &theme::prop(12.5), rect.width() - 66.0);
    ui.painter().text(pos2(rect.left() + 44.0, rect.center().y), Align2::LEFT_CENTER, text, theme::prop(12.5), color);
    resp
}

fn pr_row(ui: &mut Ui, tab: &RepoTab, pr: &PullRequest, prefix: &str, actions: &mut Vec<Action>) {
    let selected = matches!(&tab.center, Some(CenterView::PullRequest(v)) if v.number == pr.number);
    let label = format!("{prefix}{} {}", pr.number, pr.title);
    let resp = row(ui, &label, Icon::Branch, selected, pr.draft);
    let ci = tab.hosting.as_ref().and_then(|h| h.ci_by_pr.get(&pr.number).copied().flatten());
    if let Some(state) = ci {
        ui.painter().circle_filled(pos2(resp.rect.right() - 12.0, resp.rect.center().y), 4.0, ci_color(state));
    }
    if resp.clicked() {
        actions.push(act(H::OpenPr(pr.number)));
    }
    if resp.double_clicked() {
        actions.push(act(H::CheckoutPr(pr.number)));
    }
    let num = pr.number;
    let url = pr.url.clone();
    let from = pr.fork.as_ref().map(|f| format!("{f}:{}", pr.source)).unwrap_or_else(|| pr.source.clone());
    resp.on_hover_text(format!("{} → {} · por {}{} · atualizado {}", from, pr.target, pr.author, if pr.draft { " · draft" } else { "" }, pr.updated_at.replace('T', " ").chars().take(16).collect::<String>())).context_menu(|ui| {
        if ui.button("Checkout branch").clicked() {
            actions.push(act(H::CheckoutPr(num)));
            ui.close();
        }
        if ui.button("Open in browser").clicked() {
            actions.push(act(H::OpenUrl(url.clone())));
            ui.close();
        }
        if ui.button("Copy link").clicked() {
            actions.push(Action::CopyText(url.clone()));
            ui.close();
        }
    });
}

fn connect_row(ui: &mut Ui, repo: &crate::hosting::RemoteRepo, actions: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        ui.add_space(26.0);
        if ui.button(format!("Conectar ao {} ({})", repo.kind.label(), repo.host)).clicked() {
            actions.push(Action::OpenDialog(Dialog::ConnectHosting { token: Default::default() }));
        }
    });
}

fn error_row(ui: &mut Ui, e: &str, actions: &mut Vec<Action>) {
    ui.horizontal_wrapped(|ui| {
        ui.add_space(26.0);
        ui.label(RichText::new(e).small().color(theme::p().danger));
        if ui.small_button("Tentar de novo").clicked() {
            actions.push(act(H::Refresh));
        }
        if e.contains("401") && ui.small_button("Reconectar").clicked() {
            actions.push(Action::OpenDialog(Dialog::ConnectHosting { token: Default::default() }));
        }
    });
}

fn hint_row(ui: &mut Ui, text: &str) {
    ui.horizontal(|ui| {
        ui.add_space(26.0);
        ui.label(RichText::new(text).small().color(theme::p().text_disabled));
    });
}

/// Diálogo de criação de PR já preenchido a partir do branch.
pub fn create_pr_dialog(tab: &RepoTab, branch: Option<&str>) -> Dialog {
    let snap = tab.snap.as_ref();
    let source = branch.map(str::to_string).or_else(|| snap.and_then(|s| s.head.branch.clone())).unwrap_or_default();
    let target = tab.hosting.as_ref().and_then(|h| h.default_branch.clone()).unwrap_or_else(|| "main".into());
    let local = snap.and_then(|s| s.locals.iter().find(|b| b.name == source));
    // Título padrão: primeiro commit do branch, como o cliente de referência sugere.
    let title = local.and_then(|b| snap.and_then(|s| s.index_of.get(&b.oid).map(|&i| s.commits[i].summary.clone()))).unwrap_or_else(|| source.clone());
    let push_first = local.is_some_and(|b| b.upstream.is_none() || b.ahead > 0);
    Dialog::CreatePr { source, target, title, body: String::new(), draft: false, push_first }
}

fn comments(ui: &mut Ui, list: &Option<Result<Vec<Comment>, String>>) {
    match list {
        None => {
            ui.spinner();
        }
        Some(Err(e)) => {
            ui.colored_label(theme::p().danger, e);
        }
        Some(Ok(list)) if list.is_empty() => {
            ui.label(RichText::new("Sem comentários").color(theme::p().text_disabled));
        }
        Some(Ok(list)) => {
            for c in list {
                egui::Frame::new().fill(theme::p().panel_bg0).corner_radius(4).inner_margin(10).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
                        crate::ui::avatar(ui, r.center(), 10.0, &initials(&c.author), "", crate::ui::author_color(&c.author));
                        ui.label(RichText::new(&c.author).strong().color(theme::p().text_bright));
                        ui.label(RichText::new(c.created_at.replace('T', " ").chars().take(16).collect::<String>()).small().color(theme::p().text_secondary));
                    });
                    ui.label(&c.body);
                });
                ui.add_space(6.0);
            }
        }
    }
}

fn initials(name: &str) -> String {
    name.chars().take(2).flat_map(char::to_uppercase).collect()
}

pub fn ci_color(state: CiState) -> egui::Color32 {
    match state {
        CiState::Success => theme::p().success,
        CiState::Failure => theme::p().danger,
        CiState::Pending => theme::p().warning,
        CiState::Neutral => theme::p().text_secondary,
    }
}

fn ci_summary(ui: &mut Ui, ci: &Option<Result<CiStatus, String>>) {
    match ci {
        None => {
            ui.spinner();
        }
        Some(Err(_)) => {
            ui.label(RichText::new("CI indisponível").small().color(theme::p().text_disabled));
        }
        Some(Ok(c)) => match c.overall {
            None => {
                ui.label(RichText::new("Sem CI").small().color(theme::p().text_disabled));
            }
            Some(state) => {
                let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                ui.painter().circle_filled(r.center(), 5.0, ci_color(state));
                let failed = c.checks.iter().filter(|k| k.state == CiState::Failure).count();
                let text = if failed > 0 { format!("CI {} · {failed} falha(s)", state.label()) } else { format!("CI {} · {} check(s)", state.label(), c.checks.len()) };
                ui.label(RichText::new(text).small().color(ci_color(state)));
            }
        },
    }
}

pub fn pr_view(ui: &mut Ui, tab: &mut RepoTab, actions: &mut Vec<Action>) {
    let Some(h) = &tab.hosting else { return };
    let repo = h.repo.clone();
    let Some(CenterView::PullRequest(v)) = &mut tab.center else { return };
    let Some(pr) = h.pr(v.number).cloned() else {
        ui.horizontal(|ui| {
            if icons::button(ui, Icon::ArrowLeft, 24.0, "Voltar").clicked() {
                actions.push(Action::CloseCenter);
            }
            ui.label(format!("{}{} não está mais na lista de abertos.", repo.kind.pr_prefix(), v.number));
        });
        return;
    };
    egui::Frame::new().inner_margin(egui::Margin { left: 14, right: 14, top: 10, bottom: 0 }).show(ui, |ui| {
        ui.horizontal(|ui| {
            if icons::button(ui, Icon::ArrowLeft, 24.0, "Voltar ao grafo").clicked() {
                actions.push(Action::CloseCenter);
            }
            state_badge(ui, &pr);
            ui.label(RichText::new(format!("{}{}", repo.kind.pr_prefix(), pr.number)).size(18.0).color(theme::p().text_secondary));
            ui.label(RichText::new(&pr.title).size(18.0).strong().color(theme::p().text_bright));
        });
        ui.horizontal(|ui| {
            ui.add_space(30.0);
            let from = pr.fork.as_ref().map(|f| format!("{f}:{}", pr.source)).unwrap_or_else(|| pr.source.clone());
            ui.label(RichText::new(&pr.author).strong());
            ui.label("quer fazer merge de");
            ui.label(RichText::new(from).monospace().color(theme::p().link));
            ui.label("em");
            ui.label(RichText::new(&pr.target).monospace().color(theme::p().link));
            ui.separator();
            ci_summary(ui, &v.ci);
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add_space(30.0);
            if ui.button("Checkout branch").clicked() {
                actions.push(act(H::CheckoutPr(pr.number)));
            }
            if ui.button("Open in browser").clicked() {
                actions.push(act(H::OpenUrl(pr.url.clone())));
            }
            if pr.state == PrState::Open {
                ui.separator();
                egui::ComboBox::from_id_salt("merge_method").selected_text(v.method.label()).show_ui(ui, |ui| {
                    for m in [MergeMethod::Merge, MergeMethod::Squash, MergeMethod::Rebase] {
                        if repo.kind == crate::hosting::Kind::GitLab && m == MergeMethod::Rebase {
                            continue;
                        }
                        ui.selectable_value(&mut v.method, m, m.label());
                    }
                });
                let can_merge = !pr.draft;
                if crate::ui::accent_button(ui, &format!("Merge {}", repo.kind.pr_noun()), theme::p().success, can_merge).clicked() {
                    let failing = matches!(&v.ci, Some(Ok(c)) if c.overall == Some(CiState::Failure));
                    actions.push(Action::OpenDialog(Dialog::Confirm {
                        title: format!("Merge {}{}", repo.kind.pr_prefix(), pr.number),
                        body: format!(
                            "{} de {} em {} no {}?{}",
                            v.method.label(),
                            pr.source,
                            pr.target,
                            repo.kind.label(),
                            if failing { "\n\n⚠ O CI deste PR está falhando." } else { "" }
                        ),
                        confirm: "Merge".into(),
                        danger: failing,
                        action: Box::new(act(H::MergePr { number: pr.number, method: v.method })),
                    }));
                }
                if pr.draft {
                    ui.label(RichText::new("Draft não pode ser mergeado").small().color(theme::p().text_secondary));
                }
            }
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add_space(30.0);
            let files = v.review.as_ref().and_then(|r| r.as_ref().ok()).map(|r| format!("Arquivos ({})", r.files.len())).unwrap_or_else(|| "Arquivos".into());
            let checks = match &v.ci {
                Some(Ok(c)) => format!("Checks ({})", c.checks.len()),
                _ => "Checks".into(),
            };
            let before = v.tab;
            ui.selectable_value(&mut v.tab, PrTab::Conversation, "Conversa");
            ui.selectable_value(&mut v.tab, PrTab::Files, files);
            ui.selectable_value(&mut v.tab, PrTab::Checks, checks);
            if v.tab == PrTab::Files && before != PrTab::Files && v.review.is_none() {
                actions.push(act(H::LoadReview(pr.number)));
            }
        });
        ui.separator();
    });

    match v.tab {
        PrTab::Conversation => conversation(ui, &pr, v, actions),
        PrTab::Files => files_tab(ui, pr.number, v, actions),
        PrTab::Checks => checks_tab(ui, &v.ci, actions),
    }
}

fn conversation(ui: &mut Ui, pr: &PullRequest, v: &mut crate::repo_hosting::PrView, actions: &mut Vec<Action>) {
    egui::ScrollArea::vertical().id_salt("pr_conversation").auto_shrink([false, false]).show(ui, |ui| {
        egui::Frame::new().inner_margin(14).show(ui, |ui| {
            egui::Frame::new().fill(theme::p().panel_bg0).corner_radius(4).inner_margin(12).show(ui, |ui| {
                ui.set_width(ui.available_width());
                if pr.body.trim().is_empty() {
                    ui.label(RichText::new("Sem descrição").italics().color(theme::p().text_disabled));
                } else {
                    ui.label(&pr.body);
                }
            });
            ui.add_space(12.0);
            ui.label(RichText::new("Comentários").strong().color(theme::p().text_bright));
            comments(ui, &v.comments);
            ui.add(egui::TextEdit::multiline(&mut v.new_comment).hint_text("Deixe um comentário").desired_rows(3).desired_width(f32::INFINITY));
            if crate::ui::accent_button(ui, "Comment", theme::p().primary, !v.new_comment.trim().is_empty()).clicked() {
                actions.push(act(H::CommentPr { number: pr.number, body: v.new_comment.clone() }));
            }
        });
    });
}

fn checks_tab(ui: &mut Ui, ci: &Option<Result<CiStatus, String>>, actions: &mut Vec<Action>) {
    egui::Frame::new().inner_margin(14).show(ui, |ui| match ci {
        None => {
            ui.spinner();
        }
        Some(Err(e)) => {
            ui.colored_label(theme::p().danger, e);
        }
        Some(Ok(c)) if c.checks.is_empty() => {
            ui.label(RichText::new("Nenhum check/pipeline para o commit mais recente.").color(theme::p().text_secondary));
        }
        Some(Ok(c)) => {
            if !c.url.is_empty() && ui.link("Abrir pipeline no navegador").clicked() {
                actions.push(act(H::OpenUrl(c.url.clone())));
            }
            egui::ScrollArea::vertical().id_salt("pr_checks").auto_shrink([false, false]).show(ui, |ui| {
                // Falhas primeiro, depois em andamento.
                let mut list = c.checks.clone();
                list.sort_by_key(|k| match k.state {
                    CiState::Failure => 0,
                    CiState::Pending => 1,
                    CiState::Neutral => 2,
                    CiState::Success => 3,
                });
                for k in list {
                    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_filled(rect, 0.0, theme::p().hover_row);
                    }
                    ui.painter().circle_filled(rect.left_center() + vec2(10.0, 0.0), 5.0, ci_color(k.state));
                    ui.painter().text(rect.left_center() + vec2(24.0, 0.0), Align2::LEFT_CENTER, &k.name, theme::prop(12.5), theme::p().text);
                    ui.painter().text(rect.right_center() - vec2(8.0, 0.0), Align2::RIGHT_CENTER, k.state.label(), theme::prop(11.5), ci_color(k.state));
                    if !k.url.is_empty() && resp.on_hover_text("Abrir no navegador").clicked() {
                        actions.push(act(H::OpenUrl(k.url.clone())));
                    }
                }
            });
        }
    });
}

fn files_tab(ui: &mut Ui, number: u64, v: &mut crate::repo_hosting::PrView, actions: &mut Vec<Action>) {
    let review: PrReview = match &v.review {
        None => {
            ui.spinner();
            return;
        }
        Some(Err(e)) => {
            ui.colored_label(theme::p().danger, e);
            return;
        }
        Some(Ok(r)) => r.clone(),
    };
    if review.files.is_empty() {
        ui.label("Sem arquivos alterados");
        return;
    }
    v.file = v.file.min(review.files.len() - 1);
    egui::Panel::left("pr_files").resizable(true).default_size(260.0).frame(egui::Frame::new().fill(theme::p().panel_bg0)).show(ui, |ui| {
        egui::ScrollArea::vertical().id_salt("pr_file_list").show(ui, |ui| {
            for (i, f) in review.files.iter().enumerate() {
                let n_comments = review.comments.iter().filter(|c| c.path == f.path).count();
                let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
                if i == v.file {
                    ui.painter().rect_filled(rect, 0.0, theme::p().selected_row);
                } else if resp.hovered() {
                    ui.painter().rect_filled(rect, 0.0, theme::p().hover_row);
                }
                let name = crate::ui::elide(ui, &f.path, &theme::prop(12.0), rect.width() - 90.0);
                ui.painter().text(rect.left_center() + vec2(8.0, 0.0), Align2::LEFT_CENTER, name, theme::prop(12.0), theme::p().text);
                let stats = format!("+{} −{}{}", f.additions, f.deletions, if n_comments > 0 { format!(" · {n_comments} com.") } else { String::new() });
                ui.painter().text(rect.right_center() - vec2(6.0, 0.0), Align2::RIGHT_CENTER, stats, theme::prop(11.0), theme::p().text_secondary);
                if resp.clicked() {
                    v.file = i;
                    v.draft = None;
                }
            }
        });
    });
    let file = &review.files[v.file];
    egui::ScrollArea::both().id_salt(("pr_diff", v.file)).auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        if file.hunks.is_empty() {
            ui.label("Diff indisponível (binário ou grande demais).");
        }
        for h in &file.hunks {
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().max(600.0), 22.0), Sense::hover());
            ui.painter().rect_filled(rect, 0.0, theme::p().panel_bg1);
            ui.painter().text(rect.left_center() + vec2(10.0, 0.0), Align2::LEFT_CENTER, &h.header, theme::mono(11.5), theme::p().text_secondary);
            for l in &h.lines {
                let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width().max(600.0), 19.0), Sense::click());
                let p = ui.painter();
                let bg = match l.origin {
                    '+' => theme::p().added_line,
                    '-' => theme::p().deleted_line,
                    _ => egui::Color32::TRANSPARENT,
                };
                p.rect_filled(rect, 0.0, bg);
                let num = |n: Option<u32>| n.map(|n| n.to_string()).unwrap_or_default();
                p.text(pos2(rect.left() + 36.0, rect.center().y), Align2::RIGHT_CENTER, num(l.old_no), theme::mono(11.0), theme::p().text_disabled);
                p.text(pos2(rect.left() + 74.0, rect.center().y), Align2::RIGHT_CENTER, num(l.new_no), theme::mono(11.0), theme::p().text_disabled);
                p.text(pos2(rect.left() + 104.0, rect.center().y), Align2::LEFT_CENTER, format!("{} {}", l.origin, l.content.replace('\t', "    ")), theme::mono(12.0), theme::p().text);
                if resp.hovered() {
                    // "+" para comentar na linha (como na revisão dos provedores).
                    let plus = egui::Rect::from_center_size(pos2(rect.left() + 88.0, rect.center().y), vec2(14.0, 14.0));
                    p.rect_filled(plus, 3.0, theme::p().primary);
                    icons::paint(p, plus.shrink(3.0), Icon::Plus, egui::Color32::WHITE);
                }
                let anchor = LineAnchor {
                    path: file.path.clone(),
                    old_path: file.old_path.clone(),
                    new_line: l.new_no,
                    old_line: if l.new_no.is_none() { l.old_no } else { None },
                };
                if resp.clicked() {
                    v.draft = Some((anchor.clone(), String::new()));
                }
                // Comentários existentes ancorados nesta linha.
                for c in review.comments.iter().filter(|c| {
                    c.path == file.path && ((c.new_line.is_some() && c.new_line == l.new_no) || (c.new_line.is_none() && c.old_line.is_some() && c.old_line == l.old_no && l.new_no.is_none()))
                }) {
                    egui::Frame::new().fill(theme::p().panel_bg1).stroke(egui::Stroke::new(1.0, theme::p().panel_bg2)).corner_radius(4).inner_margin(8).outer_margin(egui::Margin { left: 100, right: 10, top: 2, bottom: 4 }).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new(&c.author).strong().color(theme::p().text_bright));
                        ui.label(&c.body);
                    });
                }
                if let Some((at, text)) = &mut v.draft
                    && at.path == file.path
                    && at.new_line == anchor.new_line
                    && at.old_line == anchor.old_line
                {
                    let mut submit = false;
                    let mut cancel = false;
                    egui::Frame::new().fill(theme::p().panel_bg1).stroke(egui::Stroke::new(1.0, theme::p().primary)).corner_radius(4).inner_margin(8).outer_margin(egui::Margin { left: 100, right: 10, top: 2, bottom: 4 }).show(ui, |ui| {
                        ui.add(egui::TextEdit::multiline(text).hint_text("Comentário de revisão nesta linha").desired_rows(3).desired_width(f32::INFINITY)).request_focus();
                        ui.horizontal(|ui| {
                            submit = crate::ui::accent_button(ui, "Comment", theme::p().primary, !text.trim().is_empty()).clicked();
                            cancel = ui.button("Cancel").clicked();
                        });
                    });
                    if submit {
                        actions.push(act(H::ReviewComment { number, anchor: at.clone(), body: text.clone() }));
                    }
                    if cancel {
                        v.draft = None;
                    }
                }
            }
            ui.add_space(8.0);
        }
    });
}

pub fn issue_view(ui: &mut Ui, tab: &mut RepoTab, actions: &mut Vec<Action>) {
    let Some(h) = &tab.hosting else { return };
    let Some(CenterView::Issue(v)) = &mut tab.center else { return };
    let Some(issue) = h.issue(v.number).cloned() else {
        actions.push(Action::CloseCenter);
        return;
    };
    egui::ScrollArea::vertical().id_salt("issue_view").auto_shrink([false, false]).show(ui, |ui| {
        egui::Frame::new().inner_margin(14).show(ui, |ui| {
            ui.horizontal(|ui| {
                if icons::button(ui, Icon::ArrowLeft, 24.0, "Voltar ao grafo").clicked() {
                    actions.push(Action::CloseCenter);
                }
                ui.label(RichText::new(format!("#{}", issue.number)).size(18.0).color(theme::p().text_secondary));
                ui.label(RichText::new(&issue.title).size(18.0).strong().color(theme::p().text_bright));
            });
            ui.horizontal_wrapped(|ui| {
                ui.add_space(30.0);
                ui.label(format!("aberta por {}", issue.author));
                for l in &issue.labels {
                    ui.label(RichText::new(l).small().background_color(theme::lane_bg(3)).color(theme::p().text_bright));
                }
                if !issue.assignees.is_empty() {
                    ui.label(RichText::new(format!("responsáveis: {}", issue.assignees.join(", "))).small().color(theme::p().text_secondary));
                }
            });
            ui.horizontal(|ui| {
                ui.add_space(30.0);
                if ui.button("Create branch from issue").clicked() {
                    actions.push(act(H::BranchFromIssue(issue.number)));
                }
                if ui.button("Open in browser").clicked() {
                    actions.push(act(H::OpenUrl(issue.url.clone())));
                }
            });
            ui.add_space(12.0);
            egui::Frame::new().fill(theme::p().panel_bg0).corner_radius(4).inner_margin(12).show(ui, |ui| {
                ui.set_width(ui.available_width());
                if issue.body.trim().is_empty() {
                    ui.label(RichText::new("Sem descrição").italics().color(theme::p().text_disabled));
                } else {
                    ui.label(&issue.body);
                }
            });
            ui.add_space(12.0);
            ui.label(RichText::new("Comentários").strong().color(theme::p().text_bright));
            comments(ui, &v.comments);
            ui.add(egui::TextEdit::multiline(&mut v.new_comment).hint_text("Deixe um comentário").desired_rows(3).desired_width(f32::INFINITY));
            if crate::ui::accent_button(ui, "Comment", theme::p().primary, !v.new_comment.trim().is_empty()).clicked() {
                actions.push(act(H::CommentIssue { number: issue.number, body: v.new_comment.clone() }));
            }
        });
    });
}
