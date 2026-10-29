//! Leitura completa de um repositório via libgit2 (`git2`) em um snapshot.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Instant;

use anyhow::{Context, Result};
use git2::{BranchType, Oid, Repository, RepositoryState, Sort, Status, StatusOptions};

use super::graph;
use super::model::*;

#[derive(Clone, Debug, Default)]
pub struct LoadOptions {
    pub max_commits: usize,
    /// Refs ocultas no grafo (nome curto: `main`, `origin/main`, `v1.0`).
    pub hidden: HashSet<String>,
    /// Quando não vazio, só estes refs (e HEAD) são percorridos.
    pub solo: HashSet<String>,
    pub show_stashes: bool,
}

pub fn load(path: &Path, opts: &LoadOptions) -> Result<Snapshot> {
    let started = Instant::now();
    let mut repo = Repository::open(path).with_context(|| format!("abrindo {}", path.display()))?;

    let head = read_head(&repo);
    let stashes = read_stashes(&mut repo);
    let (locals, remotes) = read_branches(&repo)?;
    let tags = read_tags(&repo);

    let mut refs_by_commit: HashMap<Oid, Vec<RefLabel>> = HashMap::new();
    let mut tips: Vec<(String, Oid)> = Vec::new();
    for b in &locals {
        refs_by_commit.entry(b.oid).or_default().push(RefLabel {
            name: b.name.clone(),
            kind: RefKind::Local,
            is_head: b.is_head,
        });
        tips.push((b.name.clone(), b.oid));
    }
    for r in &remotes {
        for b in &r.branches {
            let full = format!("{}/{}", r.name, b.name);
            refs_by_commit.entry(b.oid).or_default().push(RefLabel {
                name: full.clone(),
                kind: RefKind::Remote,
                is_head: false,
            });
            tips.push((full, b.oid));
        }
    }
    for t in &tags {
        refs_by_commit.entry(t.oid).or_default().push(RefLabel {
            name: t.name.clone(),
            kind: RefKind::Tag,
            is_head: false,
        });
        tips.push((t.name.clone(), t.oid));
    }

    let mut walk = repo.revwalk()?;
    walk.set_sorting(Sort::TOPOLOGICAL | Sort::TIME)?;
    if let Some(h) = head.oid {
        walk.push(h)?;
    }
    for (name, oid) in &tips {
        let visible = if opts.solo.is_empty() { !opts.hidden.contains(name) } else { opts.solo.contains(name) };
        if visible {
            let _ = walk.push(*oid);
        }
    }

    let max = if opts.max_commits == 0 { usize::MAX } else { opts.max_commits };
    let mut commits: Vec<CommitInfo> = Vec::new();
    let mut truncated = false;
    for oid in walk {
        if commits.len() >= max {
            truncated = true;
            break;
        }
        let oid = oid?;
        let c = repo.find_commit(oid)?;
        commits.push(commit_info(&c));
    }

    if opts.show_stashes {
        insert_stashes(&repo, &stashes, &mut commits);
    }

    let pairs: Vec<(Oid, Vec<Oid>)> = commits.iter().map(|c| (c.id, c.parents.clone())).collect();
    let graph = graph::layout(&pairs);
    let index_of = commits.iter().enumerate().map(|(i, c)| (c.id, i)).collect();

    Ok(Snapshot {
        commits,
        graph,
        index_of,
        refs_by_commit,
        submodules: read_submodules(&repo),
        worktrees: read_worktrees(&repo),
        state: op_state(&repo),
        status: read_status(&repo)?,
        locals,
        remotes,
        tags,
        stashes,
        head,
        truncated,
        gitflow: super::gitflow::read(&repo),
        lfs: lfs_installed().then(|| super::ops::lfs_patterns(path)),
        load_ms: started.elapsed().as_millis(),
    })
}

fn lfs_installed() -> bool {
    static LFS: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *LFS.get_or_init(super::ops::lfs_available)
}

pub fn commit_info(c: &git2::Commit) -> CommitInfo {
    let id = c.id();
    let message = String::from_utf8_lossy(c.message_bytes()).to_string();
    let (summary, body) = match message.split_once('\n') {
        Some((s, b)) => (s.trim().to_string(), b.trim().to_string()),
        None => (message.trim().to_string(), String::new()),
    };
    let author = c.author();
    let committer = c.committer();
    CommitInfo {
        id,
        short: id.to_string()[..7].to_string(),
        summary,
        body,
        author: author.name().unwrap_or("?").to_string(),
        email: author.email().unwrap_or("").to_string(),
        time: author.when().seconds(),
        committer: committer.name().unwrap_or("?").to_string(),
        parents: c.parent_ids().collect(),
        stash_index: None,
    }
}

/// Stashes aparecem no grafo logo acima do commit base, ligados só a ele
/// (os pais internos de índice/untracked ficam de fora, como no cliente de referência).
fn insert_stashes(repo: &Repository, stashes: &[StashInfo], commits: &mut Vec<CommitInfo>) {
    for s in stashes.iter().rev() {
        let Ok(c) = repo.find_commit(s.oid) else { continue };
        let Some(base) = c.parent_id(0).ok() else { continue };
        let Some(pos) = commits.iter().position(|x| x.id == base) else { continue };
        let mut info = commit_info(&c);
        info.parents = vec![base];
        info.summary = s.message.clone();
        info.stash_index = Some(s.index);
        commits.insert(pos, info);
    }
}

fn read_head(repo: &Repository) -> HeadInfo {
    match repo.head() {
        Ok(h) => HeadInfo {
            branch: if h.is_branch() { h.shorthand().ok().map(str::to_string) } else { None },
            oid: h.target(),
            detached: repo.head_detached().unwrap_or(false),
            unborn: false,
        },
        Err(_) => {
            // Repositório sem commits: HEAD aponta para um branch ainda não nascido.
            let branch = repo
                .find_reference("HEAD")
                .ok()
                .and_then(|r| r.symbolic_target().ok().flatten().map(|s| s.trim_start_matches("refs/heads/").to_string()));
            HeadInfo { branch, oid: None, detached: false, unborn: true }
        }
    }
}

fn read_branches(repo: &Repository) -> Result<(Vec<BranchInfo>, Vec<RemoteInfo>)> {
    let mut locals = Vec::new();
    for b in repo.branches(Some(BranchType::Local))? {
        let (b, _) = b?;
        let Some(name) = b.name()?.map(str::to_string) else { continue };
        let Some(oid) = b.get().target() else { continue };
        let upstream = b.upstream().ok();
        let (ahead, behind, upstream) = match upstream {
            Some(u) => {
                let uname = u.name().ok().flatten().map(str::to_string);
                let (a, bh) = u
                    .get()
                    .target()
                    .and_then(|uo| repo.graph_ahead_behind(oid, uo).ok())
                    .unwrap_or((0, 0));
                (a, bh, uname)
            }
            None => (0, 0, None),
        };
        locals.push(BranchInfo { name, oid, is_head: b.is_head(), upstream, ahead, behind });
    }
    locals.sort_by_key(|a| a.name.to_lowercase());

    let mut by_remote: HashMap<String, Vec<BranchInfo>> = HashMap::new();
    for b in repo.branches(Some(BranchType::Remote))? {
        let (b, _) = b?;
        let Some(full) = b.name()?.map(str::to_string) else { continue };
        let Some((remote, name)) = full.split_once('/') else { continue };
        if name == "HEAD" {
            continue;
        }
        let Some(oid) = b.get().target() else { continue };
        by_remote.entry(remote.to_string()).or_default().push(BranchInfo {
            name: name.to_string(),
            oid,
            is_head: false,
            upstream: None,
            ahead: 0,
            behind: 0,
        });
    }

    let mut remotes = Vec::new();
    for name in repo.remotes()?.iter().filter_map(|r| r.ok().flatten()) {
        // URL como configurada: o libgit2 já devolve `remote.url()` reescrita por `insteadOf`.
        let url = repo
            .config()
            .ok()
            .and_then(|c| c.get_string(&format!("remote.{name}.url")).ok())
            .or_else(|| repo.find_remote(name).ok().and_then(|r| r.url().ok().map(str::to_string)))
            .unwrap_or_default();
        let mut branches = by_remote.remove(name).unwrap_or_default();
        branches.sort_by_key(|a| a.name.to_lowercase());
        remotes.push(RemoteInfo { name: name.to_string(), url, branches });
    }
    Ok((locals, remotes))
}

fn read_tags(repo: &Repository) -> Vec<TagInfo> {
    let mut tags = Vec::new();
    let _ = repo.tag_foreach(|oid, name| {
        let name = String::from_utf8_lossy(name).trim_start_matches("refs/tags/").to_string();
        let target = match repo.find_tag(oid) {
            Ok(t) => t.target().ok().and_then(|o| o.peel_to_commit().ok()).map(|c| c.id()),
            Err(_) => Some(oid),
        };
        if let Some(target) = target {
            tags.push(TagInfo { name, oid: target });
        }
        true
    });
    tags.sort_by(|a, b| b.name.cmp(&a.name));
    tags
}

fn read_stashes(repo: &mut Repository) -> Vec<StashInfo> {
    let mut stashes = Vec::new();
    let _ = repo.stash_foreach(|index, message, oid| {
        stashes.push(StashInfo { index, message: message.to_string(), oid: *oid });
        true
    });
    stashes
}

fn read_submodules(repo: &Repository) -> Vec<SubmoduleInfo> {
    repo.submodules()
        .map(|subs| {
            subs.iter()
                .map(|s| SubmoduleInfo {
                    name: s.name().unwrap_or("?").to_string(),
                    path: s.path().display().to_string(),
                    head: s.workdir_id(),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn read_worktrees(repo: &Repository) -> Vec<WorktreeInfo> {
    let Ok(names) = repo.worktrees() else { return Vec::new() };
    names
        .iter()
        .filter_map(|n| n.ok().flatten())
        .filter_map(|n| repo.find_worktree(n).ok())
        .map(|w| WorktreeInfo {
            name: w.name().ok().flatten().unwrap_or("?").to_string(),
            path: w.path().display().to_string(),
            locked: matches!(w.is_locked(), Ok(git2::WorktreeLockStatus::Locked(_))),
        })
        .collect()
}

fn op_state(repo: &Repository) -> OpState {
    match repo.state() {
        RepositoryState::Clean => OpState::Clean,
        RepositoryState::Merge => OpState::Merge,
        RepositoryState::Rebase | RepositoryState::RebaseInteractive | RepositoryState::RebaseMerge => OpState::Rebase,
        RepositoryState::CherryPick | RepositoryState::CherryPickSequence => OpState::CherryPick,
        RepositoryState::Revert | RepositoryState::RevertSequence => OpState::Revert,
        RepositoryState::Bisect => OpState::Bisect,
        _ => OpState::Other,
    }
}

pub fn read_status(repo: &Repository) -> Result<WorkStatus> {
    let mut opts = StatusOptions::new();
    opts.include_untracked(true)
        .recurse_untracked_dirs(true)
        .renames_head_to_index(true)
        .renames_index_to_workdir(true)
        .include_ignored(false);
    let statuses = repo.statuses(Some(&mut opts))?;
    let mut ws = WorkStatus::default();
    for e in statuses.iter() {
        let s = e.status();
        let path = e.path().unwrap_or("").to_string();
        if s.is_conflicted() {
            ws.conflicted.push(change(path, None, ChangeKind::Conflicted));
            continue;
        }
        let staged_kind = if s.contains(Status::INDEX_NEW) {
            Some(ChangeKind::Added)
        } else if s.contains(Status::INDEX_MODIFIED) {
            Some(ChangeKind::Modified)
        } else if s.contains(Status::INDEX_DELETED) {
            Some(ChangeKind::Deleted)
        } else if s.contains(Status::INDEX_RENAMED) {
            Some(ChangeKind::Renamed)
        } else if s.contains(Status::INDEX_TYPECHANGE) {
            Some(ChangeKind::TypeChange)
        } else {
            None
        };
        if let Some(kind) = staged_kind {
            let old = e
                .head_to_index()
                .and_then(|d| d.old_file().path().map(|p| p.display().to_string()))
                .filter(|p| *p != path);
            ws.staged.push(change(path.clone(), old, kind));
        }
        let unstaged_kind = if s.contains(Status::WT_NEW) {
            Some(ChangeKind::Untracked)
        } else if s.contains(Status::WT_MODIFIED) {
            Some(ChangeKind::Modified)
        } else if s.contains(Status::WT_DELETED) {
            Some(ChangeKind::Deleted)
        } else if s.contains(Status::WT_RENAMED) {
            Some(ChangeKind::Renamed)
        } else if s.contains(Status::WT_TYPECHANGE) {
            Some(ChangeKind::TypeChange)
        } else {
            None
        };
        if let Some(kind) = unstaged_kind {
            ws.unstaged.push(change(path, None, kind));
        }
    }
    for list in [&mut ws.staged, &mut ws.unstaged, &mut ws.conflicted] {
        list.sort_by(|a, b| a.path.cmp(&b.path));
    }
    Ok(ws)
}

fn change(path: String, old_path: Option<String>, kind: ChangeKind) -> FileChange {
    FileChange { path, old_path, kind, additions: 0, deletions: 0 }
}

#[cfg(test)]
mod bench {
    /// `BRIO_GIT_BENCH_REPO=/caminho cargo test --release bench_load -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn bench_load() {
        let Ok(path) = std::env::var("BRIO_GIT_BENCH_REPO") else { return };
        for max in [5000usize, 0] {
            let opts = super::LoadOptions { max_commits: max, show_stashes: true, ..Default::default() };
            let t = std::time::Instant::now();
            let s = super::load(std::path::Path::new(&path), &opts).unwrap();
            println!(
                "max={max:>5}: {} commits, {} lanes, {} refs, status {} arquivos — {} ms",
                s.commits.len(),
                s.graph.max_cols,
                s.refs_by_commit.values().map(Vec::len).sum::<usize>(),
                s.status.total(),
                t.elapsed().as_millis()
            );
        }
    }
}
