//! Branch switcher opened from the branch chip in the toolbar: filters the
//! branches by name, switches to one, or creates a branch from the text typed.

use brio_core::RefKind;

use crate::data::RepoData;

/// Id of the filter input, focused when the picker opens.
pub const INPUT: &str = "branch-picker";

#[derive(Debug, Default)]
pub struct BranchPicker {
    pub query: String,
    /// Index into [`choices`] of the highlighted row (Enter picks it).
    pub highlighted: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    Local(String),
    /// A remote branch like `origin/feat` without a local branch of that name.
    Remote(String),
    /// Create a branch with this name at HEAD and switch to it.
    Create(String),
}

impl BranchPicker {
    /// Moves the highlight by `delta` rows, wrapping around `count` rows.
    pub fn step(&mut self, delta: i32, count: usize) {
        if count == 0 {
            self.highlighted = 0;
        } else {
            self.highlighted = (self.highlighted as i64 + delta as i64).rem_euclid(count as i64) as usize;
        }
    }
}

/// What the picker lists for `query`: the current branch, then the other
/// local branches, then remote ones not checked out locally, all matching the
/// query; and, when the query names no existing branch, an entry to create it.
pub fn choices(repo: &RepoData, query: &str) -> Vec<Choice> {
    let query = query.trim();
    let needle = query.to_lowercase();
    let matches = |name: &str| name.to_lowercase().contains(&needle);
    let current = repo.current_branch();

    let mut locals: Vec<&str> =
        repo.refs.iter().filter(|r| r.kind == RefKind::LocalBranch).map(|r| r.name.as_str()).collect();
    locals.sort_by_key(|name| Some(*name) != current);

    let remotes: Vec<&str> = repo
        .refs
        .iter()
        .filter(|r| r.kind == RefKind::RemoteBranch && !r.name.ends_with("/HEAD"))
        .map(|r| r.name.as_str())
        .filter(|name| {
            let short = name.split_once('/').map_or(*name, |(_, b)| b);
            !locals.contains(&short)
        })
        .collect();

    let exists = locals.contains(&query) || remotes.contains(&query);
    let mut out: Vec<Choice> = locals.iter().filter(|n| matches(n)).map(|n| Choice::Local((*n).to_owned())).collect();
    out.extend(remotes.iter().filter(|n| matches(n)).map(|n| Choice::Remote((*n).to_owned())));
    if !query.is_empty() && !exists && valid_name(query) {
        out.push(Choice::Create(query.to_owned()));
    }
    out
}

/// A rough check of git's branch name rules, enough to not offer names git
/// would reject outright.
fn valid_name(name: &str) -> bool {
    !name.starts_with(['-', '/', '.'])
        && !name.ends_with(['/', '.'])
        && !name.ends_with(".lock")
        && !name.contains("..")
        && !name.contains("//")
        && !name.contains("@{")
        && !name.chars().any(|c| c.is_whitespace() || c.is_control() || "~^:?*[\\".contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn repo() -> (tempfile::TempDir, RepoData) {
        let dir = tempfile::tempdir().unwrap();
        let git = brio_ops::Git::new(dir.path());
        for args in [
            &["init", "-q", "-b", "main"][..],
            &["config", "user.name", "Ana"],
            &["config", "user.email", "ana@x.dev"],
            &["config", "commit.gpgsign", "false"],
        ] {
            git.run(args).unwrap();
        }
        fs::write(dir.path().join("a.txt"), "one\n").unwrap();
        git.run(&["add", "."]).unwrap();
        git.run(&["commit", "-qm", "first"]).unwrap();
        git.run(&["branch", "feat/login"]).unwrap();
        git.run(&["branch", "fix-typo"]).unwrap();
        // Fake remote branches: one also local, one only remote, and the symbolic HEAD.
        git.run(&["update-ref", "refs/remotes/origin/main", "HEAD"]).unwrap();
        git.run(&["update-ref", "refs/remotes/origin/feat/blog", "HEAD"]).unwrap();
        git.run(&["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/main"]).unwrap();
        git.run(&["checkout", "-q", "fix-typo"]).unwrap();
        let data = RepoData::load(dir.path(), None).unwrap();
        (dir, data)
    }

    #[test]
    fn lists_current_first_then_locals_then_remote_only() {
        let (_d, data) = repo();
        assert_eq!(
            choices(&data, ""),
            vec![
                Choice::Local("fix-typo".into()),
                Choice::Local("feat/login".into()),
                Choice::Local("main".into()),
                Choice::Remote("origin/feat/blog".into()),
            ]
        );
    }

    #[test]
    fn filters_and_offers_to_create() {
        let (_d, data) = repo();
        assert_eq!(
            choices(&data, "FEAT"),
            vec![Choice::Local("feat/login".into()), Choice::Remote("origin/feat/blog".into()), Choice::Create("FEAT".into())]
        );
        assert_eq!(choices(&data, " feat/login "), vec![Choice::Local("feat/login".into())], "exact name: nothing to create");
        assert_eq!(choices(&data, "nova-tela"), vec![Choice::Create("nova-tela".into())]);
        assert!(choices(&data, "com espaço").is_empty(), "invalid names are not offered");
    }

    #[test]
    fn highlight_wraps_around() {
        let mut picker = BranchPicker::default();
        picker.step(-1, 3);
        assert_eq!(picker.highlighted, 2);
        picker.step(1, 3);
        assert_eq!(picker.highlighted, 0);
        picker.step(1, 0);
        assert_eq!(picker.highlighted, 0);
    }
}
