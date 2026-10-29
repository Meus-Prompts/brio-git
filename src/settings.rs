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

    pub fn load() -> Self {
        file()
            .and_then(|f| std::fs::read_to_string(f).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
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
