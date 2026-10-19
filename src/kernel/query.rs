//! Read-only queries on a [`BRep`]: lookup by persistent name, the geometry
//! of a face, and picking faces with a ray.
//!
//! ## Persistent names (v0)
//!
//! Features name the topology they create (`"Box1:Top"`, `"Box1:V101"`,
//! `"Box1:E100-101"`). Anything that refers to model geometry from *outside*
//! the B-Rep — a sketch placed on a face, a projected vertex, later a fillet
//! on an edge — stores the **name**, never the arena ID, because IDs change
//! every time the body is regenerated while names do not.
//!
//! This is the first step of the topological-naming scheme described in
//! `docs/FEATURES.md` 0.8; booleans will later derive names for split and
//! merged faces from their parents.

use nalgebra as na;

use super::{
    BRep, Curve, EdgeId, FaceId, KernelError, KernelResult, Plane, Point3, Real, Sense, Surface,
    TessellationOptions, Vector3, VertexId,
};

impl BRep {
    /// Whether `p` lies inside a closed solid of the model: the parity of a
    /// ray's crossings with the faces' triangles (a skew direction, so it
    /// does not graze edges of axis-aligned models).
    pub fn contains_point(&self, p: &Point3) -> bool {
        let dir = Vector3::new(0.5773, 0.6123, 0.5401).normalize();
        let opts = super::TessellationOptions::default();
        let mut inside = false;
        for (_, solid) in self.solids() {
            if solid.open {
                continue;
            }
            let mut hits = 0usize;
            for &f in solid.shells.iter().flat_map(|s| s.faces.iter()) {
                let Ok(m) = self.face_mesh(f, &opts) else {
                    continue;
                };
                for t in &m.triangles {
                    let [a, b, c] = t.map(|i| m.positions[i as usize]);
                    // Möller–Trumbore.
                    let (e1, e2) = (b - a, c - a);
                    let h = dir.cross(&e2);
                    let det = e1.dot(&h);
                    if det.abs() < 1e-14 {
                        continue;
                    }
                    let inv = 1.0 / det;
                    let s = p - a;
                    let u = s.dot(&h) * inv;
                    if !(0.0..=1.0).contains(&u) {
                        continue;
                    }
                    let q = s.cross(&e1);
                    let v = dir.dot(&q) * inv;
                    if v < 0.0 || u + v > 1.0 {
                        continue;
                    }
                    if e2.dot(&q) * inv > 1e-12 {
                        hits += 1;
                    }
                }
            }
            inside |= hits % 2 == 1;
        }
        inside
    }

    /// The face with persistent name `name`.
    pub fn find_face(&self, name: &str) -> Option<FaceId> {
        self.faces
            .iter()
            .find(|(_, f)| f.name.as_deref() == Some(name))
            .map(|(id, _)| id)
    }

    /// The vertex with persistent name `name`.
    pub fn find_vertex(&self, name: &str) -> Option<VertexId> {
        self.vertices
            .iter()
            .find(|(_, v)| v.name.as_deref() == Some(name))
            .map(|(id, _)| id)
    }

    /// The edge with persistent name `name`.
    pub fn find_edge(&self, name: &str) -> Option<EdgeId> {
        self.edges
            .iter()
            .find(|(_, e)| e.name.as_deref() == Some(name))
            .map(|(id, _)| id)
    }

    /// All edges of the model.
    pub fn edges(&self) -> impl Iterator<Item = (EdgeId, &super::Edge)> {
        self.edges.iter()
    }

    /// All vertices of the model.
    pub fn vertices(&self) -> impl Iterator<Item = (VertexId, &super::Vertex)> {
        self.vertices.iter()
    }

    /// All faces of the model.
    pub fn faces(&self) -> impl Iterator<Item = (FaceId, &super::Face)> {
        self.faces.iter()
    }

    /// The plane of a planar face, with its normal pointing **out of the
    /// solid** (the face's [`Sense`] is applied). `None` for non-planar faces.
    pub fn face_plane(&self, id: FaceId) -> Option<Plane> {
        let face = self.face(id)?;
        let Surface::Plane(plane) = self.surface(face.surface)? else {
            return None;
        };
        let mut plane = plane.clone();
        if face.sense == Sense::Reversed {
            plane.normal = -plane.normal;
            plane.x_axis = -plane.x_axis; // keep the frame right-handed
        }
        Some(plane)
    }

    /// Edges bounding a face (outer loop first), each listed once.
    pub fn face_edges(&self, id: FaceId) -> Vec<EdgeId> {
        let Some(face) = self.face(id) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for lp in std::iter::once(&face.outer).chain(&face.inner) {
            for ce in &lp.coedges {
                if !out.contains(&ce.edge) {
                    out.push(ce.edge);
                }
            }
        }
        out
    }

    /// Vertices bounding a face, each listed once, in loop order.
    pub fn face_vertices(&self, id: FaceId) -> Vec<VertexId> {
        let Some(face) = self.face(id) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for lp in std::iter::once(&face.outer).chain(&face.inner) {
            for ce in &lp.coedges {
                if let Some(v) = self.coedge_start(ce)
                    && !out.contains(&v)
                {
                    out.push(v);
                }
            }
        }
        out
    }

    /// Direction `end − start` of a straight edge; `None` for curved edges.
    pub fn edge_direction(&self, id: EdgeId) -> Option<Vector3> {
        let e = self.edge(id)?;
        match self.curve(e.curve)? {
            Curve::Line(_) => Some(self.vertex(e.end)?.point - self.vertex(e.start)?.point),
            _ => None,
        }
    }

    /// Every planar face hit by the ray `origin + t·dir` (`t > 0`), nearest
    /// first (for Select Other).
    pub fn pick_faces_all(&self, origin: &Point3, dir: &Vector3) -> Vec<(FaceId, Real)> {
        let opts = TessellationOptions::default();
        let mut out = Vec::new();
        for (id, face) in self.faces.iter() {
            let Some(plane) = self.face_plane(id) else {
                continue;
            };
            let denom = dir.dot(&plane.normal);
            if denom.abs() < 1e-12 {
                continue;
            }
            let t = (plane.origin - origin).dot(&plane.normal) / denom;
            if t > 0.0 && self.face_contains_on(face, &plane, &(origin + dir * t), &opts) {
                out.push((id, t));
            }
        }
        out.sort_by(|a, b| a.1.total_cmp(&b.1));
        out
    }

    /// The closest planar face hit by the ray `origin + t·dir` (`t > 0`),
    /// with its ray parameter. Faces seen from inside are hit as well, as
    /// in Inventor's face selection.
    pub fn pick_face(&self, origin: &Point3, dir: &Vector3) -> Option<(FaceId, Real)> {
        let opts = TessellationOptions::default();
        let mut best: Option<(FaceId, Real)> = None;
        for (id, face) in self.faces.iter() {
            let Some(plane) = self.face_plane(id) else {
                continue;
            };
            let denom = dir.dot(&plane.normal);
            if denom.abs() < 1e-12 {
                continue;
            }
            let t = (plane.origin - origin).dot(&plane.normal) / denom;
            if t <= 0.0 || best.is_some_and(|(_, bt)| bt <= t) {
                continue;
            }
            if self.face_contains_on(face, &plane, &(origin + dir * t), &opts) {
                best = Some((id, t));
            }
        }
        best
    }

    /// Does the planar face `id` contain the projection of `p` onto its
    /// plane (inside the outer loop, outside the holes)? `false` for
    /// non-planar faces.
    pub fn face_contains(&self, id: FaceId, p: &Point3) -> bool {
        match (self.face(id), self.face_plane(id)) {
            (Some(face), Some(plane)) => {
                self.face_contains_on(face, &plane, p, &TessellationOptions::default())
            }
            _ => false,
        }
    }

    fn face_contains_on(
        &self,
        face: &super::Face,
        plane: &Plane,
        p: &Point3,
        opts: &TessellationOptions,
    ) -> bool {
        let hit = plane.project(p);
        let inside = |lp: &super::Loop| {
            self.loop_polygon(lp, plane, opts)
                .is_ok_and(|poly| point_in_polygon(&poly, &hit))
        };
        inside(&face.outer) && !face.inner.iter().any(inside)
    }

    /// A loop as a 2D polygon in `plane` coordinates.
    pub(crate) fn loop_polygon(
        &self,
        lp: &super::Loop,
        plane: &Plane,
        opts: &TessellationOptions,
    ) -> KernelResult<Vec<na::Point2<Real>>> {
        let mut pts = Vec::new();
        for ce in &lp.coedges {
            let mut seg = self.sample_edge(ce.edge, opts)?;
            if ce.sense == Sense::Reversed {
                seg.reverse();
            }
            seg.pop();
            pts.extend(seg.iter().map(|p| plane.project(p)));
        }
        if pts.len() < 3 {
            return Err(KernelError::InvalidTopology("degenerate loop".into()));
        }
        Ok(pts)
    }
}

/// Even–odd point-in-polygon test.
fn point_in_polygon(poly: &[na::Point2<Real>], p: &na::Point2<Real>) -> bool {
    let mut inside = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a.y > p.y) != (b.y > p.y) {
            let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
            if p.x < x {
                inside = !inside;
            }
        }
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    fn sample_box() -> BRep {
        let mut m = BRep::new();
        m.make_box_named(Point3::origin(), Point3::new(10.0, 20.0, 30.0), "Box1")
            .unwrap();
        m
    }

    #[test]
    fn box_topology_is_named_and_findable() {
        let m = sample_box();
        for f in ["Bottom", "Top", "Front", "Back", "Left", "Right"] {
            assert!(m.find_face(&format!("Box1:{f}")).is_some(), "{f}");
        }
        let v = m.find_vertex("Box1:V111").unwrap();
        assert_eq!(m.vertex(v).unwrap().point, Point3::new(10.0, 20.0, 30.0));
        assert!(m.find_edge("Box1:E000-100").is_some());
        assert_eq!(m.faces().count(), 6);
    }

    #[test]
    fn face_planes_point_outward() {
        let m = sample_box();
        let center = Point3::new(5.0, 10.0, 15.0);
        for (id, _) in m.faces() {
            let plane = m.face_plane(id).unwrap();
            assert!(plane.signed_distance(&center) < 0.0);
            // Right-handed frame.
            assert_relative_eq!(
                plane.y_axis().cross(&plane.normal).dot(&plane.x_axis),
                1.0,
                epsilon = 1e-12
            );
        }
        let top = m.face_plane(m.find_face("Box1:Top").unwrap()).unwrap();
        assert_relative_eq!(top.normal.into_inner(), Vector3::z(), epsilon = 1e-12);
        assert_eq!(m.face_vertices(m.find_face("Box1:Top").unwrap()).len(), 4);
        assert_eq!(m.face_edges(m.find_face("Box1:Top").unwrap()).len(), 4);
    }

    #[test]
    fn ray_picks_nearest_face() {
        let m = sample_box();
        let (id, t) = m
            .pick_face(&Point3::new(5.0, 10.0, 100.0), &-Vector3::z())
            .unwrap();
        assert_eq!(m.face(id).unwrap().name.as_deref(), Some("Box1:Top"));
        assert_relative_eq!(t, 70.0, epsilon = 1e-9);
        // Missing the box entirely.
        assert!(
            m.pick_face(&Point3::new(50.0, 10.0, 100.0), &-Vector3::z())
                .is_none()
        );
        // From the side.
        let (id, _) = m
            .pick_face(&Point3::new(100.0, 10.0, 15.0), &-Vector3::x())
            .unwrap();
        assert_eq!(m.face(id).unwrap().name.as_deref(), Some("Box1:Right"));
    }
}
