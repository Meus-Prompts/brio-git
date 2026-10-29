//! Integração com GitHub e GitLab (inclusive Enterprise / self-hosted):
//! pull/merge requests, issues, comentários, merge e fork.

pub mod auth;
mod github;
mod gitlab;

use std::time::Duration;

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    GitHub,
    GitLab,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::GitHub => "GitHub",
            Kind::GitLab => "GitLab",
        }
    }

    /// "Pull Request" no GitHub, "Merge Request" no GitLab.
    pub fn pr_noun(self) -> &'static str {
        match self {
            Kind::GitHub => "Pull Request",
            Kind::GitLab => "Merge Request",
        }
    }

    pub fn pr_prefix(self) -> &'static str {
        match self {
            Kind::GitHub => "#",
            Kind::GitLab => "!",
        }
    }
}

/// Host configurado manualmente (Enterprise / self-hosted ou API custom).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HostConfig {
    pub host: String,
    pub kind: Kind,
    /// URL base da API; vazio = padrão do provedor.
    #[serde(default)]
    pub api: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RemoteRepo {
    pub kind: Kind,
    pub host: String,
    /// `owner/repo` (GitHub) ou `grupo/subgrupo/projeto` (GitLab).
    pub path: String,
    pub api: String,
    pub web: String,
    pub remote: String,
}

impl RemoteRepo {
    pub fn web_commit(&self, sha: &str) -> String {
        match self.kind {
            Kind::GitHub => format!("{}/{}/commit/{sha}", self.web, self.path),
            Kind::GitLab => format!("{}/{}/-/commit/{sha}", self.web, self.path),
        }
    }

    /// Página para criar um token com os escopos necessários.
    pub fn token_page(&self) -> String {
        match self.kind {
            Kind::GitHub => format!("{}/settings/tokens/new?scopes=repo,read:org&description=brio-git", self.web),
            Kind::GitLab => format!("{}/-/user_settings/personal_access_tokens?name=brio-git&scopes=api", self.web),
        }
    }

    /// Refspec para buscar o head do PR/MR (funciona também para forks).
    pub fn pr_refspec(&self, number: u64) -> String {
        match self.kind {
            Kind::GitHub => format!("pull/{number}/head:pr/{number}"),
            Kind::GitLab => format!("merge-requests/{number}/head:mr/{number}"),
        }
    }

    pub fn pr_local_branch(&self, number: u64) -> String {
        match self.kind {
            Kind::GitHub => format!("pr/{number}"),
            Kind::GitLab => format!("mr/{number}"),
        }
    }
}

/// Extrai host e caminho de URLs `https://`, `ssh://` e scp-like (`git@host:path`).
fn split_url(url: &str) -> Option<(String, String)> {
    let url = url.trim();
    let (host, path) = if let Some(rest) = url.split_once("://").map(|(_, r)| r) {
        let rest = rest.rsplit_once('@').map(|(_, r)| r).unwrap_or(rest);
        let (host, path) = rest.split_once('/')?;
        (host.split(':').next()?.to_string(), path.to_string())
    } else {
        let rest = url.rsplit_once('@').map(|(_, r)| r).unwrap_or(url);
        let (host, path) = rest.split_once(':')?;
        (host.to_string(), path.to_string())
    };
    let path = path.trim_matches('/').trim_end_matches(".git").to_string();
    (!host.is_empty() && path.contains('/')).then_some((host.to_lowercase(), path))
}

pub fn parse_remote(remote: &str, url: &str, hosts: &[HostConfig]) -> Option<RemoteRepo> {
    let (host, path) = split_url(url)?;
    let configured = hosts.iter().find(|h| h.host.eq_ignore_ascii_case(&host));
    let kind = match configured {
        Some(h) => h.kind,
        None if host == "github.com" || host.contains("github") => Kind::GitHub,
        None if host.contains("gitlab") => Kind::GitLab,
        None => return None,
    };
    let web = format!("https://{host}");
    let default_api = match kind {
        Kind::GitHub if host == "github.com" => "https://api.github.com".to_string(),
        Kind::GitHub => format!("{web}/api/v3"),
        Kind::GitLab => format!("{web}/api/v4"),
    };
    let api = configured.map(|h| h.api.trim().trim_end_matches('/').to_string()).filter(|a| !a.is_empty()).unwrap_or(default_api);
    Some(RemoteRepo { kind, host, path, api, web, remote: remote.to_string() })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrState {
    Open,
    Merged,
    Closed,
}

#[derive(Clone, Debug)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub author: String,
    pub source: String,
    pub target: String,
    pub state: PrState,
    pub draft: bool,
    pub url: String,
    pub updated_at: String,
    pub head_sha: Option<String>,
    /// Caminho do repositório de origem quando vem de um fork.
    pub fork: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Issue {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub author: String,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    pub url: String,
}

#[derive(Clone, Debug)]
pub struct Comment {
    pub author: String,
    pub body: String,
    pub created_at: String,
}

#[derive(Clone, Debug)]
pub struct NewPr {
    pub source: String,
    pub target: String,
    pub title: String,
    pub body: String,
    pub draft: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeMethod {
    Merge,
    Squash,
    Rebase,
}

impl MergeMethod {
    pub fn label(self) -> &'static str {
        match self {
            MergeMethod::Merge => "Merge commit",
            MergeMethod::Squash => "Squash and merge",
            MergeMethod::Rebase => "Rebase and merge",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CiState {
    Success,
    Failure,
    Pending,
    Neutral,
}

impl CiState {
    /// Combina estados: falha domina, depois pendente, depois sucesso.
    pub fn combine(states: impl IntoIterator<Item = CiState>) -> Option<CiState> {
        let v: Vec<CiState> = states.into_iter().collect();
        if v.is_empty() {
            None
        } else if v.contains(&CiState::Failure) {
            Some(CiState::Failure)
        } else if v.contains(&CiState::Pending) {
            Some(CiState::Pending)
        } else if v.contains(&CiState::Success) {
            Some(CiState::Success)
        } else {
            Some(CiState::Neutral)
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            CiState::Success => "sucesso",
            CiState::Failure => "falhou",
            CiState::Pending => "em andamento",
            CiState::Neutral => "neutro",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Check {
    pub name: String,
    pub state: CiState,
    pub url: String,
}

#[derive(Clone, Debug)]
pub struct CiStatus {
    pub overall: Option<CiState>,
    pub checks: Vec<Check>,
    /// Link do pipeline (GitLab) quando existe.
    pub url: String,
}

/// Arquivo alterado no PR/MR, com o diff já parseado.
#[derive(Clone, Debug)]
pub struct PrFile {
    pub path: String,
    pub old_path: String,
    pub additions: usize,
    pub deletions: usize,
    pub hunks: Vec<crate::git::diff::Hunk>,
}

/// Comentário de revisão ancorado numa linha.
#[derive(Clone, Debug)]
pub struct ReviewComment {
    pub path: String,
    /// Linha no arquivo novo (`Some`) ou no antigo (`old_line`).
    pub new_line: Option<u32>,
    pub old_line: Option<u32>,
    pub author: String,
    pub body: String,
}

/// Dados para ancorar comentários (GitLab exige os três SHAs do diff).
#[derive(Clone, Debug, Default)]
pub struct DiffRefs {
    pub base_sha: String,
    pub start_sha: String,
    pub head_sha: String,
}

#[derive(Clone, Debug)]
pub struct PrReview {
    pub files: Vec<PrFile>,
    pub comments: Vec<ReviewComment>,
    pub refs: DiffRefs,
}

/// Onde um novo comentário de revisão será ancorado.
#[derive(Clone, Debug)]
pub struct LineAnchor {
    pub path: String,
    pub old_path: String,
    pub new_line: Option<u32>,
    pub old_line: Option<u32>,
}

/// Parser de diff unificado (o `patch` do GitHub / `diff` do GitLab).
pub fn parse_patch(patch: &str) -> Vec<crate::git::diff::Hunk> {
    use crate::git::diff::{DiffLine, Hunk};
    let mut hunks: Vec<Hunk> = Vec::new();
    let (mut old_no, mut new_no) = (0u32, 0u32);
    for line in patch.lines() {
        if let Some(rest) = line.strip_prefix("@@") {
            let nums: Vec<u32> = rest
                .split(['-', '+', ',', ' ', '@'])
                .filter_map(|t| t.parse().ok())
                .collect();
            // "@@ -a,b +c,d @@" ou "@@ -a +c @@" (sem contagem quando é 1).
            let has_old_count = rest.trim_start().strip_prefix('-').and_then(|r| r.split(' ').next()).is_some_and(|t| t.contains(','));
            let new_idx = if has_old_count { 2 } else { 1 };
            old_no = nums.first().copied().unwrap_or(1);
            new_no = nums.get(new_idx).copied().unwrap_or(1);
            hunks.push(Hunk { header: line.to_string(), old_start: old_no, new_start: new_no, lines: Vec::new() });
            continue;
        }
        let Some(h) = hunks.last_mut() else { continue };
        let (origin, content) = match line.chars().next() {
            Some(c @ ('+' | '-' | ' ')) => (c, &line[1..]),
            Some('\\') => {
                if let Some(l) = h.lines.last_mut() {
                    l.no_eol = true;
                }
                continue;
            }
            _ => (' ', line),
        };
        let (o, n) = match origin {
            '+' => (None, Some(new_no)),
            '-' => (Some(old_no), None),
            _ => (Some(old_no), Some(new_no)),
        };
        if o.is_some() {
            old_no += 1;
        }
        if n.is_some() {
            new_no += 1;
        }
        h.lines.push(DiffLine { origin, old_no: o, new_no: n, content: content.to_string(), no_eol: false });
    }
    hunks
}

/// Cliente autenticado para um repositório remoto.
pub struct Client {
    pub repo: RemoteRepo,
    token: String,
    agent: ureq::Agent,
}

impl Client {
    pub fn new(repo: RemoteRepo, token: String) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(25)))
            .user_agent("brio-git")
            .build()
            .into();
        Self { repo, token, agent }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.repo.api)
    }

    fn auth<B>(&self, req: ureq::RequestBuilder<B>) -> ureq::RequestBuilder<B> {
        match self.repo.kind {
            Kind::GitHub => req
                .header("Authorization", &format!("Bearer {}", self.token))
                .header("Accept", "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28"),
            Kind::GitLab => req.header("PRIVATE-TOKEN", &self.token),
        }
    }

    fn finish(&self, what: &str, resp: Result<ureq::http::Response<ureq::Body>, ureq::Error>) -> Result<serde_json::Value> {
        let mut resp = resp.map_err(|e| anyhow!("{what}: falha de rede: {e}"))?;
        let status = resp.status().as_u16();
        let text = resp.body_mut().read_to_string().unwrap_or_default();
        let json: serde_json::Value = if text.trim().is_empty() { serde_json::Value::Null } else { serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text.clone())) };
        if (200..300).contains(&status) {
            return Ok(json);
        }
        let msg = json
            .get("message")
            .or_else(|| json.get("error"))
            .map(|m| if let Some(s) = m.as_str() { s.to_string() } else { m.to_string() })
            .unwrap_or_else(|| text.chars().take(300).collect());
        let hint = match status {
            401 => " — token inválido ou expirado",
            403 => " — token sem permissão (escopos) ou limite de requisições",
            404 => " — repositório não encontrado ou sem acesso",
            _ => "",
        };
        Err(anyhow!("{what}: HTTP {status}{hint}: {msg}"))
    }

    pub fn get(&self, what: &str, path: &str) -> Result<serde_json::Value> {
        let resp = self.auth(self.agent.get(self.url(path))).call();
        self.finish(what, resp)
    }

    fn send(&self, what: &str, method: &str, path: &str, body: serde_json::Value) -> Result<serde_json::Value> {
        let url = self.url(path);
        let req = match method {
            "POST" => self.agent.post(url),
            "PUT" => self.agent.put(url),
            _ => self.agent.patch(url),
        };
        let resp = self.auth(req).send_json(body);
        self.finish(what, resp)
    }

    pub fn post(&self, what: &str, path: &str, body: serde_json::Value) -> Result<serde_json::Value> {
        self.send(what, "POST", path, body)
    }

    pub fn put(&self, what: &str, path: &str, body: serde_json::Value) -> Result<serde_json::Value> {
        self.send(what, "PUT", path, body)
    }

    // ---- API unificada ----------------------------------------------------

    pub fn current_user(&self) -> Result<String> {
        let v = self.get("Usuário", "/user")?;
        Ok(v.get(match self.repo.kind {
            Kind::GitHub => "login",
            Kind::GitLab => "username",
        })
        .and_then(|s| s.as_str())
        .unwrap_or("?")
        .to_string())
    }

    pub fn default_branch(&self) -> Result<String> {
        match self.repo.kind {
            Kind::GitHub => github::default_branch(self),
            Kind::GitLab => gitlab::default_branch(self),
        }
    }

    pub fn list_prs(&self) -> Result<Vec<PullRequest>> {
        match self.repo.kind {
            Kind::GitHub => github::list_prs(self),
            Kind::GitLab => gitlab::list_prs(self),
        }
    }

    pub fn create_pr(&self, pr: &NewPr) -> Result<PullRequest> {
        match self.repo.kind {
            Kind::GitHub => github::create_pr(self, pr),
            Kind::GitLab => gitlab::create_pr(self, pr),
        }
    }

    pub fn merge_pr(&self, number: u64, method: MergeMethod) -> Result<()> {
        match self.repo.kind {
            Kind::GitHub => github::merge_pr(self, number, method),
            Kind::GitLab => gitlab::merge_pr(self, number, method),
        }
    }

    pub fn pr_comments(&self, number: u64) -> Result<Vec<Comment>> {
        match self.repo.kind {
            Kind::GitHub => github::comments(self, number),
            Kind::GitLab => gitlab::mr_notes(self, number),
        }
    }

    pub fn issue_comments(&self, number: u64) -> Result<Vec<Comment>> {
        match self.repo.kind {
            Kind::GitHub => github::comments(self, number),
            Kind::GitLab => gitlab::issue_notes(self, number),
        }
    }

    pub fn comment_pr(&self, number: u64, body: &str) -> Result<()> {
        match self.repo.kind {
            Kind::GitHub => github::comment(self, number, body),
            Kind::GitLab => gitlab::comment(self, "merge_requests", number, body),
        }
    }

    pub fn comment_issue(&self, number: u64, body: &str) -> Result<()> {
        match self.repo.kind {
            Kind::GitHub => github::comment(self, number, body),
            Kind::GitLab => gitlab::comment(self, "issues", number, body),
        }
    }

    pub fn list_issues(&self) -> Result<Vec<Issue>> {
        match self.repo.kind {
            Kind::GitHub => github::list_issues(self),
            Kind::GitLab => gitlab::list_issues(self),
        }
    }

    pub fn create_issue(&self, title: &str, body: &str) -> Result<Issue> {
        match self.repo.kind {
            Kind::GitHub => github::create_issue(self, title, body),
            Kind::GitLab => gitlab::create_issue(self, title, body),
        }
    }

    /// CI de um PR/MR (no GitLab inclui pipelines de merge request).
    pub fn pr_ci(&self, number: u64, head_sha: Option<&str>) -> Result<CiStatus> {
        match (self.repo.kind, head_sha) {
            (Kind::GitHub, Some(sha)) => github::ci_status(self, sha),
            (Kind::GitHub, None) => Ok(CiStatus { overall: None, checks: Vec::new(), url: String::new() }),
            (Kind::GitLab, sha) => gitlab::mr_ci_status(self, number, sha),
        }
    }

    /// Arquivos, diff e comentários de revisão do PR/MR.
    pub fn pr_review(&self, number: u64) -> Result<PrReview> {
        match self.repo.kind {
            Kind::GitHub => github::pr_review(self, number),
            Kind::GitLab => gitlab::mr_review(self, number),
        }
    }

    pub fn add_review_comment(&self, number: u64, refs: &DiffRefs, at: &LineAnchor, body: &str) -> Result<()> {
        match self.repo.kind {
            Kind::GitHub => github::add_review_comment(self, number, refs, at, body),
            Kind::GitLab => gitlab::add_review_comment(self, number, refs, at, body),
        }
    }

    /// Cria um fork e devolve a URL de clone (https) do fork.
    pub fn fork(&self) -> Result<String> {
        match self.repo.kind {
            Kind::GitHub => github::fork(self),
            Kind::GitLab => gitlab::fork(self),
        }
    }
}

pub(crate) fn s(v: &serde_json::Value, key: &str) -> String {
    v.get(key).and_then(|x| x.as_str()).unwrap_or_default().to_string()
}

pub(crate) fn path_str(v: &serde_json::Value, path: &[&str]) -> String {
    let mut cur = v;
    for k in path {
        match cur.get(k) {
            Some(x) => cur = x,
            None => return String::new(),
        }
    }
    cur.as_str().unwrap_or_default().to_string()
}

/// Nome de branch sugerido para trabalhar numa issue (`123-corrige-login`).
pub fn issue_branch_name(issue: &Issue) -> String {
    let mut slug = String::new();
    for c in issue.title.to_lowercase().chars() {
        let c = match c {
            'á' | 'à' | 'â' | 'ã' => 'a',
            'é' | 'ê' => 'e',
            'í' => 'i',
            'ó' | 'ô' | 'õ' => 'o',
            'ú' | 'ü' => 'u',
            'ç' => 'c',
            c => c,
        };
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug: String = slug.trim_matches('-').chars().take(40).collect();
    format!("{}-{}", issue.number, slug.trim_end_matches('-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_remote_urls() {
        let gh = parse_remote("origin", "git@github.com:JhonesBR/gitHydra.git", &[]).unwrap();
        assert_eq!((gh.kind, gh.path.as_str(), gh.api.as_str()), (Kind::GitHub, "JhonesBR/gitHydra", "https://api.github.com"));
        let gl = parse_remote("origin", "git@gitlab.saudetrevo.com.br:trevo/tech/web.git", &[]).unwrap();
        assert_eq!((gl.kind, gl.path.as_str(), gl.api.as_str()), (Kind::GitLab, "trevo/tech/web", "https://gitlab.saudetrevo.com.br/api/v4"));
        let https = parse_remote("up", "https://user@gitlab.com/g/sub/p", &[]).unwrap();
        assert_eq!((https.host.as_str(), https.path.as_str()), ("gitlab.com", "g/sub/p"));
        let ssh = parse_remote("o", "ssh://git@github.example.com:2222/org/r.git", &[]).unwrap();
        assert_eq!((ssh.kind, ssh.host.as_str(), ssh.api.as_str()), (Kind::GitHub, "github.example.com", "https://github.example.com/api/v3"));
        assert!(parse_remote("o", "git@code.acme.io:a/b.git", &[]).is_none(), "host desconhecido sem config");
        let cfg = [HostConfig { host: "code.acme.io".into(), kind: Kind::GitLab, api: "http://127.0.0.1:9/api/v4/".into() }];
        let custom = parse_remote("o", "git@code.acme.io:a/b.git", &cfg).unwrap();
        assert_eq!((custom.kind, custom.api.as_str()), (Kind::GitLab, "http://127.0.0.1:9/api/v4"));
    }

    #[test]
    fn parses_unified_patch_line_numbers() {
        let h = parse_patch("@@ -10,3 +10,4 @@ fn x\n a\n-b\n+B\n+C\n c\n@@ -40 +41 @@\n-z\n+Z\n");
        assert_eq!(h.len(), 2);
        let l = &h[0].lines;
        assert_eq!((l[0].old_no, l[0].new_no), (Some(10), Some(10)));
        assert_eq!((l[1].origin, l[1].old_no, l[1].new_no), ('-', Some(11), None));
        assert_eq!((l[3].origin, l[3].new_no), ('+', Some(12)));
        assert_eq!((l[4].old_no, l[4].new_no), (Some(12), Some(13)));
        assert_eq!((h[1].old_start, h[1].new_start), (40, 41), "hunk sem contagem");
    }

    #[test]
    fn ci_state_combination() {
        use CiState::*;
        assert_eq!(CiState::combine([Success, Pending, Failure]), Some(Failure));
        assert_eq!(CiState::combine([Success, Pending]), Some(Pending));
        assert_eq!(CiState::combine([Success, Neutral]), Some(Success));
        assert_eq!(CiState::combine([]), None);
    }

    #[test]
    fn issue_branch_names_are_slugged() {
        let i = Issue { number: 42, title: "Corrigir crash na tela de Login!".into(), body: String::new(), author: String::new(), labels: vec![], assignees: vec![], url: String::new() };
        assert_eq!(issue_branch_name(&i), "42-corrigir-crash-na-tela-de-login");
    }
}

#[cfg(test)]
pub mod mock;

#[cfg(test)]
mod api_tests {
    use super::mock::Mock;
    use super::*;

    fn client(kind: Kind, url: &str, path: &str) -> Client {
        let api = match kind {
            Kind::GitHub => url.to_string(),
            Kind::GitLab => format!("{url}/api/v4"),
        };
        let repo = RemoteRepo { kind, host: "test.local".into(), path: path.into(), api, web: "https://test.local".into(), remote: "origin".into() };
        Client::new(repo, "tok-123".into())
    }

    #[test]
    fn github_pull_requests_issues_and_merge() {
        let mock = Mock::start(Box::new(|m, p, _| match (m, p) {
            ("GET", "/repos/ana/app/pulls?state=open&per_page=100&sort=updated&direction=desc") => Some((200, r#"[
                {"number":7,"title":"Login","body":"desc","user":{"login":"bia"},"state":"open","draft":true,"merged_at":null,
                 "head":{"ref":"feat/login","sha":"abc","repo":{"full_name":"bia/app"}},"base":{"ref":"main"},
                 "html_url":"https://github.com/ana/app/pull/7","updated_at":"2026-10-01T00:00:00Z"}]"#.into())),
            ("POST", "/repos/ana/app/pulls") => Some((201, r#"{"number":8,"title":"Nova","user":{"login":"ana"},"state":"open",
                 "head":{"ref":"feat/x","repo":{"full_name":"ana/app"}},"base":{"ref":"main"},"html_url":"u"}"#.into())),
            ("PUT", "/repos/ana/app/pulls/7/merge") => Some((200, r#"{"merged":true}"#.into())),
            ("GET", "/repos/ana/app/issues?state=open&per_page=100&sort=updated") => Some((200, r#"[
                {"number":3,"title":"Bug","state":"open","user":{"login":"c"},"labels":[{"name":"bug"}],"assignees":[],"html_url":"i3"},
                {"number":7,"title":"é PR","state":"open","user":{"login":"bia"},"pull_request":{},"html_url":"p7"}]"#.into())),
            ("GET", "/user") => Some((401, r#"{"message":"Bad credentials"}"#.into())),
            _ => None,
        }));
        let c = client(Kind::GitHub, &mock.url, "ana/app");
        let prs = c.list_prs().unwrap();
        assert_eq!(prs.len(), 1);
        let p = &prs[0];
        assert_eq!((p.number, p.source.as_str(), p.target.as_str(), p.state, p.draft), (7, "feat/login", "main", PrState::Open, true));
        assert_eq!(p.fork.as_deref(), Some("bia/app"), "PR de fork detectado");

        let created = c.create_pr(&NewPr { source: "feat/x".into(), target: "main".into(), title: "Nova".into(), body: "b".into(), draft: false }).unwrap();
        assert_eq!(created.number, 8);
        c.merge_pr(7, MergeMethod::Squash).unwrap();
        let issues = c.list_issues().unwrap();
        assert_eq!(issues.iter().map(|i| i.number).collect::<Vec<_>>(), vec![3], "PRs filtrados da lista de issues");
        assert_eq!(issues[0].labels, vec!["bug"]);
        let err = c.current_user().unwrap_err().to_string();
        assert!(err.contains("401") && err.contains("token inválido") && err.contains("Bad credentials"), "{err}");
        assert!(!err.contains("tok-123"), "token não vaza em mensagens de erro");

        let reqs = mock.requests();
        let create = reqs.iter().find(|r| r.method == "POST").unwrap();
        let body: serde_json::Value = serde_json::from_str(&create.body).unwrap();
        assert_eq!((body["head"].as_str(), body["base"].as_str(), body["draft"].as_bool()), (Some("feat/x"), Some("main"), Some(false)));
        assert_eq!(create.header("authorization"), Some("Bearer tok-123"));
        let merge = reqs.iter().find(|r| r.method == "PUT").unwrap();
        let merge: serde_json::Value = serde_json::from_str(&merge.body).unwrap();
        assert_eq!(merge["merge_method"], "squash");
    }

    #[test]
    fn gitlab_merge_requests_with_nested_namespace() {
        let mock = Mock::start(Box::new(|m, p, _| match (m, p) {
            ("GET", "/api/v4/projects/trevo%2Ftech%2Fweb/merge_requests?state=opened&per_page=100&order_by=updated_at") => Some((200, r#"[
                {"iid":12,"title":"Draft: Pagamento","description":"d","author":{"username":"ana"},"state":"opened","draft":true,
                 "source_branch":"feat/pag","target_branch":"develop","web_url":"w","sha":"f00","source_project_id":1,"target_project_id":1}]"#.into())),
            ("POST", "/api/v4/projects/trevo%2Ftech%2Fweb/merge_requests") => Some((201, r#"{"iid":13,"title":"Draft: X","state":"opened",
                 "source_branch":"x","target_branch":"main","author":{"username":"ana"},"source_project_id":1,"target_project_id":1}"#.into())),
            ("PUT", "/api/v4/projects/trevo%2Ftech%2Fweb/merge_requests/12/merge") => Some((200, "{}".into())),
            ("GET", "/api/v4/projects/trevo%2Ftech%2Fweb/merge_requests/12/notes?sort=asc&per_page=100") => Some((200, r#"[
                {"body":"approved this merge request","system":true,"author":{"username":"bot"}},
                {"body":"LGTM","system":false,"author":{"username":"bia"},"created_at":"t"}]"#.into())),
            _ => None,
        }));
        let c = client(Kind::GitLab, &mock.url, "trevo/tech/web");
        let mrs = c.list_prs().unwrap();
        assert_eq!((mrs[0].number, mrs[0].target.as_str(), mrs[0].draft, mrs[0].fork.is_none()), (12, "develop", true, true));
        let new = c.create_pr(&NewPr { source: "x".into(), target: "main".into(), title: "X".into(), body: String::new(), draft: true }).unwrap();
        assert_eq!(new.number, 13);
        c.merge_pr(12, MergeMethod::Merge).unwrap();
        assert!(c.merge_pr(12, MergeMethod::Rebase).is_err(), "rebase-and-merge não suportado no GitLab");
        let notes = c.pr_comments(12).unwrap();
        assert_eq!(notes.iter().map(|n| n.body.as_str()).collect::<Vec<_>>(), vec!["LGTM"], "notas de sistema filtradas");

        let reqs = mock.requests();
        let post = reqs.iter().find(|r| r.method == "POST").unwrap();
        assert_eq!(post.header("private-token"), Some("tok-123"));
        let post: serde_json::Value = serde_json::from_str(&post.body).unwrap();
        assert_eq!(post["title"], "Draft: X", "draft vira prefixo no GitLab");
    }
}

#[cfg(test)]
mod live {
    use super::*;

    /// Leitura real (somente GET). Uso:
    /// `BRIO_GIT_LIVE_REMOTES="git@github.com:o/r.git git@gitlab.host:g/p.git" cargo test live_read_only -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_read_only() {
        let Ok(list) = std::env::var("BRIO_GIT_LIVE_REMOTES") else { return };
        for url in list.split_whitespace() {
            let repo = parse_remote("origin", url, &[]).expect("remote reconhecido");
            let Some((token, src)) = auth::token(repo.kind, &repo.host) else {
                println!("{} {}: sem token", repo.kind.label(), repo.host);
                continue;
            };
            let c = Client::new(repo.clone(), token);
            let user = c.current_user().map(|u| !u.is_empty());
            let branch = c.default_branch().map(|b| !b.is_empty());
            let prs = c.list_prs();
            let issues = c.list_issues();
            println!(
                "{} {} (token via {:?}): usuário ok={:?} default_branch ok={:?} | {}: {} | issues: {}",
                repo.kind.label(),
                repo.host,
                src,
                user,
                branch,
                repo.kind.pr_noun(),
                match &prs {
                    Ok(v) => format!("{} abertos, campos completos={}", v.len(), v.iter().all(|p| p.number > 0 && !p.source.is_empty() && !p.target.is_empty() && !p.url.is_empty())),
                    Err(e) => format!("erro: {e}"),
                },
                match &issues {
                    Ok(v) => format!("{} abertas", v.len()),
                    Err(e) => format!("erro: {e}"),
                },
            );
            if let Some(pr) = prs.ok().and_then(|v| v.into_iter().next()) {
                let ci = Some(c.pr_ci(pr.number, pr.head_sha.as_deref()));
                let review = c.pr_review(pr.number);
                println!(
                    "    1º {}: CI={} | revisão: {}",
                    repo.kind.pr_noun(),
                    match ci {
                        Some(Ok(ci)) => format!("{:?} com {} checks", ci.overall.map(|s| s.label()), ci.checks.len()),
                        Some(Err(e)) => format!("erro: {e}"),
                        None => "sem sha".into(),
                    },
                    match review {
                        Ok(r) => format!("{} arquivos, {} hunks, {} comentários em linha, refs ok={}", r.files.len(), r.files.iter().map(|f| f.hunks.len()).sum::<usize>(), r.comments.len(), !r.refs.head_sha.is_empty()),
                        Err(e) => format!("erro: {e}"),
                    }
                );
            }
        }
    }
}
