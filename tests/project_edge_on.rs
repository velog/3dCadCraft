//! Project Geometry of a circle seen edge-on (`docs/spec/1.8-project-geometry.md`):
//! a hole's rim projected onto a side face is the segment it is seen as
//! (Inventor), so Dimension can take it.

use cadcraft3d::kernel::{Curve, Real};
use cadcraft3d::part::{
    ExtrudeFeature, OriginPlane, PartDocument, ProfileRef, SketchOptions, SketchPlacement,
};
use cadcraft3d::sketch::{Entity, Point2};

/// A 100 × 60 × 5 plate with a Ø20 hole at (25, 30) and a 20 mm slot
/// (half circles r 5 about (60, 30) and (80, 30)).
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
    sk.add_circle(Point2::new(25.0, 30.0), 10.0).unwrap();
    sk.add_slot_center_to_center(Point2::new(60.0, 30.0), Point2::new(80.0, 30.0), 10.0)
        .unwrap();
    let r = sk.regions();
    let plate = r.iter().find(|x| x.holes.len() == 2).unwrap();
    part.add_extrude(
        i,
        ExtrudeFeature::new(vec![ProfileRef::of(plate, plate.interior_point())], 5.0),
    )
    .unwrap();
    part
}

/// A sketch on the plate's front face (normal −Y, at y = 0).
fn front_sketch(part: &mut PartDocument) -> usize {
    let front = part
        .body
        .faces()
        .find(|(id, _)| {
            part.body
                .face_plane(*id)
                .is_some_and(|p| p.normal.y < -0.99 && p.origin.y.abs() < 1e-6)
        })
        .and_then(|(_, f)| f.name.clone())
        .unwrap();
    part.create_sketch(SketchPlacement::on_face(front), &SketchOptions::default())
        .unwrap()
}

/// The ends of line `e` of sketch `i` in model coordinates.
fn ends(part: &PartDocument, i: usize, e: cadcraft3d::sketch::EntityId) -> [[Real; 3]; 2] {
    let sk = &part.sketches[i].sketch;
    let Some(Entity::Line { start, end }) = sk.entity(e) else {
        panic!("not a line: {:?}", sk.entity(e));
    };
    [*start, *end].map(|p| {
        let q = sk.point(p).unwrap().position;
        let w = sk.plane.point_at(q.x, q.y);
        [w.x, w.y, w.z]
    })
}

/// Circular edges of the body whose centre lies at height `z`, with their
/// centre x.
fn circular_edges(part: &PartDocument, z: Real) -> Vec<(cadcraft3d::kernel::EdgeId, Real)> {
    part.body
        .edges()
        .filter_map(|(id, e)| match part.body.curve(e.curve) {
            Some(Curve::Circle(c)) if (c.center.z - z).abs() < 1e-6 => Some((id, c.center.x)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_hole_rim_seen_edge_on_is_a_segment() {
    let mut part = plate();
    let i = front_sketch(&mut part);
    let (rim, _) = circular_edges(&part, 5.0)
        .into_iter()
        .find(|(_, x)| (x - 25.0).abs() < 1e-6)
        .unwrap();
    let e = part.project_model_edge(i, rim, false).unwrap();
    let [a, b] = ends(&part, i, e);
    let (lo, hi) = if a[0] < b[0] { (a, b) } else { (b, a) };
    // The hole's width at the top face: x 15..35, z 5, on the face (y 0).
    for (p, x) in [(lo, 15.0), (hi, 35.0)] {
        assert!((p[0] - x).abs() < 1e-6, "{p:?}");
        assert!((p[2] - 5.0).abs() < 1e-6, "{p:?}");
        assert!(p[1].abs() < 1e-6, "{p:?}");
    }
    // It stays a line through a regeneration (the link re-reads it).
    part.regenerate().unwrap();
    assert!(
        part.sketches[i].health.is_none(),
        "{:?}",
        part.sketches[i].health
    );
    let [a, b] = ends(&part, i, e);
    assert!(((a[0] - b[0]).abs() - 20.0).abs() < 1e-6);
}

#[test]
fn a_half_circle_seen_edge_on_spans_its_reach() {
    // The slot's right half circle about (80, 30): its extremes across X are
    // its ends at x 80 and the far point at x 85.
    let mut part = plate();
    let i = front_sketch(&mut part);
    let arcs: Vec<_> = circular_edges(&part, 5.0)
        .into_iter()
        .filter(|(_, x)| (x - 80.0).abs() < 1e-6)
        .collect();
    assert!(!arcs.is_empty());
    let mut spans = Vec::new();
    for (edge, _) in arcs {
        let e = part.project_model_edge(i, edge, false).unwrap();
        let [a, b] = ends(&part, i, e);
        let (lo, hi) = (a[0].min(b[0]), a[0].max(b[0]));
        spans.push((lo, hi));
    }
    // Every piece of the half circle reaches from its ends to x 85 at most.
    for (lo, hi) in &spans {
        assert!(*lo >= 80.0 - 1e-6 && *hi <= 85.0 + 1e-6, "{spans:?}");
    }
    assert!(
        spans.iter().any(|(_, hi)| (hi - 85.0).abs() < 1e-6),
        "{spans:?}"
    );
}
