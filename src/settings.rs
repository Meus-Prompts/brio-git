//! Preferências persistidas em `~/.config/brio-git/settings.json`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Columns {
    pub refs: bool,
    pub graph: bool,
    pub message: bool,
    pub author: bool,
    pub date: bool,
    pub sha: bool,
    pub refs_width: f32,
    pub author_width: f32,
    pub date_width: f32,
}

impl Default for Columns {
    fn default() -> Self {
        Self {
            refs: true,
            graph: true,
            message: true,
            author: false,
            date: false,
            sha: true,
            refs_width: 170.0,
            author_width: 140.0,
            date_width: 140.0,
        }
    }
}

/// Tamanhos que o usuário ajustou arrastando; `None` = proporcional à janela.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    pub left: Option<f32>,
    pub right: Option<f32>,
    pub terminal: Option<f32>,
    /// Tamanho interno da janela em pixels lógicos (sem o zoom do app).
    pub window: Option<[f32; 2]>,
    pub maximized: bool,
}

/// Identidade usada nos commits feitos pelo brio-git (não altera o gitconfig).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub label: String,
    pub name: String,
    pub email: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub recent: Vec<PathBuf>,
    pub favorites: Vec<PathBuf>,
    pub open_tabs: Vec<PathBuf>,
    pub active_tab: usize,
    pub columns: Columns,
    pub max_commits: usize,
    pub autofetch_minutes: u32,
    pub show_stashes: bool,
    pub show_body_in_graph: bool,
    pub zoom: f32,
    pub default_branch: String,
    pub clone_dir: Option<PathBuf>,
    pub date_format: String,
    pub terminal_open: bool,
    pub skip_hooks: bool,
    /// Id do tema (ver `theme::themes()`).
    pub theme: String,
    pub profiles: Vec<Profile>,
    /// `None` = perfil padrão (identidade do gitconfig).
    pub active_profile: Option<usize>,
    /// Hosts GitHub Enterprise / GitLab self-hosted com nome não óbvio.
    pub hosting_hosts: Vec<crate::hosting::HostConfig>,
    /// Barra de título própria (sem decorações do sistema), como o cliente de referência.
    pub custom_titlebar: bool,
    /// Fotos dos autores (GitHub noreply, GitLab conectado, Gravatar).
    pub show_avatars: bool,
    /// Atualização automática pelas Releases do GitHub.
    pub auto_update: bool,
    pub layout: Layout,
    /// Alterna entre `theme_light` e `theme_dark` conforme o tema do sistema.
    pub follow_system_theme: bool,
    pub theme_light: String,
    pub theme_dark: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            recent: Vec::new(),
            favorites: Vec::new(),
            open_tabs: Vec::new(),
            active_tab: 0,
            columns: Columns::default(),
            max_commits: 5000,
            autofetch_minutes: 1,
            show_stashes: true,
            show_body_in_graph: true,
            zoom: 1.0,
            default_branch: "main".into(),
            clone_dir: dirs::home_dir().map(|h| h.join("Development")),
            date_format: "%d/%m/%Y %H:%M".into(),
            terminal_open: false,
            skip_hooks: false,
            theme: "dark".into(),
            profiles: Vec::new(),
            active_profile: None,
            hosting_hosts: Vec::new(),
            custom_titlebar: true,
            show_avatars: true,
            auto_update: true,
            layout: Layout::default(),
            follow_system_theme: false,
            theme_light: "light".into(),
            theme_dark: "dark".into(),
        }
    }
}

fn file() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("brio-git").join("settings.json"))
}

impl Settings {
    pub fn profile(&self) -> Option<&Profile> {
        self.active_profile.and_then(|i| self.profiles.get(i)).filter(|p| !p.name.trim().is_empty() && !p.email.trim().is_empty())
    }

    /// Tema a aplicar: o fixo, ou o par claro/escuro seguindo o sistema.
    pub fn effective_theme(&self, system: Option<egui::Theme>) -> &str {
        match (self.follow_system_theme, system) {
            (true, Some(egui::Theme::Light)) => &self.theme_light,
            (true, Some(egui::Theme::Dark)) => &self.theme_dark,
            _ => &self.theme,
        }
    }

    pub fn load() -> Self {
        let Some(path) = file() else { return Self::default() };
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            // Primeira execução: herda abas, tema e preferência de atualização
            // de quem vem do brio-git em iced (mesmo diretório, `state.json`).
            Err(_) => path.parent().and_then(|dir| std::fs::read_to_string(dir.join("state.json")).ok()).map(|t| Self::from_iced_state(&t)).unwrap_or_default(),
        }
    }

    /// Converte o `state.json` do brio-git em iced. Campos ausentes ficam no padrão.
    pub fn from_iced_state(text: &str) -> Self {
        let mut s = Self::default();
        let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else { return s };
        if let Some(tabs) = v["tabs"].as_array() {
            s.open_tabs = tabs.iter().filter_map(|t| t.as_str()).map(PathBuf::from).collect();
            s.recent = s.open_tabs.clone();
        }
        s.active_tab = v["active"].as_u64().unwrap_or(0) as usize;
        if let Some(auto) = v["auto_update"].as_bool() {
            s.auto_update = auto;
        }
        if let Some(name) = v["theme"].as_str() {
            s.theme = crate::theme::themes()
                .into_iter()
                .find(|(_, n)| n.eq_ignore_ascii_case(name))
                .map(|(id, _)| id.to_string())
                .unwrap_or_else(|| match name {
                    "Light" | "Solarized Light" => "light".into(),
                    _ => "dark".into(),
                });
        }
        s
    }

    pub fn save(&self) {
        let Some(f) = file() else { return };
        if let Some(dir) = f.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(f, json);
        }
    }

    pub fn touch_recent(&mut self, path: &Path) {
        self.recent.retain(|p| p != path);
        self.recent.insert(0, path.to_path_buf());
        self.recent.truncate(30);
    }
}

fn lock_file() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("brio-git").join("instance.lock"))
}

/// Mantém o lock da instância primária aberto enquanto o processo vive.
static PRIMARY: std::sync::Mutex<Option<std::fs::File>> = std::sync::Mutex::new(None);

/// Tenta tornar esta instância a primária (dona de abas e autosave), com um
/// lock exclusivo do SO no arquivo: liberado automaticamente se o app fechar
/// ou travar, sem depender de PID. Secundárias não sobrescrevem a primária.
pub fn acquire_primary() -> bool {
    let mut held = PRIMARY.lock().unwrap();
    if held.is_some() {
        return true;
    }
    let Some(f) = lock_file() else { return true };
    if let Some(dir) = f.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let Ok(file) = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(&f) else { return true };
    match file.try_lock() {
        Ok(()) => {
            *held = Some(file);
            true
        }
        Err(_) => false,
    }
}

pub fn release_primary() {
    if let Some(file) = PRIMARY.lock().unwrap().take() {
        let _ = file.unlock();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_theme_follows_the_system_only_when_enabled() {
        let mut s = Settings { theme: "dracula".into(), theme_light: "catppuccin-latte".into(), theme_dark: "nord".into(), ..Default::default() };
        assert_eq!(s.effective_theme(Some(egui::Theme::Light)), "dracula");
        s.follow_system_theme = true;
        assert_eq!(s.effective_theme(Some(egui::Theme::Light)), "catppuccin-latte");
        assert_eq!(s.effective_theme(Some(egui::Theme::Dark)), "nord");
        assert_eq!(s.effective_theme(None), "dracula", "sistema sem preferência = tema fixo");
    }

    #[test]
    fn old_settings_without_layout_still_load() {
        let s: Settings = serde_json::from_str(r#"{"theme":"nord"}"#).unwrap();
        assert_eq!(s.layout, Layout::default());
        assert!(!s.follow_system_theme);
        assert_eq!((s.theme_light.as_str(), s.theme_dark.as_str()), ("light", "dark"));
    }

    #[test]
    fn imports_state_from_the_iced_version() {
        let json = r#"{"tabs":["/repo/a","/repo/b"],"active":1,"theme":"Rosé Pine","auto_update":false,
            "layout":{"sidebar":0.2},"pinned":{},"commit_prompt":"x","look":"y"}"#;
        let s = Settings::from_iced_state(json);
        assert_eq!(s.open_tabs, vec![PathBuf::from("/repo/a"), PathBuf::from("/repo/b")]);
        assert_eq!(s.active_tab, 1);
        assert_eq!(s.theme, "rose-pine");
        assert!(!s.auto_update);
        assert_eq!(Settings::from_iced_state(r#"{"theme":"Dark"}"#).theme, "dark");
        assert_eq!(Settings::from_iced_state(r#"{"theme":"Solarized Light"}"#).theme, "light");
        assert_eq!(Settings::from_iced_state(r#"{"theme":"Tokyo Night"}"#).theme, "tokyo-night");
        assert_eq!(Settings::from_iced_state("lixo").open_tabs.len(), 0, "state inválido = padrão");
    }
}
