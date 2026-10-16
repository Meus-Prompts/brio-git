//! Commands that change the repository: checkout, branches, tags, history
//! rewriting, stash, staging, commits and remotes. Each is one or two `git`
//! invocations so hooks, credentials and config behave as in a terminal.

use crate::git::{Git, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetMode {
    /// Keep changes staged.
    Soft,
    /// Keep changes in the working tree, unstaged.
    Mixed,
    /// Discard all changes.
    Hard,
}

impl ResetMode {
    fn flag(self) -> &'static str {
        match self {
            ResetMode::Soft => "--soft",
            ResetMode::Mixed => "--mixed",
            ResetMode::Hard => "--hard",
        }
    }
}

/// Operation left half-done by a conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InProgress {
    Merge,
    Rebase,
    CherryPick,
    Revert,
}

impl InProgress {
    pub fn label(self) -> &'static str {
        match self {
            InProgress::Merge => "merge",
            InProgress::Rebase => "rebase",
            InProgress::CherryPick => "cherry-pick",
            InProgress::Revert => "revert",
        }
    }
}

/// Detects a merge/rebase/cherry-pick/revert waiting for conflict resolution.
pub fn in_progress(git_dir: &std::path::Path) -> Option<InProgress> {
    if git_dir.join("rebase-merge").is_dir() || git_dir.join("rebase-apply").is_dir() {
        Some(InProgress::Rebase)
    } else if git_dir.join("MERGE_HEAD").is_file() {
        Some(InProgress::Merge)
    } else if git_dir.join("CHERRY_PICK_HEAD").is_file() {
        Some(InProgress::CherryPick)
    } else if git_dir.join("REVERT_HEAD").is_file() {
        Some(InProgress::Revert)
    } else {
        None
    }
}

pub fn continue_op(git: &Git, op: InProgress) -> Result<()> {
    match op {
        InProgress::Merge => git.run(&["commit", "--no-edit"]),
        other => git.run(&[other.label(), "--continue"]),
    }
    .map(drop)
}

pub fn abort_op(git: &Git, op: InProgress) -> Result<()> {
    git.run(&[op.label(), "--abort"]).map(drop)
}

/// Whether tracked files have uncommitted changes (untracked files do not
/// block a checkout).
pub fn has_tracked_changes(git: &Git) -> Result<bool> {
    Ok(!git.run(&["status", "--porcelain", "--untracked-files=no"])?.trim().is_empty())
}

pub fn checkout_branch(git: &Git, name: &str) -> Result<()> {
    git.run(&["checkout", name]).map(drop)
}

/// Creates (or reuses) a local branch tracking `remote_ref` (e.g. `origin/feat`).
pub fn checkout_remote(git: &Git, remote_ref: &str) -> Result<String> {
    let local = remote_ref.split_once('/').map_or(remote_ref, |(_, b)| b).to_owned();
    let exists = git.try_run(&["rev-parse", "--verify", "-q", &format!("refs/heads/{local}")])?.is_some();
    if exists {
        git.run(&["checkout", &local])?;
    } else {
        git.run(&["checkout", "-b", &local, "--track", remote_ref])?;
    }
    Ok(local)
}

pub fn checkout_detached(git: &Git, rev: &str) -> Result<()> {
    git.run(&["checkout", "--detach", rev]).map(drop)
}

pub fn create_branch(git: &Git, name: &str, at: &str, checkout: bool) -> Result<()> {
    if checkout {
        git.run(&["checkout", "-b", name, at]).map(drop)
    } else {
        git.run(&["branch", name, at]).map(drop)
    }
}

pub fn rename_branch(git: &Git, old: &str, new: &str) -> Result<()> {
    git.run(&["branch", "-m", old, new]).map(drop)
}

pub fn delete_branch(git: &Git, name: &str, force: bool) -> Result<()> {
    git.run(&["branch", if force { "-D" } else { "-d" }, name]).map(drop)
}

pub fn delete_remote_branch(git: &Git, remote: &str, name: &str) -> Result<()> {
    git.run(&["push", remote, "--delete", name]).map(drop)
}

/// Lightweight tag, or annotated when a message is given.
pub fn create_tag(git: &Git, name: &str, at: &str, message: Option<&str>) -> Result<()> {
    match message.filter(|m| !m.trim().is_empty()) {
        Some(m) => git.run(&["tag", "-a", name, "-m", m, at]),
        None => git.run(&["tag", name, at]),
    }
    .map(drop)
}

pub fn delete_tag(git: &Git, name: &str) -> Result<()> {
    git.run(&["tag", "-d", name]).map(drop)
}

pub fn delete_remote_tag(git: &Git, remote: &str, name: &str) -> Result<()> {
    git.run(&["push", remote, "--delete", &format!("refs/tags/{name}")]).map(drop)
}

pub fn push_tag(git: &Git, remote: &str, name: &str) -> Result<()> {
    git.run(&["push", remote, &format!("refs/tags/{name}")]).map(drop)
}

fn parent_count(git: &Git, id: &str) -> Result<usize> {
    Ok(git.run(&["rev-list", "--parents", "-n1", id])?.split_whitespace().count().saturating_sub(1))
}

/// Cherry-picks `id`; merge commits are applied relative to their first parent.
pub fn cherry_pick(git: &Git, id: &str) -> Result<()> {
    if parent_count(git, id)? > 1 {
        git.run(&["cherry-pick", "-m", "1", id]).map(drop)
    } else {
        git.run(&["cherry-pick", id]).map(drop)
    }
}

pub fn revert(git: &Git, id: &str) -> Result<()> {
    if parent_count(git, id)? > 1 {
        git.run(&["revert", "--no-edit", "-m", "1", id]).map(drop)
    } else {
        git.run(&["revert", "--no-edit", id]).map(drop)
    }
}

pub fn reset(git: &Git, mode: ResetMode, rev: &str) -> Result<()> {
    git.run(&["reset", mode.flag(), rev]).map(drop)
}

pub fn merge(git: &Git, rev: &str) -> Result<()> {
    git.run(&["merge", "--no-edit", rev]).map(drop)
}

pub fn rebase(git: &Git, onto: &str) -> Result<()> {
    git.run(&["rebase", onto]).map(drop)
}

// ---- stash ----

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stash {
    /// `stash@{n}`
    pub name: String,
    pub message: String,
    pub time: i64,
}

pub fn stash_list(git: &Git) -> Result<Vec<Stash>> {
    let out = git.run(&["stash", "list", "--format=%gd%x1f%ct%x1f%gs"])?;
    Ok(out
        .lines()
        .filter_map(|l| {
            let mut f = l.split('\x1f');
            let name = f.next()?.to_owned();
            let time = f.next()?.parse().unwrap_or_default();
            let message = f.next().unwrap_or_default().to_owned();
            Some(Stash { name, message, time })
        })
        .collect())
}

/// Stashes everything, untracked files included.
pub fn stash_push(git: &Git, message: &str) -> Result<()> {
    git.run(&["stash", "push", "--include-untracked", "-m", message]).map(drop)
}

pub fn stash_apply(git: &Git, name: &str) -> Result<()> {
    git.run(&["stash", "apply", name]).map(drop)
}

pub fn stash_pop(git: &Git, name: &str) -> Result<()> {
    git.run(&["stash", "pop", name]).map(drop)
}

pub fn stash_drop(git: &Git, name: &str) -> Result<()> {
    git.run(&["stash", "drop", name]).map(drop)
}

// ---- staging and commits ----

pub fn stage(git: &Git, paths: &[String]) -> Result<()> {
    let mut args = vec!["add", "--all", "--"];
    args.extend(paths.iter().map(String::as_str));
    git.run(&args).map(drop)
}

pub fn unstage(git: &Git, paths: &[String]) -> Result<()> {
    let has_head = git.try_run(&["rev-parse", "--verify", "-q", "HEAD"])?.is_some();
    let mut args = if has_head { vec!["reset", "-q", "HEAD", "--"] } else { vec!["rm", "--cached", "-r", "-q", "--"] };
    args.extend(paths.iter().map(String::as_str));
    git.run(&args).map(drop)
}

/// Throws away working tree changes of `paths` (untracked files are deleted).
pub fn discard(git: &Git, paths: &[String], untracked: &[String]) -> Result<()> {
    if !paths.is_empty() {
        let mut args = vec!["checkout", "--"];
        args.extend(paths.iter().map(String::as_str));
        git.run(&args)?;
    }
    if !untracked.is_empty() {
        let mut args = vec!["clean", "-f", "-q", "--"];
        args.extend(untracked.iter().map(String::as_str));
        git.run(&args)?;
    }
    Ok(())
}

pub fn commit(git: &Git, message: &str, amend: bool) -> Result<()> {
    let mut args = vec!["commit", "--cleanup=strip", "-F", "-"];
    if amend {
        args.push("--amend");
    }
    git.run_with_input(&args, message.as_bytes()).map(drop)
}

pub fn last_commit_message(git: &Git) -> Result<String> {
    Ok(git.run(&["log", "-1", "--format=%B"])?.trim_end().to_owned())
}

/// The staged diff, used to ask an AI for a commit message.
pub fn staged_diff(git: &Git) -> Result<String> {
    git.run(&["diff", "--cached", "--no-color", "--stat", "--patch"])
}

// ---- remotes ----

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Upstream {
    /// e.g. `origin/main`
    pub name: String,
    pub ahead: usize,
    pub behind: usize,
}

/// Upstream of the current branch and how far apart they are.
pub fn upstream(git: &Git) -> Result<Option<Upstream>> {
    let Some(name) = git.try_run(&["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}"])? else {
        return Ok(None);
    };
    let counts = git.run(&["rev-list", "--left-right", "--count", "HEAD...@{upstream}"])?;
    let mut n = counts.split_whitespace().map(|c| c.parse().unwrap_or(0));
    Ok(Some(Upstream { name: name.trim().to_owned(), ahead: n.next().unwrap_or(0), behind: n.next().unwrap_or(0) }))
}

pub fn remotes(git: &Git) -> Result<Vec<String>> {
    Ok(git.run(&["remote"])?.lines().map(str::to_owned).collect())
}

pub fn remote_url(git: &Git, remote: &str) -> Result<String> {
    Ok(git.run(&["remote", "get-url", remote])?.trim().to_owned())
}

pub fn fetch(git: &Git) -> Result<()> {
    git.run(&["fetch", "--all", "--prune", "--tags"]).map(drop)
}

pub fn pull(git: &Git) -> Result<()> {
    git.run(&["pull", "--no-edit"]).map(drop)
}

/// Pushes `branch` to its upstream remote; a branch without upstream is
/// published to `origin` (or the only remote) and starts tracking it.
pub fn push(git: &Git, branch: &str, force: bool) -> Result<()> {
    let tracked = git.try_run(&["config", &format!("branch.{branch}.remote")])?;
    let (remote, set_upstream) = match tracked {
        Some(r) => (r.trim().to_owned(), false),
        None => {
            let all = remotes(git)?;
            let remote = if all.iter().any(|r| r == "origin") { "origin".to_owned() } else { all.into_iter().next().unwrap_or_default() };
            (remote, true)
        }
    };
    let mut args = vec!["push"];
    if force {
        args.push("--force-with-lease");
    }
    if set_upstream {
        args.push("--set-upstream");
    }
    args.extend([remote.as_str(), branch]);
    git.run(&args).map(drop)
}

/// Web page to open a merge/pull request from `source` into `target`.
/// Supports GitHub, GitLab, Bitbucket and Azure DevOps remotes.
pub fn merge_request_url(remote_url: &str, source: &str, target: &str) -> Option<String> {
    let (host, path) = parse_remote(remote_url)?;
    let enc = encode_branch;
    if host.contains("github") {
        Some(format!("https://{host}/{path}/compare/{}...{}?expand=1", enc(target), enc(source)))
    } else if host.contains("gitlab") {
        Some(format!(
            "https://{host}/{path}/-/merge_requests/new?merge_request[source_branch]={}&merge_request[target_branch]={}",
            enc(source),
            enc(target)
        ))
    } else if host.contains("bitbucket") {
        Some(format!("https://{host}/{path}/pull-requests/new?source={}&dest={}", enc(source), enc(target)))
    } else if host.contains("dev.azure.com") || host.contains("visualstudio.com") {
        Some(format!(
            "https://dev.azure.com/{path}/pullrequestcreate?sourceRef={}&targetRef={}",
            enc(source),
            enc(target)
        ))
    } else {
        None
    }
}

/// Percent-encodes everything except unreserved characters and `/`, so a
/// branch name can never change the meaning of the URL.
fn encode_branch(name: &str) -> String {
    name.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Splits a remote URL (https or ssh) into host and repository path, with the
/// Azure DevOps ssh form rewritten to its web path (`org/project/_git/repo`).
fn parse_remote(url: &str) -> Option<(String, String)> {
    let url = url.trim().trim_end_matches('/').trim_end_matches(".git");
    let (host, path) = if let Some(rest) = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://")) {
        let rest = rest.split_once('@').map_or(rest, |(_, r)| r);
        rest.split_once('/')?
    } else if let Some(rest) = url.strip_prefix("ssh://") {
        let rest = rest.split_once('@').map_or(rest, |(_, r)| r);
        let (host, path) = rest.split_once('/')?;
        (host.split(':').next()?, path)
    } else {
        let rest = url.split_once('@').map_or(url, |(_, r)| r);
        rest.split_once(':')?
    };
    let host = host.to_owned();
    if host.starts_with("ssh.dev.azure.com") || host.starts_with("vs-ssh.") {
        // v3/org/project/repo -> org/project/_git/repo
        let parts: Vec<&str> = path.trim_start_matches("v3/").split('/').collect();
        if let [org, project, repo] = parts[..] {
            return Some(("dev.azure.com".into(), format!("{org}/{project}/_git/{repo}")));
        }
    }
    Some((host, path.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn repo() -> (tempfile::TempDir, Git) {
        let dir = tempfile::tempdir().unwrap();
        let git = Git::new(dir.path());
        for args in [
            &["init", "-q", "-b", "main"][..],
            &["config", "user.name", "Ana"],
            &["config", "user.email", "ana@x.dev"],
            &["config", "commit.gpgsign", "false"],
            &["config", "tag.gpgsign", "false"],
        ] {
            git.run(args).unwrap();
        }
        fs::write(dir.path().join("a.txt"), "one\n").unwrap();
        git.run(&["add", "."]).unwrap();
        git.run(&["commit", "-qm", "first"]).unwrap();
        (dir, git)
    }

    fn head_summary(git: &Git) -> String {
        git.run(&["log", "-1", "--format=%s"]).unwrap().trim().to_owned()
    }

    fn branch(git: &Git) -> String {
        git.run(&["branch", "--show-current"]).unwrap().trim().to_owned()
    }

    #[test]
    fn branches_checkout_rename_delete() {
        let (_d, git) = repo();
        create_branch(&git, "feat", "HEAD", true).unwrap();
        assert_eq!(branch(&git), "feat");
        checkout_branch(&git, "main").unwrap();
        rename_branch(&git, "feat", "feature").unwrap();
        delete_branch(&git, "feature", false).unwrap();
        assert!(git.run(&["branch", "--list", "feature"]).unwrap().trim().is_empty());
        checkout_detached(&git, "HEAD").unwrap();
        assert_eq!(branch(&git), "");
    }

    #[test]
    fn tags_light_and_annotated() {
        let (_d, git) = repo();
        create_tag(&git, "v1", "HEAD", None).unwrap();
        create_tag(&git, "v2", "HEAD", Some("release 2")).unwrap();
        assert_eq!(git.run(&["cat-file", "-t", "v2"]).unwrap().trim(), "tag");
        assert_eq!(git.run(&["cat-file", "-t", "v1"]).unwrap().trim(), "commit");
        delete_tag(&git, "v1").unwrap();
        assert_eq!(git.run(&["tag"]).unwrap().trim(), "v2");
    }

    #[test]
    fn cherry_pick_revert_reset_merge_rebase() {
        let (dir, git) = repo();
        create_branch(&git, "feat", "HEAD", true).unwrap();
        fs::write(dir.path().join("b.txt"), "b\n").unwrap();
        stage(&git, &["b.txt".into()]).unwrap();
        commit(&git, "add b", false).unwrap();
        let b = git.run(&["rev-parse", "HEAD"]).unwrap().trim().to_owned();
        checkout_branch(&git, "main").unwrap();

        cherry_pick(&git, &b).unwrap();
        assert_eq!(head_summary(&git), "add b");
        revert(&git, "HEAD").unwrap();
        assert!(head_summary(&git).starts_with("Revert"));
        assert!(!dir.path().join("b.txt").exists());

        reset(&git, ResetMode::Hard, "HEAD~2").unwrap();
        assert_eq!(head_summary(&git), "first");

        fs::write(dir.path().join("c.txt"), "c\n").unwrap();
        stage(&git, &["c.txt".into()]).unwrap();
        commit(&git, "add c", false).unwrap();
        merge(&git, "feat").unwrap();
        assert!(dir.path().join("b.txt").exists());

        reset(&git, ResetMode::Hard, "HEAD~1").unwrap();
        checkout_branch(&git, "feat").unwrap();
        rebase(&git, "main").unwrap();
        let log = git.run(&["log", "--format=%s"]).unwrap();
        assert_eq!(log.lines().collect::<Vec<_>>(), ["add b", "add c", "first"]);
    }

    #[test]
    fn soft_and_mixed_reset_keep_changes() {
        let (dir, git) = repo();
        fs::write(dir.path().join("a.txt"), "two\n").unwrap();
        stage(&git, &["a.txt".into()]).unwrap();
        commit(&git, "two", false).unwrap();
        reset(&git, ResetMode::Soft, "HEAD~1").unwrap();
        assert!(git.run(&["diff", "--cached", "--name-only"]).unwrap().contains("a.txt"));
        reset(&git, ResetMode::Mixed, "HEAD").unwrap();
        assert!(git.run(&["diff", "--name-only"]).unwrap().contains("a.txt"));
    }

    #[test]
    fn conflicts_are_detected_and_abortable() {
        let (dir, git) = repo();
        create_branch(&git, "other", "HEAD", true).unwrap();
        fs::write(dir.path().join("a.txt"), "theirs\n").unwrap();
        stage(&git, &["a.txt".into()]).unwrap();
        commit(&git, "theirs", false).unwrap();
        checkout_branch(&git, "main").unwrap();
        fs::write(dir.path().join("a.txt"), "ours\n").unwrap();
        stage(&git, &["a.txt".into()]).unwrap();
        commit(&git, "ours", false).unwrap();

        assert!(merge(&git, "other").is_err());
        let git_dir = dir.path().join(".git");
        assert_eq!(in_progress(&git_dir), Some(InProgress::Merge));
        abort_op(&git, InProgress::Merge).unwrap();
        assert_eq!(in_progress(&git_dir), None);

        assert!(rebase(&git, "other").is_err());
        assert_eq!(in_progress(&git_dir), Some(InProgress::Rebase));
        abort_op(&git, InProgress::Rebase).unwrap();
        assert_eq!(in_progress(&git_dir), None);
    }

    #[test]
    fn stash_round_trip_with_untracked() {
        let (dir, git) = repo();
        fs::write(dir.path().join("a.txt"), "dirty\n").unwrap();
        fs::write(dir.path().join("new.txt"), "n\n").unwrap();
        assert!(has_tracked_changes(&git).unwrap());
        stash_push(&git, "antes do checkout").unwrap();
        assert!(!has_tracked_changes(&git).unwrap());
        assert!(!dir.path().join("new.txt").exists());
        let list = stash_list(&git).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "stash@{0}");
        assert!(list[0].message.contains("antes do checkout"));
        stash_pop(&git, "stash@{0}").unwrap();
        assert!(dir.path().join("new.txt").exists());
        assert!(stash_list(&git).unwrap().is_empty());
    }

    #[test]
    fn stage_unstage_discard_commit_amend() {
        let (dir, git) = repo();
        fs::write(dir.path().join("a.txt"), "changed\n").unwrap();
        fs::write(dir.path().join("u.txt"), "untracked\n").unwrap();
        stage(&git, &["a.txt".into()]).unwrap();
        assert_eq!(git.run(&["diff", "--cached", "--name-only"]).unwrap().trim(), "a.txt");
        assert!(staged_diff(&git).unwrap().contains("+changed"));
        unstage(&git, &["a.txt".into()]).unwrap();
        assert_eq!(git.run(&["diff", "--cached", "--name-only"]).unwrap().trim(), "");

        discard(&git, &["a.txt".into()], &["u.txt".into()]).unwrap();
        assert_eq!(fs::read_to_string(dir.path().join("a.txt")).unwrap(), "one\n");
        assert!(!dir.path().join("u.txt").exists());

        fs::write(dir.path().join("a.txt"), "v2\n").unwrap();
        stage(&git, &["a.txt".into()]).unwrap();
        commit(&git, "feat: v2\n\ncorpo da mensagem", false).unwrap();
        assert_eq!(last_commit_message(&git).unwrap(), "feat: v2\n\ncorpo da mensagem");
        commit(&git, "feat: v2 corrigido", true).unwrap();
        assert_eq!(git.run(&["rev-list", "--count", "HEAD"]).unwrap().trim(), "2");
        assert_eq!(head_summary(&git), "feat: v2 corrigido");
    }

    #[test]
    fn push_pull_fetch_and_upstream_against_a_local_remote() {
        let (dir, git) = repo();
        let remote_dir = tempfile::tempdir().unwrap();
        Git::new(remote_dir.path()).run(&["init", "-q", "--bare", "-b", "main"]).unwrap();
        git.run(&["remote", "add", "origin", remote_dir.path().to_str().unwrap()]).unwrap();

        assert_eq!(upstream(&git).unwrap(), None);
        push(&git, "main", false).unwrap();
        assert_eq!(upstream(&git).unwrap().unwrap(), Upstream { name: "origin/main".into(), ahead: 0, behind: 0 });

        fs::write(dir.path().join("a.txt"), "local\n").unwrap();
        stage(&git, &["a.txt".into()]).unwrap();
        commit(&git, "local", false).unwrap();
        assert_eq!(upstream(&git).unwrap().unwrap().ahead, 1);
        push(&git, "main", false).unwrap();
        fetch(&git).unwrap();
        pull(&git).unwrap();
        assert_eq!(upstream(&git).unwrap().unwrap().ahead, 0);

        create_tag(&git, "v1", "HEAD", None).unwrap();
        push_tag(&git, "origin", "v1").unwrap();
        delete_remote_tag(&git, "origin", "v1").unwrap();
        create_branch(&git, "tmp", "HEAD", false).unwrap();
        push(&git, "tmp", false).unwrap();
        delete_remote_branch(&git, "origin", "tmp").unwrap();
        let remote_refs = Git::new(remote_dir.path()).run(&["for-each-ref", "--format=%(refname)"]).unwrap();
        assert_eq!(remote_refs.trim(), "refs/heads/main");
    }

    #[test]
    fn checkout_remote_creates_tracking_branch() {
        let (_dir, git) = repo();
        let remote_dir = tempfile::tempdir().unwrap();
        Git::new(remote_dir.path()).run(&["init", "-q", "--bare", "-b", "main"]).unwrap();
        git.run(&["remote", "add", "origin", remote_dir.path().to_str().unwrap()]).unwrap();
        create_branch(&git, "feat", "HEAD", false).unwrap();
        push(&git, "feat", false).unwrap();
        delete_branch(&git, "feat", true).unwrap();

        assert_eq!(checkout_remote(&git, "origin/feat").unwrap(), "feat");
        assert_eq!(branch(&git), "feat");
        assert_eq!(upstream(&git).unwrap().unwrap().name, "origin/feat");
        checkout_branch(&git, "main").unwrap();
        assert_eq!(checkout_remote(&git, "origin/feat").unwrap(), "feat", "reuses existing branch");
    }

    #[test]
    fn merge_request_urls_for_common_hosts() {
        assert_eq!(
            merge_request_url("git@github.com:acme/app.git", "feat/x", "main").unwrap(),
            "https://github.com/acme/app/compare/main...feat/x?expand=1"
        );
        assert_eq!(
            merge_request_url("https://gitlab.com/acme/group/app.git", "feat", "staging").unwrap(),
            "https://gitlab.com/acme/group/app/-/merge_requests/new?merge_request[source_branch]=feat&merge_request[target_branch]=staging"
        );
        assert_eq!(
            merge_request_url("git@ssh.dev.azure.com:v3/saude-trevo/Trevo/web", "feat", "main").unwrap(),
            "https://dev.azure.com/saude-trevo/Trevo/_git/web/pullrequestcreate?sourceRef=feat&targetRef=main"
        );
        assert_eq!(
            merge_request_url("https://user@dev.azure.com/saude-trevo/Trevo/_git/web", "a", "b").unwrap(),
            "https://dev.azure.com/saude-trevo/Trevo/_git/web/pullrequestcreate?sourceRef=a&targetRef=b"
        );
        assert!(merge_request_url("/tmp/local/repo", "a", "b").is_none());
        // Shell and URL metacharacters in branch names are always encoded.
        assert_eq!(
            merge_request_url("git@github.com:a/b.git", "x&calc|<y>^z", "main").unwrap(),
            "https://github.com/a/b/compare/main...x%26calc%7C%3Cy%3E%5Ez?expand=1"
        );
    }
}
