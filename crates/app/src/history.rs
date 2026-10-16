//! Undo/redo stacks of one tab: each entry is an operation with the refs
//! before and after it (see `brio_ops::undo`).

use brio_ops::undo::{Move, Snapshot};

#[derive(Debug, Clone)]
pub struct Entry {
    pub label: String,
    pub before: Snapshot,
    pub after: Snapshot,
    pub mode: Move,
    /// Message of an undone commit, put back in the commit box.
    pub message: Option<String>,
    /// Undoing a `reset --hard` brings the commits back, not the discarded edits.
    pub hard_reset: bool,
}

/// Oldest entries are dropped past this many.
const MAX: usize = 50;

#[derive(Debug, Default)]
pub struct History {
    undo: Vec<Entry>,
    redo: Vec<Entry>,
}

impl History {
    /// A new operation: it can be undone, and what was undone before can no longer be redone.
    pub fn record(&mut self, entry: Entry) {
        self.undo.push(entry);
        if self.undo.len() > MAX {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn next_undo(&self) -> Option<&Entry> {
        self.undo.last()
    }

    pub fn next_redo(&self) -> Option<&Entry> {
        self.redo.last()
    }

    /// The last entry was undone: it moves to the redo stack.
    pub fn undone(&mut self) {
        if let Some(e) = self.undo.pop() {
            self.redo.push(e);
        }
    }

    /// The last undone entry was redone.
    pub fn redone(&mut self) {
        if let Some(e) = self.redo.pop() {
            self.undo.push(e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use brio_ops::undo::HeadState;

    fn entry(label: &str) -> Entry {
        let snap = Snapshot { head: HeadState::Unborn, head_id: None, refs: Default::default() };
        Entry { label: label.into(), before: snap.clone(), after: snap, mode: Move::Keep, message: None, hard_reset: false }
    }

    #[test]
    fn undo_then_redo_then_new_op_clears_redo() {
        let mut h = History::default();
        h.record(entry("a"));
        h.record(entry("b"));
        assert_eq!(h.next_undo().unwrap().label, "b");
        h.undone();
        assert_eq!(h.next_undo().unwrap().label, "a");
        assert_eq!(h.next_redo().unwrap().label, "b");
        h.redone();
        assert_eq!(h.next_undo().unwrap().label, "b");
        assert!(h.next_redo().is_none());
        h.undone();
        h.record(entry("c"));
        assert!(h.next_redo().is_none(), "a new operation drops the redo stack");
    }

    #[test]
    fn keeps_the_latest_fifty() {
        let mut h = History::default();
        for i in 0..60 {
            h.record(entry(&i.to_string()));
        }
        let mut n = 0;
        while h.next_undo().is_some() {
            h.undone();
            n += 1;
        }
        assert_eq!(n, 50);
        assert_eq!(h.next_redo().unwrap().label, "10", "the oldest kept is the 11th");
    }
}
