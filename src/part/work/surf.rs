//! Geometry of curved inputs for work features
//! (`docs/spec/2.10-work-features.md`): axes and centres of analytic
//! surfaces, tangent planes, line–surface intersections, the centre of a
//! circular or elliptical edge and the tangent of a curve at a point.
//!
//! Everything works on the *infinite* analytic surface of a face (its
//! [`Surface`], or the exact surface a faceted face approximates), in
//! part-local coordinates.

use crate::kernel::{BRep, Curve, Plane, Point3, Real, Surface, Vector3};

/// Absolute + relative length tolerance for "on the surface" tests.
fn tol(scale: Real) -> Real {
    1e-6 * (1.0 + scale.abs())
}

/// Any unit vector perpendicular to `n`.
pub(super) fn perpendicular(n: &Vector3) -> Vector3 {
    let t = if n.x.abs() < 0.9 {
        Vector3::x()
    } else {
        Vector3::y()
    };
    (t - n * t.dot(n)).normalize()
}

fn plane(o: Point3, n: Vector3, x: Vector3) -> Result<Plane, String> {
    Plane::new(o, n, x).map_err(|e| format!("degenerate plane: {e}"))
}

/// The analytic surface of face `name` (for a planar facet of a curved
/// face: the exact surface it approximates).
pub(super) fn face_surface(body: &BRep, name: &str) -> Result<Surface, String> {
    let id = body
        .find_face(name)
        .ok_or_else(|| format!("face '{name}' no longer exists"))?;
    let f = body
        .face(id)
        .ok_or_else(|| format!("face '{name}' no longer exists"))?;
    body.surface(f.facet_of.unwrap_or(f.surface))
        .cloned()
        .ok_or_else(|| format!("face '{name}' has no surface"))
}

/// Short name of a surface kind for messages.
fn kind(s: &Surface) -> &'static str {
    match s {
        Surface::Plane(_) => "a plane",
        Surface::Cylinder(_) => "a cylinder",
        Surface::Cone(_) => "a cone",
        Surface::Sphere(_) => "a sphere",
        Surface::Torus(_) => "a torus",
        Surface::Extrusion(_) => "a free-form surface",
    }
}

/// **Through Revolved Face**: the axis of a cylinder, cone or torus.
pub(super) fn axis_of(s: &Surface) -> Result<(Point3, Vector3), String> {
    match s {
        Surface::Cylinder(c) => Ok((c.origin, c.axis.into_inner())),
        Surface::Cone(c) => Ok((c.origin, c.axis.into_inner())),
        Surface::Torus(t) => Ok((t.center, t.axis.into_inner())),
        other => Err(format!(
            "the face is {}, not a cylinder, cone or torus",
            kind(other)
        )),
    }
}

/// **Center Point of Torus**.
pub(super) fn torus_center(s: &Surface) -> Result<Point3, String> {
    match s {
        Surface::Torus(t) => Ok(t.center),
        other => Err(format!("the face is {}, not a torus", kind(other))),
    }
}

/// **Center Point of Sphere**.
pub(super) fn sphere_center(s: &Surface) -> Result<Point3, String> {
    match s {
        Surface::Sphere(t) => Ok(t.center),
        other => Err(format!("the face is {}, not a sphere", kind(other))),
    }
}

/// **Midplane of Torus**: through the centre, normal along the axis.
pub(super) fn torus_midplane(s: &Surface) -> Result<Plane, String> {
    match s {
        Surface::Torus(t) => plane(t.center, t.axis.into_inner(), t.x_axis.into_inner()),
        other => Err(format!("the face is {}, not a torus", kind(other))),
    }
}

/// The tangent planes about a "core" line (`c`, unit `dl`) of radius `r`
/// that contain the line through `q` along `dl` (cylinder through an
/// axis-parallel edge or point; sphere through a line, with `dl` the line
/// direction). `slide`: the tangency is a line along `dl` (cylinder), the
/// plane origin is put level with `q`; else a point (sphere). Two
/// solutions when `q` is outside: `other` takes the second one. X is along
/// `dl`, Y points from the tangency towards `q` (Inventor).
fn tangent_about(
    c: Point3,
    dl: Vector3,
    r: Real,
    q: Point3,
    slide: bool,
    other: bool,
) -> Result<Plane, String> {
    let along = dl * dl.dot(&(q - c));
    let w = (q - c) - along;
    let l = w.norm();
    let t = tol(r + l);
    if l < r - t {
        return Err("the input lies inside the surface".into());
    }
    let wh = if l > 1e-12 { w / l } else { perpendicular(&dl) };
    let n = if l <= r + t {
        wh
    } else {
        let phi = (r / l).clamp(-1.0, 1.0).acos();
        let s = if other { -1.0 } else { 1.0 };
        wh * phi.cos() + dl.cross(&wh) * (s * phi.sin())
    };
    let at = if slide { c + along + n * r } else { c + n * r };
    let mut x = dl;
    if n.cross(&x).dot(&(q - at)) < 0.0 {
        x = -x;
    }
    plane(at, n, x)
}

/// **Tangent to Surface Through Edge**: the plane through the line
/// (`p`, `d`) tangent to a cylinder (line parallel to its axis) or a
/// sphere.
pub(super) fn tangent_through_line(
    s: &Surface,
    p: Point3,
    d: Vector3,
    other: bool,
) -> Result<Plane, String> {
    let d = d.try_normalize(1e-12).ok_or("the edge has no direction")?;
    match s {
        Surface::Cylinder(c) => {
            let a = c.axis.into_inner();
            if a.cross(&d).norm() > 1e-6 {
                return Err("the edge is not parallel to the cylinder's axis".into());
            }
            tangent_about(c.origin, a, c.radius, p, true, other)
        }
        Surface::Sphere(sp) => tangent_about(sp.center, d, sp.radius, p, false, other),
        other_s => Err(format!(
            "a tangent plane through an edge needs a cylinder or a sphere, not {}",
            kind(other_s)
        )),
    }
}

/// Signed distance-like value of `p` from the surface (0 on it).
fn signed(s: &Surface, p: &Point3) -> Option<Real> {
    let radial = |o: &Point3, a: &Vector3| {
        let w = p - o;
        let h = w.dot(a);
        ((w - a * h).norm(), h)
    };
    Some(match s {
        Surface::Plane(pl) => (p - pl.origin).dot(&pl.normal),
        Surface::Cylinder(c) => radial(&c.origin, &c.axis.into_inner()).0 - c.radius,
        Surface::Cone(c) => {
            let (rho, h) = radial(&c.origin, &c.axis.into_inner());
            (rho - (c.radius + h * c.half_angle.tan())) * c.half_angle.cos()
        }
        Surface::Sphere(sp) => (p - sp.center).norm() - sp.radius,
        Surface::Torus(t) => {
            let (rho, h) = radial(&t.center, &t.axis.into_inner());
            ((rho - t.major).powi(2) + h * h).sqrt() - t.minor
        }
        Surface::Extrusion(_) => return None,
    })
}

/// **Tangent to Surface Through Point**: on the surface, the tangent plane
/// there (X along the tangency line: the axis direction of a cylinder, the
/// generator of a cone); off a cylinder, the tangent plane through the
/// axis-parallel line through the point (two solutions, `other`).
pub(super) fn tangent_through_point(s: &Surface, q: Point3, other: bool) -> Result<Plane, String> {
    let dist = signed(s, &q).ok_or("tangent planes need an analytic curved surface")?;
    if matches!(s, Surface::Plane(_)) {
        return Err("the face is planar: use Parallel to Plane Through Point".into());
    }
    if dist.abs() <= 1e-4 * (1.0 + q.coords.norm()) {
        let n = s
            .normal_near(&q)
            .ok_or("the surface has no normal at the point")?;
        let along = |a: Vector3| {
            let x = a - n * a.dot(&n);
            x.try_normalize(1e-9).unwrap_or_else(|| perpendicular(&n))
        };
        let x = match s {
            Surface::Cylinder(c) => c.axis.into_inner(),
            Surface::Cone(c) => along(c.axis.into_inner()),
            Surface::Sphere(sp) => along(sp.axis.into_inner()),
            Surface::Torus(t) => along(t.axis.into_inner()),
            _ => perpendicular(&n),
        };
        return plane(q, n, x);
    }
    match s {
        Surface::Cylinder(c) => {
            tangent_about(c.origin, c.axis.into_inner(), c.radius, q, true, other)
        }
        _ => Err(format!(
            "the point is not on the surface ({} has no single tangent plane through it)",
            kind(s)
        )),
    }
}

/// **Tangent to Surface and Parallel to Plane**: the tangent plane with
/// normal `n` (the base plane's); the coordinate system comes from the
/// base plane (`base`: its origin and X). Cylinder (plane parallel to the
/// axis), sphere, torus (normal along or across the axis) and cone (plane
/// along a generator): two sides, `other` takes the far one.
pub(super) fn tangent_parallel(s: &Surface, base: &Plane, other: bool) -> Result<Plane, String> {
    let n = base.normal.into_inner();
    let sign = if other { -1.0 } else { 1.0 };
    let touch = match s {
        Surface::Cylinder(c) => {
            if c.axis.dot(&n).abs() > 1e-6 {
                return Err("the plane is not parallel to the cylinder's axis".into());
            }
            c.origin + n * (sign * c.radius)
        }
        Surface::Sphere(sp) => sp.center + n * (sign * sp.radius),
        Surface::Torus(t) => {
            let a = t.axis.into_inner();
            let along = a.dot(&n);
            if (along.abs() - 1.0).abs() < 1e-6 {
                t.center + n * (sign * t.minor)
            } else if along.abs() < 1e-6 {
                t.center + n * (sign * (t.major + t.minor))
            } else {
                return Err("the plane must be parallel or perpendicular to the torus axis".into());
            }
        }
        Surface::Cone(c) => {
            // The cone's normals all make the same angle with its axis;
            // the plane's normal (or its reverse) must be one of them.
            let a = c.axis.into_inner();
            let k = c.half_angle.tan();
            let want = -k / (1.0 + k * k).sqrt();
            let m = if (n.dot(&a) - want).abs() < 1e-6 {
                n
            } else if (-n.dot(&a) - want).abs() < 1e-6 {
                -n
            } else {
                return Err("the plane is not parallel to a line of the cone".into());
            };
            let radial = m * (1.0 + k * k).sqrt() + a * k;
            c.origin + radial * c.radius
        }
        other_s => {
            return Err(format!(
                "a tangent plane needs a curved analytic face, not {}",
                kind(other_s)
            ));
        }
    };
    let o = base.origin + n * (touch - base.origin).dot(&n);
    plane(o, n, base.x_axis.into_inner())
}

/// Real roots of `a t² + b t + c`, ascending.
fn quadratic(a: Real, b: Real, c: Real, scale: Real) -> Vec<Real> {
    if a.abs() < 1e-14 * scale.max(1.0) {
        if b.abs() < 1e-14 {
            return Vec::new();
        }
        return vec![-c / b];
    }
    let disc = b * b - 4.0 * a * c;
    let eps = 1e-12 * (b * b).max((4.0 * a * c).abs()).max(1e-300);
    if disc < -eps {
        return Vec::new();
    }
    if disc.abs() <= eps {
        return vec![-b / (2.0 * a)];
    }
    let sq = disc.sqrt();
    // Numerically stable pair.
    let q = -0.5 * (b + b.signum() * sq);
    let mut r = vec![q / a, c / q];
    r.sort_by(Real::total_cmp);
    r
}

/// Parameters `t` (ascending, unit `d`) where the line `p + t·d` meets the
/// surface.
pub(super) fn line_hits(s: &Surface, p: Point3, d: Vector3) -> Result<Vec<Real>, String> {
    let d = d.try_normalize(1e-12).ok_or("the line has no direction")?;
    let perp = |o: &Point3, a: &Vector3| {
        let u = p - o;
        (u - a * u.dot(a), d - a * d.dot(a), u.dot(a), d.dot(a))
    };
    Ok(match s {
        Surface::Plane(pl) => {
            let n = pl.normal.into_inner();
            let den = d.dot(&n);
            if den.abs() < 1e-12 {
                Vec::new()
            } else {
                vec![(pl.origin - p).dot(&n) / den]
            }
        }
        Surface::Cylinder(c) => {
            let (u, v, _, _) = perp(&c.origin, &c.axis.into_inner());
            quadratic(
                v.dot(&v),
                2.0 * u.dot(&v),
                u.dot(&u) - c.radius * c.radius,
                c.radius,
            )
        }
        Surface::Sphere(sp) => {
            let u = p - sp.center;
            quadratic(
                1.0,
                2.0 * u.dot(&d),
                u.dot(&u) - sp.radius * sp.radius,
                sp.radius,
            )
        }
        Surface::Cone(c) => {
            let (u, v, h0, h1) = perp(&c.origin, &c.axis.into_inner());
            let k = c.half_angle.tan();
            let r0 = c.radius + h0 * k;
            let rs = quadratic(
                v.dot(&v) - h1 * h1 * k * k,
                2.0 * (u.dot(&v) - r0 * h1 * k),
                u.dot(&u) - r0 * r0,
                c.radius,
            );
            // Only the nappe where the radius is not negative.
            rs.into_iter()
                .filter(|t| r0 + t * h1 * k >= -tol(c.radius))
                .collect()
        }
        Surface::Torus(t) => {
            // Numeric: sign changes of the distance along the part of the
            // line within reach of the torus, refined by bisection.
            let reach = t.major + t.minor;
            let tc = (t.center - p).dot(&d);
            let (lo, hi) = (tc - reach * 1.01, tc + reach * 1.01);
            let f = |x: Real| signed(s, &(p + d * x)).unwrap_or(1.0);
            let n = 4000;
            let mut out = Vec::new();
            let mut prev = (lo, f(lo));
            for i in 1..=n {
                let x = lo + (hi - lo) * i as Real / n as Real;
                let fx = f(x);
                if fx == 0.0 {
                    out.push(x);
                } else if prev.1 * fx < 0.0 {
                    let (mut a, mut b, mut fa) = (prev.0, x, prev.1);
                    for _ in 0..80 {
                        let m = 0.5 * (a + b);
                        let fm = f(m);
                        if fa * fm <= 0.0 {
                            b = m;
                        } else {
                            a = m;
                            fa = fm;
                        }
                    }
                    out.push(0.5 * (a + b));
                }
                prev = (x, fx);
            }
            out
        }
        Surface::Extrusion(_) => {
            return Err("intersections with free-form surfaces are not supported yet".into());
        }
    })
}

/// A curve input, resolved: an exact circle or a polyline.
pub(super) enum CurveGeom {
    /// Centre, unit normal, radius.
    Circle(Point3, Vector3, Real),
    /// Points along the curve.
    Poly(Vec<Point3>),
}

impl CurveGeom {
    /// The curve of a model edge (straight edges are refused: they have
    /// their own methods).
    pub(super) fn of_edge(body: &BRep, name: &str) -> Result<CurveGeom, String> {
        let id = body
            .find_edge(name)
            .ok_or_else(|| format!("edge '{name}' no longer exists"))?;
        let e = body
            .edge(id)
            .ok_or_else(|| format!("edge '{name}' no longer exists"))?;
        match body.curve(e.curve) {
            Some(Curve::Circle(k)) => {
                Ok(CurveGeom::Circle(k.center, k.normal.into_inner(), k.radius))
            }
            Some(Curve::Line(_)) => Err(format!("edge '{name}' is straight")),
            Some(Curve::BSpline(b)) if b.degree() == 1 => {
                Ok(CurveGeom::Poly(b.control_points().to_vec()))
            }
            _ => {
                let opts = crate::kernel::TessellationOptions {
                    chord_tolerance: 0.001,
                    max_segments: 1024,
                };
                body.sample_edge(id, &opts)
                    .map(CurveGeom::Poly)
                    .map_err(|e| e.to_string())
            }
        }
    }

    /// The unit tangent at the curve point nearest to `q`, and that point.
    /// Errors when `q` is not on the curve.
    pub(super) fn tangent_at(&self, q: &Point3) -> Result<(Point3, Vector3), String> {
        match self {
            CurveGeom::Circle(c, n, r) => {
                let w = q - c;
                let w = w - n * w.dot(n);
                let at = c + w.try_normalize(1e-12).ok_or("the point is the centre")? * *r;
                if (at - q).norm() > 1e-4 * (1.0 + r) {
                    return Err("the point is not on the curve".into());
                }
                Ok((at, n.cross(&(at - c)).normalize()))
            }
            CurveGeom::Poly(pts) => {
                if pts.len() < 2 {
                    return Err("the curve has no length".into());
                }
                // Tangent at each vertex: central differences.
                let n = pts.len();
                let closed = (pts[0] - pts[n - 1]).norm() < 1e-9;
                let vt = |i: usize| -> Vector3 {
                    let (a, b) = match (i, closed) {
                        (0, false) => (pts[0], pts[1]),
                        (i, false) if i == n - 1 => (pts[n - 2], pts[n - 1]),
                        (0, true) => (pts[n - 2], pts[1]),
                        (i, true) if i == n - 1 => (pts[n - 2], pts[1]),
                        (i, _) => (pts[i - 1], pts[i + 1]),
                    };
                    (b - a).try_normalize(1e-15).unwrap_or_else(Vector3::x)
                };
                let mut best: Option<(Real, usize, Real)> = None;
                for i in 0..n - 1 {
                    let (a, b) = (pts[i], pts[i + 1]);
                    let ab = b - a;
                    let s = ((q - a).dot(&ab) / ab.norm_squared().max(1e-300)).clamp(0.0, 1.0);
                    let dd = (a + ab * s - q).norm();
                    if best.is_none_or(|(bd, _, _)| dd < bd) {
                        best = Some((dd, i, s));
                    }
                }
                let (dd, i, s) = best.ok_or("the curve has no length")?;
                let len: Real = pts.windows(2).map(|w| (w[1] - w[0]).norm()).sum();
                if dd > 1e-3 * (1.0 + len) {
                    return Err("the point is not on the curve".into());
                }
                let at = pts[i] + (pts[i + 1] - pts[i]) * s;
                let t = vt(i) * (1.0 - s) + vt(i + 1) * s;
                Ok((at, t.try_normalize(1e-15).ok_or("no tangent there")?))
            }
        }
    }

    /// **Through Center of Circular or Elliptical Edge**: the centre and
    /// the normal of the curve's plane.
    pub(super) fn center_axis(&self) -> Result<(Point3, Vector3), String> {
        match self {
            CurveGeom::Circle(c, n, _) => Ok((*c, *n)),
            CurveGeom::Poly(pts) => conic_center(pts),
        }
    }
}

/// Centre and plane normal of the circle or ellipse through `pts` (a
/// planar arc or closed curve): a least-squares conic fit in the plane.
pub(super) fn conic_center(pts: &[Point3]) -> Result<(Point3, Vector3), String> {
    let mut pts: Vec<Point3> = pts.to_vec();
    if pts.len() > 1 && (pts[0] - pts[pts.len() - 1]).norm() < 1e-9 {
        pts.pop();
    }
    if pts.len() < 5 {
        return Err("the edge is not circular or elliptical".into());
    }
    let n_pts = pts.len() as Real;
    let c0 = Point3::from(pts.iter().map(|p| p.coords).sum::<Vector3>() / n_pts);
    let mut nrm = Vector3::zeros();
    for w in pts.windows(2) {
        nrm += (w[0] - c0).cross(&(w[1] - c0));
    }
    let size = pts.iter().map(|p| (p - c0).norm()).fold(0.0, Real::max);
    let nrm = nrm
        .try_normalize(1e-12 * size * size + 1e-300)
        .ok_or("the edge is straight")?;
    if pts
        .iter()
        .any(|p| (p - c0).dot(&nrm).abs() > 1e-6 * (1.0 + size))
    {
        return Err("the edge is not planar".into());
    }
    let e1 = perpendicular(&nrm);
    let e2 = nrm.cross(&e1);
    let uv: Vec<(Real, Real)> = pts
        .iter()
        .map(|p| ((p - c0).dot(&e1) / size, (p - c0).dot(&e2) / size))
        .collect();
    let rows: Vec<[Real; 6]> = uv
        .iter()
        .map(|&(u, v)| [u * u, u * v, v * v, u, v, 1.0])
        .collect();
    let m = nalgebra::DMatrix::from_fn(rows.len(), 6, |i, j| rows[i][j]);
    let svd = (m.transpose() * &m).symmetric_eigen();
    let (k, _) = svd
        .eigenvalues
        .iter()
        .enumerate()
        .min_by(|a, b| a.1.total_cmp(b.1))
        .ok_or("no fit")?;
    let q = svd.eigenvectors.column(k);
    let (a, b, c, d, e) = (q[0], q[1], q[2], q[3], q[4]);
    if b * b - 4.0 * a * c >= -1e-12 {
        return Err("the edge is not circular or elliptical".into());
    }
    // Residual: algebraic over gradient ≈ geometric distance (scaled).
    let worst = uv
        .iter()
        .map(|&(u, v)| {
            let f = a * u * u + b * u * v + c * v * v + d * u + e * v + q[5];
            let g = ((2.0 * a * u + b * v + d).powi(2) + (b * u + 2.0 * c * v + e).powi(2)).sqrt();
            f.abs() / g.max(1e-300)
        })
        .fold(0.0, Real::max);
    if worst > 1e-4 {
        return Err("the edge is not circular or elliptical".into());
    }
    let det = 4.0 * a * c - b * b;
    let cu = (b * e - 2.0 * c * d) / det;
    let cv = (b * d - 2.0 * a * e) / det;
    Ok((c0 + (e1 * cu + e2 * cv) * size, nrm))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::{Cone, Cylinder, Sphere, Torus};
    use approx::assert_relative_eq;
    use nalgebra::Unit;

    fn cyl(o: [Real; 3], a: [Real; 3], r: Real) -> Surface {
        let a = Vector3::from(a);
        Surface::Cylinder(
            Cylinder::new(Point3::from(o), a, perpendicular(&a.normalize()), r).unwrap(),
        )
    }

    fn sphere(c: [Real; 3], r: Real) -> Surface {
        Surface::Sphere(Sphere {
            center: Point3::from(c),
            radius: r,
            axis: Unit::new_normalize(Vector3::z()),
            x_axis: Unit::new_normalize(Vector3::x()),
        })
    }

    fn torus(c: [Real; 3], a: [Real; 3], big: Real, small: Real) -> Surface {
        let a = Vector3::from(a).normalize();
        Surface::Torus(Torus {
            center: Point3::from(c),
            axis: Unit::new_normalize(a),
            x_axis: Unit::new_normalize(perpendicular(&a)),
            major: big,
            minor: small,
        })
    }

    fn on_plane(p: &Plane, q: &Point3) -> Real {
        (q - p.origin).dot(&p.normal)
    }

    /// The tangent plane through an axis-parallel line touches the
    /// cylinder (distance from the axis = r) and contains the line; both
    /// solutions; five cylinders (tilted, offset).
    #[test]
    fn tangent_through_line_on_cylinders() {
        let cases = [
            ([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 5.0, [10.0, 0.0, 3.0]),
            ([1.0, 2.0, 3.0], [0.0, 0.0, 1.0], 2.0, [1.0, 9.0, 0.0]),
            ([0.0, 0.0, 0.0], [1.0, 1.0, 0.0], 3.0, [0.0, 0.0, 7.0]),
            ([-4.0, 5.0, 1.0], [1.0, 2.0, 3.0], 1.5, [2.0, -3.0, 0.0]),
            ([0.0, 0.0, 0.0], [0.0, 1.0, 0.0], 4.0, [4.0, 1.0, 0.0]),
        ];
        for (o, a, r, off) in cases {
            let s = cyl(o, a, r);
            let ax = Vector3::from(a).normalize();
            // A line parallel to the axis, through o + (offset ⟂ axis).
            let off = Vector3::from(off);
            let off = off - ax * off.dot(&ax);
            let q = Point3::from(o) + off;
            for other in [false, true] {
                let p = tangent_through_line(&s, q, ax, other).unwrap();
                assert_relative_eq!(on_plane(&p, &q), 0.0, epsilon = 1e-9);
                assert_relative_eq!(on_plane(&p, &(q + ax * 7.0)), 0.0, epsilon = 1e-9);
                assert_relative_eq!(on_plane(&p, &Point3::from(o)).abs(), r, epsilon = 1e-9);
                assert_relative_eq!(p.x_axis.dot(&ax).abs(), 1.0, epsilon = 1e-9);
                // Y points from the tangency towards the edge.
                assert!(p.y_axis().dot(&(q - p.origin)) >= -1e-9);
            }
        }
        // Not parallel, or inside: errors.
        let s = cyl([0.0; 3], [0.0, 0.0, 1.0], 5.0);
        assert!(tangent_through_line(&s, Point3::new(9.0, 0.0, 0.0), Vector3::x(), false).is_err());
        assert!(tangent_through_line(&s, Point3::new(1.0, 0.0, 0.0), Vector3::z(), false).is_err());
    }

    /// Spheres: the plane contains the line and is at distance r from the
    /// centre.
    #[test]
    fn tangent_through_line_on_spheres() {
        let cases = [
            ([0.0, 0.0, 0.0], 5.0, [8.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            ([1.0, 2.0, 3.0], 2.0, [1.0, 9.0, 3.0], [1.0, 0.0, 0.0]),
            ([0.0, 0.0, 0.0], 3.0, [0.0, 0.0, 3.0], [1.0, 1.0, 0.0]),
            ([-4.0, 5.0, 1.0], 1.5, [-4.0, 5.0, 5.0], [0.0, 1.0, 0.0]),
            ([10.0, 0.0, 0.0], 4.0, [10.0, 6.0, 6.0], [1.0, 0.0, 0.0]),
        ];
        for (c, r, q, d) in cases {
            let s = sphere(c, r);
            let (q, d) = (Point3::from(q), Vector3::from(d).normalize());
            for other in [false, true] {
                let p = tangent_through_line(&s, q, d, other).unwrap();
                assert_relative_eq!(on_plane(&p, &q), 0.0, epsilon = 1e-9);
                assert_relative_eq!(on_plane(&p, &(q + d * 3.0)), 0.0, epsilon = 1e-9);
                assert_relative_eq!(on_plane(&p, &Point3::from(c)).abs(), r, epsilon = 1e-9);
                assert_relative_eq!((p.origin - Point3::from(c)).norm(), r, epsilon = 1e-9);
            }
        }
    }

    /// Through a point on the surface: the plane touches there (normal =
    /// surface normal); off a cylinder: through the axis-parallel line.
    #[test]
    fn tangent_through_point_variants() {
        let k = 20f64.to_radians();
        let cone = Surface::Cone(
            Cone::new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::z(),
                Vector3::x(),
                5.0,
                k,
            )
            .unwrap(),
        );
        let cases: Vec<(Surface, Point3, Vector3)> = vec![
            (
                cyl([0.0; 3], [0.0, 0.0, 1.0], 5.0),
                Point3::new(5.0, 0.0, 2.0),
                Vector3::x(),
            ),
            (
                cyl([1.0, 1.0, 0.0], [1.0, 0.0, 0.0], 2.0),
                Point3::new(4.0, 1.0, 2.0),
                Vector3::z(),
            ),
            (
                sphere([1.0, 2.0, 3.0], 4.0),
                Point3::new(1.0, 2.0, 7.0),
                Vector3::z(),
            ),
            (
                sphere([0.0; 3], 2.0),
                Point3::new(2.0f64.sqrt(), 2.0f64.sqrt(), 0.0),
                Vector3::new(1.0, 1.0, 0.0).normalize(),
            ),
            (
                torus([0.0; 3], [0.0, 0.0, 1.0], 10.0, 2.0),
                Point3::new(12.0, 0.0, 0.0),
                Vector3::x(),
            ),
            (
                cone.clone(),
                Point3::new(5.0 + 2.0 * k.tan(), 0.0, 2.0),
                Vector3::new(k.cos(), 0.0, -k.sin()),
            ),
        ];
        for (s, q, n) in cases {
            let p = tangent_through_point(&s, q, false).unwrap();
            assert_relative_eq!((p.origin - q).norm(), 0.0, epsilon = 1e-9);
            assert_relative_eq!(p.normal.dot(&n), 1.0, epsilon = 1e-9);
        }
        // Off a cylinder: two planes through the axis-parallel line.
        let s = cyl([0.0; 3], [0.0, 0.0, 1.0], 3.0);
        let q = Point3::new(0.0, 6.0, 1.0);
        let a = tangent_through_point(&s, q, false).unwrap();
        let b = tangent_through_point(&s, q, true).unwrap();
        for p in [&a, &b] {
            assert_relative_eq!(on_plane(p, &q), 0.0, epsilon = 1e-9);
            assert_relative_eq!(on_plane(p, &Point3::origin()).abs(), 3.0, epsilon = 1e-9);
        }
        assert!(a.normal.dot(&b.normal) < 0.99);
        // Off a sphere: no single plane.
        assert!(tangent_through_point(&sphere([0.0; 3], 1.0), q, false).is_err());
    }

    /// Parallel to a plane: five surfaces, both sides.
    #[test]
    fn tangent_parallel_variants() {
        let k = 30f64.to_radians();
        // The cone's normal on the +X side.
        let cone_n = Vector3::new(k.cos(), 0.0, -k.sin());
        let cone =
            Surface::Cone(Cone::new(Point3::origin(), Vector3::z(), Vector3::x(), 2.0, k).unwrap());
        let cases: Vec<(Surface, Vector3, Real)> = vec![
            // (surface, base normal, distance of the touching plane from
            // the origin along the normal)
            (cyl([0.0; 3], [0.0, 0.0, 1.0], 5.0), Vector3::x(), 5.0),
            (
                cyl([1.0, 2.0, 0.0], [0.0, 0.0, 1.0], 3.0),
                Vector3::y(),
                5.0,
            ),
            (sphere([1.0, 2.0, 3.0], 4.0), Vector3::z(), 7.0),
            (
                torus([0.0, 0.0, 1.0], [0.0, 0.0, 1.0], 10.0, 2.0),
                Vector3::z(),
                3.0,
            ),
            (
                torus([0.0; 3], [0.0, 0.0, 1.0], 10.0, 2.0),
                Vector3::x(),
                12.0,
            ),
            (cone, cone_n, 2.0 * k.cos()),
        ];
        for (s, n, dist) in cases {
            let base = Plane::new(Point3::new(0.0, 0.0, 0.0), n, perpendicular(&n)).unwrap();
            let p = tangent_parallel(&s, &base, false).unwrap();
            assert_relative_eq!(p.normal.dot(&n), 1.0, epsilon = 1e-9);
            assert_relative_eq!(p.origin.coords.dot(&n), dist, epsilon = 1e-9);
            assert_relative_eq!(p.x_axis.dot(&base.x_axis), 1.0, epsilon = 1e-9);
        }
        let s = cyl([0.0; 3], [0.0, 0.0, 1.0], 5.0);
        let base = Plane::new(Point3::origin(), Vector3::x(), Vector3::y()).unwrap();
        let far = tangent_parallel(&s, &base, true).unwrap();
        assert_relative_eq!(far.origin.x, -5.0, epsilon = 1e-9);
        let tilted =
            Plane::new(Point3::origin(), Vector3::new(1.0, 0.0, 1.0), Vector3::y()).unwrap();
        assert!(tangent_parallel(&s, &tilted, false).is_err());
    }

    /// Lines through cylinders, spheres, cones and tori: every hit is on
    /// the surface, counts as expected.
    #[test]
    fn line_surface_hits() {
        let k = 20f64.to_radians();
        let cone =
            Surface::Cone(Cone::new(Point3::origin(), Vector3::z(), Vector3::x(), 5.0, k).unwrap());
        let cases: Vec<(Surface, Point3, Vector3, usize)> = vec![
            (
                cyl([0.0; 3], [0.0, 0.0, 1.0], 5.0),
                Point3::new(-20.0, 0.0, 3.0),
                Vector3::x(),
                2,
            ),
            (
                cyl([0.0; 3], [1.0, 1.0, 1.0], 2.0),
                Point3::new(-10.0, 0.5, 0.0),
                Vector3::new(1.0, 0.05, 0.0),
                2,
            ),
            (
                sphere([1.0, 2.0, 3.0], 4.0),
                Point3::new(1.0, 2.0, -10.0),
                Vector3::z(),
                2,
            ),
            (
                sphere([0.0; 3], 4.0),
                Point3::new(4.0, -10.0, 0.0),
                Vector3::y(),
                1,
            ),
            (
                sphere([0.0; 3], 4.0),
                Point3::new(5.0, -10.0, 0.0),
                Vector3::y(),
                0,
            ),
            (cone, Point3::new(-30.0, 0.0, 2.0), Vector3::x(), 2),
            (
                torus([0.0; 3], [0.0, 0.0, 1.0], 10.0, 2.0),
                Point3::new(-30.0, 0.0, 0.0),
                Vector3::x(),
                4,
            ),
            (
                torus([1.0, 1.0, 1.0], [0.0, 1.0, 0.0], 8.0, 3.0),
                Point3::new(9.0, -20.0, 1.0),
                Vector3::y(),
                2,
            ),
        ];
        for (s, p, d, count) in cases {
            let hits = line_hits(&s, p, d).unwrap();
            assert_eq!(hits.len(), count, "{s:?}");
            for t in hits {
                let q = p + d.normalize() * t;
                assert!(signed(&s, &q).unwrap().abs() < 1e-7, "{q}");
            }
        }
    }

    /// Ellipses and circles from points: tilted, offset, arcs.
    #[test]
    fn conic_centres() {
        let cases = [
            (
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 1.0],
                10.0,
                5.0,
                0.0,
                std::f64::consts::TAU,
            ),
            (
                [3.0, -2.0, 7.0],
                [1.0, 1.0, 1.0],
                4.0,
                4.0,
                0.0,
                std::f64::consts::TAU,
            ),
            ([3.0, -2.0, 7.0], [0.0, 1.0, 0.0], 9.0, 2.0, 0.5, 2.0),
            ([-10.0, 0.0, 1.0], [1.0, 0.0, 2.0], 6.0, 3.0, -1.0, 1.5),
            (
                [0.0, 50.0, 0.0],
                [0.0, 0.0, -1.0],
                20.0,
                19.0,
                0.0,
                std::f64::consts::PI,
            ),
        ];
        for (c, n, a, b, t0, t1) in cases {
            let n = Vector3::from(n).normalize();
            let e1 = perpendicular(&n);
            let e2 = n.cross(&e1);
            let pts: Vec<Point3> = (0..=40)
                .map(|i| {
                    let t = t0 + (t1 - t0) * i as Real / 40.0;
                    Point3::from(c) + e1 * (a * t.cos()) + e2 * (b * t.sin())
                })
                .collect();
            let (got, axis) = conic_center(&pts).unwrap();
            assert_relative_eq!((got - Point3::from(c)).norm(), 0.0, epsilon = 1e-6);
            assert_relative_eq!(axis.dot(&n).abs(), 1.0, epsilon = 1e-9);
        }
        // A zig-zag is no conic.
        let bad: Vec<Point3> = (0..10)
            .map(|i| Point3::new(i as Real, (i % 2) as Real, 0.0))
            .collect();
        assert!(conic_center(&bad).is_err());
    }

    /// Axes and centres of revolved surfaces.
    #[test]
    fn axes_and_centres() {
        let (p, d) = axis_of(&cyl([1.0, 2.0, 3.0], [0.0, 1.0, 0.0], 2.0)).unwrap();
        assert_relative_eq!(p, Point3::new(1.0, 2.0, 3.0), epsilon = 1e-12);
        assert_relative_eq!(d, Vector3::y(), epsilon = 1e-12);
        let t = torus([4.0, 0.0, 1.0], [1.0, 0.0, 0.0], 5.0, 1.0);
        assert_relative_eq!(torus_center(&t).unwrap(), Point3::new(4.0, 0.0, 1.0));
        let m = torus_midplane(&t).unwrap();
        assert_relative_eq!(m.normal.x, 1.0, epsilon = 1e-12);
        assert!(axis_of(&sphere([0.0; 3], 1.0)).is_err());
        assert_relative_eq!(
            sphere_center(&sphere([1.0, 2.0, 3.0], 1.0)).unwrap(),
            Point3::new(1.0, 2.0, 3.0)
        );
        assert!(sphere_center(&t).is_err());
    }

    /// Tangent of a curve at a point: a circle (exact) and a sampled
    /// helix-like polyline.
    #[test]
    fn curve_tangents() {
        let c = CurveGeom::Circle(Point3::new(1.0, 1.0, 0.0), Vector3::z(), 2.0);
        let (at, t) = c.tangent_at(&Point3::new(3.0, 1.0, 0.0)).unwrap();
        assert_relative_eq!(at, Point3::new(3.0, 1.0, 0.0), epsilon = 1e-12);
        assert_relative_eq!(t, Vector3::y(), epsilon = 1e-12);
        assert!(c.tangent_at(&Point3::new(9.0, 1.0, 0.0)).is_err());
        // A parabola sampled densely: tangent at x = 1 is (1, 2)/√5.
        let pts: Vec<Point3> = (0..=200)
            .map(|i| {
                let x = -2.0 + 4.0 * i as Real / 200.0;
                Point3::new(x, x * x, 0.0)
            })
            .collect();
        let (_, t) = CurveGeom::Poly(pts)
            .tangent_at(&Point3::new(1.0, 1.0, 0.0))
            .unwrap();
        assert_relative_eq!(
            t.dot(&Vector3::new(1.0, 2.0, 0.0).normalize()),
            1.0,
            epsilon = 1e-6
        );
    }
}
