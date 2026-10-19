//! Diagnostics (`docs/spec/0.14-diagnostics.md`): a recorded session
//! replays to the same parts, and a step that does not reproduce is found.
//!
//! To turn a user's journal into a regression test: copy the
//! `session-…-journal.jsonl` into `tests/sessions/`, then
//!
//! ```ignore
//! let steps = replay::replay(Path::new("tests/sessions/x-journal.jsonl")).unwrap();
//! assert!(replay::first_difference(&steps).is_none());
//! ```

mod common;

use std::path::Path;

use cadcraft3d::diagnostics::journal::{Journal, Outcome, part_value};
use cadcraft3d::diagnostics::replay;
use common::*;

/// Box, a boss extruded on its top, the box's vertical edges filleted,
/// shelled open at the bottom: each step journaled as the GUI does.
fn record(path: &Path) -> PartDocument {
    let mut j = Journal::create(path).unwrap();
    let mut p = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
    j.entry("checkpoint", "Box", serde_json::json!({}));
    assert!(j.state("Box", &mut p));

    j.entry("command_start", "Extrude", serde_json::json!({}));
    let s = sketch_on(&mut p, "Box1:Top", |sk, at| {
        sk.add_circle(at(20.0, 15.0, 20.0), 6.0).unwrap();
    });
    boss(&mut p, s, 10.0, |_| true);
    j.entry("command_ok", "Extrude", serde_json::json!({}));
    assert!(j.state("Extrude", &mut p));

    let vertical = edges_where(&p, |h| {
        let corner = |x: Real, a: Real, b: Real| (x - a).abs() < 1e-6 || (x - b).abs() < 1e-6;
        corner(h[0], 0.0, 40.0) && corner(h[1], 0.0, 30.0) && h[2] > 1e-6 && h[2] < 20.0 - 1e-6
    });
    assert_eq!(vertical.len(), 4, "{vertical:?}");
    p.add_blend(BlendFeature::fillet(vertical, 3.0)).unwrap();
    assert!(j.state("Fillet", &mut p));

    shell(
        &mut p,
        &[("Box1:Bottom", [20.0, 15.0, 0.0])],
        2.0,
        ShellDirection::Inside,
    );
    assert!(j.state("Shell", &mut p));
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    p
}

#[test]
fn a_recorded_session_replays_to_the_same_volume() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session-journal.jsonl");
    let part = record(&path);
    let steps = replay::replay(&path).unwrap();
    assert_eq!(
        steps.iter().map(|s| s.label.as_str()).collect::<Vec<_>>(),
        ["Box", "Extrude", "Fillet", "Shell"]
    );
    assert!(
        replay::first_difference(&steps).is_none(),
        "{:?}",
        replay::first_difference(&steps)
    );
    let want = Outcome::of(&part);
    let last = steps.last().and_then(|s| s.replayed.clone()).unwrap();
    assert!(
        (last.volume - want.volume).abs() < 1e-6 * want.volume,
        "{} vs {}",
        last.volume,
        want.volume
    );
    // The shelled part is smaller than the box with its boss.
    let solid = steps[2].replayed.as_ref().unwrap().volume;
    assert!(last.volume < 0.5 * solid, "{} vs {solid}", last.volume);
    // The part at a step can be taken out (e.g. to save as a project).
    let at = replay::part_at(&path, 1).unwrap();
    assert_eq!(at.features.len(), 2);
    assert!((volume(&at) - steps[1].replayed.as_ref().unwrap().volume).abs() < 1.0);
}

#[test]
fn a_step_that_does_not_reproduce_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad-journal.jsonl");
    let part = record(&path);
    // A step whose recording says "healthy, same volume", but whose fillet
    // (radius 100 on a 40×30 box) cannot be built.
    let mut bad = part.clone();
    let k = bad
        .features
        .iter()
        .position(|f| matches!(f, Feature::Blend(_)))
        .unwrap();
    let Feature::Blend(b) = bad.features[k].clone() else {
        panic!("blend");
    };
    let mut big = BlendFeature::fillet(b.edges.clone(), 100.0);
    big.id = b.id.clone();
    big.name = b.name.clone();
    bad.features[k] = Feature::Blend(big);
    let mut j = Journal::create(&path).unwrap();
    // A journal reopened continues after the recorded states: the first
    // state of a new Journal is complete, so this is self-contained.
    let v = part_value(&mut bad).unwrap();
    assert!(j.state_value("Fillet radius", v, &Outcome::of(&part)));
    drop(j);
    let steps = replay::replay(&path).unwrap();
    let first = replay::first_difference(&steps).unwrap();
    assert_eq!(first.label, "Fillet radius");
    assert_eq!(
        first.line,
        std::fs::read_to_string(&path).unwrap().lines().count()
    );
    let diff = first.difference.as_deref().unwrap();
    assert!(diff.contains("sick") || diff.contains("volume"), "{diff}");
}

#[test]
fn damaged_and_cut_off_lines() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("j.jsonl");
    record(&path);
    let mut text = std::fs::read_to_string(&path).unwrap();
    // A crash cut the last line: ignored.
    text.push_str("{\"kind\":\"state\",\"snap\":{\"o\"");
    let steps = replay::replay_text(&text).unwrap();
    assert_eq!(steps.len(), 4);
    assert!(replay::first_difference(&steps).is_none());
    // A damaged line in the middle is reported.
    let mut lines: Vec<&str> = text.lines().collect();
    lines.insert(2, "not json");
    let steps = replay::replay_text(&lines.join("\n")).unwrap();
    assert!(replay::first_difference(&steps).unwrap().error.is_some());
    // Without states: an error.
    assert!(replay::replay_text("{\"kind\":\"session\"}\n").is_err());
}
