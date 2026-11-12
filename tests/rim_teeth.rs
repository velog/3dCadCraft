//! Teeth cut into a chamfered or rounded rim of a revolved disc, as deep as the blend is wide,
//! so the cut's inner wall touches the blend's inner edge (the owner's part, 2026-11-12: OK
//! froze the app). The first tooth lies on the revolve's seam, where the kernel's Cut came out
//! invalid; the whole part then went to the faceted kernel (seconds every regeneration) and the
//! tooth's circular pattern was refused. Every case must end within a time limit, exactly.
mod common;
use common::rim::{Blend, Case, Teeth, build};
use common::*;

use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Runs `c` on a thread; `None` when it does not end within `limit`.
fn run(c: Case, limit: Duration) -> Option<(Result<PartDocument, String>, Duration)> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(move || {
            let t = Instant::now();
            let r = build(c);
            let _ = tx.send((r, t.elapsed()));
        })
        .ok()?;
    rx.recv_timeout(limit).ok()
}

/// Every variant: chamfer and fillet 0.5 … 3, teeth as deep as the blend, a little less and a
/// little more, cut from the blended face and from the other one, Through All and 5 mm.
fn cases() -> Vec<Case> {
    let mut v = Vec::new();
    for blend in [Blend::Chamfer, Blend::Fillet] {
        for size in [0.5, 1.0, 2.0, 3.0] {
            for depth in [size - 0.05, size, size + 0.05] {
                for from_top in [true, false] {
                    for through in [true, false] {
                        v.push(Case {
                            blend,
                            size,
                            depth,
                            from_top,
                            through,
                            teeth: Teeth::Pattern(6),
                        });
                    }
                }
            }
        }
    }
    v
}

/// The volumes of `c` and of the same teeth drawn in one sketch (another path through the
/// kernel: one compound tool), or why either failed.
fn both(c: Case, limit: Duration) -> Result<(Real, Real, Duration), String> {
    let mut got = Vec::new();
    let mut took = Duration::ZERO;
    for teeth in [c.teeth, Teeth::Sketch(6)] {
        let c = Case { teeth, ..c };
        let (r, t) = run(c, limit).ok_or_else(|| format!("{c:?}: no result within {limit:?}"))?;
        took = took.max(t);
        let p = r.map_err(|e| format!("{c:?}: refused: {e}"))?;
        if !p.exact || !p.feature_health.is_empty() {
            return Err(format!(
                "{c:?}: exact {} health {:?} ({:?})",
                p.exact, p.feature_health, p.exact_failure
            ));
        }
        got.push(volume(&p));
    }
    Ok((got[0], got[1], took))
}

#[test]
fn teeth_through_a_rim_blend_are_exact() {
    // A debug build on a busy machine: the slowest case took 1.9 s (both builds).
    let limit = Duration::from_secs(60);
    // `RIM_ONLY=<text>`: only the cases whose description contains it.
    let only = std::env::var("RIM_ONLY").unwrap_or_default();
    let mut bad = Vec::new();
    for c in cases()
        .into_iter()
        .filter(|c| format!("{c:?}").contains(only.as_str()))
    {
        match both(c, limit) {
            Ok((pattern, sketch, took)) => {
                println!(
                    "{:>6.0} ms  {c:?}  V {pattern:.2} / {sketch:.2}",
                    took.as_secs_f64() * 1e3
                );
                // A tooth the kernel refused as drawn is cut a micrometre smaller
                // (`TANGENT_NUDGE`): 6 teeth × ~10 mm of wall in the material × 10 mm × 1 µm.
                if (pattern - sketch).abs() > 1.0 {
                    bad.push(format!("{c:?}: pattern {pattern:.3} vs sketch {sketch:.3}"));
                }
            }
            Err(e) => {
                println!("FAIL  {e}");
                bad.push(e);
            }
        }
    }
    assert!(bad.is_empty(), "\n{}", bad.join("\n"));
}
