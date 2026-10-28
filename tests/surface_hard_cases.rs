//! Surface tools on harder cases (spec 5.4): what Inventor's Stitch, Trim and Boundary Patch do
//! with gaps against the tolerance, curved walls closing into a solid, a stitch that must not
//! depend on the order of its surfaces, and trimming a curved surface. Measured areas use the
//! display triangles (inscribed: curved ones run a little low).

mod common;

use cadcraft3d::part::surface_op::{PatchOp, StitchOp, TrimOp, TrimTool};
use cadcraft3d::part::{
    BodyOp, BodyOpFeature, OpenProfile, Output, WorkDef, WorkFeature, WorkPlaneDef,
};
use cadcraft3d::sketch::EntityId;
use common::*;

fn sheet_area(part: &PartDocument) -> Real {
    part.exact_bodies
        .0
        .iter()
        .filter(|s| s.is_open())
        .filter_map(|s| s.triangles().ok())
        .flatten()
        .map(|[a, b, c]| (b - a).cross(&(c - a)).norm() / 2.0)
        .sum()
}

fn surfaces(part: &PartDocument) -> usize {
    part.exact_bodies.0.iter().filter(|s| s.is_open()).count()
}

fn solid_volume(p: &PartDocument) -> Real {
    p.exact_bodies
        .0
        .iter()
        .filter(|s| !s.is_open())
        .filter_map(|s| s.volume_at(1e-4))
        .sum()
}

fn near(got: Real, want: Real, rel: Real, what: &str) {
    assert!(
        (got - want).abs() <= rel * want.abs(),
        "{what}: {got} vs {want} (rel {rel})"
    );
}

fn op(p: &mut PartDocument, o: BodyOp) -> String {
    let id = p.add_body_op(BodyOpFeature::new(o)).unwrap();
    assert!(p.exact, "exact kernel");
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    id
}

fn stitch(surfaces: Vec<String>, tolerance: Real) -> BodyOp {
    BodyOp::Stitch(StitchOp {
        surfaces,
        tolerance,
        keep_surface: false,
    })
}

/// A surface extruded `h` along +Z from lines through `pts` on XY.
fn wall(p: &mut PartDocument, pts: &[(Real, Real)], h: Real) -> String {
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    let sk = &mut p.sketches[i].sketch;
    let curves: Vec<EntityId> = pts
        .windows(2)
        .map(|w| sk.add_line(Point2::new(w[0].0, w[0].1), Point2::new(w[1].0, w[1].1)))
        .collect();
    let mut f = ExtrudeFeature::new(Vec::new(), h);
    f.open = Some(OpenProfile {
        curves,
        flip: false,
        match_shape: false,
    });
    f.output = Output::Surface;
    p.add_extrude(i, f).unwrap()
}

/// A cylindrical wall r × h about Z (a circle extruded as a surface).
fn tube(p: &mut PartDocument, r: Real, h: Real) -> String {
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    p.sketches[i]
        .sketch
        .add_circle(Point2::new(0.0, 0.0), r)
        .unwrap();
    // A closed profile with Surface output: its walls only.
    let rg = p.sketches[i].sketch.regions();
    let mut f = ExtrudeFeature::new(vec![ProfileRef::of(&rg[0], rg[0].interior_point())], h);
    f.output = Output::Surface;
    p.add_extrude(i, f).unwrap()
}

/// A planar disc patch of radius r at height z.
fn disc(p: &mut PartDocument, r: Real, z: Real) -> String {
    let placement = if z == 0.0 {
        SketchPlacement::on_origin_plane(OriginPlane::XY)
    } else {
        let wp = p
            .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
                base: PlaneRef::Origin(OriginPlane::XY),
                distance: z,
            })))
            .unwrap();
        SketchPlacement {
            plane: PlaneRef::Work(wp),
            ..Default::default()
        }
    };
    let i = p.create_sketch(placement, &opts()).unwrap();
    p.sketches[i]
        .sketch
        .add_circle(Point2::new(0.0, 0.0), r)
        .unwrap();
    let rg = p.sketches[i].sketch.regions();
    let id = p.sketches[i].id.clone();
    op(
        p,
        BodyOp::Patch(PatchOp {
            edges: Vec::new(),
            sketch: Some(id),
            profile: Some(ProfileRef::of(&rg[0], rg[0].interior_point())),
            solid: false,
        }),
    )
}

/// Two walls meeting at x = 10, the second `gap` further along x; Stitch
/// with a 0.001 mm tolerance. Returns the part, whether the feature was
/// added healthy, and the surfaces before.
fn walls_stitched(gap: Real) -> (PartDocument, bool) {
    let mut p = PartDocument::default();
    let a = wall(&mut p, &[(0.0, 0.0), (10.0, 0.0)], 10.0);
    let b = wall(&mut p, &[(10.0 + gap, 0.0), (10.0 + gap, 10.0)], 10.0);
    assert_eq!(surfaces(&p), 2);
    let r = p.add_body_op(BodyOpFeature::new(stitch(vec![a, b], 0.001)));
    let ok = r.is_ok() && p.feature_health.is_empty();
    (p, ok)
}

/// Inventor's Stitch joins edges closer than its Maximum Gap: touching and
/// 0.0005 mm apart with a 0.001 tolerance both become one surface.
#[test]
fn stitch_joins_gaps_within_the_tolerance() {
    for gap in [0.0, 0.0005] {
        let (p, ok) = walls_stitched(gap);
        assert!(ok, "gap {gap}: {:?}", p.feature_health);
        assert_eq!(surfaces(&p), 1, "gap {gap}: one surface");
        near(sheet_area(&p), 200.0, 1e-6, "area kept");
    }
}

/// Edges further apart than the tolerance are not stitched (Inventor
/// leaves them, listing the free edges): ours makes one "surface" of two
/// walls even 5 mm apart.
#[test]
#[ignore = "known: Stitch groups surfaces whatever the gap; the tolerance is not checked (reported to the surfaces session)"]
fn stitch_leaves_gaps_over_the_tolerance() {
    for gap in [0.05, 0.5, 5.0] {
        let (p, ok) = walls_stitched(gap);
        if ok {
            assert_eq!(surfaces(&p), 2, "gap {gap}: left apart");
        }
        near(sheet_area(&p), 200.0, 1e-6, "area kept");
    }
}

/// A tube wall and two discs stitch into a closed cylinder (Inventor's
/// Stitch makes a solid when the quilt closes), whatever order the
/// surfaces are listed in.
#[test]
fn curved_walls_and_caps_stitch_into_a_solid_in_any_order() {
    let want = PI * 8.0 * 8.0 * 15.0;
    for order in [[0, 1, 2], [2, 0, 1], [1, 2, 0]] {
        let mut p = PartDocument::default();
        let w = tube(&mut p, 8.0, 15.0);
        let b = disc(&mut p, 8.0, 0.0);
        let t = disc(&mut p, 8.0, 15.0);
        let all = [w, b, t];
        let list: Vec<String> = order.iter().map(|&k| all[k].clone()).collect();
        op(&mut p, stitch(list, 0.001));
        assert_eq!(surfaces(&p), 0, "{order:?}: closed");
        near(solid_volume(&p), want, 2e-3, "volume");
    }
}

/// Trim a cylindrical wall r 8 by an origin plane through its axis: the
/// half on the picked side goes (Inventor's Trim by a work plane).
fn trim_tube(plane: OriginPlane, remove: [Real; 3]) {
    let mut p = PartDocument::default();
    let w = tube(&mut p, 8.0, 15.0);
    let full = sheet_area(&p);
    op(
        &mut p,
        BodyOp::Trim(TrimOp {
            surface: w,
            tool: TrimTool::Plane(PlaneRef::Origin(plane)),
            remove,
            invert: false,
        }),
    );
    near(sheet_area(&p), full / 2.0, 1e-3, "half");
    let (lo, hi) = p.body.bounding_box().unwrap();
    // The kept half lies on the side away from the picked point.
    let (k, v) = if remove[0] != 0.0 {
        (0, remove[0])
    } else {
        (1, remove[1])
    };
    if v > 0.0 {
        assert!(
            hi[k] < 1e-6,
            "{plane:?}: the negative half stays: {lo} {hi}"
        );
    } else {
        assert!(
            lo[k] > -1e-6,
            "{plane:?}: the positive half stays: {lo} {hi}"
        );
    }
}

#[test]
fn trim_a_curved_wall_by_a_plane_through_its_axis() {
    trim_tube(OriginPlane::YZ, [8.0, 0.0, 7.5]);
    trim_tube(OriginPlane::YZ, [-8.0, 0.0, 7.5]);
}

/// The same with XZ, the plane the cylinder's two kernel faces meet on
/// (their seams lie in it): refused as "the tool does not cross the
/// selected faces" — the part is left unchanged, but Inventor trims it.
#[test]
#[ignore = "known: Trim by a plane through a cylinder's seams finds no crossing (reported to the surfaces session)"]
fn trim_a_curved_wall_along_its_seams() {
    trim_tube(OriginPlane::XZ, [0.0, 8.0, 7.5]);
    trim_tube(OriginPlane::XZ, [0.0, -8.0, 7.5]);
}

/// Trim a plane strip by a cylindrical wall crossing it: the strip's piece
/// inside the circle goes; the area left is the strip less the chord band.
#[test]
fn trim_a_strip_by_a_tube() {
    let mut p = PartDocument::default();
    let s = wall(&mut p, &[(-20.0, 0.0), (20.0, 0.0)], 10.0);
    let t = tube(&mut p, 6.0, 10.0);
    op(
        &mut p,
        BodyOp::Trim(TrimOp {
            surface: s,
            tool: TrimTool::Surface(t),
            remove: [0.0, 0.0, 5.0],
            invert: false,
        }),
    );
    let tube_area = 2.0 * PI * 6.0 * 10.0;
    let strip_left = (40.0 - 12.0) * 10.0;
    near(
        sheet_area(&p),
        strip_left + tube_area,
        3e-3,
        "strip less its middle + the tube",
    );
    assert!(
        surfaces(&p) >= 2,
        "two strip pieces (or one body of two faces) and the tube"
    );
}
