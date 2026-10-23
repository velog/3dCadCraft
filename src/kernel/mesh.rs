//! Tessellation: converting exact B-Rep geometry into triangles and polylines
//! for display (and later for STL export).
//!
//! The B-Rep stays the source of truth; meshes are disposable, regenerated
//! whenever the model changes.
//!
//! Current coverage:
//! - **Edges**: lines, circular arcs and B-splines, sampled to a chord
//!   tolerance.
//! - **Faces**: planar faces (with holes, bridged and ear-clipped), and
//!   ruled faces (cylinders and extrusion surfaces bounded by two rails and
//!   two straight seams, as extrusions make them) as triangle strips.

use nalgebra as na;

use super::{
    BRep, Curve, EdgeId, Face, FaceId, KernelError, KernelResult, Point3, Real, Sense, SolidId,
    Surface, Vector3,
};

/// Controls how finely curved geometry is approximated.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TessellationOptions {
    /// Maximum distance between the true curve and its polyline (mm).
    pub chord_tolerance: Real,
    /// Upper bound on segments per edge, guarding against tiny tolerances.
    pub max_segments: usize,
}

impl Default for TessellationOptions {
    fn default() -> Self {
        Self {
            chord_tolerance: 0.01,
            max_segments: 256,
        }
    }
}

/// An indexed triangle mesh with per-vertex normals.
///
/// Vertices are not shared between faces, so every face keeps its own
/// (flat) normals and creases stay sharp.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TriangleMesh {
    /// Vertex positions.
    pub positions: Vec<Point3>,
    /// Unit outward normal per vertex.
    pub normals: Vec<Vector3>,
    /// Counter-clockwise (seen from outside) triangles indexing `positions`.
    pub triangles: Vec<[u32; 3]>,
}

/// Display data for a solid: shaded faces plus edge polylines.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SolidMesh {
    /// Triangulated faces.
    pub faces: TriangleMesh,
    /// One polyline per topological edge.
    pub edges: Vec<Vec<Point3>>,
    /// The triangles of each face (a range of `faces.triangles`), for
    /// per-face colours.
    pub face_triangles: Vec<(FaceId, std::ops::Range<usize>)>,
}

impl BRep {
    /// Samples an edge into a polyline from its start to its end vertex.
    pub fn sample_edge(&self, id: EdgeId, opts: &TessellationOptions) -> KernelResult<Vec<Point3>> {
        self.sample_edge_n(id, opts, None)
    }

    /// [`BRep::sample_edge`] with a forced number of segments.
    fn sample_edge_n(
        &self,
        id: EdgeId,
        opts: &TessellationOptions,
        forced: Option<usize>,
    ) -> KernelResult<Vec<Point3>> {
        let edge = self
            .edge(id)
            .ok_or(KernelError::DanglingReference("edge"))?;
        let curve = self
            .curve(edge.curve)
            .ok_or(KernelError::DanglingReference("curve"))?;
        let range = edge.range;

        // Circles on a global angular grid: multiples of 2π/(96k) measured
        // from world X (or Y) projected into the circle's plane. Coaxial
        // arcs of any feature then share facet angles, so Booleans between
        // them (fillets on fillets, chamfers on fillets) meet cleanly.
        if let Curve::Circle(c) = curve {
            let ratio = (1.0 - opts.chord_tolerance / c.radius).clamp(-1.0, 1.0);
            let max_angle = (2.0 * ratio.acos()).max(1e-3);
            let full = (std::f64::consts::TAU / max_angle).ceil() as usize;
            let k = full.div_ceil(96).max(1);
            let step = std::f64::consts::TAU / (96 * k) as Real;
            let n = c.normal.into_inner();
            let mut anchor = Vector3::x() - n * n.x;
            if anchor.norm() < 1e-6 {
                anchor = Vector3::y() - n * n.y;
            }
            let theta_a = anchor
                .dot(&c.y_axis().into_inner())
                .atan2(anchor.dot(&c.x_axis.into_inner()));
            let (t0, t1) = (range.min, range.max);
            let mut ts = vec![t0];
            let mut j = ((t0 - theta_a) / step).floor() as i64 + 1;
            loop {
                let t = theta_a + j as Real * step;
                if t >= t1 - step * 0.05 {
                    break;
                }
                if t > t0 + step * 0.05 {
                    ts.push(t);
                }
                j += 1;
            }
            ts.push(t1);
            if let Some(m) = forced
                && m != ts.len() - 1
            {
                return Ok((0..=m)
                    .map(|i| curve.point_at(range.lerp(i as Real / m as Real)))
                    .collect());
            }
            return Ok(ts.into_iter().map(|t| curve.point_at(t)).collect());
        }
        let segments = match curve {
            // Circles returned above.
            Curve::Line(_) | Curve::Circle(_) => 1,
            // Heuristic until adaptive subdivision lands: dense uniform sampling.
            Curve::BSpline(b) => 8 * b.control_points().len(),
        }
        .clamp(1, opts.max_segments);
        let segments = forced.unwrap_or(segments);

        Ok((0..=segments)
            .map(|i| curve.point_at(range.lerp(i as Real / segments as Real)))
            .collect())
    }

    /// Tessellates every face and edge of a solid.
    ///
    /// Faces that cannot be tessellated yet are skipped (and logged) rather
    /// than failing the whole solid, so the viewer can always show something.
    pub fn tessellate_solid(
        &self,
        id: SolidId,
        opts: &TessellationOptions,
    ) -> KernelResult<SolidMesh> {
        let solid = self
            .solid(id)
            .ok_or(KernelError::DanglingReference("solid"))?;
        let mut mesh = SolidMesh::default();
        let mut seen_edges = std::collections::HashSet::new();
        // Edges between two facets of one curved face are not drawn.
        // Nor are seams (an exact face meeting itself).
        type Side = (super::FaceId, Option<super::SurfaceId>);
        let mut edge_faces: std::collections::HashMap<EdgeId, Vec<Side>> =
            std::collections::HashMap::new();
        for shell in &solid.shells {
            for &fid in &shell.faces {
                if let Some(face) = self.face(fid) {
                    for lp in std::iter::once(&face.outer).chain(&face.inner) {
                        for ce in &lp.coedges {
                            edge_faces
                                .entry(ce.edge)
                                .or_default()
                                .push((fid, face.facet_of));
                        }
                    }
                }
            }
        }
        let smooth = |e: &EdgeId| {
            edge_faces.get(e).is_some_and(|f| {
                f.len() == 2
                    && (f[0].0 == f[1].0
                        || (f[0].1.is_some() && f[0].1 == f[1].1)
                        // A seam between kernel pieces of one curved surface.
                        || self.is_smooth_seam(f[0].0, f[1].0))
            })
        };

        for shell in &solid.shells {
            for &fid in &shell.faces {
                let face = self
                    .face(fid)
                    .ok_or(KernelError::DanglingReference("face"))?;
                let start = mesh.faces.triangles.len();
                if let Err(e) = self.tessellate_face(face, opts, &mut mesh.faces) {
                    log::warn!("skipping face {fid:?}: {e}");
                }
                mesh.face_triangles
                    .push((fid, start..mesh.faces.triangles.len()));
                for lp in std::iter::once(&face.outer).chain(&face.inner) {
                    for ce in &lp.coedges {
                        if seen_edges.insert(ce.edge) && !smooth(&ce.edge) {
                            mesh.edges.push(self.sample_edge(ce.edge, opts)?);
                        }
                    }
                }
            }
        }
        Ok(mesh)
    }

    /// Triangulates a single face (e.g. to highlight it under the mouse).
    pub fn face_mesh(&self, id: FaceId, opts: &TessellationOptions) -> KernelResult<TriangleMesh> {
        let face = self
            .face(id)
            .ok_or(KernelError::DanglingReference("face"))?;
        if let Some(m) = &face.mesh {
            return Ok((**m).clone());
        }
        let mut mesh = TriangleMesh::default();
        self.tessellate_face(face, opts, &mut mesh)?;
        Ok(mesh)
    }

    /// Appends the triangles of one face to `out`.
    fn tessellate_face(
        &self,
        face: &Face,
        opts: &TessellationOptions,
        out: &mut TriangleMesh,
    ) -> KernelResult<()> {
        if let Some(fm) = &face.mesh {
            let base = out.positions.len() as u32;
            out.positions.extend_from_slice(&fm.positions);
            out.normals.extend_from_slice(&fm.normals);
            out.triangles.extend(
                fm.triangles
                    .iter()
                    .map(|t| [t[0] + base, t[1] + base, t[2] + base]),
            );
            return Ok(());
        }
        let plane = match self.surface(face.surface) {
            Some(Surface::Plane(plane)) => plane,
            Some(Surface::Cylinder(_) | Surface::Cone(_) | Surface::Extrusion(_)) => {
                return self.tessellate_ruled(face, opts, out);
            }
            Some(Surface::Sphere(_) | Surface::Torus(_)) => {
                return Err(KernelError::Unsupported("exact sphere/torus faces"));
            }
            None => return Err(KernelError::DanglingReference("surface")),
        };

        // Each loop as a polygon: edge polylines concatenated in loop direction.
        let ring = |lp: &super::Loop| -> KernelResult<Vec<Point3>> {
            let mut polygon: Vec<Point3> = Vec::new();
            for ce in &lp.coedges {
                let mut pts = self.sample_edge(ce.edge, opts)?;
                if ce.sense == Sense::Reversed {
                    pts.reverse();
                }
                pts.pop(); // the next coedge starts here
                polygon.extend(pts);
            }
            Ok(polygon)
        };
        let outer = ring(&face.outer)?;
        let holes = face
            .inner
            .iter()
            .map(&ring)
            .collect::<KernelResult<Vec<_>>>()?;
        let uv = |pts: &[Point3]| -> Vec<na::Point2<Real>> {
            pts.iter().map(|p| plane.project(p)).collect()
        };
        let hole_uv: Vec<Vec<na::Point2<Real>>> = holes.iter().map(|h| uv(h)).collect();
        let (_, tris) = triangulate_with_holes(&uv(&outer), &hole_uv);
        // triangulate_with_holes numbers the outer points first, then each hole.
        let polygon: Vec<Point3> = outer
            .into_iter()
            .chain(holes.into_iter().flatten())
            .collect();
        let outward = match face.sense {
            Sense::Same => plane.normal.into_inner(),
            Sense::Reversed => -plane.normal.into_inner(),
        };

        let base = out.positions.len() as u32;
        out.positions.extend_from_slice(&polygon);
        // A facet of a curved face is shaded with the exact surface's normal
        // (on the facet's side), so it looks smooth.
        let exact = face.facet_of.and_then(|s| self.surface(s));
        out.normals.extend(polygon.iter().map(|p| {
            exact
                .and_then(|s| s.normal_near(p))
                .map(|n| if n.dot(&outward) < 0.0 { -n } else { n })
                .unwrap_or(outward)
        }));
        for [a, b, c] in tris {
            // Triangles are CCW in (u, v), i.e. around +plane.normal.
            let tri = if face.sense == Sense::Same {
                [a, b, c]
            } else {
                [a, c, b]
            };
            out.triangles.push(tri.map(|i| base + i as u32));
        }
        Ok(())
    }
}

impl BRep {
    /// Triangulates a ruled face — the side of an extrusion: an outer loop
    /// of two curved "rails" (start and end level) joined by two straight
    /// seams. The rails are sampled alike and joined by a triangle strip;
    /// normals are averaged per vertex for smooth shading.
    fn tessellate_ruled(
        &self,
        face: &Face,
        opts: &TessellationOptions,
        out: &mut TriangleMesh,
    ) -> KernelResult<()> {
        let rail_edges: Vec<&super::Coedge> = face
            .outer
            .coedges
            .iter()
            .filter(|ce| {
                self.edge(ce.edge)
                    .and_then(|e| self.curve(e.curve))
                    .is_some_and(|c| !matches!(c, Curve::Line(_)))
            })
            .collect();
        // Both rails get the same number of segments (a cone's rails have
        // different radii, so their own counts differ).
        let mut n = 0;
        for ce in &rail_edges {
            n = n.max(self.sample_edge(ce.edge, opts)?.len() - 1);
        }
        let rails: Vec<Vec<Point3>> = rail_edges
            .iter()
            .map(|ce| {
                let mut pts = self.sample_edge_n(ce.edge, opts, Some(n))?;
                if ce.sense == Sense::Reversed {
                    pts.reverse();
                }
                Ok(pts)
            })
            .collect::<KernelResult<_>>()?;
        let [a, b] = &rails[..] else {
            return Err(KernelError::Unsupported("curved face without two rails"));
        };
        // The loop runs a forward, then b backward: reverse b to pair them up.
        let b: Vec<Point3> = b.iter().rev().copied().collect();
        if a.len() != b.len() || a.len() < 2 {
            return Err(KernelError::Unsupported("rails sampled differently"));
        }
        let n = a.len();
        let quad_normal = |i: usize| (a[i + 1] - a[i]).cross(&(b[i] - a[i]));
        let base = out.positions.len() as u32;
        for pts in [a, &b] {
            for (i, &p) in pts.iter().enumerate() {
                let mut nrm = Vector3::zeros();
                if i > 0 {
                    nrm += quad_normal(i - 1);
                }
                if i + 1 < n {
                    nrm += quad_normal(i);
                }
                // Closed rails (full circle): the seam vertices see both sides.
                if (a[0] - a[n - 1]).norm() < 1e-9 && (i == 0 || i == n - 1) {
                    nrm += quad_normal(if i == 0 { n - 2 } else { 0 });
                }
                out.positions.push(p);
                out.normals.push(nrm.try_normalize(1e-15).unwrap_or(nrm));
            }
        }
        let n32 = n as u32;
        for i in 0..n32 - 1 {
            let (p0, p1, q0, q1) = (base + i, base + i + 1, base + n32 + i, base + n32 + i + 1);
            out.triangles.push([p0, p1, q1]);
            out.triangles.push([p0, q1, q0]);
        }
        Ok(())
    }
}

/// Twice the signed area of a 2D polygon (positive ⇔ counter-clockwise).
fn signed_area2(pts: &[na::Point2<Real>]) -> Real {
    (0..pts.len())
        .map(|i| {
            let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
            p.x * q.y - q.x * p.y
        })
        .sum()
}

/// `(b − a) × (c − a)` in 2D: > 0 if `a, b, c` turn left.
fn cross(a: &na::Point2<Real>, b: &na::Point2<Real>, c: &na::Point2<Real>) -> Real {
    (b - a).perp(&(c - a))
}

/// Triangulates a simple polygon by ear clipping; triangles are returned
/// counter-clockwise regardless of the input orientation.
///
/// O(n²), which is fine for display meshes of CAD faces; a sweep-line
/// triangulator can replace it if profiling says so.
#[cfg(test)]
pub(crate) fn triangulate(pts: &[na::Point2<Real>]) -> Vec<[usize; 3]> {
    let mut idx: Vec<usize> = (0..pts.len()).collect();
    if signed_area2(pts) < 0.0 {
        idx.reverse();
    }
    clip_ears(pts, idx)
}

/// Triangulates a polygon with holes: each hole is joined to the outer
/// boundary by a bridge (a zero-width cut), then the result is ear-clipped.
/// Returns all points (outer first, then the holes) and CCW triangles
/// indexing them. Orientation of the inputs does not matter.
pub fn triangulate_with_holes(
    outer: &[na::Point2<Real>],
    holes: &[Vec<na::Point2<Real>>],
) -> (Vec<na::Point2<Real>>, Vec<[usize; 3]>) {
    let mut pts: Vec<na::Point2<Real>> = outer.to_vec();
    let mut poly: Vec<usize> = (0..outer.len()).collect();
    if signed_area2(outer) < 0.0 {
        poly.reverse();
    }
    // Holes clockwise, rightmost first (Eberly's bridging order).
    let mut rings: Vec<Vec<usize>> = Vec::new();
    for h in holes.iter().filter(|h| h.len() >= 3) {
        let base = pts.len();
        pts.extend_from_slice(h);
        let mut ring: Vec<usize> = (base..pts.len()).collect();
        if signed_area2(h) > 0.0 {
            ring.reverse();
        }
        rings.push(ring);
    }
    let max_x = |r: &Vec<usize>| r.iter().map(|&i| pts[i].x).fold(Real::MIN, Real::max);
    rings.sort_by(|a, b| max_x(b).total_cmp(&max_x(a)));
    for k in 0..rings.len() {
        let ring = rings[k].clone();
        let Some(m) = (0..ring.len()).max_by(|&a, &b| pts[ring[a]].x.total_cmp(&pts[ring[b]].x))
        else {
            continue; // rings have at least three points
        };
        let mp = pts[ring[m]];
        // Nearest polygon vertex visible from the hole's rightmost point.
        let blocked = |v: usize| {
            let vp = pts[v];
            let crosses = |ring: &[usize]| {
                (0..ring.len()).any(|i| {
                    let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
                    a != v && b != v && segments_cross(&mp, &vp, &pts[a], &pts[b])
                })
            };
            // A bridge through another vertex (collinear points) would
            // make the merged polygon touch itself.
            let len2 = (vp - mp).norm_squared();
            let through = |ring: &[usize]| {
                ring.iter().any(|&q| {
                    let p = pts[q];
                    if q == v || p == mp || p == vp {
                        return false;
                    }
                    let t = (p - mp).dot(&(vp - mp)) / len2;
                    t > 0.0 && t < 1.0 && cross(&mp, &vp, &p).abs() <= 1e-9 * len2.max(1e-12)
                })
            };
            crosses(&poly)
                || rings[k..].iter().any(|r| crosses(r))
                || through(&poly)
                || rings[k..].iter().any(|r| through(r))
        };
        let j = (0..poly.len())
            .filter(|&j| !blocked(poly[j]))
            .min_by(|&a, &b| {
                (pts[poly[a]] - mp)
                    .norm_squared()
                    .total_cmp(&(pts[poly[b]] - mp).norm_squared())
            })
            .unwrap_or(0);
        // The vertex may be passed several times (earlier bridges to it):
        // take the pass whose interior wedge holds the bridge, or the
        // merged polygon crosses itself at that vertex.
        let vp = pts[poly[j]];
        let np = poly.len();
        let j = (0..np)
            .filter(|&q| pts[poly[q]] == vp)
            .find(|&q| {
                let (u, w) = (pts[poly[(q + np - 1) % np]], pts[poly[(q + 1) % np]]);
                let (l1, l2) = (cross(&u, &vp, &mp), cross(&vp, &w, &mp));
                if cross(&u, &vp, &w) > 0.0 {
                    l1 > 0.0 && l2 > 0.0
                } else {
                    l1 > 0.0 || l2 > 0.0
                }
            })
            .unwrap_or(j);
        // poly[..=j], hole from m around back to m, then poly[j..].
        let mut merged: Vec<usize> = poly[..=j].to_vec();
        merged.extend((0..=ring.len()).map(|i| ring[(m + i) % ring.len()]));
        merged.extend_from_slice(&poly[j..]);
        poly = merged;
    }
    let tris = clip_ears(&pts, poly);
    (pts, tris)
}

/// Do segments `ab` and `cd` cross at a point interior to both?
fn segments_cross(
    a: &na::Point2<Real>,
    b: &na::Point2<Real>,
    c: &na::Point2<Real>,
    d: &na::Point2<Real>,
) -> bool {
    let (d1, d2) = (cross(a, b, c), cross(a, b, d));
    let (d3, d4) = (cross(c, d, a), cross(c, d, b));
    d1 * d2 < 0.0 && d3 * d4 < 0.0
}

/// Ear clipping of the counter-clockwise index polygon `idx` (indices may
/// repeat, as at hole bridges).
fn clip_ears(pts: &[na::Point2<Real>], mut idx: Vec<usize>) -> Vec<[usize; 3]> {
    let n = idx.len();
    if n < 3 {
        return Vec::new();
    }

    // Scale-relative zero for orientation tests: collinear points must be
    // recognised in any rotation of the frame (rounding gives ±1e-15).
    let size2 = idx
        .iter()
        .map(|&k| pts[k].coords.norm_squared())
        .fold(0.0, Real::max)
        .max(1e-12);
    let eps = 1e-12 * size2;
    let mut tris = Vec::with_capacity(n - 2);
    let mut i = 0;
    let mut since_last_ear = 0;
    while idx.len() > 3 {
        let m = idx.len();
        let (a, b, c) = (idx[(i + m - 1) % m], idx[i % m], idx[(i + 1) % m]);
        let convex = cross(&pts[a], &pts[b], &pts[c]) > eps;
        let is_ear = convex
            && (0..m).all(|j| {
                let k = idx[j];
                if j == i % m || j == (i + m - 1) % m || j == (i + 1) % m {
                    return true;
                }
                let p = pts[k];
                if p == pts[a] || p == pts[b] || p == pts[c] {
                    // Another pass through a corner (a hole bridge, or
                    // several holes bridged to one vertex): the ear must not
                    // cover the polygon's edges leaving that pass.
                    let size = (pts[c] - pts[a]).norm().max((pts[b] - pts[a]).norm());
                    return [idx[(j + m - 1) % m], idx[(j + 1) % m]].iter().all(|&q| {
                        let d = pts[q] - p;
                        let len = d.norm();
                        if len <= 0.0 {
                            return true;
                        }
                        let x = p + d * (1e-4 * size / len).min(0.5);
                        let inside = |e: Real| {
                            cross(&pts[a], &pts[b], &x) > e
                                && cross(&pts[b], &pts[c], &x) > e
                                && cross(&pts[c], &pts[a], &x) > e
                        };
                        !inside(1e-3 * eps)
                    });
                }
                // Strictly outside triangle abc? (On its boundary blocks.)
                cross(&pts[a], &pts[b], &p) < -eps
                    || cross(&pts[b], &pts[c], &p) < -eps
                    || cross(&pts[c], &pts[a], &p) < -eps
            });

        if is_ear {
            tris.push([a, b, c]);
            idx.remove(i % m);
            since_last_ear = 0;
        } else {
            i += 1;
            since_last_ear += 1;
            if since_last_ear > m {
                // Stuck on collinear points: drop a straight vertex (it lies
                // on the segment between its neighbours, so no area is lost).
                let straight = (0..m).find(|&k| {
                    let (a, b, c) = (idx[(k + m - 1) % m], idx[k], idx[(k + 1) % m]);
                    let (pa, pb, pc) = (pts[a], pts[b], pts[c]);
                    cross(&pa, &pb, &pc).abs() <= 1e-12 * (1.0 + (pc - pa).norm_squared())
                        && (pb - pa).dot(&(pc - pb)) >= 0.0
                });
                if let Some(k) = straight {
                    idx.remove(k);
                    since_last_ear = 0;
                    continue;
                }
                // Degenerate / self-intersecting input: fall back to a fan so
                // something is still drawn.
                log::warn!("ear clipping failed on a {n}-gon; using fan triangulation");
                for k in 1..idx.len() - 1 {
                    tris.push([idx[0], idx[k], idx[k + 1]]);
                }
                return tris;
            }
        }
    }
    tris.push([idx[0], idx[1], idx[2]]);
    tris
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn polygon_with_holes_triangulates_to_its_area() {
        let p = na::Point2::new;
        let outer = vec![p(0.0, 0.0), p(40.0, 0.0), p(40.0, 20.0), p(0.0, 20.0)];
        let square =
            |x: Real, y: Real, s: Real| vec![p(x, y), p(x + s, y), p(x + s, y + s), p(x, y + s)];
        let holes = vec![square(5.0, 5.0, 4.0), square(25.0, 8.0, 6.0)];
        let (pts, tris) = triangulate_with_holes(&outer, &holes);
        let area: Real = tris
            .iter()
            .map(|t| cross(&pts[t[0]], &pts[t[1]], &pts[t[2]]) / 2.0)
            .sum();
        assert_relative_eq!(area, 800.0 - 16.0 - 36.0, epsilon = 1e-9);
        assert!(
            tris.iter()
                .all(|t| cross(&pts[t[0]], &pts[t[1]], &pts[t[2]]) > 0.0),
            "all CCW"
        );
    }

    #[test]
    fn concave_polygon_triangulates_to_its_area() {
        // An "L" shape, given clockwise, area 3.
        let pts: Vec<_> = [
            (0.0, 0.0),
            (0.0, 2.0),
            (1.0, 2.0),
            (1.0, 1.0),
            (2.0, 1.0),
            (2.0, 0.0),
        ]
        .iter()
        .map(|&(x, y)| na::Point2::new(x, y))
        .collect();
        let tris = triangulate(&pts);
        assert_eq!(tris.len(), 4);
        let area: Real = tris
            .iter()
            .map(|t| 0.5 * cross(&pts[t[0]], &pts[t[1]], &pts[t[2]]))
            .sum();
        assert_relative_eq!(area, 3.0, epsilon = 1e-12);
        assert!(
            tris.iter()
                .all(|t| cross(&pts[t[0]], &pts[t[1]], &pts[t[2]]) > 0.0)
        );
    }

    #[test]
    fn box_mesh_is_closed_and_outward() {
        let mut m = BRep::new();
        let id = m
            .make_box(Point3::origin(), Point3::new(2.0, 3.0, 4.0))
            .unwrap();
        let mesh = m
            .tessellate_solid(id, &TessellationOptions::default())
            .unwrap();
        assert_eq!(mesh.faces.triangles.len(), 12);
        assert_eq!(mesh.edges.len(), 12);

        // Divergence theorem: Σ (centroid · n) · area / 3 = volume, which only
        // holds if every triangle is wound outward.
        let p = &mesh.faces.positions;
        let volume: Real = mesh
            .faces
            .triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| p[i as usize]);
                a.coords.dot(&(b - a).cross(&(c - a))) / 6.0
            })
            .sum();
        assert_relative_eq!(volume, 24.0, epsilon = 1e-9);
    }
}

#[cfg(test)]
mod collinear_tests {
    use super::*;

    /// A hole whose edges are collinear with split points of the outer
    /// loop (typical after a Boolean): the area must be exact in any frame.
    #[test]
    fn hole_with_collinear_points_in_rotated_frames() {
        let p = |v: &[(Real, Real)]| {
            v.iter()
                .map(|&(x, y)| na::Point2::new(x, y))
                .collect::<Vec<_>>()
        };
        let outer = p(&[
            (40.0, 0.0),
            (0.0, 0.0),
            (0.0, 5.0),
            (0.0, 15.0),
            (0.0, 20.0),
            (40.0, 20.0),
            (40.0, 15.0),
            (40.0, 5.0),
        ]);
        let hole = p(&[
            (15.0, 12.5),
            (15.0, 5.0),
            (23.571, 5.0),
            (25.0, 5.0),
            (25.0, 7.5),
            (25.0, 13.571),
            (25.0, 15.0),
            (23.571, 15.0),
            (15.0, 15.0),
            (15.0, 13.571),
        ]);
        for k in 0..64 {
            let (sn, cs) = (k as Real * 0.1).sin_cos();
            let rot = |v: &Vec<na::Point2<Real>>| {
                v.iter()
                    .map(|q| na::Point2::new(cs * q.x - sn * q.y, sn * q.x + cs * q.y))
                    .collect::<Vec<_>>()
            };
            let (pts, tris) = triangulate_with_holes(&rot(&outer), &[rot(&hole)]);
            let area: Real = tris
                .iter()
                .map(|t| cross(&pts[t[0]], &pts[t[1]], &pts[t[2]]).abs() / 2.0)
                .sum();
            assert!((area - 700.0).abs() < 1e-6, "angle {k}: area {area}");
        }
    }
}

#[cfg(test)]
mod grid_holes {
    use super::triangulate_with_holes;
    use nalgebra as na;

    /// A plate with a 10 × 4 grid of rectangular holes (aligned rightmost
    /// points): the triangles cover exactly plate − holes, no overlap.
    #[test]
    fn plate_with_a_grid_of_holes() {
        let p = |x: f64, y: f64| na::Point2::new(x, y);
        let outer = vec![p(0.0, 0.0), p(100.0, 0.0), p(100.0, 50.0), p(0.0, 50.0)];
        let mut holes = Vec::new();
        for a in 0..10 {
            for b in 0..4 {
                let (x, y) = (2.0 + 10.0 * a as f64, 2.0 + 12.0 * b as f64);
                holes.push(vec![
                    p(x + 6.0, y),
                    p(x, y),
                    p(x, y + 8.0),
                    p(x + 6.0, y + 8.0),
                ]);
            }
        }
        let (pts, tris) = triangulate_with_holes(&outer, &holes);
        let area: f64 = tris
            .iter()
            .map(|t| {
                let (a, b, c) = (pts[t[0]], pts[t[1]], pts[t[2]]);
                ((b - a).perp(&(c - a)) / 2.0).abs()
            })
            .sum();
        let expected = 100.0 * 50.0 - 40.0 * 48.0;
        assert!((area - expected).abs() < 1e-6, "{area} vs {expected}");
    }
}
