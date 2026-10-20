//! The owner's session of 2026-10-20 (`tests/data/sweep_session_journal.jsonl`):
//! a circle R17.72 on XY swept along an interpolation spline on YZ failed
//! with "the geometry kernel cannot build it (sweep: internal error in the
//! geometry kernel)". The whole circle was one arc with a single vertex and
//! the kernel's point-path sweep panicked on it ("This wire is not simple").
//! Also: a failed OK is journaled with the feature it tried, and replays.

mod common;
use common::*;

use std::path::Path;

use cadcraft3d::diagnostics::journal::{Journal, attempt_json};
use cadcraft3d::diagnostics::replay;
use cadcraft3d::part::{Feature, PathRef, SweepFeature};

const JOURNAL: &str = "tests/data/sweep_session_journal.jsonl";

/// The session's part just before the Sweep, and the sweep it tried.
fn session() -> (PartDocument, usize, SweepFeature) {
    let p = replay::part_at(Path::new(JOURNAL), 24).unwrap();
    let i = p.sketches.iter().position(|s| s.id == "Sketch1").unwrap();
    let rg = p.sketches[i].sketch.regions();
    let j = p.sketches.iter().position(|s| s.id == "Sketch2").unwrap();
    let curves = p.sketches[j].sketch.entities().map(|(id, _)| id).collect();
    let def = SweepFeature::new(
        vec![ProfileRef::of(&rg[0], rg[0].interior_point())],
        PathRef::Sketch {
            sketch: "Sketch2".into(),
            curves,
        },
    );
    (p, i, def)
}

#[test]
fn the_session_sweep_builds() {
    let (mut p, i, def) = session();
    let r = 17.721586226130526;
    let path = def
        .path
        .resolve(&p.sketches, &p.body, Point3::origin())
        .unwrap();
    let before = volume(&p);
    let t = std::time::Instant::now();
    p.add_sweep(i, def).unwrap();
    let ms = t.elapsed().as_millis();
    assert!(
        p.exact && p.feature_health.is_empty(),
        "{:?}",
        p.feature_health
    );
    for (sid, _) in p.body.solids() {
        p.body.validate_solid(sid).unwrap();
    }
    // Pappus, with the angle the spline leaves the profile at (the kernel
    // keeps it along the path).
    let cos = path.eval(0.0).1.z.abs();
    let want = PI * r * r * path.length() * cos;
    let got = volume(&p) - before;
    println!(
        "session sweep: volume {got:.1} mm3, expected {want:.1} (path {:.2} mm, cos {cos:.5}), {ms} ms",
        path.length()
    );
    assert!((got - want).abs() < 5e-3 * want, "{got} vs {want}");
}

#[test]
fn a_failed_ok_is_journaled_and_replays() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("j.jsonl");
    let (mut p, i, good) = session();
    let sketch = p.sketches[i].id.clone();
    // As the GUI writes it: the state, then the failing OK's error with
    // the definition the dialog held (no id or sketch yet).
    let mut bad = good.clone();
    bad.taper = 2.0;
    let mut j = Journal::create(&file).unwrap();
    assert!(j.state("Sketch", &mut p));
    let attempt = attempt_json(&Feature::Sweep(bad), Some(&sketch)).unwrap();
    assert_eq!(attempt["sketch"], serde_json::json!(sketch));
    j.entry(
        "error",
        "",
        serde_json::json!({
            "text": "Sweep: the taper must be between -90 and 90 deg",
            "command": "Sweep",
            "attempt": attempt,
        }),
    );
    let ok = attempt_json(&Feature::Sweep(good), Some(&sketch)).unwrap();
    drop(j);

    let text = std::fs::read_to_string(&file).unwrap();
    let found = replay::attempts(&text);
    assert_eq!(found.len(), 1);
    let a = &found[0];
    assert_eq!((a.command.as_str(), a.state), ("Sweep", Some(0)));
    assert!(a.text.contains("taper"));
    // Retried on the part the command saw: the same refusal, part unchanged.
    let mut q = replay::part_at(&file, a.state.unwrap()).unwrap();
    let features = q.features.len();
    let e = replay::retry(&mut q, &a.feature).unwrap_err();
    assert!(e.contains("taper"), "{e}");
    assert_eq!(q.features.len(), features);
    // The corrected definition builds there.
    let id = replay::retry(&mut q, &ok).unwrap();
    assert_eq!(id, "Sweep1");
    assert!(q.feature_health.is_empty(), "{:?}", q.feature_health);
    // The session's own journal (written before this change) has no attempts.
    let old = std::fs::read_to_string(JOURNAL).unwrap();
    assert!(replay::attempts(&old).is_empty());
}
