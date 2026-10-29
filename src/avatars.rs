//! Fotos dos autores por e-mail: noreply do GitHub → GitLab conectado →
//! Gravatar. Download em background, cache em disco (inclusive "sem foto")
//! e texturas em memória. Sem foto, a UI continua mostrando as iniciais.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, SystemTime};

use md5::{Digest, Md5};

const SIZE: u32 = 64;
const WORKERS: usize = 4;
/// "Sem foto" é revalidado depois deste prazo.
const NEGATIVE_TTL: Duration = Duration::from_secs(7 * 24 * 3600);

#[derive(Clone)]
enum State {
    Queued,
    Ready(egui::TextureHandle),
    Missing,
}

/// GitLab conectado capaz de resolver avatar por e-mail (`/avatar?email=`).
#[derive(Clone, PartialEq)]
pub struct GitLabSource {
    pub api: String,
    pub token: String,
}

struct Shared {
    states: Mutex<HashMap<String, State>>,
    queue: Mutex<VecDeque<String>>,
    wake: Condvar,
    gitlab: Mutex<Vec<GitLabSource>>,
    enabled: std::sync::atomic::AtomicBool,
    ctx: egui::Context,
}

static AVATARS: OnceLock<Arc<Shared>> = OnceLock::new();

/// Inicia os workers (uma vez por processo).
pub fn init(ctx: &egui::Context, enabled: bool) {
    let shared = AVATARS.get_or_init(|| {
        let s = Arc::new(Shared {
            states: Mutex::new(HashMap::new()),
            queue: Mutex::new(VecDeque::new()),
            wake: Condvar::new(),
            gitlab: Mutex::new(Vec::new()),
            enabled: std::sync::atomic::AtomicBool::new(enabled),
            ctx: ctx.clone(),
        });
        for _ in 0..WORKERS {
            let s = s.clone();
            std::thread::spawn(move || worker(&s));
        }
        s
    });
    shared.enabled.store(enabled, std::sync::atomic::Ordering::Relaxed);
}

pub fn set_enabled(enabled: bool) {
    if let Some(s) = AVATARS.get() {
        s.enabled.store(enabled, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Registra um GitLab conectado como fonte de avatares (repetido = ignorado).
pub fn add_gitlab(source: GitLabSource) {
    if let Some(s) = AVATARS.get() {
        let mut g = s.gitlab.lock().unwrap();
        if !g.contains(&source) {
            g.push(source);
            // E-mails sem foto podem ter avatar neste GitLab: tenta de novo.
            let mut states = s.states.lock().unwrap();
            states.retain(|_, st| !matches!(st, State::Missing));
        }
    }
}

/// Textura do avatar, se já disponível; agenda o download na primeira chamada.
pub fn get(email: &str) -> Option<egui::TextureHandle> {
    let s = AVATARS.get()?;
    if !s.enabled.load(std::sync::atomic::Ordering::Relaxed) {
        return None;
    }
    let key = email.trim().to_lowercase();
    if key.is_empty() || !key.contains('@') {
        return None;
    }
    let mut states = s.states.lock().unwrap();
    match states.get(&key) {
        Some(State::Ready(t)) => Some(t.clone()),
        Some(_) => None,
        None => {
            states.insert(key.clone(), State::Queued);
            drop(states);
            s.queue.lock().unwrap().push_back(key);
            s.wake.notify_one();
            None
        }
    }
}

fn worker(s: &Shared) {
    loop {
        let email = {
            let mut q = s.queue.lock().unwrap();
            loop {
                if let Some(e) = q.pop_front() {
                    break e;
                }
                q = s.wake.wait(q).unwrap();
            }
        };
        let state = match resolve(s, &email) {
            Some(img) => State::Ready(s.ctx.load_texture(format!("avatar:{email}"), img, egui::TextureOptions::LINEAR)),
            None => State::Missing,
        };
        s.states.lock().unwrap().insert(email, state);
        s.ctx.request_repaint();
    }
}

pub fn md5_hex(email: &str) -> String {
    let digest = Md5::digest(email.trim().to_lowercase().as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn cache_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("brio-git").join("avatars"))
}

/// URLs candidatas em ordem de preferência (sem rede, exceto o GitLab).
pub fn candidate_urls(email: &str, gitlab: &[GitLabSource], agent: Option<&ureq::Agent>) -> Vec<String> {
    let email = email.trim().to_lowercase();
    let mut urls = Vec::new();
    if let Some(local) = email.strip_suffix("@users.noreply.github.com") {
        // `12345+login@...` (formato novo) ou `login@...` (antigo).
        match local.split_once('+') {
            Some((id, _)) if id.chars().all(|c| c.is_ascii_digit()) => urls.push(format!("https://avatars.githubusercontent.com/u/{id}?s={SIZE}")),
            _ => urls.push(format!("https://github.com/{local}.png?size={SIZE}")),
        }
    }
    if let Some(agent) = agent {
        for g in gitlab {
            let url = format!("{}/avatar?email={}&size={SIZE}", g.api, encode(&email));
            let found = agent
                .get(&url)
                .header("PRIVATE-TOKEN", &g.token)
                .call()
                .ok()
                .filter(|r| r.status().is_success())
                .and_then(|mut r| r.body_mut().read_json::<serde_json::Value>().ok())
                .and_then(|v| v["avatar_url"].as_str().map(str::to_string))
                // O GitLab devolve o Gravatar quando o usuário não tem foto própria;
                // nesse caso o passo do Gravatar já cobre.
                .filter(|u| !u.contains("gravatar.com"));
            if let Some(u) = found {
                urls.push(u);
            }
        }
    }
    urls.push(format!("https://www.gravatar.com/avatar/{}?s={SIZE}&d=404", md5_hex(&email)));
    urls
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn decode(bytes: &[u8]) -> Option<egui::ColorImage> {
    let img = image::load_from_memory(bytes).ok()?.resize_to_fill(SIZE, SIZE, image::imageops::FilterType::Triangle).to_rgba8();
    Some(egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw()))
}

fn resolve(s: &Shared, email: &str) -> Option<egui::ColorImage> {
    let hash = md5_hex(email);
    let dir = cache_dir();
    if let Some(dir) = &dir {
        if let Ok(bytes) = std::fs::read(dir.join(format!("{hash}.img"))) {
            return decode(&bytes);
        }
        let negative = dir.join(format!("{hash}.none"));
        let fresh = std::fs::metadata(&negative).and_then(|m| m.modified()).ok().and_then(|t| SystemTime::now().duration_since(t).ok()).is_some_and(|age| age < NEGATIVE_TTL);
        if fresh && s.gitlab.lock().unwrap().is_empty() {
            return None;
        }
    }
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(8))).user_agent("brio-git").build().into();
    let gitlab = s.gitlab.lock().unwrap().clone();
    for url in candidate_urls(email, &gitlab, Some(&agent)) {
        let Ok(mut resp) = agent.get(&url).call() else { continue };
        if !resp.status().is_success() {
            continue;
        }
        let Ok(bytes) = resp.body_mut().with_config().limit(2 * 1024 * 1024).read_to_vec() else { continue };
        if let Some(img) = decode(&bytes) {
            if let Some(dir) = &dir {
                let _ = std::fs::create_dir_all(dir);
                let _ = std::fs::write(dir.join(format!("{hash}.img")), &bytes);
                let _ = std::fs::remove_file(dir.join(format!("{hash}.none")));
            }
            return Some(img);
        }
    }
    if let Some(dir) = &dir {
        let _ = std::fs::create_dir_all(dir);
        let _ = std::fs::write(dir.join(format!("{hash}.none")), b"");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gravatar_hash_matches_spec() {
        // Exemplo da documentação do Gravatar (e-mail normalizado em minúsculas).
        assert_eq!(md5_hex(" MyEmailAddress@example.com "), "0bc83cb571cd1c50ba6f3e8a78ef1346");
    }

    #[test]
    fn github_noreply_emails_map_to_github_avatars() {
        let urls = candidate_urls("1234567+ana@users.noreply.github.com", &[], None);
        assert_eq!(urls[0], "https://avatars.githubusercontent.com/u/1234567?s=64");
        let old = candidate_urls("ana@users.noreply.github.com", &[], None);
        assert_eq!(old[0], "https://github.com/ana.png?size=64");
        assert!(urls.last().unwrap().starts_with("https://www.gravatar.com/avatar/"), "Gravatar sempre por último");
    }
}
