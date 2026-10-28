//! A hole drilled through a block at 45° (owner, 2026-10-28): it leaves the top face in an
//! ellipse — the edge where a cylinder meets a plane at an angle, a B-spline in the exact
//! kernel. Fillet and Chamfer of that edge must work, as on a square hole's circle.

mod common;

use cadcraft3d::kernel::{Point3, Real};
use cadcraft3d::part::{
    BlendFeature, EdgeRef, Extent, ExtentDirection, ExtrudeFeature, LineRef, Operation,
    OriginPlane, PartDocument, PlaneRef, ProfileRef, SketchPlacement, WorkDef, WorkFeature,
    WorkPlaneDef,
};
use cadcraft3d::sketch::Point2;
use common::*;

const R: Real = 4.0;

/// A 40 x 30 x 20 block with a Ø8 hole through it whose axis leans 45° in YZ and leaves the
/// top at (20, 15, 20): the top edge is an ellipse with semi-axes 4 (x) and 4√2 (y).
fn block_with_inclined_hole() -> PartDocument {
    block_with_hole(R)
}

fn block_with_hole(radius: Real) -> PartDocument {
    let mut p = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
    // The sketch plane through the X axis, 45° from XY (its normal leans in YZ).
    let wp = p
        .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Angle {
            base: PlaneRef::Origin(OriginPlane::XY),
            axis: LineRef::Origin(0),
            angle: std::f64::consts::FRAC_PI_4,
        })))
        .unwrap();
    let i = p
        .create_sketch(
            SketchPlacement {
                plane: PlaneRef::Work(wp),
                ..Default::default()
            },
            &opts(),
        )
        .unwrap();
    let sk = &mut p.sketches[i].sketch;
    let n = sk.plane.normal.into_inner();
    // The axis line through the top's middle meets the sketch plane at the centre.
    let top = Point3::new(20.0, 15.0, 20.0);
    let t = (sk.plane.origin - top).dot(&n);
    let c = sk.plane.project(&(top + n * t));
    sk.add_circle(Point2::new(c.x, c.y), radius).unwrap();
    let r = sk.regions();
    let mut f = ExtrudeFeature::new(vec![ProfileRef::of(&r[0], r[0].interior_point())], 10.0);
    f.extent = Extent::ThroughAll;
    f.direction = ExtentDirection::Symmetric;
    f.operation = Operation::Cut;
    p.add_extrude(i, f).unwrap();
    assert!(p.exact, "exact kernel");
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    p
}

/// The hole's edge on the top face (an ellipse), as an edge reference.
fn top_ellipse(p: &PartDocument) -> EdgeRef {
    let body = &p.body;
    let at = Point3::new(20.0 + R, 15.0, 20.0);
    let e = body
        .edges()
        .map(|(e, _)| e)
        .filter(|&e| {
            body.sample_edge(e, &Default::default()).is_ok_and(|pts| {
                pts.iter().all(|q| (q.z - 20.0).abs() < 1e-6)
                    && pts.iter().any(|q| (q - at).norm() < 0.3)
            })
        })
        .min_by_key(|&e| {
            body.sample_edge(e, &Default::default())
                .map(|pts| {
                    (pts.iter()
                        .map(|q| (q - at).norm())
                        .fold(Real::MAX, Real::min)
                        * 1e6) as i64
                })
                .unwrap_or(i64::MAX)
        })
        .expect("the ellipse on the top");
    cadcraft3d::part::blend::edge_ref(body, e).expect("an edge between two named faces")
}

fn volume(p: &PartDocument) -> Real {
    p.exact_bodies
        .0
        .iter()
        .filter_map(|s| s.volume_at(1e-4))
        .sum()
}

#[test]
fn fillet_and_chamfer_the_exit_of_an_inclined_hole() {
    let base = block_with_inclined_hole();
    let v0 = volume(&base);
    for (what, f) in [
        (
            "fillet",
            BlendFeature::fillet(vec![top_ellipse(&base)], 1.0),
        ),
        (
            "chamfer",
            BlendFeature::chamfer(vec![top_ellipse(&base)], 1.0),
        ),
    ] {
        let mut p = base.clone();
        p.add_blend(f).unwrap_or_else(|e| panic!("{what}: {e}"));
        assert!(p.exact, "{what}: exact kernel");
        assert!(
            p.feature_health.is_empty(),
            "{what}: {:?}",
            p.feature_health
        );
        let v = volume(&p);
        // Material goes at the rim: a little, not a lot.
        assert!(v < v0 && v > v0 - 200.0, "{what}: {v0} -> {v}");
    }
}

/// The hole's edges on face `face` (every piece of the ellipse there).
fn hole_edges_on(p: &PartDocument, face: &str) -> Vec<EdgeRef> {
    p.body
        .edges()
        .map(|(e, _)| e)
        .filter_map(|e| cadcraft3d::part::blend::edge_ref(&p.body, e))
        .filter(|r| {
            r.faces.iter().any(|f| f == face) && r.faces.iter().any(|f| f.starts_with("Extrusion1"))
        })
        .collect()
}

#[test]
fn fillet_and_chamfer_the_exit_on_a_side_wall() {
    // r 3: the hole leaves through the back wall (y = 30) in a whole ellipse.
    let base = block_with_hole(3.0);
    let v0 = volume(&base);
    let exit = hole_edges_on(&base, "Box1:Back");
    assert!(!exit.is_empty());
    for (what, f) in [
        ("fillet", BlendFeature::fillet(exit.clone(), 1.0)),
        ("chamfer", BlendFeature::chamfer(exit.clone(), 1.0)),
    ] {
        let mut p = base.clone();
        let t = std::time::Instant::now();
        p.add_blend(f).unwrap_or_else(|e| panic!("{what}: {e}"));
        eprintln!("{what}: {:?}", t.elapsed());
        assert!(
            p.exact && p.feature_health.is_empty(),
            "{what}: {:?}",
            p.feature_health
        );
        let v = volume(&p);
        assert!(v < v0 && v > v0 - 200.0, "{what}: {v0} -> {v}");
    }
}

#[test]
fn the_exit_across_a_corner_is_refused_quickly() {
    // r 4: the exit runs over the back wall and the bottom, through their edge; the ball
    // would have to turn that corner, which the kernel's blend does not do yet. It used to try
    // all 24 orders of the four edges (100-200 s, the window frozen); now it says so at once.
    let base = block_with_hole(R);
    let mut exit = hole_edges_on(&base, "Box1:Back");
    exit.extend(hole_edges_on(&base, "Box1:Bottom"));
    let mut p = base.clone();
    let t = std::time::Instant::now();
    let r = p.add_blend(BlendFeature::fillet(exit, 1.0));
    let took = t.elapsed();
    assert!(r.is_err(), "it works now: turn this into a volume check");
    assert!(took.as_secs_f64() < 30.0, "{took:?}");
    assert_eq!(
        p.features.len(),
        base.features.len(),
        "the part is unchanged"
    );
}
