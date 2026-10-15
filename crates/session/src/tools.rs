//! Discovery of AI command line tools installed on this machine.

use std::path::{Path, PathBuf};

/// A program that can run inside the embedded terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tool {
    /// Display name, e.g. `claude`.
    pub name: String,
    pub program: PathBuf,
    pub args: Vec<String>,
}

impl std::fmt::Display for Tool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

/// Known AI CLIs, probed in this order.
pub const KNOWN_AI_TOOLS: &[&str] = &[
    "claude",
    "codex",
    "gemini",
    "aider",
    "opencode",
    "cursor-agent",
    "copilot",
    "qwen",
    "goose",
    "amp",
];

/// AI tools found on `PATH`, followed by the user's shell.
pub fn detect() -> Vec<Tool> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut tools: Vec<Tool> = KNOWN_AI_TOOLS
        .iter()
        .filter_map(|name| find_in(&path, name).map(|p| launcher(name, p)))
        .collect();
    tools.push(shell(&path));
    tools
}

/// Searches `name` in a `PATH`-style list of directories.
pub fn find_in(path: &std::ffi::OsStr, name: &str) -> Option<PathBuf> {
    std::env::split_paths(path).find_map(|dir| {
        candidates(name).into_iter().map(|n| dir.join(n)).find(|p| is_executable(p))
    })
}

#[cfg(windows)]
fn candidates(name: &str) -> Vec<String> {
    let exts = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
    exts.split(';').filter(|e| !e.is_empty()).map(|e| format!("{name}{}", e.to_lowercase())).collect()
}

#[cfg(not(windows))]
fn candidates(name: &str) -> Vec<String> {
    vec![name.to_owned()]
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(p: &Path) -> bool {
    p.is_file()
}

/// Batch scripts (npm installs `claude.cmd` on Windows) must go through `cmd`.
fn launcher(name: &str, program: PathBuf) -> Tool {
    let is_script = program
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"));
    if is_script {
        Tool {
            name: name.to_owned(),
            program: PathBuf::from("cmd.exe"),
            args: vec!["/c".into(), program.to_string_lossy().into_owned()],
        }
    } else {
        Tool { name: name.to_owned(), program, args: Vec::new() }
    }
}

fn shell(path: &std::ffi::OsStr) -> Tool {
    #[cfg(windows)]
    let program = find_in(path, "pwsh")
        .or_else(|| find_in(path, "powershell"))
        .unwrap_or_else(|| PathBuf::from("cmd.exe"));
    #[cfg(not(windows))]
    let program = {
        let _ = path;
        std::env::var_os("SHELL").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/bin/sh"))
    };
    Tool { name: "terminal".into(), program, args: Vec::new() }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn finds_only_executables_on_path() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        std::fs::write(a.path().join("claude"), "").unwrap(); // not executable
        let exe = b.path().join("claude");
        std::fs::write(&exe, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();

        let path = std::env::join_paths([a.path(), b.path()]).unwrap();
        assert_eq!(find_in(&path, "claude"), Some(exe));
        assert_eq!(find_in(&path, "codex"), None);
    }

    #[test]
    fn detect_always_offers_a_shell() {
        assert_eq!(detect().last().map(|t| t.name.as_str()), Some("terminal"));
    }
}
