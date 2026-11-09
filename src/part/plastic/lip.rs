//! Lip and Groove (`docs/spec/5.8-plastic-parts.md`) on a flat rim. The path's edges lie in the
//! rim's plane; the bead or the groove runs beside them, on the rim's side, as one prism between
//! two levels (its drafts are the difference of the two sections).

use serde::{Deserialize, Serialize};

use super::common::{self, Piece};
use super::is_zero;
use crate::kernel::exact::{ExactSolid, PathPiece};
use crate::kernel::{BRep, Plane, Point3, Real, Surface, Vector3};
use crate::part::EdgeRef;
use crate::part::boolean::Operation;
use crate::part::exact::{Fail, combine};
use crate::part::placement::WorkGeom;
use crate::part::rib::Rays;
use crate::part::shell::FaceRef;
use crate::part::sweep::PathRef;
use crate::part::work::LineRef;
use crate::sketch::{Point2, Vector2};

/// Lip or Groove.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LipKind {
    /// A bead standing on the rim.
    #[default]
    Lip,
    /// A channel in the rim.
    Groove,
}

/// Lip.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LipOp {
    /// Lip or Groove.
    #[serde(default)]
    #[serde(rename = "style")]
    pub kind: LipKind,
    /// The path's edges.
    pub path: Vec<EdgeRef>,
    /// The guide face (a wall next to the path).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guide: Option<FaceRef>,
    /// The pull direction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pull: Option<LineRef>,
    /// Target bodies; empty = whichever holds the path.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub solids: Vec<String>,
    /// Height of the bead, depth of the channel.
    pub height: Real,
    /// Its width.
    pub width: Real,
    /// From the path to its nearer wall.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub shoulder: Real,
    /// Lip: how far the rim beyond the bead is lowered. Groove: how much deeper it goes.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub clearance: Real,
    /// Draft of the wall nearer the path (radians).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub inside_angle: Real,
    /// Draft of the wall farther from the path (radians).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub outside_angle: Real,
}

impl LipOp {
    /// A lip along `path`.
    pub fn new(path: Vec<EdgeRef>, height: Real, width: Real) -> Self {
        Self {
            kind: LipKind::Lip,
            path,
            guide: None,
            pull: None,
            solids: Vec::new(),
            height,
            width,
            shoulder: 0.0,
            clearance: 0.0,
            inside_angle: 0.0,
            outside_angle: 0.0,
        }
    }

    /// Refuses a definition that cannot be built (before any geometry).
    pub fn validate(&self) -> Result<(), String> {
        if self.path.is_empty() {
            return Err("select the path edges".into());
        }
        if self.height <= 0.0 || self.width <= 0.0 {
            return Err("the height and the width must be positive".into());
        }
        if self.shoulder < 0.0 || self.clearance < 0.0 {
            return Err("the shoulder and the clearance cannot be negative".into());
        }
        // The drafts must leave a top on the bead.
        let top = self.width - self.height * (self.inside_angle.tan() + self.outside_angle.tan());
        if top <= 1e-6 {
            return Err("the drafts leave no top on the bead".into());
        }
        Ok(())
    }

    /// The browser's tooltip.
    pub fn describe(&self) -> String {
        let kind = match self.kind {
            LipKind::Lip => "Lip",
            LipKind::Groove => "Groove",
        };
        format!("{kind} {} × {} mm", self.width, self.height)
    }
}

/// The rim: its plane with the normal pointing out of the material.
fn rim_plane(op: &LipOp, body: &BRep, works: &WorkGeom) -> Result<Plane, Fail> {
    let user = |m: &str| Fail::User(m.to_owned());
    let e = op
        .path
        .first()
        .ok_or_else(|| user("select the path edges"))?;
    let pull = match &op.pull {
        Some(l) => Some(
            l.resolve(body, works)
                .map_err(|e| Fail::User(format!("the pull direction: {e}")))?
                .1,
        ),
        None => None,
    };
    let guide = op.guide.as_ref().map(|g| g.face.as_str());
    // Neither given: the rim is the plane every edge of the path lies on: the face they share, or
    // for edges sharing none (one straight edge of two) the planar one facing most upwards.
    let mut names: Vec<&String> = e.faces.iter().collect();
    if op.guide.is_none() && op.pull.is_none() {
        let up = |name: &String| {
            body.find_face(name)
                .and_then(|id| body.face(id))
                .and_then(|f| body.surface(f.surface))
                .map_or(-2.0, |s| match s {
                    Surface::Plane(p) => p.normal.z.abs(),
                    _ => -1.0,
                })
        };
        let shared = |name: &String| op.path.iter().all(|x| x.faces.contains(name));
        names.sort_by(|a, b| {
            shared(b)
                .cmp(&shared(a))
                .then_with(|| up(b).total_cmp(&up(a)))
        });
    }
    for name in names {
        if guide == Some(name.as_str()) {
            continue;
        }
        let Some(f) = body.find_face(name).and_then(|id| body.face(id)) else {
            continue;
        };
        let Some(Surface::Plane(p)) = body.surface(f.surface) else {
            continue;
        };
        if pull.is_some_and(|d| p.normal.dot(&d).abs() < 1.0 - 1e-6) {
            continue;
        }
        let out = if f.sense == crate::kernel::Sense::Same {
            p.normal.into_inner()
        } else {
            -p.normal.into_inner()
        };
        return Plane::new(p.origin, out, p.x_axis.into_inner())
            .map_err(|e| Fail::Kernel(e.to_string()));
    }
    Err(user(
        "only a flat rim so far: one of the path's faces must be planar and square to the pull",
    ))
}

/// How far a fitted arc may leave the points of a spline edge (mm).
const SPLINE_FIT: Real = 2e-3;

/// How far a bead or a groove stays off a spline edge it would be flush with (mm): arcs fitted to
/// the edge cross it to and fro by up to [`SPLINE_FIT`], and walls that close broke the Boolean.
const SPLINE_CLEAR: Real = 0.01;

/// The arc leaving `p` along the unit `t` and ending at `q`; a line when it is straight.
fn arc_from(p: Point2, t: Vector2, q: Point2) -> Piece {
    let n = Vector2::new(-t.y, t.x);
    let w = q - p;
    let bend = 2.0 * n.dot(&w);
    if bend.abs() < 1e-9 * w.norm_squared().max(1e-12) {
        return Piece::Line(p, q);
    }
    let along = w.norm_squared() / bend;
    // Nearly straight: a line is as close, and the kernel refused the NURBS form of a cylinder
    // that wide and that narrow.
    if w.norm_squared() / (8.0 * along.abs()) < SPLINE_FIT / 10.0 {
        return Piece::Line(p, q);
    }
    let c = p + n * along;
    let (a, b) = (p - c, q - c);
    let mut sweep = (a.x * b.y - a.y * b.x).atan2(a.dot(&b));
    if along > 0.0 && sweep < 0.0 {
        sweep += std::f64::consts::TAU;
    } else if along < 0.0 && sweep > 0.0 {
        sweep -= std::f64::consts::TAU;
    }
    Piece::Arc {
        c,
        r: along.abs(),
        a0: a.y.atan2(a.x),
        sweep,
    }
}

/// Two tangent arcs from `p0` (unit tangent `t0`) to `p1` (`t1`), with equal tangent lengths.
fn biarc(p0: Point2, t0: Vector2, p1: Point2, t1: Vector2) -> [Piece; 2] {
    let v = p1 - p0;
    let vt = v.dot(&(t0 + t1));
    let a = 2.0 * (1.0 - t0.dot(&t1));
    let root = vt + (vt * vt + a * v.norm_squared()).max(0.0).sqrt();
    if root.abs() < 1e-12 {
        let m = p0 + v / 2.0;
        return [Piece::Line(p0, m), Piece::Line(m, p1)];
    }
    let d = v.norm_squared() / root;
    let (q0, q1) = (p0 + t0 * d, p1 - t1 * d);
    let m = q0 + (q1 - q0) / 2.0;
    let tm = (q1 - q0).try_normalize(1e-12).unwrap_or(t0);
    [arc_from(p0, t0, m), arc_from(m, tm, p1)]
}

/// Distance from `q` to a piece.
fn distance(piece: &Piece, q: Point2) -> Real {
    match *piece {
        Piece::Line(a, b) => {
            let d = b - a;
            let s = ((q - a).dot(&d) / d.norm_squared().max(1e-18)).clamp(0.0, 1.0);
            (a + d * s - q).norm()
        }
        Piece::Arc { c, r, a0, sweep } => {
            let v = q - c;
            let along = ((v.y.atan2(v.x) - a0) * sweep.signum()).rem_euclid(std::f64::consts::TAU);
            if along <= sweep.abs() {
                (v.norm() - r).abs()
            } else {
                let at = |t: Real| c + Vector2::new(t.cos(), t.sin()) * r;
                (at(a0) - q).norm().min((at(a0 + sweep) - q).norm())
            }
        }
    }
}

/// Tangent arcs through the points of a curve, within [`SPLINE_FIT`]. A polyline made every sample
/// a wall of its own (minutes in the Boolean, or no result); arcs offset exactly.
fn fitted_arcs(points: &[Point2]) -> Vec<Piece> {
    let n = points.len();
    if n < 3 {
        return points.windows(2).map(|w| Piece::Line(w[0], w[1])).collect();
    }
    let unit = |v: Vector2| {
        v.try_normalize(1e-12)
            .unwrap_or_else(|| Vector2::new(1.0, 0.0))
    };
    let tangents: Vec<Vector2> = (0..n)
        .map(|k| {
            unit(if k == 0 {
                (points[1] - points[0]) * 4.0 - (points[2] - points[0])
            } else if k + 1 == n {
                (points[n - 1] - points[n - 2]) * 4.0 - (points[n - 1] - points[n - 3])
            } else {
                points[k + 1] - points[k - 1]
            })
        })
        .collect();
    let mut out = Vec::new();
    // Spans still to fit, the first on top so the pieces come out in order.
    let mut todo = vec![(0, n - 1)];
    while let Some((i, j)) = todo.pop() {
        let pair = biarc(points[i], tangents[i], points[j], tangents[j]);
        let off = points[i + 1..j]
            .iter()
            .map(|q| distance(&pair[0], *q).min(distance(&pair[1], *q)))
            .fold(0.0, Real::max);
        if off > SPLINE_FIT && j - i > 1 {
            let m = (i + j) / 2;
            todo.push((m, j));
            todo.push((i, m));
        } else {
            out.extend(pair);
        }
    }
    out
}

/// The path's pieces in the rim's plane, joined in order, whether it is closed, and whether a
/// curve in it is fitted (a spline edge).
fn path_pieces(op: &LipOp, body: &BRep, plane: &Plane) -> Result<(Vec<Piece>, bool, bool), Fail> {
    let user = |m: String| Fail::User(m);
    let near = Point3::from(op.path[0].hint);
    let geom = PathRef::Edges(op.path.clone())
        .resolve(&[], body, near)
        .map_err(user)?;
    let n = plane.normal.into_inner();
    let flat = |q: &Point3| (q - plane.origin).dot(&n).abs() < 1e-6;
    let mut out = Vec::with_capacity(geom.pieces.len());
    let mut fitted = false;
    for p in &geom.pieces {
        match p {
            PathPiece::Line { a, b } => {
                if !(flat(a) && flat(b)) {
                    return Err(user("the path must lie in the rim's plane".into()));
                }
                out.push(Piece::Line(plane.project(a), plane.project(b)));
            }
            PathPiece::Arc {
                a,
                center,
                axis,
                angle,
            } => {
                if !flat(a) || axis.dot(&n).abs() < 1.0 - 1e-6 {
                    return Err(user("the path must lie in the rim's plane".into()));
                }
                let c = plane.project(center);
                let v: Vector2 = plane.project(a) - c;
                out.push(Piece::Arc {
                    c,
                    r: v.norm(),
                    a0: v.y.atan2(v.x),
                    sweep: *angle * axis.dot(&n).signum(),
                });
            }
            PathPiece::Points(pts) | PathPiece::Spline { points: pts, .. } => {
                if !pts.iter().all(flat) {
                    return Err(user("the path must lie in the rim's plane".into()));
                }
                let flat_pts: Vec<Point2> = pts.iter().map(|q| plane.project(q)).collect();
                out.extend(fitted_arcs(&flat_pts));
                fitted = true;
            }
        }
    }
    Ok((out, geom.closed, fitted))
}

/// Which way from the path the rim lies (+1: left of the chain) and how wide it is there.
fn rim_side(
    rays: &Rays,
    plane: &Plane,
    chain: &[Piece],
    clear: Real,
) -> Result<(Real, Real), Fail> {
    let first = chain
        .first()
        .ok_or_else(|| Fail::User("select the path edges".into()))?;
    // A point half way along the first piece, and the left of its direction there.
    let (q, t): (Point2, Vector2) = match *first {
        Piece::Line(a, b) => (a + (b - a) / 2.0, (b - a).normalize()),
        Piece::Arc { c, r, a0, sweep } => {
            let m = a0 + sweep / 2.0;
            let s = sweep.signum();
            (
                c + Vector2::new(m.cos(), m.sin()) * r,
                Vector2::new(-m.sin() * s, m.cos() * s),
            )
        }
    };
    let left = Vector2::new(-t.y, t.x);
    let down = -plane.normal.into_inner();
    let world = |v: Vector2| plane.x_axis.into_inner() * v.x + plane.y_axis().into_inner() * v.y;
    let probe = plane.point_at(q.x, q.y) + down * 1e-3;
    for sign in [1.0, -1.0] {
        let dir: Vector3 = world(left * sign);
        let from = probe + dir * clear;
        if let Some((0.0, exit)) = rays.entry(from, dir) {
            return Ok((sign, exit));
        }
    }
    Err(Fail::User(
        "the path's edge has no flat rim beside it".into(),
    ))
}

/// Builds Lip / Groove (`cur`: the solids before it; surface bodies stay outside).
pub(in crate::part) fn lip(
    op: &LipOp,
    id: &str,
    body: &BRep,
    works: &WorkGeom,
    cur: &[ExactSolid],
) -> Result<Vec<ExactSolid>, Fail> {
    let user = |m: String| Fail::User(m);
    let plane = rim_plane(op, body, works)?;
    let rays = Rays::of(body, &op.solids);
    // A face's sense can be inside-out: the material under the rim decides which way is out.
    // Arcs fitted to a spline edge pass it on either side: the probe starts past that.
    let clear = |fitted: bool| if fitted { SPLINE_CLEAR } else { 1e-4 };
    let (plane, chain, closed, fitted, side, rim) = {
        let (chain, closed, fitted) = path_pieces(op, body, &plane)?;
        match rim_side(&rays, &plane, &chain, clear(fitted)) {
            Ok((side, rim)) => (plane, chain, closed, fitted, side, rim),
            Err(first) => {
                let out = -plane.normal.into_inner();
                let turned = Plane::new(plane.origin, out, plane.x_axis.into_inner())
                    .map_err(|e| Fail::Kernel(e.to_string()))?;
                let (chain, closed, fitted) = path_pieces(op, body, &turned)?;
                let (side, rim) =
                    rim_side(&rays, &turned, &chain, clear(fitted)).map_err(|_| first)?;
                (turned, chain, closed, fitted, side, rim)
            }
        }
    };
    // A wall flush with a spline edge moves off it: a bead stays on the rim, a groove's opening
    // reaches over the edge into the air. The far wall stays where it was asked for.
    let s = match op.kind {
        LipKind::Lip if fitted => op.shoulder.max(SPLINE_CLEAR),
        LipKind::Groove if fitted && op.shoulder < SPLINE_CLEAR => -SPLINE_CLEAR,
        _ => op.shoulder,
    };
    let w = op.width - (s - op.shoulder);
    let (ti, to) = (op.inside_angle.tan(), op.outside_angle.tan());
    // A band between two offsets (from the path, into the rim), outline and holes.
    let band = |d0: Real, d1: Real| -> Result<(Vec<Piece>, Vec<Vec<Piece>>), String> {
        if closed {
            let (a, b) = (
                common::offset(&chain, true, d0 * side)?,
                common::offset(&chain, true, d1 * side)?,
            );
            Ok(if common::area(&a).abs() >= common::area(&b).abs() {
                (a, vec![b])
            } else {
                (b, vec![a])
            })
        } else {
            Ok((common::open_band(&chain, d0 * side, d1 * side)?, Vec::new()))
        }
    };
    // Between levels z0 and z1 above the rim: the band's two offsets at each level.
    let solid = |(z0, d00, d01): (Real, Real, Real),
                 (z1, d10, d11): (Real, Real, Real),
                 prefix: String|
     -> Result<ExactSolid, Fail> {
        let (o0, h0) = band(d00, d01).map_err(user)?;
        let (o1, h1) = band(d10, d11).map_err(user)?;
        let holes: Vec<(Vec<Piece>, Vec<Piece>)> = h0.into_iter().zip(h1).collect();
        let build = if fitted {
            common::prism_smooth
        } else {
            common::prism
        };
        build(&plane, (z0, &o0), (z1, &o1), &holes, &prefix)
    };
    let h = op.height;
    match op.kind {
        LipKind::Lip => {
            // The bead's foot goes a hair into the rim, along the same drafted walls. How deep
            // shows nowhere, but with a wall flush with the body's own the kernel's intersection
            // gave up at some depths (a 0.01 foot on a rounded cover, status 1001).
            let e = (h * 0.01).clamp(1e-3, 0.05);
            let join = |foot: Real| -> Result<Vec<ExactSolid>, Fail> {
                let tool = solid(
                    (-foot, s - foot * ti, s + w + foot * to),
                    (h, s + h * ti, s + w - h * to),
                    format!("{id}:Bead"),
                )?;
                combine(cur.to_vec(), vec![tool], Operation::Join, &op.solids, false)
            };
            let mut joined = join(e);
            for f in [0.6, 1.7, 3.1] {
                if !matches!(joined, Err(Fail::Kernel(_))) {
                    break;
                }
                joined = join(e * f);
            }
            let mut out = joined?;
            if op.clearance > 0.0 {
                let air = 1.0;
                let reach = s + w + (rim - (s + w)).max(0.0) + 1.0;
                let cut = solid(
                    (-op.clearance, s + w, reach),
                    (air, s + w, reach),
                    format!("{id}:Clearance"),
                )?;
                out = combine(out, vec![cut], Operation::Cut, &op.solids, false)?;
            }
            Ok(out)
        }
        LipKind::Groove => {
            let depth = h + op.clearance;
            let air = (h * 0.5).clamp(0.05, 1.0);
            // Walls widen towards the opening.
            let tool = solid(
                (-depth, s, s + w),
                (air, s - (air + depth) * ti, s + w + (air + depth) * to),
                format!("{id}:Groove"),
            )?;
            combine(cur.to_vec(), vec![tool], Operation::Cut, &op.solids, false)
        }
    }
}
