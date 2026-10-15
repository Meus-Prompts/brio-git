//! Decides when an automatic checkpoint is due.
//!
//! A checkpoint is taken once the working tree changed and either the AI went
//! quiet for `quiet` (a natural pause, e.g. it finished a step) or `max_gap`
//! passed since the last checkpoint (long uninterrupted edits).

use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct CheckpointScheduler {
    quiet: Duration,
    max_gap: Duration,
    last_change: Option<Instant>,
    last_checkpoint: Instant,
}

impl CheckpointScheduler {
    pub fn new(now: Instant, quiet: Duration, max_gap: Duration) -> Self {
        Self { quiet, max_gap, last_change: None, last_checkpoint: now }
    }

    pub fn on_change(&mut self, now: Instant) {
        self.last_change = Some(now);
    }

    pub fn is_due(&self, now: Instant) -> bool {
        let Some(change) = self.last_change else { return false };
        now.duration_since(change) >= self.quiet || now.duration_since(self.last_checkpoint) >= self.max_gap
    }

    /// Call when a checkpoint attempt starts; changes after `now` count again.
    pub fn checkpointed(&mut self, now: Instant) {
        self.last_checkpoint = now;
        self.last_change = None;
    }
}

impl Default for CheckpointScheduler {
    fn default() -> Self {
        Self::new(Instant::now(), Duration::from_secs(4), Duration::from_secs(45))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sched(t0: Instant) -> CheckpointScheduler {
        CheckpointScheduler::new(t0, Duration::from_secs(4), Duration::from_secs(45))
    }

    #[test]
    fn nothing_due_without_changes() {
        let t0 = Instant::now();
        assert!(!sched(t0).is_due(t0 + Duration::from_secs(600)));
    }

    #[test]
    fn due_after_quiet_period() {
        let t0 = Instant::now();
        let mut s = sched(t0);
        s.on_change(t0 + Duration::from_secs(1));
        assert!(!s.is_due(t0 + Duration::from_secs(3)));
        assert!(s.is_due(t0 + Duration::from_secs(5)));
    }

    #[test]
    fn continuous_changes_still_checkpoint_after_max_gap() {
        let t0 = Instant::now();
        let mut s = sched(t0);
        let mut t = t0;
        for _ in 0..30 {
            t += Duration::from_secs(2);
            s.on_change(t);
            if s.is_due(t) {
                assert!(t.duration_since(t0) >= Duration::from_secs(45));
                return;
            }
        }
        panic!("max gap never triggered");
    }

    #[test]
    fn checkpoint_resets_state() {
        let t0 = Instant::now();
        let mut s = sched(t0);
        s.on_change(t0);
        s.checkpointed(t0 + Duration::from_secs(10));
        assert!(!s.is_due(t0 + Duration::from_secs(100)));
    }
}
