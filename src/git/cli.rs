//! Execução do executável `git` para mutações e operações de rede.
//!
//! Usar o CLI (e não libgit2) para escrever garante hooks, assinatura GPG/SSH,
//! credential helpers e estados de merge/rebase idênticos ao terminal.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Result, anyhow};

const GIT_ENV: [(&str, &str); 4] = [("GIT_TERMINAL_PROMPT", "0"), ("GIT_EDITOR", "true"), ("GIT_MERGE_AUTOEDIT", "no"), ("LC_ALL", "C")];

fn base(repo: &Path, extra_env: &[(&str, &str)]) -> Command {
    // Windows abrindo repositório do WSL: o git roda dentro da distro.
    if cfg!(windows)
        && let Some(w) = crate::wsl::parse(repo)
    {
        let mut cmd = Command::new("wsl.exe");
        cmd.args(["-d", &w.distro, "--cd", &w.linux, "--exec", "env"]);
        for (k, v) in GIT_ENV.iter().chain(extra_env) {
            cmd.arg(format!("{k}={v}"));
        }
        cmd.arg("git").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        no_window(&mut cmd);
        return cmd;
    }
    let mut cmd = Command::new("git");
    cmd.current_dir(repo).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    for (k, v) in GIT_ENV.iter().chain(extra_env) {
        cmd.env(k, v);
    }
    for (k, v) in crate::askpass::env() {
        cmd.env(k, v);
    }
    no_window(&mut cmd);
    cmd
}

/// No Windows, processos de console não abrem uma janela preta por comando.
fn no_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let _ = cmd;
}

pub fn git(repo: &Path, args: &[&str]) -> Result<String> {
    git_with(repo, args, None, &[])
}

pub fn git_stdin(repo: &Path, args: &[&str], stdin: &str) -> Result<String> {
    git_with(repo, args, Some(stdin), &[])
}

pub fn git_with(repo: &Path, args: &[&str], stdin: Option<&str>, env: &[(&str, &str)]) -> Result<String> {
    let mut cmd = base(repo, env);
    cmd.args(args);
    if stdin.is_some() {
        cmd.stdin(Stdio::piped());
    }
    let mut child = cmd.spawn().map_err(|e| anyhow!("não foi possível executar git: {e}"))?;
    if let Some(input) = stdin {
        child.stdin.take().unwrap().write_all(input.as_bytes())?;
    }
    let out = child.wait_with_output()?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    if out.status.success() {
        Ok(if stdout.trim().is_empty() { stderr } else { stdout })
    } else {
        let msg = [stderr.trim(), stdout.trim()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n");
        Err(anyhow!("git {}: {}", args.first().copied().unwrap_or(""), if msg.is_empty() { "falhou".into() } else { msg }))
    }
}

/// Clona fora de um repositório existente.
pub fn clone(url: &str, dest: &Path, depth: Option<u32>) -> Result<String> {
    let parent = dest.parent().ok_or_else(|| anyhow!("destino inválido"))?;
    std::fs::create_dir_all(parent)?;
    // Dentro do WSL o git enxerga o destino como caminho Linux.
    let dest_s = if crate::wsl::parse(parent).is_some() { crate::wsl::to_linux(dest) } else { dest.display().to_string() };
    let depth_s = depth.map(|d| d.to_string());
    let mut args = vec!["clone", "--progress"];
    if let Some(d) = &depth_s {
        args.extend(["--depth", d.as_str()]);
    }
    args.extend(["--", url, dest_s.as_str()]);
    git(parent, &args)
}

pub fn init(dir: &Path, default_branch: &str) -> Result<String> {
    std::fs::create_dir_all(dir)?;
    git(dir, &["init", "-b", default_branch])
}
