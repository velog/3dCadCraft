//! Holes whose bore reaches the far side of a plate, and modeled threads on
//! large plates (`docs/spec/2.3-hole.md`, `docs/spec/2.9-thread.md`).
//!
//! The owner's session (`tests/data/m8_modeled_blind_journal.jsonl`): a
//! tapped M8 hole 10 deep (Distance = the plate's thickness, 118° point) on
//! a 283 × 136 × 10 plate took 6 s to cut, and its modeled thread was
//! refused after 150 s ("shell is not oriented and closed"). The second
//! journal (`sweep_session_journal.jsonl`) is the same session later: the
//! plate edited to 50 × 50, then Rebuild All failed the hole with a
//! misleading "needs the exact geometry kernel".

mod common;

use std::path::Path;

use cadcraft3d::diagnostics::journal::Outcome;
use cadcraft3d::diagnostics::replay;
use cadcraft3d::part::hole::HoleCenter;
use cadcraft3d::part::standards::{self, Standards};
use cadcraft3d::part::{DrillPoint, HoleFeature, HoleSpec, Termination, ThreadSpec};
use common::*;

const OWNER: &str = "tests/data/m8_modeled_blind_journal.jsonl";
const SESSION: &str = "tests/data/sweep_session_journal.jsonl";

fn thread(size: &str) -> ThreadSpec {
    let mut t =
        ThreadSpec::from_table(&Standards::builtin(), standards::ISO_METRIC, size, true).unwrap();
    t.modeled = true;
    t
}

/// The volume a modeled internal groove removes per turn beyond the bore
/// (radius `rb`): 2π ∫ r·w(r) dr of the section.
fn groove_per_turn(t: &ThreadSpec, rb: Real) -> Real {
    let g = t.internal_groove(rb).unwrap();
    let (r0, w0, r1, w1) = (g[0][0], -g[0][1], g[1][0], -g[1][1]);
    let n = 4000;
    let mut s = 0.0;
    for i in 0..n {
        let r = rb + (r1 - rb) * (i as Real + 0.5) / n as Real;
        let w = w1 + (w0 - w1) * (r1 - r) / (r1 - r0);
        s += r * 2.0 * w * (r1 - rb) / n as Real;
    }
    2.0 * PI * s
}

/// Holes `spec` at `centers` (x, y) on the top (z = `h`) of a plate.
fn holes(part: &mut PartDocument, spec: HoleSpec, h: Real, centers: &[(Real, Real)]) {
    let pts: Vec<Point3> = centers.iter().map(|&(x, y)| Point3::new(x, y, h)).collect();
    let (i, ids) = part.create_hole_sketch("Extrusion1:End", &pts).unwrap();
    let mut def = HoleFeature::new(spec);
    def.sketch = part.sketches[i].id.clone();
    def.centers = ids.into_iter().map(HoleCenter::Point).collect();
    part.add_hole(def).unwrap();
}

fn faces_named(part: &PartDocument, name: &str) -> usize {
    part.body
        .faces()
        .filter(|(_, f)| f.name.as_deref() == Some(name))
        .count()
}

fn cone(r: Real, angle: Real) -> Real {
    PI * r * r * r / (angle / 2.0).tan() / 3.0
}

// ------------------------------------------------------- plain holes ---

/// A Ø6.647 hole `depth` deep in a W×D×10 plate, with or without a 118°
/// point: exact, valid, the analytic volume (a point only where it lies
/// in the material) and the bore named. A bore reaching the far face used
/// to send the kernel down a slow path (283 mm: 6.1 s; 150 mm: an invalid
/// non-manifold result).
#[test]
fn plain_hole_depth_around_the_thickness() {
    let r: Real = 6.647 / 2.0;
    for (w, d) in [(50.0, 30.0), (150.0, 80.0), (300.0, 136.0)] {
        for depth in [8.0, 9.0, 10.0, 12.0] {
            for point in [DrillPoint::Flat, DrillPoint::Angle] {
                let mut part = plate(w, d, 10.0);
                let mut s = HoleSpec::simple(6.647, depth);
                s.drill_point = point;
                holes(&mut part, s, 10.0, &[(20.0, 15.0)]);
                let tip = if point == DrillPoint::Angle {
                    r / (118f64.to_radians() / 2.0).tan()
                } else {
                    0.0
                };
                // The bore's wall, then what of the cone is in the plate.
                let wall = depth.min(10.0);
                let mut removed = PI * r * r * wall;
                if tip > 0.0 && depth < 10.0 {
                    let full = cone(r, 118f64.to_radians());
                    let inside = (10.0 - depth).min(tip);
                    // The cone's part above the far face: the whole cone
                    // less the similar cone below it.
                    let below = (tip - inside) / tip;
                    removed += full * (1.0 - below.powi(3));
                }
                let case = format!("{w}x{d}, depth {depth}, {point:?}");
                assert!(part.exact, "{case}: faceted");
                check(&part, w * d * 10.0 - removed, 2e-4);
                assert!(faces_named(&part, "Hole1:Bore:1") >= 1, "{case}");
            }
        }
    }
}

// --------------------------------------------------- modeled threads ---

/// The owner's part: Hole1 cosmetic, then made modeled.
fn owner_modeled() -> (PartDocument, Result<(), String>) {
    let mut part = replay::part_at(Path::new(OWNER), 8).unwrap();
    let k = part
        .features
        .iter()
        .position(|f| f.id() == "Hole1")
        .unwrap();
    let Feature::Hole(mut h) = part.features[k].clone() else {
        panic!("Hole1 is not a hole")
    };
    h.spec.thread.as_mut().unwrap().modeled = true;
    let r = part
        .replace_feature(k, Feature::Hole(h))
        .map(|_| ())
        .map_err(|e| e.to_string());
    (part, r)
}

#[test]
fn owner_m8_blind_through_the_plate_is_threaded() {
    let (part, r) = owner_modeled();
    r.unwrap();
    // Plate before the hole (recorded by the session), the Ø6.647 bore
    // through its 10 mm, and the groove over the whole thickness (it runs
    // out at both faces).
    let plate = 386316.30873540504;
    let t = thread("M8");
    let rb = t.minor / 2.0;
    let bored = plate - PI * rb * rb * 10.0;
    let groove = groove_per_turn(&t, rb) * 10.0 / t.pitch;
    check(&part, bored - groove, 1e-3);
    let v = volume(&part);
    assert!(
        ((bored - v) - groove).abs() < 0.03 * groove,
        "groove {} vs {groove}",
        bored - v
    );
    assert!(faces_named(&part, "Hole1:Thread:1") > 100);
    // The plate's own faces keep their names (Extrusion1 on XZ).
    assert!(faces_named(&part, "Extrusion1:Start") >= 1);
    assert!(faces_named(&part, "Extrusion1:End") >= 1);
}

#[test]
fn m8_blind_shorter_than_the_plate() {
    let mut part = plate(283.3, 136.4, 10.0);
    let mut t = thread("M8");
    t.full_depth = false;
    t.depth = 6.0;
    holes(
        &mut part,
        HoleSpec::tapped(t.clone(), 8.0),
        10.0,
        &[(20.0, 120.0)],
    );
    let rb = t.minor / 2.0;
    let bored = 283.3 * 136.4 * 10.0 - PI * rb * rb * 8.0 - cone(rb, 118f64.to_radians());
    let w0 = -t.internal_groove(rb).unwrap()[0][1];
    let groove = groove_per_turn(&t, rb) * (6.0 - w0) / t.pitch;
    let v = volume(&part);
    check(&part, v, 0.0);
    // 36 stations a turn and the faceted bore of the zone (see holes.rs).
    assert!(
        ((bored - v) - groove).abs() < 0.06 * groove,
        "groove {} vs {groove}",
        bored - v
    );
    assert!(faces_named(&part, "Hole1:Thread:1") > 50);
    assert!(faces_named(&part, "Hole1:Tip:1") >= 1, "the point stays");
}

#[test]
fn m8_through_all_in_a_large_plate() {
    let mut part = plate(283.3, 136.4, 10.0);
    let t = thread("M8");
    let mut s = HoleSpec::tapped(t.clone(), 10.0);
    s.termination = Termination::ThroughAll;
    holes(&mut part, s, 10.0, &[(140.0, 70.0)]);
    let rb = t.minor / 2.0;
    let bored = 283.3 * 136.4 * 10.0 - PI * rb * rb * 10.0;
    let groove = groove_per_turn(&t, rb) * 10.0 / t.pitch;
    check(&part, bored - groove, 1e-3);
    assert!(faces_named(&part, "Hole1:Thread:1") > 100);
}

#[test]
fn m3_through_a_300_x_3_plate() {
    // Noted in the spec as a known failure of the one-zone cut.
    let mut part = plate(300.0, 300.0, 3.0);
    let t = thread("M3");
    let mut s = HoleSpec::tapped(t.clone(), 3.0);
    s.termination = Termination::ThroughAll;
    holes(&mut part, s, 3.0, &[(20.0, 20.0)]);
    let rb = t.minor / 2.0;
    let bored = 300.0 * 300.0 * 3.0 - PI * rb * rb * 3.0;
    let groove = groove_per_turn(&t, rb) * 3.0 / t.pitch;
    check(&part, bored - groove, 2e-5);
    let v = volume(&part);
    assert!(
        ((bored - v) - groove).abs() < 0.05 * groove,
        "groove {} vs {groove}",
        bored - v
    );
    assert!(faces_named(&part, "Hole1:Thread:1") > 50);
}

#[test]
fn m10_close_to_the_plate_edge() {
    // 5.5 mm from the side: the zone around the thread crosses the side
    // face, the wall left there is 0.5 mm.
    let mut part = plate(60.0, 40.0, 12.0);
    let t = thread("M10");
    let mut s = HoleSpec::tapped(t.clone(), 12.0);
    s.termination = Termination::ThroughAll;
    holes(&mut part, s, 12.0, &[(5.5, 20.0)]);
    let rb = t.minor / 2.0;
    let bored = 60.0 * 40.0 * 12.0 - PI * rb * rb * 12.0;
    let groove = groove_per_turn(&t, rb) * 12.0 / t.pitch;
    check(&part, bored - groove, 1e-3);
    assert!(faces_named(&part, "Hole1:Thread:1") > 100);
    let sides = part
        .body
        .faces()
        .filter(|(_, f)| {
            f.name
                .as_deref()
                .is_some_and(|n| n.starts_with("Extrusion1:Side"))
        })
        .count();
    assert!(sides >= 4, "{sides} side faces keep their names");
}

#[test]
fn nine_modeled_m6_holes() {
    let mut part = plate(60.0, 60.0, 8.0);
    let t = thread("M6");
    let mut s = HoleSpec::tapped(t.clone(), 8.0);
    s.termination = Termination::ThroughAll;
    let centers: Vec<(Real, Real)> = (0..9)
        .map(|i| (15.0 + 15.0 * (i % 3) as Real, 15.0 + 15.0 * (i / 3) as Real))
        .collect();
    holes(&mut part, s, 8.0, &centers);
    let rb = t.minor / 2.0;
    let bored = 60.0 * 60.0 * 8.0 - 9.0 * PI * rb * rb * 8.0;
    let groove = 9.0 * groove_per_turn(&t, rb) * 8.0 / t.pitch;
    check(&part, bored - groove, 1e-3);
    for n in 1..=9 {
        assert!(
            faces_named(&part, &format!("Hole1:Thread:{n}")) > 50,
            "thread {n}"
        );
    }
}

#[test]
fn modeled_thread_then_fillet_of_the_plate_edges() {
    let (w, d, h) = (80.0, 60.0, 10.0);
    let mut part = plate(w, d, h);
    let t = thread("M8");
    let mut s = HoleSpec::tapped(t.clone(), h);
    s.termination = Termination::ThroughAll;
    holes(&mut part, s, h, &[(40.0, 30.0)]);
    let edges = vertical(&part, w, d, h);
    assert_eq!(edges.len(), 4, "{edges:?}");
    part.add_blend(BlendFeature::fillet(edges, 5.0)).unwrap();
    let rb = t.minor / 2.0;
    let bored = w * d * h - PI * rb * rb * h - 4.0 * round_loss(5.0) * h;
    let groove = groove_per_turn(&t, rb) * h / t.pitch;
    check(&part, bored - groove, 1e-3);
    assert!(faces_named(&part, "Hole1:Thread:1") > 100);
    let rounds = part
        .body
        .faces()
        .filter(|(_, f)| f.name.as_deref().is_some_and(|n| n.starts_with("Fillet1:")))
        .count();
    assert!(rounds >= 4, "{rounds} round faces");
}

// ------------------------------------------- the later session state ---

#[test]
fn session_hole_after_the_plate_was_edited_rebuilds() {
    // State 12: the plate edited to 50 × 50 × 10, the hole's centre
    // dragged to (12, 36). Rebuild All failed the hole after 15 s ("This
    // wire is not simple" in the kernel, shown as "needs the exact geometry
    // kernel").
    let mut part = replay::part_at(Path::new(SESSION), 12).unwrap();
    part.build_cache = Default::default();
    part.regenerate().unwrap();
    let rb: Real = 6.647 / 2.0;
    check(&part, 50.0 * 50.0 * 10.0 - PI * rb * rb * 10.0, 2e-4);
    // Rebuilt from scratch it is the same part: the replay is deterministic.
    let mut again = replay::part_at(Path::new(SESSION), 12).unwrap();
    again.build_cache = Default::default();
    again.regenerate().unwrap();
    assert_eq!(Outcome::of(&part), Outcome::of(&again));
}

#[test]
fn session_replay_differs_only_where_a_sketch_was_being_edited() {
    // States 9–12 were recorded inside a sketch (Edit Dimension, Drag): the
    // session kept the old body until Rebuild All, the replay regenerates
    // every state. Journals now mark such states (`in_sketch`) and the
    // replay does not compare them; this old journal has no mark, so the
    // differences are exactly those states (and state 8, whose hole now
    // has 9 faces instead of 12: no point in the air, the same volume).
    let steps = replay::replay(Path::new(SESSION)).unwrap();
    let differ: Vec<u64> = steps
        .iter()
        .filter(|s| s.differs())
        .map(|s| s.seq)
        .collect();
    assert_eq!(differ, vec![8, 9, 10, 11, 12], "{steps:#?}");
    let hole = steps.iter().find(|s| s.seq == 8).unwrap();
    let (a, b) = (
        hole.recorded.as_ref().unwrap(),
        hole.replayed.as_ref().unwrap(),
    );
    assert!((a.volume - b.volume).abs() < 1e-6 * a.volume);
    for s in &steps {
        assert!(s.error.is_none(), "{s:?}");
    }
    // Marked states are not compared.
    let text = std::fs::read_to_string(SESSION).unwrap();
    let marked: String = text
        .lines()
        .enumerate()
        .map(|(i, l)| {
            if (31..=38).contains(&(i + 1)) && l.contains("\"kind\":\"state\"") {
                l.replacen("\"result\":{", "\"result\":{\"in_sketch\":true,", 1)
            } else {
                l.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let steps = replay::replay_text(&marked).unwrap();
    let differ: Vec<u64> = steps
        .iter()
        .filter(|s| s.differs())
        .map(|s| s.seq)
        .collect();
    assert_eq!(differ, vec![8]);
}
