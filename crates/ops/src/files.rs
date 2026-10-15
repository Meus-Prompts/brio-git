//! File-level views of history: changed files between revisions, file
//! contents at a revision, commit details, blame and per-file history.

use crate::git::{Git, Result};
use crate::status::{ChangeKind, FileChange};

/// Files larger than this are not loaded into the viewer.
pub const MAX_FILE_BYTES: usize = 4 * 1024 * 1024;

/// Contents of one side of a file diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Content {
    Text(String),
    Binary,
    TooLarge,
}

impl Content {
    pub fn text(&self) -> &str {
        match self {
            Content::Text(t) => t,
            _ => "",
        }
    }

    fn from_bytes(bytes: Vec<u8>) -> Self {
        if bytes.len() > MAX_FILE_BYTES {
            Content::TooLarge
        } else if bytes.iter().take(8000).any(|b| *b == 0) {
            Content::Binary
        } else {
            Content::Text(String::from_utf8_lossy(&bytes).into_owned())
        }
    }
}

/// `path` as of `rev`; `None` reads the working tree. A file that does not
/// exist there (new or deleted) is empty.
pub fn content(git: &Git, rev: Option<&str>, path: &str) -> Content {
    let bytes = match rev {
        Some(rev) => git.run_bytes(&["show", &format!("{rev}:{path}")]).ok(),
        None => std::fs::read(git.workdir().join(path)).ok(),
    };
    bytes.map_or(Content::Text(String::new()), Content::from_bytes)
}

/// Files changed from `from` to `to`; without `from`, everything `to` adds
/// (a root commit).
pub fn changed(git: &Git, from: Option<&str>, to: &str) -> Result<Vec<FileChange>> {
    let mut args = vec!["diff-tree", "-r", "-z", "-M", "--name-status", "--no-commit-id"];
    match from {
        Some(from) => args.extend([from, to]),
        None => args.extend(["--root", to]),
    }
    Ok(parse_name_status(&git.run(&args)?))
}

fn parse_name_status(out: &str) -> Vec<FileChange> {
    let mut fields = out.split('\0').filter(|f| !f.is_empty());
    let mut changes = Vec::new();
    while let Some(status) = fields.next() {
        let kind = match status.chars().next() {
            Some('A') => ChangeKind::Added,
            Some('D') => ChangeKind::Deleted,
            Some('R' | 'C') => ChangeKind::Renamed,
            Some('U') => ChangeKind::Conflicted,
            _ => ChangeKind::Modified,
        };
        let (from, path) = if kind == ChangeKind::Renamed {
            let from = fields.next().map(str::to_owned);
            (from, fields.next())
        } else {
            (None, fields.next())
        };
        if let Some(path) = path {
            changes.push(FileChange { path: path.to_owned(), from, kind, staged: false });
        }
    }
    changes
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitInfo {
    pub id: String,
    pub parents: Vec<String>,
    pub author: String,
    pub email: String,
    pub time: i64,
    pub summary: String,
    pub body: String,
}

pub fn commit_info(git: &Git, id: &str) -> Result<CommitInfo> {
    let out = git.run(&["show", "-s", "--format=%H%x00%P%x00%an%x00%ae%x00%at%x00%B", id])?;
    let mut f = out.splitn(6, '\0');
    let mut next = || f.next().unwrap_or_default().to_owned();
    let (id, parents, author, email, time, message) = (next(), next(), next(), next(), next(), next());
    let message = message.trim_end();
    let (summary, body) = message.split_once('\n').unwrap_or((message, ""));
    Ok(CommitInfo {
        id,
        parents: parents.split_whitespace().map(str::to_owned).collect(),
        author,
        email,
        time: time.trim().parse().unwrap_or_default(),
        summary: summary.trim().to_owned(),
        body: body.trim().to_owned(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlameLine {
    /// Commit id; all zeros for lines not committed yet.
    pub id: String,
    pub author: String,
    pub time: i64,
    pub summary: String,
    pub text: String,
}

impl BlameLine {
    pub fn is_uncommitted(&self) -> bool {
        self.id.bytes().all(|b| b == b'0')
    }
}

/// Blame of `path` at `rev` (`None` = working tree).
pub fn blame(git: &Git, rev: Option<&str>, path: &str) -> Result<Vec<BlameLine>> {
    let mut args = vec!["blame", "--porcelain"];
    if let Some(rev) = rev {
        args.push(rev);
    }
    args.extend(["--", path]);
    Ok(parse_blame(&git.run(&args)?))
}

fn parse_blame(out: &str) -> Vec<BlameLine> {
    use std::collections::HashMap;
    let mut known: HashMap<String, (String, i64, String)> = HashMap::new();
    let mut lines = Vec::new();
    let mut current = String::new();
    let (mut author, mut time, mut summary) = (String::new(), 0, String::new());

    for line in out.lines() {
        if let Some(text) = line.strip_prefix('\t') {
            let info = known
                .entry(current.clone())
                .or_insert_with(|| (author.clone(), time, summary.clone()))
                .clone();
            lines.push(BlameLine { id: current.clone(), author: info.0, time: info.1, summary: info.2, text: text.to_owned() });
        } else if let Some(v) = line.strip_prefix("author ") {
            author = v.to_owned();
        } else if let Some(v) = line.strip_prefix("author-time ") {
            time = v.parse().unwrap_or_default();
        } else if let Some(v) = line.strip_prefix("summary ") {
            summary = v.to_owned();
        } else if let Some(id) = line.split(' ').next().filter(|w| w.len() >= 40 && w.bytes().all(|b| b.is_ascii_hexdigit())) {
            current = id.to_owned();
        }
    }
    lines
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    pub id: String,
    pub author: String,
    pub email: String,
    pub time: i64,
    pub summary: String,
    /// Path of the file in that commit (it may have been renamed since).
    pub path: String,
}

/// Commits that touched `path`, newest first, following renames.
pub fn history(git: &Git, path: &str, limit: usize) -> Result<Vec<HistoryEntry>> {
    let out = git.run(&[
        "log",
        "--follow",
        &format!("-n{limit}"),
        "--format=%x1e%H%x1f%an%x1f%ae%x1f%at%x1f%s",
        "--name-only",
        "--",
        path,
    ])?;
    Ok(out
        .split('\x1e')
        .filter_map(|record| {
            let mut lines = record.lines();
            let header = lines.next()?;
            let mut f = header.split('\x1f');
            let mut next = || f.next().unwrap_or_default().to_owned();
            let (id, author, email, time, summary) = (next(), next(), next(), next(), next());
            if id.is_empty() {
                return None;
            }
            let file = lines.rfind(|l| !l.trim().is_empty()).unwrap_or(path).to_owned();
            Some(HistoryEntry { id, author, email, time: time.parse().unwrap_or_default(), summary, path: file })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn repo() -> (tempfile::TempDir, Git) {
        let dir = tempfile::tempdir().unwrap();
        let git = Git::new(dir.path());
        for args in [
            &["init", "-q"][..],
            &["config", "user.name", "Ana"],
            &["config", "user.email", "ana@x.dev"],
            &["config", "commit.gpgsign", "false"],
        ] {
            git.run(args).unwrap();
        }
        fs::write(dir.path().join("a.txt"), "one\ntwo\n").unwrap();
        git.run(&["add", "."]).unwrap();
        git.run(&["commit", "-q", "-m", "first\n\nlonger body\nline 2"]).unwrap();
        fs::write(dir.path().join("a.txt"), "one\nTWO\n").unwrap();
        fs::write(dir.path().join("b.bin"), [0u8, 1, 2]).unwrap();
        git.run(&["add", "."]).unwrap();
        git.run(&["commit", "-q", "-m", "second"]).unwrap();
        git.run(&["mv", "a.txt", "renamed.txt"]).unwrap();
        git.run(&["commit", "-q", "-m", "rename"]).unwrap();
        (dir, git)
    }

    #[test]
    fn changed_files_between_revisions() {
        let (_d, git) = repo();
        let second: Vec<_> = changed(&git, Some("HEAD~2"), "HEAD~1").unwrap().into_iter().map(|c| (c.letter(), c.path)).collect();
        assert_eq!(second, vec![('M', "a.txt".to_owned()), ('A', "b.bin".to_owned())]);
        let rename = changed(&git, Some("HEAD~1"), "HEAD").unwrap();
        assert_eq!(rename[0].kind, ChangeKind::Renamed);
        assert_eq!(rename[0].from.as_deref(), Some("a.txt"));
        assert_eq!(rename[0].path, "renamed.txt");
        let root = changed(&git, None, "HEAD~2").unwrap();
        assert_eq!(root.len(), 1);
    }

    #[test]
    fn reads_contents_at_revisions() {
        let (dir, git) = repo();
        assert_eq!(content(&git, Some("HEAD~2"), "a.txt"), Content::Text("one\ntwo\n".into()));
        assert_eq!(content(&git, Some("HEAD"), "b.bin"), Content::Binary);
        assert_eq!(content(&git, Some("HEAD~2"), "b.bin"), Content::Text(String::new()), "missing file is empty");
        fs::write(dir.path().join("renamed.txt"), "edited\n").unwrap();
        assert_eq!(content(&git, None, "renamed.txt").text(), "edited\n");
    }

    #[test]
    fn commit_info_splits_message() {
        let (_d, git) = repo();
        let info = commit_info(&git, "HEAD~2").unwrap();
        assert_eq!(info.summary, "first");
        assert_eq!(info.body, "longer body\nline 2");
        assert_eq!((info.author.as_str(), info.email.as_str()), ("Ana", "ana@x.dev"));
        assert!(info.parents.is_empty());
        assert_eq!(commit_info(&git, "HEAD").unwrap().parents.len(), 1);
    }

    #[test]
    fn blame_attributes_lines() {
        let (dir, git) = repo();
        let lines = blame(&git, Some("HEAD"), "renamed.txt").unwrap();
        let summary: Vec<_> = lines.iter().map(|l| (l.text.as_str(), l.summary.as_str())).collect();
        assert_eq!(summary, vec![("one", "first"), ("TWO", "second")]);

        fs::write(dir.path().join("renamed.txt"), "one\nTWO\nnew\n").unwrap();
        let live = blame(&git, None, "renamed.txt").unwrap();
        assert!(live[2].is_uncommitted());
    }

    #[test]
    fn history_follows_renames() {
        let (_d, git) = repo();
        let h = history(&git, "renamed.txt", 50).unwrap();
        let entries: Vec<_> = h.iter().map(|e| (e.summary.as_str(), e.path.as_str())).collect();
        assert_eq!(entries, vec![("rename", "renamed.txt"), ("second", "a.txt"), ("first", "a.txt")]);
        assert_eq!(h[0].email, "ana@x.dev");
    }
}
