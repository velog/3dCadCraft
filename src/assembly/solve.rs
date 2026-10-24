//! The assembly solver (`docs/spec/3.1-assemblies.md` §6.2): every
//! occurrence that is not grounded has six unknowns — a translation and a
//! small rotation about its own origin, applied to its current placement
//! and re-linearised each iteration (no gimbal lock). Constraints are a
//! few scalar residuals on their geometry moved into the assembly
//! (`relation::residuals`). Levenberg steps in minimum-norm form
//! (`dx = −Jᵀ(JJᵀ + λI)⁻¹r`): components move as little as the
//! constraints allow, so free degrees of freedom keep their place — and a
//! dragged component weighs more, so the others follow it.
//!
//! Sizes are small (tens of components), so the Jacobian is dense and
//! taken by finite differences; the degrees of freedom come from the rank
//! of each component's block (the others held).

use std::collections::HashMap;

use nalgebra::{DMatrix, DVector};

use super::relation::{Constraint, Geom, GeomRef, residuals};
use crate::document::AssemblyDocument;
use crate::kernel::{Real, Transform3, Vector3};

/// The result of a solve.
#[derive(Clone, Debug, Default)]
pub struct Solved {
    /// New placements of the components that moved.
    pub transforms: Vec<(u32, Transform3)>,
    /// Constraints whose geometry could not be found: (id, reason).
    pub sick: Vec<(u32, String)>,
    /// Every active constraint holds (to 1e-6).
    pub converged: bool,
    /// The largest remaining residual.
    pub residual: Real,
    /// Free degrees of freedom of each component that is not grounded
    /// (the others held where they are).
    pub dof: HashMap<u32, usize>,
}

/// Length scale of the rotation unknowns: a turn of 1/L radian counts like
/// a move of 1 mm.
const L: Real = 50.0;

/// Solves the constraints of `asm`. `local` gives a reference's geometry in
/// its part's coordinates; `heavy` is a component being dragged (it moves
/// last).
pub fn solve(
    asm: &AssemblyDocument,
    local: &dyn Fn(&GeomRef) -> Result<Geom, String>,
    heavy: Option<u32>,
) -> Solved {
    let mut out = Solved::default();
    // Active constraints with their local geometry (joint origins as frames).
    let mut cons: Vec<(Constraint, Geom, Geom)> = Vec::new();
    for c in asm.constraints.iter().filter(|c| !c.suppressed) {
        match (local(&c.a), local(&c.b)) {
            (Ok(a), Ok(b)) if c.kind.is_joint() => {
                cons.push((c.clone(), a.as_frame(), b.as_frame()))
            }
            (Ok(a), Ok(b)) => cons.push((c.clone(), a, b)),
            (Err(e), _) | (_, Err(e)) => out.sick.push((c.id, e)),
        }
    }
    // Unknown blocks: components used by a constraint, not grounded.
    let mut blocks: Vec<u32> = Vec::new();
    for (c, _, _) in &cons {
        for id in [c.a.occurrence(), c.b.occurrence()] {
            let free = asm.find(id).is_some_and(|o| !o.grounded);
            if free && !blocks.contains(&id) {
                blocks.push(id);
            }
        }
    }
    let mut place: HashMap<u32, Transform3> = asm
        .occurrences
        .iter()
        .map(|o| (o.id, o.transform))
        .collect();
    // The assembly's own origin (`relation::ASSEMBLY`) never moves.
    place.insert(super::relation::ASSEMBLY, Transform3::identity());
    // Column weights: a dragged component is expensive to move.
    let weight: Vec<Real> = blocks
        .iter()
        .flat_map(|id| {
            let w = if Some(*id) == heavy { 1e-3 } else { 1.0 };
            [w; 6]
        })
        .collect();
    let mut r = run(&cons, &blocks, &weight, &mut place);
    // Joint limits: a value past one is held there and solved again.
    for _ in 0..3 {
        let mut held = false;
        for (c, a, b) in cons.iter_mut() {
            if !c.kind.is_joint() || c.locked || (c.min.is_none() && c.max.is_none()) {
                continue;
            }
            let (Some(ta), Some(tb)) = (place.get(&c.a.occurrence()), place.get(&c.b.occurrence()))
            else {
                continue;
            };
            let fa = super::joint::frame_of(&a.moved(ta));
            let fb = super::joint::frame_of(&b.moved(tb));
            let Some(v) = super::joint::measure(c, &fa, &fb) else {
                continue;
            };
            let lo = c.min.unwrap_or(Real::NEG_INFINITY);
            let hi = c.max.unwrap_or(Real::INFINITY);
            if v < lo - 1e-9 || v > hi + 1e-9 {
                c.locked = true;
                c.value = v.clamp(lo, hi);
                held = true;
            }
        }
        if !held {
            break;
        }
        r = run(&cons, &blocks, &weight, &mut place);
    }
    let n = blocks.len() * 6;
    out.residual = r.iter().fold(0.0, |m: Real, v| m.max(v.abs()));
    out.converged = out.residual < 1e-6;
    // Degrees of freedom of each block, the others held (not while
    // dragging: nobody reads them then, and they cost a Jacobian).
    if n > 0 && heavy.is_none() {
        let j = jacobian(&cons, &blocks, &place, &r);
        for (k, id) in blocks.iter().enumerate() {
            let sub = j.columns(6 * k, 6).into_owned();
            let rank = if sub.nrows() == 0 {
                0
            } else {
                let sv = sub.svd(false, false).singular_values;
                let top = sv.iter().cloned().fold(0.0, Real::max);
                sv.iter().filter(|&&s| s > 1e-6 * top.max(1e-9)).count()
            };
            out.dof.insert(*id, 6 - rank.min(6));
        }
    }
    for o in &asm.occurrences {
        if !o.grounded && !out.dof.contains_key(&o.id) {
            out.dof.insert(o.id, 6);
        }
    }
    out.transforms = blocks
        .iter()
        .filter_map(|id| place.get(id).map(|t| (*id, *t)))
        .collect();
    out
}

fn eval(cons: &[(Constraint, Geom, Geom)], place: &HashMap<u32, Transform3>) -> DVector<Real> {
    let mut r = Vec::new();
    for (c, a, b) in cons {
        let (Some(ta), Some(tb)) = (place.get(&c.a.occurrence()), place.get(&c.b.occurrence()))
        else {
            continue;
        };
        let _ = residuals(c, &a.moved(ta), &b.moved(tb), &mut r);
    }
    DVector::from_vec(r)
}

/// Moves each block by its six unknowns (translation, turn about its own
/// origin scaled by `L`).
fn apply(blocks: &[u32], place: &mut HashMap<u32, Transform3>, x: &DVector<Real>) {
    for (k, id) in blocks.iter().enumerate() {
        let Some(t) = place.get_mut(id) else {
            continue;
        };
        let v = |i: usize| x[6 * k + i];
        let c = t.translation.vector;
        let rot = Transform3::rotation(Vector3::new(v(3), v(4), v(5)) / L);
        *t = Transform3::translation(v(0), v(1), v(2))
            * Transform3::translation(c.x, c.y, c.z)
            * rot
            * Transform3::translation(-c.x, -c.y, -c.z)
            * *t;
    }
}

fn jacobian(
    cons: &[(Constraint, Geom, Geom)],
    blocks: &[u32],
    place: &HashMap<u32, Transform3>,
    r0: &DVector<Real>,
) -> DMatrix<Real> {
    let n = blocks.len() * 6;
    let mut j = DMatrix::zeros(r0.len(), n);
    let h = 1e-7;
    for col in 0..n {
        let mut p = place.clone();
        let mut x = DVector::zeros(n);
        x[col] = h;
        apply(blocks, &mut p, &x);
        let r = eval(cons, &p);
        for row in 0..r0.len().min(r.len()) {
            j[(row, col)] = (r[row] - r0[row]) / h;
        }
    }
    j
}

/// Levenberg iterations in minimum-norm form; returns the residuals left.
fn run(
    cons: &[(Constraint, Geom, Geom)],
    blocks: &[u32],
    weight: &[Real],
    place: &mut HashMap<u32, Transform3>,
) -> DVector<Real> {
    let norm = |r: &DVector<Real>| r.iter().fold(0.0, |m: Real, v| m.max(v.abs()));
    let mut r = eval(cons, place);
    if blocks.is_empty() {
        return r;
    }
    let mut lambda = 1e-6;
    let w = DMatrix::from_diagonal(&DVector::from_vec(weight.to_vec()));
    for _ in 0..200 {
        if norm(&r) < 1e-10 {
            break;
        }
        let j = jacobian(cons, blocks, place, &r);
        let jw = &j * &w;
        let mut improved = false;
        for _ in 0..12 {
            let a = &jw * jw.transpose() + DMatrix::identity(r.len(), r.len()) * lambda;
            let Some(y) = a.lu().solve(&r) else {
                lambda *= 10.0;
                continue;
            };
            let mut dx = -(jw.transpose() * y);
            for (i, w) in weight.iter().enumerate() {
                dx[i] *= w;
            }
            let mut p = place.clone();
            apply(blocks, &mut p, &dx);
            let r1 = eval(cons, &p);
            if r1.norm() < r.norm() {
                *place = p;
                r = r1;
                lambda = (lambda / 3.0).max(1e-12);
                improved = true;
                break;
            }
            lambda *= 4.0;
        }
        if !improved {
            break;
        }
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assembly::relation::{Constraint, ConstraintKind};
    use crate::document::{DocRef, DocumentId};
    use crate::kernel::Point3;

    /// Two blocks: geometry given directly (part coordinates): a 20 mm cube.
    fn cube_geom(r: &GeomRef) -> Result<Geom, String> {
        if let GeomRef::Origin { item, .. } = r {
            return Ok(item.geom());
        }
        let GeomRef::Face { name, .. } = r else {
            return circle_geom(r);
        };
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

    /// A hole's rim on the top (Ø6 at the centre).
    fn circle_geom(_r: &GeomRef) -> Result<Geom, String> {
        Ok(Geom::Circle {
            c: Point3::new(10.0, 10.0, 20.0),
            n: Vector3::z(),
            r: 3.0,
        })
    }

    fn face(o: u32, name: &str) -> GeomRef {
        GeomRef::Face {
            occurrence: o,
            name: name.into(),
            hint: [0.0; 3],
        }
    }

    fn rim(o: u32) -> GeomRef {
        GeomRef::Edge {
            occurrence: o,
            edge: crate::part::EdgeRef {
                faces: ["Top".into(), "Hole".into()],
                hint: [0.0; 3],
            },
        }
    }

    fn two_cubes() -> AssemblyDocument {
        let mut a = AssemblyDocument::default();
        let r = DocRef::local(DocumentId::new_random());
        a.place(r, "Cube", Transform3::identity(), true);
        // The second one somewhere, turned a little.
        a.place(
            r,
            "Cube",
            Transform3::new(Vector3::new(50.0, 30.0, 7.0), Vector3::new(0.2, -0.1, 0.3)),
            false,
        );
        a
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
        }
    }

    fn applied(a: &mut AssemblyDocument, s: &Solved) {
        for (id, t) in &s.transforms {
            if let Some(o) = a.find_mut(*id) {
                o.transform = *t;
            }
        }
    }

    #[test]
    fn mate_leaves_three_freedoms_and_three_mates_none() {
        let mut a = two_cubes();
        a.add_constraint(con(
            ConstraintKind::Mate,
            face(1, "Top"),
            face(2, "Bottom"),
            0.0,
        ));
        let s = solve(&a, &cube_geom, None);
        assert!(s.converged, "{}", s.residual);
        assert_eq!(s.dof[&2], 3, "two moves in the plane, one turn");
        applied(&mut a, &s);
        // Cube 2's bottom now lies on cube 1's top (z = 20), facing down.
        let t = a.find(2).unwrap().transform;
        let bottom = t * Point3::new(10.0, 10.0, 0.0);
        assert!((bottom.z - 20.0).abs() < 1e-6, "{bottom}");
        assert!(((t * -Vector3::z()) + Vector3::z()).norm() < 1e-6);
        // Flush sides: left on left, front on front.
        a.add_constraint(con(
            ConstraintKind::Flush,
            face(1, "Left"),
            face(2, "Left"),
            0.0,
        ));
        a.add_constraint(con(
            ConstraintKind::Flush,
            face(1, "Front"),
            face(2, "Front"),
            5.0,
        ));
        let s = solve(&a, &cube_geom, None);
        assert!(s.converged, "{}", s.residual);
        assert_eq!(s.dof[&2], 0);
        applied(&mut a, &s);
        let t = a.find(2).unwrap().transform;
        let corner = t * Point3::origin();
        // Front flush at offset 5 along the front's normal (−y): y = −5.
        assert!(
            (corner - Point3::new(0.0, -5.0, 20.0)).norm() < 1e-6,
            "{corner}"
        );
        assert_eq!(a.constraints[1].name, "Flush:1");
    }

    #[test]
    fn insert_keeps_the_turn_free_and_conflicts_do_not_converge() {
        let mut a = two_cubes();
        a.add_constraint(Constraint {
            aligned: false,
            ..con(ConstraintKind::Insert, rim(1), rim(2), 0.0)
        });
        let s = solve(&a, &cube_geom, None);
        assert!(s.converged, "{}", s.residual);
        assert_eq!(s.dof[&2], 1, "the turn about the axis");
        applied(&mut a, &s);
        let t = a.find(2).unwrap().transform;
        // Opposed: cube 2's rim normal points down onto cube 1's rim.
        let c = t * Point3::new(10.0, 10.0, 20.0);
        assert!((c - Point3::new(10.0, 10.0, 20.0)).norm() < 1e-6, "{c}");
        // A Mate of the same faces at another offset cannot hold with it.
        a.add_constraint(con(
            ConstraintKind::Mate,
            face(1, "Top"),
            face(2, "Top"),
            3.0,
        ));
        let s = solve(&a, &cube_geom, None);
        assert!(!s.converged);
    }

    #[test]
    fn angle_and_missing_geometry() {
        let mut a = two_cubes();
        a.add_constraint(con(
            ConstraintKind::Mate,
            face(1, "Top"),
            face(2, "Bottom"),
            0.0,
        ));
        a.add_constraint(con(
            ConstraintKind::Angle,
            face(1, "Right"),
            face(2, "Right"),
            30f64.to_radians(),
        ));
        a.add_constraint(con(
            ConstraintKind::Mate,
            face(1, "Gone"),
            face(2, "Left"),
            0.0,
        ));
        let s = solve(&a, &cube_geom, None);
        assert!(s.converged, "{}", s.residual);
        assert_eq!(s.sick.len(), 1);
        assert!(s.sick[0].1.contains("Gone"));
        assert_eq!(s.dof[&2], 2);
        applied(&mut a, &s);
        let t = a.find(2).unwrap().transform;
        let cos = (t * Vector3::x()).dot(&Vector3::x());
        assert!((cos - 30f64.to_radians().cos()).abs() < 1e-6);
    }

    /// The origin planes of a component flush with the assembly's own
    /// (occurrence `ASSEMBLY`): it lands on the origin, square to it.
    #[test]
    fn origin_planes_flush_with_the_assembly_origin() {
        use crate::assembly::relation::{ASSEMBLY, OriginItem};
        let mut a = two_cubes();
        let o = |occurrence, item| GeomRef::Origin { occurrence, item };
        for item in [
            OriginItem::XyPlane,
            OriginItem::XzPlane,
            OriginItem::YzPlane,
        ] {
            a.add_constraint(con(
                ConstraintKind::Flush,
                o(ASSEMBLY, item),
                o(2, item),
                0.0,
            ));
        }
        let s = solve(&a, &cube_geom, None);
        assert!(s.converged, "{}", s.residual);
        assert_eq!(s.dof[&2], 0);
        applied(&mut a, &s);
        let t = a.find(2).unwrap().transform;
        assert!((t * Point3::new(1.0, 2.0, 3.0) - Point3::new(1.0, 2.0, 3.0)).norm() < 1e-6);
        // A component's centre point on the assembly's Z axis: a Mate of a
        // point and an axis.
        let mut a = two_cubes();
        a.add_constraint(con(
            ConstraintKind::Mate,
            o(ASSEMBLY, OriginItem::ZAxis),
            o(2, OriginItem::Center),
            0.0,
        ));
        let s = solve(&a, &cube_geom, None);
        assert!(s.converged);
        assert_eq!(s.dof[&2], 4, "it slides along Z and turns freely");
        applied(&mut a, &s);
        let c = a.find(2).unwrap().transform * Point3::origin();
        assert!(c.x.abs() < 1e-6 && c.y.abs() < 1e-6, "{c}");
    }

    #[test]
    fn a_dragged_component_pulls_its_partner() {
        // Both free (the first grounded one is freed): mating them and
        // dragging the second moves the first along.
        let mut a = two_cubes();
        a.find_mut(1).unwrap().grounded = false;
        a.add_constraint(con(
            ConstraintKind::Mate,
            face(1, "Top"),
            face(2, "Bottom"),
            0.0,
        ));
        let s = solve(&a, &cube_geom, None);
        applied(&mut a, &s);
        // Drag cube 2 up by 10: the first follows (it is the light one).
        let before1 = a.find(1).unwrap().transform;
        let o2 = a.find_mut(2).unwrap();
        o2.transform = Transform3::translation(0.0, 0.0, 10.0) * o2.transform;
        let z2 = (a.find(2).unwrap().transform * Point3::origin()).z;
        let s = solve(&a, &cube_geom, Some(2));
        assert!(s.converged);
        applied(&mut a, &s);
        let z2_after = (a.find(2).unwrap().transform * Point3::origin()).z;
        let moved1 =
            (a.find(1).unwrap().transform.translation.vector - before1.translation.vector).norm();
        assert!(
            (z2_after - z2).abs() < 0.1,
            "the dragged one stays: {z2} {z2_after}"
        );
        assert!(moved1 > 9.0, "the other follows: {moved1}");
    }
}
