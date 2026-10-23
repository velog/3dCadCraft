//! One visible face / edge from several kernel pieces.
//!
//! The exact kernel (truck) has no periodic faces or closed edges: a
//! circle comes as two or more arcs, a cylinder as two or more faces, each
//! with its own copy of the same surface. Inventor and SolidCraft show and
//! pick such pieces as one face and one edge. Here: two faces are one
//! *smooth group* when they share an edge and lie on the same curved
//! analytic surface (cylinder, cone, sphere, torus — planes keep their
//! edges, Split Face cuts them on purpose); the edges between them are not
//! drawn ([`BRep::visible_edges`]); an edge's group is the chain of pieces
//! on the same curve whose faces are in the same smooth groups.

use std::collections::HashMap;

use super::{BRep, Curve, EdgeId, FaceId, Point3, Real, Surface, Vector3};

/// Relative tolerance of the surface comparison (positions scale with the
/// sizes involved).
const TOL: Real = 1e-7;

fn close(a: Real, b: Real, scale: Real) -> bool {
    (a - b).abs() <= TOL * scale.max(1.0)
}

/// The same infinite line: parallel directions, `q` on the line `(o, d)`.
fn same_line(o: &Point3, d: &Vector3, q: &Point3, e: &Vector3, scale: Real) -> bool {
    let w = q - o;
    d.cross(e).norm() <= TOL && (w - d * w.dot(d)).norm() <= TOL * scale.max(1.0)
}

/// Whether two surfaces are the same curved analytic surface (a plane is
/// never merged).
pub fn same_curved(a: &Surface, b: &Surface) -> bool {
    match (a, b) {
        (Surface::Cylinder(x), Surface::Cylinder(y)) => {
            let s = x.radius.max(y.radius);
            close(x.radius, y.radius, s)
                && same_line(
                    &x.origin,
                    &x.axis,
                    &y.origin,
                    &y.axis,
                    s + (x.origin - y.origin).norm(),
                )
        }
        (Surface::Sphere(x), Surface::Sphere(y)) => {
            let s = x.radius.max(y.radius);
            close(x.radius, y.radius, s) && (x.center - y.center).norm() <= TOL * s.max(1.0)
        }
        (Surface::Torus(x), Surface::Torus(y)) => {
            let s = x.major.max(y.major);
            close(x.major, y.major, s)
                && close(x.minor, y.minor, s)
                && (x.center - y.center).norm() <= TOL * s.max(1.0)
                && x.axis.cross(&y.axis).norm() <= TOL
        }
        (Surface::Cone(x), Surface::Cone(y)) => {
            // The same apex, axis and half-angle (the reference circles
            // may sit at different heights).
            let apex = |c: &super::Cone| {
                (c.half_angle.abs() > 1e-12)
                    .then(|| c.origin - c.axis.into_inner() * (c.radius / c.half_angle.tan()))
            };
            let (Some(p), Some(q)) = (apex(x), apex(y)) else {
                return false;
            };
            let s = x.radius.max(y.radius);
            (p - q).norm() <= TOL * s.max(1.0)
                && close(x.half_angle.abs(), y.half_angle.abs(), 1.0)
                && x.axis.cross(&y.axis).norm() <= TOL
        }
        _ => false,
    }
}

/// The same curve: one circle (centre, axis, radius) or one line.
fn same_curve(a: &Curve, b: &Curve) -> bool {
    match (a, b) {
        (Curve::Circle(x), Curve::Circle(y)) => {
            let s = x.radius.max(y.radius);
            close(x.radius, y.radius, s)
                && (x.center - y.center).norm() <= TOL * s.max(1.0)
                && x.normal.cross(&y.normal).norm() <= TOL
        }
        (Curve::Line(x), Curve::Line(y)) => {
            let s = (x.origin - y.origin).norm();
            same_line(&x.origin, &x.direction, &y.origin, &y.direction, s)
        }
        _ => false,
    }
}

impl BRep {
    /// The analytic surface of face `f` (the exact one of a facet).
    pub(crate) fn face_surface_of(&self, f: FaceId) -> Option<&Surface> {
        let face = self.face(f)?;
        self.surface(face.facet_of.unwrap_or(face.surface))
    }

    /// Edge → the faces using it, for every edge (one walk).
    pub(crate) fn edge_face_map(&self) -> HashMap<EdgeId, Vec<FaceId>> {
        let mut m: HashMap<EdgeId, Vec<FaceId>> = HashMap::new();
        for (fid, face) in self.faces() {
            for lp in std::iter::once(&face.outer).chain(&face.inner) {
                for ce in &lp.coedges {
                    let v = m.entry(ce.edge).or_default();
                    if !v.contains(&fid) {
                        v.push(fid);
                    }
                }
            }
        }
        m
    }

    /// Whether the edge between faces `a` and `b` is a seam inside one
    /// visible face (both on the same curved surface).
    pub(crate) fn is_smooth_seam(&self, a: FaceId, b: FaceId) -> bool {
        match (self.face_surface_of(a), self.face_surface_of(b)) {
            (Some(x), Some(y)) => a != b && same_curved(x, y),
            _ => false,
        }
    }

    /// The pieces of the visible face that face `f` belongs to: `f` and
    /// every face reached across seams on the same curved surface.
    pub fn smooth_group(&self, f: FaceId) -> Vec<FaceId> {
        let Some(own) = self.face_surface_of(f) else {
            return vec![f];
        };
        if matches!(own, Surface::Plane(_) | Surface::Extrusion(_)) {
            return vec![f];
        }
        let map = self.edge_face_map();
        let mut out = vec![f];
        let mut k = 0;
        while k < out.len() {
            let cur = out[k];
            k += 1;
            let Some(face) = self.face(cur) else { continue };
            for lp in std::iter::once(&face.outer).chain(&face.inner) {
                for ce in &lp.coedges {
                    for &g in map.get(&ce.edge).into_iter().flatten() {
                        if !out.contains(&g) && self.is_smooth_seam(cur, g) {
                            out.push(g);
                        }
                    }
                }
            }
        }
        out
    }

    /// The pieces of the visible edge that edge `e` belongs to: edges on
    /// the same circle or line, chained through shared vertices, whose
    /// faces are the same visible faces (smooth groups) as `e`'s.
    pub fn edge_group(&self, e: EdgeId) -> Vec<EdgeId> {
        let Some(edge) = self.edge(e) else {
            return vec![e];
        };
        let Some(curve) = self.curve(edge.curve) else {
            return vec![e];
        };
        if matches!(curve, Curve::BSpline(_)) {
            return vec![e];
        }
        let map = self.edge_face_map();
        // The visible faces of `e`: the smooth groups of its faces.
        let groups: Vec<Vec<FaceId>> = map
            .get(&e)
            .into_iter()
            .flatten()
            .map(|f| self.smooth_group(*f))
            .collect();
        let same_faces = |x: EdgeId| {
            let fs = map.get(&x).cloned().unwrap_or_default();
            fs.len() == groups.len() && fs.iter().all(|f| groups.iter().any(|g| g.contains(f)))
        };
        let mut out = vec![e];
        let mut k = 0;
        while k < out.len() {
            let Some(cur) = self.edge(out[k]) else {
                k += 1;
                continue;
            };
            let ends = [cur.start, cur.end];
            k += 1;
            for (id, x) in self.edges() {
                if out.contains(&id) || !(ends.contains(&x.start) || ends.contains(&x.end)) {
                    continue;
                }
                if self.curve(x.curve).is_some_and(|c| same_curve(curve, c)) && same_faces(id) {
                    out.push(id);
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::{Cylinder, UnitVector3};

    fn cyl(o: [Real; 3], a: [Real; 3], r: Real) -> Surface {
        Surface::Cylinder(Cylinder {
            origin: Point3::from(o),
            axis: UnitVector3::new_normalize(Vector3::from(a)),
            x_axis: UnitVector3::new_normalize(Vector3::x()),
            radius: r,
        })
    }

    #[test]
    fn same_cylinders_only() {
        let a = cyl([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 5.0);
        // The same axis line seen from another origin on it.
        assert!(same_curved(
            &a,
            &cyl([0.0, 0.0, 7.0], [0.0, 0.0, -1.0], 5.0)
        ));
        assert!(!same_curved(
            &a,
            &cyl([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 5.1)
        ));
        assert!(!same_curved(
            &a,
            &cyl([0.1, 0.0, 0.0], [0.0, 0.0, 1.0], 5.0)
        ));
        assert!(!same_curved(
            &a,
            &cyl([0.0, 0.0, 0.0], [0.0, 1.0, 1.0], 5.0)
        ));
    }
}
