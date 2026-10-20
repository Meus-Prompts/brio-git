//! One AI session of a tab: its terminal, the name shown on its tab and
//! whether it is working right now.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use iced::widget::canvas::Cache;
use brio_session::{TermSnapshot, Terminal};

pub type SessionId = u64;

/// Output this recent means the session is working.
pub const WORKING_WINDOW: Duration = Duration::from_secs(3);
/// Output this soon after a keystroke is the echo of the typing, not work.
const ECHO_WINDOW: Duration = Duration::from_millis(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activity {
    /// Printed something in the last [`WORKING_WINDOW`].
    Working,
    /// Running but quiet: usually waiting for the user.
    Waiting,
    Exited,
}

pub struct Session {
    pub id: SessionId,
    pub name: String,
    pub tool: String,
    pub terminal: Option<Terminal>,
    pub snapshot: Option<TermSnapshot>,
    pub cache: Cache,
    pub running: bool,
    pub exit_code: Option<u32>,
    last_output: Option<Instant>,
    last_input: Option<Instant>,
}

impl Session {
    pub fn new(id: SessionId, name: String, tool: String, terminal: Option<Terminal>) -> Self {
        Self {
            id,
            name,
            tool,
            running: terminal.is_some(),
            terminal,
            snapshot: None,
            cache: Cache::new(),
            exit_code: None,
            last_output: None,
            last_input: None,
        }
    }

    /// Re-reads the terminal screen.
    pub fn refresh(&mut self) {
        if let Some(t) = &self.terminal {
            self.snapshot = Some(t.snapshot());
            self.cache.clear();
        }
    }

    /// New output arrived.
    pub fn mark_output(&mut self, now: Instant) {
        let echo = self.last_input.is_some_and(|i| now.saturating_duration_since(i) < ECHO_WINDOW);
        if !echo {
            self.last_output = Some(now);
        }
    }

    /// The user typed or pasted into the terminal.
    pub fn mark_input(&mut self, now: Instant) {
        self.last_input = Some(now);
    }

    /// The child exited; the terminal is kept so its output can still be
    /// scrolled, selected and copied.
    pub fn exited(&mut self, code: Option<u32>) {
        self.refresh();
        self.running = false;
        self.exit_code = code;
        self.last_output = None;
    }

    pub fn activity(&self, now: Instant) -> Activity {
        match self.last_output {
            _ if !self.running => Activity::Exited,
            Some(t) if now.saturating_duration_since(t) < WORKING_WINDOW => Activity::Working,
            _ => Activity::Waiting,
        }
    }
}

/// `tool`, or `tool 2`, `tool 3`… when that name is already taken.
pub fn default_name<'a>(tool: &str, taken: impl IntoIterator<Item = &'a str>) -> String {
    let taken: Vec<&str> = taken.into_iter().collect();
    (1..)
        .map(|n| if n == 1 { tool.to_owned() } else { format!("{tool} {n}") })
        .find(|name| !taken.contains(&name.as_str()))
        .unwrap_or_default()
}

/// Opacity of the "working" dot, breathing once every 1.2 s.
pub fn pulse() -> f32 {
    let ms = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() % 1200) as f32;
    let phase = (ms / 1200.0 * std::f32::consts::TAU).sin();
    0.6 + 0.4 * phase
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running() -> Session {
        let mut s = Session::new(0, "claude".into(), "claude".into(), None);
        s.running = true;
        s
    }

    #[test]
    fn names_are_numbered_when_taken() {
        assert_eq!(default_name("claude", []), "claude");
        assert_eq!(default_name("claude", ["claude"]), "claude 2");
        assert_eq!(default_name("claude", ["claude", "claude 2", "codex"]), "claude 3");
        assert_eq!(default_name("claude", ["claude 2"]), "claude");
    }

    #[test]
    fn recent_output_means_working() {
        let t0 = Instant::now();
        let mut s = running();
        assert_eq!(s.activity(t0), Activity::Waiting);
        s.mark_output(t0);
        assert_eq!(s.activity(t0 + Duration::from_secs(1)), Activity::Working);
        assert_eq!(s.activity(t0 + WORKING_WINDOW), Activity::Waiting);
    }

    #[test]
    fn echo_of_typing_is_not_work() {
        let t0 = Instant::now();
        let mut s = running();
        s.mark_input(t0);
        s.mark_output(t0 + Duration::from_millis(50));
        assert_eq!(s.activity(t0 + Duration::from_millis(100)), Activity::Waiting);
        s.mark_output(t0 + Duration::from_secs(1));
        assert_eq!(s.activity(t0 + Duration::from_secs(2)), Activity::Working);
    }

    #[test]
    fn exited_sessions_are_not_working() {
        let t0 = Instant::now();
        let mut s = running();
        s.mark_output(t0);
        s.exited(Some(1));
        assert_eq!(s.activity(t0), Activity::Exited);
        assert_eq!(s.exit_code, Some(1));
    }
}
