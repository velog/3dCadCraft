//! Assembly constraint solver on harder cases (spec 3.1 / 3.2): what Inventor and SolidWorks
//! accept — consistent redundant constraints, components starting far from (even upside down
//! against) their solution, long chains and closed loops — and what they must refuse without
//! moving anything wildly (inconsistent sets). GUI-free: geometry is given directly, as a 20 mm
//! cube with named faces, a Ø10 shaft along its z and a Ø6 rim on its top.

use cadcraft3d::assembly::{Constraint, ConstraintKind, Geom, GeomRef, Solved, solve};
use cadcraft3d::document::{AssemblyDocument, DocRef, DocumentId};
use cadcraft3d::kernel::{Point3, Real, Transform3, Vector3};

fn cube_geom(r: &GeomRef) -> Result<Geom, String> {
    if let GeomRef::Origin { item, .. } = r {
        return Ok(item.geom());
    }
    if let GeomRef::Edge { .. } = r {
        return Ok(Geom::Circle {
            c: Point3::new(10.0, 10.0, 20.0),
            n: Vector3::z(),
            r: 3.0,
        });
    }
    let GeomRef::Face { name, .. } = r else {
        return Err("unknown".into());
    };
    if name == "Shaft" {
        return Ok(Geom::Cylinder {
            p: Point3::new(10.0, 10.0, 0.0),
            d: Vector3::z(),
            r: 5.0,
        });
    }
    let (p, n) = match name.as_str() {
        "Top" => (Point3::new(10.0, 10.0, 20.0), Vector3::z()),
        "Bottom" => (Point3::new(10.0, 10.0, 0.0), -Vector3::z()),
        "Right" => (Point3::new(20.0, 10.0, 10.0), Vector3::x()),
        "Left" => (Point3::new(0.0, 10.0, 10.0), -Vector3::x()),
        "Front" => (Point3::new(10.0, 0.0, 10.0), -Vector3::y()),
        "Back" => (Point3::new(10.0, 20.0, 10.0), Vector3::y()),
        _ => return Err(format!("the face {name} no longer exists")),
    };
    Ok(Geom::Plane { p, n })
}

fn face(o: u32, name: &str) -> GeomRef {
    GeomRef::Face {
        occurrence: o,
        name: name.into(),
        hint: [0.0; 3],
    }
}

fn con(kind: ConstraintKind, a: GeomRef, b: GeomRef, value: Real) -> Constraint {
    Constraint {
        id: 0,
        name: String::new(),
        kind,
        a,
        b,
        value,
        expr: None,
        aligned: false,
        suppressed: false,
        locked: false,
        min: None,
        max: None,
        resting: false,
        plane: None,
    }
}

fn applied(a: &mut AssemblyDocument, s: &Solved) {
    for (id, t) in &s.transforms {
        if let Some(o) = a.find_mut(*id) {
            o.transform = *t;
        }
    }
}

/// Cube 1 grounded at the origin, cube 2 placed at `t`.
fn pair(t: Transform3) -> AssemblyDocument {
    let mut a = AssemblyDocument::default();
    let r = DocRef::local(DocumentId::new_random());
    a.place(r, "Cube", Transform3::identity(), true);
    a.place(r, "Cube", t, false);
    a
}

/// Stack: 2's bottom on 1's top, left and front flush (front offset 5).
fn stack(a: &mut AssemblyDocument, lower: u32, upper: u32) {
    a.add_constraint(con(
        ConstraintKind::Mate,
        face(lower, "Top"),
        face(upper, "Bottom"),
        0.0,
    ));
    a.add_constraint(con(
        ConstraintKind::Flush,
        face(lower, "Left"),
        face(upper, "Left"),
        0.0,
    ));
    a.add_constraint(con(
        ConstraintKind::Flush,
        face(lower, "Front"),
        face(upper, "Front"),
        0.0,
    ));
}

fn corner(a: &AssemblyDocument, id: u32) -> Point3 {
    a.find(id).unwrap().transform * Point3::origin()
}

fn up(a: &AssemblyDocument, id: u32) -> Vector3 {
    a.find(id).unwrap().transform * Vector3::z()
}

/// Inventor accepts a constraint that repeats what others already hold
/// (here the same Mate twice and a Mate of the opposite faces at the
/// matching offset): the set converges to the same placement.
#[test]
fn consistent_redundant_constraints_converge() {
    let t = Transform3::new(Vector3::new(50.0, 30.0, 7.0), Vector3::new(0.2, -0.1, 0.3));
    let mut a = pair(t);
    stack(&mut a, 1, 2);
    a.add_constraint(con(
        ConstraintKind::Mate,
        face(1, "Top"),
        face(2, "Bottom"),
        0.0,
    ));
    // 2's top lies 20 above 1's top (along 1's top normal): a Flush of the
    // tops at 20 says the same as the Mate.
    a.add_constraint(con(
        ConstraintKind::Flush,
        face(1, "Top"),
        face(2, "Top"),
        20.0,
    ));
    let s = solve(&a, &cube_geom, None);
    assert!(s.converged, "residual {}", s.residual);
    applied(&mut a, &s);
    assert!(
        (corner(&a, 2) - Point3::new(0.0, 0.0, 20.0)).norm() < 1e-6,
        "{}",
        corner(&a, 2)
    );
    assert!((up(&a, 2) - Vector3::z()).norm() < 1e-6);
}

/// A set that cannot hold (two Mates of the same faces at offsets 0 and 1):
/// not converged, the residual says how far, and nothing flies away.
#[test]
fn inconsistent_constraints_fail_without_flying_away() {
    for (gap, from) in [(1.0, 7.0), (1e-3, 7.0), (5.0, 500.0)] {
        let t = Transform3::new(Vector3::new(from, 30.0, 7.0), Vector3::new(0.2, -0.1, 0.3));
        let mut a = pair(t);
        stack(&mut a, 1, 2);
        a.add_constraint(con(
            ConstraintKind::Mate,
            face(1, "Top"),
            face(2, "Bottom"),
            gap,
        ));
        let s = solve(&a, &cube_geom, None);
        assert!(!s.converged, "gap {gap}: should not converge");
        assert!(s.residual > 0.1 * gap, "gap {gap}: residual {}", s.residual);
        for (_, t) in &s.transforms {
            let p = t * Point3::origin();
            assert!(p.coords.iter().all(|c| c.is_finite()), "finite");
            assert!(
                p.coords.norm() < 2.0 * from + 100.0,
                "gap {gap}: flew to {p}"
            );
        }
    }
}

/// Starting far away and turned (90°, 179°, upside down about x and y), a
/// stack still finds the one solution — Inventor solves from any start.
#[test]
fn components_far_from_their_solution() {
    for (axis, angle) in [
        (Vector3::z(), 1.5),
        (Vector3::x(), 3.1),
        (Vector3::y(), 3.1),
        (Vector3::new(1.0, 1.0, 0.0).normalize(), 2.5),
        (Vector3::new(0.3, -0.7, 0.6).normalize(), 3.0),
    ] {
        let t = Transform3::new(Vector3::new(-300.0, 450.0, 120.0), axis * angle);
        let mut a = pair(t);
        stack(&mut a, 1, 2);
        let s = solve(&a, &cube_geom, None);
        assert!(s.converged, "{axis} × {angle}: residual {}", s.residual);
        applied(&mut a, &s);
        assert!(
            (corner(&a, 2) - Point3::new(0.0, 0.0, 20.0)).norm() < 1e-6,
            "{axis} × {angle}: {}",
            corner(&a, 2)
        );
        assert!((up(&a, 2) - Vector3::z()).norm() < 1e-6, "upright");
    }
}

/// A tower of 25 cubes, each placed anywhere and turned: every one ends on
/// the one below (z = 20·k), all fully constrained.
#[test]
fn a_long_chain_of_stacked_components() {
    let mut a = AssemblyDocument::default();
    let r = DocRef::local(DocumentId::new_random());
    a.place(r, "Cube", Transform3::identity(), true);
    let n = 25u32;
    for k in 1..n {
        let f = k as Real;
        a.place(
            r,
            "Cube",
            Transform3::new(
                Vector3::new(37.0 * f.sin() * 10.0, 13.0 * f, -5.0 * f),
                Vector3::new(0.3 * f.cos(), 0.2, -0.25 * f.sin()),
            ),
            false,
        );
    }
    for k in 1..n {
        stack(&mut a, k, k + 1);
    }
    let s = solve(&a, &cube_geom, None);
    assert!(s.converged, "residual {}", s.residual);
    applied(&mut a, &s);
    for k in 1..=n {
        let c = corner(&a, k);
        assert!(
            (c - Point3::new(0.0, 0.0, 20.0 * (k - 1) as Real)).norm() < 1e-5,
            "cube {k}: {c}"
        );
        if k > 1 {
            assert_eq!(s.dof.get(&k).copied(), Some(0), "cube {k} fixed");
        }
    }
}

/// A closed loop: 2 and 3 side by side on 1, and 3 also flush with 2 —
/// three components constrained in a ring, solved together.
#[test]
fn a_closed_loop_of_constraints() {
    let mut a = AssemblyDocument::default();
    let r = DocRef::local(DocumentId::new_random());
    a.place(r, "Cube", Transform3::identity(), true);
    a.place(
        r,
        "Cube",
        Transform3::new(
            Vector3::new(80.0, -40.0, 30.0),
            Vector3::new(0.4, 0.1, -0.2),
        ),
        false,
    );
    a.place(
        r,
        "Cube",
        Transform3::new(
            Vector3::new(-60.0, 70.0, -20.0),
            Vector3::new(-0.3, 0.5, 0.9),
        ),
        false,
    );
    // 2 on 1's top, flush left and front.
    stack(&mut a, 1, 2);
    // 3 beside 2 (its left on 2's right), its bottom flush with 2's bottom,
    // its front flush with 1's front: the loop 1 – 2 – 3 – 1.
    a.add_constraint(con(
        ConstraintKind::Mate,
        face(2, "Right"),
        face(3, "Left"),
        0.0,
    ));
    a.add_constraint(con(
        ConstraintKind::Flush,
        face(2, "Bottom"),
        face(3, "Bottom"),
        0.0,
    ));
    a.add_constraint(con(
        ConstraintKind::Flush,
        face(1, "Front"),
        face(3, "Front"),
        0.0,
    ));
    let s = solve(&a, &cube_geom, None);
    assert!(s.converged, "residual {}", s.residual);
    applied(&mut a, &s);
    assert!((corner(&a, 2) - Point3::new(0.0, 0.0, 20.0)).norm() < 1e-6);
    assert!(
        (corner(&a, 3) - Point3::new(20.0, 0.0, 20.0)).norm() < 1e-6,
        "{}",
        corner(&a, 3)
    );
    assert_eq!(s.dof.get(&3).copied(), Some(0));
}

/// Shaft on shaft (an axis–axis Mate) and face on face: one turn about the
/// shaft left free; an Angle between the fronts fixes it.
fn shaft_and_angle(angle: Real) -> (AssemblyDocument, Solved) {
    let t = Transform3::new(
        Vector3::new(40.0, -25.0, 60.0),
        Vector3::new(0.5, 0.3, -0.4),
    );
    let mut a = pair(t);
    a.add_constraint(con(
        ConstraintKind::Mate,
        face(1, "Shaft"),
        face(2, "Shaft"),
        0.0,
    ));
    a.add_constraint(con(
        ConstraintKind::Mate,
        face(1, "Top"),
        face(2, "Bottom"),
        0.0,
    ));
    let s = solve(&a, &cube_geom, None);
    assert!(s.converged, "residual {}", s.residual);
    assert_eq!(s.dof.get(&2).copied(), Some(1), "the turn about the shaft");
    a.add_constraint(con(
        ConstraintKind::Angle,
        face(1, "Front"),
        face(2, "Front"),
        angle,
    ));
    let s = solve(&a, &cube_geom, None);
    applied(&mut a, &s);
    (a, s)
}

fn front_angle(a: &AssemblyDocument) -> Real {
    let f2 = a.find(2).unwrap().transform * -Vector3::y();
    (-Vector3::y()).dot(&f2).clamp(-1.0, 1.0).acos()
}

#[test]
fn axis_mates_and_angles() {
    for angle in [0.5, 1.0, std::f64::consts::FRAC_PI_2, 2.5] {
        let (a, s) = shaft_and_angle(angle);
        assert!(s.converged, "angle {angle}: residual {}", s.residual);
        assert_eq!(s.dof.get(&2).copied(), Some(0), "angle {angle}: fixed");
        let got = front_angle(&a);
        assert!((got - angle).abs() < 1e-7, "angle {angle}: got {got}");
    }
}

/// Inventor holds an Angle of 0° or 180° (parallel faces) exactly and
/// counts the turn as fixed. The cosine residual used to be flat there (a
/// double root: 7.5e-5 rad left, one freedom); the solver now measures the
/// angle by atan2 and parallel faces by their normals' difference.
#[test]
fn angles_at_the_ends_of_their_range() {
    for angle in [0.0, std::f64::consts::PI, 1e-4, std::f64::consts::PI - 1e-4] {
        let (a, s) = shaft_and_angle(angle);
        assert!(s.converged, "angle {angle}: residual {}", s.residual);
        let got = front_angle(&a);
        assert!((got - angle).abs() < 1e-7, "angle {angle}: got {got}");
        assert_eq!(s.dof.get(&2).copied(), Some(0), "angle {angle}: fixed");
    }
}
