//! Author avatars: Gravatar pictures cached on disk, with a generated
//! fallback (initials on a color derived from the e-mail).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

use iced::Color;
use iced::widget::image;
use sha2::{Digest, Sha256};

use crate::style;

/// Retry e-mails without a Gravatar after this long.
const MISSING_TTL: Duration = Duration::from_secs(3 * 24 * 3600);

enum Avatar {
    Pending,
    Image(image::Handle),
    Missing,
}

#[derive(Default)]
pub struct Avatars {
    map: HashMap<String, Avatar>,
}

impl Avatars {
    pub fn get(&self, email: &str) -> Option<&image::Handle> {
        match self.map.get(&key(email)) {
            Some(Avatar::Image(h)) => Some(h),
            _ => None,
        }
    }

    /// Marks unseen e-mails as pending and returns them for fetching.
    pub fn request<'a>(&mut self, emails: impl IntoIterator<Item = &'a str>) -> Vec<String> {
        let mut new = Vec::new();
        for email in emails {
            let k = key(email);
            if k.is_empty() || self.map.contains_key(&k) {
                continue;
            }
            self.map.insert(k.clone(), Avatar::Pending);
            new.push(k);
        }
        new
    }

    pub fn loaded(&mut self, email: String, bytes: Option<Vec<u8>>) {
        let avatar = match bytes {
            Some(b) => Avatar::Image(image::Handle::from_bytes(b)),
            None => Avatar::Missing,
        };
        self.map.insert(email, avatar);
    }
}

pub fn key(email: &str) -> String {
    email.trim().to_lowercase()
}

/// Up to two initials: "Filipe Crespo" -> "FC".
pub fn initials(name: &str) -> String {
    let mut words = name.split_whitespace().filter_map(|w| w.chars().next());
    let first = words.next();
    let last = words.next_back();
    [first, last].into_iter().flatten().flat_map(char::to_uppercase).collect()
}

/// Stable color per person.
pub fn color(email: &str) -> Color {
    let hash = key(email).bytes().fold(5381u32, |h, b| h.wrapping_mul(33) ^ u32::from(b));
    style::p().lane(hash as usize)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn cache_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("lazy-git").join("avatars"))
}

fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(6)))
            .http_status_as_error(false)
            .build()
            .into()
    })
}

/// Picture for `email` (already normalized), from the disk cache or Gravatar.
/// Blocking; run it off the UI thread.
pub fn fetch(email: &str) -> Option<Vec<u8>> {
    let hash = hex(&Sha256::digest(email.as_bytes()));
    let dir = cache_dir();
    if let Some(dir) = &dir {
        if let Ok(bytes) = std::fs::read(dir.join(&hash)) {
            return Some(bytes);
        }
        let missing = dir.join(format!("{hash}.none"));
        let fresh = std::fs::metadata(&missing)
            .and_then(|m| m.modified())
            .is_ok_and(|t| t.elapsed().is_ok_and(|e| e < MISSING_TTL));
        if fresh {
            return None;
        }
    }

    let url = format!("https://gravatar.com/avatar/{hash}?s=64&d=404");
    let mut response = agent().get(&url).call().ok()?;
    let status = response.status().as_u16();
    let save = |name: &str, bytes: &[u8]| {
        if let Some(dir) = &dir {
            let _ = std::fs::create_dir_all(dir);
            let _ = std::fs::write(dir.join(name), bytes);
        }
    };
    match status {
        200 => {
            let bytes = response.body_mut().read_to_vec().ok()?;
            save(&hash, &bytes);
            Some(bytes)
        }
        404 => {
            save(&format!("{hash}.none"), b"");
            None
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_from_names() {
        assert_eq!(initials("Filipe Crespo"), "FC");
        assert_eq!(initials("ana"), "A");
        assert_eq!(initials("João da Silva"), "JS");
        assert_eq!(initials(""), "");
    }

    #[test]
    fn requests_each_email_once_case_insensitive() {
        let mut a = Avatars::default();
        assert_eq!(a.request(["A@x.dev", "a@x.dev ", "b@x.dev", ""]), vec!["a@x.dev", "b@x.dev"]);
        assert!(a.request(["a@x.dev"]).is_empty());
        a.loaded("a@x.dev".into(), None);
        assert!(a.get("A@X.dev").is_none());
    }

    #[test]
    fn gravatar_hash_is_sha256_hex() {
        let h = hex(&Sha256::digest(b"test@example.com"));
        assert_eq!(h, "973dfe463ec85785f5f95af5ba3906eedb2d931c24e69824a89ea65dba4e813b");
    }
}
