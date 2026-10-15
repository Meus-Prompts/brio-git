//! The details panel: working tree changes, or a commit/checkpoint with its
//! message, author and changed files (as a list or a folder tree).

use std::collections::{BTreeMap, HashSet};

use iced::widget::{button, column, container, image, row, rule, scrollable, space, text};
use iced::{Alignment, Color, Element, Fill, Font, font};
use lazygit_ops::files::CommitInfo;
use lazygit_ops::{ChangeKind, FileChange};

use crate::avatars::{self, Avatars};
use crate::data::RepoData;
use crate::fileview::FileTarget;
use crate::style;
use crate::util;
use crate::workspace::{Selection, Workspace, WsMessage};

pub fn details<'a>(ws: &'a Workspace, repo: &'a RepoData, avatars: &'a Avatars) -> Element<'a, WsMessage> {
    let p = style::p();
    let worktree = ws.selection == Selection::Worktree;
    let mut content = column![
        button(text(format!("Working tree ({})", repo.changes.len())).size(13))
            .on_press(WsMessage::ShowWorktree)
            .style(if worktree { button::primary } else { button::secondary }),
    ]
    .spacing(10);

    match &ws.selection {
        Selection::Worktree => {
            let old_rev = repo.head.id.as_ref().map(|_| "HEAD");
            content = content.push(files(ws, &repo.changes, old_rev, None, "ALTERAÇÕES NO WORKING TREE"));
        }
        Selection::Commit(_) | Selection::Checkpoint(_) => match &ws.details {
            None => content = content.push(text("Carregando…").color(p.muted)),
            Some(Err(e)) => content = content.push(text(e).color(p.removed)),
            Some(Ok(d)) => {
                if let Some(info) = &d.info {
                    content = content.push(commit_header(info, avatars));
                }
                if let Selection::Checkpoint(seq) = ws.selection
                    && let Some(cp) = ws.checkpoints.iter().find(|c| c.seq == seq)
                {
                    content = content.push(
                        row![
                            column![
                                text(format!("Checkpoint #{} — {}", cp.seq, cp.label)).size(15),
                                text(format!("{} · sessão {}", util::date_time(cp.time), cp.session)).size(12).color(p.muted),
                            ]
                            .spacing(2)
                            .width(Fill),
                            button("Restaurar").on_press(WsMessage::RestoreCheckpoint(cp.seq)).style(button::danger),
                        ]
                        .align_y(Alignment::Center),
                    );
                }
                let title = format!("{} FILE(S) CHANGED", d.files.len());
                content = content.push(files(ws, &d.files, d.old_rev.as_deref(), Some(&d.new_rev), &title));
            }
        },
    }

    scrollable(content.padding(10)).height(Fill).into()
}

fn commit_header<'a>(info: &'a CommitInfo, avatars: &'a Avatars) -> Element<'a, WsMessage> {
    let p = style::p();
    let mut header = column![
        button(text(format!("commit: {}", &info.id[..7.min(info.id.len())])).font(Font::MONOSPACE).size(12).color(p.muted))
            .on_press(WsMessage::CopyHash(info.id.clone()))
            .style(button::text)
            .padding(0),
        text(&info.summary).size(16).font(Font { weight: font::Weight::Bold, ..Font::default() }),
    ]
    .spacing(6);
    if !info.body.is_empty() {
        header = header.push(text(&info.body).size(13).color(p.muted));
    }
    header
        .push(space().height(4))
        .push(
            row![
                avatar(avatars, &info.email, &info.author, 38.0),
                column![
                    text(&info.author).size(14),
                    text(&info.email).size(12).color(p.muted),
                    text(format!("authored {}", util::date_time(info.time))).size(12).color(p.muted),
                ]
                .spacing(1),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .push(rule::horizontal(1))
        .into()
}

/// Gravatar picture, or initials on the person's color.
pub fn avatar<'a>(avatars: &'a Avatars, email: &str, name: &str, size: f32) -> Element<'a, WsMessage> {
    match avatars.get(email) {
        Some(handle) => image(handle.clone()).width(size).height(size).border_radius(size / 2.0).into(),
        None => container(text(avatars::initials(name)).size(size * 0.4).color(Color::WHITE))
            .center(size)
            .style(style::fill(avatars::color(email), size / 2.0))
            .into(),
    }
}

fn files<'a>(
    ws: &'a Workspace,
    changes: &'a [FileChange],
    old_rev: Option<&str>,
    new_rev: Option<&str>,
    title: &str,
) -> Element<'a, WsMessage> {
    let p = style::p();
    let mode = |label: &'static str, tree: bool| {
        button(text(label).size(11))
            .on_press(WsMessage::SetTreeView(tree))
            .style(if ws.tree_view == tree { button::primary } else { button::text })
            .padding([2, 8])
    };
    let mut list = column![
        row![text(title.to_owned()).size(11).color(p.muted).width(Fill), mode("Lista", false), mode("Árvore", true)]
            .spacing(4)
            .align_y(Alignment::Center),
    ]
    .spacing(1);

    if changes.is_empty() {
        return list.push(text("Nenhum arquivo alterado.").size(13).color(p.muted)).into();
    }

    let open = ws.file_view.as_ref().map(|f| &f.target);
    let entry = |change: &'a FileChange, label: String, depth: usize| {
        let target = FileTarget::new(change, old_rev, new_rev);
        let selected = open.is_some_and(|t| t.path == target.path && t.new_rev == target.new_rev);
        button(
            row![
                space().width(depth as f32 * 14.0),
                text(change.letter().to_string()).font(Font::MONOSPACE).size(12).color(kind_color(change.kind)).width(14),
                text(label).size(13),
            ]
            .spacing(4),
        )
        .on_press(WsMessage::OpenFile(target))
        .style(if selected { button::secondary } else { button::text })
        .padding([3, 6])
        .width(Fill)
    };

    if ws.tree_view {
        let paths: Vec<&str> = changes.iter().map(|c| c.path.as_str()).collect();
        for r in tree_rows(&paths, &ws.collapsed_dirs) {
            list = list.push(match r {
                TreeRow::Dir { path, name, depth, open } => button(
                    row![
                        space().width(depth as f32 * 14.0),
                        text(format!("{} {name}", if open { "▾" } else { "▸" })).size(13).color(p.muted),
                    ],
                )
                .on_press(WsMessage::ToggleDir(path))
                .style(button::text)
                .padding([3, 6])
                .width(Fill),
                TreeRow::File { index, name, depth } => entry(&changes[index], name, depth),
            });
        }
    } else {
        for change in changes {
            list = list.push(entry(change, change.path.clone(), 0));
        }
    }
    list.into()
}

fn kind_color(kind: ChangeKind) -> Color {
    let p = style::p();
    match kind {
        ChangeKind::Added | ChangeKind::Untracked => p.added,
        ChangeKind::Deleted => p.removed,
        ChangeKind::Conflicted => p.warn,
        ChangeKind::Modified | ChangeKind::Renamed => p.accent,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeRow {
    /// `path` identifies the folder (for collapsing); `name` may span several
    /// folders when they have a single child, like `features/cart/components`.
    Dir { path: String, name: String, depth: usize, open: bool },
    File { index: usize, name: String, depth: usize },
}

#[derive(Default)]
struct Dir {
    dirs: BTreeMap<String, Dir>,
    files: Vec<(String, usize)>,
}

/// Flattens `paths` into folder and file rows, skipping collapsed folders.
pub fn tree_rows(paths: &[&str], collapsed: &HashSet<String>) -> Vec<TreeRow> {
    let mut root = Dir::default();
    for (index, path) in paths.iter().enumerate() {
        let mut parts: Vec<&str> = path.split('/').collect();
        let file = parts.pop().unwrap_or_default();
        let mut dir = &mut root;
        for part in parts {
            dir = dir.dirs.entry(part.to_owned()).or_default();
        }
        dir.files.push((file.to_owned(), index));
    }
    let mut rows = Vec::new();
    emit(&root, "", 0, collapsed, &mut rows);
    rows
}

fn emit(dir: &Dir, prefix: &str, depth: usize, collapsed: &HashSet<String>, rows: &mut Vec<TreeRow>) {
    for (name, mut child) in &dir.dirs {
        // Merge chains of single folders into one row.
        let mut label = name.clone();
        while child.files.is_empty() && child.dirs.len() == 1 {
            let (next_name, next) = child.dirs.iter().next().expect("one child");
            label = format!("{label}/{next_name}");
            child = next;
        }
        let path = if prefix.is_empty() { label.clone() } else { format!("{prefix}/{label}") };
        let open = !collapsed.contains(&path);
        rows.push(TreeRow::Dir { path: path.clone(), name: label, depth, open });
        if open {
            emit(child, &path, depth + 1, collapsed, rows);
        }
    }
    for (name, index) in &dir.files {
        rows.push(TreeRow::File { index: *index, name: name.clone(), depth });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(rows: &[TreeRow]) -> Vec<String> {
        rows.iter()
            .map(|r| match r {
                TreeRow::Dir { name, depth, open, .. } => format!("{}{}{name}/", "  ".repeat(*depth), if *open { "" } else { "+" }),
                TreeRow::File { name, depth, .. } => format!("{}{name}", "  ".repeat(*depth)),
            })
            .collect()
    }

    #[test]
    fn builds_a_compact_tree() {
        let paths = ["features/cart/components/Form.vue", "features/cart/api.ts", "README.md", "src/main.rs"];
        let rows = tree_rows(&paths, &HashSet::new());
        assert_eq!(
            render(&rows),
            vec![
                "features/cart/",
                "  components/",
                "    Form.vue",
                "  api.ts",
                "src/",
                "  main.rs",
                "README.md",
            ]
        );
        assert!(matches!(rows[2], TreeRow::File { index: 0, .. }));
    }

    #[test]
    fn single_folder_chains_merge_and_collapse() {
        let paths = ["a/b/c/file.rs"];
        let rows = tree_rows(&paths, &HashSet::new());
        assert_eq!(render(&rows), vec!["a/b/c/", "  file.rs"]);

        let collapsed: HashSet<String> = ["a/b/c".to_owned()].into();
        assert_eq!(render(&tree_rows(&paths, &collapsed)), vec!["+a/b/c/"]);
    }
}
