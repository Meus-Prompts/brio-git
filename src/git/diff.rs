//! Diffs (commit, working dir, index), patches parciais para staging por
//! hunk/linha, blame e histórico de arquivo.

use std::collections::HashSet;

use anyhow::{Result, anyhow};
use git2::{Delta, Diff, DiffFindOptions, DiffOptions, Oid, Patch, Repository};

use super::model::{ChangeKind, FileChange};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffSource {
    Commit(Oid),
    /// Mudanças do working dir ainda não adicionadas ao índice.
    Unstaged,
    /// Mudanças do índice em relação ao HEAD.
    Staged,
    /// Diferença entre dois commits (`from` → `to`).
    Range(Oid, Oid),
}

#[derive(Clone, Debug)]
pub struct DiffLine {
    pub origin: char,
    pub old_no: Option<u32>,
    pub new_no: Option<u32>,
    pub content: String,
    /// A linha não termina com `\n` no arquivo original.
    pub no_eol: bool,
}

#[derive(Clone, Debug)]
pub struct Hunk {
    pub header: String,
    pub old_start: u32,
    pub new_start: u32,
    pub lines: Vec<DiffLine>,
}

#[derive(Clone, Debug)]
pub struct FileDiff {
    pub path: String,
    pub old_path: String,
    pub kind: ChangeKind,
    pub binary: bool,
    pub hunks: Vec<Hunk>,
    pub additions: usize,
    pub deletions: usize,
}

fn base_opts(path: Option<&str>) -> DiffOptions {
    let mut o = DiffOptions::new();
    o.context_lines(3).include_typechange(true);
    if let Some(p) = path {
        o.pathspec(p).disable_pathspec_match(true);
    }
    o
}

fn commit_diff<'r>(repo: &'r Repository, oid: Oid, path: Option<&str>) -> Result<Diff<'r>> {
    let commit = repo.find_commit(oid)?;
    let tree = commit.tree()?;
    let parent_tree = match commit.parent(0) {
        Ok(p) => Some(p.tree()?),
        Err(_) => None,
    };
    let mut o = base_opts(path);
    let mut diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), Some(&mut o))?;
    find_renames(&mut diff)?;
    Ok(diff)
}

fn find_renames(diff: &mut Diff) -> Result<()> {
    let mut f = DiffFindOptions::new();
    f.renames(true).copies(false);
    diff.find_similar(Some(&mut f))?;
    Ok(())
}

fn source_diff<'r>(repo: &'r Repository, source: DiffSource, path: Option<&str>) -> Result<Diff<'r>> {
    match source {
        DiffSource::Commit(oid) => commit_diff(repo, oid, path),
        DiffSource::Range(from, to) => {
            let a = repo.find_commit(from)?.tree()?;
            let b = repo.find_commit(to)?.tree()?;
            let mut o = base_opts(path);
            let mut d = repo.diff_tree_to_tree(Some(&a), Some(&b), Some(&mut o))?;
            find_renames(&mut d)?;
            Ok(d)
        }
        DiffSource::Unstaged => {
            let mut o = base_opts(path);
            o.include_untracked(true).recurse_untracked_dirs(true).show_untracked_content(true);
            Ok(repo.diff_index_to_workdir(None, Some(&mut o))?)
        }
        DiffSource::Staged => {
            let head = repo.head().ok().and_then(|h| h.peel_to_tree().ok());
            let mut o = base_opts(path);
            let mut d = repo.diff_tree_to_index(head.as_ref(), None, Some(&mut o))?;
            find_renames(&mut d)?;
            Ok(d)
        }
    }
}

fn kind_of(delta: Delta) -> ChangeKind {
    match delta {
        Delta::Added => ChangeKind::Added,
        Delta::Deleted => ChangeKind::Deleted,
        Delta::Renamed => ChangeKind::Renamed,
        Delta::Typechange => ChangeKind::TypeChange,
        Delta::Untracked => ChangeKind::Untracked,
        Delta::Conflicted => ChangeKind::Conflicted,
        _ => ChangeKind::Modified,
    }
}

/// Lista de arquivos alterados com contagem de linhas.
pub fn changed_files(repo: &Repository, source: DiffSource) -> Result<Vec<FileChange>> {
    let diff = source_diff(repo, source, None)?;
    let mut out = Vec::new();
    for i in 0..diff.deltas().len() {
        let delta = diff.get_delta(i).ok_or_else(|| anyhow!("delta {i}"))?;
        let path = delta.new_file().path().or(delta.old_file().path()).map(|p| p.display().to_string()).unwrap_or_default();
        let old = delta.old_file().path().map(|p| p.display().to_string()).filter(|p| *p != path);
        let (additions, deletions) = match Patch::from_diff(&diff, i) {
            Ok(Some(p)) => p.line_stats().map(|(_, a, d)| (a, d)).unwrap_or((0, 0)),
            _ => (0, 0),
        };
        out.push(FileChange { path, old_path: old, kind: kind_of(delta.status()), additions, deletions });
    }
    Ok(out)
}

pub fn file_diff(repo: &Repository, source: DiffSource, path: &str) -> Result<FileDiff> {
    let diff = source_diff(repo, source, Some(path))?;
    let idx = (0..diff.deltas().len())
        .find(|&i| {
            let d = diff.get_delta(i).unwrap();
            d.new_file().path().map(|p| p.to_string_lossy() == path).unwrap_or(false)
                || d.old_file().path().map(|p| p.to_string_lossy() == path).unwrap_or(false)
        })
        .ok_or_else(|| anyhow!("{path}: sem alterações"))?;
    let delta = diff.get_delta(idx).unwrap();
    let old_path = delta.old_file().path().map(|p| p.display().to_string()).unwrap_or_else(|| path.to_string());
    let kind = kind_of(delta.status());
    let mut fd = FileDiff {
        path: path.to_string(),
        old_path,
        kind,
        binary: delta.flags().is_binary(),
        hunks: Vec::new(),
        additions: 0,
        deletions: 0,
    };
    let Some(patch) = Patch::from_diff(&diff, idx)? else {
        fd.binary = true;
        return Ok(fd);
    };
    for h in 0..patch.num_hunks() {
        let (hunk, n) = patch.hunk(h)?;
        let mut lines: Vec<DiffLine> = Vec::with_capacity(n);
        for l in 0..n {
            let line = patch.line_in_hunk(h, l)?;
            match line.origin() {
                '=' | '>' | '<' => {
                    if let Some(last) = lines.last_mut() {
                        last.no_eol = true;
                    }
                }
                origin => {
                    match origin {
                        '+' => fd.additions += 1,
                        '-' => fd.deletions += 1,
                        _ => {}
                    }
                    let content = String::from_utf8_lossy(line.content()).trim_end_matches(['\n', '\r']).to_string();
                    lines.push(DiffLine { origin, old_no: line.old_lineno(), new_no: line.new_lineno(), content, no_eol: false });
                }
            }
        }
        fd.hunks.push(Hunk {
            header: String::from_utf8_lossy(hunk.header()).trim_end().to_string(),
            old_start: hunk.old_start(),
            new_start: hunk.new_start(),
            lines,
        });
    }
    Ok(fd)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatchMode {
    /// Aplicar no índice (stage) um patch vindo do diff unstaged.
    Stage,
    /// Remover do índice (`--reverse`) um patch vindo do diff staged.
    Unstage,
    /// Descartar do working dir (`--reverse`) um patch vindo do diff unstaged.
    Discard,
}

/// Monta um patch unificado contendo apenas as linhas selecionadas do hunk.
/// `selected == None` significa o hunk inteiro.
pub fn partial_patch(fd: &FileDiff, hunk_idx: usize, selected: Option<&HashSet<usize>>, mode: PatchMode) -> Option<String> {
    let hunk = fd.hunks.get(hunk_idx)?;
    let mut body = String::new();
    let (mut old_n, mut new_n, mut changes) = (0u32, 0u32, 0usize);
    for (i, line) in hunk.lines.iter().enumerate() {
        let picked = selected.is_none_or(|s| s.contains(&i));
        let out = match (line.origin, picked, mode) {
            (' ', _, _) => Some(' '),
            ('+' | '-', true, _) => Some(line.origin),
            // Stage: o lado "antigo" é o índice; remoções não escolhidas continuam lá.
            ('-', false, PatchMode::Stage) => Some(' '),
            ('+', false, PatchMode::Stage) => None,
            // Unstage/Discard aplicam ao contrário contra o lado "novo".
            ('+', false, _) => Some(' '),
            ('-', false, _) => None,
            _ => None,
        };
        let Some(origin) = out else { continue };
        match origin {
            ' ' => {
                old_n += 1;
                new_n += 1;
            }
            '-' => {
                old_n += 1;
                changes += 1;
            }
            '+' => {
                new_n += 1;
                changes += 1;
            }
            _ => {}
        }
        body.push(origin);
        body.push_str(&line.content);
        body.push('\n');
        if line.no_eol {
            body.push_str("\\ No newline at end of file\n");
        }
    }
    if changes == 0 {
        return None;
    }
    let (old_file, new_file) = match fd.kind {
        ChangeKind::Added | ChangeKind::Untracked => ("/dev/null".to_string(), format!("b/{}", fd.path)),
        ChangeKind::Deleted => (format!("a/{}", fd.old_path), "/dev/null".to_string()),
        _ => (format!("a/{}", fd.old_path), format!("b/{}", fd.path)),
    };
    let mut patch = format!("diff --git a/{} b/{}\n", fd.old_path, fd.path);
    match fd.kind {
        ChangeKind::Added | ChangeKind::Untracked => patch.push_str("new file mode 100644\n"),
        ChangeKind::Deleted => patch.push_str("deleted file mode 100644\n"),
        _ => {}
    }
    let old_start = if old_n == 0 { hunk.old_start.saturating_sub(1) } else { hunk.old_start };
    patch.push_str(&format!(
        "--- {old_file}\n+++ {new_file}\n@@ -{},{} +{},{} @@\n{body}",
        old_start, old_n, hunk.new_start, new_n
    ));
    Some(patch)
}

/// Conteúdo de um arquivo em um commit, no índice ou no working dir.
pub fn file_content(repo: &Repository, source: DiffSource, path: &str) -> Result<String> {
    let bytes = match source {
        DiffSource::Commit(oid) | DiffSource::Range(_, oid) => {
            let tree = repo.find_commit(oid)?.tree()?;
            let entry = tree.get_path(std::path::Path::new(path))?;
            repo.find_blob(entry.id())?.content().to_vec()
        }
        DiffSource::Staged => {
            let index = repo.index()?;
            let entry = index.get_path(std::path::Path::new(path), 0).ok_or_else(|| anyhow!("não está no índice"))?;
            repo.find_blob(entry.id)?.content().to_vec()
        }
        DiffSource::Unstaged => {
            let root = repo.workdir().ok_or_else(|| anyhow!("repositório bare"))?;
            std::fs::read(root.join(path))?
        }
    };
    if bytes.iter().take(8000).any(|&b| b == 0) {
        return Err(anyhow!("arquivo binário"));
    }
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

#[derive(Clone, Debug)]
pub struct BlameLine {
    pub oid: Oid,
    pub short: String,
    pub author: String,
    pub time: i64,
    pub line_no: usize,
    pub content: String,
    /// Primeira linha de um bloco do mesmo commit.
    pub block_start: bool,
}

pub fn blame(repo: &Repository, path: &str, at: Option<Oid>) -> Result<Vec<BlameLine>> {
    let mut opts = git2::BlameOptions::new();
    if let Some(oid) = at {
        opts.newest_commit(oid);
    }
    let blame = repo.blame_file(std::path::Path::new(path), Some(&mut opts))?;
    let content = match at {
        Some(oid) => file_content(repo, DiffSource::Commit(oid), path)?,
        None => file_content(repo, DiffSource::Unstaged, path)?,
    };
    let mut out = Vec::new();
    let mut last: Option<Oid> = None;
    for (i, text) in content.lines().enumerate() {
        let Some(h) = blame.get_line(i + 1) else { continue };
        let oid = h.final_commit_id();
        let sig = h.final_signature();
        out.push(BlameLine {
            oid,
            short: oid.to_string()[..7].to_string(),
            author: sig.as_ref().and_then(|s| s.name().ok()).unwrap_or("?").to_string(),
            time: sig.as_ref().map(|s| s.when().seconds()).unwrap_or(0),
            line_no: i + 1,
            content: text.to_string(),
            block_start: last != Some(oid),
        });
        last = Some(oid);
    }
    Ok(out)
}

#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub oid: Oid,
    pub short: String,
    pub summary: String,
    pub author: String,
    pub email: String,
    pub time: i64,
    pub path: String,
}

/// Histórico de um arquivo seguindo renomeações (`git log --follow`).
pub fn file_history(repo_path: &std::path::Path, path: &str, limit: usize) -> Result<Vec<HistoryEntry>> {
    let out = super::cli::git(
        repo_path,
        &["log", "--follow", "--name-only", &format!("-n{limit}"), "--format=%x1e%H%x1f%an%x1f%at%x1f%ae%x1f%s", "--", path],
    )?;
    let mut entries = Vec::new();
    for rec in out.split('\x1e').filter(|r| !r.trim().is_empty()) {
        let mut lines = rec.lines();
        let head = lines.next().unwrap_or("");
        let f: Vec<&str> = head.split('\x1f').collect();
        if f.len() < 5 {
            continue;
        }
        let Ok(oid) = Oid::from_str(f[0]) else { continue };
        let file = lines.find(|l| !l.trim().is_empty()).unwrap_or(path).to_string();
        entries.push(HistoryEntry {
            oid,
            short: f[0][..7].to_string(),
            author: f[1].to_string(),
            time: f[2].parse().unwrap_or(0),
            email: f[3].to_string(),
            summary: f[4].to_string(),
            path: file,
        });
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> FileDiff {
        let l = |origin: char, content: &str| DiffLine { origin, old_no: None, new_no: None, content: content.into(), no_eol: false };
        FileDiff {
            path: "a.txt".into(),
            old_path: "a.txt".into(),
            kind: ChangeKind::Modified,
            binary: false,
            additions: 2,
            deletions: 1,
            hunks: vec![Hunk {
                header: String::new(),
                old_start: 1,
                new_start: 1,
                lines: vec![l(' ', "ctx"), l('-', "old"), l('+', "new1"), l('+', "new2"), l(' ', "end")],
            }],
        }
    }

    #[test]
    fn stage_selected_line_turns_unselected_removal_into_context() {
        let sel: HashSet<usize> = [2].into();
        let p = partial_patch(&sample(), 0, Some(&sel), PatchMode::Stage).unwrap();
        assert!(p.contains("@@ -1,3 +1,4 @@"));
        assert!(p.contains(" old\n+new1\n"));
        assert!(!p.contains("new2"));
    }

    #[test]
    fn unstage_selected_line_keeps_unselected_addition_as_context() {
        let sel: HashSet<usize> = [1].into();
        let p = partial_patch(&sample(), 0, Some(&sel), PatchMode::Unstage).unwrap();
        assert!(p.contains("-old\n new1\n new2\n"));
        assert!(p.contains("@@ -1,5 +1,4 @@"));
    }

    #[test]
    fn empty_selection_yields_no_patch() {
        let sel: HashSet<usize> = [0].into();
        assert!(partial_patch(&sample(), 0, Some(&sel), PatchMode::Stage).is_none());
    }
}
