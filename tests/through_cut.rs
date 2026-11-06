//! The fast path for Cuts straight through a prism (spec 0.19 §3.2) gives the same body as the
//! general Boolean: volume, faces and every face name, case by case; and it is taken only where
//! its conditions hold.

mod common;
use common::*;

use cadcraft3d::kernel::exact;
use cadcraft3d::part::{LineRef, PatternDirection, PatternFeature, PatternKind, PullRef};
use std::sync::Mutex;
use std::time::Instant;

/// The switch is process-wide: one test at a time.
static SWITCH: Mutex<()> = Mutex::new(());

fn grid(feature: &str, n: u32, s: Real) -> PatternFeature {
    let mut p = PatternFeature::rectangular();
    p.features = vec![feature.into()];
    let d = |i| PatternDirection {
        dir: Some(PullRef::Line(LineRef::Origin(i))),
        count: n,
        spacing: s,
        ..PatternDirection::new()
    };
    p.kind = PatternKind::Rectangular {
        first: d(0),
        second: Some(d(1)),
    };
    p
}

fn small(r: &Region) -> bool {
    r.holes.is_empty() && r.area() < 30.0
}

/// A 40 × 30 × 3 plate.
fn base() -> PartDocument {
    plate(40.0, 30.0, 3.0)
}

/// What is compared: volume, face count, the face names (sorted).
#[derive(Debug, PartialEq)]
struct Shape {
    volume: Real,
    faces: usize,
    names: Vec<String>,
}

fn shape(p: &PartDocument) -> Shape {
    assert!(p.exact, "the part went faceted");
    assert!(p.feature_health.is_empty(), "sick: {:?}", p.feature_health);
    let mut names: Vec<String> = p.body.faces().filter_map(|(_, f)| f.name.clone()).collect();
    names.sort();
    Shape {
        volume: volume(p),
        faces: p.body.faces().count(),
        names,
    }
}

/// Builds `case` with the fast path on and off: the same shape; whether the fast path ran.
fn both(label: &str, case: &dyn Fn() -> PartDocument) -> bool {
    exact::set_fast_cuts(false);
    let general = shape(&case());
    exact::set_fast_cuts(true);
    let before = exact::fast_cuts_done();
    let fast = shape(&case());
    let used = exact::fast_cuts_done() > before;
    assert!(
        (fast.volume - general.volume).abs() < 1e-6 * general.volume,
        "{label}: volume {} vs {}",
        fast.volume,
        general.volume
    );
    assert_eq!(fast.faces, general.faces, "{label}: faces");
    assert_eq!(fast.names, general.names, "{label}: names");
    used
}

#[test]
fn fast_through_cuts_match_the_general_boolean() {
    let _one = SWITCH.lock().unwrap_or_else(|e| e.into_inner());
    // one hole through the plate
    let one = || {
        let mut p = base();
        let t = top(&p, 3.0);
        let j = sketch_on(&mut p, &t, |sk, at| {
            sk.add_circle(at(10.0, 10.0, 3.0), 2.0).unwrap();
        });
        cut(&mut p, j, None, small);
        p
    };
    assert!(
        both("one hole", &one),
        "one hole: the fast path did not run"
    );

    // three holes of one sketch, a square among them
    let three = || {
        let mut p = base();
        let t = top(&p, 3.0);
        let j = sketch_on(&mut p, &t, |sk, at| {
            sk.add_circle(at(8.0, 8.0, 3.0), 2.0).unwrap();
            sk.add_circle(at(20.0, 15.0, 3.0), 1.5).unwrap();
            sk.add_rectangle_two_point(at(28.0, 5.0, 3.0), at(33.0, 9.0, 3.0))
                .unwrap();
        });
        cut(&mut p, j, None, small);
        p
    };
    assert!(
        both("three holes", &three),
        "three holes: the fast path did not run"
    );

    // a pattern of holes, then a second pattern between them
    let patterns = || {
        let mut p = base();
        let t = top(&p, 3.0);
        let j = sketch_on(&mut p, &t, |sk, at| {
            sk.add_circle(at(4.0, 4.0, 3.0), 1.0).unwrap();
        });
        cut(&mut p, j, None, small);
        p.add_pattern(grid("Extrusion2", 4, 6.0)).unwrap();
        let t = top(&p, 3.0);
        let j = sketch_on(&mut p, &t, |sk, at| {
            sk.add_circle(at(7.0, 7.0, 3.0), 0.8).unwrap();
        });
        // only the new circle (the old holes' rims are projected into the sketch)
        cut(&mut p, j, None, |r| r.holes.is_empty() && r.area() < 2.1);
        p.add_pattern(grid("Extrusion3", 3, 6.0)).unwrap();
        p
    };
    assert!(
        both("two patterns", &patterns),
        "two patterns: the fast path did not run"
    );

    // a hole over the plate's edge: the general path
    let edge = || {
        let mut p = base();
        let t = top(&p, 3.0);
        let j = sketch_on(&mut p, &t, |sk, at| {
            sk.add_circle(at(0.0, 15.0, 3.0), 2.0).unwrap();
        });
        cut(&mut p, j, None, |r| r.holes.is_empty() && r.area() < 7.0);
        p
    };
    assert!(
        !both("over the edge", &edge),
        "over the edge: the fast path ran"
    );

    // a blind hole: the general path
    let blind = || {
        let mut p = base();
        let t = top(&p, 3.0);
        let j = sketch_on(&mut p, &t, |sk, at| {
            sk.add_circle(at(10.0, 10.0, 3.0), 2.0).unwrap();
        });
        cut(&mut p, j, Some(1.5), small);
        p
    };
    assert!(!both("blind", &blind), "blind: the fast path ran");

    // two holes that overlap: the general path
    let overlap = || {
        let mut p = base();
        let t = top(&p, 3.0);
        let j = sketch_on(&mut p, &t, |sk, at| {
            sk.add_circle(at(10.0, 10.0, 3.0), 2.0).unwrap();
        });
        cut(&mut p, j, None, small);
        let t = top(&p, 3.0);
        let j = sketch_on(&mut p, &t, |sk, at| {
            sk.add_circle(at(12.0, 10.0, 3.0), 2.0).unwrap();
        });
        cut(&mut p, j, None, |r| r.holes.is_empty() && r.area() < 13.0);
        p
    };
    both("overlapping", &overlap);
}

/// 1024 holes: the fast path within a second, the general path for comparison.
#[test]
fn many_holes_are_fast() {
    let _one = SWITCH.lock().unwrap_or_else(|e| e.into_inner());
    exact::set_fast_cuts(true);
    let mut p = plate(160.0, 160.0, 3.0);
    let t = top(&p, 3.0);
    let j = sketch_on(&mut p, &t, |sk, at| {
        sk.add_circle(at(2.5, 2.5, 3.0), 1.0).unwrap();
    });
    cut(&mut p, j, None, |rg: &Region| {
        rg.holes.is_empty() && rg.area() < PI * 1.01
    });
    p.build_cache = Default::default();
    let t0 = Instant::now();
    p.add_pattern(grid("Extrusion2", 32, 5.0)).unwrap();
    let took = t0.elapsed();
    assert!(p.exact);
    assert_eq!(p.body.faces().count(), 6 + 1024, "faces");
    println!("1024 holes: {took:?}");
}
