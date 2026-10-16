//! Short-lived messages stacked in the bottom-right corner.
//!
//! Tabs keep reporting through their `error` / `notice` fields; the app moves
//! whatever they left there into this queue after each update.

use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Info,
    Error,
}

impl Kind {
    fn lifetime(self) -> Duration {
        match self {
            Kind::Info => Duration::from_secs(4),
            Kind::Error => Duration::from_secs(10),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub id: u64,
    pub kind: Kind,
    pub text: String,
    /// Repository the message is about, when it is not the active tab.
    pub source: Option<String>,
    shown: Instant,
}

/// At most this many are kept; older ones make room for new ones.
const MAX: usize = 4;

#[derive(Debug, Default)]
pub struct Toasts {
    items: Vec<Toast>,
    next_id: u64,
}

impl Toasts {
    pub fn push(&mut self, kind: Kind, text: String, source: Option<String>, now: Instant) {
        // The same message again only restarts its timer.
        if let Some(last) = self.items.last_mut()
            && last.kind == kind
            && last.text == text
            && last.source == source
        {
            last.shown = now;
            return;
        }
        self.next_id += 1;
        self.items.push(Toast { id: self.next_id, kind, text, source, shown: now });
        if self.items.len() > MAX {
            self.items.remove(0);
        }
    }

    pub fn dismiss(&mut self, id: u64) {
        self.items.retain(|t| t.id != id);
    }

    /// Drops the toasts whose time is up; returns whether any was dropped.
    pub fn expire(&mut self, now: Instant) -> bool {
        let before = self.items.len();
        self.items.retain(|t| now.duration_since(t.shown) < t.kind.lifetime());
        self.items.len() != before
    }

    pub fn items(&self) -> &[Toast] {
        &self.items
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_newest_four() {
        let mut t = Toasts::default();
        let now = Instant::now();
        for i in 0..6 {
            t.push(Kind::Info, format!("m{i}"), None, now);
        }
        let texts: Vec<_> = t.items().iter().map(|t| t.text.as_str()).collect();
        assert_eq!(texts, ["m2", "m3", "m4", "m5"]);
    }

    #[test]
    fn errors_stay_longer() {
        let mut t = Toasts::default();
        let now = Instant::now();
        t.push(Kind::Info, "ok".into(), None, now);
        t.push(Kind::Error, "falhou".into(), None, now);
        assert!(t.expire(now + Duration::from_secs(5)));
        assert_eq!(t.items().len(), 1);
        assert_eq!(t.items()[0].kind, Kind::Error);
        assert!(!t.expire(now + Duration::from_secs(9)));
        assert!(t.expire(now + Duration::from_secs(11)));
        assert!(t.is_empty());
    }

    #[test]
    fn repeated_message_restarts_its_timer() {
        let mut t = Toasts::default();
        let now = Instant::now();
        t.push(Kind::Info, "Hash copiado.".into(), None, now);
        t.push(Kind::Info, "Hash copiado.".into(), None, now + Duration::from_secs(3));
        assert_eq!(t.items().len(), 1);
        assert!(!t.expire(now + Duration::from_secs(5)), "timer restarted by the repeat");
        t.push(Kind::Info, "Hash copiado.".into(), Some("outro".into()), now);
        assert_eq!(t.items().len(), 2, "same text from another repository is a new toast");
    }

    #[test]
    fn dismiss_by_id() {
        let mut t = Toasts::default();
        let now = Instant::now();
        t.push(Kind::Info, "a".into(), None, now);
        t.push(Kind::Error, "b".into(), None, now);
        let id = t.items()[0].id;
        t.dismiss(id);
        assert_eq!(t.items().len(), 1);
        assert_eq!(t.items()[0].text, "b");
    }
}
