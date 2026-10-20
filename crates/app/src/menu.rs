//! Context menus: what can be done with a commit, branch, tag or stash.

use brio_ops::actions::ResetMode;

use crate::actions::{CheckoutTarget, Op, short};
use crate::data::RepoData;
use crate::workspace::{DialogKind, WsMessage};

/// What was right-clicked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// The "uncommitted changes" row of the graph.
    Worktree,
    Commit(String),
    /// Local branch, or remote branch like `origin/feat` when `remote`.
    Branch { name: String, remote: bool },
    Tag(String),
    Stash(String),
}

#[derive(Debug, Clone)]
pub struct MenuItem {
    pub label: String,
    /// `None` shows the item disabled.
    pub action: Option<WsMessage>,
    pub children: Vec<MenuItem>,
    pub danger: bool,
    /// Draws a divider above the item.
    pub divider: bool,
}

impl MenuItem {
    fn new(label: impl Into<String>, action: WsMessage) -> Self {
        Self { label: label.into(), action: Some(action), children: Vec::new(), danger: false, divider: false }
    }

    fn disabled(label: impl Into<String>) -> Self {
        Self { label: label.into(), action: None, children: Vec::new(), danger: false, divider: false }
    }

    fn submenu(label: impl Into<String>, children: Vec<MenuItem>) -> Self {
        Self { label: label.into(), action: None, children, danger: false, divider: false }
    }

    fn danger(mut self) -> Self {
        self.danger = true;
        self
    }

    fn divided(mut self) -> Self {
        self.divider = true;
        self
    }
}

fn op(label: impl Into<String>, op: Op) -> MenuItem {
    MenuItem::new(label, WsMessage::Request(op))
}

fn reset_menu(branch: &str, rev: &str) -> Vec<MenuItem> {
    let item = |label: &str, mode| op(label, Op::Reset { mode, rev: rev.to_owned(), branch: branch.to_owned() });
    vec![
        item("Soft (mantém as alterações em stage)", ResetMode::Soft),
        item("Mixed (mantém as alterações fora do stage)", ResetMode::Mixed),
        item("Hard (descarta todas as alterações)", ResetMode::Hard).danger(),
    ]
}

pub fn build(repo: &RepoData, pinned: Option<&str>, target: &Target) -> Vec<MenuItem> {
    let current = repo.current_branch();
    let cur = current.unwrap_or("HEAD");
    let remote = repo.main_remote();
    match target {
        Target::Worktree => worktree_menu(repo),
        Target::Commit(id) => commit_menu(repo, id, cur),
        Target::Branch { name, remote: false } => local_branch_menu(repo, name, current, cur, remote, pinned),
        Target::Branch { name, remote: true } => remote_branch_menu(name, cur, pinned),
        Target::Tag(name) => tag_menu(name, remote),
        Target::Stash(name) => vec![
            op("Aplicar", Op::StashApply(name.clone())),
            op("Aplicar e remover (pop)", Op::StashPop(name.clone())),
            op("Descartar", Op::StashDrop(name.clone())).danger().divided(),
        ],
    }
}

fn worktree_menu(repo: &RepoData) -> Vec<MenuItem> {
    let unstaged = repo.changes.iter().filter(|c| c.unstaged);
    let (untracked, paths): (Vec<_>, Vec<_>) = unstaged.partition(|c| c.kind == brio_ops::ChangeKind::Untracked);
    let names = |v: Vec<&brio_ops::FileChange>| v.into_iter().map(|c| c.path.clone()).collect::<Vec<_>>();
    let mut discard = op(
        "Descartar alterações fora do stage…",
        Op::Discard { paths: names(paths), untracked: names(untracked) },
    )
    .danger()
    .divided();
    if !repo.changes.iter().any(|c| c.unstaged) {
        discard.action = None;
    }
    vec![
        MenuItem::new("Fazer stage de tudo", WsMessage::StageAll),
        MenuItem::new("Remover tudo do stage", WsMessage::UnstageAll),
        op("Guardar no stash", Op::StashPush).divided(),
        discard,
    ]
}

fn commit_menu(repo: &RepoData, id: &str, cur: &str) -> Vec<MenuItem> {
    let is_head = repo.head.id.as_deref() == Some(id);
    let mut items = vec![
        MenuItem::new("Copiar SHA do commit", WsMessage::CopyText(id.to_owned())),
        MenuItem::new("Copiar mensagem do commit", WsMessage::CopyCommitMessage(id.to_owned())),
        MenuItem::new("Criar branch aqui…", WsMessage::OpenDialog(DialogKind::CreateBranch { at: id.to_owned() })).divided(),
        MenuItem::new("Criar tag aqui…", WsMessage::OpenDialog(DialogKind::CreateTag { at: id.to_owned(), name: String::new() })),
        MenuItem::new("Checkout deste commit", WsMessage::CheckoutCommit(id.to_owned())),
    ];
    let mut cherry = op("Cherry-pick", Op::CherryPick(id.to_owned())).divided();
    if is_head {
        cherry.action = None;
    }
    items.push(cherry);
    items.push(op("Reverter commit", Op::Revert(id.to_owned())));
    items.push(MenuItem::disabled("Rebase interativo a partir daqui… (em breve)"));
    let mut reset = MenuItem::submenu(format!("Resetar {cur} para este commit"), reset_menu(cur, id));
    if is_head {
        reset.children.retain(|c| c.danger);
    }
    items.push(reset);
    items.push(MenuItem::new("Comparar com o working tree", WsMessage::Compare { from: id.to_owned(), to: None }).divided());
    if let (false, Some(head)) = (is_head, &repo.head.id) {
        items.push(MenuItem::new("Comparar com HEAD", WsMessage::Compare { from: id.to_owned(), to: Some(head.clone()) }));
    }
    items
}

fn local_branch_menu(
    repo: &RepoData,
    name: &str,
    current: Option<&str>,
    cur: &str,
    remote: Option<&str>,
    pinned: Option<&str>,
) -> Vec<MenuItem> {
    let is_current = current == Some(name);
    let mut items = Vec::new();
    if is_current {
        items.push(op("Push", Op::Push { branch: name.to_owned(), force: false }));
        items.push(op("Force push (with lease)…", Op::Push { branch: name.to_owned(), force: true }));
        if repo.upstream.is_some() {
            items.push(op("Pull", Op::Pull));
        }
        items.push(op("Fetch", Op::Fetch));
    } else {
        items.push(MenuItem::new(format!("Checkout {name}"), WsMessage::Checkout(CheckoutTarget::Branch(name.to_owned()))));
        if current.is_some() {
            items.push(op(format!("Merge {name} em {cur}"), Op::Merge(name.to_owned())).divided());
            items.push(op(format!("Rebase {cur} sobre {name}"), Op::Rebase(name.to_owned())));
            items.push(MenuItem::submenu(format!("Resetar {cur} para {name}"), reset_menu(cur, name)));
        }
        items.push(MenuItem::new(format!("Comparar {name} com {cur}"), WsMessage::Compare { from: cur.to_owned(), to: Some(name.to_owned()) }));
        if let (Some(remote), Some(current)) = (remote, current) {
            items.push(op(
                format!("Criar merge request: {current} → {name}"),
                Op::MergeRequest { remote: remote.to_owned(), source: current.to_owned(), target: name.to_owned() },
            ));
        }
        items.push(op(format!("Push {name}"), Op::Push { branch: name.to_owned(), force: false }));
    }
    items.push(MenuItem::new("Copiar nome da branch", WsMessage::CopyText(name.to_owned())).divided());
    items.push(MenuItem::new("Renomear…", WsMessage::OpenDialog(DialogKind::RenameBranch { old: name.to_owned() })));
    items.push(MenuItem::new("Criar branch a partir daqui…", WsMessage::OpenDialog(DialogKind::CreateBranch { at: name.to_owned() })));
    items.push(tag_from_branch(name, name));
    items.push(pin_item(name, pinned));
    if !is_current {
        items.push(op(format!("Apagar {name}"), Op::DeleteBranch { name: name.to_owned(), force: true }).danger().divided());
    }
    if let Some(remote) = remote {
        let on_remote = repo.refs.iter().any(|r| r.name == format!("{remote}/{name}"));
        if on_remote {
            items.push(op(format!("Apagar {name} em {remote}"), Op::DeleteRemoteBranch { remote: remote.to_owned(), name: name.to_owned() }).danger());
        }
    }
    items
}

/// Pins `name` (a local or remote branch) to the first lane of the graph, or unpins it.
fn pin_item(name: &str, pinned: Option<&str>) -> MenuItem {
    let label = if pinned == Some(name) { "Desafixar da esquerda" } else { "Fixar à esquerda" };
    MenuItem::new(label, WsMessage::TogglePin(name.to_owned()))
}

/// "Create tag" on the tip of branch `at`, named like the branch (without the remote).
fn tag_from_branch(at: &str, name: &str) -> MenuItem {
    MenuItem::new("Criar tag a partir daqui…", WsMessage::OpenDialog(DialogKind::CreateTag { at: at.to_owned(), name: name.to_owned() }))
}

fn remote_branch_menu(name: &str, cur: &str, pinned: Option<&str>) -> Vec<MenuItem> {
    let (remote, branch) = name.split_once('/').unwrap_or(("origin", name));
    vec![
        MenuItem::new(format!("Checkout (cria {branch} local)"), WsMessage::Checkout(CheckoutTarget::Remote(name.to_owned()))),
        op(format!("Merge {name} em {cur}"), Op::Merge(name.to_owned())).divided(),
        op(format!("Rebase {cur} sobre {name}"), Op::Rebase(name.to_owned())),
        MenuItem::submenu(format!("Resetar {cur} para {name}"), reset_menu(cur, name)),
        MenuItem::new(format!("Comparar {name} com {cur}"), WsMessage::Compare { from: cur.to_owned(), to: Some(name.to_owned()) }),
        MenuItem::new("Copiar nome da branch", WsMessage::CopyText(branch.to_owned())).divided(),
        MenuItem::new("Criar branch a partir daqui…", WsMessage::OpenDialog(DialogKind::CreateBranch { at: name.to_owned() })),
        tag_from_branch(name, branch),
        pin_item(name, pinned),
        op(format!("Apagar {branch} em {remote}"), Op::DeleteRemoteBranch { remote: remote.to_owned(), name: branch.to_owned() })
            .danger()
            .divided(),
    ]
}

fn tag_menu(name: &str, remote: Option<&str>) -> Vec<MenuItem> {
    let mut items = vec![
        MenuItem::new("Copiar nome da tag", WsMessage::CopyText(name.to_owned())),
        MenuItem::new("Checkout da tag", WsMessage::Checkout(CheckoutTarget::Detached(name.to_owned()))),
        MenuItem::new("Criar branch a partir daqui…", WsMessage::OpenDialog(DialogKind::CreateBranch { at: name.to_owned() })),
    ];
    if let Some(remote) = remote {
        items.push(op(format!("Enviar tag para {remote}"), Op::PushTag { remote: remote.to_owned(), name: name.to_owned() }));
    }
    items.push(op("Apagar tag", Op::DeleteTag(name.to_owned())).danger().divided());
    if let Some(remote) = remote {
        items.push(op(format!("Apagar tag em {remote}"), Op::DeleteRemoteTag { remote: remote.to_owned(), name: name.to_owned() }).danger());
    }
    items
}

/// Short label used in dialogs for a revision.
pub fn rev_label(rev: &str) -> String {
    if rev.len() == 40 && rev.bytes().all(|b| b.is_ascii_hexdigit()) { short(rev).to_owned() } else { rev.to_owned() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag_dialog(item: &MenuItem) -> Option<&DialogKind> {
        match &item.action {
            Some(WsMessage::OpenDialog(kind @ DialogKind::CreateTag { .. })) => Some(kind),
            _ => None,
        }
    }

    #[test]
    fn tag_from_a_branch_is_named_after_it() {
        let local = tag_from_branch("feat/x", "feat/x");
        assert_eq!(tag_dialog(&local), Some(&DialogKind::CreateTag { at: "feat/x".into(), name: "feat/x".into() }));

        // A remote branch: tagged at the remote ref, named without the remote.
        let remote = tag_from_branch("origin/feat/x", "feat/x");
        assert_eq!(tag_dialog(&remote), Some(&DialogKind::CreateTag { at: "origin/feat/x".into(), name: "feat/x".into() }));
    }
}
