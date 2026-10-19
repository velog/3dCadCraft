//! Undo/Redo.
//!
//! **Current stage: snapshot history.** Before each user action the caller
//! records a copy of the document state with a label ("Create Sketch1",
//! "Drag point"). Undo swaps the current state with the last snapshot; Redo
//! swaps it back. Arena-based models make copies cheap and always correct —
//! there is no inverse operation that could be subtly wrong.
//!
//! Rapid repeats of the same action (dragging, spinning a value) are
//! *coalesced* into one step, so Undo reverts the whole gesture.
//!
//! **Next stage** (docs/FEATURES.md 0.4): a serialisable `Command` enum
//! executed through one entry point, shared by GUI, scripts and the MCP API,
//! with this history storing commands/deltas instead of full copies.

use std::time::{Duration, Instant};

/// Undo/redo stacks of labelled snapshots of `T`.
#[derive(Debug)]
pub struct History<T> {
    undo: Vec<(String, T)>,
    redo: Vec<(String, T)>,
    limit: usize,
    /// Label and time of the last record, for coalescing.
    last: Option<(String, Instant)>,
    coalesce_window: Duration,
}

impl<T> Default for History<T> {
    fn default() -> Self {
        Self::new(100)
    }
}

impl<T> History<T> {
    /// A history keeping at most `limit` undo steps.
    pub fn new(limit: usize) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            limit: limit.max(1),
            last: None,
            coalesce_window: Duration::from_millis(800),
        }
    }

    /// Records `before` — the state *before* an action named `label`.
    /// Clears the redo stack. If the same label was recorded within the
    /// coalescing window, nothing is pushed (the gesture is one step).
    pub fn record(&mut self, label: &str, before: T) {
        let now = Instant::now();
        let coalesce = self
            .last
            .as_ref()
            .is_some_and(|(l, t)| l == label && now.duration_since(*t) < self.coalesce_window);
        self.last = Some((label.to_owned(), now));
        self.redo.clear();
        if coalesce {
            return;
        }
        self.undo.push((label.to_owned(), before));
        if self.undo.len() > self.limit {
            self.undo.remove(0);
        }
    }

    /// Ends coalescing, so the next record starts a new step (call when a
    /// drag ends).
    pub fn break_coalescing(&mut self) {
        self.last = None;
    }

    /// Undoes the last step: returns the state to restore; `current` goes to
    /// the redo stack.
    pub fn undo(&mut self, current: T) -> Option<T> {
        let (label, state) = self.undo.pop()?;
        self.redo.push((label, current));
        self.last = None;
        Some(state)
    }

    /// Redoes the last undone step.
    pub fn redo(&mut self, current: T) -> Option<T> {
        let (label, state) = self.redo.pop()?;
        self.undo.push((label, current));
        self.last = None;
        Some(state)
    }

    /// Label of the step Undo would revert.
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|(l, _)| l.as_str())
    }

    /// Label of the step Redo would re-apply.
    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|(l, _)| l.as_str())
    }

    /// Drops the step just recorded, when the action it was recorded for
    /// failed and changed nothing (so Undo does not show an empty step).
    pub fn cancel(&mut self) {
        self.undo.pop();
        self.last = None;
    }

    /// Forgets everything (e.g. after opening another file).
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.last = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_redo_roundtrip() {
        let mut h = History::new(10);
        let mut state = 0;
        h.record("a", state);
        state = 1;
        h.break_coalescing();
        h.record("b", state);
        state = 2;

        state = h.undo(state).unwrap();
        assert_eq!(
            (state, h.undo_label(), h.redo_label()),
            (1, Some("a"), Some("b"))
        );
        state = h.undo(state).unwrap();
        assert_eq!(state, 0);
        assert!(h.undo(state).is_none());
        state = h.redo(state).unwrap();
        state = h.redo(state).unwrap();
        assert_eq!(state, 2);

        // A new action clears redo.
        state = h.undo(state).unwrap();
        h.record("c", state);
        assert!(h.redo_label().is_none());
    }

    #[test]
    fn repeated_action_is_one_step() {
        let mut h = History::new(10);
        for s in 0..5 {
            h.record("Drag point", s);
        }
        assert_eq!(h.undo(99), Some(0), "the whole drag undoes at once");
        assert!(h.undo(0).is_none());
    }

    #[test]
    fn limit_drops_oldest() {
        let mut h = History::new(2);
        for s in 0..3 {
            h.record(&format!("s{s}"), s);
        }
        assert_eq!(h.undo(9), Some(2));
        assert_eq!(h.undo(2), Some(1));
        assert!(h.undo(1).is_none());
    }
}
