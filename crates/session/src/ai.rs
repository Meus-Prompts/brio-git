//! Asks an installed AI CLI for a commit message, non-interactively.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::tools::Tool;

/// Diffs larger than this are cut before being sent.
const MAX_DIFF: usize = 60_000;
const TIMEOUT: Duration = Duration::from_secs(180);

pub const PROMPT: &str = "Escreva uma mensagem de commit para o diff abaixo, no padrão Conventional Commits \
(feat, fix, refactor, docs, chore...), em português do Brasil. Primeira linha com no máximo 72 caracteres; \
se precisar, uma linha em branco e um corpo curto explicando o porquê. \
Responda somente com a mensagem, sem comentários e sem blocos de código.";

/// How to run each known tool without its interactive UI: extra arguments,
/// and whether the prompt goes in an argument (diff on stdin) or on stdin.
fn headless_args(name: &str) -> Option<(Vec<&'static str>, bool)> {
    match name {
        "claude" | "gemini" | "qwen" => Some((vec!["-p"], true)),
        "codex" => Some((vec!["exec", "-"], false)),
        "opencode" => Some((vec!["run"], true)),
        "cursor-agent" => Some((vec!["-p", "--output-format", "text"], true)),
        _ => None,
    }
}

pub fn supports(tool: &Tool) -> bool {
    headless_args(&tool.name).is_some()
}

/// Runs `tool` in `cwd` with the staged `diff` and returns the cleaned message.
pub fn commit_message(tool: &Tool, cwd: &Path, diff: &str) -> Result<String, String> {
    let (extra, prompt_as_arg) = headless_args(&tool.name)
        .ok_or_else(|| format!("{} não tem modo não interativo conhecido; escolha claude, codex ou gemini.", tool.name))?;
    let diff = truncate(diff, MAX_DIFF);

    let mut cmd = Command::new(&tool.program);
    cmd.args(&tool.args).args(&extra).current_dir(cwd);
    let stdin = if prompt_as_arg {
        cmd.arg(PROMPT);
        diff.to_owned()
    } else {
        format!("{PROMPT}\n\n{diff}")
    };
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("não foi possível executar {}: {e}", tool.name))?;
    if let Some(mut input) = child.stdin.take() {
        input.write_all(stdin.as_bytes()).map_err(|e| e.to_string())?;
    }

    let started = Instant::now();
    loop {
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            break;
        }
        if started.elapsed() > TIMEOUT {
            let _ = child.kill();
            return Err(format!("{} demorou demais para responder.", tool.name));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("{} falhou: {}", tool.name, err.trim()));
    }
    let message = clean(&String::from_utf8_lossy(&output.stdout));
    if message.is_empty() {
        Err(format!("{} não devolveu nenhuma mensagem.", tool.name))
    } else {
        Ok(message)
    }
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Strips code fences and surrounding blank lines models like to add.
pub fn clean(raw: &str) -> String {
    let lines: Vec<&str> = raw.trim().lines().filter(|l| !l.trim_start().starts_with("```")).collect();
    lines.join("\n").trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_fences_and_whitespace() {
        assert_eq!(clean("\n```\nfeat: x\n\ncorpo\n```\n"), "feat: x\n\ncorpo");
        assert_eq!(clean("fix: y"), "fix: y");
    }

    #[test]
    fn truncates_on_char_boundary() {
        assert_eq!(truncate("ááá", 3), "á");
        assert_eq!(truncate("abc", 10), "abc");
    }

    #[test]
    fn unknown_tools_are_rejected() {
        let tool = Tool { name: "aider".into(), program: "aider".into(), args: vec![] };
        assert!(!supports(&tool));
        assert!(commit_message(&tool, Path::new("."), "diff").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn runs_a_fake_claude_with_diff_on_stdin() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("claude");
        // Echo a fenced message that mentions the first diff line it received.
        std::fs::write(&script, "#!/bin/sh\nread first\necho '```'\necho \"feat: $first\"\necho '```'\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let tool = Tool { name: "claude".into(), program: script, args: vec![] };
        let msg = commit_message(&tool, dir.path(), "adiciona login\n+x").unwrap();
        assert_eq!(msg, "feat: adiciona login");
    }
}
