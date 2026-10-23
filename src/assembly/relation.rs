//! Assembly constraints, step A2 (`docs/spec/3.1-assemblies.md` §3.2):
//! Mate (with its Flush solution), Angle and Insert between faces and
//! edges of two occurrences, referenced by persistent names in the parts
//! (rules 2 and 8), and their geometry in assembly coordinates.

use serde::{Deserialize, Serialize};

use crate::document::{AssemblyDocument, Occurrence, Project};
use crate::kernel::{BRep, Curve, Point3, Real, Sense, Surface, Transform3, Vector3};
use crate::part::EdgeRef;

/// What a constraint picks on an occurrence: a face or an edge of its part.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum GeomRef {
    /// A face by persistent name; `hint` (part coordinates) picks the piece
    /// when a Boolean split the name and places a plane's point.
    Face {
        /// The occurrence's id.
        occurrence: u32,
        /// Persistent face name (`Box1:Top`).
        name: String,
        /// A point on it when it was picked (part coordinates).
        hint: [Real; 3],
    },
    /// An edge by its two faces' names.
    Edge {
        /// The occurrence's id.
        occurrence: u32,
        /// The edge.
        edge: EdgeRef,
    },
}

impl GeomRef {
    /// The occurrence it belongs to.
    pub fn occurrence(&self) -> u32 {
        match self {
            GeomRef::Face { occurrence, .. } | GeomRef::Edge { occurrence, .. } => *occurrence,
        }
    }

    /// Short text for the browser and the panel (`Box1:Top`, `Box1:Top|Box1:Front`).
    pub fn label(&self) -> String {
        match self {
            GeomRef::Face { name, .. } => name.clone(),
            GeomRef::Edge { edge, .. } => format!("{} | {}", edge.faces[0], edge.faces[1]),
        }
    }
}

/// The kinds of constraint (Inventor's Place Constraint, Assembly tab).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConstraintKind {
    /// Faces face to face (normals opposed) at an offset; axes on one line;
    /// a plane and an axis: the axis in the plane.
    Mate,
    /// Mate's Flush solution: faces side by side, normals the same way.
    Flush,
    /// The angle between two faces' normals or two edges (undirected).
    Angle,
    /// A circular edge into another: axes on one line and the circles'
    /// planes mated (Opposed) or flush (Aligned) at an offset; the turn
    /// about the axis stays free.
    Insert,
}

impl ConstraintKind {
    /// English name (browser, names `Mate:1`).
    pub fn label(self) -> &'static str {
        match self {
            ConstraintKind::Mate => "Mate",
            ConstraintKind::Flush => "Flush",
            ConstraintKind::Angle => "Angle",
            ConstraintKind::Insert => "Insert",
        }
    }
}

/// One assembly constraint.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Constraint {
    /// Stable id inside the assembly (never reused).
    pub id: u32,
    /// Display name (`Mate:1`).
    pub name: String,
    /// What it holds.
    pub kind: ConstraintKind,
    /// First selection (its component defines the positive offset side).
    pub a: GeomRef,
    /// Second selection.
    pub b: GeomRef,
    /// Offset (mm) or angle (radians).
    pub value: Real,
    /// The value's equation when it names parameters or units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expr: Option<String>,
    /// Insert: Aligned (planes flush) instead of Opposed (mated).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub aligned: bool,
    /// Suppressed: kept, but not solved.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub suppressed: bool,
}

impl Constraint {
    /// Whether it uses occurrence `id`.
    pub fn uses(&self, id: u32) -> bool {
        self.a.occurrence() == id || self.b.occurrence() == id
    }
}

impl AssemblyDocument {
    /// The next free constraint name `Kind:k`.
    pub fn next_constraint_name(&self, kind: ConstraintKind) -> String {
        let base = kind.label();
        let k = self
            .constraints
            .iter()
            .filter_map(|c| {
                c.name
                    .strip_prefix(base)?
                    .strip_prefix(':')?
                    .parse::<u32>()
                    .ok()
            })
            .max()
            .unwrap_or(0);
        format!("{base}:{}", k + 1)
    }

    /// Adds `c` with a new id and name; returns the id.
    pub fn add_constraint(&mut self, mut c: Constraint) -> u32 {
        c.id = self.constraints.iter().map(|c| c.id).max().unwrap_or(0) + 1;
        c.name = self.next_constraint_name(c.kind);
        let id = c.id;
        self.constraints.push(c);
        id
    }

    /// The constraint with id `id`.
    pub fn constraint(&self, id: u32) -> Option<&Constraint> {
        self.constraints.iter().find(|c| c.id == id)
    }

    /// The constraint with id `id`, to change.
    pub fn constraint_mut(&mut self, id: u32) -> Option<&mut Constraint> {
        self.constraints.iter_mut().find(|c| c.id == id)
    }
}

/// Picked geometry in part (or, moved, assembly) coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Geom {
    /// A plane: a point and the outward normal (away from the material).
    Plane {
        /// A point on it (the pick's point projected).
        p: Point3,
        /// Unit outward normal.
        n: Vector3,
    },
    /// A line or an axis (straight edge, cylinder or cone axis).
    Axis {
        /// A point on it.
        p: Point3,
        /// Unit direction.
        d: Vector3,
    },
    /// A circle: centre, the normal of the face it bounds (outward) and the
    /// radius. Its axis is the line through `c` along `n`.
    Circle {
        /// Centre.
        c: Point3,
        /// Unit normal (outward of its planar face when it has one).
        n: Vector3,
        /// Radius.
        r: Real,
    },
}

impl Geom {
    /// Moved by `t` (part → assembly coordinates).
    pub fn moved(self, t: &Transform3) -> Geom {
        match self {
            Geom::Plane { p, n } => Geom::Plane { p: t * p, n: t * n },
            Geom::Axis { p, d } => Geom::Axis { p: t * p, d: t * d },
            Geom::Circle { c, n, r } => Geom::Circle {
                c: t * c,
                n: t * n,
                r,
            },
        }
    }

    /// The direction for an angle: a plane's normal, an axis, a circle's
    /// normal.
    pub fn direction(&self) -> Vector3 {
        match *self {
            Geom::Plane { n, .. } | Geom::Circle { n, .. } => n,
            Geom::Axis { d, .. } => d,
        }
    }

    /// The line of an axis or a circle (point, direction).
    pub fn line(&self) -> Option<(Point3, Vector3)> {
        match *self {
            Geom::Axis { p, d } => Some((p, d)),
            Geom::Circle { c, n, .. } => Some((c, n)),
            Geom::Plane { .. } => None,
        }
    }
}

/// The outward normal of a planar face.
fn face_normal(body: &BRep, f: crate::kernel::FaceId) -> Option<(Point3, Vector3)> {
    let face = body.face(f)?;
    let Some(Surface::Plane(pl)) = body.surface(face.surface) else {
        return None;
    };
    let n = pl.normal.into_inner();
    Some((
        pl.origin,
        match face.sense {
            Sense::Same => n,
            Sense::Reversed => -n,
        },
    ))
}

/// The geometry `r` names in `body` (part coordinates).
pub fn resolve_local(body: &BRep, r: &GeomRef) -> Result<Geom, String> {
    match r {
        GeomRef::Face { name, hint, .. } => {
            let hint = Point3::from(*hint);
            // The piece of that name nearest the pick.
            let f = body
                .faces()
                .filter(|(_, f)| f.name.as_deref() == Some(name.as_str()))
                .map(|(id, f)| {
                    let d = f.mesh.as_ref().map_or(0.0, |m| {
                        m.positions
                            .iter()
                            .map(|p| (p - hint).norm())
                            .fold(Real::INFINITY, Real::min)
                    });
                    (id, d)
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(id, _)| id)
                .ok_or_else(|| format!("the face {name} no longer exists"))?;
            let face = body.face(f).ok_or("the face no longer exists")?;
            let exact = face.facet_of.unwrap_or(face.surface);
            match body.surface(exact) {
                Some(Surface::Plane(_)) => {
                    let (o, n) = face_normal(body, f).ok_or("the face is not planar")?;
                    let p = hint - n * (hint - o).dot(&n);
                    Ok(Geom::Plane { p, n })
                }
                Some(Surface::Cylinder(c)) => Ok(Geom::Axis {
                    p: c.origin,
                    d: c.axis.into_inner(),
                }),
                Some(Surface::Cone(c)) => Ok(Geom::Axis {
                    p: c.origin,
                    d: c.axis.into_inner(),
                }),
                _ => Err(format!("the face {name} is neither planar nor round")),
            }
        }
        GeomRef::Edge { edge, .. } => {
            let e = crate::part::draft::edge_of(body, edge).ok_or_else(|| {
                format!(
                    "the edge between {} and {} no longer exists",
                    edge.faces[0], edge.faces[1]
                )
            })?;
            let x = body.edge(e).ok_or("the edge no longer exists")?;
            match body.curve(x.curve) {
                Some(Curve::Line(l)) => Ok(Geom::Axis {
                    p: l.origin,
                    d: l.direction.into_inner(),
                }),
                Some(Curve::Circle(c)) => {
                    // The normal of the planar face it bounds, outwards
                    // (a hole's rim: out of the hole).
                    let n = body
                        .edge_faces(e)
                        .iter()
                        .find_map(|&f| face_normal(body, f))
                        .map_or(c.normal.into_inner(), |(_, n)| n);
                    Ok(Geom::Circle {
                        c: c.center,
                        n,
                        r: c.radius,
                    })
                }
                _ => Err("the edge is neither straight nor circular".into()),
            }
        }
    }
}

/// The local geometry of `r` in the assembly `asm` of `project` (parts of
/// the same project only; the GUI also reaches linked part files).
pub fn resolve(project: &Project, asm: &AssemblyDocument, r: &GeomRef) -> Result<Geom, String> {
    let o: &Occurrence = asm
        .find(r.occurrence())
        .ok_or("the component no longer exists")?;
    let part =
        super::part_of(project, o.component).ok_or_else(|| format!("{} shows no part", o.name))?;
    resolve_local(&part.body, r).map_err(|e| format!("{}: {e}", o.name))
}

/// The residuals of constraint `c` for its two world geometries: zero when
/// it holds (some are dependent: `nA + nB` has rank 2).
pub fn residuals(c: &Constraint, a: &Geom, b: &Geom, out: &mut Vec<Real>) -> Result<(), String> {
    use ConstraintKind::*;
    let d = c.value;
    let vec3 = |out: &mut Vec<Real>, v: Vector3| out.extend([v.x, v.y, v.z]);
    // The offset of point `q` from the line (p, u), as a vector.
    let off_line = |p: Point3, u: Vector3, q: Point3| {
        let w = q - p;
        w - u * u.dot(&w)
    };
    match (c.kind, a, b) {
        (Mate | Flush, Geom::Plane { p: pa, n: na }, Geom::Plane { p: pb, n: nb }) => {
            let s = if c.kind == Mate { 1.0 } else { -1.0 };
            vec3(out, na + nb * s);
            out.push(na.dot(&(pb - pa)) - d);
        }
        (Mate | Flush, Geom::Plane { p, n }, other)
        | (Mate | Flush, other, Geom::Plane { p, n }) => {
            match *other {
                // A circle's centre on the plane (at the offset).
                Geom::Circle { c: q, .. } => out.push(n.dot(&(q - p)) - d),
                // The axis in the plane (at the offset).
                Geom::Axis { p: q, d: u } => {
                    out.push(n.dot(&u));
                    out.push(n.dot(&(q - p)) - d);
                }
                Geom::Plane { .. } => return Err("two planes".into()),
            }
        }
        (Mate | Flush, x, y) => {
            // Two axes (or circles' axes) on one line.
            let (pa, ua) = x.line().ok_or("not an axis")?;
            let (pb, ub) = y.line().ok_or("not an axis")?;
            vec3(out, ua.cross(&ub));
            vec3(out, off_line(pa, ua, pb));
        }
        (Angle, x, y) => {
            out.push(x.direction().dot(&y.direction()) - d.cos());
        }
        (Insert, Geom::Circle { c: ca, n: na, .. }, Geom::Circle { c: cb, n: nb, .. }) => {
            let s = if c.aligned { -1.0 } else { 1.0 };
            vec3(out, na + nb * s);
            vec3(out, (cb - ca) - na * d);
        }
        (Insert, _, _) => return Err("Insert needs two circular edges".into()),
    }
    Ok(())
}

/// What the current positions say for a new constraint between `a` and `b`
/// (Predict Offset and Orientation): Mate or Flush from the normals, and
/// the offset or angle as it is now.
pub fn predict(kind: ConstraintKind, a: &Geom, b: &Geom) -> (ConstraintKind, Real) {
    use ConstraintKind::*;
    match (kind, a, b) {
        (Mate | Flush, Geom::Plane { p: pa, n: na }, Geom::Plane { p: pb, n: nb }) => {
            let kind = if na.dot(nb) > 0.0 { Flush } else { Mate };
            (kind, na.dot(&(pb - pa)))
        }
        (
            Mate | Flush,
            Geom::Plane { p, n },
            Geom::Circle { c: q, .. } | Geom::Axis { p: q, .. },
        )
        | (
            Mate | Flush,
            Geom::Circle { c: q, .. } | Geom::Axis { p: q, .. },
            Geom::Plane { p, n },
        ) => (Mate, n.dot(&(q - p))),
        (Angle, x, y) => (
            Angle,
            x.direction().dot(&y.direction()).clamp(-1.0, 1.0).acos(),
        ),
        (Insert, Geom::Circle { c: ca, n: na, .. }, Geom::Circle { c: cb, .. }) => {
            (Insert, na.dot(&(cb - ca)))
        }
        (k, _, _) => (k, 0.0),
    }
}

/// Which geometry each kind takes, for the panel's message.
pub fn accepts(kind: ConstraintKind, g: &Geom) -> bool {
    match kind {
        ConstraintKind::Mate | ConstraintKind::Flush => true,
        ConstraintKind::Angle => !matches!(g, Geom::Circle { .. }),
        ConstraintKind::Insert => matches!(g, Geom::Circle { .. }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::part::{HoleCenter, HoleFeature, HoleSpec, PartDocument, Termination};

    /// A real part: the box's top is a plane facing up at z = 20; the rim of
    /// a through hole on it is a circle whose normal points out of the hole.
    #[test]
    fn faces_and_rims_of_a_part() {
        let mut p = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
        let (k, ids) = p
            .create_hole_sketch("Box1:Top", &[Point3::new(20.0, 15.0, 20.0)])
            .unwrap();
        let mut s = HoleSpec::simple(6.0, 10.0);
        s.termination = Termination::ThroughAll;
        let mut def = HoleFeature::new(s);
        def.sketch = p.sketches[k].id.clone();
        def.centers = ids.into_iter().map(HoleCenter::Point).collect();
        p.add_hole(def).unwrap();
        let top = GeomRef::Face {
            occurrence: 1,
            name: "Box1:Top".into(),
            hint: [5.0, 5.0, 20.0],
        };
        let Geom::Plane { p: q, n } = resolve_local(&p.body, &top).unwrap() else {
            panic!("not a plane");
        };
        assert!((n - Vector3::z()).norm() < 1e-9 && (q.z - 20.0).abs() < 1e-9);
        let bottom = GeomRef::Face {
            occurrence: 1,
            name: "Box1:Bottom".into(),
            hint: [5.0, 5.0, 0.0],
        };
        let Geom::Plane { n, .. } = resolve_local(&p.body, &bottom).unwrap() else {
            panic!("not a plane");
        };
        assert!((n + Vector3::z()).norm() < 1e-9, "outward: {n}");
        // The rim: the edge between the top and the bore.
        let e = p
            .body
            .edges()
            .find_map(|(e, _)| {
                let r = crate::part::blend::edge_ref(&p.body, e)?;
                let both = r.faces.contains(&"Box1:Top".to_owned())
                    && r.faces.iter().any(|f| f.starts_with("Hole1:Bore"));
                both.then_some(r)
            })
            .expect("a rim edge");
        let rim = GeomRef::Edge {
            occurrence: 1,
            edge: e,
        };
        let Geom::Circle { c, n, r } = resolve_local(&p.body, &rim).unwrap() else {
            panic!("not a circle");
        };
        assert!((c - Point3::new(20.0, 15.0, 20.0)).norm() < 1e-6, "{c}");
        assert!((n - Vector3::z()).norm() < 1e-9 && (r - 3.0).abs() < 1e-6);
        let bore = GeomRef::Face {
            occurrence: 1,
            name: "Hole1:Bore:1".into(),
            hint: [23.0, 15.0, 10.0],
        };
        let Geom::Axis { p: a, d } = resolve_local(&p.body, &bore).unwrap() else {
            panic!("not an axis");
        };
        assert!(d.cross(&Vector3::z()).norm() < 1e-9);
        assert!(((a - Point3::new(20.0, 15.0, 0.0)).xy()).norm() < 1e-6);
        let gone = GeomRef::Face {
            occurrence: 1,
            name: "Box1:Nowhere".into(),
            hint: [0.0; 3],
        };
        assert!(
            resolve_local(&p.body, &gone)
                .unwrap_err()
                .contains("no longer exists")
        );
    }
}
