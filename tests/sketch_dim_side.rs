//! A dimensioned point keeps its side of the geometry it is measured from when that geometry moves (spec 1.4).

use cadcraft3d::part::PartDocument;
use cadcraft3d::part::params::ParamOwner;
use cadcraft3d::sketch::{Constraint, ConstraintId, Point2, PointId, Shape, Sketch, SolveStatus};

struct Case {
    s: Sketch,
    width: ConstraintId,
    corner_bl: PointId,
    corner_br: PointId,
    corner_tr: PointId,
    right: cadcraft3d::sketch::EntityId,
    pt: PointId,
}

fn rectangle(px: f64, py: f64) -> Case {
    let mut s = Sketch::default();
    let shape: Shape = s
        .add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(100.0, 50.0))
        .unwrap();
    let at = |s: &Sketch, x: f64, y: f64| {
        s.points()
            .find(|(_, p)| (p.position - Point2::new(x, y)).norm() < 1e-6)
            .map(|(id, _)| id)
            .unwrap()
    };
    let (bl, br, tr) = (at(&s, 0.0, 0.0), at(&s, 100.0, 0.0), at(&s, 100.0, 50.0));
    let right = shape
        .entities
        .iter()
        .copied()
        .find(|&e| {
            let (a, b) = s.line_points(e).unwrap();
            let (a, b) = (s.point(a).unwrap().position, s.point(b).unwrap().position);
            (a.x - 100.0).abs() < 1e-6 && (b.x - 100.0).abs() < 1e-6
        })
        .unwrap();
    s.set_fixed(bl, true).unwrap();
    let width = s
        .add_constraint(Constraint::HorizontalDistance {
            a: bl,
            b: br,
            value: 100.0,
        })
        .unwrap();
    s.add_constraint(Constraint::VerticalDistance {
        a: bl,
        b: tr,
        value: 50.0,
    })
    .unwrap();
    let pt = s.add_point(Point2::new(px, py));
    assert_eq!(s.solve().unwrap().status, SolveStatus::Solved);
    Case {
        s,
        width,
        corner_bl: bl,
        corner_br: br,
        corner_tr: tr,
        right,
        pt,
    }
}

fn resize(c: &mut Case, w: f64) -> Point2 {
    c.s.set_dimension(c.width, w).unwrap();
    assert_eq!(
        c.s.solve_keeping_sides().unwrap().status,
        SolveStatus::Solved
    );
    c.s.point(c.pt).unwrap().position
}

#[test]
fn corner_horizontal_and_vertical() {
    let mut c = rectangle(90.0, 25.0);
    c.s.add_constraint(Constraint::HorizontalDistance {
        a: c.pt,
        b: c.corner_br,
        value: 10.0,
    })
    .unwrap();
    c.s.add_constraint(Constraint::VerticalDistance {
        a: c.corner_br,
        b: c.pt,
        value: 25.0,
    })
    .unwrap();
    let p = resize(&mut c, 5.0);
    assert!((p.x + 5.0).abs() < 1e-6, "{p:?}");
    assert!((p.y - 25.0).abs() < 1e-6, "{p:?}");
    let p = resize(&mut c, 30.0);
    assert!((p.x - 20.0).abs() < 1e-6, "{p:?}");
}

#[test]
fn side_offset_keeps_its_side() {
    let mut c = rectangle(90.0, 25.0);
    c.s.add_constraint(Constraint::PointLineDistance {
        point: c.pt,
        line: c.right,
        value: 10.0,
    })
    .unwrap();
    c.s.add_constraint(Constraint::VerticalDistance {
        a: c.corner_bl,
        b: c.pt,
        value: 25.0,
    })
    .unwrap();
    let p = resize(&mut c, 5.0);
    assert!((p.x + 5.0).abs() < 1e-6, "{p:?}");
    let p = resize(&mut c, 30.0);
    assert!((p.x - 20.0).abs() < 1e-6, "{p:?}");
}

#[test]
fn corner_aligned_keeps_its_side() {
    let mut c = rectangle(90.0, 25.0);
    c.s.add_constraint(Constraint::Distance {
        a: c.pt,
        b: c.corner_tr,
        value: (100.0f64 + 625.0).sqrt(),
    })
    .unwrap();
    let p = resize(&mut c, 5.0);
    // Inventor: the point goes with the corner, (-5, 25).
    assert!((p.x + 5.0).abs() < 1e-6, "{p:?}");
    assert!((p.y - 25.0).abs() < 1e-6, "{p:?}");
}

/// Lengths that cannot all keep their directions: the solve still succeeds,
/// and a lone point dimensioned to the triangle still follows its corner.
#[test]
fn a_triangle_of_lengths_changes_its_shape() {
    let mut s = Sketch::default();
    let a = s.add_point(Point2::new(0.0, 0.0));
    let b = s.add_point(Point2::new(40.0, 0.0));
    let c = s.add_point(Point2::new(20.0, 30.0));
    s.set_fixed(a, true).unwrap();
    s.add_line_between(a, b).unwrap();
    s.add_line_between(b, c).unwrap();
    s.add_line_between(c, a).unwrap();
    let ab = s
        .add_constraint(Constraint::Distance { a, b, value: 40.0 })
        .unwrap();
    let side = (20.0f64 * 20.0 + 30.0 * 30.0).sqrt();
    s.add_constraint(Constraint::Distance {
        a: b,
        b: c,
        value: side,
    })
    .unwrap();
    s.add_constraint(Constraint::Distance {
        a: c,
        b: a,
        value: side,
    })
    .unwrap();
    let pt = s.add_point(Point2::new(50.0, 10.0));
    s.add_constraint(Constraint::Distance {
        a: b,
        b: pt,
        value: (100.0f64 + 100.0).sqrt(),
    })
    .unwrap();
    s.set_dimension(ab, 30.0).unwrap();
    assert_eq!(s.solve_keeping_sides().unwrap().status, SolveStatus::Solved);
    let (pb, pc, pp) = (
        s.point(b).unwrap().position,
        s.point(c).unwrap().position,
        s.point(pt).unwrap().position,
    );
    assert!(((pb - s.point(a).unwrap().position).norm() - 30.0).abs() < 1e-8);
    assert!(((pc - pb).norm() - side).abs() < 1e-8);
    let v = pp - pb;
    assert!(v.x > 0.0 && v.y > 0.0, "{v:?}");
}

/// A saved, solved sketch does not move, and failure changes nothing.
#[test]
fn nothing_open_moves_nothing_and_a_conflict_leaves_the_sketch() {
    let mut c = rectangle(90.0, 25.0);
    c.s.add_constraint(Constraint::Distance {
        a: c.pt,
        b: c.corner_tr,
        value: (100.0f64 + 625.0).sqrt(),
    })
    .unwrap();
    assert_eq!(c.s.solve().unwrap().status, SolveStatus::Solved);
    let was = c.s.point(c.pt).unwrap().position;
    assert_eq!(
        c.s.solve_keeping_sides().unwrap().status,
        SolveStatus::Solved
    );
    assert_eq!(c.s.point(c.pt).unwrap().position, was);
    // Both ends of the width held: the new width cannot be reached.
    c.s.set_fixed(c.corner_br, true).unwrap();
    c.s.set_dimension(c.width, 5.0).unwrap();
    let report = c.s.solve_keeping_sides().unwrap();
    assert_eq!(report.status, SolveStatus::Failed);
    assert_eq!(c.s.point(c.pt).unwrap().position, was);
}

fn part_with_rectangle() -> (
    PartDocument,
    ConstraintId,
    ConstraintId,
    ConstraintId,
    PointId,
) {
    let mut part = PartDocument::default();
    let mut c = rectangle(90.0, 25.0);
    let height =
        c.s.constraints()
            .find(|(_, k)| matches!(k, Constraint::VerticalDistance { .. }))
            .map(|(id, _)| id)
            .unwrap();
    // The point stands by itself (from the fixed corner); its distance to
    // the right side is driven.
    c.s.add_constraint(Constraint::HorizontalDistance {
        a: c.corner_bl,
        b: c.pt,
        value: 90.0,
    })
    .unwrap();
    c.s.add_constraint(Constraint::VerticalDistance {
        a: c.corner_bl,
        b: c.pt,
        value: 25.0,
    })
    .unwrap();
    let driven =
        c.s.add_constraint(Constraint::HorizontalDistance {
            a: c.pt,
            b: c.corner_br,
            value: 10.0,
        })
        .unwrap();
    c.s.set_driven(driven, true).unwrap();
    part.sketches
        .push(cadcraft3d::part::PartSketch::new("Sketch1", c.s));
    part.sync_parameters();
    (part, c.width, height, driven, c.pt)
}

/// Inventor's reference parameter: named, listed, read by equations, set by
/// nobody; the equation follows the geometry.
#[test]
fn a_driven_dimension_drives_another_through_its_name() {
    let (mut part, width, height, driven, _) = part_with_rectangle();
    let sk = |p: &PartDocument| p.sketches[0].sketch.clone();
    let name = |p: &PartDocument, c| sk(p).dim_param(c).name;
    let (dw, dh, dr) = (name(&part, width), name(&part, height), name(&part, driven));
    let row = part.parameter(&dr).expect("a driven dimension is listed");
    assert!(matches!(row.owner, ParamOwner::Reference { .. }));
    assert!((row.value - 10.0).abs() < 1e-9);

    part.set_parameter_expr(&dh, &format!("{dr} * 3")).unwrap();
    part.solve_sketch(0).unwrap().unwrap();
    assert!((sk(&part).measure(height).unwrap() - 30.0).abs() < 1e-8);

    // The width moves the driven value; the height follows in one solve
    // (Inventor: W = 120 gives driven 30, height 90).
    part.set_parameter_expr(&dw, "120").unwrap();
    part.solve_sketch(0).unwrap().unwrap();
    assert!((sk(&part).measure(driven).unwrap() - 30.0).abs() < 1e-8);
    assert!((sk(&part).measure(height).unwrap() - 90.0).abs() < 1e-8);
    // The equation stays as typed.
    assert_eq!(
        sk(&part).dim_param(height).expr.as_deref(),
        Some(format!("{dr} * 3").as_str())
    );
    // Regeneration gives the same.
    part.regenerate().unwrap();
    assert!((sk(&part).measure(height).unwrap() - 90.0).abs() < 1e-8);

    // Read only.
    let before = sk(&part).measure(driven);
    assert!(part.set_parameter_expr(&dr, "5 mm").is_err());
    assert_eq!(sk(&part).measure(driven), before);
    // It survives a save and load as an equation.
    let text = serde_json::to_string(&part.sketches[0].sketch).unwrap();
    let back: Sketch = serde_json::from_str(&text).unwrap();
    assert!(back.is_driven(driven));
}

/// An equation reading a driven dimension that follows it would creep at
/// every solve: refused, nothing changed.
#[test]
fn an_equation_on_its_own_driven_dimension_is_refused() {
    let (mut part, width, _, driven, _) = part_with_rectangle();
    let sk = &part.sketches[0].sketch;
    let (dw, dr) = (sk.dim_param(width).name, sk.dim_param(driven).name);
    let was = sk.constraint(width).and_then(Constraint::value);
    assert!(
        part.set_parameter_expr(&dw, &format!("{dr} + 100"))
            .is_err()
    );
    let sk = &part.sketches[0].sketch;
    assert_eq!(sk.constraint(width).and_then(Constraint::value), was);
    assert!(sk.dim_param(width).expr.is_none());
}
