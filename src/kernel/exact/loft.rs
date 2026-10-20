//! Guided lofts (`docs/spec/5.2-loft.md`): Rails, Center Line, Area Loft,
//! the Direction / Tangent end conditions, manual point mapping and Closed
//! Loop.
//!
//! The exact kernel lofts through planar sections only: one smooth B-spline
//! face per section segment through every section (`sk::loft_matched`, our
//! addition to the vendored kernel, which keeps the caller's segment pairing).
//! Every option is therefore turned into **stations** — planar sections the
//! loft passes through, placed between the user's sections:
//!
//! 1. every section becomes a closed loop of degree-7 Bézier pieces (exact for
//!    lines, cubics and spline pieces; arcs within ~2e-7 of the radius) in an
//!    in-plane frame that is carried from section to section by the smallest
//!    rotation between their normals (a centre line carries it with
//!    rotation-minimising frames, as the sweep does);
//! 2. the sections are **matched**: rail contacts and map points become
//!    vertices that pair up, the pieces between them are split until every
//!    section has the same count; without them the vertices are paired by
//!    direction from the centroid (as the kernel's own loft does);
//! 3. **stations** are interpolated between the sections (cubic Hermite of
//!    the section origins and of the matched control points; end conditions
//!    set the end derivatives) and then **fitted to the guides**: a station
//!    is moved, turned and scaled so that its rail-contact vertices land on
//!    the rails (one rail: translation; two: similarity; three: affine; more:
//!    affine plus a smooth correction), set square to the centre line, or
//!    scaled to the area law of an Area Loft.

use std::sync::OnceLock;

use nalgebra::{Matrix2, Matrix3, Rotation3, SMatrix, Vector2};

use super::sweep::{side_names, split_names};
use super::{ExactProfile, ExactSolid, Plane, Point3, Real, Vector3, d3, guarded, names_from};
use super::{sg, sk, v3};

type V2 = Vector2<Real>;

/// Control points of a degree-7 Bézier piece.
const N: usize = 8;

/// The most sections the kernel lofts through (its limit is 50).
const MAX_STATIONS: usize = 48;

// ------------------------------------------------------------ Béziers ---

#[derive(Clone, Copy, Debug, PartialEq)]
struct Bez([V2; N]);

const BINOM: [Real; N] = [1.0, 7.0, 21.0, 35.0, 35.0, 21.0, 7.0, 1.0];

fn bernstein(t: Real) -> [Real; N] {
    let mut b = [0.0; N];
    let s = 1.0 - t;
    for (i, x) in b.iter_mut().enumerate() {
        *x = BINOM[i] * t.powi(i as i32) * s.powi((N - 1 - i) as i32);
    }
    b
}

/// Inverse of the Bernstein matrix at the Chebyshev–Lobatto nodes.
fn interp_matrix() -> Option<&'static SMatrix<Real, N, N>> {
    static M: OnceLock<Option<SMatrix<Real, N, N>>> = OnceLock::new();
    M.get_or_init(|| {
        let mut a = SMatrix::<Real, N, N>::zeros();
        for i in 0..N {
            let b = bernstein(node(i));
            for j in 0..N {
                a[(i, j)] = b[j];
            }
        }
        a.try_inverse()
    })
    .as_ref()
}

fn node(i: usize) -> Real {
    (1.0 - (std::f64::consts::PI * i as Real / (N - 1) as Real).cos()) / 2.0
}

impl Bez {
    /// The piece through `f` at the interpolation nodes (exact for
    /// polynomials of degree ≤ 7).
    fn from_fn(f: impl Fn(Real) -> V2) -> Option<Bez> {
        let m = interp_matrix()?;
        let vals: Vec<V2> = (0..N).map(|i| f(node(i))).collect();
        let mut p = [V2::zeros(); N];
        for (j, pj) in p.iter_mut().enumerate() {
            for (i, v) in vals.iter().enumerate() {
                *pj += v * m[(j, i)];
            }
        }
        // Ends exactly (shared vertices must stay shared).
        p[0] = f(0.0);
        p[N - 1] = f(1.0);
        Some(Bez(p))
    }

    fn eval(&self, t: Real) -> V2 {
        let b = bernstein(t);
        self.0.iter().zip(b).map(|(p, w)| p * w).sum()
    }

    fn deriv(&self, t: Real) -> V2 {
        // Degree-6 Bernstein of the differences, times 7.
        let s = 1.0 - t;
        const B6: [Real; 7] = [1.0, 6.0, 15.0, 20.0, 15.0, 6.0, 1.0];
        let mut d = V2::zeros();
        for (i, (b, w)) in B6.iter().zip(self.0.windows(2)).enumerate() {
            let f = b * t.powi(i as i32) * s.powi((N - 2 - i) as i32);
            d += (w[1] - w[0]) * f;
        }
        d * (N - 1) as Real
    }

    fn split(&self, t: Real) -> (Bez, Bez) {
        let mut pts = self.0;
        let mut left = [V2::zeros(); N];
        let mut right = [V2::zeros(); N];
        for r in 0..N {
            left[r] = pts[0];
            right[N - 1 - r] = pts[N - 1 - r];
            for i in 0..N - 1 - r {
                pts[i] = pts[i] * (1.0 - t) + pts[i + 1] * t;
            }
        }
        (Bez(left), Bez(right))
    }

    fn reversed(&self) -> Bez {
        let mut p = self.0;
        p.reverse();
        Bez(p)
    }

    fn map(&self, f: impl Fn(V2) -> V2) -> Bez {
        Bez(self.0.map(f))
    }

    fn start(&self) -> V2 {
        self.0[0]
    }

    fn length(&self) -> Real {
        let mut l = 0.0;
        let mut prev = self.start();
        for i in 1..=16 {
            let q = self.eval(i as Real / 16.0);
            l += (q - prev).norm();
            prev = q;
        }
        l
    }

    fn seg(&self) -> sg::Seg2 {
        let mut p = [sg::Vec2::new(0.0, 0.0); 8];
        for (q, b) in p.iter_mut().zip(self.0) {
            *q = sg::Vec2::new(b.x, b.y);
        }
        sg::Seg2::Bezier { n: 7, p }
    }
}

fn arc_bez(c: V2, r: Real, a0: Real, sweep: Real) -> Option<Bez> {
    Bez::from_fn(|t| {
        let a = a0 + sweep * t;
        c + V2::new(a.cos(), a.sin()) * r
    })
}

/// Area and centroid of a closed loop of pieces.
fn area_centroid(segs: &[Bez]) -> (Real, V2) {
    let mut a = 0.0;
    let mut c = V2::zeros();
    for s in segs {
        let mut p = s.start();
        for i in 1..=24 {
            let q = s.eval(i as Real / 24.0);
            let cr = p.perp(&q);
            a += cr;
            c += (p + q) * cr;
            p = q;
        }
    }
    if a.abs() < 1e-300 {
        let n = segs.len().max(1) as Real;
        return (0.0, segs.iter().map(Bez::start).sum::<V2>() / n);
    }
    (a / 2.0, c / (3.0 * a))
}

/// The piece and parameter of the loop point nearest `q`, and the distance.
fn nearest(segs: &[Bez], q: V2) -> (usize, Real, Real) {
    let mut best = (0, 0.0, Real::MAX);
    for (i, s) in segs.iter().enumerate() {
        let mut t0 = 0.0;
        let mut d0 = Real::MAX;
        for j in 0..=64 {
            let t = j as Real / 64.0;
            let d = (s.eval(t) - q).norm();
            if d < d0 {
                (t0, d0) = (t, d);
            }
        }
        // Golden-section refinement around the best sample.
        let (mut lo, mut hi) = ((t0 - 1.0 / 64.0).max(0.0), (t0 + 1.0 / 64.0).min(1.0));
        for _ in 0..40 {
            let m1 = lo + (hi - lo) * 0.382;
            let m2 = lo + (hi - lo) * 0.618;
            if (s.eval(m1) - q).norm() < (s.eval(m2) - q).norm() {
                hi = m2;
            } else {
                lo = m1;
            }
        }
        let t = (lo + hi) / 2.0;
        let d = (s.eval(t) - q).norm();
        let (t, d) = if d < d0 { (t, d) } else { (t0, d0) };
        if d < best.2 {
            best = (i, t, d);
        }
    }
    best
}

/// The unit outward normal (counter-clockwise loop) at every control point,
/// flattened in piece order; at the vertices the mitre of both pieces.
fn control_normals(segs: &[Bez]) -> Vec<V2> {
    let n = segs.len();
    let unit = |d: V2| {
        let v = V2::new(d.y, -d.x);
        let l = v.norm();
        if l > 1e-300 { v / l } else { V2::zeros() }
    };
    let vertex = |i: usize| {
        let a = unit(segs[(i + n - 1) % n].deriv(1.0));
        let b = unit(segs[i].deriv(0.0));
        let m = a + b;
        let c = 1.0 + a.dot(&b);
        if c < 1e-3 {
            b
        } else {
            (m / c).cap_magnitude(3.0)
        }
    };
    let mut out = Vec::with_capacity(n * N);
    for (i, s) in segs.iter().enumerate() {
        for j in 0..N {
            out.push(if j == 0 {
                vertex(i)
            } else if j == N - 1 {
                vertex((i + 1) % n)
            } else {
                unit(s.deriv(j as Real / (N - 1) as Real))
            });
        }
    }
    out
}

// ----------------------------------------------------------- sections ---

/// A section (or station): a plane frame and a closed loop in it.
#[derive(Clone, Debug)]
struct Sec {
    o: Point3,
    gx: Vector3,
    gy: Vector3,
    segs: Vec<Bez>,
    names: Vec<String>,
}

impl Sec {
    fn n(&self) -> Vector3 {
        self.gx.cross(&self.gy)
    }

    fn world(&self, p: V2) -> Point3 {
        self.o + self.gx * p.x + self.gy * p.y
    }

    fn local(&self, q: &Point3) -> V2 {
        let d = q - self.o;
        V2::new(d.dot(&self.gx), d.dot(&self.gy))
    }

    /// The loop in another frame of the same plane.
    fn coords_in(&self, o: Point3, gx: Vector3, gy: Vector3) -> Vec<Bez> {
        self.segs
            .iter()
            .map(|s| {
                s.map(|p| {
                    let d = self.world(p) - o;
                    V2::new(d.dot(&gx), d.dot(&gy))
                })
            })
            .collect()
    }

    /// Re-expressed in the frame `(o, gx, gy)` of its own plane.
    fn reframe(&mut self, o: Point3, gx: Vector3, gy: Vector3) {
        self.segs = self.coords_in(o, gx, gy);
        (self.o, self.gx, self.gy) = (o, gx, gy);
    }

    fn reverse(&mut self) {
        self.segs.reverse();
        for s in self.segs.iter_mut() {
            *s = s.reversed();
        }
        self.names.reverse();
    }

    fn rotate_left(&mut self, k: usize) {
        if k < self.segs.len() {
            self.segs.rotate_left(k);
            self.names.rotate_left(k);
        }
    }

    fn split(&mut self, i: usize, t: Real) {
        let (a, b) = self.segs[i].split(t);
        self.segs[i] = a;
        self.segs.insert(i + 1, b);
        let name = self.names.get(i).cloned().unwrap_or_default();
        self.names.insert(i + 1, name);
    }

    fn vertex(&self, i: usize) -> V2 {
        self.segs[i % self.segs.len()].start()
    }

    fn kernel(&self) -> Result<(sg::Plane, sg::Loop2), String> {
        let pl = sg::Plane::new(v3(&self.o), d3(&self.gx), d3(&self.gy))
            .ok_or("a loft station has a degenerate plane")?;
        Ok((
            pl,
            sg::Loop2 {
                segs: self.segs.iter().map(Bez::seg).collect(),
            },
        ))
    }
}

/// The smallest rotation taking `a` to `b` (unit vectors).
fn turn(a: &Vector3, b: &Vector3) -> Rotation3<Real> {
    Rotation3::rotation_between(a, b).unwrap_or_else(|| {
        // Opposite: half a turn about any axis square to `a`.
        let axis = a
            .cross(&Vector3::x())
            .try_normalize(1e-9)
            .unwrap_or_else(|| {
                a.cross(&Vector3::y())
                    .try_normalize(1e-9)
                    .unwrap_or_else(Vector3::z)
            });
        Rotation3::from_axis_angle(&nalgebra::Unit::new_unchecked(axis), std::f64::consts::PI)
    })
}

/// A section from the user's profile: its outer loop as Bézier pieces (arcs
/// cut to quarter turns), and whether it is one whole circle (centre and
/// radius, sketch coordinates).
fn raw_section(
    pl: &Plane,
    prof: &ExactProfile,
    k: usize,
) -> Result<(Sec, Option<(V2, Real)>), String> {
    if !prof.region.holes.is_empty() {
        return Err(format!(
            "section {} has holes, which a loft with rails, a centre line, conditions, \
             point mapping or Closed Loop cannot follow yet",
            k + 1
        ));
    }
    let (names, _) = split_names(prof);
    let v = |p: sg::Vec2| V2::new(p.x, p.y);
    let mut segs = Vec::new();
    let mut out_names = Vec::new();
    let mut arcs: Vec<(V2, Real, Real)> = Vec::new();
    let mut all_arcs = true;
    for (i, s) in prof.region.outer.segs.iter().enumerate() {
        let name = names.get(i).cloned().unwrap_or_default();
        match *s {
            sg::Seg2::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                let c = v(center);
                arcs.push((c, radius, sweep));
                let m = ((sweep.abs() / std::f64::consts::FRAC_PI_2 - 1e-9).ceil() as usize).max(1);
                for j in 0..m {
                    let a0 = start + sweep * j as Real / m as Real;
                    segs.push(arc_bez(c, radius, a0, sweep / m as Real).ok_or("arc")?);
                    out_names.push(name.clone());
                }
            }
            ref other => {
                all_arcs = false;
                let s = *other;
                segs.push(Bez::from_fn(|t| v(s.point_at(t))).ok_or("profile piece")?);
                out_names.push(name);
            }
        }
    }
    if segs.is_empty() {
        return Err(format!("section {} is empty", k + 1));
    }
    let circle = (all_arcs && !arcs.is_empty())
        .then(|| {
            let (c, r, _) = arcs[0];
            let same = arcs
                .iter()
                .all(|(c2, r2, _)| (c2 - c).norm() < 1e-9 && (r2 - r).abs() < 1e-9);
            let total: Real = arcs.iter().map(|a| a.2).sum();
            (same && (total.abs() - std::f64::consts::TAU).abs() < 1e-6).then_some((c, r))
        })
        .flatten();
    Ok((
        Sec {
            o: pl.origin,
            gx: pl.x_axis.into_inner(),
            gy: pl.y_axis().into_inner(),
            segs,
            names: out_names,
        },
        circle,
    ))
}

// -------------------------------------------------------------- paths ---

/// A polyline with arc length.
#[derive(Clone, Debug)]
struct Track {
    pts: Vec<Point3>,
    cum: Vec<Real>,
}

impl Track {
    fn new(pts: &[Point3]) -> Result<Self, String> {
        let mut v: Vec<Point3> = Vec::with_capacity(pts.len());
        for p in pts {
            if v.last().is_none_or(|l: &Point3| (p - l).norm() > 1e-9) {
                v.push(*p);
            }
        }
        if v.len() < 2 {
            return Err("a guide curve has no length".into());
        }
        let mut cum = vec![0.0];
        for w in v.windows(2) {
            let l = cum.last().copied().unwrap_or(0.0);
            cum.push(l + (w[1] - w[0]).norm());
        }
        Ok(Self { pts: v, cum })
    }

    fn length(&self) -> Real {
        self.cum.last().copied().unwrap_or(0.0)
    }

    fn reversed(&self) -> Self {
        let mut pts = self.pts.clone();
        pts.reverse();
        let l = self.length();
        let cum = self.cum.iter().rev().map(|c| l - c).collect();
        Self { pts, cum }
    }

    /// The piece index holding arc length `s`.
    fn piece(&self, s: Real) -> usize {
        let n = self.pts.len() - 1;
        match self
            .cum
            .binary_search_by(|c| c.total_cmp(&s.clamp(0.0, self.length())))
        {
            Ok(i) => i.min(n - 1),
            Err(i) => i.saturating_sub(1).min(n - 1),
        }
    }

    fn at(&self, s: Real) -> Point3 {
        let i = self.piece(s);
        let l = self.cum[i + 1] - self.cum[i];
        let t = if l > 0.0 {
            ((s - self.cum[i]) / l).clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.pts[i] + (self.pts[i + 1] - self.pts[i]) * t
    }

    fn tangent_of(&self, i: usize) -> Vector3 {
        (self.pts[i + 1] - self.pts[i])
            .try_normalize(1e-300)
            .unwrap_or_else(Vector3::z)
    }

    /// The (smoothed) unit tangent at arc length `s`.
    fn tangent(&self, s: Real) -> Vector3 {
        let i = self.piece(s);
        let n = self.pts.len() - 1;
        let l = self.cum[i + 1] - self.cum[i];
        let t = if l > 0.0 {
            ((s - self.cum[i]) / l).clamp(0.0, 1.0)
        } else {
            0.5
        };
        // Blend towards the neighbour's direction near the piece's ends.
        let here = self.tangent_of(i);
        let other = if t < 0.5 && i > 0 {
            Some((self.tangent_of(i - 1), 0.5 - t))
        } else if t >= 0.5 && i + 1 < n {
            Some((self.tangent_of(i + 1), t - 0.5))
        } else {
            None
        };
        match other {
            Some((o, w)) => (here * (1.0 - w) + o * w)
                .try_normalize(1e-300)
                .unwrap_or(here),
            None => here,
        }
    }

    /// Where it crosses the plane through `o` square to `n` (arc lengths
    /// and points; an end lying on the plane counts).
    fn crossings(&self, o: &Point3, n: &Vector3, tol: Real) -> Vec<(Real, Point3)> {
        let h: Vec<Real> = self.pts.iter().map(|p| (p - o).dot(n)).collect();
        let mut out: Vec<(Real, Point3)> = Vec::new();
        let mut push = |s: Real, p: Point3| {
            if out.last().is_none_or(|l| (l.0 - s).abs() > 1e-9) {
                out.push((s, p));
            }
        };
        for i in 0..self.pts.len() - 1 {
            let (a, b) = (h[i], h[i + 1]);
            if a.abs() <= tol {
                push(self.cum[i], self.pts[i]);
            }
            if (a < -tol && b > tol) || (a > tol && b < -tol) {
                let t = a / (a - b);
                let s = self.cum[i] + t * (self.cum[i + 1] - self.cum[i]);
                push(s, self.pts[i] + (self.pts[i + 1] - self.pts[i]) * t);
            }
        }
        if let (Some(&last), Some(&p)) = (h.last(), self.pts.last())
            && last.abs() <= tol
        {
            push(self.length(), p);
        }
        out
    }

    fn nearest(&self, q: &Point3) -> (Real, Point3) {
        let mut best = (0.0, self.pts[0], Real::MAX);
        for i in 0..self.pts.len() - 1 {
            let (a, b) = (self.pts[i], self.pts[i + 1]);
            let d = b - a;
            let l2 = d.norm_squared();
            let t = if l2 > 0.0 {
                ((q - a).dot(&d) / l2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let p = a + d * t;
            let dist = (q - p).norm();
            if dist < best.2 {
                best = (self.cum[i] + t * (self.cum[i + 1] - self.cum[i]), p, dist);
            }
        }
        (best.0, best.1)
    }

    /// Rotation-minimising in-plane x axes, one per piece, starting from
    /// `x0` (square to the first piece), by double reflection (Wang, Jüttler,
    /// Zheng, Liu 2008).
    fn frames(&self, x0: Vector3) -> Vec<Vector3> {
        let n = self.pts.len() - 1;
        let mut out = Vec::with_capacity(n);
        let mut x = x0;
        for i in 0..n {
            if i > 0 {
                let (t0, t1) = (self.tangent_of(i - 1), self.tangent_of(i));
                let v1 = self.pts[i] - self.pts[i - 1];
                let reflect = |v: Vector3, w: Vector3| {
                    let c = w.dot(&w);
                    if c > 1e-300 {
                        v - w * (2.0 / c * w.dot(&v))
                    } else {
                        v
                    }
                };
                let xl = reflect(x, v1);
                let tl = reflect(t0, v1);
                let v2 = t1 - tl;
                x = reflect(xl, v2);
                let t = t1;
                x = (x - t * x.dot(&t)).try_normalize(1e-300).unwrap_or(x);
            }
            out.push(x);
        }
        out
    }
}

// --------------------------------------------------------------- input ---

/// A boundary condition at an end section.
#[derive(Clone, Debug, PartialEq)]
pub enum LoftEnd {
    /// None.
    Free,
    /// The loft leaves the section plane at `angle` (radians; π/2 square to
    /// it, below that leaning inwards, π flat outwards), `weight` × the
    /// distance to the next section far.
    Direction {
        /// Angle to the section plane.
        angle: Real,
        /// Weight.
        weight: Real,
    },
    /// Tangent to neighbouring faces: at points of the outline, the
    /// direction the loft continues in (into the loft), `weight` as above.
    Tangent {
        /// Points of the outline with the continuing direction.
        dirs: Vec<(Point3, Vector3)>,
        /// Weight.
        weight: Real,
    },
}

/// A placed section of an Area Loft.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LoftPlaced {
    /// Position: a fraction of the centre line between the first and the
    /// last section, or millimetres from the first section.
    pub position: Real,
    /// `position` is a fraction.
    pub proportional: bool,
    /// Area (mm²) or a scale factor.
    pub size: Real,
    /// `size` is a scale factor.
    pub scale: bool,
}

/// A guided loft.
pub struct GuidedLoft<'a> {
    /// Sections, in order, on their own planes.
    pub sections: Vec<(Plane, &'a ExactProfile)>,
    /// Rails (polylines in model space), each touching every section.
    pub rails: Vec<Vec<Point3>>,
    /// The centre line (Center Line, Area Loft).
    pub center: Option<Vec<Point3>>,
    /// Area Loft: the placed sections (Some, possibly empty).
    pub area: Option<Vec<LoftPlaced>>,
    /// Condition at the first section.
    pub start: LoftEnd,
    /// Condition at the last section.
    pub end: LoftEnd,
    /// Point sets: one model point on each section, in section order.
    pub map: Vec<Vec<Point3>>,
    /// The last section joins the first.
    pub closed: bool,
}

/// One station of a loft (for previews and tests): its outline in model
/// space.
pub type StationOutline = Vec<Point3>;

/// The loft through `g`'s sections, faces named as by [`super::loft`]
/// (`<prefix>:Side:<segment of the first section>`, `Start`, `End`).
pub fn loft_guided(g: &GuidedLoft, prefix: &str) -> Result<ExactSolid, String> {
    let stations = stations(g)?;
    let names = stations
        .first()
        .map(|s| s.names.clone())
        .unwrap_or_default();
    let input = stations
        .iter()
        .map(Sec::kernel)
        .collect::<Result<Vec<_>, _>>()?;
    let built = if g.closed {
        guarded("loft", || sk::loft_matched_ring(&input))
    } else {
        guarded("loft", || sk::loft_matched(&input))
    };
    let body = built.map_err(|e| format!("the geometry kernel cannot build it ({e})"))?;
    let face_names = names_from(&body, &[], &side_names(&names, prefix));
    Ok(ExactSolid {
        name: Some(prefix.to_owned()),
        body,
        face_names,
        profiles: Vec::new(),
        sheet: false,
    })
}

/// The stations' outlines (model space), the user's sections among them.
pub fn loft_stations(g: &GuidedLoft) -> Result<Vec<StationOutline>, String> {
    Ok(stations(g)?
        .iter()
        .map(|s| {
            s.segs
                .iter()
                .flat_map(|b| (0..8).map(move |i| b.eval(i as Real / 8.0)))
                .map(|p| s.world(p))
                .collect()
        })
        .collect())
}

// ------------------------------------------------------------ building ---

fn stations(g: &GuidedLoft) -> Result<Vec<Sec>, String> {
    let k = g.sections.len();
    if k < 2 {
        return Err("a loft needs two sections or more".into());
    }
    if g.closed && k < 3 {
        return Err("Closed Loop needs at least three sections".into());
    }
    if g.closed && (!g.rails.is_empty() || g.center.is_some()) {
        return Err("Closed Loop with rails or a centre line is not supported yet".into());
    }
    let guided = !g.rails.is_empty() || g.center.is_some();
    if guided && (g.start != LoftEnd::Free || g.end != LoftEnd::Free) {
        return Err("conditions together with rails or a centre line are not supported yet".into());
    }
    if g.area.is_some() && g.center.is_none() {
        return Err("Area Loft needs a centre line".into());
    }
    if !g.rails.is_empty() && g.center.is_some() {
        return Err("use either rails or a centre line".into());
    }
    let mut raw = Vec::with_capacity(k);
    let mut circles = Vec::with_capacity(k);
    for (i, (pl, prof)) in g.sections.iter().enumerate() {
        let (s, c) = raw_section(pl, prof, i)?;
        raw.push(s);
        circles.push(c);
    }
    let size = raw
        .iter()
        .flat_map(|s| s.segs.iter().map(move |b| s.world(b.start())))
        .fold((Point3::origin(), 0.0_f64), |acc, p| {
            (acc.0, acc.1.max((p - raw[0].o).norm()))
        })
        .1
        .max(1.0);
    // Centroids (model space) and the centre line's stations.
    let centroid = |s: &Sec| s.world(area_centroid(&s.segs).1);
    let center = match &g.center {
        Some(c) => Some(center_stations(&Track::new(c)?, &raw, size)?),
        None => None,
    };
    // Directions the loft runs in at each section.
    let cents: Vec<Point3> = raw.iter().map(centroid).collect();
    let travel: Vec<Vector3> = (0..k)
        .map(|i| match &center {
            Some(c) => c.tangents[i],
            None => {
                let (a, b) = if g.closed {
                    ((i + k - 1) % k, (i + 1) % k)
                } else {
                    (i.saturating_sub(1), (i + 1).min(k - 1))
                };
                cents[b] - cents[a]
            }
        })
        .collect();
    // Orient: normal along the run, loop counter-clockwise.
    for (s, (t, c)) in raw.iter_mut().zip(travel.iter().zip(circles.iter_mut())) {
        if s.n().dot(t) < 0.0 {
            s.gy = -s.gy;
            for b in s.segs.iter_mut() {
                *b = b.map(|p| V2::new(p.x, -p.y));
            }
            if let Some((cc, _)) = c.as_mut() {
                cc.y = -cc.y;
            }
        }
        if area_centroid(&s.segs).0 < 0.0 {
            s.reverse();
        }
        if s.n().dot(t).abs() < 1e-9 * t.norm().max(1e-300) {
            return Err("a section lies along the loft's direction".into());
        }
    }
    // Carried frames, origins.
    let frames: Vec<(Point3, Vector3, Vector3)> = match &center {
        Some(c) => (0..k)
            .map(|i| {
                let r = turn(&c.tangents[i], &raw[i].n());
                let (hx, hy) = c.frame_at(c.s[i]);
                let o = c.points[i];
                let o = o - raw[i].n() * (o - raw[i].o).dot(&raw[i].n());
                (o, r * hx, r * hy)
            })
            .collect(),
        None => {
            let mut v = Vec::with_capacity(k);
            let (mut gx, mut gy) = (raw[0].gx, raw[0].gy);
            for i in 0..k {
                if i > 0 {
                    let r = turn(&raw[i - 1].n(), &raw[i].n());
                    (gx, gy) = (r * gx, r * gy);
                }
                v.push((cents[i], gx, gy));
            }
            v
        }
    };
    for (s, (o, gx, gy)) in raw.iter_mut().zip(frames) {
        s.reframe(o, gx, gy);
    }
    // Circles in the new frames (a circle's centre is its loop's centroid).
    for (i, c) in circles.iter_mut().enumerate() {
        if let Some((_, r)) = *c {
            *c = Some((area_centroid(&raw[i].segs).1, r));
        }
    }
    // Rails: contacts (anchors) and arc lengths.
    let mut anchors: Vec<Vec<V2>> = vec![Vec::new(); k];
    let mut rails: Vec<(Track, Vec<Real>)> = Vec::new();
    for (j, pts) in g.rails.iter().enumerate() {
        let (track, s, contact) = rail_contacts(&Track::new(pts)?, &raw, j, size)?;
        for (a, c) in anchors.iter_mut().zip(contact) {
            a.push(c);
        }
        rails.push((track, s));
    }
    for (m, set) in g.map.iter().enumerate() {
        if set.len() != k {
            return Err(format!(
                "point set {} needs one point on each of the {k} sections",
                m + 1
            ));
        }
        for (i, q) in set.iter().enumerate() {
            let l = raw[i].local(q);
            let (si, t, d) = nearest(&raw[i].segs, l);
            if d > 0.05 + 2e-3 * size {
                return Err(format!(
                    "map point {} is {d:.2} mm off section {}",
                    m + 1,
                    i + 1
                ));
            }
            anchors[i].push(raw[i].segs[si].eval(t));
        }
    }
    let idx = matching(&mut raw, &circles, &anchors)?;
    let rail_idx: Vec<usize> = idx.iter().take(rails.len()).copied().collect();
    // Stations.
    let mut out = if let Some(c) = &center {
        center_line_stations(&raw, c, g.area.as_deref())?
    } else {
        let start = end_rates(&g.start, &raw, true)?;
        let end = end_rates(&g.end, &raw, false)?;
        let conditions = start.is_some() || end.is_some();
        let spans = if g.closed { k } else { k - 1 };
        let per = if !rails.is_empty() {
            per_span(&raw, spans, 20, 2)
        } else if conditions {
            per_span(&raw, spans, 10, 5)
        } else if g.closed {
            vec![6; spans]
        } else {
            vec![1; spans]
        };
        let mut st = free_stations(&raw, &per, start.as_ref(), end.as_ref(), g.closed);
        if !rails.is_empty() {
            for s in st.iter_mut().filter(|s| !s.original) {
                let targets: Vec<Point3> = rails
                    .iter()
                    .map(|(t, sj)| t.at(sj[s.span] + s.tau * (sj[s.span + 1] - sj[s.span])))
                    .collect();
                rail_fit(&mut s.sec, &rail_idx, &targets)?;
            }
        }
        st.into_iter().map(|s| s.sec).collect()
    };
    check_folds(&out, g.closed)?;
    if out.len() > 50 {
        return Err(format!(
            "too many sections for these options ({} stations, the kernel lofts through 50 at \
             most)",
            out.len()
        ));
    }
    // The user's names follow the first section.
    let names = raw[0].names.clone();
    for s in out.iter_mut() {
        s.names = names.clone();
    }
    Ok(out)
}

/// Refuses stations that fold back: every point of a station must lie in
/// front of the previous station's plane (the planes face the way the loft
/// runs). Otherwise the side faces between them cross each other — a
/// self-intersecting solid the kernel would still return.
fn check_folds(st: &[Sec], closed: bool) -> Result<(), String> {
    let n = st.len();
    let pairs = if closed { n } else { n.saturating_sub(1) };
    for i in 0..pairs {
        let (a, b) = (&st[i], &st[(i + 1) % n]);
        let normal = a.n();
        let pts: Vec<Point3> = b
            .segs
            .iter()
            .flat_map(|z| (0..8).map(move |j| z.eval(j as Real / 8.0)))
            .map(|q| b.world(q))
            .collect();
        let size = pts
            .iter()
            .map(|p| (p - b.o).norm())
            .fold(0.0, Real::max)
            .max(1e-3);
        let worst = pts
            .iter()
            .map(|p| (p - a.o).dot(&normal))
            .fold(Real::MAX, Real::min);
        if worst < -1e-4 * size {
            return Err(format!(
                "the loft would cross itself: its surface folds back {:.2} mm behind a section \
                 between sections (the guide bends too sharply for the section size, or a \
                 section leans too far from it)",
                -worst
            ));
        }
    }
    Ok(())
}

/// Intermediate stations per span (∝ the span's length, at least `min`,
/// `total` in all, the kernel's limit kept).
fn per_span(secs: &[Sec], spans: usize, total: usize, min: usize) -> Vec<usize> {
    let k = secs.len();
    let len: Vec<Real> = (0..spans)
        .map(|i| (secs[(i + 1) % k].o - secs[i].o).norm())
        .collect();
    let sum: Real = len.iter().sum::<Real>().max(1e-300);
    let budget = total.min(MAX_STATIONS.saturating_sub(k)).max(spans * min);
    len.iter()
        .map(|l| ((budget as Real * l / sum).round() as usize).max(min))
        .collect()
}

/// Matches the sections: equal piece counts, vertex `i` of each section
/// paired with vertex `i` of the next. Anchors (rail contacts, then map
/// points; one list per section) become shared vertices; returns their
/// vertex indices (the same on every section).
fn matching(
    secs: &mut [Sec],
    circles: &[Option<(V2, Real)>],
    anchors: &[Vec<V2>],
) -> Result<Vec<usize>, String> {
    let k = secs.len();
    let a = anchors.first().map_or(0, Vec::len);
    if a == 0 {
        // Whole circles take the vertex directions of the busiest other section.
        let lead = (0..k)
            .filter(|&i| circles[i].is_none())
            .max_by_key(|&i| (secs[i].segs.len(), std::cmp::Reverse(i)));
        if let Some(l) = lead {
            let cl = area_centroid(&secs[l].segs).1;
            let angles: Vec<Real> = (0..secs[l].segs.len())
                .map(|j| {
                    let d = secs[l].vertex(j) - cl;
                    d.y.atan2(d.x)
                })
                .collect();
            for (i, c) in circles.iter().enumerate() {
                let Some((cc, r)) = *c else { continue };
                if angles.len() < 2 {
                    continue;
                }
                let name = secs[i].names.first().cloned().unwrap_or_default();
                let mut segs = Vec::with_capacity(angles.len());
                for j in 0..angles.len() {
                    let a0 = angles[j];
                    let mut a1 = angles[(j + 1) % angles.len()];
                    while a1 <= a0 + 1e-12 {
                        a1 += std::f64::consts::TAU;
                    }
                    segs.push(arc_bez(cc, r, a0, a1 - a0).ok_or("arc")?);
                }
                let n = segs.len();
                secs[i].segs = segs;
                secs[i].names = vec![name; n];
            }
        }
        let n = secs.iter().map(|s| s.segs.len()).max().unwrap_or(0);
        for s in secs.iter_mut() {
            while s.segs.len() < n {
                split_longest(s, 0, s.segs.len());
            }
        }
        // Each section turned to start where its vertices best follow the
        // previous section's (by direction from the centroids).
        for i in 1..k {
            let (prev, cur) = secs.split_at_mut(i);
            let (p, c) = (&prev[i - 1], &mut cur[0]);
            let dirs = |s: &Sec| -> Vec<V2> {
                let cc = area_centroid(&s.segs).1;
                (0..s.segs.len())
                    .map(|j| (s.vertex(j) - cc).try_normalize(1e-300).unwrap_or_default())
                    .collect()
            };
            let (dp, dc) = (dirs(p), dirs(c));
            let m = dc.len();
            let fit = |r: usize| -> Real { (0..m).map(|j| dp[j].dot(&dc[(j + r) % m])).sum() };
            let best = (0..m)
                .max_by(|x, y| fit(*x).total_cmp(&fit(*y)))
                .unwrap_or(0);
            c.rotate_left(best);
        }
        return Ok(Vec::new());
    }
    // Anchors become vertices.
    let mut order0: Vec<usize> = Vec::new();
    let mut chains_of: Vec<Vec<usize>> = Vec::with_capacity(k);
    for (i, s) in secs.iter_mut().enumerate() {
        for q in &anchors[i] {
            let (si, t, _) = nearest(&s.segs, *q);
            let tol = 1e-6 / s.segs[si].length().max(1e-6);
            if t > tol && t < 1.0 - tol {
                s.split(si, t);
            }
        }
        let idx: Vec<usize> = anchors[i]
            .iter()
            .map(|q| {
                (0..s.segs.len())
                    .min_by(|x, y| {
                        (s.vertex(*x) - q)
                            .norm()
                            .total_cmp(&(s.vertex(*y) - q).norm())
                    })
                    .unwrap_or(0)
            })
            .collect();
        for x in 0..idx.len() {
            for y in x + 1..idx.len() {
                if idx[x] == idx[y] {
                    return Err(format!(
                        "two rails or map points meet section {} at the same point",
                        i + 1
                    ));
                }
            }
        }
        // Start at the first anchor.
        let n = s.segs.len();
        s.rotate_left(idx[0]);
        let idx: Vec<usize> = idx.iter().map(|v| (v + n - idx[0]) % n).collect();
        let mut order: Vec<usize> = (0..a).collect();
        order.sort_by_key(|&j| idx[j]);
        if i == 0 {
            order0 = order.clone();
        } else if order != order0 {
            return Err(format!(
                "the rails or map points run round section {} in a different order than \
                 round section 1",
                i + 1
            ));
        }
        // Chain boundaries in that order, closed by the loop's end.
        let mut b: Vec<usize> = order.iter().map(|&j| idx[j]).collect();
        b.push(n);
        chains_of.push(b);
    }
    // Every chain gets the same number of pieces on every section.
    for c in (0..a).rev() {
        let want = chains_of.iter().map(|b| b[c + 1] - b[c]).max().unwrap_or(1);
        for (s, b) in secs.iter_mut().zip(chains_of.iter_mut()) {
            while b[c + 1] - b[c] < want {
                split_longest(s, b[c], b[c + 1]);
                for x in b.iter_mut().skip(c + 1) {
                    *x += 1;
                }
            }
        }
    }
    let b = &chains_of[0];
    let mut idx = vec![0; a];
    for (pos, &j) in order0.iter().enumerate() {
        idx[j] = b[pos];
    }
    Ok(idx)
}

/// Splits the longest piece among `from..to` in half.
fn split_longest(s: &mut Sec, from: usize, to: usize) {
    let Some(i) = (from..to.min(s.segs.len()))
        .max_by(|x, y| s.segs[*x].length().total_cmp(&s.segs[*y].length()))
    else {
        return;
    };
    s.split(i, 0.5);
}

/// Where every rail touches every section: the rail (turned to run from the
/// first section), its arc length at each section and the contact on each
/// section's outline (section coordinates).
fn rail_contacts(
    track: &Track,
    secs: &[Sec],
    j: usize,
    size: Real,
) -> Result<(Track, Vec<Real>, Vec<V2>), String> {
    let tol = 0.05 + 2e-3 * size;
    let mut s = Vec::with_capacity(secs.len());
    let mut contacts = Vec::with_capacity(secs.len());
    for (i, sec) in secs.iter().enumerate() {
        let n = sec.n();
        let mut best: Option<(Real, Real, V2)> = None;
        for (sv, p) in track.crossings(&sec.o, &n, 1e-7 * size) {
            let l = sec.local(&p);
            let (si, t, d) = nearest(&sec.segs, l);
            if best.as_ref().is_none_or(|b| d < b.1) {
                best = Some((sv, d, sec.segs[si].eval(t)));
            }
        }
        let Some((sv, d, c)) = best else {
            return Err(format!(
                "rail {} does not reach section {} (every section must touch every rail)",
                j + 1,
                i + 1
            ));
        };
        if d > tol {
            return Err(format!(
                "rail {} does not touch section {}: it crosses the section's plane {d:.2} mm \
                 from its outline",
                j + 1,
                i + 1
            ));
        }
        s.push(sv);
        contacts.push(c);
    }
    let up = s.windows(2).all(|w| w[1] > w[0] + 1e-9);
    let down = s.windows(2).all(|w| w[1] < w[0] - 1e-9);
    if down {
        let l = track.length();
        return Ok((
            track.reversed(),
            s.iter().map(|x| l - x).collect(),
            contacts,
        ));
    }
    if !up {
        return Err(format!(
            "rail {} does not run through the sections in their order",
            j + 1
        ));
    }
    Ok((track.clone(), s, contacts))
}

/// A station between sections, with where it lies.
struct Station {
    sec: Sec,
    span: usize,
    tau: Real,
    original: bool,
}

/// End tangent of an end section: rate of its origin along its normal and of
/// each control point along its outward normal (per unit of the span's
/// parameter).
struct EndRates {
    normal: Real,
    inplane: Vec<Real>,
}

fn end_rates(c: &LoftEnd, secs: &[Sec], start: bool) -> Result<Option<EndRates>, String> {
    let k = secs.len();
    let (i, j) = if start { (0, 1) } else { (k - 1, k - 2) };
    let len = (secs[j].o - secs[i].o).norm();
    let ncp = secs[i].segs.len() * N;
    match c {
        LoftEnd::Free => Ok(None),
        LoftEnd::Direction { angle, weight } => {
            let w = weight.clamp(0.0, 50.0) * len;
            let (s, co) = angle.sin_cos();
            // The loft leaves the plane along sin·n − cos·ν (ν outward); at
            // the end section it arrives, so the in-plane part turns round.
            let inplane = if start { -co * w } else { co * w };
            Ok(Some(EndRates {
                normal: s * w,
                inplane: vec![inplane; ncp],
            }))
        }
        LoftEnd::Tangent { dirs, weight } => {
            if dirs.is_empty() {
                return Err("the tangent condition has no neighbouring faces".into());
            }
            let w = weight.clamp(0.0, 50.0) * len;
            let sec = &secs[i];
            let n = sec.n();
            let normals = control_normals(&sec.segs);
            let mut inplane = Vec::with_capacity(ncp);
            for (q, nu) in sec.segs.iter().flat_map(|b| b.0.iter()).zip(normals.iter()) {
                let p = sec.world(*q);
                let d = dirs
                    .iter()
                    .min_by(|a, b| (a.0 - p).norm().total_cmp(&(b.0 - p).norm()))
                    .map(|x| x.1)
                    .unwrap_or(n);
                let a = d.dot(&n).abs().max(0.2);
                let nw = sec.gx * nu.x + sec.gy * nu.y;
                let b = d.dot(&nw);
                let sign = if start { 1.0 } else { -1.0 };
                inplane.push(sign * (b / a).clamp(-5.0, 5.0) * w);
            }
            Ok(Some(EndRates { normal: w, inplane }))
        }
    }
}

fn flat(segs: &[Bez]) -> Vec<V2> {
    segs.iter().flat_map(|b| b.0).collect()
}

fn unflat(v: &[V2]) -> Vec<Bez> {
    v.chunks(N)
        .map(|c| {
            let mut p = [V2::zeros(); N];
            p.copy_from_slice(c);
            Bez(p)
        })
        .collect()
}

fn hermite(t: Real) -> [Real; 4] {
    let (t2, t3) = (t * t, t * t * t);
    [
        2.0 * t3 - 3.0 * t2 + 1.0,
        t3 - 2.0 * t2 + t,
        -2.0 * t3 + 3.0 * t2,
        t3 - t2,
    ]
}

/// The tangent at `b` of a curve through `a`, `b`, `c`, per span of chord
/// `chord`: along `c − a` (Catmull-Rom), as long as a circular arc through
/// the points would need (a ring of sections stays round: plain
/// Catmull-Rom shrinks six points on a circle by 2.6 % between them).
fn through(a: &Point3, b: &Point3, c: &Point3, chord: Real) -> Vector3 {
    let (u, v) = (b - a, c - b);
    let Some(d) = (c - a).try_normalize(1e-300) else {
        return v;
    };
    let cos = match (u.try_normalize(1e-300), v.try_normalize(1e-300)) {
        (Some(x), Some(y)) => x.dot(&y).clamp(-1.0, 1.0),
        _ => 1.0,
    };
    let half = cos.acos() / 2.0;
    let f = if half < 1e-6 { 1.0 } else { half / half.sin() };
    d * chord * f
}

/// Stations between the sections without a guide: frames turned by the
/// smallest rotation, origins and shapes as cubic Hermite curves through
/// the sections (Catmull-Rom tangents; end conditions override the ends).
fn free_stations(
    secs: &[Sec],
    per: &[usize],
    start: Option<&EndRates>,
    end: Option<&EndRates>,
    closed: bool,
) -> Vec<Station> {
    let k = secs.len();
    let at = |i: isize| -> Option<usize> {
        if closed {
            Some(i.rem_euclid(k as isize) as usize)
        } else {
            (i >= 0 && (i as usize) < k).then_some(i as usize)
        }
    };
    let mut out = Vec::new();
    for (sp, &count) in per.iter().enumerate() {
        let a = sp;
        let b = (sp + 1) % k;
        let sa = &secs[a];
        let rab = turn(&sa.n(), &secs[b].n());
        let (gbx, gby) = (rab * sa.gx, rab * sa.gy);
        let shape_a = flat(&sa.segs);
        let shape_b = flat(&secs[b].coords_in(secs[b].o, gbx, gby));
        let prev = at(a as isize - 1).filter(|_| closed || a > 0).map(|p| {
            let r = turn(&sa.n(), &secs[p].n());
            (
                flat(&secs[p].coords_in(secs[p].o, r * sa.gx, r * sa.gy)),
                secs[p].o,
            )
        });
        let next = at(sp as isize + 2)
            .filter(|_| closed || sp + 2 < k)
            .map(|q| {
                let r = turn(&secs[b].n(), &secs[q].n());
                (
                    flat(&secs[q].coords_in(secs[q].o, r * gbx, r * gby)),
                    secs[q].o,
                )
            });
        let (oa, ob) = (sa.o, secs[b].o);
        let m = shape_a.len();
        let mut ma: Vec<V2> = match &prev {
            Some((p, _)) => (0..m).map(|i| (shape_b[i] - p[i]) / 2.0).collect(),
            None => (0..m).map(|i| shape_b[i] - shape_a[i]).collect(),
        };
        let mut mb: Vec<V2> = match &next {
            Some((q, _)) => (0..m).map(|i| (q[i] - shape_a[i]) / 2.0).collect(),
            None => (0..m).map(|i| shape_b[i] - shape_a[i]).collect(),
        };
        let chord = (ob - oa).norm();
        let mut moa = match &prev {
            Some((_, p)) => through(p, &oa, &ob, chord),
            None => ob - oa,
        };
        let mut mob = match &next {
            Some((_, q)) => through(&oa, &ob, q, chord),
            None => ob - oa,
        };
        if !closed
            && a == 0
            && let Some(r) = start
        {
            moa = sa.n() * r.normal;
            let nu = control_normals(&sa.segs);
            for (i, x) in ma.iter_mut().enumerate() {
                *x = nu[i] * r.inplane[i];
            }
        }
        if !closed
            && b == k - 1
            && let Some(r) = end
        {
            mob = secs[b].n() * r.normal;
            let nu = control_normals(&unflat(&shape_b));
            for (i, x) in mb.iter_mut().enumerate() {
                *x = nu[i] * r.inplane[i];
            }
        }
        out.push(Station {
            sec: sa.clone(),
            span: sp,
            tau: 0.0,
            original: true,
        });
        let j = count.max(1);
        for step in 1..j {
            let tau = step as Real / j as Real;
            let h = hermite(tau);
            let r = rab.powf(tau);
            let o = Point3::from(oa.coords * h[0] + moa * h[1] + ob.coords * h[2] + mob * h[3]);
            let shape: Vec<V2> = (0..m)
                .map(|i| shape_a[i] * h[0] + ma[i] * h[1] + shape_b[i] * h[2] + mb[i] * h[3])
                .collect();
            out.push(Station {
                sec: Sec {
                    o,
                    gx: r * sa.gx,
                    gy: r * sa.gy,
                    segs: unflat(&shape),
                    names: sa.names.clone(),
                },
                span: sp,
                tau,
                original: false,
            });
        }
    }
    if !closed {
        out.push(Station {
            sec: secs[k - 1].clone(),
            span: k - 1,
            tau: 0.0,
            original: true,
        });
    }
    out
}

/// Moves, turns and scales a station so that its vertices `idx` land on the
/// rail points `targets`.
fn rail_fit(st: &mut Sec, idx: &[usize], targets: &[Point3]) -> Result<(), String> {
    let m = targets.len();
    let a: Vec<V2> = idx.iter().map(|&v| st.vertex(v)).collect();
    if m == 0 {
        return Ok(());
    }
    if m == 1 {
        let x = st.world(a[0]);
        st.o += targets[0] - x;
        return Ok(());
    }
    let nf = st.n();
    let tc = Point3::from(targets.iter().map(|p| p.coords).sum::<Vector3>() / m as Real);
    let mut n = if m == 2 {
        let d = (targets[1] - targets[0])
            .try_normalize(1e-12)
            .ok_or("two rails meet at one point")?;
        (nf - d * nf.dot(&d)).try_normalize(1e-12).unwrap_or(nf)
    } else {
        let mut cov = Matrix3::zeros();
        for p in targets {
            let d = p - tc;
            cov += d * d.transpose();
        }
        let e = cov.symmetric_eigen();
        let mut order = [0, 1, 2];
        order.sort_by(|x, y| e.eigenvalues[*x].total_cmp(&e.eigenvalues[*y]));
        let spread = e.eigenvalues[order[1]].max(0.0).sqrt();
        let big = e.eigenvalues[order[2]].max(0.0).sqrt();
        if spread < 1e-6 * big.max(1e-300) {
            return Err("the rails meet the stations in a line".into());
        }
        e.eigenvectors.column(order[0]).into_owned()
    };
    if n.dot(&nf) < 0.0 {
        n = -n;
    }
    // Turn the plane about the contacts' centre, then bring it onto the rails.
    let ac = Point3::from(a.iter().map(|p| st.world(*p).coords).sum::<Vector3>() / m as Real);
    let r = turn(&nf, &n);
    st.gx = r * st.gx;
    st.gy = r * st.gy;
    st.o = ac + r * (st.o - ac);
    st.o += n * (tc - st.o).dot(&n);
    let b: Vec<V2> = targets.iter().map(|p| st.local(p)).collect();
    let f: Box<dyn Fn(V2) -> V2> = if m == 2 {
        // Similarity (z ↦ b0 + (b1 − b0)/(a1 − a0)·(z − a0), complex numbers).
        let (da, db) = (a[1] - a[0], b[1] - b[0]);
        let l2 = da.norm_squared();
        if l2 < 1e-24 {
            return Err("two rails touch a section at one point".into());
        }
        let q = V2::new(
            (db.x * da.x + db.y * da.y) / l2,
            (db.y * da.x - db.x * da.y) / l2,
        );
        let (a0, b0) = (a[0], b[0]);
        Box::new(move |z: V2| {
            let d = z - a0;
            b0 + V2::new(q.x * d.x - q.y * d.y, q.x * d.y + q.y * d.x)
        })
    } else {
        let am = a.iter().sum::<V2>() / m as Real;
        let bm = b.iter().sum::<V2>() / m as Real;
        let mut saa = Matrix2::zeros();
        let mut sba = Matrix2::zeros();
        for (p, q) in a.iter().zip(&b) {
            let (pa, qb) = (p - am, q - bm);
            saa += pa * pa.transpose();
            sba += qb * pa.transpose();
        }
        let inv = saa
            .try_inverse()
            .ok_or("the rails touch a section along a line")?;
        let mat = sba * inv;
        if mat.determinant() <= 0.0 {
            return Err("the rails cross each other between two sections".into());
        }
        let res: Vec<V2> = a
            .iter()
            .zip(&b)
            .map(|(p, q)| q - (bm + mat * (p - am)))
            .collect();
        let a2 = a.clone();
        Box::new(move |z: V2| {
            let mut out = bm + mat * (z - am);
            if a2.len() > 3 {
                // Smooth correction so that every contact lands exactly.
                let mut wsum = 0.0;
                let mut corr = V2::zeros();
                for (p, r) in a2.iter().zip(&res) {
                    let d2 = (z - p).norm_squared();
                    if d2 < 1e-24 {
                        return out + r;
                    }
                    let w = 1.0 / d2;
                    wsum += w;
                    corr += r * w;
                }
                out += corr / wsum;
            }
            out
        })
    };
    for s in st.segs.iter_mut() {
        *s = s.map(&f);
    }
    Ok(())
}

/// A centre line with the sections' places on it.
struct CenterLine {
    track: Track,
    /// Arc length of each section.
    s: Vec<Real>,
    /// Where it meets each section's plane (or comes nearest).
    points: Vec<Point3>,
    /// Its unit tangent there.
    tangents: Vec<Vector3>,
    /// Rotation-minimising x axes, one per piece.
    xs: Vec<Vector3>,
}

impl CenterLine {
    /// The carried frame (x, y; normal = tangent) at arc length `s`.
    fn frame_at(&self, s: Real) -> (Vector3, Vector3) {
        let i = self.track.piece(s);
        let t = self.track.tangent_of(i);
        let x = self.xs[i];
        let tt = self.track.tangent(s);
        let r = turn(&t, &tt);
        let x = r * x;
        (x, tt.cross(&x))
    }
}

fn center_stations(track: &Track, secs: &[Sec], size: Real) -> Result<CenterLine, String> {
    let place = |t: &Track, sec: &Sec| -> (Real, Point3) {
        let c = sec.world(area_centroid(&sec.segs).1);
        let n = sec.n();
        t.crossings(&sec.o, &n, 1e-7 * size)
            .into_iter()
            .min_by(|a, b| (a.1 - c).norm().total_cmp(&(b.1 - c).norm()))
            .unwrap_or_else(|| t.nearest(&c))
    };
    let mut track = track.clone();
    let mut s: Vec<Real> = secs.iter().map(|x| place(&track, x).0).collect();
    if s.windows(2).all(|w| w[1] < w[0] - 1e-9) {
        track = track.reversed();
        s = secs.iter().map(|x| place(&track, x).0).collect();
    }
    if !s.windows(2).all(|w| w[1] > w[0] + 1e-9) {
        return Err("the centre line does not run through the sections in their order".into());
    }
    let points: Vec<Point3> = secs.iter().map(|x| place(&track, x).1).collect();
    let tangents: Vec<Vector3> = s.iter().map(|&x| track.tangent(x)).collect();
    // The first frame: the first section's x axis turned square to the line.
    let t0 = track.tangent_of(0);
    let n0 = secs[0].n();
    let n0 = if n0.dot(&tangents[0]) < 0.0 { -n0 } else { n0 };
    let x0 = turn(&n0, &tangents[0]) * secs[0].gx;
    let x0 = turn(&tangents[0], &t0) * x0;
    let x0 = (x0 - t0 * x0.dot(&t0))
        .try_normalize(1e-12)
        .ok_or("the centre line runs along the first section")?;
    let xs = track.frames(x0);
    Ok(CenterLine {
        track,
        s,
        points,
        tangents,
        xs,
    })
}

/// Stations along a centre line: square to it, carried frames, shapes
/// interpolated between the sections (relative to the line), scaled to the
/// area law of an Area Loft.
fn center_line_stations(
    secs: &[Sec],
    c: &CenterLine,
    area: Option<&[LoftPlaced]>,
) -> Result<Vec<Sec>, String> {
    let k = secs.len();
    let (s0, s1) = (c.s[0], c.s[k - 1]);
    let shapes: Vec<Vec<V2>> = secs.iter().map(|x| flat(&x.segs)).collect();
    let m = shapes[0].len();
    // How each section's plane is tilted from square to the line, as a
    // rotation vector in the carried frame there (x, y, tangent). The
    // stations blend it out away from the section: a section that is not
    // square to the line keeps its plane, and the stations next to it lean
    // like it instead of cutting through it (the owner's loft dipped below
    // its bottom section when the stations were square right up to it).
    let tilts: Vec<Vector3> = (0..k)
        .map(|i| {
            let (hx, hy) = c.frame_at(c.s[i]);
            let t = c.track.tangent(c.s[i]);
            let w = turn(&t, &secs[i].n()).scaled_axis();
            Vector3::new(w.dot(&hx), w.dot(&hy), w.dot(&t))
        })
        .collect();
    let frame = |s: Real, span: usize, tau: Real| -> (Vector3, Vector3) {
        let (hx, hy) = c.frame_at(s);
        let t = c.track.tangent(s);
        let h = hermite(tau);
        let w = tilts[span] * h[0] + tilts[span + 1] * h[2];
        let r = Rotation3::new(hx * w.x + hy * w.y + t * w.z);
        (r * hx, r * hy)
    };
    // Shape (unscaled) at arc length s: span and Catmull-Rom Hermite.
    let shape_at = |s: Real| -> (usize, Real, Vec<V2>) {
        let span = (0..k - 1)
            .find(|&i| s <= c.s[i + 1] + 1e-12)
            .unwrap_or(k - 2);
        let (a, b) = (span, span + 1);
        let tau = ((s - c.s[a]) / (c.s[b] - c.s[a])).clamp(0.0, 1.0);
        let h = hermite(tau);
        let tan = |i: usize, j: usize| -> V2 {
            let lo = if i > 0 { i - 1 } else { i };
            let hi = if i + 1 < k { i + 1 } else { i };
            let f = if hi - lo == 2 { 0.5 } else { 1.0 };
            (shapes[hi][j] - shapes[lo][j]) * f
        };
        let v = (0..m)
            .map(|j| {
                shapes[a][j] * h[0] + tan(a, j) * h[1] + shapes[b][j] * h[2] + tan(b, j) * h[3]
            })
            .collect();
        (span, tau, v)
    };
    // Area law: (arc length, area) knots, linear between them.
    let mut knots: Vec<(Real, Real)> = Vec::new();
    let mut breaks: Vec<Real> = c.s.clone();
    if let Some(placed) = area {
        for (i, x) in secs.iter().enumerate() {
            knots.push((c.s[i], area_centroid(&x.segs).0));
        }
        for p in placed {
            let s = if p.proportional {
                s0 + p.position * (s1 - s0)
            } else {
                s0 + p.position
            };
            if !(s > s0 + 1e-6 && s < s1 - 1e-6) {
                return Err(format!(
                    "a placed section at {:.3} lies outside the loft (between its first and \
                     last section)",
                    p.position
                ));
            }
            if p.size <= 0.0 {
                return Err("a placed section needs a positive area or scale".into());
            }
            let a = if p.scale {
                let (_, _, v) = shape_at(s);
                area_centroid(&unflat(&v)).0 * p.size * p.size
            } else {
                p.size
            };
            knots.push((s, a));
            breaks.push(s);
        }
        knots.sort_by(|x, y| x.0.total_cmp(&y.0));
    }
    breaks.sort_by(|x, y| x.total_cmp(y));
    breaks.dedup_by(|x, y| (*x - *y).abs() < 1e-6);
    let target = |s: Real| -> Option<Real> {
        if knots.is_empty() {
            return None;
        }
        let i = knots.iter().position(|kn| kn.0 >= s - 1e-12)?;
        if i == 0 {
            return Some(knots[0].1);
        }
        let (a, b) = (knots[i - 1], knots[i]);
        let t = if b.0 - a.0 > 1e-12 {
            (s - a.0) / (b.0 - a.0)
        } else {
            1.0
        };
        Some(a.1 + (b.1 - a.1) * t)
    };
    let total = (s1 - s0).max(1e-300);
    let budget = 22usize.min(MAX_STATIONS.saturating_sub(breaks.len()));
    let mut out = Vec::new();
    for w in breaks.windows(2) {
        let (a, b) = (w[0], w[1]);
        // The user's section at `a`.
        if let Some(i) = c.s.iter().position(|x| (x - a).abs() < 1e-6) {
            out.push(secs[i].clone());
        }
        let j = ((budget as Real * (b - a) / total).round() as usize).max(2);
        let first = if c.s.iter().any(|x| (x - a).abs() < 1e-6) {
            1
        } else {
            0
        };
        for step in first..j {
            let s = a + (b - a) * step as Real / j as Real;
            let (span, tau, v) = shape_at(s);
            let mut segs = unflat(&v);
            if let Some(want) = target(s) {
                let (have, cen) = area_centroid(&segs);
                if have <= 1e-12 {
                    return Err("a station of the area loft has no area".into());
                }
                let f = (want / have).sqrt();
                segs = segs
                    .iter()
                    .map(|bz| bz.map(|p| cen + (p - cen) * f))
                    .collect();
            }
            let (gx, gy) = frame(s, span, tau);
            out.push(Sec {
                o: c.track.at(s),
                gx,
                gy,
                segs,
                names: secs[0].names.clone(),
            });
        }
    }
    out.push(secs[k - 1].clone());
    Ok(out)
}

/// Points along a profile's outer loop on `plane` (model space, closed: the
/// first point repeated at the end).
pub fn profile_outline(plane: &Plane, prof: &ExactProfile) -> Vec<Point3> {
    let mut out: Vec<Point3> = Vec::new();
    for s in &prof.region.outer.segs {
        let n = if matches!(s, sg::Seg2::Line { .. }) {
            1
        } else {
            16
        };
        for i in 0..n {
            let p = s.point_at(i as Real / n as Real);
            out.push(plane.point_at(p.x, p.y));
        }
    }
    if let Some(f) = out.first().copied() {
        out.push(f);
    }
    out
}

/// The area of a profile's outer loop less its holes (mm²).
pub fn profile_area(prof: &ExactProfile) -> Real {
    let outer = prof.region.outer.signed_area().abs();
    let holes: Real = prof
        .region
        .holes
        .iter()
        .map(|h| h.signed_area().abs())
        .sum();
    outer - holes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bezier_pieces() {
        let b = arc_bez(V2::new(1.0, 2.0), 5.0, 0.3, std::f64::consts::FRAC_PI_2).unwrap();
        for i in 0..=20 {
            let t = i as Real / 20.0;
            let p = b.eval(t);
            let err = ((p - V2::new(1.0, 2.0)).norm() - 5.0).abs();
            assert!(err < 2e-6, "{t}: {err}");
        }
        let (l, r) = b.split(0.3);
        assert!((l.eval(1.0) - b.eval(0.3)).norm() < 1e-12);
        assert!((r.eval(0.5) - b.eval(0.65)).norm() < 1e-12);
        let line = Bez::from_fn(|t| V2::new(t * 3.0, 1.0)).unwrap();
        assert!((line.deriv(0.4) - V2::new(3.0, 0.0)).norm() < 1e-9);
        let sq: Vec<Bez> = [(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)]
            .iter()
            .zip([(2.0, 0.0), (2.0, 2.0), (0.0, 2.0), (0.0, 0.0)])
            .map(|(a, b)| {
                Bez::from_fn(|t| V2::new(a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)).unwrap()
            })
            .collect();
        let (a, c) = area_centroid(&sq);
        assert!((a - 4.0).abs() < 1e-9 && (c - V2::new(1.0, 1.0)).norm() < 1e-9);
        let nu = control_normals(&sq);
        assert!((nu[1] - V2::new(0.0, -1.0)).norm() < 1e-9, "{:?}", nu[1]);
        assert!((nu[0] - V2::new(-1.0, -1.0)).norm() < 1e-9, "{:?}", nu[0]);
    }
}
