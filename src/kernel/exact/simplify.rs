//! Simplify's geometry (`docs/spec/2.18-simplify.md`, `docs/spec/3.11-simplify.md`): envelopes
//! (bounding boxes and cylinders round a set of bodies), filled voids, and which bodies can be
//! seen from outside at all (Remove internal bodies / parts).
//!
//! Every function here reads a body through its display mesh (chord tolerance 0.2 % of its
//! size): an envelope is a coarse stand-in by definition, and the visibility test asks only
//! whether a ray gets out.

#[allow(unused_imports)]
use super::compat::prelude::*;
use super::{ExactSolid, box_named, guarded, moved, names_from, p3, revolved_tool, sk};
use crate::kernel::{Point3, Real, Vector3};

/// The shape of an envelope (Inventor's Envelope Type).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvelopeShape {
    /// A box along the part's axes.
    Box,
    /// The smallest box found along the bodies' principal directions (or the part's axes,
    /// when that is smaller).
    OrientedBox,
    /// The smallest cylinder found about the principal directions or the part's axes.
    Cylinder,
}

/// The points of `s`'s display mesh.
pub fn mesh_points(s: &ExactSolid) -> Result<Vec<Point3>, String> {
    let tol = (s.size() * 2e-3).max(1e-3);
    let mesh = guarded("tessellate", || s.body.tessellate(tol))?;
    Ok(mesh.positions.iter().map(|p| p3(*p)).collect())
}

/// The triangles of `s`'s display mesh.
pub fn mesh_triangles(s: &ExactSolid) -> Result<Vec<[Point3; 3]>, String> {
    let tol = (s.size() * 2e-3).max(1e-3);
    let mesh = guarded("tessellate", || s.body.tessellate(tol))?;
    Ok(mesh
        .triangles
        .iter()
        .filter_map(|t| mesh.tri(t))
        .map(|[a, b, c]| [p3(a), p3(b), p3(c)])
        .collect())
}

/// `s` with every void filled with material, and how many there were.
pub fn fill_voids(s: &ExactSolid) -> Result<(ExactSolid, usize), String> {
    let (body, n) = guarded("fill voids", || sk::fill_voids(&s.body))?;
    if n == 0 {
        return Ok((s.clone(), 0));
    }
    let face_names = names_from(&body, &[s], &|_| None);
    Ok((
        ExactSolid {
            prism: None,
            sheet: s.sheet,
            name: s.name.clone(),
            body,
            face_names,
            profiles: Vec::new(),
        },
        n,
    ))
}

/// The axis-aligned box of `points` (none for no points).
fn aabb(points: &[Point3]) -> Option<(Point3, Point3)> {
    let first = points.first()?;
    Some(
        points
            .iter()
            .fold((*first, *first), |(lo, hi), p| (lo.inf(p), hi.sup(p))),
    )
}

/// Three orthonormal directions, right-handed: the principal directions of `points`.
fn principal(points: &[Point3]) -> [Vector3; 3] {
    let n = points.len().max(1) as Real;
    let c = points.iter().fold(Vector3::zeros(), |a, p| a + p.coords) / n;
    let mut m = nalgebra::Matrix3::<Real>::zeros();
    for p in points {
        let d = p.coords - c;
        m += d * d.transpose();
    }
    let e = nalgebra::SymmetricEigen::new(m);
    let mut cols: Vec<(Real, Vector3)> = (0..3)
        .map(|i| (e.eigenvalues[i], e.eigenvectors.column(i).into_owned()))
        .collect();
    cols.sort_by(|a, b| b.0.total_cmp(&a.0));
    let x = cols[0].1.try_normalize(1e-12).unwrap_or_else(Vector3::x);
    let y0 = cols[1].1 - x * cols[1].1.dot(&x);
    let y = y0.try_normalize(1e-12).unwrap_or_else(|| {
        let t = if x.x.abs() < 0.9 {
            Vector3::x()
        } else {
            Vector3::y()
        };
        (t - x * t.dot(&x)).normalize()
    });
    [x, y, x.cross(&y)]
}

/// The extents of `points` along the frame `axes`: (min, max) per axis.
fn extents(points: &[Point3], axes: &[Vector3; 3]) -> [(Real, Real); 3] {
    let mut out = [(Real::INFINITY, Real::NEG_INFINITY); 3];
    for p in points {
        for (k, a) in axes.iter().enumerate() {
            let t = p.coords.dot(a);
            out[k] = (out[k].0.min(t), out[k].1.max(t));
        }
    }
    out
}

fn frame_matrix(axes: &[Vector3; 3]) -> nalgebra::Matrix4<Real> {
    let mut m = nalgebra::Matrix4::<Real>::identity();
    for (c, a) in axes.iter().enumerate() {
        for r in 0..3 {
            m[(r, c)] = a[r];
        }
    }
    m
}

/// The smallest circle round 2D points (Welzl's algorithm, iterative, in a fixed shuffled
/// order so the result does not depend on chance): centre and radius.
fn enclosing_circle(pts: &[[Real; 2]]) -> ([Real; 2], Real) {
    let mut p: Vec<[Real; 2]> = pts.to_vec();
    // A fixed pseudo-random order (Welzl needs one to be fast on sorted input).
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    for i in (1..p.len()).rev() {
        seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let j = (seed >> 33) as usize % (i + 1);
        p.swap(i, j);
    }
    let d = |a: [Real; 2], b: [Real; 2]| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt();
    let inside = |c: [Real; 2], r: Real, q: [Real; 2]| d(c, q) <= r * (1.0 + 1e-12) + 1e-12;
    let two =
        |a: [Real; 2], b: [Real; 2]| ([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0], d(a, b) / 2.0);
    let three = |a: [Real; 2], b: [Real; 2], c: [Real; 2]| {
        let (bx, by) = (b[0] - a[0], b[1] - a[1]);
        let (cx, cy) = (c[0] - a[0], c[1] - a[1]);
        let den = 2.0 * (bx * cy - by * cx);
        if den.abs() < 1e-18 {
            // Collinear: the two farthest apart.
            let cands = [two(a, b), two(a, c), two(b, c)];
            return cands
                .into_iter()
                .max_by(|x, y| x.1.total_cmp(&y.1))
                .unwrap_or(two(a, b));
        }
        let b2 = bx * bx + by * by;
        let c2 = cx * cx + cy * cy;
        let ux = (cy * b2 - by * c2) / den;
        let uy = (bx * c2 - cx * b2) / den;
        ([a[0] + ux, a[1] + uy], (ux * ux + uy * uy).sqrt())
    };
    let Some(&first) = p.first() else {
        return ([0.0, 0.0], 0.0);
    };
    let (mut c, mut r) = (first, 0.0);
    for i in 1..p.len() {
        if inside(c, r, p[i]) {
            continue;
        }
        (c, r) = (p[i], 0.0);
        for j in 0..i {
            if inside(c, r, p[j]) {
                continue;
            }
            (c, r) = two(p[i], p[j]);
            for k in 0..j {
                if !inside(c, r, p[k]) {
                    (c, r) = three(p[i], p[j], p[k]);
                }
            }
        }
    }
    (c, r)
}

/// An envelope round `points` of shape `shape`, its faces named `<prefix>:<role>` (a box's
/// `Top`, `Bottom`, `Left`… in its own frame, a cylinder's `Bottom`, `Side`, `Top`) and the
/// body `prefix`. A set of points with no extent in some direction gets 1 µm there.
pub fn envelope(
    points: &[Point3],
    shape: EnvelopeShape,
    prefix: &str,
) -> Result<ExactSolid, String> {
    let (lo, hi) = aabb(points).ok_or("nothing to put an envelope round")?;
    let pad = |lo: Real, hi: Real| {
        if hi - lo < 1e-3 {
            let m = (lo + hi) / 2.0;
            (m - 5e-4, m + 5e-4)
        } else {
            (lo, hi)
        }
    };
    let world = [Vector3::x(), Vector3::y(), Vector3::z()];
    match shape {
        EnvelopeShape::Box => {
            let (x0, x1) = pad(lo.x, hi.x);
            let (y0, y1) = pad(lo.y, hi.y);
            let (z0, z1) = pad(lo.z, hi.z);
            box_named(Point3::new(x0, y0, z0), Point3::new(x1, y1, z1), prefix)
        }
        EnvelopeShape::OrientedBox => {
            let axes = principal(points);
            let vol =
                |e: &[(Real, Real); 3]| e.iter().map(|(a, b)| (b - a).max(1e-3)).product::<Real>();
            let ew = extents(points, &world);
            let ep = extents(points, &axes);
            if vol(&ew) <= vol(&ep) * (1.0 + 1e-9) {
                return envelope(points, EnvelopeShape::Box, prefix);
            }
            let (x0, x1) = pad(ep[0].0, ep[0].1);
            let (y0, y1) = pad(ep[1].0, ep[1].1);
            let (z0, z1) = pad(ep[2].0, ep[2].1);
            let local = box_named(Point3::new(x0, y0, z0), Point3::new(x1, y1, z1), prefix)?;
            let mut placed = moved(&local, &frame_matrix(&axes))?;
            placed.name = Some(prefix.to_owned());
            Ok(placed)
        }
        EnvelopeShape::Cylinder => {
            let pa = principal(points);
            let mut best: Option<(Real, Point3, Vector3, Real, Real)> = None;
            for axis in world.iter().chain(pa.iter()) {
                let u = if axis.x.abs() < 0.9 {
                    Vector3::x()
                } else {
                    Vector3::y()
                };
                let u = (u - axis * u.dot(axis)).normalize();
                let v = axis.cross(&u);
                let flat: Vec<[Real; 2]> = points
                    .iter()
                    .map(|p| [p.coords.dot(&u), p.coords.dot(&v)])
                    .collect();
                let (c, r) = enclosing_circle(&flat);
                let (h0, h1) =
                    points
                        .iter()
                        .fold((Real::INFINITY, Real::NEG_INFINITY), |(a, b), p| {
                            let t = p.coords.dot(axis);
                            (a.min(t), b.max(t))
                        });
                let (h0, h1) = pad(h0, h1);
                let r = r.max(5e-4);
                let vol = std::f64::consts::PI * r * r * (h1 - h0);
                if best.as_ref().is_none_or(|b| vol < b.0 * (1.0 - 1e-9)) {
                    let base = Point3::from(u * c[0] + v * c[1] + axis * h0);
                    best = Some((vol, base, *axis, r, h1 - h0));
                }
            }
            let (_, base, axis, r, h) = best.ok_or("nothing to put an envelope round")?;
            let mut s = revolved_tool(
                base,
                axis,
                0.0,
                &[[0.0, 0.0], [r, 0.0], [r, h], [0.0, h]],
                &["Bottom", "Side", "Top", "Axis"],
                prefix,
            )?;
            s.name = Some(prefix.to_owned());
            Ok(s)
        }
    }
}

/// A ray against a triangle (Möller–Trumbore): the distance along `dir` when it hits in
/// front of `o`.
fn ray_hits(o: &Point3, dir: &Vector3, t: &[Point3; 3]) -> Option<Real> {
    let e1 = t[1] - t[0];
    let e2 = t[2] - t[0];
    let p = dir.cross(&e2);
    let det = e1.dot(&p);
    if det.abs() < 1e-14 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - t[0];
    let u = s.dot(&p) * inv;
    if !(-1e-9..=1.0 + 1e-9).contains(&u) {
        return None;
    }
    let q = s.cross(&e1);
    let v = dir.dot(&q) * inv;
    if v < -1e-9 || u + v > 1.0 + 1e-9 {
        return None;
    }
    let d = e2.dot(&q) * inv;
    (d > 0.0).then_some(d)
}

fn ray_meets_box(o: &Point3, dir: &Vector3, lo: &Point3, hi: &Point3) -> bool {
    let (mut t0, mut t1) = (0.0_f64, Real::INFINITY);
    for k in 0..3 {
        if dir[k].abs() < 1e-15 {
            if o[k] < lo[k] || o[k] > hi[k] {
                return false;
            }
            continue;
        }
        let a = (lo[k] - o[k]) / dir[k];
        let b = (hi[k] - o[k]) / dir[k];
        t0 = t0.max(a.min(b));
        t1 = t1.min(a.max(b));
        if t0 > t1 {
            return false;
        }
    }
    true
}

/// The 14 directions Inventor looks from: the 6 along the axes and the 8 corners.
pub fn view_directions() -> Vec<Vector3> {
    let mut d = Vec::with_capacity(14);
    for k in 0..3 {
        for s in [-1.0, 1.0] {
            let mut v = Vector3::zeros();
            v[k] = s;
            d.push(v);
        }
    }
    for x in [-1.0, 1.0] {
        for y in [-1.0, 1.0] {
            for z in [-1.0, 1.0] {
                d.push(Vector3::new(x, y, z).normalize());
            }
        }
    }
    d
}

/// Which of `meshes` (triangles per body) can be seen from outside from any of the 14 view
/// directions: a body is seen when a ray from a point of its surface towards the viewer leaves
/// every other body's triangles behind it. Up to `samples` points per body are tried (its mesh
/// points, evenly picked, and its triangles' centres).
pub fn visible_from_outside(meshes: &[Vec<[Point3; 3]>], samples: usize) -> Vec<bool> {
    let boxes: Vec<Option<(Point3, Point3)>> = meshes
        .iter()
        .map(|m| {
            let pts: Vec<Point3> = m.iter().flatten().copied().collect();
            aabb(&pts)
        })
        .collect();
    let dirs = view_directions();
    let size = boxes
        .iter()
        .flatten()
        .map(|(lo, hi)| (hi - lo).norm())
        .fold(0.0, Real::max)
        .max(1e-3);
    let eps = size * 1e-6;
    meshes
        .iter()
        .enumerate()
        .map(|(i, m)| {
            if m.is_empty() {
                return false;
            }
            let step = (m.len() / samples.max(1)).max(1);
            let points = m.iter().step_by(step).flat_map(|t| {
                let c = Point3::from((t[0].coords + t[1].coords + t[2].coords) / 3.0);
                [c, t[0]]
            });
            for p in points {
                for d in &dirs {
                    let o = p + d * eps;
                    let blocked = meshes.iter().enumerate().any(|(j, other)| {
                        j != i
                            && boxes[j]
                                .as_ref()
                                .is_some_and(|(lo, hi)| ray_meets_box(&o, d, lo, hi))
                            && other.iter().any(|t| ray_hits(&o, d, t).is_some())
                    });
                    if !blocked {
                        return true;
                    }
                }
            }
            false
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube(a: Real, b: Real, name: &str) -> ExactSolid {
        box_named(Point3::new(a, a, a), Point3::new(b, b, b), name)
            .unwrap_or_else(|e| panic!("{e}"))
    }

    fn volume(s: &ExactSolid) -> Real {
        s.measure().map(|m| m.0).unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn a_box_envelope_is_the_bounding_box() {
        let s = cube(0.0, 10.0, "A");
        let pts = mesh_points(&s).unwrap_or_else(|e| panic!("{e}"));
        let e = envelope(&pts, EnvelopeShape::Box, "E").unwrap_or_else(|e| panic!("{e}"));
        assert!((volume(&e) - 1000.0).abs() < 1e-6);
        assert_eq!(e.name.as_deref(), Some("E"));
        assert!(e.face_names.iter().any(|n| n.as_deref() == Some("E:Top")));
    }

    #[test]
    fn an_oriented_box_round_a_turned_bar_is_smaller_than_the_axis_box() {
        // A 40 x 4 x 4 bar turned 45° about Z.
        let s = box_named(
            Point3::new(-20.0, -2.0, 0.0),
            Point3::new(20.0, 2.0, 4.0),
            "B",
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let r =
            nalgebra::Rotation3::from_axis_angle(&Vector3::z_axis(), std::f64::consts::FRAC_PI_4);
        let m = r.to_homogeneous();
        let s = moved(&s, &m).unwrap_or_else(|e| panic!("{e}"));
        let pts = mesh_points(&s).unwrap_or_else(|e| panic!("{e}"));
        let axis = envelope(&pts, EnvelopeShape::Box, "E").unwrap_or_else(|e| panic!("{e}"));
        let tight =
            envelope(&pts, EnvelopeShape::OrientedBox, "E").unwrap_or_else(|e| panic!("{e}"));
        assert!((volume(&tight) - 640.0).abs() < 1.0, "{}", volume(&tight));
        assert!(volume(&axis) > 3.0 * volume(&tight));
    }

    #[test]
    fn a_cylinder_envelope_round_a_cube() {
        let s = cube(0.0, 10.0, "A");
        let pts = mesh_points(&s).unwrap_or_else(|e| panic!("{e}"));
        let c = envelope(&pts, EnvelopeShape::Cylinder, "C").unwrap_or_else(|e| panic!("{e}"));
        // The circle round a 10 x 10 square: r = 5·√2; height 10.
        let expected = std::f64::consts::PI * 50.0 * 10.0;
        assert!(
            (volume(&c) - expected).abs() < expected * 0.01,
            "{}",
            volume(&c)
        );
    }

    #[test]
    fn a_void_is_filled() {
        let outer = cube(0.0, 10.0, "A");
        let inner = cube(3.0, 7.0, "V");
        let hollow = super::super::boolean(&outer, &inner, super::super::ExactOp::Cut)
            .unwrap_or_else(|e| panic!("{e}"))
            .unwrap_or_else(|| panic!("no result"));
        assert!((volume(&hollow) - 936.0).abs() < 1e-6);
        let (full, n) = fill_voids(&hollow).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(n, 1);
        assert!((volume(&full) - 1000.0).abs() < 1e-6, "{}", volume(&full));
    }

    #[test]
    fn a_body_inside_another_is_not_seen() {
        let shell = {
            let outer = cube(0.0, 10.0, "A");
            let inner = cube(2.0, 8.0, "V");
            super::super::boolean(&outer, &inner, super::super::ExactOp::Cut)
                .unwrap_or_else(|e| panic!("{e}"))
                .unwrap_or_else(|| panic!("no result"))
        };
        let core = cube(4.0, 6.0, "C");
        let beside = box_named(
            Point3::new(20.0, 0.0, 0.0),
            Point3::new(22.0, 2.0, 2.0),
            "B",
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let meshes: Vec<_> = [&shell, &core, &beside]
            .iter()
            .map(|s| mesh_triangles(s).unwrap_or_else(|e| panic!("{e}")))
            .collect();
        assert_eq!(visible_from_outside(&meshes, 64), vec![true, false, true]);
    }
}
