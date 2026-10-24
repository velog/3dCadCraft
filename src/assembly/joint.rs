//! Joints, step A3 (`docs/spec/3.1-assemblies.md` §3.3): a joint connects
//! two **joint origins** — a frame (point, z axis, x axis) on each
//! component — and leaves the motion of its kind free. After SolidCraft's
//! joint kinds (`crates/doc/src/joints.rs`, MIT OR Apache-2.0), but as
//! residuals of our simultaneous solver (loops of joints solve too).
//!
//! The second origin faces the first (z axes opposed) unless flipped; its
//! x axis follows the first one's where the kind locks the turn.

use super::relation::{Constraint, ConstraintKind, Geom};
use crate::kernel::{Point3, Real, Vector3};

/// A joint origin in part or assembly coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    /// The origin.
    pub p: Point3,
    /// Unit z axis (out of the face, along the axis).
    pub z: Vector3,
    /// Unit x axis (square to z).
    pub x: Vector3,
}

/// A unit vector square to `z`: the part's X projected, else its Y — so a
/// face's frame turns with the part (it is computed in part coordinates).
fn square_x(z: Vector3) -> Vector3 {
    let pick = if z.x.abs() < 0.9 {
        Vector3::x()
    } else {
        Vector3::y()
    };
    (pick - z * z.dot(&pick)).normalize()
}

/// The joint origin of picked geometry (part coordinates): a face's centre
/// and normal, a circle's centre and normal, a point on an axis and the
/// axis, a point with the part's Z.
pub fn frame_of(g: &Geom) -> Frame {
    let (p, z) = match *g {
        Geom::Plane { p, n } => (p, n),
        Geom::Circle { c, n, .. } => (c, n),
        Geom::Axis { p, d } => (p, d),
        Geom::Point { p } | Geom::Sphere { c: p, .. } => (p, Vector3::z()),
        Geom::Cylinder { p, d, .. } => (p, d),
        Geom::Frame { p, z, x } => return Frame { p, z, x },
    };
    let z = z.try_normalize(1e-12).unwrap_or(Vector3::z());
    Frame {
        p,
        z,
        x: square_x(z),
    }
}

impl ConstraintKind {
    /// Whether this is a joint (connects joint origins).
    pub fn is_joint(self) -> bool {
        matches!(
            self,
            ConstraintKind::Rigid
                | ConstraintKind::Rotational
                | ConstraintKind::Slider
                | ConstraintKind::Cylindrical
                | ConstraintKind::Planar
                | ConstraintKind::Ball
        )
    }

    /// What the joint's value is: an angle, a distance, or none.
    pub fn joint_value(self) -> Option<&'static str> {
        match self {
            ConstraintKind::Rotational => Some("Angle"),
            ConstraintKind::Slider | ConstraintKind::Cylindrical => Some("Distance"),
            _ => None,
        }
    }
}

/// The joint's value as the components stand: the turn of the second
/// origin's x about the first z (Rotational), or the slide along it.
pub fn measure(c: &Constraint, a: &Frame, b: &Frame) -> Option<Real> {
    let ya = a.z.cross(&a.x);
    match c.kind {
        ConstraintKind::Rotational => Some(b.x.dot(&ya).atan2(b.x.dot(&a.x))),
        ConstraintKind::Slider | ConstraintKind::Cylindrical => Some(a.z.dot(&(b.p - a.p))),
        _ => None,
    }
}

/// The residuals of joint `c` between the world frames `a` and `b`.
pub fn residuals(c: &Constraint, a: &Frame, b: &Frame, out: &mut Vec<Real>) {
    use ConstraintKind::*;
    let vec3 = |out: &mut Vec<Real>, v: Vector3| out.extend([v.x, v.y, v.z]);
    // Facing (z opposed) unless flipped.
    let s = if c.aligned { -1.0 } else { 1.0 };
    let z = b.z + a.z * s;
    let lateral = {
        let w = b.p - a.p;
        w - a.z * a.z.dot(&w)
    };
    let ya = a.z.cross(&a.x);
    let lock_angle = |out: &mut Vec<Real>, v: Real| {
        out.push(b.x.dot(&a.x) - v.cos());
        out.push(b.x.dot(&ya) - v.sin());
    };
    match c.kind {
        Rigid => {
            vec3(out, b.p - a.p);
            vec3(out, z);
            lock_angle(out, c.value);
        }
        Rotational => {
            vec3(out, b.p - a.p);
            vec3(out, z);
            if c.locked {
                lock_angle(out, c.value);
            }
        }
        Slider => {
            vec3(out, z);
            lock_angle(out, 0.0);
            vec3(out, lateral);
            if c.locked {
                out.push(a.z.dot(&(b.p - a.p)) - c.value);
            }
        }
        Cylindrical => {
            vec3(out, z);
            vec3(out, lateral);
            if c.locked {
                out.push(a.z.dot(&(b.p - a.p)) - c.value);
            }
        }
        Planar => {
            vec3(out, z);
            out.push(a.z.dot(&(b.p - a.p)));
        }
        Ball => vec3(out, b.p - a.p),
        Mate | Flush | Angle | Insert | Tangent | Symmetry => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assembly::relation::GeomRef;
    use crate::assembly::solve;
    use crate::document::{AssemblyDocument, DocRef, DocumentId};
    use crate::kernel::Transform3;

    /// Joint origins: the top of a 20 mm cube (face centre) and its
    /// bottom.
    fn geom(r: &GeomRef) -> Result<Geom, String> {
        let GeomRef::Face { name, .. } = r else {
            return Err("face".into());
        };
        Ok(match name.as_str() {
            "Top" => Geom::Plane {
                p: Point3::new(10.0, 10.0, 20.0),
                n: Vector3::z(),
            },
            _ => Geom::Plane {
                p: Point3::new(10.0, 10.0, 0.0),
                n: -Vector3::z(),
            },
        })
    }

    fn face(o: u32, n: &str) -> GeomRef {
        GeomRef::Face {
            occurrence: o,
            name: n.into(),
            hint: [0.0; 3],
        }
    }

    fn joint(kind: ConstraintKind) -> AssemblyDocument {
        let mut a = AssemblyDocument::default();
        let r = DocRef::local(DocumentId::new_random());
        a.place(r, "Cube", Transform3::identity(), true);
        a.place(
            r,
            "Cube",
            Transform3::new(Vector3::new(40.0, 25.0, 9.0), Vector3::new(0.3, 0.1, -0.2)),
            false,
        );
        a.add_constraint(Constraint {
            id: 0,
            name: String::new(),
            kind,
            a: face(1, "Top"),
            b: face(2, "Bottom"),
            value: 0.0,
            expr: None,
            aligned: false,
            suppressed: false,
            locked: false,
            min: None,
            max: None,
            resting: false,
            plane: None,
        });
        a
    }

    #[test]
    fn each_kind_leaves_its_freedoms() {
        for (kind, dof) in [
            (ConstraintKind::Rigid, 0),
            (ConstraintKind::Rotational, 1),
            (ConstraintKind::Slider, 1),
            (ConstraintKind::Cylindrical, 2),
            (ConstraintKind::Planar, 3),
            (ConstraintKind::Ball, 3),
        ] {
            let a = joint(kind);
            let s = solve::solve(&a, &geom, None);
            assert!(s.converged, "{kind:?}: {}", s.residual);
            assert_eq!(s.dof[&2], dof, "{kind:?}");
            if kind == ConstraintKind::Rigid {
                // Stacked: the second cube's bottom centre on the top centre.
                let t = s.transforms[0].1;
                let c = t * Point3::new(10.0, 10.0, 0.0);
                assert!((c - Point3::new(10.0, 10.0, 20.0)).norm() < 1e-6, "{c}");
            }
        }
    }

    #[test]
    fn a_locked_angle_and_limits() {
        let mut a = joint(ConstraintKind::Rotational);
        a.constraints[0].locked = true;
        a.constraints[0].value = 30f64.to_radians();
        let s = solve::solve(&a, &geom, None);
        assert!(s.converged);
        assert_eq!(s.dof[&2], 0);
        let t = s.transforms[0].1;
        let x = t * Vector3::x();
        assert!((x.y.atan2(x.x) - 30f64.to_radians()).abs() < 1e-6, "{x}");
        // A slider limited to 0..5 mm, dragged to 12 mm: it stops at 5.
        let mut a = joint(ConstraintKind::Slider);
        a.constraints[0].min = Some(0.0);
        a.constraints[0].max = Some(5.0);
        let s = solve::solve(&a, &geom, None);
        let mut b = a.clone();
        for (id, t) in &s.transforms {
            b.find_mut(*id).unwrap().transform = *t;
        }
        // The bottom centre pulled 12 mm up the slide.
        let t = b.find(2).unwrap().transform;
        let p = Point3::new(10.0, 10.0, 0.0);
        let drag = solve::Drag {
            occurrence: 2,
            local: p,
            target: t * p + Vector3::new(0.0, 0.0, 12.0),
        };
        let s = solve::solve(&b, &geom, Some(&drag));
        assert!(s.converged, "{}", s.residual);
        let z = (s.transforms[0].1 * Point3::new(10.0, 10.0, 0.0)).z;
        assert!((z - 25.0).abs() < 1e-6, "slid to {z}");
    }
}
