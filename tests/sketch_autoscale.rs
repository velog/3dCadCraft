//! Auto-scale sketch geometry on initial dimension (`docs/spec/1.4-dimensions.md`):
//! the first dimension of a sketch scales the whole sketch about its origin,
//! so its shape stays; later dimensions, angles and sketches tied to the model
//! do not scale.

use cadcraft3d::part::{OriginPlane, PartDocument, SketchOptions, SketchPlacement};
use cadcraft3d::sketch::{Constraint, Point2, SolveStatus};

/// A part with one sketch on XY (the origin projected) holding a
/// 100 × 50 rectangle from (10, 10) and a circle r 5 at (60, 35).
fn sketch() -> PartDocument {
    let mut part = PartDocument::default();
    let i = part
        .create_sketch(
            SketchPlacement::on_origin_plane(OriginPlane::XY),
            &SketchOptions::default(),
        )
        .unwrap();
    let sk = &mut part.sketches[i].sketch;
    sk.add_rectangle_two_point(Point2::new(10.0, 10.0), Point2::new(110.0, 60.0))
        .unwrap();
    sk.add_circle(Point2::new(60.0, 35.0), 5.0).unwrap();
    part
}

/// The bottom edge's two corners (y = 10, x 10 and 110).
fn bottom(part: &PartDocument) -> (cadcraft3d::sketch::PointId, cadcraft3d::sketch::PointId) {
    let sk = &part.sketches[0].sketch;
    let at = |x: f64, y: f64| {
        sk.points()
            .find(|(_, p)| (p.position - Point2::new(x, y)).norm() < 1e-6)
            .map(|(id, _)| id)
            .unwrap()
    };
    (at(10.0, 10.0), at(110.0, 10.0))
}

#[test]
fn first_dimension_scales_the_sketch_about_its_origin() {
    let mut part = sketch();
    let (a, b) = bottom(&part);
    let before = part.sketches[0].sketch.solve().unwrap();
    let ps = &mut part.sketches[0];
    let c = ps
        .sketch
        .add_constraint(Constraint::Distance { a, b, value: 100.0 })
        .unwrap();
    // The user types 20: everything shrinks to a fifth about (0, 0).
    let k = ps.autoscale_factor(c, 20.0).unwrap();
    assert!((k - 0.2).abs() < 1e-12, "{k}");
    ps.sketch.scale_about_origin(k).unwrap();
    let rep = ps.sketch.solve().unwrap();
    assert_eq!(rep.status, SolveStatus::Solved);
    assert_eq!(rep.redundant, 0);
    // One more equation than before: the dimension.
    assert_eq!(rep.dof + 1, before.dof);
    let sk = &ps.sketch;
    let pa = sk.point(a).unwrap().position;
    let pb = sk.point(b).unwrap().position;
    assert!((pa - Point2::new(2.0, 2.0)).norm() < 1e-9, "{pa:?}");
    assert!((pb - Point2::new(22.0, 2.0)).norm() < 1e-9, "{pb:?}");
    assert!((sk.measure(c).unwrap() - 20.0).abs() < 1e-9);
    // The circle went with it: centre (12, 7), radius 1.
    let (_, circle) = sk
        .entities()
        .find(|(_, e)| matches!(e, cadcraft3d::sketch::Entity::Circle { .. }))
        .unwrap();
    let cadcraft3d::sketch::Entity::Circle { center, radius } = circle else {
        panic!("not a circle");
    };
    assert!((radius - 1.0).abs() < 1e-12);
    assert!((sk.point(*center).unwrap().position - Point2::new(12.0, 7.0)).norm() < 1e-9);
    // The projected origin stays at the origin.
    assert!(
        sk.points()
            .filter(|(p, _)| ps.is_projected(*p))
            .all(|(_, q)| q.position.coords.norm() < 1e-12)
    );
}

#[test]
fn only_the_first_length_dimension_scales() {
    let mut part = sketch();
    let (a, b) = bottom(&part);
    let ps = &mut part.sketches[0];
    let c = ps
        .sketch
        .add_constraint(Constraint::Distance { a, b, value: 100.0 })
        .unwrap();
    // A second dimension: no scaling for either.
    let circle = ps
        .sketch
        .entities()
        .find(|(_, e)| matches!(e, cadcraft3d::sketch::Entity::Circle { .. }))
        .map(|(id, _)| id)
        .unwrap();
    let r = ps
        .sketch
        .add_constraint(Constraint::Radius { circle, value: 5.0 })
        .unwrap();
    assert_eq!(ps.autoscale_factor(r, 2.0), None);
    assert_eq!(ps.autoscale_factor(c, 20.0), None);
    // The same value: nothing to scale.
    let mut fresh = sketch();
    let (a, b) = bottom(&fresh);
    let ps = &mut fresh.sketches[0];
    let c = ps
        .sketch
        .add_constraint(Constraint::Distance { a, b, value: 100.0 })
        .unwrap();
    assert_eq!(ps.autoscale_factor(c, 100.0), None);
    // A point fixed away from the origin holds the sketch where it is.
    let mut held = ps.clone();
    held.sketch.set_fixed(a, true).unwrap();
    assert_eq!(held.autoscale_factor(c, 20.0), None);
    // A driven dimension does not scale.
    ps.sketch.set_driven(c, true).unwrap();
    assert_eq!(ps.autoscale_factor(c, 20.0), None);
}

#[test]
fn a_sketch_on_model_edges_does_not_scale() {
    // A sketch on a box's top face holds its projected edges: scaling would
    // tear the geometry off them.
    let mut part = PartDocument::with_box("Box1", [40.0, 30.0, 10.0]).unwrap();
    let i = part
        .create_sketch(
            SketchPlacement::on_face("Box1:Top"),
            &SketchOptions::default(),
        )
        .unwrap();
    let ps = &mut part.sketches[i];
    assert!(!ps.reference_lines.is_empty() || !ps.links.is_empty());
    let s = ps
        .sketch
        .add_rectangle_two_point(Point2::new(5.0, 5.0), Point2::new(15.0, 10.0))
        .unwrap();
    let (a, b) = (s.points[0], s.points[1]);
    let c = ps
        .sketch
        .add_constraint(Constraint::Distance { a, b, value: 10.0 })
        .unwrap();
    assert_eq!(ps.autoscale_factor(c, 20.0), None);
}
