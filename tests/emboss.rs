//! Emboss / Engrave (spec 2.17): the expected values are Inventor's behaviour on the same cases.

use cadcraft3d::kernel::{Point3, Real, Surface};
use cadcraft3d::part::emboss::{EmbossKind, EmbossOp, EmbossSide};
use cadcraft3d::part::work::WorkPlaneDef;
use cadcraft3d::part::{
    BodyOp, BodyOpFeature, ExtrudeFeature, FaceRef, OriginPlane, PartDocument, PlaneRef,
    ProfileRef, SketchOptions, SketchPlacement, WorkDef, WorkFeature,
};
use nalgebra::Point2;

const PLAIN: fn() -> SketchOptions = || SketchOptions {
    auto_project_origin: false,
    auto_reference_edges: false,
    ..Default::default()
};

fn volume(p: &PartDocument) -> Real {
    p.exact_bodies
        .0
        .iter()
        .filter(|s| !s.is_open())
        .filter_map(|s| s.volume_at(1e-3))
        .sum()
}

fn plane_at(p: &mut PartDocument, base: OriginPlane, distance: Real) -> usize {
    let wp = p
        .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
            base: PlaneRef::Origin(base),
            distance,
        })))
        .unwrap_or_else(|e| panic!("{e}"));
    p.create_sketch(
        SketchPlacement {
            plane: PlaneRef::Work(wp),
            ..Default::default()
        },
        &PLAIN(),
    )
    .unwrap_or_else(|e| panic!("{e}"))
}

/// A 100 x 60 x 20 box (120000 mm³) and a sketch at height `z`: a 20 x 10 rectangle.
fn plate(z: Real) -> (PartDocument, String) {
    let mut p =
        PartDocument::with_box("Box1", [100.0, 60.0, 20.0]).unwrap_or_else(|e| panic!("{e}"));
    let i = plane_at(&mut p, OriginPlane::XY, z);
    p.sketches[i]
        .sketch
        .add_rectangle_two_point(Point2::new(40.0, 25.0), Point2::new(60.0, 35.0))
        .unwrap_or_else(|e| panic!("{e}"));
    let id = p.sketches[i].id.clone();
    (p, id)
}

/// A cylinder Ø40 x 60 about Z (75398.2237 mm³).
fn cylinder() -> PartDocument {
    let mut p = PartDocument::default();
    let i = p
        .create_sketch(
            SketchPlacement {
                plane: PlaneRef::Origin(OriginPlane::XY),
                ..Default::default()
            },
            &PLAIN(),
        )
        .unwrap_or_else(|e| panic!("{e}"));
    let sk = &mut p.sketches[i].sketch;
    sk.add_circle(Point2::new(0.0, 0.0), 20.0)
        .unwrap_or_else(|e| panic!("{e}"));
    let r: Vec<ProfileRef> = sk
        .regions()
        .iter()
        .map(|r| ProfileRef::of(r, r.interior_point()))
        .collect();
    let mut ex = ExtrudeFeature::new(r, 60.0);
    ex.operation = cadcraft3d::part::Operation::NewSolid;
    p.add_extrude(i, ex).unwrap_or_else(|e| panic!("{e}"));
    p
}

/// A plane parallel to XZ at `y`, with a rectangle `half` to each side of the axis, z 20..40.
fn side_sketch(p: &mut PartDocument, y: Real, half: Real) -> String {
    let i = plane_at(p, OriginPlane::XZ, -y);
    let plane = p.sketches[i].sketch.plane.clone();
    let uv = |x: Real, z: Real| plane.project(&Point3::new(x, y, z));
    p.sketches[i]
        .sketch
        .add_rectangle_two_point(uv(-half, 20.0), uv(half, 40.0))
        .unwrap_or_else(|e| panic!("{e}"));
    p.sketches[i].id.clone()
}

/// The cylinder's wall, with a point on it.
fn wall_of(p: &PartDocument, at: [Real; 3]) -> FaceRef {
    let name = p
        .body
        .faces()
        .find(|(_, f)| matches!(p.body.surface(f.surface), Some(Surface::Cylinder(_))))
        .and_then(|(_, f)| f.name.clone())
        .unwrap_or_else(|| panic!("no wall"));
    FaceRef {
        face: name,
        hint: at,
    }
}

#[test]
fn from_face_raises_or_recesses_the_face_it_reaches() {
    use EmbossKind::{EmbossFromFace as E, EngraveFromFace as G};
    use EmbossSide::{Direction1 as D1, Direction2 as D2};
    for (kind, z, side, expected) in [
        (E, 20.0, D1, 120400.0),
        (E, 20.0, D2, 120400.0),
        (E, 25.0, D2, 120400.0),
        (E, 10.0, D1, 120400.0),
        (E, 10.0, D2, 120400.0),
        (G, 20.0, D1, 119600.0),
        (G, 20.0, D2, 119600.0),
        (G, 25.0, D2, 119600.0),
        (G, 10.0, D1, 119600.0),
        (G, 10.0, D2, 119600.0),
    ] {
        let (mut p, id) = plate(z);
        let mut op = EmbossOp::new(id, 2.0);
        (op.style, op.side) = (kind, side);
        p.add_body_op(BodyOpFeature::new(BodyOp::Emboss(op)))
            .unwrap_or_else(|e| panic!("{kind:?} z={z} {side:?}: {e}"));
        let v = volume(&p);
        assert!((v - expected).abs() < 1e-2, "{kind:?} z={z} {side:?}: {v}");
        // The sketch is consumed, as an Extrude's.
        assert!(!p.sketches.iter().any(|s| s.visible), "{kind:?} z={z}");
    }
}

#[test]
fn a_direction_that_misses_or_a_profile_over_the_edge_changes_nothing() {
    // Over the box, pointing away from it.
    let (mut p, id) = plate(25.0);
    let mut op = EmbossOp::new(id, 2.0);
    op.side = EmbossSide::Direction1;
    let n = p.features.len();
    assert!(
        p.add_body_op(BodyOpFeature::new(BodyOp::Emboss(op)))
            .is_err()
    );
    assert_eq!(p.features.len(), n);
    // Running past the top face's edge.
    let mut p =
        PartDocument::with_box("Box1", [100.0, 60.0, 20.0]).unwrap_or_else(|e| panic!("{e}"));
    let i = plane_at(&mut p, OriginPlane::XY, 20.0);
    p.sketches[i]
        .sketch
        .add_rectangle_two_point(Point2::new(90.0, 25.0), Point2::new(110.0, 35.0))
        .unwrap_or_else(|e| panic!("{e}"));
    let op = EmbossOp::new(p.sketches[i].id.clone(), 2.0);
    let n = p.features.len();
    assert!(
        p.add_body_op(BodyOpFeature::new(BodyOp::Emboss(op)))
            .is_err()
    );
    assert_eq!(p.features.len(), n);
}

#[test]
fn from_plane_adds_outside_and_removes_inside() {
    use EmbossSide::{Direction1 as D1, Direction2 as D2, Symmetric as S};
    for (z, side, taper, expected) in [
        (25.0, D2, 0.0, 121000.0),
        (25.0, S, 0.0, 121000.0),
        (22.0, D2, 0.0, 120400.0),
        (22.0, S, 0.0, 120400.0),
        (10.0, D1, 0.0, 118000.0),
        (10.0, D2, 0.0, 118000.0),
        (10.0, S, 0.0, 116000.0),
        (25.0, D2, 10.0, 121137.4271),
        (25.0, D2, -10.0, 120872.9366),
        (22.0, D2, 10.0, 120421.4909),
        (22.0, D2, -10.0, 120379.1724),
        (10.0, D1, 10.0, 117429.5641),
        (10.0, D1, -10.0, 118487.5260),
        (10.0, S, 10.0, 114859.1282),
        (10.0, S, -10.0, 116975.0520),
    ] {
        let (mut p, id) = plate(z);
        let mut op = EmbossOp::new(id, 0.0);
        op.style = EmbossKind::FromPlane;
        op.side = side;
        op.taper = Real::to_radians(taper);
        p.add_body_op(BodyOpFeature::new(BodyOp::Emboss(op)))
            .unwrap_or_else(|e| panic!("z={z} {side:?} {taper}: {e}"));
        let v = volume(&p);
        assert!(
            (v - expected).abs() < 0.05,
            "z={z} {side:?} taper {taper}: {v}"
        );
    }
}

#[test]
fn from_plane_on_the_face_or_pointing_away_is_refused() {
    for (z, side) in [
        (20.0, EmbossSide::Direction1),
        (20.0, EmbossSide::Direction2),
        (25.0, EmbossSide::Direction1),
    ] {
        let (mut p, id) = plate(z);
        let mut op = EmbossOp::new(id, 0.0);
        op.style = EmbossKind::FromPlane;
        op.side = side;
        assert!(
            p.add_body_op(BodyOpFeature::new(BodyOp::Emboss(op)))
                .is_err(),
            "z={z} {side:?}"
        );
    }
}

#[test]
fn a_curved_face_gets_an_offset_top_and_walls_along_the_direction() {
    // Inventor: Ø40 x 60, profile 10 x 20 seen along the sketch's normal, depth 2.
    for (kind, expected) in [
        (EmbossKind::EmbossFromFace, 75802.1121),
        (EmbossKind::EngraveFromFace, 74993.4425),
    ] {
        let mut p = cylinder();
        let id = side_sketch(&mut p, 25.0, 5.0);
        let mut op = EmbossOp::new(id, 2.0);
        op.style = kind;
        // Towards the cylinder, whichever way the sketch's normal points.
        let n = p.sketches.last().map_or(1.0, |s| s.sketch.plane.normal.y);
        op.side = if n > 0.0 {
            EmbossSide::Direction2
        } else {
            EmbossSide::Direction1
        };
        p.add_body_op(BodyOpFeature::new(BodyOp::Emboss(op)))
            .unwrap_or_else(|e| panic!("{kind:?}: {e}"));
        // Measured on the display mesh, like the bare cylinder: compare what changed.
        let v = volume(&p) - volume(&cylinder());
        let expected = expected - 75398.2237;
        assert!((v - expected).abs() < 0.5, "{kind:?}: {v} vs {expected}");
    }
}

#[test]
fn wrap_to_face_rolls_the_profile_round_the_cylinder() {
    // Inventor: the arc length equals the sketch's width, the walls are radial (420, 380, 1260
    // mm³ are the exact ring sectors).
    for (kind, half, expected) in [
        (EmbossKind::EmbossFromFace, 5.0, 75818.2237),
        (EmbossKind::EngraveFromFace, 5.0, 75018.2237),
        (EmbossKind::EmbossFromFace, 15.0, 76658.2237),
    ] {
        let mut p = cylinder();
        let id = side_sketch(&mut p, 25.0, half);
        let mut op = EmbossOp::new(id, 2.0);
        op.style = kind;
        op.wrap = Some(wall_of(&p, [0.0, 20.0, 30.0]));
        p.add_body_op(BodyOpFeature::new(BodyOp::Emboss(op)))
            .unwrap_or_else(|e| panic!("{kind:?} {half}: {e}"));
        let v = volume(&p) - volume(&cylinder());
        let expected = expected - 75398.2237;
        assert!(
            (v - expected).abs() < 0.5,
            "{kind:?} {half}: {v} vs {expected}"
        );
    }
}

#[test]
fn survives_a_rebuild_and_a_save() {
    let (mut p, id) = plate(20.0);
    p.add_body_op(BodyOpFeature::new(BodyOp::Emboss(EmbossOp::new(id, 2.0))))
        .unwrap_or_else(|e| panic!("{e}"));
    let json = serde_json::to_string(&p).unwrap_or_else(|e| panic!("{e}"));
    let mut q: PartDocument = serde_json::from_str(&json).unwrap_or_else(|e| panic!("{e}"));
    q.regenerate().unwrap_or_else(|e| panic!("{e}"));
    assert!((volume(&q) - 120400.0).abs() < 1e-2);
    assert!(q.feature_health.is_empty());
}
