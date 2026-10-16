//! Working tree status (`git status --porcelain`).

use crate::git::{Git, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    Untracked,
    Conflicted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    pub path: String,
    /// Original path for renames and copies.
    pub from: Option<String>,
    pub kind: ChangeKind,
    /// Whether the change is (at least partly) staged.
    pub staged: bool,
    /// Whether the working tree has changes not staged yet (or is untracked).
    pub unstaged: bool,
}

impl FileChange {
    pub fn letter(&self) -> char {
        match self.kind {
            ChangeKind::Added => 'A',
            ChangeKind::Modified => 'M',
            ChangeKind::Deleted => 'D',
            ChangeKind::Renamed => 'R',
            ChangeKind::Untracked => '?',
            ChangeKind::Conflicted => '!',
        }
    }
}

pub fn status(git: &Git) -> Result<Vec<FileChange>> {
    let out = git.run(&["status", "--porcelain=v1", "-z", "--untracked-files=all"])?;
    Ok(parse(&out))
}

/// Parses `git status --porcelain=v1 -z` output.
pub fn parse(out: &str) -> Vec<FileChange> {
    let mut fields = out.split('\0').filter(|f| !f.is_empty());
    let mut changes = Vec::new();
    while let Some(entry) = fields.next() {
        if entry.len() < 4 {
            continue;
        }
        let (code, path) = entry.split_at(3);
        let mut chars = code.chars();
        let (x, y) = (chars.next().unwrap_or(' '), chars.next().unwrap_or(' '));
        let from = if matches!(x, 'R' | 'C') || matches!(y, 'R' | 'C') {
            fields.next().map(str::to_owned)
        } else {
            None
        };
        let kind = match (x, y) {
            ('?', '?') => ChangeKind::Untracked,
            ('U', _) | (_, 'U') | ('A', 'A') | ('D', 'D') => ChangeKind::Conflicted,
            ('R' | 'C', _) | (_, 'R' | 'C') => ChangeKind::Renamed,
            ('A', _) => ChangeKind::Added,
            ('D', _) | (_, 'D') => ChangeKind::Deleted,
            _ => ChangeKind::Modified,
        };
        let staged = !matches!(x, ' ' | '?');
        let unstaged = y != ' ';
        changes.push(FileChange { path: path.to_owned(), from, kind, staged, unstaged });
    }
    changes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_porcelain_entries() {
        let out = " M src/a.rs\0A  new.rs\0?? notes.txt\0R  b.rs\0old_b.rs\0UU conflict.rs\0 D gone.rs\0";
        let changes = parse(out);
        let summary: Vec<_> = changes.iter().map(|c| (c.letter(), c.path.as_str(), c.staged, c.unstaged)).collect();
        assert_eq!(
            summary,
            vec![
                ('M', "src/a.rs", false, true),
                ('A', "new.rs", true, false),
                ('?', "notes.txt", false, true),
                ('R', "b.rs", true, false),
                ('!', "conflict.rs", true, true),
                ('D', "gone.rs", false, true),
            ]
        );
        assert_eq!(changes[3].from.as_deref(), Some("old_b.rs"));
    }

    #[test]
    fn keeps_spaces_in_paths() {
        let changes = parse(" M dir with space/file name.txt\0");
        assert_eq!(changes[0].path, "dir with space/file name.txt");
    }
}
