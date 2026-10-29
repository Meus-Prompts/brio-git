//! Aba "New Tab": Open / Clone / Create + repositórios recentes.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};

use egui::{RichText, Ui};

use crate::git::cli;
use crate::icons::{self, Icon};
use crate::settings::Settings;
use crate::theme;

#[derive(Default, PartialEq)]
enum Form {
    #[default]
    None,
    Clone,
    Init,
}

#[derive(Default)]
pub struct Home {
    form: Form,
    search: String,
    url: String,
    dest: String,
    depth: String,
    init_name: String,
    init_parent: String,
    busy: Option<(String, Receiver<anyhow::Result<PathBuf>>)>,
    error: Option<String>,
}

pub enum HomeRequest {
    Open(PathBuf),
}

fn repo_name_from_url(url: &str) -> String {
    url.trim_end_matches('/').rsplit(['/', ':']).next().unwrap_or("repo").trim_end_matches(".git").to_string()
}

pub fn pick_folder() -> Option<PathBuf> {
    rfd::FileDialog::new().set_title("Escolha uma pasta").pick_folder()
}

impl Home {
    pub fn show(&mut self, ui: &mut Ui, settings: &Settings, out: &mut Vec<HomeRequest>) {
        if let Some((label, rx)) = &self.busy
            && let Ok(res) = rx.try_recv()
        {
            let _ = label;
            match res {
                Ok(p) => {
                    out.push(HomeRequest::Open(p));
                    self.form = Form::None;
                    self.error = None;
                }
                Err(e) => self.error = Some(format!("{e:#}")),
            }
            self.busy = None;
        }

        egui::Frame::new().fill(theme::p().app_bg).inner_margin(egui::Margin { left: 90, right: 40, top: 30, bottom: 20 }).show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            ui.columns(2, |cols| {
                self.left(&mut cols[0], settings, out);
                Self::right(&mut cols[1]);
            });
        });
    }

    fn left(&mut self, ui: &mut Ui, settings: &Settings, out: &mut Vec<HomeRequest>) {
        ui.label(RichText::new("Repositories").size(22.0).color(theme::p().text_bright));
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            let big = |ui: &mut Ui, icon: Icon, label: &str| {
                let resp = ui.add_sized([100.0, 44.0], egui::Button::new(RichText::new(format!("      {label}")).size(15.0)).fill(theme::p().panel_bg1));
                icons::paint(ui.painter(), egui::Rect::from_center_size(resp.rect.left_center() + egui::vec2(20.0, 0.0), egui::vec2(18.0, 18.0)), icon, theme::p().text_bright);
                resp
            };
            if big(ui, Icon::Folder, "Open").clicked()
                && let Some(p) = pick_folder()
            {
                out.push(HomeRequest::Open(p));
            }
            if big(ui, Icon::Pull, "Clone").clicked() {
                self.form = if self.form == Form::Clone { Form::None } else { Form::Clone };
                if self.dest.is_empty() {
                    self.dest = settings.clone_dir.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
                }
            }
            if big(ui, Icon::Plus, "Create").clicked() {
                self.form = if self.form == Form::Init { Form::None } else { Form::Init };
                if self.init_parent.is_empty() {
                    self.init_parent = settings.clone_dir.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
                }
            }
        });
        ui.add_space(12.0);

        match self.form {
            Form::Clone => self.clone_form(ui),
            Form::Init => self.init_form(ui, settings),
            Form::None => {}
        }
        if let Some(e) = &self.error {
            ui.colored_label(theme::p().danger, e);
        }

        ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search repositories (Ctrl + Alt + F)").desired_width(f32::INFINITY));
        ui.add_space(10.0);
        let q = self.search.to_lowercase();
        let mut section = |ui: &mut Ui, title: &str, list: &[PathBuf]| {
            let items: Vec<&PathBuf> = list.iter().filter(|p| q.is_empty() || p.to_string_lossy().to_lowercase().contains(&q)).collect();
            if items.is_empty() {
                return;
            }
            ui.label(RichText::new(title).color(theme::p().text));
            for p in items {
                ui.horizontal(|ui| {
                    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    let resp = ui.add(egui::Label::new(RichText::new(name).color(theme::p().link).strong()).sense(egui::Sense::click()));
                    let home = dirs::home_dir().map(|h| h.display().to_string()).unwrap_or_default();
                    let shown = p.display().to_string().replacen(&home, "~", 1);
                    ui.label(RichText::new(shown).color(theme::p().text_secondary));
                    if resp.clicked() {
                        out.push(HomeRequest::Open(p.clone()));
                    }
                });
            }
            ui.add_space(8.0);
        };
        section(ui, "Favorites", &settings.favorites);
        section(ui, "Recent", &settings.recent);
    }

    fn clone_form(&mut self, ui: &mut Ui) {
        egui::Frame::new().fill(theme::p().panel_bg0).inner_margin(12).corner_radius(4).show(ui, |ui| {
            ui.label(RichText::new("Clone a repo").strong().color(theme::p().text_bright));
            egui::Grid::new("clone_grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                ui.label("URL");
                ui.add(egui::TextEdit::singleline(&mut self.url).hint_text("https://github.com/user/repo.git").desired_width(320.0));
                ui.end_row();
                ui.label("Where to clone to");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.dest).desired_width(250.0));
                    if ui.button("Browse").clicked()
                        && let Some(p) = pick_folder()
                    {
                        self.dest = p.display().to_string();
                    }
                });
                ui.end_row();
                ui.label("Shallow (depth)");
                ui.add(egui::TextEdit::singleline(&mut self.depth).hint_text("vazio = histórico completo").desired_width(120.0));
                ui.end_row();
            });
            let name = repo_name_from_url(&self.url);
            let target = PathBuf::from(&self.dest).join(&name);
            if !self.url.trim().is_empty() {
                ui.label(RichText::new(format!("Full path: {}", target.display())).small().color(theme::p().text_secondary));
            }
            ui.add_space(6.0);
            let busy = self.busy.is_some();
            ui.horizontal(|ui| {
                let ok = !self.url.trim().is_empty() && !self.dest.trim().is_empty() && !busy;
                if crate::ui::accent_button(ui, "Clone the repo!", theme::p().success, ok).clicked() {
                    let url = self.url.trim().to_string();
                    let depth = self.depth.trim().parse::<u32>().ok();
                    let (tx, rx) = channel();
                    let ctx = ui.ctx().clone();
                    std::thread::spawn(move || {
                        let res = cli::clone(&url, &target, depth).map(|_| target);
                        let _ = tx.send(res);
                        ctx.request_repaint();
                    });
                    self.busy = Some(("clone".into(), rx));
                    self.error = None;
                }
                if busy {
                    ui.spinner();
                    ui.label("Clonando…");
                }
            });
        });
        ui.add_space(10.0);
    }

    fn init_form(&mut self, ui: &mut Ui, settings: &Settings) {
        egui::Frame::new().fill(theme::p().panel_bg0).inner_margin(12).corner_radius(4).show(ui, |ui| {
            ui.label(RichText::new("Initialize a repo").strong().color(theme::p().text_bright));
            egui::Grid::new("init_grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                ui.label("Name");
                ui.add(egui::TextEdit::singleline(&mut self.init_name).desired_width(250.0));
                ui.end_row();
                ui.label("Initialize in");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.init_parent).desired_width(250.0));
                    if ui.button("Browse").clicked()
                        && let Some(p) = pick_folder()
                    {
                        self.init_parent = p.display().to_string();
                    }
                });
                ui.end_row();
            });
            let target = PathBuf::from(&self.init_parent).join(self.init_name.trim());
            let ok = !self.init_name.trim().is_empty() && !self.init_parent.trim().is_empty();
            if crate::ui::accent_button(ui, "Create Repository", theme::p().success, ok).clicked() {
                let branch = settings.default_branch.clone();
                let (tx, rx) = channel();
                std::thread::spawn(move || {
                    let _ = tx.send(cli::init(&target, &branch).map(|_| target));
                });
                self.busy = Some(("init".into(), rx));
            }
        });
        ui.add_space(10.0);
    }

    fn right(ui: &mut Ui) {
        ui.add_space(4.0);
        ui.label(RichText::new("brio-git").size(20.0).color(theme::p().text_bright));
        ui.add_space(8.0);
        ui.label("Cliente git gráfico nativo, escrito em Rust.");
        ui.add_space(14.0);
        ui.label(RichText::new("Atalhos").strong());
        for (k, d) in [
            ("Ctrl+Shift+P", "Paleta de comandos"),
            ("Ctrl+Shift+O", "Abrir repositório"),
            ("Ctrl+T", "Nova aba"),
            ("Ctrl+F", "Buscar commits"),
            ("Ctrl+L", "Fetch"),
            ("Ctrl+B", "Criar branch"),
            ("Ctrl+Shift+S / U", "Stage / Unstage all"),
            ("Ctrl+Enter", "Commit"),
            ("Ctrl+J / Ctrl+K", "Painel esquerdo / detalhes"),
            ("Ctrl+` ", "Terminal"),
            ("Ctrl+Z / Ctrl+Y", "Undo / Redo"),
        ] {
            ui.horizontal(|ui| {
                ui.label(RichText::new(k).monospace().color(theme::p().text_accent));
                ui.label(RichText::new(d).color(theme::p().text_secondary));
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::repo_name_from_url;

    #[test]
    fn derives_repo_name() {
        assert_eq!(repo_name_from_url("https://github.com/a/brio.git"), "brio");
        assert_eq!(repo_name_from_url("git@github.com:a/brio.git"), "brio");
        assert_eq!(repo_name_from_url("https://host/a/b/"), "b");
    }
}
