//! Where a slow frame went: named spans of the frame's work, summed per name and written into
//! the session log next to "slow frame" (rule 20: a frame the user feels must say what it did).
//! Spans may nest; each name counts its own time, so a parent includes its children.

use std::cell::RefCell;
use std::time::{Duration, Instant};

thread_local! {
    static SPANS: RefCell<Vec<(&'static str, Duration, u32)>> = const { RefCell::new(Vec::new()) };
}

/// Times its scope under `name` until dropped.
pub(super) struct Span(&'static str, Instant);

/// Starts a span named `name`.
pub(super) fn span(name: &'static str) -> Span {
    Span(name, Instant::now())
}

impl Drop for Span {
    fn drop(&mut self) {
        let took = self.1.elapsed();
        SPANS.with(|s| {
            let mut s = s.borrow_mut();
            match s.iter_mut().find(|(n, _, _)| *n == self.0) {
                Some(x) => {
                    x.1 += took;
                    x.2 += 1;
                }
                None => s.push((self.0, took, 1)),
            }
        });
    }
}

/// The frame's spans of at least 1 ms, longest first ("viewport 120 ms, scene 80 ms ×1"), and
/// a fresh start for the next frame.
pub(super) fn take() -> String {
    let mut spans = SPANS.with(|s| std::mem::take(&mut *s.borrow_mut()));
    spans.retain(|(_, d, _)| d.as_secs_f64() >= 1e-3);
    spans.sort_by(|a, b| b.1.cmp(&a.1));
    spans
        .iter()
        .map(|(n, d, k)| {
            let ms = d.as_secs_f64() * 1e3;
            if *k > 1 {
                format!("{n} {ms:.0} ms x{k}")
            } else {
                format!("{n} {ms:.0} ms")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}
