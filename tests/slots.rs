//! Slot (`docs/spec/2.3-hole.md` §3a, Inventor 2027's Slot feature): slots
//! on a 100 × 60 × 20 plate, a centre at (50, 30) on the top. The removed
//! volumes are Inventor's for the same cases: width 10, overall length 30.

use std::f64::consts::PI;

use cadcraft3d::kernel::{Point3, Real};
use cadcraft3d::part::hole::{HoleCenter, SlotCorners, SlotDirection};
use cadcraft3d::part::{
    DrillPoint, ExtrudeFeature, Feature, HoleFeature, HoleSpec, LineRef, OriginPlane, PartDocument,
    ProfileRef, SeatType, SketchOptions, SketchPlacement, Termination,
};
use cadcraft3d::sketch::Point2;

const TOP: &str = "Extrusion1:End";
const PLATE: Real = 100.0 * 60.0 * 20.0;

fn volume(part: &PartDocument) -> Real {
    let opts = cadcraft3d::kernel::TessellationOptions::default();
    part.body
        .solids()
        .map(|(sid, _)| {
            let m = part.body.tessellate_solid(sid, &opts).unwrap();
            m.faces
                .triangles
                .iter()
                .map(|t| {
                    let [p, q, s] = t.map(|i| m.faces.positions[i as usize].coords);
                    p.dot(&q.cross(&s)) / 6.0
                })
                .sum::<Real>()
        })
        .sum()
}

fn plate() -> PartDocument {
    let mut part = PartDocument::default();
    let opts = SketchOptions {
        auto_project_origin: false,
        ..Default::default()
    };
    let i = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts)
        .unwrap();
    let sk = &mut part.sketches[i].sketch;
    sk.add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(100.0, 60.0))
        .unwrap();
    let r = sk.regions();
    part.add_extrude(
        i,
        ExtrudeFeature::new(vec![ProfileRef::of(&r[0], r[0].interior_point())], 20.0),
    )
    .unwrap();
    part
}

fn slot_def(part: &mut PartDocument, spec: HoleSpec, centers: &[(Real, Real)]) -> HoleFeature {
    let pts: Vec<Point3> = centers
        .iter()
        .map(|&(x, y)| Point3::new(x, y, 20.0))
        .collect();
    let (i, ids) = part.create_hole_sketch(TOP, &pts).unwrap();
    let mut def = HoleFeature::new(spec);
    def.sketch = part.sketches[i].id.clone();
    def.centers = ids.into_iter().map(HoleCenter::Point).collect();
    def
}

fn slot(spec: HoleSpec) -> PartDocument {
    let mut part = plate();
    let def = slot_def(&mut part, spec, &[(50.0, 30.0)]);
    let id = part.add_hole(def).unwrap();
    assert_eq!(id, "Slot1");
    part
}

/// The plate's volume less `removed` (Inventor's numbers), within `tol` of it.
fn check(part: &PartDocument, removed: Real, tol: Real) {
    assert!(part.exact, "exact kernel");
    assert!(part.feature_health.is_empty(), "{:?}", part.feature_health);
    for (sid, _) in part.body.solids() {
        part.body.validate_solid(sid).unwrap();
    }
    let got = PLATE - volume(part);
    assert!(
        (got - removed).abs() <= tol * removed,
        "removed {got:.3} vs {removed:.3}"
    );
}

/// The box of the faces named `<prefix>…`.
fn faces_box(part: &PartDocument, prefix: &str) -> (Point3, Point3) {
    let mut lo = Point3::new(Real::MAX, Real::MAX, Real::MAX);
    let mut hi = Point3::new(Real::MIN, Real::MIN, Real::MIN);
    for (_, f) in part.body.faces() {
        if !f.name.as_deref().is_some_and(|n| n.starts_with(prefix)) {
            continue;
        }
        for p in &f.mesh.as_ref().unwrap().positions {
            for i in 0..3 {
                lo[i] = lo[i].min(p[i]);
                hi[i] = hi[i].max(p[i]);
            }
        }
    }
    (lo, hi)
}

fn near(a: Real, b: Real) -> bool {
    (a - b).abs() < 2e-3
}

#[test]
fn round_ends_through_all() {
    let part = slot(HoleSpec::slot(10.0, 30.0));
    // 20 between the end centres (the length is overall).
    check(&part, 20.0 * (10.0 * 20.0 + PI * 25.0), 2e-3);
    let names: Vec<&str> = part
        .body
        .faces()
        .filter_map(|(_, f)| f.name.as_deref())
        .collect();
    for n in [
        "Slot1:BoreEnd1:1",
        "Slot1:BoreSide1:1",
        "Slot1:BoreEnd2:1",
        "Slot1:BoreSide2:1",
    ] {
        assert!(names.contains(&n), "{n} in {names:?}");
    }
    let (lo, hi) = faces_box(&part, "Slot1:");
    assert!(near(lo.x, 35.0) && near(hi.x, 65.0), "{lo:?} {hi:?}");
    assert!(near(lo.y, 25.0) && near(hi.y, 35.0), "{lo:?} {hi:?}");
    assert_eq!(part.body.faces().count(), 10);
}

#[test]
fn sharp_corners_through_all() {
    let mut s = HoleSpec::slot(10.0, 30.0);
    s.slot.as_mut().unwrap().corners = SlotCorners::Sharp;
    let part = slot(s);
    check(&part, 6000.0, 1e-6);
    assert_eq!(part.body.faces().count(), 10);
}

#[test]
fn blind_flat_and_angled() {
    let mut s = HoleSpec::slot(10.0, 30.0);
    s.termination = Termination::Distance;
    s.depth = 8.0;
    check(&slot(s.clone()), 2228.3185, 2e-3);
    s.drill_point = DrillPoint::Angle;
    s.point_angle = 118f64.to_radians();
    let part = slot(s.clone());
    // Inventor: 2607.4 (a V along the slot, half cones at the ends); ours stops
    // the V 1 % of the width short of its edge.
    check(&part, 2607.4013, 2e-3);
    let (lo, _) = faces_box(&part, "Slot1:");
    assert!((lo.z - 8.996).abs() < 0.05, "{lo:?}");
    // Sharp corners: the angle is ignored (Inventor).
    s.slot.as_mut().unwrap().corners = SlotCorners::Sharp;
    check(&slot(s), 2400.0, 1e-6);
}

#[test]
fn seats() {
    let mut s = HoleSpec::slot(10.0, 30.0);
    s.termination = Termination::Distance;
    s.depth = 8.0;
    s.seat = SeatType::Counterbore;
    s.seat_diameter = 16.0;
    s.seat_depth = 4.0;
    // The counterbore's depth is within the slot's.
    check(&slot(s.clone()), 3198.4070, 2e-3);
    s.seat = SeatType::Spotface;
    s.seat_depth = 1.0;
    // A spotface's floor is where the depth starts.
    check(&slot(s.clone()), 2749.3805, 2e-3);
    let mut c = HoleSpec::slot(10.0, 30.0);
    c.seat = SeatType::Countersink;
    c.csink_diameter = 16.0;
    c.csink_angle = 90f64.to_radians();
    let part = slot(c.clone());
    check(&part, 5920.4423, 2e-3);
    let (lo, hi) = faces_box(&part, "Slot1:CSink");
    assert!(near(lo.x, 32.0) && near(hi.x, 68.0), "{lo:?} {hi:?}");
    // No seat on sharp corners (Inventor ignores it).
    c.slot.as_mut().unwrap().corners = SlotCorners::Sharp;
    check(&slot(c), 6000.0, 1e-6);
}

#[test]
fn direction_and_rotation() {
    let mut s = HoleSpec::slot(10.0, 30.0);
    s.slot.as_mut().unwrap().direction = SlotDirection::Vertical;
    let part = slot(s.clone());
    let (lo, hi) = faces_box(&part, "Slot1:");
    assert!(near(lo.x, 45.0) && near(hi.y, 45.0), "{lo:?} {hi:?}");
    s.slot.as_mut().unwrap().direction = SlotDirection::Horizontal;
    s.slot.as_mut().unwrap().rotation = 30f64.to_radians();
    let part = slot(s.clone());
    check(&part, 5570.7963, 2e-3);
    let (lo, hi) = faces_box(&part, "Slot1:");
    assert!(
        (lo.x - 36.340).abs() < 0.01 && (hi.y - 40.0).abs() < 0.01,
        "{lo:?} {hi:?}"
    );
    // Counter-clockwise from above: the +x end goes up.
    let end1 = faces_box(&part, "Slot1:BoreEnd1");
    assert!(end1.1.y > 38.0 && end1.1.x > 60.0, "{end1:?}");
    s.slot.as_mut().unwrap().flip_rotation = true;
    let part = slot(s);
    let end1 = faces_box(&part, "Slot1:BoreEnd1");
    assert!(end1.0.y < 22.0 && end1.1.x > 60.0, "{end1:?}");
}

#[test]
fn custom_direction_along_an_edge() {
    let mut s = HoleSpec::slot(10.0, 30.0);
    s.slot.as_mut().unwrap().direction = SlotDirection::Custom;
    let mut part = plate();
    let mut def = slot_def(&mut part, s.clone(), &[(50.0, 30.0)]);
    // Without its edge: refused, nothing changes.
    let before = part.features.len();
    assert!(part.add_hole(def.clone()).is_err());
    assert_eq!(part.features.len(), before);
    // A short side of the plate (along y).
    let edge = part
        .body
        .edges()
        .filter_map(|(_, e)| e.name.clone())
        .find(|n| n.contains("Side") && n.contains("End"))
        .map(LineRef::Edge);
    let mut found = None;
    for (_, e) in part.body.edges() {
        let Some(n) = e.name.clone() else { continue };
        let r = LineRef::Edge(n);
        if let Ok((p, d)) = r.resolve(&part.body, &Default::default())
            && d.y.abs() > 0.99
            && p.z > 19.0
        {
            found = Some(r);
            break;
        }
    }
    def.slot_axis = found.or(edge);
    part.add_hole(def).unwrap();
    let (lo, hi) = faces_box(&part, "Slot1:");
    assert!(near(lo.x, 45.0) && near(hi.y, 45.0), "{lo:?} {hi:?}");
}

#[test]
fn refused_values_leave_the_part() {
    for (w, l) in [(10.0, 10.0), (10.0, 6.0), (0.0, 30.0)] {
        let mut part = plate();
        let def = slot_def(&mut part, HoleSpec::slot(w, l), &[(50.0, 30.0)]);
        let v = volume(&part);
        assert!(part.add_hole(def).is_err(), "{w} x {l}");
        assert!((volume(&part) - v).abs() < 1e-9);
    }
    let mut s = HoleSpec::slot(10.0, 30.0);
    s.symmetric = true;
    assert!(s.validate().is_err());
}

#[test]
fn several_centres_note_and_round_trip() {
    let mut part = plate();
    let mut s = HoleSpec::slot(8.0, 24.0);
    s.termination = Termination::Distance;
    s.depth = 5.0;
    let def = slot_def(&mut part, s, &[(25.0, 20.0), (75.0, 40.0)]);
    part.add_hole(def).unwrap();
    let one = 5.0 * (8.0 * 16.0 + PI * 16.0);
    check(&part, 2.0 * one, 2e-3);
    let holes = part.holes();
    assert_eq!(holes.len(), 2);
    assert_eq!(holes[0].spec.note_line(), "SLOT 8 X 24 ↧ 5");
    // A save keeps it a slot (format revision 73).
    let json = serde_json::to_string(&part).unwrap();
    let mut back: PartDocument = serde_json::from_str(&json).unwrap();
    back.regenerate().unwrap();
    check(&back, 2.0 * one, 2e-3);
    let Some(Feature::Hole(h)) = back.features.last() else {
        panic!("no slot");
    };
    assert!(h.is_slot());
}

#[test]
fn convert_to_slot() {
    let mut part = plate();
    let mut s = HoleSpec::simple(10.0, 8.0);
    s.drill_point = DrillPoint::Flat;
    let def = slot_def(&mut part, s, &[(50.0, 30.0)]);
    part.add_hole(def).unwrap();
    let k = part.features.len() - 1;
    part.convert_to_slot(k, 30.0).unwrap();
    // The hole keeps its id: its faces keep the prefix.
    check(&part, 8.0 * (10.0 * 20.0 + PI * 25.0), 2e-3);
    assert!(
        part.body
            .faces()
            .any(|(_, f)| f.name.as_deref() == Some("Hole1:BoreSide1:1"))
    );
    // Converting again is refused (one way).
    assert!(part.convert_to_slot(k, 30.0).is_err());
}
