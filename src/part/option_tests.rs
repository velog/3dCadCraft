//! Tests of the Extrude / Revolve options from the Inventor 2027 comparison
//! (`docs/spec/2.1-extrude.md`, `docs/spec/2.2-revolve.md`): Predict Boolean,
//! To a point / plane, inclined To, Alternate Solution on a cylinder,
//! Extend face, Terminator, Match Shape, Revolve To with Minimum Solution.

use std::f64::consts::PI;

use crate::kernel::{Point3, Real};
use crate::sketch::Point2;

use super::{
    AxisRef, Extent, ExtentDirection, ExtrudeFeature, OpenProfile, Operation, OriginPlane,
    PartDocument, PlaneRef, PointRef, ProfileRef, RevolveExtent, RevolveFeature, SketchOptions,
    SketchPlacement, TargetRef, WorkDef, WorkFeature, WorkPlaneDef, WorkPointDef,
};

fn opts() -> SketchOptions {
    SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    }
}

fn exact_volume(part: &PartDocument) -> Real {
    assert!(part.exact, "built by the exact kernel");
    part.exact_bodies
        .0
        .iter()
        .map(|s| s.measure().unwrap().0)
        .sum()
}

fn body_volume(part: &PartDocument, name: &str) -> Real {
    part.exact_bodies
        .0
        .iter()
        .filter(|s| s.name.as_deref() == Some(name))
        .map(|s| s.measure().unwrap().0)
        .sum()
}

fn close(got: Real, want: Real, what: &str) {
    assert!(
        (got - want).abs() <= 3e-4 * want.abs().max(1.0),
        "{what}: {got} vs {want}"
    );
}

/// A sketch on `placement` with a rectangle given in model coordinates
/// (projected onto the sketch plane); returns its index and profile.
fn rect_sketch(
    part: &mut PartDocument,
    placement: SketchPlacement,
    a: Point3,
    b: Point3,
) -> (usize, ProfileRef) {
    let i = part.create_sketch(placement, &opts()).unwrap();
    let sk = &mut part.sketches[i].sketch;
    let (p, q) = (sk.plane.project(&a), sk.plane.project(&b));
    sk.add_rectangle_two_point(p, q).unwrap();
    let r = sk.regions();
    (i, ProfileRef::of(&r[0], r[0].interior_point()))
}

fn circle_sketch(
    part: &mut PartDocument,
    placement: SketchPlacement,
    c: Point3,
    r: Real,
) -> (usize, ProfileRef) {
    let i = part.create_sketch(placement, &opts()).unwrap();
    let sk = &mut part.sketches[i].sketch;
    let p = sk.plane.project(&c);
    sk.add_circle(p, r).unwrap();
    let rg = sk.regions();
    (i, ProfileRef::of(&rg[0], rg[0].interior_point()))
}

fn block() -> PartDocument {
    PartDocument::with_box("Box1", [80.0, 60.0, 20.0]).unwrap()
}

fn xy() -> SketchPlacement {
    SketchPlacement::on_origin_plane(OriginPlane::XY)
}

// ------------------------------------------------------ Predict Boolean ---

#[test]
fn predicted_operation_in_several_layouts() {
    // No body: New Solid.
    let mut part = PartDocument::default();
    let (i, pr) = circle_sketch(&mut part, xy(), Point3::new(10.0, 10.0, 0.0), 5.0);
    let e = ExtrudeFeature::new(vec![pr], 5.0);
    assert_eq!(
        e.predict_operation(&part.body, &part.sketches[i].sketch),
        Operation::NewSolid
    );
    type Case = (SketchPlacement, ExtentDirection, Real, Operation);
    let cases: Vec<Case> = vec![
        // On the top face, outwards: Join; into the part: Cut.
        (
            SketchPlacement::on_face("Box1:Top"),
            ExtentDirection::Default,
            5.0,
            Operation::Join,
        ),
        (
            SketchPlacement::on_face("Box1:Top"),
            ExtentDirection::Flipped,
            5.0,
            Operation::Cut,
        ),
        // On XY (the bottom), the normal points into the part.
        (xy(), ExtentDirection::Default, 5.0, Operation::Cut),
        (xy(), ExtentDirection::Flipped, 5.0, Operation::Join),
        // Symmetric on the top face: one side is outside, Join.
        (
            SketchPlacement::on_face("Box1:Top"),
            ExtentDirection::Symmetric,
            6.0,
            Operation::Join,
        ),
    ];
    for (k, (placement, dir, d, want)) in cases.into_iter().enumerate() {
        let mut part = block();
        let (i, pr) = circle_sketch(&mut part, placement, Point3::new(30.0, 30.0, 20.0), 5.0);
        let mut e = ExtrudeFeature::new(vec![pr], d);
        e.direction = dir;
        let got = e.predict_operation(&part.body, &part.sketches[i].sketch);
        assert_eq!(got, want, "case {k}");
        // And the prediction builds: Join adds, Cut removes.
        e.operation = got;
        part.add_extrude(i, e).unwrap();
        let v = exact_volume(&part);
        let base = 80.0 * 60.0 * 20.0;
        match got {
            Operation::Join => assert!(v > base + 1.0, "case {k}: {v}"),
            _ => assert!(v < base - 1.0, "case {k}: {v}"),
        }
    }
    // A sketch on a work plane above the part, extruded down through it:
    // Cut; up: Join. Inside the part (mid plane), symmetric: Cut.
    for (offset, dir, d, want) in [
        (40.0, ExtentDirection::Flipped, 50.0, Operation::Cut),
        (40.0, ExtentDirection::Default, 50.0, Operation::Join),
        (10.0, ExtentDirection::Symmetric, 4.0, Operation::Cut),
    ] {
        let mut part = block();
        let wp = part
            .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
                base: PlaneRef::Origin(OriginPlane::XY),
                distance: offset,
            })))
            .unwrap();
        let placement = SketchPlacement {
            plane: PlaneRef::Work(wp),
            ..Default::default()
        };
        let (i, pr) = circle_sketch(&mut part, placement, Point3::new(30.0, 30.0, offset), 5.0);
        let mut e = ExtrudeFeature::new(vec![pr], d);
        e.direction = dir;
        assert_eq!(
            e.predict_operation(&part.body, &part.sketches[i].sketch),
            want,
            "offset {offset} {dir:?}"
        );
    }
}

#[test]
fn predicted_revolve_operation() {
    let mut part = PartDocument::default();
    // A shaft r 10, 40 long around Y.
    let i = part.create_sketch(xy(), &opts()).unwrap();
    let sk = &mut part.sketches[i].sketch;
    sk.add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(10.0, 40.0))
        .unwrap();
    let r = sk.regions();
    let pr = ProfileRef::of(&r[0], r[0].interior_point());
    let shaft = RevolveFeature::new(vec![pr], AxisRef::Origin(1));
    assert_eq!(
        shaft.predict_operation(&part.body, &part.sketches[i].sketch),
        Operation::NewSolid
    );
    part.add_revolve(i, shaft).unwrap();
    // A groove inside the shaft: Cut; a flange outside it: Join.
    for (a, b, want) in [
        ((8.0, 10.0), (10.0, 14.0), Operation::Cut),
        ((10.0, 20.0), (16.0, 24.0), Operation::Join),
        ((2.0, 30.0), (6.0, 35.0), Operation::Cut),
        ((12.0, 2.0), (14.0, 4.0), Operation::Join),
    ] {
        let j = part.create_sketch(xy(), &opts()).unwrap();
        let sk = &mut part.sketches[j].sketch;
        sk.add_rectangle_two_point(Point2::new(a.0, a.1), Point2::new(b.0, b.1))
            .unwrap();
        let r = sk.regions();
        let pr = ProfileRef::of(&r[0], r[0].interior_point());
        let rv = RevolveFeature::new(vec![pr], AxisRef::Origin(1));
        assert_eq!(
            rv.predict_operation(&part.body, &part.sketches[j].sketch),
            want
        );
    }
}

// ------------------------------------------------- To a point / a plane ---

#[test]
fn to_points_and_planes_parallel_to_the_sketch() {
    // A cylinder r 5 from the top face (z = 20) down to the target: Cut.
    let area = PI * 25.0;
    let base = 80.0 * 60.0 * 20.0;
    let mut part = block();
    let wp = part
        .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
            base: PlaneRef::Origin(OriginPlane::XY),
            distance: 7.0,
        })))
        .unwrap();
    let wpt = part
        .add_work(WorkFeature::new(WorkDef::Point(WorkPointDef::Fixed {
            x: 70.0,
            y: 50.0,
            z: 12.0,
        })))
        .unwrap();
    let cases = [
        (TargetRef::Point(PointRef::PartOrigin), 20.0),
        (TargetRef::Plane(PlaneRef::Origin(OriginPlane::XY)), 20.0),
        (TargetRef::Plane(PlaneRef::Work(wp.clone())), 13.0),
        (TargetRef::Point(PointRef::Work(wpt.clone())), 8.0),
        (TargetRef::Vertex("Box1:V000".into()), 20.0),
    ];
    for (t, depth) in cases {
        let mut p = part.clone();
        let (i, pr) = circle_sketch(
            &mut p,
            SketchPlacement::on_face("Box1:Top"),
            Point3::new(30.0, 30.0, 20.0),
            5.0,
        );
        let mut e = ExtrudeFeature::new(vec![pr], 1.0);
        e.extent = Extent::To(t.clone());
        e.operation = Operation::Cut;
        p.add_extrude(i, e).unwrap_or_else(|e| panic!("{t:?}: {e}"));
        close(exact_volume(&p), base - area * depth, &format!("{t:?}"));
        // Saved and loaded: the same.
        let json = serde_json::to_string(&p).unwrap();
        let mut back: PartDocument = serde_json::from_str(&json).unwrap();
        back.regenerate().unwrap();
        assert!(back.feature_health.is_empty(), "{:?}", back.feature_health);
        close(exact_volume(&back), exact_volume(&p), "reloaded");
    }
    // A plane that is perpendicular to the sketch never meets the
    // extrusion: refused, the part unchanged.
    let mut p = part.clone();
    let (i, pr) = circle_sketch(
        &mut p,
        SketchPlacement::on_face("Box1:Top"),
        Point3::new(30.0, 30.0, 20.0),
        5.0,
    );
    let before = serde_json::to_string(&p.features).unwrap();
    let mut e = ExtrudeFeature::new(vec![pr], 1.0);
    e.extent = Extent::To(TargetRef::Plane(PlaneRef::Origin(OriginPlane::YZ)));
    assert!(p.add_extrude(i, e).is_err());
    assert_eq!(serde_json::to_string(&p.features).unwrap(), before);
}

#[test]
fn to_an_inclined_plane() {
    // A tall box 80×60×40; a work plane through the Y axis, tilted from XY.
    let mut part = PartDocument::with_box("Box1", [80.0, 60.0, 40.0]).unwrap();
    let mut wp = String::new();
    let mut slope = 0.0;
    for angle in [30f64.to_radians(), -30f64.to_radians()] {
        let mut p = part.clone();
        let id = p
            .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Angle {
                base: PlaneRef::Origin(OriginPlane::XY),
                axis: super::LineRef::Origin(1),
                angle,
            })))
            .unwrap();
        let pl = &p.work.planes[&id];
        let n = pl.normal.into_inner();
        // z on the plane at x = 1 (y free): n·(x, y, z) = n·o.
        let z1 = (pl.origin.coords.dot(&n) - n.x) / n.z;
        if z1 > 0.0 {
            part = p;
            wp = id;
            slope = z1;
            break;
        }
    }
    assert!(slope > 0.5, "a plane rising with x");
    let base = 80.0 * 60.0 * 40.0;
    for (a, b) in [
        (10.0, 20.0),
        (30.0, 50.0),
        (5.0, 60.0),
        (60.0, 65.0),
        (0.5, 40.0),
    ] {
        let mut p = part.clone();
        let (i, pr) = rect_sketch(
            &mut p,
            SketchPlacement::on_face("Box1:Top"),
            Point3::new(a, 10.0, 40.0),
            Point3::new(b, 20.0, 40.0),
        );
        let mut e = ExtrudeFeature::new(vec![pr], 1.0);
        e.extent = Extent::To(TargetRef::Plane(PlaneRef::Work(wp.clone())));
        e.operation = Operation::Cut;
        p.add_extrude(i, e)
            .unwrap_or_else(|e| panic!("[{a}, {b}]: {e}"));
        // Removed: 10 · ∫ (40 − slope·x) dx.
        let removed = 10.0 * (40.0 * (b - a) - slope * (b * b - a * a) / 2.0);
        close(exact_volume(&p), base - removed, &format!("[{a}, {b}]"));
        assert!(
            p.body.find_face("Extrusion1:End").is_some(),
            "the termination face is the End"
        );
    }
    // A plane crossing the profile is refused.
    let mut p = part.clone();
    let (i, pr) = rect_sketch(
        &mut p,
        xy(),
        Point3::new(-10.0, 10.0, 0.0),
        Point3::new(10.0, 20.0, 0.0),
    );
    let mut e = ExtrudeFeature::new(vec![pr], 1.0);
    e.extent = Extent::To(TargetRef::Plane(PlaneRef::Work(wp)));
    e.operation = Operation::NewSolid;
    let err = p.add_extrude(i, e).unwrap_err().to_string();
    assert!(err.contains("crosses"), "{err}");
}

#[test]
fn to_an_inclined_face_of_a_wedge() {
    // A wedge: triangle (0,0) (60,0) (0,30) in XZ, 40 deep (y −40..0).
    let mut part = PartDocument::default();
    let i = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XZ), &opts())
        .unwrap();
    let sk = &mut part.sketches[i].sketch;
    let pts: Vec<_> = [(0.0, 0.0), (60.0, 0.0), (0.0, 30.0)]
        .iter()
        .map(|&(x, z)| {
            let q = sk.plane.project(&Point3::new(x, 0.0, z));
            sk.add_point(q)
        })
        .collect();
    for k in 0..3 {
        sk.add_line_between(pts[k], pts[(k + 1) % 3]).unwrap();
    }
    let r = sk.regions();
    let pr = ProfileRef::of(&r[0], r[0].interior_point());
    part.add_extrude(i, ExtrudeFeature::new(vec![pr], 40.0))
        .unwrap();
    let (lo, hi) = part.body.bounding_box().unwrap();
    let (y0, y1) = (lo.y, hi.y);
    let slope_face = part
        .body
        .faces()
        .find(|(f, _)| {
            part.body
                .face_plane(*f)
                .is_some_and(|p| p.normal.x.abs() > 0.1 && p.normal.z.abs() > 0.1)
        })
        .and_then(|(_, f)| f.name.clone())
        .expect("the sloped face");
    let wedge = 0.5 * 60.0 * 30.0 * 40.0;
    close(exact_volume(&part), wedge, "wedge");
    // Columns under the slope, up to it, as new solids.
    let ym = 0.5 * (y0 + y1);
    for (a, b) in [
        (10.0, 20.0),
        (0.5, 30.0),
        (40.0, 59.0),
        (25.0, 26.0),
        (5.0, 55.0),
    ] {
        let mut p = part.clone();
        let (j, pr) = rect_sketch(
            &mut p,
            xy(),
            Point3::new(a, ym - 5.0, 0.0),
            Point3::new(b, ym + 5.0, 0.0),
        );
        let mut e = ExtrudeFeature::new(vec![pr], 1.0);
        e.extent = Extent::To(TargetRef::Face(slope_face.clone()));
        e.operation = Operation::NewSolid;
        let id = p
            .add_extrude(j, e)
            .unwrap_or_else(|e| panic!("[{a}, {b}]: {e}"));
        // z = 30 (1 − x / 60): 10 · ∫ z dx.
        let want = 10.0 * 30.0 * ((b - a) - (b * b - a * a) / 120.0);
        close(body_volume(&p, &id), want, &format!("[{a}, {b}]"));
    }
}

// ------------------------------------------ Alternate Solution (curved) ---

#[test]
fn alternate_solution_on_a_cylinder() {
    // A bar r 10 along X, its axis at y = 0, z = 30.
    let mut part = PartDocument::default();
    let i = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::YZ), &opts())
        .unwrap();
    let sk = &mut part.sketches[i].sketch;
    let c = sk.plane.project(&Point3::new(0.0, 0.0, 30.0));
    sk.add_circle(c, 10.0).unwrap();
    let r = sk.regions();
    let pr = ProfileRef::of(&r[0], r[0].interior_point());
    let mut bar = ExtrudeFeature::new(vec![pr], 100.0);
    bar.direction = ExtentDirection::Symmetric;
    part.add_extrude(i, bar).unwrap();
    let face = part
        .body
        .faces()
        .find(|(f, _)| part.body.face_plane(*f).is_none())
        .and_then(|(_, f)| f.name.clone())
        .expect("the round face");
    // ∫_{-a}^{a} √(R² − y²) dy.
    let segment = |a: Real| a * (100.0 - a * a).sqrt() + 100.0 * (a / 10.0).asin();
    for (half_w, len) in [
        (4.0, 10.0),
        (2.0, 6.0),
        (8.0, 20.0),
        (9.5, 4.0),
        (0.5, 30.0),
    ] {
        for near in [false, true] {
            let mut p = part.clone();
            let (j, pr) = rect_sketch(
                &mut p,
                xy(),
                Point3::new(-len / 2.0, -half_w, 0.0),
                Point3::new(len / 2.0, half_w, 0.0),
            );
            let mut e = ExtrudeFeature::new(vec![pr], 1.0);
            e.extent = Extent::To(TargetRef::Face(face.clone()));
            e.alternate = near;
            e.operation = Operation::NewSolid;
            assert!(e.extent != Extent::Distance);
            let id = p
                .add_extrude(j, e)
                .unwrap_or_else(|e| panic!("w {half_w} near {near}: {e}"));
            let flat = 30.0 * 2.0 * half_w;
            let want = len
                * if near {
                    flat - segment(half_w)
                } else {
                    flat + segment(half_w)
                };
            close(
                body_volume(&p, &id),
                want,
                &format!("w {half_w} near {near}"),
            );
        }
    }
    // A profile wider than the bar misses it: refused.
    let mut p = part.clone();
    let (j, pr) = rect_sketch(
        &mut p,
        xy(),
        Point3::new(-5.0, -12.0, 0.0),
        Point3::new(5.0, 12.0, 0.0),
    );
    let mut e = ExtrudeFeature::new(vec![pr], 1.0);
    e.extent = Extent::To(TargetRef::Face(face));
    e.operation = Operation::NewSolid;
    assert!(p.add_extrude(j, e).is_err());
}

// --------------------------------------------- Extend face, Terminator ---

#[test]
fn extend_face_to_end_feature() {
    // A boss 10×10×10 on the top face; a larger profile on XY, To the
    // boss's top: only with Extend face.
    let mut part = block();
    let (i, pr) = rect_sketch(
        &mut part,
        SketchPlacement::on_face("Box1:Top"),
        Point3::new(30.0, 30.0, 20.0),
        Point3::new(40.0, 40.0, 20.0),
    );
    part.add_extrude(i, ExtrudeFeature::new(vec![pr], 10.0))
        .unwrap();
    let base = exact_volume(&part);
    for (size, covered) in [
        (4.0, true),
        (9.0, true),
        (12.0, false),
        (30.0, false),
        (2.0, true),
    ] {
        let mut p = part.clone();
        let (j, pr) = rect_sketch(
            &mut p,
            SketchPlacement::on_origin_plane(OriginPlane::XY),
            Point3::new(35.0 - size / 2.0, 35.0 - size / 2.0, 0.0),
            Point3::new(35.0 + size / 2.0, 35.0 + size / 2.0, 0.0),
        );
        let mut e = ExtrudeFeature::new(vec![pr], 1.0);
        e.extent = Extent::To(TargetRef::Face("Extrusion1:End".into()));
        e.operation = Operation::NewSolid;
        // The command's start state (the API default extends, as old files).
        e.extend_face = false;
        assert_eq!(
            e.needs_extended_face(&p.body, &p.work, &p.sketches[j].sketch),
            !covered,
            "size {size}"
        );
        let before = serde_json::to_string(&p.features).unwrap();
        let r = p.add_extrude(j, e.clone());
        if covered {
            r.unwrap();
        } else {
            let msg = r.unwrap_err().to_string();
            assert!(msg.contains("Extend face"), "{msg}");
            assert_eq!(serde_json::to_string(&p.features).unwrap(), before);
            e.extend_face = true;
            p.add_extrude(j, e).unwrap();
        }
        close(
            exact_volume(&p) - base,
            size * size * 30.0,
            &format!("size {size}"),
        );
    }
}

#[test]
fn to_next_with_a_terminator() {
    // Box1 (z 0..20) and two slabs above it: A (z 30..35), B (z 50..55).
    let mut part = block();
    let slab = |part: &mut PartDocument, z: Real| {
        let wp = part
            .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
                base: PlaneRef::Origin(OriginPlane::XY),
                distance: z,
            })))
            .unwrap();
        let (i, pr) = rect_sketch(
            part,
            SketchPlacement {
                plane: PlaneRef::Work(wp),
                ..Default::default()
            },
            Point3::new(0.0, 0.0, z),
            Point3::new(80.0, 60.0, z),
        );
        let mut e = ExtrudeFeature::new(vec![pr], 5.0);
        e.operation = Operation::NewSolid;
        part.add_extrude(i, e).unwrap()
    };
    let a = slab(&mut part, 30.0);
    let b = slab(&mut part, 50.0);
    for (term, height) in [
        (None, Some(10.0)),
        (Some(a.clone()), Some(10.0)),
        (Some(b.clone()), Some(30.0)),
        (Some("Box1".to_owned()), None),
        (Some("Gone".to_owned()), None),
    ] {
        let mut p = part.clone();
        let (i, pr) = circle_sketch(
            &mut p,
            SketchPlacement::on_face("Box1:Top"),
            Point3::new(20.0, 20.0, 20.0),
            4.0,
        );
        let mut e = ExtrudeFeature::new(vec![pr], 1.0);
        e.extent = Extent::ToNext;
        e.terminator = term.clone();
        e.operation = Operation::NewSolid;
        match (p.add_extrude(i, e), height) {
            (Ok(id), Some(h)) => close(body_volume(&p, &id), PI * 16.0 * h, &format!("{term:?}")),
            (Err(_), None) => {}
            (r, h) => panic!("{term:?}: {:?} (expected {h:?})", r.map(|_| ())),
        }
    }
}

// --------------------------------------------------------- Match Shape ---

/// A cut along an open chain from the bottom through the 80×60×20 block:
/// the two sides remove complementary parts.
#[test]
fn match_shape_cuts_with_open_profiles() {
    let base = 80.0 * 60.0 * 20.0;
    type Chain = Vec<(Real, Real)>;
    let cases: Vec<(Chain, Option<Real>)> = vec![
        // A vertical line beyond the block on both ends: 20 / 60 of it.
        (vec![(20.0, -5.0), (20.0, 65.0)], Some(20.0 * 60.0 * 20.0)),
        // Ending inside the footprint: its ends are extended.
        (vec![(50.0, 10.0), (50.0, 50.0)], Some(50.0 * 60.0 * 20.0)),
        // A horizontal line.
        (vec![(-5.0, 15.0), (85.0, 15.0)], Some(80.0 * 15.0 * 20.0)),
        // The diagonal: half.
        (vec![(0.0, 0.0), (80.0, 60.0)], Some(base / 2.0)),
        // A polyline (two lines): sides add up to the block.
        (vec![(10.0, -5.0), (30.0, 30.0), (10.0, 65.0)], None),
    ];
    for (k, (chain, side)) in cases.into_iter().enumerate() {
        let mut got = Vec::new();
        for flip in [false, true] {
            let mut p = block();
            let i = p.create_sketch(xy(), &opts()).unwrap();
            let sk = &mut p.sketches[i].sketch;
            let pts: Vec<_> = chain
                .iter()
                .map(|&(x, y)| sk.add_point(Point2::new(x, y)))
                .collect();
            let curves: Vec<_> = pts
                .windows(2)
                .map(|w| sk.add_line_between(w[0], w[1]).unwrap())
                .collect();
            let mut e = ExtrudeFeature::new(Vec::new(), 1.0);
            e.open = Some(OpenProfile {
                curves,
                flip,
                match_shape: true,
            });
            e.extent = Extent::ThroughAll;
            e.operation = Operation::Cut;
            p.add_extrude(i, e)
                .unwrap_or_else(|e| panic!("case {k} flip {flip}: {e}"));
            got.push(base - exact_volume(&p));
        }
        close(got[0] + got[1], base, &format!("case {k}: both sides"));
        if let Some(s) = side {
            assert!(
                (got[0] - s).abs() < 1e-3 * base || (got[1] - s).abs() < 1e-3 * base,
                "case {k}: removed {got:?}, one side should be {s}"
            );
        }
    }
}

/// A gusset in an L: Join with Match Shape stops at the body's box, without
/// it the full distance; no body: refused.
#[test]
fn match_shape_join_fills_an_l() {
    let l_part = || {
        let mut part = PartDocument::default();
        let i = part.create_sketch(xy(), &opts()).unwrap();
        let sk = &mut part.sketches[i].sketch;
        let pts: Vec<_> = [
            (0.0, 0.0),
            (40.0, 0.0),
            (40.0, 10.0),
            (10.0, 10.0),
            (10.0, 40.0),
            (0.0, 40.0),
        ]
        .iter()
        .map(|&(x, y)| sk.add_point(Point2::new(x, y)))
        .collect();
        for k in 0..6 {
            sk.add_line_between(pts[k], pts[(k + 1) % 6]).unwrap();
        }
        let r = sk.regions();
        let pr = ProfileRef::of(&r[0], r[0].interior_point());
        part.add_extrude(i, ExtrudeFeature::new(vec![pr], 5.0))
            .unwrap();
        part
    };
    let l_area = 700.0;
    close(exact_volume(&l_part()), l_area * 5.0, "L");
    // The line x + y = 45 from (40, 5) to (5, 40).
    let gusset = |part: &mut PartDocument, flip: bool, match_shape: bool, d: Real| {
        let i = part.create_sketch(xy(), &opts()).unwrap();
        let sk = &mut part.sketches[i].sketch;
        let l = sk.add_line(Point2::new(40.0, 5.0), Point2::new(5.0, 40.0));
        let mut e = ExtrudeFeature::new(Vec::new(), d);
        e.open = Some(OpenProfile {
            curves: vec![l],
            flip,
            match_shape,
        });
        e.operation = Operation::Join;
        part.add_extrude(i, e)
    };
    // Corner side: x + y ≤ 45 inside the 40×40 box (987.5) plus the L's
    // tips beyond the line (2 × 12.5).
    let mut found = None;
    for flip in [false, true] {
        let mut p = l_part();
        gusset(&mut p, flip, true, 5.0).unwrap();
        let v = exact_volume(&p);
        if (v - 1012.5 * 5.0).abs() < 1.0 {
            found = Some(flip);
        }
    }
    let flip = found.expect("one side fills the corner");
    // Without Match Shape the side goes the full 8 mm.
    let mut p = l_part();
    gusset(&mut p, flip, false, 8.0).unwrap();
    close(exact_volume(&p), 987.5 * 8.0 + 25.0 * 5.0, "full distance");
    // With it, the fill stops at the body's box (5 mm).
    let mut p = l_part();
    gusset(&mut p, flip, true, 8.0).unwrap();
    close(exact_volume(&p), 1012.5 * 5.0, "flooded");
    // No body: an open profile cannot be the first feature.
    let mut p = PartDocument::default();
    let err = gusset(&mut p, flip, true, 5.0).unwrap_err().to_string();
    assert!(err.contains("open profile"), "{err}");
    assert!(p.features.is_empty());
    // Saved and loaded.
    let mut p = l_part();
    gusset(&mut p, flip, true, 5.0).unwrap();
    let json = serde_json::to_string(&p).unwrap();
    let mut back: PartDocument = serde_json::from_str(&json).unwrap();
    back.regenerate().unwrap();
    close(exact_volume(&back), exact_volume(&p), "reloaded");
}

// ------------------------------------------- Revolve To, Minimum Solution ---

/// A ring section x 10..20, y 0..10 on XY, revolved about Y.
fn ring() -> (PartDocument, usize, ProfileRef) {
    let mut part = PartDocument::default();
    let i = part.create_sketch(xy(), &opts()).unwrap();
    let sk = &mut part.sketches[i].sketch;
    sk.add_rectangle_two_point(Point2::new(10.0, 0.0), Point2::new(20.0, 10.0))
        .unwrap();
    let r = sk.regions();
    (part, i, ProfileRef::of(&r[0], r[0].interior_point()))
}

#[test]
fn revolve_to_with_minimum_solution() {
    let full = PI * (400.0 - 100.0) * 10.0;
    let base = {
        let (mut p, _, _) = ring();
        let wp = p
            .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Angle {
                base: PlaneRef::Origin(OriginPlane::XY),
                axis: super::LineRef::Origin(1),
                angle: 45f64.to_radians(),
            })))
            .unwrap();
        let pt = p
            .add_work(WorkFeature::new(WorkDef::Point(WorkPointDef::Fixed {
                x: 0.0,
                y: 5.0,
                z: -15.0,
            })))
            .unwrap();
        (p, wp, pt)
    };
    let (part, wp, pt) = base;
    // (target, minimum, flipped) → fraction of a full turn.
    let cases: Vec<(TargetRef, bool, bool, Vec<Real>)> = vec![
        // YZ contains the axis: half-planes at 90° and 270°.
        (
            TargetRef::Plane(PlaneRef::Origin(OriginPlane::YZ)),
            true,
            false,
            vec![0.25],
        ),
        (
            TargetRef::Plane(PlaneRef::Origin(OriginPlane::YZ)),
            false,
            false,
            vec![0.75],
        ),
        (
            TargetRef::Plane(PlaneRef::Origin(OriginPlane::YZ)),
            true,
            true,
            vec![0.25],
        ),
        // The 45° work plane: 45° / 225° or 135° / 315°.
        (
            TargetRef::Plane(PlaneRef::Work(wp.clone())),
            true,
            false,
            vec![0.125, 0.375],
        ),
        (
            TargetRef::Plane(PlaneRef::Work(wp.clone())),
            false,
            false,
            vec![0.625, 0.875],
        ),
        // A point: one half-plane, Minimum Solution does not matter.
        (
            TargetRef::Point(PointRef::Work(pt.clone())),
            false,
            false,
            vec![0.25, 0.75],
        ),
    ];
    for (t, minimum, flipped, fractions) in cases {
        let mut p = part.clone();
        let i = 0;
        let (_, _, pr) = ring();
        let mut r = RevolveFeature::new(vec![pr], AxisRef::Origin(1));
        r.extent = RevolveExtent::To;
        r.to = Some(t.clone());
        r.minimum = minimum;
        if flipped {
            r.direction = ExtentDirection::Flipped;
        }
        p.add_revolve(i, r)
            .unwrap_or_else(|e| panic!("{t:?} {minimum}: {e}"));
        let v = exact_volume(&p);
        assert!(
            fractions.iter().any(|f| (v - f * full).abs() < 3e-4 * full),
            "{t:?} min {minimum} flip {flipped}: {} turns",
            v / full
        );
    }
    // The point case: the material really ends at the point's side; the
    // two directions add up to a full turn.
    let turn = |flipped: bool| {
        let mut p = part.clone();
        let (_, _, pr) = ring();
        let mut r = RevolveFeature::new(vec![pr], AxisRef::Origin(1));
        r.extent = RevolveExtent::To;
        r.to = Some(TargetRef::Point(PointRef::Work(pt.clone())));
        if flipped {
            r.direction = ExtentDirection::Flipped;
        }
        p.add_revolve(0, r).unwrap();
        (exact_volume(&p), p)
    };
    let (a, pa) = turn(false);
    let (b, _) = turn(true);
    close(a + b, full, "both directions");
    // Just before the point's half-plane (−Z) there is material, just past
    // it there is none.
    let near = |deg: Real| {
        let t = deg.to_radians();
        // From −Z towards the start (+X) or away from it.
        Point3::new(15.0 * t.sin(), 5.0, -15.0 * t.cos())
    };
    let inside = [near(5.0), near(-5.0)].map(|q| pa.body.contains_point(&q));
    {
        let (mut p, i, pr) = ring();
        let mut r = RevolveFeature::new(vec![pr], AxisRef::Origin(1));
        r.extent = RevolveExtent::Angle;
        r.angle = PI / 2.0;
        r.direction = ExtentDirection::Symmetric;
        p.add_revolve(i, r).unwrap();
        let at = |deg: Real| {
            let t = deg.to_radians();
            p.body
                .contains_point(&Point3::new(15.0 * t.cos(), 5.0, 15.0 * t.sin()))
        };
        // Regression (found here): the exact kernel's Symmetric revolve
        // turned its start the wrong way and covered 45°..135°.
        assert!(
            at(30.0) && at(-30.0) && !at(60.0) && !at(-60.0),
            "symmetric 90°: ±45° around the sketch"
        );
    }
    assert!(
        inside[0] != inside[1],
        "the revolve ends at the point: {inside:?}"
    );
    // A plane that misses the axis is refused, the part unchanged.
    let mut p = part.clone();
    let (_, _, pr) = ring();
    let mut r = RevolveFeature::new(vec![pr], AxisRef::Origin(1));
    r.extent = RevolveExtent::To;
    r.to = Some(TargetRef::Plane(PlaneRef::Origin(OriginPlane::XZ)));
    let before = serde_json::to_string(&p.features).unwrap();
    assert!(p.add_revolve(0, r).is_err());
    assert_eq!(serde_json::to_string(&p.features).unwrap(), before);
}
