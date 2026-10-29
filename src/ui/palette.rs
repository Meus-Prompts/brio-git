//! Paleta de comandos (Ctrl+Shift+P) com busca fuzzy simples.

use egui::{Id, Key, Modal, RichText};

use crate::git::ops::PullMode;
use crate::repo_tab::Action;
use crate::theme;

#[derive(Clone)]
pub enum Command {
    Repo(Action),
    NewTab,
    OpenRepo,
    CloseTab,
    ToggleLeft,
    ToggleRight,
    ToggleTerminal,
    Search,
    Preferences,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    Minimize,
    ToggleMaximize,
    ToggleFullscreen,
    SetTheme(&'static str),
}

#[derive(Default)]
pub struct Palette {
    pub open: bool,
    query: String,
    selected: usize,
}


/// Rótulos "Theme: Nome" criados uma vez (a paleta trabalha com `&'static str`).
static THEME_LABELS: std::sync::LazyLock<Vec<(&'static str, &'static str)>> = std::sync::LazyLock::new(|| {
    crate::theme::themes().into_iter().map(|(id, name)| (id, &*Box::leak(format!("Theme: {name}").into_boxed_str()))).collect()
});

fn commands(has_repo: bool) -> Vec<(&'static str, Command)> {
    let mut v = vec![
        ("New Tab", Command::NewTab),
        ("Open Repo", Command::OpenRepo),
        ("Preferences", Command::Preferences),
        ("Increase Zoom", Command::ZoomIn),
        ("Decrease Zoom", Command::ZoomOut),
        ("Reset Zoom", Command::ZoomReset),
        ("Maximize / Restore Window", Command::ToggleMaximize),
        ("Toggle Full Screen (F11)", Command::ToggleFullscreen),
    ];
    v.extend(THEME_LABELS.iter().map(|(id, label)| (*label, Command::SetTheme(id))));
    if crate::app::compositor_supports_minimize() {
        v.push(("Minimize Window", Command::Minimize));
    }
    if has_repo {
        v.extend([
            ("Close Tab", Command::CloseTab),
            ("Fetch All", Command::Repo(Action::Fetch)),
            ("Pull (fast-forward if possible)", Command::Repo(Action::Pull(PullMode::FastForwardIfPossible))),
            ("Pull (fast-forward only)", Command::Repo(Action::Pull(PullMode::FastForwardOnly))),
            ("Pull (rebase)", Command::Repo(Action::Pull(PullMode::Rebase))),
            ("Push", Command::Repo(Action::Push { force: false })),
            ("Stash", Command::Repo(Action::StashSave { message: String::new(), include_untracked: true })),
            ("Pop Stash", Command::Repo(Action::StashPop(0))),
            ("Stage All Changes", Command::Repo(Action::StageAll)),
            ("Unstage All Changes", Command::Repo(Action::UnstageAll)),
            ("Undo", Command::Repo(Action::Undo)),
            ("Redo", Command::Repo(Action::Redo)),
            ("Refresh", Command::Repo(Action::Refresh)),
            ("Show Work in Progress", Command::Repo(Action::SelectWip)),
            ("Toggle Left Panel", Command::ToggleLeft),
            ("Toggle Commit Details Panel", Command::ToggleRight),
            ("Toggle Terminal", Command::ToggleTerminal),
            ("New Terminal Tab", Command::Repo(Action::NewTerminal)),
            ("Search Commits", Command::Search),
            ("Unsolo All Branches", Command::Repo(Action::ClearSolo)),
            ("Open in File Manager", Command::Repo(Action::OpenFolder)),
        ]);
    }
    v
}

/// Pontuação fuzzy: todas as letras da consulta em ordem; bônus para contiguidade.
pub fn score(query: &str, text: &str) -> Option<i32> {
    let q: Vec<char> = query.to_lowercase().chars().filter(|c| !c.is_whitespace()).collect();
    if q.is_empty() {
        return Some(0);
    }
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let (mut qi, mut score, mut last) = (0, 0i32, None::<usize>);
    for (i, c) in t.iter().enumerate() {
        if qi < q.len() && *c == q[qi] {
            score += if last == Some(i.wrapping_sub(1)) { 5 } else { 1 };
            if i == 0 || t[i - 1] == ' ' {
                score += 3;
            }
            last = Some(i);
            qi += 1;
        }
    }
    (qi == q.len()).then_some(score)
}

impl Palette {
    pub fn toggle(&mut self) {
        self.open = !self.open;
        self.query.clear();
        self.selected = 0;
    }

    pub fn show(&mut self, ctx: &egui::Context, has_repo: bool) -> Option<Command> {
        if !self.open {
            return None;
        }
        let mut items: Vec<(i32, &'static str, Command)> =
            commands(has_repo).into_iter().filter_map(|(l, c)| score(&self.query, l).map(|s| (s, l, c))).collect();
        items.sort_by_key(|i| std::cmp::Reverse(i.0));
        self.selected = self.selected.min(items.len().saturating_sub(1));

        let mut chosen = None;
        let (up, down, enter, esc) =
            ctx.input(|i| (i.key_pressed(Key::ArrowUp), i.key_pressed(Key::ArrowDown), i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
        if up {
            self.selected = self.selected.saturating_sub(1);
        }
        if down && self.selected + 1 < items.len() {
            self.selected += 1;
        }
        let resp = Modal::new(Id::unique("palette")).show(ctx, |ui| {
            ui.set_width(520.0);
            let r = ui.add(egui::TextEdit::singleline(&mut self.query).hint_text("Digite um comando…").desired_width(f32::INFINITY));
            r.request_focus();
            if r.changed() {
                self.selected = 0;
            }
            ui.add_space(4.0);
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                for (i, (_, label, cmd)) in items.iter().enumerate() {
                    let sel = i == self.selected;
                    let text = RichText::new(*label).color(if sel { theme::p().text_bright } else { theme::p().text });
                    let resp = ui.add_sized([ui.available_width(), 24.0], egui::Button::selectable(sel, text));
                    if sel {
                        resp.scroll_to_me(None);
                    }
                    if resp.clicked() {
                        chosen = Some(cmd.clone());
                    }
                }
            });
        });
        if enter && let Some((_, _, c)) = items.get(self.selected) {
            chosen = Some(c.clone());
        }
        if chosen.is_some() || esc || resp.should_close() {
            self.open = false;
        }
        chosen
    }
}

#[cfg(test)]
mod tests {
    use super::score;

    #[test]
    fn fuzzy_matches_in_order_only() {
        assert!(score("pll", "Pull (rebase)").is_some());
        assert!(score("xyz", "Pull").is_none());
        assert!(score("fetch", "Fetch All").unwrap() > score("fh", "Fetch All").unwrap());
    }
}
