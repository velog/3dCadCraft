//! Feature patterns of many instances (docs/PERF.md, log entry
//! "patterns with thousands of copies"): a through hole r1 on a 5n × 5n × 3 plate patterned n × n at 5,
//! a circular pattern of holes on a ring, a pattern of joined bosses, and
//! a fillet after 2500 holes. Each checks that the exact kernel
//! built it (one valid body), the volume against the analytic value within
//! the display mesh's chord bound, the face count and the copies' persistent
//! names `<pattern id>#k:<name>`.
//!
//! The larger sizes are `#[ignore]` (minutes in the test profile):
//! `cargo test --release --no-default-features --test pattern_scale -- --ignored`.

mod common;
use common::*;

use cadcraft3d::part::{LineRef, PatternDirection, PatternFeature, PatternKind, PullRef};

/// The kernel's display chord tolerance (`kernel::exact::DISPLAY_TOL`).
const CHORD: Real = 0.01;

/// The volume enclosed by the display triangles of every face.
fn mesh_volume(part: &PartDocument) -> Real {
    part.body
        .faces()
        .filter_map(|(_, f)| f.mesh.as_ref())
        .flat_map(|m| {
            m.triangles.iter().map(|t| {
                let [p, q, s] = t.map(|i| m.positions[i as usize].coords);
                p.dot(&q.cross(&s)) / 6.0
            })
        })
        .sum()
}

/// A plate `side` × `side` × `h` with one round feature r at (x, y): a
/// through hole, or a boss `boss` high joined on top (`Extrusion2`).
fn plate_with(side: Real, h: Real, x: Real, y: Real, r: Real, boss: Option<Real>) -> PartDocument {
    let mut p = plate(side, side, h);
    let t = top(&p, h);
    let j = sketch_on(&mut p, &t, |sk, at| {
        sk.add_circle(at(x, y, h), r).unwrap();
    });
    let small = |rg: &Region| rg.holes.is_empty() && rg.area() < PI * r * r * 1.01;
    match boss {
        None => cut(&mut p, j, None, small),
        Some(b) => common::boss(&mut p, j, b, small),
    }
    p
}

fn grid(n: u32, s: Real) -> PatternFeature {
    let mut p = PatternFeature::rectangular();
    p.features = vec!["Extrusion2".into()];
    p.kind = PatternKind::Rectangular {
        first: PatternDirection {
            dir: Some(PullRef::Line(LineRef::Origin(0))),
            count: n,
            spacing: s,
            ..PatternDirection::new()
        },
        second: Some(PatternDirection {
            dir: Some(PullRef::Line(LineRef::Origin(1))),
            count: n,
            spacing: s,
            ..PatternDirection::new()
        }),
    };
    p
}

/// Exact, healthy, one valid solid, `faces` faces, every copy present by
/// name, and the volume within `slack` above `expect` (holes
/// meshed as inscribed polygons only make the mesh larger) or below it by
/// at most `slack` (bosses).
fn verify(part: &PartDocument, id: &str, count: usize, faces: usize, expect: Real, slack: Real) {
    assert!(part.exact, "built with the faceted kernel");
    assert!(
        part.feature_health.is_empty(),
        "sick: {:?}",
        part.feature_health
    );
    assert_eq!(part.body.solids().count(), 1);
    for (sid, _) in part.body.solids() {
        part.body
            .validate_solid(sid)
            .unwrap_or_else(|e| panic!("invalid solid: {e:?}"));
    }
    assert_eq!(part.body.faces().count(), faces);
    let mut seen = std::collections::BTreeSet::new();
    for (_, f) in part.body.faces() {
        let Some(n) = f.name.as_deref() else {
            panic!("a face without a name")
        };
        if let Some(rest) = n.strip_prefix(&format!("{id}#")) {
            let (k, name) = rest.split_once(':').expect("<id>#k:<name>");
            let k: usize = k.parse().unwrap();
            assert!(name.starts_with("Extrusion2:"), "{n}");
            assert!(k <= count, "{n}");
            seen.insert(k);
        }
    }
    // Every instance but the original (its faces keep the source's names).
    assert_eq!(seen.len(), count - 1, "a copy has no faces");
    let v = mesh_volume(part);
    assert!(
        (v - expect).abs() <= slack + 1e-9 * expect,
        "volume {v} vs {expect} (slack {slack})"
    );
}

/// The display mesh's volume error bound for `k` round walls r × h: an
/// inscribed polygon misses at most the chord sag along the perimeter.
fn chord_slack(k: usize, r: Real, h: Real) -> Real {
    k as Real * 2.0 * PI * r * CHORD * h
}

/// n × n through holes r1 on a 5n plate 3 thick: 6 + 2n² faces (each hole
/// wall in two halves).
fn holes(n: u32) {
    let side = 5.0 * n as Real;
    let mut p = plate_with(side, 3.0, 2.5, 2.5, 1.0, None);
    let id = p.add_pattern(grid(n, 5.0)).unwrap();
    let k = (n * n) as usize;
    let expect = side * side * 3.0 - k as Real * PI * 3.0;
    verify(&p, &id, k, 6 + 2 * k, expect, chord_slack(k, 1.0, 3.0));
}

#[test]
fn pattern_of_100_holes() {
    holes(10);
}

#[test]
fn pattern_of_400_holes() {
    holes(20);
}

#[test]
fn pattern_of_1024_holes() {
    holes(32);
}

#[test]
#[ignore = "large: run with --ignored in release"]
fn pattern_of_2500_holes() {
    holes(50);
}

#[test]
#[ignore = "large: run with --ignored in release"]
fn pattern_of_5041_holes() {
    holes(71);
}

#[test]
#[ignore = "large: run with --ignored in release"]
fn pattern_of_10000_holes() {
    holes(100);
}

/// `n` holes r0.4 at 1.2 mm on a ring about Z (a disc 10 wider, 3 thick;
/// its rim in two halves, so 4 + 2n faces):
/// the copies' boxes, turned, touch; their oriented boxes do not.
fn ring(n: u32) {
    let ring = (n as Real * 1.2 / (2.0 * PI)).max(180.0);
    let disc = ring + 10.0;
    let mut p = PartDocument::default();
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    p.sketches[i]
        .sketch
        .add_circle(Point2::new(0.0, 0.0), disc)
        .unwrap();
    let rg = p.sketches[i].sketch.regions();
    p.add_extrude(i, ExtrudeFeature::new(region_profiles(&rg, |_| true), 3.0))
        .unwrap();
    let t = top(&p, 3.0);
    let j = sketch_on(&mut p, &t, |sk, at| {
        sk.add_circle(at(ring, 0.0, 3.0), 0.4).unwrap();
    });
    cut(&mut p, j, None, |rg| rg.holes.is_empty() && rg.area() < 1.0);
    let mut c = PatternFeature::circular();
    c.features = vec!["Extrusion2".into()];
    if let PatternKind::Circular { axis, count, .. } = &mut c.kind {
        (*axis, *count) = (Some(LineRef::Origin(2)), n);
    }
    let id = p.add_pattern(c).unwrap();
    let k = n as usize;
    let expect = PI * disc * disc * 3.0 - k as Real * PI * 0.16 * 3.0;
    // The disc's rim is meshed as a polygon too.
    let slack = chord_slack(k, 0.4, 3.0) + chord_slack(1, disc, 3.0);
    verify(&p, &id, k, 4 + 2 * k, expect, slack);
}

#[test]
fn circular_pattern_of_1000_holes() {
    ring(1000);
}

#[test]
#[ignore = "large: run with --ignored in release"]
fn circular_pattern_of_5000_holes() {
    ring(5000);
}

/// n × n bosses r1.5 × 2 joined on a 5n plate 3 thick (one Join of all
/// copies at once): 6 + 3n² faces (each boss: two wall halves and a top).
fn bosses(n: u32) {
    let side = 5.0 * n as Real;
    let mut p = plate_with(side, 3.0, 2.5, 2.5, 1.5, Some(2.0));
    let id = p.add_pattern(grid(n, 5.0)).unwrap();
    let k = (n * n) as usize;
    let expect = side * side * 3.0 + k as Real * PI * 2.25 * 2.0;
    let faces = p.body.faces().count();
    verify(&p, &id, k, faces, expect, chord_slack(k, 1.5, 2.0));
    assert_eq!(faces, 6 + 3 * k);
}

#[test]
fn pattern_of_100_bosses() {
    bosses(10);
}

#[test]
#[ignore = "large: run with --ignored in release"]
fn pattern_of_1024_bosses() {
    bosses(32);
}

/// Cross test: 2500 holes, then a fillet r2 of the plate's four vertical
/// edges.
#[test]
#[ignore = "large: run with --ignored in release"]
fn fillet_after_2500_holes() {
    let n = 50;
    let side = 5.0 * n as Real;
    let mut p = plate_with(side, 3.0, 2.5, 2.5, 1.0, None);
    p.add_pattern(grid(n, 5.0)).unwrap();
    let k = (n * n) as usize;
    let holes = side * side * 3.0 - k as Real * PI * 3.0;
    let edges = vertical(&p, side, side, 3.0);
    assert_eq!(edges.len(), 4);
    let mut f = p.clone();
    f.add_blend(BlendFeature::fillet(edges, 2.0)).unwrap();
    assert!(f.exact && f.feature_health.is_empty());
    let expect = holes - 4.0 * round_loss(2.0) * 3.0;
    let v = mesh_volume(&f);
    let slack = chord_slack(k, 1.0, 3.0) + chord_slack(4, 2.0, 3.0);
    assert!((v - expect).abs() <= slack, "fillet: {v} vs {expect}");
    // The copies keep their names through the fillet.
    let named = f
        .body
        .faces()
        .filter(|(_, x)| {
            x.name
                .as_deref()
                .is_some_and(|s| s.starts_with("RectangularPattern1#"))
        })
        .count();
    assert_eq!(named, 2 * (k - 1));
}
