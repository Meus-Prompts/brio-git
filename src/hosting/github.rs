//! GitHub REST API v3 (github.com e GitHub Enterprise).

use anyhow::Result;
use serde_json::{Value, json};

use super::{Check, CiState, CiStatus, Client, Comment, DiffRefs, Issue, LineAnchor, MergeMethod, NewPr, PrFile, PrReview, PrState, PullRequest, ReviewComment, parse_patch, path_str, s};

fn repo(c: &Client) -> String {
    format!("/repos/{}", c.repo.path)
}

fn pr(v: &Value, own: &str) -> PullRequest {
    let state = if !v.get("merged_at").is_none_or(Value::is_null) {
        PrState::Merged
    } else if s(v, "state") == "open" {
        PrState::Open
    } else {
        PrState::Closed
    };
    let head_repo = path_str(v, &["head", "repo", "full_name"]);
    PullRequest {
        number: v["number"].as_u64().unwrap_or(0),
        title: s(v, "title"),
        body: s(v, "body"),
        author: path_str(v, &["user", "login"]),
        source: path_str(v, &["head", "ref"]),
        target: path_str(v, &["base", "ref"]),
        state,
        draft: v["draft"].as_bool().unwrap_or(false),
        url: s(v, "html_url"),
        updated_at: s(v, "updated_at"),
        head_sha: Some(path_str(v, &["head", "sha"])).filter(|x| !x.is_empty()),
        fork: (!head_repo.is_empty() && !head_repo.eq_ignore_ascii_case(own)).then_some(head_repo),
    }
}

pub fn default_branch(c: &Client) -> Result<String> {
    Ok(s(&c.get("Repositório", &repo(c))?, "default_branch"))
}

pub fn list_prs(c: &Client) -> Result<Vec<PullRequest>> {
    let v = c.get("Pull requests", &format!("{}/pulls?state=open&per_page=100&sort=updated&direction=desc", repo(c)))?;
    Ok(v.as_array().map(|a| a.iter().map(|x| pr(x, &c.repo.path)).collect()).unwrap_or_default())
}

pub fn create_pr(c: &Client, p: &NewPr) -> Result<PullRequest> {
    let body = json!({ "title": p.title, "head": p.source, "base": p.target, "body": p.body, "draft": p.draft });
    Ok(pr(&c.post("Criar pull request", &format!("{}/pulls", repo(c)), body)?, &c.repo.path))
}

pub fn merge_pr(c: &Client, n: u64, m: MergeMethod) -> Result<()> {
    let method = match m {
        MergeMethod::Merge => "merge",
        MergeMethod::Squash => "squash",
        MergeMethod::Rebase => "rebase",
    };
    c.put("Merge do pull request", &format!("{}/pulls/{n}/merge", repo(c)), json!({ "merge_method": method }))?;
    Ok(())
}

pub fn comments(c: &Client, n: u64) -> Result<Vec<Comment>> {
    let v = c.get("Comentários", &format!("{}/issues/{n}/comments?per_page=100", repo(c)))?;
    Ok(v.as_array()
        .map(|a| a.iter().map(|x| Comment { author: path_str(x, &["user", "login"]), body: s(x, "body"), created_at: s(x, "created_at") }).collect())
        .unwrap_or_default())
}

pub fn comment(c: &Client, n: u64, body: &str) -> Result<()> {
    c.post("Comentar", &format!("{}/issues/{n}/comments", repo(c)), json!({ "body": body }))?;
    Ok(())
}

fn issue(v: &Value) -> Issue {
    Issue {
        number: v["number"].as_u64().unwrap_or(0),
        title: s(v, "title"),
        body: s(v, "body"),
        author: path_str(v, &["user", "login"]),
        labels: v["labels"].as_array().map(|a| a.iter().map(|l| s(l, "name")).collect()).unwrap_or_default(),
        assignees: v["assignees"].as_array().map(|a| a.iter().map(|u| s(u, "login")).collect()).unwrap_or_default(),
        url: s(v, "html_url"),
    }
}

pub fn list_issues(c: &Client) -> Result<Vec<Issue>> {
    let v = c.get("Issues", &format!("{}/issues?state=open&per_page=100&sort=updated", repo(c)))?;
    // A API de issues do GitHub também devolve PRs; eles têm a chave `pull_request`.
    Ok(v.as_array().map(|a| a.iter().filter(|x| x.get("pull_request").is_none()).map(issue).collect()).unwrap_or_default())
}

pub fn create_issue(c: &Client, title: &str, body: &str) -> Result<Issue> {
    Ok(issue(&c.post("Criar issue", &format!("{}/issues", repo(c)), json!({ "title": title, "body": body }))?))
}

pub fn fork(c: &Client) -> Result<String> {
    let v = c.post("Fork", &format!("{}/forks", repo(c)), json!({}))?;
    Ok(s(&v, "clone_url"))
}

fn check_state(status: &str, conclusion: &str) -> CiState {
    match (status, conclusion) {
        ("completed", "success") => CiState::Success,
        ("completed", "failure" | "timed_out" | "cancelled" | "action_required" | "startup_failure") => CiState::Failure,
        ("completed", _) => CiState::Neutral,
        _ => CiState::Pending,
    }
}

pub fn ci_status(c: &Client, sha: &str) -> Result<CiStatus> {
    let mut checks = Vec::new();
    let runs = c.get("Checks", &format!("{}/commits/{sha}/check-runs?per_page=100", repo(c)))?;
    for r in runs["check_runs"].as_array().into_iter().flatten() {
        checks.push(Check { name: s(r, "name"), state: check_state(&s(r, "status"), &s(r, "conclusion")), url: s(r, "html_url") });
    }
    // Statuses "clássicos" (CIs externos) complementam os check runs.
    let combined = c.get("Status", &format!("{}/commits/{sha}/status", repo(c)))?;
    for st in combined["statuses"].as_array().into_iter().flatten() {
        let state = match s(st, "state").as_str() {
            "success" => CiState::Success,
            "failure" | "error" => CiState::Failure,
            _ => CiState::Pending,
        };
        checks.push(Check { name: s(st, "context"), state, url: s(st, "target_url") });
    }
    Ok(CiStatus { overall: CiState::combine(checks.iter().map(|c| c.state)), checks, url: String::new() })
}

pub fn pr_review(c: &Client, n: u64) -> Result<PrReview> {
    let pr_json = c.get("Pull request", &format!("{}/pulls/{n}", repo(c)))?;
    let head = path_str(&pr_json, &["head", "sha"]);
    let files_json = c.get("Arquivos do PR", &format!("{}/pulls/{n}/files?per_page=100", repo(c)))?;
    let files = files_json
        .as_array()
        .into_iter()
        .flatten()
        .map(|f| {
            let path = s(f, "filename");
            let prev = s(f, "previous_filename");
            PrFile {
                old_path: if prev.is_empty() { path.clone() } else { prev },
                path,
                additions: f["additions"].as_u64().unwrap_or(0) as usize,
                deletions: f["deletions"].as_u64().unwrap_or(0) as usize,
                hunks: parse_patch(&s(f, "patch")),
            }
        })
        .collect();
    let comments_json = c.get("Comentários de revisão", &format!("{}/pulls/{n}/comments?per_page=100", repo(c)))?;
    let comments = comments_json
        .as_array()
        .into_iter()
        .flatten()
        .filter(|x| !x["line"].is_null())
        .map(|x| {
            let line = x["line"].as_u64().map(|l| l as u32);
            let left = s(x, "side") == "LEFT";
            ReviewComment {
                path: s(x, "path"),
                new_line: if left { None } else { line },
                old_line: if left { line } else { None },
                author: path_str(x, &["user", "login"]),
                body: s(x, "body"),
            }
        })
        .collect();
    Ok(PrReview { files, comments, refs: DiffRefs { head_sha: head, ..Default::default() } })
}

pub fn add_review_comment(c: &Client, n: u64, refs: &DiffRefs, at: &LineAnchor, body: &str) -> Result<()> {
    let (line, side) = match (at.new_line, at.old_line) {
        (Some(l), _) => (l, "RIGHT"),
        (None, Some(l)) => (l, "LEFT"),
        _ => return Err(anyhow::anyhow!("linha inválida")),
    };
    let payload = json!({ "body": body, "commit_id": refs.head_sha, "path": at.path, "line": line, "side": side });
    c.post("Comentário de revisão", &format!("{}/pulls/{n}/comments", repo(c)), payload)?;
    Ok(())
}
