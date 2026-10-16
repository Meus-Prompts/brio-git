//! The details panel: working tree changes, or a commit with its
//! message, author and changed files (as a list or a folder tree).

use std::collections::{BTreeMap, HashSet};

use iced::widget::{button, checkbox, column, container, image, row, rule, scrollable, space, text, text_editor, text_input};
use iced::{Alignment, Color, Element, Fill, Font, font};
use brio_ops::files::CommitInfo;
use brio_ops::{ChangeKind, FileChange};

use crate::avatars::{self, Avatars};
use crate::data::RepoData;
use crate::actions::Op;
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
            let unstaged: Vec<&FileChange> = repo.changes.iter().filter(|c| c.unstaged).collect();
            let staged: Vec<&FileChange> = repo.changes.iter().filter(|c| c.staged).collect();
            let title = format!("FORA DO STAGE ({})", unstaged.len());
            content = content.push(files(ws, &unstaged, old_rev, None, &title, ListMode::Unstaged));
            let title = format!("EM STAGE ({})", staged.len());
            content = content.push(files(ws, &staged, old_rev, None, &title, ListMode::Staged));
            return column![scrollable(content.padding(10)).height(Fill), commit_box(ws)].into();
        }
        Selection::Compare { from, to } => {
            let to_label = to.as_deref().map_or("working tree".to_owned(), crate::menu::rev_label);
            content = content.push(text(format!("Comparando {} → {to_label}", crate::menu::rev_label(from))).size(15));
            match &ws.details {
                None => content = content.push(text("Carregando…").color(p.muted)),
                Some(Err(e)) => content = content.push(text(e).color(p.removed)),
                Some(Ok(d)) => {
                    let all: Vec<&FileChange> = d.files.iter().collect();
                    let title = format!("{} ARQUIVO(S) DIFERENTES", d.files.len());
                    content = content.push(files(ws, &all, d.old_rev.as_deref(), d.new_rev.as_deref(), &title, ListMode::Plain));
                }
            }
        }
        Selection::Commit(_) => match &ws.details {
            None => content = content.push(text("Carregando…").color(p.muted)),
            Some(Err(e)) => content = content.push(text(e).color(p.removed)),
            Some(Ok(d)) => {
                if let Some(info) = &d.info {
                    content = content.push(commit_header(info, avatars));
                }
                let title = format!("{} FILE(S) CHANGED", d.files.len());
                let all: Vec<&FileChange> = d.files.iter().collect();
                content = content.push(files(ws, &all, d.old_rev.as_deref(), d.new_rev.as_deref(), &title, ListMode::Plain));
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
        Some(handle) => image(handle.clone()).width(size).height(size).into(),
        None => container(text(avatars::initials(name)).size(size * 0.4).color(Color::WHITE))
            .center(size)
            .style(style::fill(avatars::color(email), size / 2.0))
            .into(),
    }
}

/// Which per-file buttons a list shows.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ListMode {
    Plain,
    Unstaged,
    Staged,
}

fn files<'a>(
    ws: &'a Workspace,
    changes: &[&'a FileChange],
    old_rev: Option<&str>,
    new_rev: Option<&str>,
    title: &str,
    mode_kind: ListMode,
) -> Element<'a, WsMessage> {
    let p = style::p();
    let mode = |label: &'static str, tree: bool| {
        button(text(label).size(11))
            .on_press(WsMessage::SetTreeView(tree))
            .style(if ws.tree_view == tree { button::primary } else { button::text })
            .padding([2, 8])
    };
    let bulk = match mode_kind {
        ListMode::Unstaged if !changes.is_empty() => Some(small("Stage all", WsMessage::StageAll)),
        ListMode::Staged if !changes.is_empty() => Some(small("Unstage all", WsMessage::UnstageAll)),
        _ => None,
    };
    let mut header = row![text(title.to_owned()).size(11).color(p.muted).width(Fill)].spacing(4).align_y(Alignment::Center);
    if let Some(b) = bulk {
        header = header.push(b);
    }
    if mode_kind != ListMode::Staged {
        header = header.push(mode("Lista", false)).push(mode("Árvore", true));
    }
    let mut list = column![header].spacing(1);

    if changes.is_empty() {
        return list.push(text("Nenhum arquivo alterado.").size(13).color(p.muted)).into();
    }

    let open = ws.file_view.as_ref().map(|f| &f.target);
    let entry = |change: &'a FileChange, label: String, depth: usize| -> Element<'a, WsMessage> {
        let target = FileTarget::new(change, old_rev, new_rev);
        let selected = open.is_some_and(|t| t.path == target.path && t.new_rev == target.new_rev);
        let file = button(
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
        .width(Fill);
        let path = vec![change.path.clone()];
        match mode_kind {
            ListMode::Plain => file.into(),
            ListMode::Unstaged => {
                let untracked = change.kind == ChangeKind::Untracked;
                let discard = if untracked {
                    Op::Discard { paths: Vec::new(), untracked: path.clone() }
                } else {
                    Op::Discard { paths: path.clone(), untracked: Vec::new() }
                };
                row![file, icon("⟲", WsMessage::Request(discard)), icon("+", WsMessage::Request(Op::Stage(path)))]
                    .align_y(Alignment::Center)
                    .into()
            }
            ListMode::Staged => row![file, icon("−", WsMessage::Request(Op::Unstage(path)))].align_y(Alignment::Center).into(),
        }
    };

    if ws.tree_view {
        let paths: Vec<&str> = changes.iter().map(|c| c.path.as_str()).collect();
        for r in tree_rows(&paths, &ws.collapsed_dirs) {
            list = list.push(match r {
                TreeRow::Dir { path, name, depth, open } => Element::from(button(
                    row![
                        space().width(depth as f32 * 14.0),
                        text(format!("{} {name}", if open { "▾" } else { "▸" })).size(13).color(p.muted),
                    ],
                )
                .on_press(WsMessage::ToggleDir(path))
                .style(button::text)
                .padding([3, 6])
                .width(Fill)),
                TreeRow::File { index, name, depth } => entry(changes[index], name, depth),
            });
        }
    } else {
        for change in changes {
            list = list.push(entry(change, change.path.clone(), 0));
        }
    }
    list.into()
}

fn small<'a>(label: &'static str, msg: WsMessage) -> Element<'a, WsMessage> {
    button(text(label).size(11)).on_press(msg).style(button::secondary).padding([2, 8]).into()
}

fn icon<'a>(label: &'static str, msg: WsMessage) -> Element<'a, WsMessage> {
    button(text(label).size(14).font(Font::MONOSPACE)).on_press(msg).style(button::text).padding([0, 6]).into()
}

/// Summary, description, amend, AI message and the commit button.
fn commit_box(ws: &Workspace) -> Element<'_, WsMessage> {
    let p = style::p();
    let staged = ws.repo.as_ref().is_some_and(|r| r.changes.iter().any(|c| c.staged));
    let can_commit = (staged || ws.amend) && !ws.commit_summary.trim().is_empty() && ws.busy.is_none();
    let ai_label = match (&ws.tool, ws.generating) {
        (_, true) => "Gerando…".to_owned(),
        (Some(t), false) => format!("✨ Gerar com {t}"),
        (None, false) => "✨ Gerar com IA".to_owned(),
    };
    let commit_label = if ws.amend { "Amend" } else { "Commit" };
    container(
        column![
            text(if ws.amend { "ALTERAR ÚLTIMO COMMIT (AMEND)" } else { "MENSAGEM DO COMMIT" }).size(11).color(p.muted),
            text_input("Resumo (obrigatório)", &ws.commit_summary).on_input(WsMessage::CommitSummary).on_submit(WsMessage::DoCommit).padding(6),
            text_editor(&ws.commit_body).placeholder("Descrição (opcional)").on_action(WsMessage::CommitBody).height(80).padding(6),
            row![
                checkbox(ws.amend).label("Amend").on_toggle(WsMessage::ToggleAmend).size(14).text_size(13),
                space::horizontal(),
                button(text(ai_label).size(12))
                    .on_press_maybe((!ws.generating && staged).then_some(WsMessage::GenerateMessage))
                    .style(button::secondary)
                    .padding([4, 10]),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            button(text(commit_label).size(14).width(Fill).align_x(Alignment::Center))
                .on_press_maybe(can_commit.then_some(WsMessage::DoCommit))
                .style(button::primary)
                .padding([6, 14])
                .width(Fill),
        ]
        .spacing(6),
    )
    .padding(10)
    .style(style::bar)
    .into()
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
