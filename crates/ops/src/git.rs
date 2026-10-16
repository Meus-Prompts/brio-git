//! Thin wrapper around the `git` executable.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::io::Write;
use std::process::{Command, Output, Stdio};

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("não foi possível executar o git (ele está instalado?): {0}")]
    Spawn(#[from] std::io::Error),
    #[error("`git {args}` falhou: {stderr}")]
    Failed { args: String, stderr: String },
}

pub type Result<T> = std::result::Result<T, GitError>;

/// Returns stdout, or the command and its stderr when git failed.
fn checked<S: AsRef<OsStr>>(args: &[S], output: Output) -> Result<Vec<u8>> {
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(GitError::Failed {
            args: args.iter().map(|a| a.as_ref().to_string_lossy()).collect::<Vec<_>>().join(" "),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        })
    }
}

/// Runs `git` commands inside one work tree.
#[derive(Debug, Clone)]
pub struct Git {
    workdir: PathBuf,
}

impl Git {
    pub fn new(workdir: impl Into<PathBuf>) -> Self {
        Self { workdir: workdir.into() }
    }

    pub fn workdir(&self) -> &Path {
        &self.workdir
    }

    /// Runs `git <args>` and returns stdout.
    pub fn run<S: AsRef<OsStr>>(&self, args: &[S]) -> Result<String> {
        self.run_with_env(args, &[])
    }

    pub fn run_with_env<S: AsRef<OsStr>>(&self, args: &[S], env: &[(&str, &OsStr)]) -> Result<String> {
        let stdout = checked(args, self.output(args, env)?)?;
        Ok(String::from_utf8_lossy(&stdout).into_owned())
    }

    /// Like [`run`](Self::run) but returns raw stdout bytes (file contents).
    pub fn run_bytes<S: AsRef<OsStr>>(&self, args: &[S]) -> Result<Vec<u8>> {
        checked(args, self.output(args, &[])?)
    }

    /// Runs `git <args>` feeding `input` on stdin (commit messages, patches).
    pub fn run_with_input<S: AsRef<OsStr>>(&self, args: &[S], input: &[u8]) -> Result<String> {
        let mut child = self
            .command(args, &[])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(input)?;
        }
        let stdout = checked(args, child.wait_with_output()?)?;
        Ok(String::from_utf8_lossy(&stdout).into_owned())
    }

    /// Like [`run`](Self::run) but returns `None` instead of an error when git
    /// exits with a non-zero status (useful for `rev-parse --verify`).
    pub fn try_run<S: AsRef<OsStr>>(&self, args: &[S]) -> Result<Option<String>> {
        let output = self.output(args, &[])?;
        Ok(output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).into_owned()))
    }

    fn output<S: AsRef<OsStr>>(&self, args: &[S], env: &[(&str, &OsStr)]) -> Result<Output> {
        Ok(self.command(args, env).output()?)
    }

    fn command<S: AsRef<OsStr>>(&self, args: &[S], env: &[(&str, &OsStr)]) -> Command {
        let mut cmd = Command::new("git");
        cmd.args(args)
            .current_dir(&self.workdir)
            // Never block on a credential or editor prompt we cannot show.
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_EDITOR", "true");
        for (k, v) in env {
            cmd.env(k, v);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        cmd
    }

    /// Resolves a path git reports (relative to the work tree) to an absolute one.
    pub fn resolve(&self, path: &str) -> PathBuf {
        let p = Path::new(path.trim());
        if p.is_absolute() { p.to_path_buf() } else { self.workdir.join(p) }
    }
}
