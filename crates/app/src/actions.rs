//! Git operations the UI can trigger. Each [`Op`] knows its label, whether it
//! needs confirmation and how to run off the UI thread.

use brio_ops::Git;
use brio_ops::actions::{self as git_actions, InProgress, ResetMode};
use brio_ops::undo::{self, Move, Snapshot};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckoutTarget {
    Branch(String),
    /// A remote branch such as `origin/feat`; checked out as a tracking branch.
    Remote(String),
    Detached(String),
}

impl CheckoutTarget {
    pub fn label(&self) -> String {
        match self {
            CheckoutTarget::Branch(b) | CheckoutTarget::Remote(b) => b.clone(),
            CheckoutTarget::Detached(id) => short(id).to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    Checkout(CheckoutTarget),
    /// Stashes local changes, then checks out.
    StashAndCheckout(CheckoutTarget),
    CreateBranch { name: String, at: String, checkout: bool },
    RenameBranch { old: String, new: String },
    DeleteBranch { name: String, force: bool },
    DeleteRemoteBranch { remote: String, name: String },
    CreateTag { name: String, at: String, message: Option<String> },
    DeleteTag(String),
    DeleteRemoteTag { remote: String, name: String },
    PushTag { remote: String, name: String },
    CherryPick(String),
    Revert(String),
    Reset { mode: ResetMode, rev: String, branch: String },
    Merge(String),
    Rebase(String),
    Fetch,
    Pull,
    Push { branch: String, force: bool },
    StashPush,
    StashApply(String),
    StashPop(String),
    StashDrop(String),
    Stage(Vec<String>),
    Unstage(Vec<String>),
    Discard { paths: Vec<String>, untracked: Vec<String> },
    Commit { message: String, amend: bool },
    Continue(InProgress),
    Abort(InProgress),
    /// Opens the hosting site's "new merge request" page.
    MergeRequest { remote: String, source: String, target: String },
}

/// Refs before and after an operation.
pub type Change = (Snapshot, Snapshot);

pub fn short(id: &str) -> &str {
    &id[..id.len().min(7)]
}

impl Op {
    /// Shown while the operation runs.
    pub fn progress(&self) -> String {
        match self {
            Op::Checkout(t) | Op::StashAndCheckout(t) => format!("Checkout de {}…", t.label()),
            Op::Fetch => "Buscando do remoto (fetch)…".into(),
            Op::Pull => "Fazendo pull…".into(),
            Op::Push { branch, .. } => format!("Enviando {branch} (push)…"),
            Op::Commit { amend: true, .. } => "Alterando o último commit…".into(),
            Op::Commit { .. } => "Commitando…".into(),
            Op::Merge(r) => format!("Merge de {r}…"),
            Op::Rebase(r) => format!("Rebase sobre {r}…"),
            _ => "Executando…".into(),
        }
    }

    /// Shown after success.
    pub fn done(&self) -> String {
        match self {
            Op::Checkout(t) => format!("Agora em {}.", t.label()),
            Op::StashAndCheckout(t) => format!("Alterações guardadas no stash; agora em {}.", t.label()),
            Op::CreateBranch { name, .. } => format!("Branch {name} criada."),
            Op::RenameBranch { new, .. } => format!("Branch renomeada para {new}."),
            Op::DeleteBranch { name, .. } => format!("Branch {name} apagada."),
            Op::DeleteRemoteBranch { remote, name } => format!("Branch {name} apagada em {remote}."),
            Op::CreateTag { name, .. } => format!("Tag {name} criada."),
            Op::DeleteTag(name) => format!("Tag {name} apagada."),
            Op::DeleteRemoteTag { remote, name } => format!("Tag {name} apagada em {remote}."),
            Op::PushTag { remote, name } => format!("Tag {name} enviada para {remote}."),
            Op::CherryPick(id) => format!("Cherry-pick de {} aplicado.", short(id)),
            Op::Revert(id) => format!("Commit {} revertido.", short(id)),
            Op::Reset { branch, rev, .. } => format!("{branch} agora aponta para {}.", short(rev)),
            Op::Merge(r) => format!("Merge de {r} concluído."),
            Op::Rebase(r) => format!("Rebase sobre {r} concluído."),
            Op::Fetch => "Fetch concluído.".into(),
            Op::Pull => "Pull concluído.".into(),
            Op::Push { branch, .. } => format!("{branch} enviada."),
            Op::StashPush => "Alterações guardadas no stash.".into(),
            Op::StashApply(s) => format!("{s} aplicado."),
            Op::StashPop(s) => format!("{s} aplicado e removido."),
            Op::StashDrop(s) => format!("{s} descartado."),
            Op::Stage(_) | Op::Unstage(_) => String::new(),
            Op::Discard { .. } => "Alterações descartadas.".into(),
            Op::Commit { amend: true, .. } => "Último commit alterado.".into(),
            Op::Commit { .. } => "Commit criado.".into(),
            Op::Continue(op) => format!("{} concluído.", op.label()),
            Op::Abort(op) => format!("{} abortado.", op.label()),
            Op::MergeRequest { .. } => "Página de merge request aberta no navegador.".into(),
        }
    }

    /// `Some((title, explanation, button))` when the user must confirm first.
    pub fn confirmation(&self) -> Option<(String, String, String)> {
        let c = |t: String, b: String, k: &str| Some((t, b, k.to_owned()));
        match self {
            Op::Checkout(CheckoutTarget::Detached(id)) => c(
                format!("Checkout de {}?", short(id)),
                "Você ficará em HEAD destacado (fora de qualquer branch). Commits feitos aqui só ficam salvos se você criar uma branch.".into(),
                "Fazer checkout",
            ),
            Op::Reset { mode: ResetMode::Hard, rev, branch } => c(
                format!("Reset hard de {branch} para {}?", short(rev)),
                "Todas as alterações não commitadas e os commits depois deste serão descartados.".into(),
                "Descartar e resetar",
            ),
            Op::DeleteBranch { name, .. } => c(format!("Apagar a branch {name}?"), "Commits que só existem nela deixarão de aparecer no grafo.".into(), "Apagar"),
            Op::DeleteRemoteBranch { remote, name } => c(
                format!("Apagar {name} em {remote}?"),
                "A branch some do servidor para todo o time.".into(),
                "Apagar no remoto",
            ),
            Op::DeleteTag(name) => c(format!("Apagar a tag {name}?"), "A tag local será removida.".into(), "Apagar"),
            Op::DeleteRemoteTag { remote, name } => c(format!("Apagar a tag {name} em {remote}?"), "A tag some do servidor para todo o time.".into(), "Apagar no remoto"),
            Op::Push { branch, force: true } => c(
                format!("Force push de {branch}?"),
                "Usa --force-with-lease: sobrescreve o remoto, mas falha se alguém tiver enviado algo que você ainda não buscou.".into(),
                "Force push",
            ),
            Op::Discard { paths, untracked } => c(
                "Descartar alterações?".into(),
                format!("{} arquivo(s) voltarão ao último commit; arquivos novos serão apagados. Não dá para desfazer.", paths.len() + untracked.len()),
                "Descartar",
            ),
            Op::StashDrop(s) => c(format!("Descartar {s}?"), "As alterações guardadas nesse stash serão perdidas.".into(), "Descartar"),
            Op::Revert(id) => c(format!("Reverter {}?", short(id)), "Cria um novo commit desfazendo as mudanças deste.".into(), "Reverter"),
            Op::Rebase(onto) => c(format!("Rebase sobre {onto}?"), "Reescreve os commits da branch atual por cima da outra.".into(), "Fazer rebase"),
            _ => None,
        }
    }

    /// How undo moves the checked-out branch back, or `None` when the
    /// operation cannot be undone (remote side, stash, file contents).
    pub fn undo_move(&self) -> Option<Move> {
        match self {
            Op::Commit { .. } => Some(Move::Soft),
            Op::Checkout(_)
            | Op::CreateBranch { .. }
            | Op::RenameBranch { .. }
            | Op::DeleteBranch { .. }
            | Op::CreateTag { .. }
            | Op::DeleteTag(_)
            | Op::CherryPick(_)
            | Op::Revert(_)
            | Op::Reset { .. }
            | Op::Merge(_)
            | Op::Rebase(_)
            | Op::Pull
            | Op::Continue(_) => Some(Move::Keep),
            _ => None,
        }
    }

    /// Short description for the undo/redo buttons.
    pub fn history_label(&self) -> String {
        match self {
            Op::Commit { message, amend } => {
                let summary = message.lines().next().unwrap_or_default();
                let summary: String = if summary.chars().count() > 48 {
                    summary.chars().take(47).chain(['…']).collect()
                } else {
                    summary.to_owned()
                };
                format!("{} \"{summary}\"", if *amend { "amend" } else { "commit" })
            }
            Op::Checkout(t) => format!("checkout de {}", t.label()),
            Op::CreateBranch { name, .. } => format!("criar branch {name}"),
            Op::RenameBranch { old, new } => format!("renomear {old} para {new}"),
            Op::DeleteBranch { name, .. } => format!("apagar branch {name}"),
            Op::CreateTag { name, .. } => format!("criar tag {name}"),
            Op::DeleteTag(name) => format!("apagar tag {name}"),
            Op::CherryPick(id) => format!("cherry-pick de {}", short(id)),
            Op::Revert(id) => format!("revert de {}", short(id)),
            Op::Reset { rev, .. } => format!("reset para {}", short(rev)),
            Op::Merge(r) => format!("merge de {r}"),
            Op::Rebase(r) => format!("rebase sobre {r}"),
            Op::Pull => "pull".into(),
            Op::Continue(op) => format!("{} concluído", op.label()),
            other => other.progress(),
        }
    }

    /// Runs the operation; for undoable ones, also the refs before and after
    /// (taken on the same thread, right around it), when something changed.
    pub fn run_recorded(&self, git: &Git) -> (Result<Option<String>, String>, Option<Change>) {
        let before = self.undo_move().and_then(|_| undo::snapshot(git).ok());
        let result = self.run(git);
        let change = match (&result, before) {
            (Ok(_), Some(before)) => undo::snapshot(git).ok().filter(|after| *after != before).map(|after| (before, after)),
            _ => None,
        };
        (result, change)
    }

    /// Runs the operation. Returns a URL to open, for merge requests.
    pub fn run(&self, git: &Git) -> Result<Option<String>, String> {
        use git_actions as a;
        let r = match self {
            Op::Checkout(t) => checkout(git, t),
            Op::StashAndCheckout(t) => a::stash_push(git, &format!("brio-git: antes do checkout de {}", t.label())).and_then(|()| checkout(git, t)),
            Op::CreateBranch { name, at, checkout } => a::create_branch(git, name, at, *checkout),
            Op::RenameBranch { old, new } => a::rename_branch(git, old, new),
            Op::DeleteBranch { name, force } => a::delete_branch(git, name, *force),
            Op::DeleteRemoteBranch { remote, name } => a::delete_remote_branch(git, remote, name),
            Op::CreateTag { name, at, message } => a::create_tag(git, name, at, message.as_deref()),
            Op::DeleteTag(name) => a::delete_tag(git, name),
            Op::DeleteRemoteTag { remote, name } => a::delete_remote_tag(git, remote, name),
            Op::PushTag { remote, name } => a::push_tag(git, remote, name),
            Op::CherryPick(id) => a::cherry_pick(git, id),
            Op::Revert(id) => a::revert(git, id),
            Op::Reset { mode, rev, .. } => a::reset(git, *mode, rev),
            Op::Merge(r) => a::merge(git, r),
            Op::Rebase(r) => a::rebase(git, r),
            Op::Fetch => a::fetch(git),
            Op::Pull => a::pull(git),
            Op::Push { branch, force } => a::push(git, branch, *force),
            Op::StashPush => a::stash_push(git, "brio-git: stash"),
            Op::StashApply(s) => a::stash_apply(git, s),
            Op::StashPop(s) => a::stash_pop(git, s),
            Op::StashDrop(s) => a::stash_drop(git, s),
            Op::Stage(paths) => a::stage(git, paths),
            Op::Unstage(paths) => a::unstage(git, paths),
            Op::Discard { paths, untracked } => a::discard(git, paths, untracked),
            Op::Commit { message, amend } => a::commit(git, message, *amend),
            Op::Continue(op) => a::continue_op(git, *op),
            Op::Abort(op) => a::abort_op(git, *op),
            Op::MergeRequest { remote, source, target } => {
                let url = a::remote_url(git, remote).map_err(|e| e.to_string())?;
                return a::merge_request_url(&url, source, target)
                    .map(Some)
                    .ok_or_else(|| format!("Não sei abrir merge requests para {url}."));
            }
        };
        r.map(|()| None).map_err(|e| e.to_string())
    }
}

fn checkout(git: &Git, target: &CheckoutTarget) -> brio_ops::git::Result<()> {
    match target {
        CheckoutTarget::Branch(b) => git_actions::checkout_branch(git, b),
        CheckoutTarget::Remote(r) => git_actions::checkout_remote(git, r).map(drop),
        CheckoutTarget::Detached(id) => git_actions::checkout_detached(git, id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destructive_ops_ask_first() {
        let hard = Op::Reset { mode: ResetMode::Hard, rev: "abcdef123".into(), branch: "main".into() };
        assert!(hard.confirmation().is_some());
        let soft = Op::Reset { mode: ResetMode::Soft, rev: "abcdef123".into(), branch: "main".into() };
        assert!(soft.confirmation().is_none());
        assert!(Op::DeleteRemoteBranch { remote: "origin".into(), name: "x".into() }.confirmation().is_some());
        assert!(Op::Checkout(CheckoutTarget::Detached("abc".into())).confirmation().is_some());
        assert!(Op::Checkout(CheckoutTarget::Branch("main".into())).confirmation().is_none());
    }

    #[test]
    fn runs_a_stash_and_checkout() {
        let dir = tempfile::tempdir().unwrap();
        let git = Git::new(dir.path());
        for a in [&["init", "-q", "-b", "main"][..], &["config", "user.name", "a"], &["config", "user.email", "a@a"], &["config", "commit.gpgsign", "false"]] {
            git.run(a).unwrap();
        }
        std::fs::write(dir.path().join("f"), "1\n").unwrap();
        git.run(&["add", "."]).unwrap();
        git.run(&["commit", "-qm", "c"]).unwrap();
        git.run(&["branch", "other"]).unwrap();
        std::fs::write(dir.path().join("f"), "dirty\n").unwrap();

        Op::StashAndCheckout(CheckoutTarget::Branch("other".into())).run(&git).unwrap();
        assert_eq!(git.run(&["branch", "--show-current"]).unwrap().trim(), "other");
        assert_eq!(git_actions::stash_list(&git).unwrap().len(), 1);
    }
}
