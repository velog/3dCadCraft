//! What the plastic part commands share: exact offsets of a sketch's lines and arcs, bands along
//! a curve, prisms between planes with a draft, and the wall of the body under a point.

use crate::kernel::exact::{self, ExactSolid, PrismSeg};
use crate::kernel::{Plane, Point3, Real, Vector3};
use crate::part::exact::{EdgePoints, Fail};
use crate::part::rib::Rays;
use crate::sketch::{Point2, ProfileCurve, ProfileLoop, Vector2};

/// A piece of a planar curve: only lines and arcs are offset exactly.
#[derive(Clone, Copy, Debug)]
pub(super) enum Piece {
    /// From `a` to `b`.
    Line(Point2, Point2),
    /// About `c`, radius `r`, from angle `a0` over the signed `sweep`.
    Arc {
        /// Centre.
        c: Point2,
        /// Radius.
        r: Real,
        /// Start angle.
        a0: Real,
        /// Signed sweep.
        sweep: Real,
    },
}

impl Piece {
    pub(super) fn start(&self) -> Point2 {
        match *self {
            Piece::Line(a, _) => a,
            Piece::Arc { c, r, a0, .. } => c + Vector2::new(a0.cos(), a0.sin()) * r,
        }
    }

    pub(super) fn end(&self) -> Point2 {
        match *self {
            Piece::Line(_, b) => b,
            Piece::Arc { c, r, a0, sweep } => {
                c + Vector2::new((a0 + sweep).cos(), (a0 + sweep).sin()) * r
            }
        }
    }

    fn closed(&self) -> bool {
        matches!(self, Piece::Arc { sweep, .. } if sweep.abs() >= std::f64::consts::TAU - 1e-9)
    }

    /// The other way round.
    pub(super) fn reversed(&self) -> Piece {
        match *self {
            Piece::Line(a, b) => Piece::Line(b, a),
            Piece::Arc { c, r, a0, sweep } => Piece::Arc {
                c,
                r,
                a0: a0 + sweep,
                sweep: -sweep,
            },
        }
    }

    /// Offset by `d` to its left. `None`: an arc would vanish.
    fn offset(&self, d: Real) -> Option<Piece> {
        match *self {
            Piece::Line(a, b) => {
                let t = (b - a).try_normalize(1e-12)?;
                let n = Vector2::new(-t.y, t.x) * d;
                Some(Piece::Line(a + n, b + n))
            }
            Piece::Arc { c, r, a0, sweep } => {
                let r2 = if sweep > 0.0 { r - d } else { r + d };
                (r2 > 1e-9).then_some(Piece::Arc {
                    c,
                    r: r2,
                    a0,
                    sweep,
                })
            }
        }
    }

    /// This piece with its end moved to `to` (on its line or circle).
    fn with_end(self, to: Point2) -> Piece {
        match self {
            Piece::Line(a, _) => Piece::Line(a, to),
            Piece::Arc { c, r, a0, sweep } => {
                let v = to - c;
                Piece::Arc {
                    c,
                    r,
                    a0,
                    sweep: same_turn(v.y.atan2(v.x) - a0, sweep),
                }
            }
        }
    }

    /// This piece with its start moved to `to` (on its line or circle).
    fn with_start(self, to: Point2) -> Piece {
        match self {
            Piece::Line(_, b) => Piece::Line(to, b),
            Piece::Arc { c, r, a0, sweep } => {
                let v = to - c;
                let a = v.y.atan2(v.x);
                Piece::Arc {
                    c,
                    r,
                    a0: a,
                    sweep: same_turn(a0 + sweep - a, sweep),
                }
            }
        }
    }

    /// As prism section segments; arcs in conics of at most a third of a turn.
    fn prism(&self) -> Vec<PrismSeg> {
        match *self {
            Piece::Line(a, b) => vec![PrismSeg::Line(a, b)],
            Piece::Arc { c, r, a0, sweep } => {
                let n = (sweep.abs() / (std::f64::consts::TAU / 3.0))
                    .ceil()
                    .max(1.0) as usize;
                let step = sweep / n as Real;
                (0..n)
                    .map(|k| {
                        let a = a0 + step * k as Real;
                        let m = a + step / 2.0;
                        let w = (step / 2.0).cos();
                        let at = |t: Real| c + Vector2::new(t.cos(), t.sin()) * r;
                        let apex = c + Vector2::new(m.cos(), m.sin()) * (r / w);
                        PrismSeg::Conic(at(a), apex, at(a + step), w)
                    })
                    .collect()
            }
        }
    }
}

/// An angle wrapped to the turn of `sweep` (same sign, no more than a full turn).
fn same_turn(mut a: Real, sweep: Real) -> Real {
    let tau = std::f64::consts::TAU;
    while sweep > 0.0 && a <= 0.0 {
        a += tau;
    }
    while sweep < 0.0 && a >= 0.0 {
        a -= tau;
    }
    while a.abs() > tau {
        a -= tau * a.signum();
    }
    a
}

/// A curve as pieces (a spline followed by short lines).
pub(super) fn pieces_of_curve(curve: &ProfileCurve) -> Vec<Piece> {
    match curve {
        ProfileCurve::Line { a, b } => vec![Piece::Line(*a, *b)],
        ProfileCurve::Arc { c, r, a0, sweep } => vec![Piece::Arc {
            c: *c,
            r: *r,
            a0: *a0,
            sweep: *sweep,
        }],
        spline => spline
            .sample(0.02)
            .windows(2)
            .map(|w| Piece::Line(w[0], w[1]))
            .collect(),
    }
}

/// A loop's curves as pieces.
pub(super) fn pieces_of(lp: &ProfileLoop) -> Vec<Piece> {
    lp.edges
        .iter()
        .flat_map(|e| pieces_of_curve(&e.curve))
        .collect()
}

/// Where the offset curves `a` (its end) and `b` (its start) join, near `at`.
fn join(a: &Piece, b: &Piece, at: Point2) -> Option<Point2> {
    match (*a, *b) {
        (Piece::Line(p, q), Piece::Line(r, s)) => {
            let (u, v) = (q - p, s - r);
            let cross = u.x * v.y - u.y * v.x;
            if cross.abs() < 1e-12 * u.norm() * v.norm() {
                return None;
            }
            let w = r - p;
            Some(p + u * ((w.x * v.y - w.y * v.x) / cross))
        }
        (Piece::Line(p, q), Piece::Arc { c, r, .. })
        | (Piece::Arc { c, r, .. }, Piece::Line(p, q)) => {
            let u = (q - p).try_normalize(1e-12)?;
            let w = p - c;
            let along = w.dot(&u);
            let disc = along * along - (w.norm_squared() - r * r);
            if disc < 0.0 {
                return None;
            }
            let root = disc.sqrt();
            let (x, y) = (p + u * (-along + root), p + u * (-along - root));
            Some(if (x - at).norm() < (y - at).norm() {
                x
            } else {
                y
            })
        }
        (Piece::Arc { c: c1, r: r1, .. }, Piece::Arc { c: c2, r: r2, .. }) => {
            let d = c2 - c1;
            let l = d.norm();
            if l < 1e-12 {
                return None;
            }
            let x = (l * l + r1 * r1 - r2 * r2) / (2.0 * l);
            let h2 = r1 * r1 - x * x;
            if h2 < 0.0 {
                return None;
            }
            let (u, h) = (d / l, h2.sqrt());
            let n = Vector2::new(-u.y, u.x);
            let (p, q) = (c1 + u * x + n * h, c1 + u * x - n * h);
            Some(if (p - at).norm() < (q - at).norm() {
                p
            } else {
                q
            })
        }
    }
}

/// `pieces` offset by `d` to the left, joined again (mitres at sharp corners, tangent joins stay
/// tangent). A corner the offsets do not meet at gets a straight piece.
pub(super) fn offset(pieces: &[Piece], closed: bool, d: Real) -> Result<Vec<Piece>, String> {
    if d == 0.0 {
        return Ok(pieces.to_vec());
    }
    let mut out = Vec::with_capacity(pieces.len());
    for p in pieces {
        out.push(
            p.offset(d)
                .ok_or("the offset is larger than the radius of an arc")?,
        );
    }
    if out.len() == 1 && out[0].closed() {
        return Ok(out);
    }
    let n = out.len();
    let joints = if closed { n } else { n.saturating_sub(1) };
    let mut extra: Vec<(usize, Piece)> = Vec::new();
    for k in 0..joints {
        let m = (k + 1) % n;
        let (a, b) = (out[k], out[m]);
        if (a.end() - b.start()).norm() < 1e-9 {
            continue;
        }
        match join(&a, &b, pieces[k].end()) {
            Some(q) if (q - pieces[k].end()).norm() < 50.0 * d.abs().max(1e-3) => {
                out[k] = a.with_end(q);
                out[m] = b.with_start(q);
            }
            _ => extra.push((k + 1, Piece::Line(a.end(), b.start()))),
        }
    }
    for (at, p) in extra.into_iter().rev() {
        out.insert(at, p);
    }
    Ok(out)
}

/// The signed area of a closed chain (counter-clockwise positive).
pub(super) fn area(pieces: &[Piece]) -> Real {
    let poly = polygon(pieces);
    (0..poly.len())
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            a.x * b.y - b.x * a.y
        })
        .sum::<Real>()
        / 2.0
}

/// The chain as points (its last point left out: closed use).
pub(super) fn polygon(pieces: &[Piece]) -> Vec<Point2> {
    let mut out = Vec::new();
    for p in pieces {
        match *p {
            Piece::Line(a, _) => out.push(a),
            Piece::Arc { c, r, a0, sweep } => {
                let n = ((sweep.abs() / 0.02).ceil() as usize).max(2);
                out.extend((0..n).map(|k| {
                    let t = a0 + sweep * k as Real / n as Real;
                    c + Vector2::new(t.cos(), t.sin()) * r
                }));
            }
        }
    }
    out
}

/// `pieces` run backwards.
pub(super) fn reversed(pieces: &[Piece]) -> Vec<Piece> {
    pieces.iter().rev().map(Piece::reversed).collect()
}

/// The closed outline of the band beside an open chain, between the offsets `d0` and `d1`.
pub(super) fn open_band(chain: &[Piece], d0: Real, d1: Real) -> Result<Vec<Piece>, String> {
    let (a, b) = (offset(chain, false, d0)?, offset(chain, false, d1)?);
    let mut out = a.clone();
    let (Some(first), Some(last)) = (a.first(), a.last()) else {
        return Err("the path is empty".into());
    };
    let (Some(bf), Some(bl)) = (b.first(), b.last()) else {
        return Err("the path is empty".into());
    };
    out.push(Piece::Line(last.end(), bl.end()));
    out.extend(reversed(&b));
    out.push(Piece::Line(bf.start(), first.start()));
    Ok(out)
}

/// Prism section segments of a closed chain, counter-clockwise when `ccw`.
fn section(pieces: &[Piece], ccw: bool) -> Vec<PrismSeg> {
    let flip = (area(pieces) > 0.0) != ccw;
    let chain = if flip {
        reversed(pieces)
    } else {
        pieces.to_vec()
    };
    chain.iter().flat_map(Piece::prism).collect()
}

/// A prism between two parallel levels (`z0`, `z1` along `plane`'s normal) whose outline changes
/// from `at0` to `at1`, with holes likewise: pieces matched one to one, so drafted walls come out
/// flat or conical, no Boolean.
pub(super) fn prism(
    plane: &Plane,
    (z0, at0): (Real, &[Piece]),
    (z1, at1): (Real, &[Piece]),
    holes: &[(Vec<Piece>, Vec<Piece>)],
    prefix: &str,
) -> Result<ExactSolid, Fail> {
    let n = plane.normal.into_inner();
    // The kernel's prism runs along the normal of its planes.
    let (lo, hi, a, b, swap) = if z1 >= z0 {
        (z0, z1, at0, at1, false)
    } else {
        (z1, z0, at1, at0, true)
    };
    let _ = swap;
    let level = |z: Real| Plane {
        origin: plane.origin + n * z,
        normal: plane.normal,
        x_axis: plane.x_axis,
    };
    // The outline's direction decides for both ends.
    let ccw = area(a) >= 0.0;
    let turned = |p: &[Piece], want: bool| -> Vec<PrismSeg> {
        let chain = if (area(a) >= 0.0) == want {
            p.to_vec()
        } else {
            reversed(p)
        };
        chain.iter().flat_map(Piece::prism).collect()
    };
    let _ = ccw;
    let mut loops = vec![(turned(a, true), turned(b, true))];
    for (h0, h1) in holes {
        let (x, y) = if z1 >= z0 { (h0, h1) } else { (h1, h0) };
        let cw = area(x) <= 0.0;
        let seg = |p: &[Piece]| -> Vec<PrismSeg> {
            let chain = if cw { p.to_vec() } else { reversed(p) };
            chain.iter().flat_map(Piece::prism).collect()
        };
        loops.push((seg(x), seg(y)));
    }
    let _ = section;
    exact::prism_between(&loops, &level(lo), &level(hi), prefix).map_err(Fail::Kernel)
}

/// The material of the wall the ray from far outside along `d` first meets at `p`: `(enter, exit)`
/// as distances along `d` from `p` (negative = behind `p`). `None`: no wall there.
pub(super) fn wall_at(rays: &Rays, p: Point3, d: Vector3) -> Option<(Real, Real)> {
    let far = rays.size + 1.0;
    rays.entry(p - d * far, d).map(|(a, b)| (a - far, b - far))
}

/// A plane along the normal of `plane`, through points given as `(u, v, height)`: least squares.
/// `None`: the points do not span the plane.
pub(super) fn fit_heights(pts: &[(Real, Real, Real)]) -> Option<(Real, Real, Real)> {
    let n = pts.len() as Real;
    let m = nalgebra::Matrix3::from_fn(|r, c| {
        let f = |p: &(Real, Real, Real), i: usize| match i {
            0 => 1.0,
            1 => p.0,
            _ => p.1,
        };
        pts.iter().map(|p| f(p, r) * f(p, c)).sum::<Real>() / n
    });
    let v = nalgebra::Vector3::new(
        pts.iter().map(|p| p.2).sum::<Real>() / n,
        pts.iter().map(|p| p.0 * p.2).sum::<Real>() / n,
        pts.iter().map(|p| p.1 * p.2).sum::<Real>() / n,
    );
    let s = m.lu().solve(&v)?;
    Some((s[0], s[1], s[2]))
}

/// Rounds the edges `pick` gives a radius to: `pick(face a, face b, its point)`, on the solids
/// `out`. Edges that bend smoothly (a seam, a tangent edge) are never offered.
pub(in crate::part) fn fillet_where(
    out: Vec<ExactSolid>,
    prefix: &str,
    mut pick: impl FnMut(&crate::kernel::BRep, crate::kernel::EdgeId, &str, &str) -> Option<Real>,
) -> Result<Vec<ExactSolid>, Fail> {
    use crate::part::blend::edge_ref;
    use crate::part::select::edge_convex;
    let body = exact::to_brep(&out).map_err(Fail::Kernel)?;
    let mut chosen: Vec<(crate::part::EdgeRef, Real)> = Vec::new();
    for e in body.visible_edges() {
        let fs = body.edge_faces(e);
        let [f1, f2] = fs[..] else {
            continue;
        };
        let (Some(a), Some(b)) = (
            body.face(f1).and_then(|x| x.name.clone()),
            body.face(f2).and_then(|x| x.name.clone()),
        ) else {
            continue;
        };
        let Some(r) = pick(&body, e, &a, &b) else {
            continue;
        };
        if edge_convex(&body, e).is_none() {
            continue;
        }
        if let Some(rf) = edge_ref(&body, e) {
            chosen.push((rf, r));
        }
    }
    apply_fillets(out, &body, &chosen, prefix)
}

/// The kernel's fillet of chosen edges, each with its radius, solid by solid.
pub(in crate::part) fn apply_fillets(
    mut out: Vec<ExactSolid>,
    body: &crate::kernel::BRep,
    chosen: &[(crate::part::EdgeRef, Real)],
    prefix: &str,
) -> Result<Vec<ExactSolid>, Fail> {
    if chosen.is_empty() {
        return Ok(out);
    }
    let mut picker = EdgePoints::new(body, &out);
    let mut pts: Vec<Vec<Point3>> = vec![Vec::new(); out.len()];
    let mut radii: Vec<Vec<Real>> = vec![Vec::new(); out.len()];
    for (e, r) in chosen {
        for (i, p) in picker.points(std::slice::from_ref(e)).map_err(Fail::User)? {
            pts[i].push(p);
            radii[i].push(*r);
        }
    }
    for i in 0..out.len() {
        if pts[i].is_empty() {
            continue;
        }
        out[i] = exact::fillet_each(&out[i], &pts[i], &radii[i], prefix)
            .map_err(|e| Fail::User(format!("the fillet cannot be built ({e})")))?;
    }
    Ok(out)
}
