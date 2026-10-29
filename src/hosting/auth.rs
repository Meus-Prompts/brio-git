//! Tokens de acesso por host. Ordem: cache em memória → keyring do sistema
//! (Secret Service) → variáveis de ambiente → CLIs `gh` / `glab` já logados.
//! Tokens nunca são gravados em arquivo de configuração nem em logs.

use std::collections::HashMap;
use std::process::Command;
use std::sync::Mutex;

use anyhow::{Result, anyhow};

use super::Kind;

const SERVICE: &str = "brio-git";

/// Segredo cujo `Debug` nunca mostra o valor (evita vazar tokens em logs/panics).
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Secret(pub String);

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_empty() { "Secret(<vazio>)" } else { "Secret(***)" })
    }
}

static CACHE: Mutex<Option<HashMap<String, (String, Source)>>> = Mutex::new(None);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Keyring,
    Env,
    Cli,
    /// Definido em memória (testes ou sessão atual).
    Session,
}


fn cached(host: &str) -> Option<(String, Source)> {
    CACHE.lock().ok()?.as_ref()?.get(host).cloned()
}

fn remember(host: &str, token: String, src: Source) {
    if let Ok(mut c) = CACHE.lock() {
        c.get_or_insert_with(HashMap::new).insert(host.to_string(), (token, src));
    }
}

pub fn forget(host: &str) {
    if let Ok(mut c) = CACHE.lock()
        && let Some(m) = c.as_mut()
    {
        m.remove(host);
    }
}

/// Define o token só em memória (usado pelos testes).
#[cfg(test)]
pub fn set_session(host: &str, token: &str) {
    remember(host, token.to_string(), Source::Session);
}

fn from_keyring(host: &str) -> Option<String> {
    keyring::Entry::new(SERVICE, host).ok()?.get_password().ok().filter(|t| !t.trim().is_empty())
}

fn from_env(kind: Kind, host: &str) -> Option<String> {
    let vars: &[&str] = match kind {
        Kind::GitHub if host == "github.com" => &["GH_TOKEN", "GITHUB_TOKEN"],
        Kind::GitHub => &["GH_ENTERPRISE_TOKEN", "GITHUB_ENTERPRISE_TOKEN"],
        Kind::GitLab => &["GITLAB_TOKEN"],
    };
    vars.iter().find_map(|v| std::env::var(v).ok()).filter(|t| !t.trim().is_empty())
}

fn from_cli(kind: Kind, host: &str) -> Option<String> {
    let out = match kind {
        Kind::GitHub => Command::new("gh").args(["auth", "token", "--hostname", host]).output(),
        Kind::GitLab => Command::new("glab").args(["config", "get", "token", "--host", host]).output(),
    }
    .ok()?;
    let token = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !token.is_empty()).then_some(token)
}

/// Token para o host, se houver algum disponível.
pub fn token(kind: Kind, host: &str) -> Option<(String, Source)> {
    if let Some(t) = cached(host) {
        return Some(t);
    }
    let found = from_keyring(host)
        .map(|t| (t, Source::Keyring))
        .or_else(|| from_env(kind, host).map(|t| (t, Source::Env)))
        .or_else(|| from_cli(kind, host).map(|t| (t, Source::Cli)));
    if let Some((t, s)) = &found {
        remember(host, t.clone(), *s);
    }
    found
}

/// Salva o token no keyring do sistema (e no cache da sessão).
pub fn save(host: &str, token: &str) -> Result<()> {
    let token = token.trim();
    if token.is_empty() {
        return Err(anyhow!("token vazio"));
    }
    // Testes nunca tocam o keyring real do usuário.
    if cfg!(test) {
        remember(host, token.to_string(), Source::Session);
        return Ok(());
    }
    let stored = keyring::Entry::new(SERVICE, host).and_then(|e| e.set_password(token));
    remember(host, token.to_string(), if stored.is_ok() { Source::Keyring } else { Source::Session });
    stored.map_err(|e| anyhow!("keyring indisponível ({e}); o token vale só nesta sessão"))
}

/// Remove do keyring e do cache.
pub fn remove(host: &str) {
    forget(host);
    if let Ok(e) = keyring::Entry::new(SERVICE, host) {
        let _ = e.delete_credential();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn secret_debug_is_redacted() {
        let s = super::Secret("glpat-super-secreto".into());
        assert_eq!(format!("{s:?}"), "Secret(***)");
        assert!(!format!("{:?}", crate::repo_hosting::HostingAction::Connect(s)).contains("super"));
    }
}
