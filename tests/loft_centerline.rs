//! Center Line lofts whose sections are not square to the centre line
//! (`docs/spec/5.2-loft.md`). The owner's session
//! (`tests/data/loft_centerline_journal.jsonl`): a circle on XY and one on a
//! work plane 58 mm up, a spline centre line leaving the bottom section 22°
//! off its normal. The stations were square to the line right up to the
//! sections, so the first ones cut through the bottom plane: the wall bulged
//! below z = 0, the solid crossed itself and was accepted anyway.

mod common;
use common::*;

use std::path::Path;

use cadcraft3d::diagnostics::replay;
use cadcraft3d::part::{
    Feature, LoftFeature, LoftGuide, LoftSection, OriginPlane, PathRef, PlaneRef, WorkDef,
    WorkFeature, WorkPlaneDef,
};

const JOURNAL: &str = "tests/data/loft_centerline_journal.jsonl";

fn the_loft(p: &PartDocument) -> LoftFeature {
    p.features
        .iter()
        .find_map(|f| match f {
            Feature::Loft(l) => Some(l.clone()),
            _ => None,
        })
        .unwrap()
}

/// The loft is healthy and valid, lies between the planes z = 0 and `h`
/// (stations and the solid's box), and its volume is between the smaller
/// section's area × h and the larger one's × h / cos(the steepest angle of
/// the centre line from Z).
fn check(p: &PartDocument, h: Real) -> Real {
    assert!(p.exact, "built with the faceted kernel");
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    for (sid, _) in p.body.solids() {
        p.body.validate_solid(sid).unwrap();
    }
    let l = the_loft(p);
    let tol = 1e-3;
    for s in l.stations(&p.sketches, &p.body).unwrap() {
        for q in s {
            assert!(q.z > -tol && q.z < h + tol, "station point at z = {}", q.z);
        }
    }
    let m = p.exact_bodies.0[0].measure_full().unwrap();
    assert!(
        m.bbox.0.z > -tol && m.bbox.1.z < h + tol,
        "box z {} .. {}",
        m.bbox.0.z,
        m.bbox.1.z
    );
    let areas: Vec<Real> = (0..2)
        .map(|k| l.section_area(&p.sketches, &p.body, k).unwrap())
        .collect();
    let c = l
        .center_line
        .as_ref()
        .unwrap()
        .resolve(&p.sketches, &p.body, Point3::origin())
        .unwrap();
    let pts = c.polyline();
    let steepest = pts
        .windows(2)
        .filter_map(|w| (w[1] - w[0]).try_normalize(1e-12))
        .map(|d| d.z.abs().clamp(0.0, 1.0).acos())
        .fold(0.0, Real::max);
    let (lo, hi) = (
        areas.iter().copied().fold(Real::MAX, Real::min) * h,
        areas.iter().copied().fold(0.0, Real::max) * h / steepest.cos(),
    );
    let v = m.volume;
    assert!(
        v > 0.98 * lo && v < 1.02 * hi,
        "volume {v} outside {lo} .. {hi}"
    );
    v
}

#[test]
fn the_session_loft_stays_between_its_sections() {
    let p = replay::part_at(Path::new(JOURNAL), 10).unwrap();
    let l = the_loft(&p);
    assert_eq!(l.guide, LoftGuide::CenterLine);
    let v = check(&p, 58.0);
    println!("session loft: volume {v:.1} mm3");
}

// ------------------------------------------------------------ variants ---

/// Circles r = 10 on XY (centre at the origin) and on z = h (centre at
/// `top_x`), and the centre line drawn by `line` on XZ.
fn part(
    h: Real,
    top_x: Real,
    line: impl FnOnce(&mut PartDocument, usize) -> Vec<cadcraft3d::sketch::EntityId>,
) -> PartDocument {
    let mut p = PartDocument::default();
    let a = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    p.sketches[a]
        .sketch
        .add_circle(Point2::new(0.0, 0.0), 10.0)
        .unwrap();
    let wp = p
        .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
            base: PlaneRef::Origin(OriginPlane::XY),
            distance: h,
        })))
        .unwrap();
    let b = p
        .create_sketch(
            SketchPlacement {
                plane: PlaneRef::Work(wp),
                ..Default::default()
            },
            &opts(),
        )
        .unwrap();
    let c = p.sketches[b]
        .sketch
        .plane
        .project(&Point3::new(top_x, 0.0, h));
    p.sketches[b]
        .sketch
        .add_circle(Point2::new(c.x, c.y), 10.0)
        .unwrap();
    let k = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XZ), &opts())
        .unwrap();
    let curves = line(&mut p, k);
    let section = |p: &PartDocument, i: usize| {
        let r = p.sketches[i].sketch.regions();
        LoftSection::Profile {
            sketch: p.sketches[i].id.clone(),
            profile: ProfileRef::of(&r[0], r[0].interior_point()),
        }
    };
    let mut def = LoftFeature::new(vec![section(&p, a), section(&p, b)]);
    def.guide = LoftGuide::CenterLine;
    def.center_line = Some(PathRef::Sketch {
        sketch: p.sketches[k].id.clone(),
        curves,
    });
    p.add_loft(def).unwrap();
    p
}

/// Sketch coordinates on sketch `k` of the model point (x, 0, z).
fn xz(p: &PartDocument, k: usize, x: Real, z: Real) -> Point2 {
    let c = p.sketches[k].sketch.plane.project(&Point3::new(x, 0.0, z));
    Point2::new(c.x, c.y)
}

/// A counter-clockwise arc (in X–Z) about `c` from `a` to `b` on sketch `k`.
fn arc(
    p: &mut PartDocument,
    k: usize,
    c: (Real, Real),
    a: (Real, Real),
    b: (Real, Real),
) -> cadcraft3d::sketch::EntityId {
    // The sketch's own axes may mirror X–Z: keep the arc's sense.
    let (o, ux, uz) = (xz(p, k, 0.0, 0.0), xz(p, k, 1.0, 0.0), xz(p, k, 0.0, 1.0));
    let mirrored = (ux - o).perp(&(uz - o)) < 0.0;
    let (c, a, b) = (xz(p, k, c.0, c.1), xz(p, k, a.0, a.1), xz(p, k, b.0, b.1));
    let s = &mut p.sketches[k].sketch;
    let (pc, pa, pb) = (s.add_point(c), s.add_point(a), s.add_point(b));
    if mirrored {
        s.add_arc(pc, pb, pa).unwrap()
    } else {
        s.add_arc(pc, pa, pb).unwrap()
    }
}

/// An arc leaving the bottom section `deg` off its normal and arriving at
/// the top section square to it.
fn start_tilted(deg: Real) -> (PartDocument, Real) {
    let t = deg.to_radians();
    let h = 60.0;
    let r = h / t.sin();
    let top_x = r * (1.0 - t.cos());
    let c = (-r * t.cos(), r * t.sin());
    let p = part(h, top_x, |p, k| {
        // About c from (0, 0) (angle −t) to (top_x, h) (angle 0): CCW in X–Z.
        vec![arc(p, k, c, (0.0, 0.0), (top_x, h))]
    });
    (p, h)
}

#[test]
fn centre_line_leaving_the_start_tilted() {
    for deg in [10.0, 30.0, 45.0] {
        let (p, h) = start_tilted(deg);
        let v = check(&p, h);
        println!("start tilted {deg} deg: volume {v:.1} mm3");
    }
}

#[test]
fn centre_line_tilted_at_both_ends() {
    // A straight centre line 20° off Z: both sections lean from it.
    let h = 60.0;
    let dx = h * 20f64.to_radians().tan();
    let p = part(h, dx, |p, k| {
        let (a, b) = (xz(p, k, 0.0, 0.0), xz(p, k, dx, h));
        vec![p.sketches[k].sketch.add_line(a, b)]
    });
    let v = check(&p, h);
    println!("tilted at both ends: volume {v:.1} mm3");
}

#[test]
fn s_shaped_centre_line() {
    // Two arcs R50: up from (0, 0), through (10, 30), up into (20, 60);
    // square to both sections.
    let p = part(60.0, 20.0, |p, k| {
        vec![
            arc(p, k, (50.0, 0.0), (10.0, 30.0), (0.0, 0.0)),
            arc(p, k, (-30.0, 60.0), (10.0, 30.0), (20.0, 60.0)),
        ]
    });
    let v = check(&p, 60.0);
    println!("S-shaped: volume {v:.1} mm3");
}
