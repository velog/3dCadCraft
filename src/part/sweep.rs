//! The **Sweep** feature (Inventor: 3D Model → Create → Sweep), see
//! `docs/spec/5.1-sweep.md`.
//!
//! Profiles of one sketch move along a path: a chain of curves of another
//! sketch (on any plane) or a chain of model edges. A plain sweep (the
//! profile at a fixed angle to the path) is the exact kernel's own sweep;
//! taper, twist, a guide rail and Parallel orientation loft placed copies of
//! the profile along the path ([`crate::kernel::exact::loft_frames`]).
//!
//! Paths are also used by Loft (rails, centre line): [`PathRef`].

use serde::{Deserialize, Serialize};

use crate::kernel::exact::{self as kx, ExactSolid, Frame, PathPiece};
use crate::kernel::{BRep, Curve, Plane, Point3, Real, TessellationOptions, Vector3};
use crate::sketch::{EntityId, ProfileCurve};
use nalgebra::Matrix3;

use super::PartSketch;
use super::blend::EdgeRef;
use super::exact::Fail;
use super::expr::Unit;
use super::extrude::{ProfileRef, kernel_profile};
use super::params::FeatureParams;
use super::shell::FaceRef;
use super::{Operation, Output};

/// A path: curves of a sketch or model edges, chained end to end.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PathRef {
    /// Curves of a 2D sketch (by entity id; any order — they are chained).
    Sketch {
        /// `id` of the sketch.
        sketch: String,
        /// Its curves.
        curves: Vec<EntityId>,
    },
    /// Model edges (each by its two faces and a point on it).
    Edges(Vec<EdgeRef>),
}

impl PathRef {
    /// The sketch it uses, if any.
    pub fn sketch_id(&self) -> Option<&str> {
        match self {
            PathRef::Sketch { sketch, .. } => Some(sketch),
            PathRef::Edges(_) => None,
        }
    }

    /// Whether it names nothing.
    pub fn is_empty(&self) -> bool {
        match self {
            PathRef::Sketch { curves, .. } => curves.is_empty(),
            PathRef::Edges(e) => e.is_empty(),
        }
    }

    /// The path in model space, chained from the end nearest `near`.
    pub fn resolve(
        &self,
        sketches: &[PartSketch],
        body: &BRep,
        near: Point3,
    ) -> Result<PathGeom, String> {
        let pieces = match self {
            PathRef::Sketch { sketch, curves } => {
                let ps = sketches
                    .iter()
                    .find(|s| &s.id == sketch)
                    .ok_or_else(|| format!("the path sketch {sketch} is missing"))?;
                if curves.is_empty() {
                    return Err("select the path".into());
                }
                curves
                    .iter()
                    .map(|&c| {
                        let curve = ps
                            .sketch
                            .curve_of(c)
                            .ok_or("a path curve no longer exists in its sketch")?;
                        Ok(sketch_piece(&ps.sketch.plane, &curve))
                    })
                    .collect::<Result<Vec<_>, String>>()?
            }
            PathRef::Edges(refs) => {
                if refs.is_empty() {
                    return Err("select the path".into());
                }
                refs.iter()
                    .map(|r| edge_piece(body, r))
                    .collect::<Result<Vec<_>, _>>()?
            }
        };
        PathGeom::chained(pieces, near)
    }
}

/// A sketch curve as a path piece in model space.
fn sketch_piece(plane: &Plane, c: &ProfileCurve) -> PathPiece {
    let w = |p: crate::sketch::Point2| plane.point_at(p.x, p.y);
    match c {
        ProfileCurve::Line { a, b } => PathPiece::Line { a: w(*a), b: w(*b) },
        ProfileCurve::Arc {
            c: center, sweep, ..
        } => PathPiece::Arc {
            a: w(c.point(0.0)),
            center: w(*center),
            axis: plane.normal.into_inner(),
            angle: *sweep,
        },
        ProfileCurve::Spline { .. } => {
            PathPiece::Points(c.sample(1e-3).into_iter().map(w).collect())
        }
    }
}

/// A model edge as a path piece: lines and circles exactly, other curves
/// by their points.
fn edge_piece(body: &BRep, r: &EdgeRef) -> Result<PathPiece, String> {
    let named = |n: &str| -> Vec<crate::kernel::FaceId> {
        body.faces()
            .filter(|(_, f)| f.name.as_deref() == Some(n))
            .map(|(id, _)| id)
            .collect()
    };
    let (fa, fb) = (named(&r.faces[0]), named(&r.faces[1]));
    let hint = Point3::from(r.hint);
    let opts = TessellationOptions::default();
    let mut best: Option<(Real, crate::kernel::EdgeId, Vec<Point3>)> = None;
    for (e, _) in body.edges() {
        let fs = body.edge_faces(e);
        if !(fs.iter().any(|f| fa.contains(f)) && fs.iter().any(|f| fb.contains(f))) {
            continue;
        }
        let Ok(pts) = body.sample_edge(e, &opts) else {
            continue;
        };
        let d = pts
            .windows(2)
            .map(|w| seg_dist(&w[0], &w[1], &hint))
            .fold(Real::INFINITY, Real::min)
            .min(pts.first().map_or(Real::INFINITY, |p| (p - hint).norm()));
        if best.as_ref().is_none_or(|b| d < b.0) {
            best = Some((d, e, pts));
        }
    }
    let (_, e, pts) = best.ok_or("a path edge no longer exists")?;
    let (Some(&a), Some(&b)) = (pts.first(), pts.last()) else {
        return Err("a path edge has no points".into());
    };
    let curve = body.edge(e).and_then(|x| body.curve(x.curve));
    Ok(match curve {
        Some(Curve::Line(_)) => PathPiece::Line { a, b },
        Some(Curve::Circle(c)) => {
            let n = c.normal.into_inner();
            let angle: Real = pts
                .windows(2)
                .map(|w| {
                    let (p, q) = (w[0] - c.center, w[1] - c.center);
                    p.cross(&q).dot(&n).atan2(p.dot(&q))
                })
                .sum();
            PathPiece::Arc {
                a,
                center: c.center,
                axis: n,
                angle,
            }
        }
        _ => PathPiece::Points(pts),
    })
}

fn seg_dist(p: &Point3, q: &Point3, x: &Point3) -> Real {
    let d = q - p;
    let l2 = d.norm_squared();
    let s = if l2 > 0.0 {
        ((x - p).dot(&d) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (p + d * s - x).norm()
}

/// A path in model space: pieces head to tail.
#[derive(Clone, Debug, PartialEq)]
pub struct PathGeom {
    /// The pieces, in order.
    pub pieces: Vec<PathPiece>,
    /// Its end meets its start.
    pub closed: bool,
}

fn ends(p: &PathPiece) -> (Point3, Point3) {
    match p {
        PathPiece::Line { a, b } => (*a, *b),
        PathPiece::Arc {
            a,
            center,
            axis,
            angle,
        } => (*a, rotate(*a, *center, *axis, *angle)),
        PathPiece::Points(v) => (
            v.first().copied().unwrap_or_else(Point3::origin),
            v.last().copied().unwrap_or_else(Point3::origin),
        ),
    }
}

fn rotate(p: Point3, c: Point3, axis: Vector3, angle: Real) -> Point3 {
    let k = nalgebra::Unit::new_normalize(axis);
    c + nalgebra::Rotation3::from_axis_angle(&k, angle) * (p - c)
}

fn reversed(p: &PathPiece) -> PathPiece {
    match p {
        PathPiece::Line { a, b } => PathPiece::Line { a: *b, b: *a },
        PathPiece::Arc {
            a,
            center,
            axis,
            angle,
        } => PathPiece::Arc {
            a: rotate(*a, *center, *axis, *angle),
            center: *center,
            axis: *axis,
            angle: -angle,
        },
        PathPiece::Points(v) => PathPiece::Points(v.iter().rev().copied().collect()),
    }
}

fn piece_len(p: &PathPiece) -> Real {
    match p {
        PathPiece::Line { a, b } => (b - a).norm(),
        PathPiece::Arc {
            a, center, angle, ..
        } => (a - center).norm() * angle.abs(),
        PathPiece::Points(v) => v.windows(2).map(|w| (w[1] - w[0]).norm()).sum(),
    }
}

/// Point and unit tangent at arc length `s` along the piece.
fn piece_eval(p: &PathPiece, s: Real) -> (Point3, Vector3) {
    match p {
        PathPiece::Line { a, b } => {
            let d = b - a;
            let l = d.norm().max(1e-300);
            (a + d * (s / l).clamp(0.0, 1.0), d / l)
        }
        PathPiece::Arc {
            a,
            center,
            axis,
            angle,
        } => {
            let r = (a - center).norm().max(1e-300);
            let t = (s / r).clamp(0.0, angle.abs()) * angle.signum();
            let q = rotate(*a, *center, *axis, t);
            let k = axis.normalize();
            let tan = (k.cross(&(q - center)) * angle.signum()).normalize();
            (q, tan)
        }
        PathPiece::Points(v) => {
            let mut acc = 0.0;
            for w in v.windows(2) {
                let l = (w[1] - w[0]).norm();
                if l < 1e-12 {
                    continue;
                }
                if acc + l >= s {
                    let d = (w[1] - w[0]) / l;
                    return (w[0] + d * (s - acc).max(0.0), d);
                }
                acc += l;
            }
            let n = v.len();
            let d = if n >= 2 {
                (v[n - 1] - v[n - 2])
                    .try_normalize(1e-300)
                    .unwrap_or_else(Vector3::x)
            } else {
                Vector3::x()
            };
            (v.last().copied().unwrap_or_else(Point3::origin), d)
        }
    }
}

impl PathGeom {
    /// Chains `pieces` head to tail, starting from the free end nearest
    /// `near`.
    pub fn chained(pieces: Vec<PathPiece>, near: Point3) -> Result<Self, String> {
        let scale = pieces
            .iter()
            .flat_map(|p| {
                let (a, b) = ends(p);
                [a.coords.amax(), b.coords.amax()]
            })
            .fold(1.0, Real::max);
        let tol = 1e-6 * scale;
        let n = pieces.len();
        let mut best: Option<(Real, Vec<PathPiece>)> = None;
        for first in 0..n {
            for flip in [false, true] {
                let p0 = if flip {
                    reversed(&pieces[first])
                } else {
                    pieces[first].clone()
                };
                let mut used = vec![false; n];
                used[first] = true;
                let mut chain = vec![p0];
                loop {
                    let end = chain
                        .last()
                        .map(|p| ends(p).1)
                        .unwrap_or_else(Point3::origin);
                    let next = (0..n).filter(|&j| !used[j]).find_map(|j| {
                        let (a, b) = ends(&pieces[j]);
                        if (a - end).norm() <= tol {
                            Some((j, pieces[j].clone()))
                        } else if (b - end).norm() <= tol {
                            Some((j, reversed(&pieces[j])))
                        } else {
                            None
                        }
                    });
                    match next {
                        Some((j, p)) => {
                            used[j] = true;
                            chain.push(p);
                        }
                        None => break,
                    }
                }
                if chain.len() != n {
                    continue;
                }
                let start = ends(&chain[0]).0;
                let d = (start - near).norm();
                if best.as_ref().is_none_or(|b| d < b.0 - tol) {
                    best = Some((d, chain));
                }
            }
        }
        let pieces = best
            .map(|b| b.1)
            .ok_or("the path curves are not connected end to end")?;
        let (Some(f), Some(l)) = (pieces.first(), pieces.last()) else {
            return Err("select the path".into());
        };
        let closed = n > 0 && (ends(f).0 - ends(l).1).norm() <= tol;
        Ok(Self { pieces, closed })
    }

    /// Total length.
    pub fn length(&self) -> Real {
        self.pieces.iter().map(piece_len).sum()
    }

    /// Start point.
    pub fn start(&self) -> Point3 {
        self.pieces
            .first()
            .map(|p| ends(p).0)
            .unwrap_or_else(Point3::origin)
    }

    /// Point and unit tangent at arc length `s`.
    pub fn eval(&self, s: Real) -> (Point3, Vector3) {
        let mut acc = 0.0;
        let n = self.pieces.len();
        for (i, p) in self.pieces.iter().enumerate() {
            let l = piece_len(p);
            if s <= acc + l || i + 1 == n {
                return piece_eval(p, s - acc);
            }
            acc += l;
        }
        (Point3::origin(), Vector3::x())
    }

    /// The largest turn between neighbouring pieces (radians): a corner.
    pub fn sharpest_joint(&self) -> Real {
        self.pieces
            .windows(2)
            .map(|w| {
                let (_, t0) = piece_eval(&w[0], piece_len(&w[0]));
                let (_, t1) = piece_eval(&w[1], 0.0);
                t0.dot(&t1).clamp(-1.0, 1.0).acos()
            })
            .fold(0.0, Real::max)
    }

    /// How far the tangent turns along the path (radians).
    pub fn total_turn(&self) -> Real {
        let n = 256;
        let l = self.length();
        (0..n)
            .map(|i| {
                let (_, a) = self.eval(l * i as Real / n as Real);
                let (_, b) = self.eval(l * (i + 1) as Real / n as Real);
                a.dot(&b).clamp(-1.0, 1.0).acos()
            })
            .sum()
    }

    /// Points along the path (for previews and rails), arcs within 0.01 mm.
    pub fn polyline(&self) -> Vec<Point3> {
        kx::path_points(&self.pieces)
            .into_iter()
            .map(|p| Point3::new(p.x, p.y, p.z))
            .collect()
    }

    /// Whether every piece is a line along one direction.
    fn straight(&self) -> bool {
        let dirs: Vec<Vector3> = self
            .pieces
            .iter()
            .map(|p| match p {
                PathPiece::Line { a, b } => (b - a).try_normalize(1e-12),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
            .unwrap_or_default();
        !dirs.is_empty() && dirs.iter().all(|d| d.dot(&dirs[0]) > 1.0 - 1e-9)
    }
}

/// The kind of sweep (Inventor's Type).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SweepType {
    /// Along the path only.
    #[default]
    Path,
    /// A guide rail sets the profile's turn and size.
    PathAndGuideRail,
    /// A guide surface sets the profile's turn about the path.
    PathAndGuideSurface,
}

/// How a guide rail scales the profile.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProfileScaling {
    /// Uniformly, by the distance path → rail.
    #[default]
    XY,
    /// Only toward the rail.
    X,
    /// Not at all (the rail only turns the profile).
    None,
}

/// How the profile is held along the path.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SweepOrientation {
    /// At a fixed angle to the path (turns with it).
    #[default]
    Path,
    /// Parallel to where it started.
    Parallel,
}

/// A sweep of sketch regions along a path.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SweepFeature {
    /// Immutable identity, the prefix of its topology names.
    pub id: String,
    /// Display name.
    pub name: String,
    /// `id` of the profile sketch.
    pub sketch: String,
    /// The chosen regions.
    pub profiles: Vec<ProfileRef>,
    /// The path.
    pub path: PathRef,
    /// Path, Path & Guide Rail or Path & Guide Surface.
    #[serde(default)]
    pub sweep_type: SweepType,
    /// The guide rail (Path & Guide Rail).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rail: Option<PathRef>,
    /// Profile Scaling with a guide rail.
    #[serde(default)]
    pub scaling: ProfileScaling,
    /// The guide surface (Path & Guide Surface).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guide_surface: Option<FaceRef>,
    /// Orientation (Type Path).
    #[serde(default)]
    pub orientation: SweepOrientation,
    /// Taper angle (radians; positive grows away from the start).
    #[serde(default)]
    pub taper: Real,
    /// Twist from start to end (radians).
    #[serde(default)]
    pub twist: Real,
    /// Solid Sweep: a body moves along the path instead of a profile.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub solid_sweep: bool,
    /// The toolbody of a Solid Sweep (body name).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toolbody: Option<String>,
    /// Keep Toolbody (Solid Sweep).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub keep_toolbody: bool,
    /// Boolean operation with the body.
    #[serde(default)]
    pub operation: Operation,
    /// Solid or surface.
    #[serde(default)]
    pub output: Output,
    /// Bodies the operation works on (by body name); empty = the default.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub solids: Vec<String>,
    /// Parameter names and equations (`taper`, `twist`).
    #[serde(default, skip_serializing_if = "FeatureParams::is_empty")]
    pub params: FeatureParams,
    /// The preset the feature was made with (name only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
}

impl SweepFeature {
    /// A sweep of `profiles` along `path` (id, name and sketch are set by
    /// [`super::PartDocument::add_sweep`]).
    pub fn new(profiles: Vec<ProfileRef>, path: PathRef) -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            sketch: String::new(),
            profiles,
            path,
            sweep_type: SweepType::Path,
            rail: None,
            scaling: ProfileScaling::XY,
            guide_surface: None,
            orientation: SweepOrientation::Path,
            taper: 0.0,
            twist: 0.0,
            solid_sweep: false,
            toolbody: None,
            keep_toolbody: false,
            operation: Operation::NewSolid,
            output: Output::Solid,
            solids: Vec::new(),
            params: FeatureParams::default(),
            preset: None,
        }
    }

    /// Every sketch it uses: the profile's, the path's and the rail's.
    pub fn sketch_ids(&self) -> Vec<&str> {
        let mut v = vec![self.sketch.as_str()];
        v.extend(self.path.sketch_id());
        if self.sweep_type == SweepType::PathAndGuideRail
            && let Some(r) = &self.rail
        {
            v.extend(r.sketch_id());
        }
        v.retain(|s| !s.is_empty());
        v.dedup();
        v
    }

    /// Whether taper and twist apply (Type Path, Orientation Path).
    pub fn tapers(&self) -> bool {
        self.sweep_type == SweepType::Path
            && self.orientation == SweepOrientation::Path
            && !self.solid_sweep
    }

    /// Parameter fields.
    pub fn fields(&self) -> Vec<(&'static str, Unit)> {
        if self.tapers() {
            vec![("taper", Unit::Deg), ("twist", Unit::Deg)]
        } else {
            Vec::new()
        }
    }

    /// A field's value.
    pub fn value(&self, field: &str) -> Option<Real> {
        match field {
            "taper" => Some(self.taper),
            "twist" => Some(self.twist),
            _ => None,
        }
    }

    /// Sets a field.
    pub fn set(&mut self, field: &str, v: Real) {
        match field {
            "taper" => self.taper = v,
            "twist" => self.twist = v,
            _ => {}
        }
    }

    /// The solids of the sweep (one per profile), before its operation.
    pub(super) fn build(
        &self,
        sketches: &[PartSketch],
        body: &BRep,
    ) -> Result<Vec<ExactSolid>, Fail> {
        use Fail::User;
        if self.solid_sweep {
            return Err(User(
                "Solid Sweep (a toolbody along a path) is not supported yet".into(),
            ));
        }
        if self.output == Output::Surface {
            return Err(User("Surface output is not supported yet".into()));
        }
        if self.sweep_type == SweepType::PathAndGuideSurface {
            return Err(User("Path & Guide Surface is not supported yet".into()));
        }
        let ps = sketches
            .iter()
            .find(|s| s.id == self.sketch)
            .ok_or_else(|| User(format!("sketch {} is missing", self.sketch)))?;
        if self.profiles.is_empty() {
            return Err(User("select a profile".into()));
        }
        let plane = &ps.sketch.plane;
        let regions = ps.sketch.regions();
        let mut chosen = Vec::new();
        for (n, pr) in self.profiles.iter().enumerate() {
            let region = pr.resolve(&regions).ok_or_else(|| {
                User(format!("profile #{} no longer exists in the sketch", n + 1))
            })?;
            chosen.push(region.clone());
        }
        let c = chosen[0].interior_point();
        let near = plane.point_at(c.x, c.y);
        let path = self.path.resolve(sketches, body, near).map_err(User)?;
        let len = path.length();
        if len < 1e-6 {
            return Err(User("the path has no length".into()));
        }
        let n = plane.normal.into_inner();
        let off = |p: &Point3| (p - plane.origin).dot(&n).abs();
        if path.polyline().iter().all(|p| off(p) < 1e-6) {
            return Err(User("the path lies in the profile plane".into()));
        }
        let rail = match (self.sweep_type, &self.rail) {
            (SweepType::PathAndGuideRail, Some(r)) => {
                Some(r.resolve(sketches, body, path.start()).map_err(User)?)
            }
            (SweepType::PathAndGuideRail, None) => {
                return Err(User("select the guide rail".into()));
            }
            _ => None,
        };
        let (taper, twist) = if self.tapers() {
            (self.taper, self.twist)
        } else {
            (0.0, 0.0)
        };
        if !(taper.is_finite() && taper.abs() < std::f64::consts::FRAC_PI_2 - 1e-3) {
            return Err(User("the taper must be between -90 and 90 deg".into()));
        }
        if !twist.is_finite() {
            return Err(User("the twist is not a number".into()));
        }
        let plain = rail.is_none()
            && taper.abs() < 1e-12
            && twist.abs() < 1e-12
            && (self.orientation == SweepOrientation::Path || path.straight());
        let mut out = Vec::new();
        for (k, region) in chosen.iter().enumerate() {
            let (outer, holes) = kernel_profile(region);
            let prof = kx::profile(&outer, &holes).map_err(User)?;
            if self.orientation == SweepOrientation::Path && rail.is_none() {
                kx::check_bend(plane, &prof, &path.pieces).map_err(User)?;
            }
            let prefix = super::exact::prefix(&self.id, k);
            let solid = if plain {
                kx::sweep(plane, &prof, &path.pieces, &prefix)
                    .map_err(|e| User(format!("the geometry kernel cannot build it ({e})")))?
            } else {
                if path.sharpest_joint() > 1e-3 {
                    return Err(User(
                        "a path with sharp corners can only be swept without taper, twist, a guide rail or Parallel orientation"
                            .into(),
                    ));
                }
                let area = region.outer.area().abs();
                let segs = outer.len().max(1);
                let round = outer
                    .iter()
                    .all(|s| matches!(s.geometry, crate::kernel::Segment2::Arc { .. }));
                let chunks = self
                    .frames(plane, &path, rail.as_ref(), area, taper, twist, segs, round)
                    .map_err(User)?;
                let mut acc: Option<ExactSolid> = None;
                for frames in &chunks {
                    let piece = kx::loft_frames(&prof, frames, &prefix)
                        .map_err(|e| User(format!("the geometry kernel cannot build it ({e})")))?;
                    acc = Some(match acc {
                        None => piece,
                        Some(a) => kx::boolean(&a, &piece, kx::ExactOp::Union)
                            .map_err(|e| {
                                User(format!("the geometry kernel cannot join its pieces ({e})"))
                            })?
                            .ok_or_else(|| User("the sweep is empty".into()))?,
                    });
                }
                acc.ok_or_else(|| User("the sweep is empty".into()))?
            };
            out.push(solid);
        }
        Ok(out)
    }

    /// Placed copies of the profile along the path, in chunks small enough
    /// for the kernel's loft to pair their vertices (it matches each section
    /// to the last one by direction, so a chunk turns or twists at most
    /// `limit`); neighbouring chunks share a section.
    #[allow(clippy::too_many_arguments)]
    fn frames(
        &self,
        plane: &Plane,
        path: &PathGeom,
        rail: Option<&PathGeom>,
        area: Real,
        taper: Real,
        twist: Real,
        segs: usize,
        round: bool,
    ) -> Result<Vec<Vec<Frame>>, String> {
        let len = path.length();
        let turn = path.total_turn();
        let deg = Real::to_radians;
        let mut n = ((turn / deg(12.0)).ceil() as usize)
            .max((twist.abs() / deg(8.0)).ceil() as usize)
            .max(6);
        if rail.is_some() {
            n = n.max(16);
        }
        let n = n.min(240);
        // Rotation-minimising frames (double reflection) on a finer grid.
        let fine = n * 8;
        let (p0, t0) = path.eval(0.0);
        let helper = if t0.x.abs() < 0.9 {
            Vector3::x()
        } else {
            Vector3::y()
        };
        let mut r = t0.cross(&helper).normalize();
        let mut rmf = vec![(p0, t0, r)];
        let (mut xp, mut tp) = (p0, t0);
        for i in 1..=fine {
            let (x, t) = path.eval(len * i as Real / fine as Real);
            let v1 = x - xp;
            let c1 = v1.dot(&v1);
            let (rl, tl) = if c1 > 1e-24 {
                (
                    r - v1 * (2.0 / c1 * v1.dot(&r)),
                    tp - v1 * (2.0 / c1 * v1.dot(&tp)),
                )
            } else {
                (r, tp)
            };
            let v2 = t - tl;
            let c2 = v2.dot(&v2);
            r = if c2 > 1e-24 {
                rl - v2 * (2.0 / c2 * v2.dot(&rl))
            } else {
                rl
            };
            r = (r - t * r.dot(&t))
                .try_normalize(1e-12)
                .ok_or("the path turns back on itself")?;
            if i % 8 == 0 {
                rmf.push((x, t, r));
            }
            (xp, tp) = (x, t);
        }
        let basis = |t: Vector3, r: Vector3| Matrix3::from_columns(&[t, r, t.cross(&r)]);
        let f0 = basis(t0, rmf[0].2);
        let pn = plane.normal.into_inner();
        // The rail's points, and where it meets each station's plane.
        let rail_pts = rail.map(PathGeom::polyline);
        let mut rail_at: Vec<Point3> = Vec::new();
        if let Some(pts) = &rail_pts {
            let mut prev: Option<Point3> = None;
            for (x, t, _) in &rmf {
                let mut hits = Vec::new();
                for w in pts.windows(2) {
                    let (da, db) = ((w[0] - x).dot(t), (w[1] - x).dot(t));
                    if (da <= 0.0 && db >= 0.0) || (da >= 0.0 && db <= 0.0) {
                        let s = if (da - db).abs() > 1e-300 {
                            da / (da - db)
                        } else {
                            0.0
                        };
                        hits.push(w[0] + (w[1] - w[0]) * s);
                    }
                }
                let refp = prev.unwrap_or(*x);
                let hit = hits
                    .into_iter()
                    .min_by(|a, b| (a - refp).norm().total_cmp(&(b - refp).norm()))
                    .ok_or(
                        "the guide rail does not cross the profile at every point of the path",
                    )?;
                rail_at.push(hit);
                prev = Some(hit);
            }
        }
        // Rotation (orthonormal) and in-plane scale of each station.
        let mut rots: Vec<Matrix3<Real>> = Vec::new();
        let mut scales: Vec<Matrix3<Real>> = Vec::new();
        let r_eq = (area / std::f64::consts::PI).sqrt().max(1e-9);
        let (u0, e0) = match rail_at.first() {
            Some(q) => {
                let u = q - p0;
                let up = u - t0 * t0.dot(&u);
                let e = up
                    .try_normalize(1e-9)
                    .ok_or("the guide rail starts on the path")?;
                (up.norm(), e)
            }
            None => (1.0, rmf[0].2),
        };
        // The scaling direction in the profile plane.
        let e0p = (e0 - pn * pn.dot(&e0)).try_normalize(1e-9).unwrap_or(e0);
        for (i, (x, t, rr)) in rmf.iter().enumerate() {
            let s = len * i as Real / n as Real;
            let (rot, sc) = if let Some(q) = rail_at.get(i) {
                let u = q - x;
                let up = u - t * t.dot(&u);
                let e = up
                    .try_normalize(1e-9)
                    .ok_or("the guide rail touches the path")?;
                let rot = basis(*t, e) * basis(t0, e0).transpose();
                let k = up.norm() / u0;
                let sc = match self.scaling {
                    ProfileScaling::XY => Matrix3::identity() * k,
                    ProfileScaling::X => Matrix3::identity() + e0p * e0p.transpose() * (k - 1.0),
                    ProfileScaling::None => Matrix3::identity(),
                };
                (rot, sc)
            } else if self.orientation == SweepOrientation::Parallel {
                (Matrix3::identity(), Matrix3::identity())
            } else {
                let rot = basis(*t, *rr) * f0.transpose();
                let tw = nalgebra::Rotation3::from_axis_angle(
                    &nalgebra::Unit::new_normalize(*t),
                    twist * s / len,
                );
                let k = 1.0 + s * taper.tan() / r_eq;
                if k < 0.02 {
                    return Err("the taper closes the profile before the end of the path".into());
                }
                (tw.matrix() * rot, Matrix3::identity() * k)
            };
            rots.push(rot);
            scales.push(sc);
        }
        let o = plane.origin;
        let (xa, ya) = (plane.x_axis.into_inner(), plane.y_axis().into_inner());
        let frame = |i: usize| {
            let m = rots[i] * scales[i];
            let x = rmf[i].0;
            Frame {
                origin: x + m * (o - p0),
                x: m * xa,
                y: m * ya,
            }
        };
        // Chunks: each turns at most `limit` from its first section.
        let limit = if round {
            deg(60.0)
        } else {
            (deg(144.0) / segs as Real).min(deg(60.0))
        };
        let angle = |a: &Matrix3<Real>, b: &Matrix3<Real>| {
            let m = b * a.transpose();
            ((m.trace() - 1.0) / 2.0).clamp(-1.0, 1.0).acos()
        };
        let mut chunks = Vec::new();
        let mut start = 0;
        while start < n {
            let mut end = start + 1;
            while end < n && end - start < 40 && angle(&rots[start], &rots[end + 1]) <= limit {
                end += 1;
            }
            chunks.push((start..=end).map(frame).collect::<Vec<_>>());
            start = end;
        }
        Ok(chunks)
    }
}

#[cfg(test)]
#[path = "sweep_tests.rs"]
mod tests;
