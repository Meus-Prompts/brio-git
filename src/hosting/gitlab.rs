//! GitLab REST API v4 (gitlab.com e self-hosted).

use anyhow::{Result, anyhow};
use serde_json::{Value, json};

use super::{Check, CiState, CiStatus, Client, Comment, DiffRefs, Issue, LineAnchor, MergeMethod, NewPr, PrFile, PrReview, PrState, PullRequest, ReviewComment, parse_patch, path_str, s};

/// O ID do projeto aceito pela API é o caminho completo URL-encoded.
fn project(c: &Client) -> String {
    let mut id = String::new();
    for b in c.repo.path.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => id.push(b as char),
            _ => id.push_str(&format!("%{b:02X}")),
        }
    }
    format!("/projects/{id}")
}

fn mr(v: &Value) -> PullRequest {
    let state = match s(v, "state").as_str() {
        "merged" => PrState::Merged,
        "opened" => PrState::Open,
        _ => PrState::Closed,
    };
    let fork = (v["source_project_id"] != v["target_project_id"]).then(|| path_str(v, &["source_project", "path_with_namespace"])).filter(|p| !p.is_empty());
    PullRequest {
        number: v["iid"].as_u64().unwrap_or(0),
        title: s(v, "title"),
        body: s(v, "description"),
        author: path_str(v, &["author", "username"]),
        source: s(v, "source_branch"),
        target: s(v, "target_branch"),
        state,
        draft: v["draft"].as_bool().or(v["work_in_progress"].as_bool()).unwrap_or(false),
        url: s(v, "web_url"),
        updated_at: s(v, "updated_at"),
        head_sha: Some(s(v, "sha")).filter(|x| !x.is_empty()),
        fork,
    }
}

pub fn default_branch(c: &Client) -> Result<String> {
    Ok(s(&c.get("Projeto", &project(c))?, "default_branch"))
}

pub fn list_prs(c: &Client) -> Result<Vec<PullRequest>> {
    let v = c.get("Merge requests", &format!("{}/merge_requests?state=opened&per_page=100&order_by=updated_at", project(c)))?;
    Ok(v.as_array().map(|a| a.iter().map(mr).collect()).unwrap_or_default())
}

pub fn create_pr(c: &Client, p: &NewPr) -> Result<PullRequest> {
    let title = if p.draft && !p.title.starts_with("Draft:") { format!("Draft: {}", p.title) } else { p.title.clone() };
    let body = json!({ "source_branch": p.source, "target_branch": p.target, "title": title, "description": p.body });
    Ok(mr(&c.post("Criar merge request", &format!("{}/merge_requests", project(c)), body)?))
}

pub fn merge_pr(c: &Client, n: u64, m: MergeMethod) -> Result<()> {
    if m == MergeMethod::Rebase {
        return Err(anyhow!("O GitLab não faz rebase-and-merge numa chamada só; use Merge ou Squash (ou rebase local + push)."));
    }
    c.put("Merge do merge request", &format!("{}/merge_requests/{n}/merge", project(c)), json!({ "squash": m == MergeMethod::Squash }))?;
    Ok(())
}

fn notes(c: &Client, kind: &str, n: u64) -> Result<Vec<Comment>> {
    let v = c.get("Comentários", &format!("{}/{kind}/{n}/notes?sort=asc&per_page=100", project(c)))?;
    Ok(v.as_array()
        .map(|a| {
            a.iter()
                // Notas de sistema ("changed the description", "added label") não são comentários.
                .filter(|x| !x["system"].as_bool().unwrap_or(false))
                .map(|x| Comment { author: path_str(x, &["author", "username"]), body: s(x, "body"), created_at: s(x, "created_at") })
                .collect()
        })
        .unwrap_or_default())
}

pub fn mr_notes(c: &Client, n: u64) -> Result<Vec<Comment>> {
    notes(c, "merge_requests", n)
}

pub fn issue_notes(c: &Client, n: u64) -> Result<Vec<Comment>> {
    notes(c, "issues", n)
}

pub fn comment(c: &Client, kind: &str, n: u64, body: &str) -> Result<()> {
    c.post("Comentar", &format!("{}/{kind}/{n}/notes", project(c)), json!({ "body": body }))?;
    Ok(())
}

fn issue(v: &Value) -> Issue {
    Issue {
        number: v["iid"].as_u64().unwrap_or(0),
        title: s(v, "title"),
        body: s(v, "description"),
        author: path_str(v, &["author", "username"]),
        labels: v["labels"].as_array().map(|a| a.iter().filter_map(|l| l.as_str().map(str::to_string)).collect()).unwrap_or_default(),
        assignees: v["assignees"].as_array().map(|a| a.iter().map(|u| s(u, "username")).collect()).unwrap_or_default(),
        url: s(v, "web_url"),
    }
}

pub fn list_issues(c: &Client) -> Result<Vec<Issue>> {
    let v = c.get("Issues", &format!("{}/issues?state=opened&per_page=100&order_by=updated_at", project(c)))?;
    Ok(v.as_array().map(|a| a.iter().map(issue).collect()).unwrap_or_default())
}

pub fn create_issue(c: &Client, title: &str, body: &str) -> Result<Issue> {
    Ok(issue(&c.post("Criar issue", &format!("{}/issues", project(c)), json!({ "title": title, "description": body }))?))
}

pub fn fork(c: &Client) -> Result<String> {
    let v = c.post("Fork", &format!("{}/fork", project(c)), json!({}))?;
    Ok(s(&v, "http_url_to_repo"))
}

fn job_state(status: &str) -> CiState {
    match status {
        "success" => CiState::Success,
        "failed" => CiState::Failure,
        "canceled" | "skipped" | "manual" => CiState::Neutral,
        _ => CiState::Pending,
    }
}

pub fn ci_status(c: &Client, sha: &str) -> Result<CiStatus> {
    let pipelines = c.get("Pipelines", &format!("{}/pipelines?sha={sha}&per_page=1&order_by=id&sort=desc", project(c)))?;
    pipeline_status(c, pipelines.as_array().and_then(|a| a.first()).cloned())
}

/// CI de um MR: pipelines de MR (detached / merged results rodam num SHA de
/// merge, não no head) e, se não houver, o pipeline do SHA do head.
pub fn mr_ci_status(c: &Client, iid: u64, sha: Option<&str>) -> Result<CiStatus> {
    let mr = c.get("Pipelines do MR", &format!("{}/merge_requests/{iid}/pipelines?per_page=1", project(c)))?;
    match mr.as_array().and_then(|a| a.first()).cloned() {
        Some(p) => pipeline_status(c, Some(p)),
        None => match sha {
            Some(sha) => ci_status(c, sha),
            None => Ok(CiStatus { overall: None, checks: Vec::new(), url: String::new() }),
        },
    }
}

fn pipeline_status(c: &Client, pipeline: Option<Value>) -> Result<CiStatus> {
    let Some(p) = pipeline else {
        return Ok(CiStatus { overall: None, checks: Vec::new(), url: String::new() });
    };
    let id = p["id"].as_u64().unwrap_or(0);
    let jobs = c.get("Jobs", &format!("{}/pipelines/{id}/jobs?per_page=100", project(c)))?;
    let checks = jobs
        .as_array()
        .into_iter()
        .flatten()
        .map(|j| Check { name: format!("{} · {}", s(j, "stage"), s(j, "name")), state: job_state(&s(j, "status")), url: s(j, "web_url") })
        .collect();
    Ok(CiStatus { overall: Some(job_state(&s(&p, "status"))), checks, url: s(&p, "web_url") })
}

pub fn mr_review(c: &Client, n: u64) -> Result<PrReview> {
    let changes = c.get("Alterações do MR", &format!("{}/merge_requests/{n}/changes", project(c)))?;
    let files = changes["changes"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|f| {
            let hunks = parse_patch(&s(f, "diff"));
            let count = |o: char| hunks.iter().flat_map(|h| &h.lines).filter(|l| l.origin == o).count();
            PrFile { path: s(f, "new_path"), old_path: s(f, "old_path"), additions: count('+'), deletions: count('-'), hunks }
        })
        .collect();
    let refs = DiffRefs {
        base_sha: path_str(&changes, &["diff_refs", "base_sha"]),
        start_sha: path_str(&changes, &["diff_refs", "start_sha"]),
        head_sha: path_str(&changes, &["diff_refs", "head_sha"]),
    };
    let discussions = c.get("Discussões", &format!("{}/merge_requests/{n}/discussions?per_page=100", project(c)))?;
    let mut comments = Vec::new();
    for d in discussions.as_array().into_iter().flatten() {
        for note in d["notes"].as_array().into_iter().flatten() {
            let pos = &note["position"];
            if pos.is_null() || note["system"].as_bool().unwrap_or(false) {
                continue;
            }
            comments.push(ReviewComment {
                path: { let np = s(pos, "new_path"); if np.is_empty() { s(pos, "old_path") } else { np } },
                new_line: pos["new_line"].as_u64().map(|l| l as u32),
                old_line: pos["old_line"].as_u64().map(|l| l as u32),
                author: path_str(note, &["author", "username"]),
                body: s(note, "body"),
            });
        }
    }
    Ok(PrReview { files, comments, refs })
}

pub fn add_review_comment(c: &Client, n: u64, refs: &DiffRefs, at: &LineAnchor, body: &str) -> Result<()> {
    let mut position = json!({
        "position_type": "text",
        "base_sha": refs.base_sha,
        "start_sha": refs.start_sha,
        "head_sha": refs.head_sha,
        "new_path": at.path,
        "old_path": at.old_path,
    });
    if let Some(l) = at.new_line {
        position["new_line"] = json!(l);
    }
    if let Some(l) = at.old_line {
        position["old_line"] = json!(l);
    }
    c.post("Comentário de revisão", &format!("{}/merge_requests/{n}/discussions", project(c)), json!({ "body": body, "position": position }))?;
    Ok(())
}
