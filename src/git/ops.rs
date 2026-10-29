//! Operações de escrita. Cada função é síncrona e roda em thread de job.

use std::path::Path;

use anyhow::{Result, anyhow};

use super::cli::{git, git_stdin, git_with};
use super::diff::PatchMode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResetMode {
    Soft,
    Mixed,
    Hard,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PullMode {
    FastForwardIfPossible,
    FastForwardOnly,
    Rebase,
}

pub fn stage(repo: &Path, paths: &[String]) -> Result<String> {
    let mut args = vec!["add", "-A", "--"];
    args.extend(paths.iter().map(String::as_str));
    git(repo, &args)
}

pub fn stage_all(repo: &Path) -> Result<String> {
    git(repo, &["add", "-A"])
}

pub fn unstage(repo: &Path, paths: &[String], has_head: bool) -> Result<String> {
    let mut args: Vec<&str> = if has_head { vec!["reset", "-q", "HEAD", "--"] } else { vec!["rm", "--cached", "-r", "-q", "--"] };
    args.extend(paths.iter().map(String::as_str));
    git(repo, &args)
}

pub fn unstage_all(repo: &Path, has_head: bool) -> Result<String> {
    if has_head { git(repo, &["reset", "-q"]) } else { git(repo, &["rm", "--cached", "-r", "-q", "."]) }
}

/// Descarta mudanças do working dir: restaura arquivos rastreados e remove
/// arquivos não rastreados.
pub fn discard(repo: &Path, tracked: &[String], untracked: &[String]) -> Result<String> {
    if !tracked.is_empty() {
        let mut args = vec!["checkout", "--"];
        args.extend(tracked.iter().map(String::as_str));
        git(repo, &args)?;
    }
    if !untracked.is_empty() {
        let mut args = vec!["clean", "-f", "-q", "--"];
        args.extend(untracked.iter().map(String::as_str));
        git(repo, &args)?;
    }
    Ok(String::new())
}

pub fn discard_all(repo: &Path) -> Result<String> {
    git(repo, &["reset", "--hard", "-q"])?;
    git(repo, &["clean", "-fd", "-q"])
}

pub fn apply_patch(repo: &Path, patch: &str, mode: PatchMode) -> Result<String> {
    let args: &[&str] = match mode {
        PatchMode::Stage => &["apply", "--cached", "--recount", "--whitespace=nowarn", "-"],
        PatchMode::Unstage => &["apply", "--cached", "--reverse", "--recount", "--whitespace=nowarn", "-"],
        PatchMode::Discard => &["apply", "--reverse", "--recount", "--whitespace=nowarn", "-"],
    };
    git_stdin(repo, args, patch)
}

pub struct CommitRequest {
    pub message: String,
    pub amend: bool,
    pub skip_hooks: bool,
    /// (nome, email) do perfil ativo; `None` usa o gitconfig.
    pub identity: Option<(String, String)>,
}

pub fn commit(repo: &Path, req: &CommitRequest) -> Result<String> {
    let mut args = vec!["commit", "-F", "-", "--cleanup=strip"];
    if req.amend {
        args.push("--amend");
    }
    if req.skip_hooks {
        args.push("--no-verify");
    }
    let mut env: Vec<(&str, &str)> = Vec::new();
    if let Some((name, email)) = &req.identity {
        env.extend([("GIT_AUTHOR_NAME", name.as_str()), ("GIT_AUTHOR_EMAIL", email.as_str())]);
        env.extend([("GIT_COMMITTER_NAME", name.as_str()), ("GIT_COMMITTER_EMAIL", email.as_str())]);
    }
    git_with(repo, &args, Some(&req.message), &env)
}

pub fn checkout(repo: &Path, target: &str) -> Result<String> {
    git(repo, &["checkout", target])
}

/// Faz checkout de um branch remoto criando (ou reaproveitando) o local.
pub fn checkout_remote(repo: &Path, remote_branch: &str) -> Result<String> {
    let local = remote_branch.split_once('/').map(|(_, b)| b).unwrap_or(remote_branch);
    if git(repo, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{local}")]).is_ok() {
        git(repo, &["checkout", local])
    } else {
        git(repo, &["checkout", "-b", local, "--track", remote_branch])
    }
}

/// Checkout de um branch de PR do próprio repositório: reaproveita o branch
/// local (definindo upstream se faltar e avançando por fast-forward) ou cria
/// um novo rastreando o remoto.
pub fn checkout_pr_branch(repo: &Path, remote: &str, branch: &str) -> Result<String> {
    let remote_ref = format!("{remote}/{branch}");
    if git(repo, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")]).is_err() {
        return git(repo, &["checkout", "-b", branch, "--track", &remote_ref]);
    }
    git(repo, &["checkout", branch])?;
    if git(repo, &["rev-parse", "--abbrev-ref", &format!("{branch}@{{upstream}}")]).is_err() {
        git(repo, &["branch", &format!("--set-upstream-to={remote_ref}"), branch])?;
    }
    // Só avança se for fast-forward; divergência local é preservada.
    match git(repo, &["merge", "--ff-only", &remote_ref]) {
        Ok(_) => Ok(format!("{branch} atualizado com {remote_ref}")),
        Err(_) => Ok(format!("{branch} tem commits locais que divergem de {remote_ref}; nada foi sobrescrito")),
    }
}

pub fn create_branch(repo: &Path, name: &str, at: &str, checkout: bool) -> Result<String> {
    if checkout { git(repo, &["checkout", "-b", name, at]) } else { git(repo, &["branch", name, at]) }
}

pub fn delete_branch(repo: &Path, name: &str, force: bool) -> Result<String> {
    git(repo, &["branch", if force { "-D" } else { "-d" }, name])
}

pub fn delete_remote_branch(repo: &Path, remote: &str, branch: &str) -> Result<String> {
    git(repo, &["push", remote, "--delete", branch])
}

pub fn rename_branch(repo: &Path, old: &str, new: &str) -> Result<String> {
    git(repo, &["branch", "-m", old, new])
}

pub fn set_upstream(repo: &Path, branch: &str, upstream: &str) -> Result<String> {
    git(repo, &["branch", &format!("--set-upstream-to={upstream}"), branch])
}

pub fn create_tag(repo: &Path, name: &str, at: &str, message: Option<&str>) -> Result<String> {
    match message.filter(|m| !m.trim().is_empty()) {
        Some(m) => git(repo, &["tag", "-a", name, at, "-m", m]),
        None => git(repo, &["tag", name, at]),
    }
}

pub fn delete_tag(repo: &Path, name: &str) -> Result<String> {
    git(repo, &["tag", "-d", name])
}

pub fn push_tag(repo: &Path, remote: &str, name: &str) -> Result<String> {
    git(repo, &["push", remote, &format!("refs/tags/{name}")])
}

pub fn delete_remote_tag(repo: &Path, remote: &str, name: &str) -> Result<String> {
    git(repo, &["push", remote, "--delete", &format!("refs/tags/{name}")])
}

pub fn merge(repo: &Path, what: &str, ff_only: bool) -> Result<String> {
    if ff_only { git(repo, &["merge", "--ff-only", what]) } else { git(repo, &["merge", "--no-edit", what]) }
}

/// Faz checkout de `target` (se preciso) e mescla `source` nele.
pub fn merge_into(repo: &Path, source: &str, target: &str, current: Option<&str>) -> Result<String> {
    if current != Some(target) {
        git(repo, &["checkout", target])?;
    }
    git(repo, &["merge", "--no-edit", source])
}

/// `git rebase <onto> <branch>`: faz checkout de `branch` e o rebaseia.
pub fn rebase_branch(repo: &Path, branch: &str, onto: &str) -> Result<String> {
    git(repo, &["rebase", onto, branch])
}

pub fn rebase(repo: &Path, onto: &str) -> Result<String> {
    git(repo, &["rebase", onto])
}

pub fn cherry_pick(repo: &Path, commits: &[String]) -> Result<String> {
    let mut args = vec!["cherry-pick"];
    args.extend(commits.iter().map(String::as_str));
    git(repo, &args)
}

pub fn revert(repo: &Path, commit: &str) -> Result<String> {
    git(repo, &["revert", "--no-edit", commit])
}

pub fn reset(repo: &Path, to: &str, mode: ResetMode) -> Result<String> {
    let flag = match mode {
        ResetMode::Soft => "--soft",
        ResetMode::Mixed => "--mixed",
        ResetMode::Hard => "--hard",
    };
    git(repo, &["reset", flag, to])
}

/// Move um branch que não está em checkout para outro commit (fast-forward).
pub fn move_branch(repo: &Path, branch: &str, to: &str) -> Result<String> {
    git(repo, &["branch", "-f", branch, to])
}

pub fn fetch_all(repo: &Path) -> Result<String> {
    git(repo, &["fetch", "--all", "--prune", "--tags"])
}

pub fn fetch_remote_branch(repo: &Path, remote: &str, branch: &str) -> Result<String> {
    git(repo, &["fetch", remote, &format!("+refs/heads/{branch}:refs/remotes/{remote}/{branch}")])
}

pub fn fetch_refspec(repo: &Path, remote: &str, refspec: &str) -> Result<String> {
    git(repo, &["fetch", remote, refspec])
}

pub fn fetch_remote(repo: &Path, remote: &str) -> Result<String> {
    git(repo, &["fetch", remote, "--prune"])
}

pub fn pull(repo: &Path, mode: PullMode) -> Result<String> {
    let flag = match mode {
        PullMode::FastForwardIfPossible => "--no-rebase",
        PullMode::FastForwardOnly => "--ff-only",
        PullMode::Rebase => "--rebase",
    };
    git(repo, &["pull", flag, "--no-edit"])
}

pub fn push(repo: &Path, remote: &str, branch: &str, set_upstream: bool, force: bool) -> Result<String> {
    let mut args = vec!["push"];
    if set_upstream {
        args.push("-u");
    }
    if force {
        args.push("--force-with-lease");
    }
    args.extend([remote, branch]);
    git(repo, &args)
}

pub fn stash_save(repo: &Path, message: &str, include_untracked: bool) -> Result<String> {
    let mut args = vec!["stash", "push"];
    if include_untracked {
        args.push("--include-untracked");
    }
    if !message.trim().is_empty() {
        args.extend(["-m", message]);
    }
    git(repo, &args)
}

pub fn stash_apply(repo: &Path, index: usize) -> Result<String> {
    git(repo, &["stash", "apply", &format!("stash@{{{index}}}")])
}

pub fn stash_pop(repo: &Path, index: usize) -> Result<String> {
    git(repo, &["stash", "pop", &format!("stash@{{{index}}}")])
}

pub fn stash_drop(repo: &Path, index: usize) -> Result<String> {
    git(repo, &["stash", "drop", &format!("stash@{{{index}}}")])
}

pub fn abort(repo: &Path, state: super::model::OpState) -> Result<String> {
    use super::model::OpState::*;
    match state {
        Merge => git(repo, &["merge", "--abort"]),
        Rebase => git(repo, &["rebase", "--abort"]),
        CherryPick => git(repo, &["cherry-pick", "--abort"]),
        Revert => git(repo, &["revert", "--abort"]),
        _ => Err(anyhow!("nenhuma operação em andamento")),
    }
}

pub fn continue_op(repo: &Path, state: super::model::OpState) -> Result<String> {
    use super::model::OpState::*;
    let env = [("GIT_EDITOR", "true")];
    match state {
        Merge => git_with(repo, &["commit", "--no-edit"], None, &env),
        Rebase => git_with(repo, &["rebase", "--continue"], None, &env),
        CherryPick => git_with(repo, &["cherry-pick", "--continue"], None, &env),
        Revert => git_with(repo, &["revert", "--continue"], None, &env),
        _ => Err(anyhow!("nenhuma operação em andamento")),
    }
}

pub fn skip_op(repo: &Path) -> Result<String> {
    git(repo, &["rebase", "--skip"])
}

/// Resolve um conflito ficando inteiramente com um dos lados.
pub fn take_side(repo: &Path, path: &str, ours: bool) -> Result<String> {
    git(repo, &["checkout", if ours { "--ours" } else { "--theirs" }, "--", path])?;
    git(repo, &["add", "--", path])
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RebaseAction {
    Pick,
    Reword,
    Squash,
    Fixup,
    Drop,
}

impl RebaseAction {
    pub const ALL: [RebaseAction; 5] = [Self::Pick, Self::Reword, Self::Squash, Self::Fixup, Self::Drop];

    pub fn label(self) -> &'static str {
        match self {
            Self::Pick => "pick",
            Self::Reword => "reword",
            Self::Squash => "squash",
            Self::Fixup => "fixup",
            Self::Drop => "drop",
        }
    }
}

pub struct RebaseStep {
    pub sha: String,
    pub action: RebaseAction,
    /// Nova mensagem quando `action == Reword`.
    pub message: String,
}

/// Rebase interativo não-interativo: escrevemos o todo e injetamos com
/// `GIT_SEQUENCE_EDITOR`. Rewords viram `pick` + `exec git commit --amend`.
/// `steps` vem na ordem de aplicação (mais antigo primeiro).
pub fn interactive_rebase(repo: &Path, base: &str, steps: &[RebaseStep]) -> Result<String> {
    let dir = std::env::temp_dir().join(format!("brio-git-rebase-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let mut todo = String::new();
    for (i, s) in steps.iter().enumerate() {
        match s.action {
            RebaseAction::Reword => {
                let msg_file = dir.join(format!("msg-{i}"));
                std::fs::write(&msg_file, &s.message)?;
                todo.push_str(&format!("pick {}\n", s.sha));
                todo.push_str(&format!("exec git commit --amend --only --no-verify -F '{}'\n", crate::wsl::for_git_shell(repo, &msg_file)));
            }
            a => todo.push_str(&format!("{} {}\n", a.label(), s.sha)),
        }
    }
    let todo_file = dir.join("todo");
    std::fs::write(&todo_file, todo)?;
    let editor = format!("cp '{}'", crate::wsl::for_git_shell(repo, &todo_file));
    let result = git_with(repo, &["rebase", "-i", base], None, &[("GIT_SEQUENCE_EDITOR", &editor), ("GIT_EDITOR", "true")]);
    let _ = std::fs::remove_dir_all(&dir);
    result
}

pub fn add_remote(repo: &Path, name: &str, url: &str) -> Result<String> {
    git(repo, &["remote", "add", name, url])
}

pub fn remove_remote(repo: &Path, name: &str) -> Result<String> {
    git(repo, &["remote", "remove", name])
}

pub fn config_get(repo: &Path, key: &str) -> String {
    git(repo, &["config", "--get", key]).map(|s| s.trim().to_string()).unwrap_or_default()
}

pub fn config_set(repo: &Path, key: &str, value: &str, global: bool) -> Result<String> {
    if global { git(repo, &["config", "--global", key, value]) } else { git(repo, &["config", key, value]) }
}

// ---- Worktrees -------------------------------------------------------------

/// Cria um worktree em `path`; com `new_branch` cria o branch a partir de `base`.
pub fn worktree_add(repo: &Path, path: &str, branch: &str, new_branch: bool, base: &str) -> Result<String> {
    if new_branch {
        git(repo, &["worktree", "add", "-b", branch, path, base])
    } else {
        git(repo, &["worktree", "add", path, branch])
    }
}

pub fn worktree_remove(repo: &Path, path: &str, force: bool) -> Result<String> {
    if force { git(repo, &["worktree", "remove", "--force", path]) } else { git(repo, &["worktree", "remove", path]) }
}

pub fn worktree_lock(repo: &Path, path: &str, lock: bool) -> Result<String> {
    git(repo, &["worktree", if lock { "lock" } else { "unlock" }, path])
}

// ---- Submodules ------------------------------------------------------------

pub fn submodule_add(repo: &Path, url: &str, path: &str) -> Result<String> {
    git(repo, &["submodule", "add", "--", url, path])
}

pub fn submodule_update(repo: &Path, path: Option<&str>) -> Result<String> {
    let mut args = vec!["submodule", "update", "--init", "--recursive"];
    if let Some(p) = path {
        args.extend(["--", p]);
    }
    git(repo, &args)
}

// ---- Git LFS ---------------------------------------------------------------

pub fn lfs_available() -> bool {
    std::process::Command::new("git")
        .args(["lfs", "version"])
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Padrões rastreados pelo LFS, lidos do `.gitattributes` da raiz.
pub fn lfs_patterns(repo: &Path) -> Vec<String> {
    std::fs::read_to_string(repo.join(".gitattributes"))
        .unwrap_or_default()
        .lines()
        .filter(|l| l.contains("filter=lfs"))
        .filter_map(|l| l.split_whitespace().next().map(str::to_string))
        .collect()
}

pub fn lfs_track(repo: &Path, pattern: &str) -> Result<String> {
    git(repo, &["lfs", "track", pattern])
}

pub fn lfs_untrack(repo: &Path, pattern: &str) -> Result<String> {
    git(repo, &["lfs", "untrack", pattern])
}

pub fn lfs_pull(repo: &Path) -> Result<String> {
    git(repo, &["lfs", "pull"])
}

pub fn lfs_install(repo: &Path) -> Result<String> {
    git(repo, &["lfs", "install", "--local"])
}

// ---- Assinatura ------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signature {
    None,
    Good,
    Bad,
    /// Assinado, mas a chave não é confiável ou não pôde ser verificada.
    Unverified,
}

/// Status de assinatura do commit (`%G?`: G/U/X/Y/R/E/B/N).
pub fn signature_status(repo: &Path, sha: &str) -> (Signature, String) {
    let out = git(repo, &["log", "-1", "--format=%G?%x1f%GS", sha]).unwrap_or_default();
    let (code, signer) = out.trim().split_once('\x1f').unwrap_or((out.trim(), ""));
    let s = match code {
        "G" => Signature::Good,
        "B" => Signature::Bad,
        "U" | "X" | "Y" | "R" | "E" => Signature::Unverified,
        _ => Signature::None,
    };
    (s, signer.to_string())
}
