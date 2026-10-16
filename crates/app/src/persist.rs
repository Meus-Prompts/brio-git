//! Saved state between runs: open tabs and panel layout.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::layout::Layout;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Saved {
    pub tabs: Vec<PathBuf>,
    pub active: usize,
    pub layout: Layout,
    pub theme: String,
    /// Work tree path -> branch pinned to the left of the graph.
    pub pinned: HashMap<String, String>,
}

impl Default for Saved {
    fn default() -> Self {
        Self { tabs: Vec::new(), active: 0, layout: Layout::default(), theme: "Dark".into(), pinned: HashMap::new() }
    }
}

pub fn default_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("brio-git").join("state.json"))
}

/// Loads saved state; a missing or broken file yields the defaults.
pub fn load(path: &Path) -> Saved {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn save(path: &Path, saved: &Saved) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // Write then rename so a crash never leaves a half-written file.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(saved)?)?;
    std::fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::PaneKind;

    #[test]
    fn round_trip_and_tolerates_garbage() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/state.json");
        assert_eq!(load(&path), Saved::default());

        let mut layout = Layout::default();
        layout.set_visible(PaneKind::Sidebar, false);
        let pinned = HashMap::from([("/a".to_owned(), "main".to_owned())]);
        let saved = Saved { tabs: vec!["/a".into(), "/b".into()], active: 1, layout, theme: "Nord".into(), pinned };
        save(&path, &saved).unwrap();
        assert_eq!(load(&path), saved);

        std::fs::write(&path, "{not json").unwrap();
        assert_eq!(load(&path), Saved::default());
    }

    #[test]
    fn missing_fields_use_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        std::fs::write(&path, r#"{"tabs":["/x"]}"#).unwrap();
        let saved = load(&path);
        assert_eq!(saved.tabs, vec![PathBuf::from("/x")]);
        assert_eq!(saved.layout, Layout::default());
        assert_eq!(saved.theme, "Dark");
    }
}
