//! Estado e ações de GitHub/GitLab de uma aba (PRs/MRs, issues, comentários).

use std::sync::mpsc::{Receiver, Sender, channel};

use crate::git::ops;
use crate::hosting::{self, CiState, CiStatus, Client, Comment, Issue, LineAnchor, MergeMethod, NewPr, PrReview, PullRequest, RemoteRepo, auth};
use crate::repo_tab::{CenterView, RepoTab};
use crate::settings::Settings;

pub enum Msg {
    Loaded {
        user: Result<String, String>,
        default_branch: Option<String>,
        prs: Result<Vec<PullRequest>, String>,
        issues: Result<Vec<Issue>, String>,
    },
    Comments { pr: bool, number: u64, comments: Result<Vec<Comment>, String> },
    Ci { number: u64, ci: Result<CiStatus, String> },
    Review { number: u64, review: Result<PrReview, String> },
    /// Operação terminada: (rótulo, resultado, recarregar listas, PR para abrir).
    Done { label: String, result: Result<String, String>, reload: bool, open_pr: Option<u64> },
    /// Fim de uma thread de rede (enviado sempre, por último).
    Finished,
}

pub struct HostingState {
    pub repo: RemoteRepo,
    pub connected: Option<auth::Source>,
    pub user: Option<String>,
    pub default_branch: Option<String>,
    pub prs: Option<Result<Vec<PullRequest>, String>>,
    pub issues: Option<Result<Vec<Issue>, String>>,
    pub loading: bool,
    /// Operações de rede em andamento (para spinners e para os testes).
    pub pending: usize,
    /// Estado geral de CI por PR/MR (bolinha na lista).
    pub ci_by_pr: std::collections::HashMap<u64, Option<CiState>>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrTab {
    Conversation,
    Files,
    Checks,
}

pub struct PrView {
    pub number: u64,
    pub comments: Option<Result<Vec<Comment>, String>>,
    pub new_comment: String,
    pub method: MergeMethod,
    pub tab: PrTab,
    pub ci: Option<Result<CiStatus, String>>,
    pub review: Option<Result<PrReview, String>>,
    pub file: usize,
    /// Comentário de revisão sendo escrito: (âncora, texto).
    pub draft: Option<(LineAnchor, String)>,
}

pub struct IssueView {
    pub number: u64,
    pub comments: Option<Result<Vec<Comment>, String>>,
    pub new_comment: String,
}

#[derive(Clone, Debug)]
pub enum HostingAction {
    Refresh,
    Connect(auth::Secret),
    Disconnect,
    OpenPr(u64),
    OpenIssue(u64),
    CheckoutPr(u64),
    CreatePr { pr: NewPr, push_first: bool },
    MergePr { number: u64, method: MergeMethod },
    CommentPr { number: u64, body: String },
    LoadReview(u64),
    ReviewComment { number: u64, anchor: LineAnchor, body: String },
    CommentIssue { number: u64, body: String },
    CreateIssue { title: String, body: String },
    BranchFromIssue(u64),
    Fork,
    OpenUrl(String),
}

impl HostingState {
    fn new(repo: RemoteRepo) -> Self {
        let (tx, rx) = channel();
        let connected = auth::token(repo.kind, &repo.host).map(|(_, s)| s);
        Self { repo, connected, user: None, default_branch: None, prs: None, issues: None, loading: false, pending: 0, ci_by_pr: Default::default(), tx, rx }
    }

    fn client(&self) -> Option<Client> {
        auth::token(self.repo.kind, &self.repo.host).map(|(t, _)| Client::new(self.repo.clone(), t))
    }

    pub fn pr(&self, number: u64) -> Option<&PullRequest> {
        self.prs.as_ref()?.as_ref().ok()?.iter().find(|p| p.number == number)
    }

    pub fn issue(&self, number: u64) -> Option<&Issue> {
        self.issues.as_ref()?.as_ref().ok()?.iter().find(|i| i.number == number)
    }

    fn spawn(&mut self, ctx: &egui::Context, f: impl FnOnce(&Sender<Msg>) + Send + 'static) {
        self.pending += 1;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            f(&tx);
            let _ = tx.send(Msg::Finished);
            ctx.request_repaint();
        });
    }
}

impl RepoTab {
    /// Detecta o provedor a partir dos remotos (prefere `origin`).
    pub fn detect_hosting(&mut self, settings: &Settings) {
        let Some(snap) = &self.snap else { return };
        let found = snap
            .remotes
            .iter()
            .filter(|r| r.name == "origin")
            .chain(snap.remotes.iter().filter(|r| r.name != "origin"))
            .find_map(|r| hosting::parse_remote(&r.name, &r.url, &settings.hosting_hosts));
        match (found, &self.hosting) {
            (Some(repo), Some(h)) if h.repo == repo => {}
            (Some(repo), _) => {
                self.hosting = Some(HostingState::new(repo));
                self.hosting_action(HostingAction::Refresh);
            }
            (None, _) => self.hosting = None,
        }
    }

    /// Coleta respostas das threads de rede.
    pub fn hosting_tick(&mut self) {
        let Some(h) = &mut self.hosting else { return };
        let msgs: Vec<Msg> = h.rx.try_iter().collect();
        let mut follow_up = Vec::new();
        for m in msgs {
            match m {
                Msg::Loaded { user, default_branch, prs, issues } => {
                    let h = self.hosting.as_mut().unwrap();
                    h.loading = false;
                    match user {
                        Ok(u) => {
                            h.user = Some(u);
                            // GitLab conectado também resolve fotos de autores por e-mail.
                            if h.repo.kind == hosting::Kind::GitLab
                                && let Some((token, _)) = auth::token(h.repo.kind, &h.repo.host)
                            {
                                crate::avatars::add_gitlab(crate::avatars::GitLabSource { api: h.repo.api.clone(), token });
                            }
                        }
                        Err(e) => {
                            // Token rejeitado: some do cache para o usuário reconectar.
                            if e.contains("401") {
                                auth::forget(&h.repo.host);
                                h.connected = None;
                            }
                        }
                    }
                    h.default_branch = default_branch;
                    if let Ok(list) = &prs {
                        let heads: Vec<(u64, Option<String>)> = list.iter().take(30).map(|p| (p.number, p.head_sha.clone())).collect();
                        if let Some(client) = h.client() {
                            let ctx = self.ctx().clone();
                            let h = self.hosting.as_mut().unwrap();
                            h.spawn(&ctx, move |tx| {
                                for (number, sha) in heads {
                                    let _ = tx.send(Msg::Ci { number, ci: client.pr_ci(number, sha.as_deref()).map_err(|e| e.to_string()) });
                                }
                            });
                        }
                    }
                    let h = self.hosting.as_mut().unwrap();
                    h.prs = Some(prs);
                    h.issues = Some(issues);
                }
                Msg::Ci { number, ci } => {
                    let state = ci.as_ref().ok().and_then(|c| c.overall);
                    self.hosting.as_mut().unwrap().ci_by_pr.insert(number, state);
                    if let Some(CenterView::PullRequest(v)) = &mut self.center
                        && v.number == number
                    {
                        v.ci = Some(ci);
                    }
                }
                Msg::Review { number, review } => {
                    if let Some(CenterView::PullRequest(v)) = &mut self.center
                        && v.number == number
                    {
                        v.review = Some(review);
                    }
                }
                Msg::Comments { pr, number, comments } => match &mut self.center {
                    Some(CenterView::PullRequest(v)) if pr && v.number == number => v.comments = Some(comments),
                    Some(CenterView::Issue(v)) if !pr && v.number == number => v.comments = Some(comments),
                    _ => {}
                },
                Msg::Finished => {
                    let h = self.hosting.as_mut().unwrap();
                    h.pending = h.pending.saturating_sub(1);
                }
                Msg::Done { label, result, reload, open_pr } => {
                    match result {
                        Ok(text) => self.notify(if text.is_empty() { format!("{label} concluído") } else { format!("{label}: {text}") }, false),
                        Err(e) => self.dialog = Some(crate::repo_tab::Dialog::Error { title: format!("{label} falhou"), message: e }),
                    }
                    if reload {
                        follow_up.push(HostingAction::Refresh);
                    }
                    if let Some(n) = open_pr {
                        follow_up.push(HostingAction::OpenPr(n));
                    }
                }
            }
        }
        for a in follow_up {
            self.hosting_action(a);
        }
    }

    pub fn hosting_action(&mut self, action: HostingAction) {
        let ctx = self.ctx().clone();
        let path = self.path.clone();
        let Some(h) = &mut self.hosting else { return };
        match action {
            HostingAction::Refresh => {
                h.connected = auth::token(h.repo.kind, &h.repo.host).map(|(_, s)| s);
                let Some(client) = h.client() else {
                    h.prs = None;
                    h.issues = None;
                    return;
                };
                h.loading = true;
                h.spawn(&ctx, move |tx| {
                    let user = client.current_user().map_err(|e| e.to_string());
                    let failed = user.as_ref().err().cloned();
                    let (default_branch, prs, issues) = match failed {
                        // Sem autenticação não adianta fazer as outras chamadas.
                        Some(e) => (None, Err(e.clone()), Err(e)),
                        None => (
                            client.default_branch().ok(),
                            client.list_prs().map_err(|e| e.to_string()),
                            client.list_issues().map_err(|e| e.to_string()),
                        ),
                    };
                    let _ = tx.send(Msg::Loaded { user, default_branch, prs, issues });
                });
            }
            HostingAction::Connect(token) => {
                let res = auth::save(&h.repo.host, &token.0);
                if let Err(e) = res {
                    self.notify(e.to_string(), true);
                }
                self.hosting_action(HostingAction::Refresh);
            }
            HostingAction::Disconnect => {
                auth::remove(&h.repo.host);
                h.connected = None;
                h.user = None;
                h.prs = None;
                h.issues = None;
            }
            HostingAction::OpenPr(number) => {
                self.center = Some(CenterView::PullRequest(Box::new(PrView {
                    number,
                    comments: None,
                    new_comment: String::new(),
                    method: MergeMethod::Merge,
                    tab: PrTab::Conversation,
                    ci: None,
                    review: None,
                    file: 0,
                    draft: None,
                })));
                let head = h.pr(number).and_then(|p| p.head_sha.clone());
                if let Some(client) = h.client() {
                    h.spawn(&ctx, move |tx| {
                        let _ = tx.send(Msg::Comments { pr: true, number, comments: client.pr_comments(number).map_err(|e| e.to_string()) });
                        let _ = tx.send(Msg::Ci { number, ci: client.pr_ci(number, head.as_deref()).map_err(|e| e.to_string()) });
                    });
                }
            }
            HostingAction::OpenIssue(number) => {
                self.center = Some(CenterView::Issue(Box::new(IssueView { number, comments: None, new_comment: String::new() })));
                if let Some(client) = h.client() {
                    h.spawn(&ctx, move |tx| {
                        let _ = tx.send(Msg::Comments { pr: false, number, comments: client.issue_comments(number).map_err(|e| e.to_string()) });
                    });
                }
            }
            HostingAction::CheckoutPr(number) => {
                let Some(pr) = h.pr(number).cloned() else { return };
                let repo = h.repo.clone();
                h.spawn(&ctx, move |tx| {
                    let result = (|| -> anyhow::Result<String> {
                        if pr.fork.is_none() {
                            // Mesmo repositório: branch local rastreando o remoto (push funciona).
                            ops::fetch_remote_branch(&path, &repo.remote, &pr.source)?;
                            ops::checkout_pr_branch(&path, &repo.remote, &pr.source)
                        } else {
                            let local = repo.pr_local_branch(number);
                            ops::fetch_refspec(&path, &repo.remote, &format!("+{}", repo.pr_refspec(number)))?;
                            ops::checkout(&path, &local)
                        }
                    })();
                    let _ = tx.send(Msg::Done { label: format!("Checkout {}{number}", repo.kind.pr_prefix()), result: result.map_err(|e| format!("{e:#}")), reload: false, open_pr: None });
                });
            }
            HostingAction::CreatePr { pr, push_first } => {
                let Some(client) = h.client() else { return };
                let remote = h.repo.remote.clone();
                let noun = h.repo.kind.pr_noun();
                h.spawn(&ctx, move |tx| {
                    let result = (|| -> anyhow::Result<(String, u64)> {
                        if push_first {
                            ops::push(&path, &remote, &pr.source, true, false)?;
                        }
                        let created = client.create_pr(&pr)?;
                        Ok((format!("{}{} criado", client.repo.kind.pr_prefix(), created.number), created.number))
                    })();
                    let (result, open) = match result {
                        Ok((t, n)) => (Ok(t), Some(n)),
                        Err(e) => (Err(format!("{e:#}")), None),
                    };
                    let _ = tx.send(Msg::Done { label: format!("Criar {noun}"), result, reload: true, open_pr: open });
                });
            }
            HostingAction::MergePr { number, method } => {
                let Some(client) = h.client() else { return };
                let noun = h.repo.kind.pr_noun();
                h.spawn(&ctx, move |tx| {
                    let result = client.merge_pr(number, method).map(|_| String::new()).map_err(|e| format!("{e:#}"));
                    let _ = tx.send(Msg::Done { label: format!("Merge do {noun}"), result, reload: true, open_pr: None });
                });
            }
            HostingAction::LoadReview(number) => {
                let Some(client) = h.client() else { return };
                h.spawn(&ctx, move |tx| {
                    let _ = tx.send(Msg::Review { number, review: client.pr_review(number).map_err(|e| format!("{e:#}")) });
                });
            }
            HostingAction::ReviewComment { number, anchor, body } => {
                let Some(client) = h.client() else { return };
                let refs = match &self.center {
                    Some(CenterView::PullRequest(v)) => v.review.as_ref().and_then(|r| r.as_ref().ok()).map(|r| r.refs.clone()),
                    _ => None,
                };
                let Some(refs) = refs else { return };
                if let Some(CenterView::PullRequest(v)) = &mut self.center {
                    v.draft = None;
                }
                let h = self.hosting.as_mut().unwrap();
                h.spawn(&ctx, move |tx| {
                    let result = client.add_review_comment(number, &refs, &anchor, &body).map(|_| String::new()).map_err(|e| format!("{e:#}"));
                    let ok = result.is_ok();
                    let _ = tx.send(Msg::Done { label: "Comentário de revisão".into(), result, reload: false, open_pr: None });
                    if ok {
                        let _ = tx.send(Msg::Review { number, review: client.pr_review(number).map_err(|e| format!("{e:#}")) });
                    }
                });
            }
            HostingAction::CommentPr { number, body } | HostingAction::CommentIssue { number, body } if body.trim().is_empty() => {
                let _ = number;
            }
            HostingAction::CommentPr { number, body } => {
                let Some(client) = h.client() else { return };
                h.spawn(&ctx, move |tx| {
                    let result = client.comment_pr(number, &body).map(|_| String::new()).map_err(|e| format!("{e:#}"));
                    let ok = result.is_ok();
                    let _ = tx.send(Msg::Done { label: "Comentário".into(), result, reload: false, open_pr: None });
                    if ok {
                        let _ = tx.send(Msg::Comments { pr: true, number, comments: client.pr_comments(number).map_err(|e| e.to_string()) });
                    }
                });
                if let Some(CenterView::PullRequest(v)) = &mut self.center {
                    v.new_comment.clear();
                }
            }
            HostingAction::CommentIssue { number, body } => {
                let Some(client) = h.client() else { return };
                h.spawn(&ctx, move |tx| {
                    let result = client.comment_issue(number, &body).map(|_| String::new()).map_err(|e| format!("{e:#}"));
                    let ok = result.is_ok();
                    let _ = tx.send(Msg::Done { label: "Comentário".into(), result, reload: false, open_pr: None });
                    if ok {
                        let _ = tx.send(Msg::Comments { pr: false, number, comments: client.issue_comments(number).map_err(|e| e.to_string()) });
                    }
                });
                if let Some(CenterView::Issue(v)) = &mut self.center {
                    v.new_comment.clear();
                }
            }
            HostingAction::CreateIssue { title, body } => {
                let Some(client) = h.client() else { return };
                h.spawn(&ctx, move |tx| {
                    let result = client.create_issue(&title, &body).map(|i| format!("issue #{} criada", i.number)).map_err(|e| format!("{e:#}"));
                    let _ = tx.send(Msg::Done { label: "Criar issue".into(), result, reload: true, open_pr: None });
                });
            }
            HostingAction::BranchFromIssue(number) => {
                let Some(issue) = h.issue(number) else { return };
                let name = hosting::issue_branch_name(issue);
                if let Some(at) = self.head_oid() {
                    self.dialog = Some(crate::repo_tab::Dialog::CreateBranch { at, name, checkout: true });
                }
            }
            HostingAction::Fork => {
                let Some(client) = h.client() else { return };
                let user = h.user.clone().unwrap_or_else(|| "fork".into());
                h.spawn(&ctx, move |tx| {
                    let result = (|| -> anyhow::Result<String> {
                        let url = client.fork()?;
                        ops::add_remote(&path, &user, &url)?;
                        // O fork pode levar alguns segundos para ficar disponível; o fetch é melhor-esforço.
                        let _ = ops::fetch_remote(&path, &user);
                        Ok(format!("remoto '{user}' adicionado"))
                    })();
                    let _ = tx.send(Msg::Done { label: "Fork".into(), result: result.map_err(|e| format!("{e:#}")), reload: false, open_pr: None });
                });
            }
            HostingAction::OpenUrl(url) => {
                if url.starts_with("https://") || url.starts_with("http://") {
                    crate::ui::open_path(std::ffi::OsStr::new(&url));
                }
            }
        }
    }
}
