//! Geração da mensagem de commit pelos CLIs de IA instalados na máquina
//! (Claude Code, Codex, Gemini). O diff vai pelo stdin; nenhuma chave é
//! guardada pelo brio-git: cada CLI usa o próprio login.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use anyhow::{Result, anyhow};

/// Geração em andamento: a ferramenta e onde chega `(summary, description)`.
pub type Pending = (Tool, std::sync::mpsc::Receiver<Result<(String, String)>>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Claude,
    Codex,
    Gemini,
}

impl Tool {
    const ALL: [Tool; 3] = [Tool::Claude, Tool::Codex, Tool::Gemini];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Claude => "Claude",
            Tool::Codex => "Codex",
            Tool::Gemini => "Gemini",
        }
    }

    fn bin(self) -> &'static str {
        match self {
            Tool::Claude => "claude",
            Tool::Codex => "codex",
            Tool::Gemini => "gemini",
        }
    }
}

/// Diff maior que isso é cortado: os CLIs têm limite de contexto e o resumo
/// não melhora com arquivos gerados inteiros.
const MAX_DIFF: usize = 60_000;

const PROMPT: &str = "Write a git commit message for the diff below.\n\
Follow the style of the recent commits listed (language, Conventional Commits prefix if they use one).\n\
Output ONLY the message, no code fences, no commentary:\n\
- first line: summary, imperative, at most 72 characters;\n\
- then a blank line and a short description (bullet points ok) explaining what and why. Omit the description for trivial changes.";

/// CLIs encontrados (procurados uma vez por execução do app).
pub fn available() -> &'static [(Tool, PathBuf)] {
    static FOUND: OnceLock<Vec<(Tool, PathBuf)>> = OnceLock::new();
    FOUND.get_or_init(|| Tool::ALL.into_iter().filter_map(|t| find(t.bin()).map(|p| (t, p))).collect())
}

/// Procura no PATH e em diretórios de instalação comuns que um app aberto
/// pelo menu do desktop não herda (npm global, bun, `~/.local/bin`...).
fn find(bin: &str) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    if let Some(home) = dirs::home_dir() {
        for d in [".local/bin", ".claude/local", ".npm-global/bin", ".bun/bin", ".volta/bin", ".cargo/bin", "AppData/Roaming/npm"] {
            dirs.push(home.join(d));
        }
    }
    dirs.extend(["/usr/local/bin", "/opt/homebrew/bin"].map(PathBuf::from));
    let exts: &[&str] = if cfg!(windows) { &[".exe", ".cmd", ".bat"] } else { &[""] };
    dirs.iter().flat_map(|d| exts.iter().map(move |e| d.join(format!("{bin}{e}")))).find(|p| p.is_file())
}

/// Monta o contexto (diff staged, ou da working tree se nada estiver staged)
/// e pede a mensagem ao CLI. Retorna `(summary, description)`.
pub fn generate(repo: &Path, tool: Tool) -> Result<(String, String)> {
    let exe = available().iter().find(|(t, _)| *t == tool).map(|(_, p)| p.clone()).ok_or_else(|| anyhow!("{} não encontrado", tool.label()))?;
    let input = format!("{PROMPT}\n\n{}", context(repo)?);

    // `.cmd`/`.bat` (instalação via npm no Windows) só rodam pelo cmd.
    let mut cmd = if exe.extension().is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat")) {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(&exe);
        c
    } else {
        Command::new(&exe)
    };
    let last_msg = std::env::temp_dir().join(format!("brio-git-codex-{}.txt", std::process::id()));
    match tool {
        // Só a configuração do usuário e nenhuma ferramenta: a resposta é texto puro.
        Tool::Claude => cmd.args(["-p", "--setting-sources", "user", "--tools", ""]),
        Tool::Gemini => cmd.args(["-p", ""]),
        // A saída do `codex exec` mistura progresso; a resposta final vai para o arquivo.
        Tool::Codex => cmd.args(["exec", "--sandbox", "read-only", "--skip-git-repo-check", "-o"]).arg(&last_msg).arg("-"),
    };
    // Fora do repo: `.claude/`, `.gemini/` e afins de um repositório de terceiros
    // não são carregados (hooks e permissões rodariam comandos arbitrários).
    let workdir = std::env::temp_dir().join(format!("brio-git-ai-{}", std::process::id()));
    std::fs::create_dir_all(&workdir)?;
    cmd.current_dir(&workdir);
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }

    let mut child = cmd.spawn().map_err(|e| anyhow!("não foi possível executar {}: {e}", tool.label()))?;
    child.stdin.take().unwrap().write_all(input.as_bytes())?;
    let out = child.wait_with_output()?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let err = err.trim();
        return Err(anyhow!("{} falhou: {}", tool.label(), if err.is_empty() { String::from_utf8_lossy(&out.stdout).trim().to_string() } else { err.to_string() }));
    }
    let text = match tool {
        Tool::Codex => {
            let t = std::fs::read_to_string(&last_msg).unwrap_or_else(|_| String::from_utf8_lossy(&out.stdout).to_string());
            let _ = std::fs::remove_file(&last_msg);
            t
        }
        _ => String::from_utf8_lossy(&out.stdout).to_string(),
    };
    parse(&text).ok_or_else(|| anyhow!("{} não retornou uma mensagem", tool.label()))
}

fn context(repo: &Path) -> Result<String> {
    use crate::git::cli::git;
    let log = git(repo, &["log", "-n", "10", "--format=%s"]).unwrap_or_default();
    let mut diff = git(repo, &["diff", "--cached", "--no-color", "--stat", "--patch"])?;
    if diff.trim().is_empty() {
        diff = git(repo, &["diff", "--no-color", "--stat", "--patch"])?;
        let untracked = git(repo, &["ls-files", "--others", "--exclude-standard"])?;
        if !untracked.trim().is_empty() {
            diff.push_str(&format!("\nNew untracked files:\n{untracked}"));
        }
    }
    if diff.trim().is_empty() {
        return Err(anyhow!("não há alterações para descrever"));
    }
    if diff.len() > MAX_DIFF {
        let cut = (0..=MAX_DIFF).rev().find(|&i| diff.is_char_boundary(i)).unwrap_or(0);
        diff.truncate(cut);
        diff.push_str("\n[diff truncated]");
    }
    Ok(format!("Recent commits:\n{log}\nDiff:\n{diff}"))
}

/// Separa summary e descrição, tolerando cercas de código em volta.
fn parse(text: &str) -> Option<(String, String)> {
    let text = text.trim();
    let text = text.strip_prefix("```").map(|t| t.split_once('\n').map_or("", |(_, r)| r)).unwrap_or(text);
    let text = text.strip_suffix("```").unwrap_or(text).trim();
    let (summary, body) = text.split_once('\n').unwrap_or((text, ""));
    let summary = summary.trim().to_string();
    (!summary.is_empty()).then(|| (summary, body.trim().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_message() {
        assert_eq!(parse("feat: x\n\n- a\n- b\n"), Some(("feat: x".into(), "- a\n- b".into())));
        assert_eq!(parse("```\nfix: y\n```"), Some(("fix: y".into(), String::new())));
        assert_eq!(parse("```text\nfix: y\n\ncorpo\n```"), Some(("fix: y".into(), "corpo".into())));
        assert_eq!(parse("  \n"), None);
    }
}
