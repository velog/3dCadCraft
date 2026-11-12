//! Curves a 3D sketch takes from the model (`docs/spec/1.10-3d-sketch.md`):
//! **Intersection Curve** (two faces, or a face and a plane), **Project to
//! Surface** (points along a direction or to the closest point of faces),
//! **Silhouette Curve** (where a body's faces turn away from a direction)
//! and **Equation Curve** (x(t), y(t), z(t)). Each works on the display
//! triangles (within their 0.01 mm) and returns polylines, which the sketch
//! keeps as fixed interpolation splines (`Sketch3d::include`) — copies, not
//! associative yet. GUI-free.

use std::collections::HashMap;

use crate::kernel::{BRep, FaceId, Plane, Point3, Real, Vector3};

/// A triangle in space.
pub type Tri = [Point3; 3];

/// Every display triangle of `faces` of `body` (faces without a mesh have
/// none).
pub fn face_triangles(body: &BRep, faces: &[FaceId]) -> Vec<Tri> {
    let mut out = Vec::new();
    for &f in faces {
        let Some(m) = body.face(f).and_then(|x| x.mesh.as_ref()) else {
            continue;
        };
        for t in &m.triangles {
            out.push(t.map(|i| m.positions[i as usize]));
        }
    }
    out
}

/// How close to a plane a point counts as on it (mm).
const ON: Real = 1e-7;

/// Where the plane through `o` with normal `n` meets triangle `t`: a
/// segment, also when it only touches along an edge (two corners on the
/// plane: faces meeting at a model edge); `None` for a triangle in the plane.
fn cut_triangle(t: &Tri, o: Point3, n: Vector3) -> Option<[Point3; 2]> {
    let s = t.map(|p| {
        let d = (p - o).dot(&n);
        if d.abs() < ON { 0.0 } else { d }
    });
    if s.iter().all(|x| *x > 0.0) || s.iter().all(|x| *x < 0.0) || s.iter().all(|x| *x == 0.0) {
        return None;
    }
    let mut pts: Vec<Point3> = Vec::with_capacity(3);
    for i in 0..3 {
        let j = (i + 1) % 3;
        if s[i] == 0.0 {
            pts.push(t[i]);
        } else if s[j] != 0.0 && (s[i] > 0.0) != (s[j] > 0.0) {
            let u = s[i] / (s[i] - s[j]);
            pts.push(t[i] + (t[j] - t[i]) * u);
        }
    }
    pts.dedup_by(|a, b| (*a - *b).norm() < ON);
    if pts.len() > 2 && (pts[0] - pts[pts.len() - 1]).norm() < ON {
        pts.pop();
    }
    (pts.len() == 2).then(|| [pts[0], pts[1]])
}

/// Where `plane` crosses the triangles: one segment per triangle it cuts.
pub fn plane_section(tris: &[Tri], plane: &Plane) -> Vec<[Point3; 2]> {
    let n = plane.normal.into_inner();
    tris.iter()
        .filter_map(|t| cut_triangle(t, plane.origin, n))
        .collect()
}

/// The segment where triangles `a` and `b` cross, if they do.
fn tri_tri(a: &Tri, b: &Tri) -> Option<[Point3; 2]> {
    let plane_of = |t: &Tri| -> Option<(Point3, Vector3)> {
        let n = (t[1] - t[0]).cross(&(t[2] - t[0])).try_normalize(1e-14)?;
        Some((t[0], n))
    };
    let (pa, na) = plane_of(a)?;
    let (pb, nb) = plane_of(b)?;
    // Each triangle cut by the other's plane: a segment on the line both
    // planes share; the overlap of the two segments along it.
    let sa = cut_triangle(a, pb, nb)?;
    let sb = cut_triangle(b, pa, na)?;
    let dir = na.cross(&nb).try_normalize(1e-12)?;
    let along = |p: &Point3| (p - sa[0]).dot(&dir);
    let (a0, a1) = (along(&sa[0]), along(&sa[1]));
    let (b0, b1) = (along(&sb[0]), along(&sb[1]));
    let lo = a0.min(a1).max(b0.min(b1));
    let hi = a0.max(a1).min(b0.max(b1));
    if hi - lo <= 1e-9 {
        return None;
    }
    let at = |t: Real| {
        // On segment a at parameter t along `dir`.
        let u = if (a1 - a0).abs() < 1e-14 {
            0.0
        } else {
            (t - a0) / (a1 - a0)
        };
        sa[0] + (sa[1] - sa[0]) * u
    };
    Some([at(lo), at(hi)])
}

fn bbox(t: &Tri) -> (Point3, Point3) {
    let mut lo = t[0];
    let mut hi = t[0];
    for p in &t[1..] {
        for i in 0..3 {
            lo[i] = lo[i].min(p[i]);
            hi[i] = hi[i].max(p[i]);
        }
    }
    (lo, hi)
}

/// Where two triangle sets cross (boxes first: only pairs whose boxes meet
/// are intersected).
pub fn mesh_intersection(a: &[Tri], b: &[Tri]) -> Vec<[Point3; 2]> {
    let boxes_b: Vec<(Point3, Point3)> = b.iter().map(bbox).collect();
    let mut out = Vec::new();
    for ta in a {
        let (lo, hi) = bbox(ta);
        for (tb, (l2, h2)) in b.iter().zip(&boxes_b) {
            if (0..3).any(|i| hi[i] < l2[i] - 1e-9 || h2[i] < lo[i] - 1e-9) {
                continue;
            }
            if let Some(s) = tri_tri(ta, tb) {
                out.push(s);
            }
        }
    }
    out
}

/// Segments joined end to end into polylines (ends within `tol`).
pub fn chain_segments(segs: &[[Point3; 2]], tol: Real) -> Vec<Vec<Point3>> {
    let q = |p: &Point3| {
        let k = |x: Real| (x / tol).round() as i64;
        (k(p.x), k(p.y), k(p.z))
    };
    // Each end's neighbours.
    let mut at: HashMap<(i64, i64, i64), Vec<usize>> = HashMap::new();
    for (i, s) in segs.iter().enumerate() {
        if (s[0] - s[1]).norm() <= tol {
            continue;
        }
        at.entry(q(&s[0])).or_default().push(i);
        at.entry(q(&s[1])).or_default().push(i);
    }
    // Short ones and repeats (two faces touching along an edge give it once
    // per pair of triangles) are left out.
    let mut used = vec![false; segs.len()];
    let mut seen = std::collections::HashSet::new();
    for (i, s) in segs.iter().enumerate() {
        let (a, b) = (q(&s[0]), q(&s[1]));
        if (s[0] - s[1]).norm() <= tol || !seen.insert(if a <= b { (a, b) } else { (b, a) }) {
            used[i] = true;
        }
    }
    let mut out = Vec::new();
    for start in 0..segs.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let mut line = vec![segs[start][0], segs[start][1]];
        // Grow forwards, then backwards.
        for back in [false, true] {
            loop {
                let end = if back { line[0] } else { line[line.len() - 1] };
                let next = at
                    .get(&q(&end))
                    .and_then(|v| v.iter().copied().find(|&j| !used[j]));
                let Some(j) = next else { break };
                used[j] = true;
                let s = segs[j];
                let far = if (s[0] - end).norm() <= (s[1] - end).norm() {
                    s[1]
                } else {
                    s[0]
                };
                if back {
                    line.insert(0, far);
                } else {
                    line.push(far);
                }
            }
        }
        out.push(line);
    }
    out
}

/// At most `n` + 1 points evenly along `line` (by length), ends kept.
pub fn resample(line: &[Point3], n: usize) -> Vec<Point3> {
    if line.len() < 2 || n == 0 {
        return line.to_vec();
    }
    let mut acc = vec![0.0];
    for w in line.windows(2) {
        let l = acc[acc.len() - 1] + (w[1] - w[0]).norm();
        acc.push(l);
    }
    let total = acc[acc.len() - 1];
    if total <= 1e-12 {
        return vec![line[0]];
    }
    let mut out = Vec::with_capacity(n + 1);
    let mut k = 0;
    for i in 0..=n {
        let s = total * i as Real / n as Real;
        while k + 1 < acc.len() - 1 && acc[k + 1] < s {
            k += 1;
        }
        let seg = (acc[k + 1] - acc[k]).max(1e-300);
        let u = ((s - acc[k]) / seg).clamp(0.0, 1.0);
        out.push(line[k] + (line[k + 1] - line[k]) * u);
    }
    out
}

/// Whether `line` is straight (every point within `tol` of the chord).
pub fn is_straight(line: &[Point3], tol: Real) -> bool {
    let (a, b) = (line[0], line[line.len() - 1]);
    let Some(d) = (b - a).try_normalize(1e-12) else {
        return false;
    };
    line.iter().all(|p| {
        let v = p - a;
        (v - d * v.dot(&d)).norm() <= tol
    })
}

/// The nearest hit of the ray `o` + t·`d` (both senses when `both`) on the
/// triangles: the point.
pub fn ray_hit(tris: &[Tri], o: Point3, d: Vector3, both: bool) -> Option<Point3> {
    let mut best: Option<Real> = None;
    for t in tris {
        let (e1, e2) = (t[1] - t[0], t[2] - t[0]);
        let h = d.cross(&e2);
        let det = e1.dot(&h);
        if det.abs() < 1e-14 {
            continue;
        }
        let s = o - t[0];
        let u = s.dot(&h) / det;
        let qv = s.cross(&e1);
        let v = d.dot(&qv) / det;
        let tt = e2.dot(&qv) / det;
        if u < -1e-9 || v < -1e-9 || u + v > 1.0 + 1e-9 || (!both && tt < 0.0) {
            continue;
        }
        if best.is_none_or(|b| tt.abs() < b.abs()) {
            best = Some(tt);
        }
    }
    best.map(|t| o + d * t)
}

/// The point of the triangles nearest `p`.
pub fn closest_on(tris: &[Tri], p: Point3) -> Option<Point3> {
    tris.iter()
        .map(|t| crate::kernel::exact::closest_on_triangle(&p, &t[0], &t[1], &t[2]))
        .min_by(|a, b| (a - p).norm_squared().total_cmp(&(b - p).norm_squared()))
}

/// How Project to Surface carries a point onto the faces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Projection {
    /// Along a direction (either sense, the nearer hit).
    Along(Vector3),
    /// To the faces' nearest point.
    Closest,
}

/// The polylines `curves` projected onto `tris`: points that miss the faces
/// break a polyline (a piece needs two points).
pub fn project_onto(curves: &[Vec<Point3>], tris: &[Tri], how: Projection) -> Vec<Vec<Point3>> {
    let mut out = Vec::new();
    for c in curves {
        let mut cur: Vec<Point3> = Vec::new();
        for &p in c {
            let q = match how {
                Projection::Along(d) => d
                    .try_normalize(1e-12)
                    .and_then(|d| ray_hit(tris, p, d, true)),
                Projection::Closest => closest_on(tris, p),
            };
            match q {
                Some(q) => cur.push(q),
                None => {
                    if cur.len() >= 2 {
                        out.push(std::mem::take(&mut cur));
                    }
                    cur.clear();
                }
            }
        }
        if cur.len() >= 2 {
            out.push(cur);
        }
    }
    out
}

/// The silhouette of the triangles `faces` (one list per face of a body) seen
/// along `dir`: the edges between a triangle facing `dir` and one facing away,
/// inside a face or between faces meeting smoothly (within 30°); sharp model
/// edges are edges already.
pub fn silhouette(faces: &[Vec<(Tri, Vector3)>], dir: Vector3) -> Vec<[Point3; 2]> {
    let Some(d) = dir.try_normalize(1e-12) else {
        return Vec::new();
    };
    let key = |p: &Point3| {
        let k = |x: Real| (x * 1e5).round() as i64;
        (k(p.x), k(p.y), k(p.z))
    };
    type K = (i64, i64, i64);
    /// The normals of the triangles along an edge, and the edge.
    type Sides = Vec<(Vector3, [Point3; 2])>;
    let mut edges: HashMap<(K, K), Sides> = HashMap::new();
    for f in faces {
        for (t, n) in f {
            for i in 0..3 {
                let (a, b) = (t[i], t[(i + 1) % 3]);
                let (ka, kb) = (key(&a), key(&b));
                let k = if ka <= kb { (ka, kb) } else { (kb, ka) };
                edges.entry(k).or_default().push((*n, [a, b]));
            }
        }
    }
    let smooth = 30f64.to_radians().cos();
    edges
        .into_values()
        .filter_map(|v| {
            let [(n1, s), (n2, _)] = v.as_slice() else {
                return None;
            };
            let (a, b) = (n1.dot(&d), n2.dot(&d));
            ((a > 0.0) != (b > 0.0) && n1.dot(n2) >= smooth).then_some(*s)
        })
        .collect()
}

/// Which coordinates an equation curve's three expressions give.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EquationKind {
    /// x(t), y(t), z(t).
    #[default]
    Cartesian,
    /// r(t), θ(t), z(t).
    Cylindrical,
    /// r(t), θ(t), φ(t) (φ from the Z axis).
    Spherical,
}

/// Points of the equation curve: `exprs` (lengths in mm, angles in degrees
/// when bare) for `t` from `t0` to `t1` in `n` steps; `vars` gives the
/// part's parameters.
pub fn equation_points(
    kind: EquationKind,
    exprs: [&str; 3],
    (t0, t1): (Real, Real),
    n: usize,
    vars: &crate::part::expr::Vars<'_>,
) -> Result<Vec<Point3>, String> {
    use crate::part::expr::{Unit, Value, eval};
    use crate::part::units::NONE;
    if !(t0.is_finite() && t1.is_finite() && t1 > t0) {
        return Err("Equation Curve: t max must be greater than t min".into());
    }
    let n = n.clamp(2, 400);
    let units = match kind {
        EquationKind::Cartesian => [Unit::Mm, Unit::Mm, Unit::Mm],
        EquationKind::Cylindrical => [Unit::Mm, Unit::Deg, Unit::Mm],
        EquationKind::Spherical => [Unit::Mm, Unit::Deg, Unit::Deg],
    };
    let mut out = Vec::with_capacity(n + 1);
    for i in 0..=n {
        let t = t0 + (t1 - t0) * i as Real / n as Real;
        let lookup = |name: &str| {
            if name == "t" {
                Some(Value::Num(t, NONE))
            } else {
                vars(name)
            }
        };
        let mut v = [0.0; 3];
        for k in 0..3 {
            v[k] = eval(exprs[k], units[k], &lookup)
                .map_err(|e| format!("Equation Curve: {}: {e}", ["first", "second", "third"][k]))?
                .0;
        }
        out.push(match kind {
            EquationKind::Cartesian => Point3::new(v[0], v[1], v[2]),
            EquationKind::Cylindrical => Point3::new(v[0] * v[1].cos(), v[0] * v[1].sin(), v[2]),
            EquationKind::Spherical => Point3::new(
                v[0] * v[2].sin() * v[1].cos(),
                v[0] * v[2].sin() * v[1].sin(),
                v[0] * v[2].cos(),
            ),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(z: Real, s: Real) -> Vec<Tri> {
        let p = |x: Real, y: Real| Point3::new(x, y, z);
        vec![
            [p(0.0, 0.0), p(s, 0.0), p(s, s)],
            [p(0.0, 0.0), p(s, s), p(0.0, s)],
        ]
    }

    #[test]
    fn a_plane_cuts_a_square_in_one_line() {
        let t = square(0.0, 10.0);
        let pl = Plane::new(Point3::new(4.0, 0.0, 0.0), Vector3::x(), Vector3::y()).unwrap();
        let segs = plane_section(&t, &pl);
        let lines = chain_segments(&segs, 1e-6);
        assert_eq!(lines.len(), 1);
        assert!(is_straight(&lines[0], 1e-9));
        let len: Real = lines[0].windows(2).map(|w| (w[1] - w[0]).norm()).sum();
        assert!((len - 10.0).abs() < 1e-9, "{len}");
    }

    #[test]
    fn two_crossing_squares_meet_in_a_segment() {
        let a = square(0.0, 10.0);
        // A vertical square through x = 5.
        let p = |y: Real, z: Real| Point3::new(5.0, y, z);
        let b = vec![
            [p(-2.0, -3.0), p(12.0, -3.0), p(12.0, 3.0)],
            [p(-2.0, -3.0), p(12.0, 3.0), p(-2.0, 3.0)],
        ];
        let lines = chain_segments(&mesh_intersection(&a, &b), 1e-6);
        assert_eq!(lines.len(), 1);
        let len: Real = lines[0].windows(2).map(|w| (w[1] - w[0]).norm()).sum();
        assert!((len - 10.0).abs() < 1e-6, "{len}");
    }

    #[test]
    fn faces_meeting_at_an_edge_give_the_edge() {
        let a = square(0.0, 10.0);
        // A wall standing on the square's edge x = 10.
        let p = |y: Real, z: Real| Point3::new(10.0, y, z);
        let b = vec![
            [p(0.0, 0.0), p(10.0, 0.0), p(10.0, 5.0)],
            [p(0.0, 0.0), p(10.0, 5.0), p(0.0, 5.0)],
        ];
        let lines = chain_segments(&mesh_intersection(&a, &b), 1e-6);
        assert_eq!(lines.len(), 1, "{lines:?}");
        let len: Real = lines[0].windows(2).map(|w| (w[1] - w[0]).norm()).sum();
        assert!((len - 10.0).abs() < 1e-6, "{len}");
    }

    #[test]
    fn projection_along_and_to_the_closest_point() {
        let t = square(5.0, 10.0);
        let c = vec![vec![
            Point3::new(1.0, 1.0, 0.0),
            Point3::new(9.0, 1.0, 0.0),
            Point3::new(20.0, 1.0, 0.0),
        ]];
        let along = project_onto(&c, &t, Projection::Along(Vector3::z()));
        assert_eq!(along.len(), 1);
        assert_eq!(along[0].len(), 2);
        assert!((along[0][0].z - 5.0).abs() < 1e-9);
        let near = project_onto(&c, &t, Projection::Closest);
        assert_eq!(near[0].len(), 3);
        assert!((near[0][2] - Point3::new(10.0, 1.0, 5.0)).norm() < 1e-9);
    }

    #[test]
    fn a_cylinder_has_two_silhouette_lines() {
        // A faceted cylinder r 5 along z, 10 long, seen along y.
        let n = 48;
        let mut face = Vec::new();
        for i in 0..n {
            let a0 = std::f64::consts::TAU * i as Real / n as Real;
            let a1 = std::f64::consts::TAU * (i + 1) as Real / n as Real;
            let p = |a: Real, z: Real| Point3::new(5.0 * a.cos(), 5.0 * a.sin(), z);
            let nm = Vector3::new(((a0 + a1) / 2.0).cos(), ((a0 + a1) / 2.0).sin(), 0.0);
            face.push(([p(a0, 0.0), p(a1, 0.0), p(a1, 10.0)], nm));
            face.push(([p(a0, 0.0), p(a1, 10.0), p(a0, 10.0)], nm));
        }
        let lines = chain_segments(&silhouette(&[face], Vector3::y()), 1e-6);
        assert_eq!(lines.len(), 2, "{lines:?}");
        for l in &lines {
            assert!(is_straight(l, 1e-9));
            assert!((l[0].x.abs() - 5.0).abs() < 1e-9, "{l:?}");
        }
    }

    #[test]
    fn equation_curve_of_a_helix() {
        let pts = equation_points(
            EquationKind::Cylindrical,
            ["10", "t * 360", "t * 5"],
            (0.0, 2.0),
            48,
            &|_| None,
        )
        .unwrap();
        assert_eq!(pts.len(), 49);
        assert!(
            (pts[48] - Point3::new(10.0, 0.0, 10.0)).norm() < 1e-9,
            "{:?}",
            pts[48]
        );
        assert!(
            (pts[12] - Point3::new(-10.0, 0.0, 2.5)).norm() < 1e-9,
            "{:?}",
            pts[12]
        );
        assert!(
            equation_points(
                EquationKind::Cartesian,
                ["t", "q", "0"],
                (0.0, 1.0),
                4,
                &|_| None
            )
            .is_err()
        );
        assert!(
            equation_points(
                EquationKind::Cartesian,
                ["t", "0", "0"],
                (1.0, 0.0),
                4,
                &|_| None
            )
            .is_err()
        );
    }

    #[test]
    fn resample_keeps_the_ends() {
        let l = vec![
            Point3::origin(),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(1.0, 3.0, 0.0),
        ];
        let r = resample(&l, 8);
        assert_eq!(r.len(), 9);
        assert!((r[8] - l[2]).norm() < 1e-12);
        assert!((r[2] - Point3::new(1.0, 0.0, 0.0)).norm() < 1e-12);
    }
}
