//! The **Rib / Web** feature (Inventor: 3D Model → Create → Rib), see
//! `docs/spec/2.7-rib.md`.
//!
//! Open sketch curves become thin walls that fill out to the part:
//! - **Parallel to Sketch Plane** (a rib): from each point of the curve the
//!   material runs across the sketch plane toward the body until it meets
//!   it (To Next) or for a distance (Finite); the thickness is along the
//!   sketch normal. After SolidCraft's `rib_tool` (bands toward the
//!   material, MIT/Apache-2.0).
//! - **Normal to Sketch Plane** (a web): a band of the thickness around the
//!   curve, drawn along the sketch normal down to the next face, with draft
//!   (Hold Thickness At Top / Root) and bosses at sketch points on it.
//!
//! Every chain of the chosen curves makes its own tool (a network); the
//! tools are joined to the body. Where the body begins is found with rays
//! against the body's display triangles. Faces: `<id>:Side:Left` /
//! `Right` / `End0` / `End1` (`<id>#2:…` for the second chain…), the top
//! `<id>:Start`, the far end `<id>:End`, bosses `<id>:Boss<k>:…`.

use serde::{Deserialize, Serialize};

use crate::kernel::{BRep, Point3, Real, TessellationOptions, Vector3};
use crate::sketch::{Entity, EntityId, Point2, PointId, Sketch, Style, Vector2};

use super::expr::Unit;
use super::params::FeatureParams;

/// Which way the rib is drawn from its sketch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RibDirection {
    /// Normal to Sketch Plane (a web): drawn along the sketch normal, the
    /// thickness in the sketch plane.
    #[default]
    Normal,
    /// Parallel to Sketch Plane (a rib): spreads in the sketch plane, the
    /// thickness along its normal.
    Parallel,
}

/// Where the thickness goes from the profile.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RibSide {
    /// All of it to the first side (left of the curve / along the normal).
    Side1,
    /// All of it to the other side.
    Side2,
    /// Half each way.
    #[default]
    Symmetric,
}

/// How far the rib goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RibExtent {
    /// To the next face of the body.
    #[default]
    ToNext,
    /// A given depth.
    Finite,
}

/// Where a drafted web keeps its thickness.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DraftHold {
    /// At the sketch plane (the web widens toward its root).
    #[default]
    Top,
    /// Where the web meets the next face (narrower at the top).
    Root,
}

/// Bosses on a web (Inventor's Boss tab).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RibBoss {
    /// Sketch points on the profile where bosses stand.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub centers: Vec<PointId>,
    /// Diameter (mm).
    pub diameter: Real,
    /// Start above (+) or below (−) the sketch plane (mm).
    #[serde(default)]
    pub offset: Real,
    /// Draft angle (radians).
    #[serde(default)]
    pub draft: Real,
}

impl Default for RibBoss {
    fn default() -> Self {
        Self {
            centers: Vec::new(),
            diameter: 6.0,
            offset: 0.0,
            draft: 0.0,
        }
    }
}

fn yes() -> bool {
    true
}

/// The Rib feature.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RibFeature {
    /// Immutable identity (topology-name prefix).
    pub id: String,
    /// Display name.
    pub name: String,
    /// `id` of the profile sketch.
    pub sketch: String,
    /// The profile curves (chains of them are the ribs).
    pub curves: Vec<EntityId>,
    /// Normal (web) or Parallel (rib).
    #[serde(default)]
    pub direction: RibDirection,
    /// The direction flip: away from the side the body is on.
    #[serde(default)]
    pub flip: bool,
    /// Bodies it joins (empty: the first body it meets).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub solids: Vec<String>,
    /// Extend Profile: open ends that stop short of the body run on to it.
    #[serde(default = "yes")]
    pub extend: bool,
    /// Thickness (mm).
    pub thickness: Real,
    /// Where the thickness goes.
    #[serde(default)]
    pub side: RibSide,
    /// To Next or Finite.
    #[serde(default)]
    pub extent: RibExtent,
    /// Depth for Finite (mm).
    #[serde(default = "ten")]
    pub depth: Real,
    /// Taper of the side walls (radians; webs only).
    #[serde(default)]
    pub taper: Real,
    /// Draft: where the thickness holds (webs only).
    #[serde(default)]
    pub hold: DraftHold,
    /// Draft angle (radians; webs only), added to the taper.
    #[serde(default)]
    pub draft: Real,
    /// Bosses (webs only).
    #[serde(default)]
    pub boss: RibBoss,
    /// Equations of its values.
    #[serde(default, skip_serializing_if = "FeatureParams::is_empty")]
    pub params: FeatureParams,
    /// The preset it was made with (name only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
}

fn ten() -> Real {
    10.0
}

/// A chain of profile curves, sampled (sketch coordinates).
#[derive(Clone, Debug, PartialEq)]
pub struct RibChain {
    /// Points along it.
    pub points: Vec<Point2>,
    /// Closed loop.
    pub closed: bool,
}

/// Rays against a body's triangles.
pub(super) struct Rays {
    tris: Vec<[Point3; 3]>,
    /// Size of the body (diagonal of its box).
    pub size: Real,
}

impl Rays {
    /// The triangles of `body`'s closed solids (only those named in
    /// `solids` when it is not empty).
    pub fn of(body: &BRep, solids: &[String]) -> Self {
        let opts = TessellationOptions::default();
        let mut tris = Vec::new();
        let (mut lo, mut hi) = (
            Point3::new(Real::INFINITY, Real::INFINITY, Real::INFINITY),
            Point3::new(Real::NEG_INFINITY, Real::NEG_INFINITY, Real::NEG_INFINITY),
        );
        for (sid, s) in body.solids() {
            if s.open || !(solids.is_empty() || s.name.as_ref().is_some_and(|n| solids.contains(n)))
            {
                continue;
            }
            let Ok(m) = body.tessellate_solid(sid, &opts) else {
                continue;
            };
            for t in &m.faces.triangles {
                let [a, b, c] = t.map(|i| m.faces.positions[i as usize]);
                for p in [a, b, c] {
                    lo = lo.inf(&p);
                    hi = hi.sup(&p);
                }
                tris.push([a, b, c]);
            }
        }
        let size = if tris.is_empty() {
            0.0
        } else {
            (hi - lo).norm()
        };
        Self { tris, size }
    }

    /// Ray parameters (`t > 0`, sorted, near duplicates merged) where
    /// `p + t·d` (unit `d`) crosses the triangles.
    fn hits(&self, p: Point3, d: Vector3) -> Vec<Real> {
        let mut out = Vec::new();
        for [a, b, c] in &self.tris {
            let (e1, e2) = (b - a, c - a);
            let h = d.cross(&e2);
            let det = e1.dot(&h);
            if det.abs() < 1e-14 {
                continue;
            }
            let inv = 1.0 / det;
            let s = p - a;
            let u = s.dot(&h) * inv;
            if !(-1e-9..=1.0 + 1e-9).contains(&u) {
                continue;
            }
            let q = s.cross(&e1);
            let v = d.dot(&q) * inv;
            if v < -1e-9 || u + v > 1.0 + 1e-9 {
                continue;
            }
            let t = e2.dot(&q) * inv;
            if t > 1e-7 {
                out.push(t);
            }
        }
        out.sort_by(Real::total_cmp);
        out.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
        out
    }

    /// Where the ray (starting in the air) leaves the material: every
    /// second crossing.
    pub fn exits(&self, p: Point3, d: Vector3) -> Vec<Real> {
        self.hits(p, d).into_iter().skip(1).step_by(2).collect()
    }

    /// Where the ray enters the material and leaves it again: `(0, exit)`
    /// when it starts inside; `None` when it never meets the body.
    pub fn entry(&self, p: Point3, d: Vector3) -> Option<(Real, Real)> {
        let h = self.hits(p, d);
        if h.is_empty() {
            return None;
        }
        if h.len() % 2 == 1 {
            Some((0.0, h[0]))
        } else {
            Some((h[0], h[1]))
        }
    }
}

impl RibFeature {
    /// A web of `thickness` on `curves` with Inventor's defaults (Normal
    /// to Sketch Plane, symmetric, To Next, Extend Profile).
    pub fn new(curves: Vec<EntityId>, thickness: Real) -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            sketch: String::new(),
            curves,
            direction: RibDirection::Normal,
            flip: false,
            solids: Vec::new(),
            extend: true,
            thickness,
            side: RibSide::Symmetric,
            extent: RibExtent::ToNext,
            depth: 10.0,
            taper: 0.0,
            hold: DraftHold::Top,
            draft: 0.0,
            boss: RibBoss::default(),
            params: FeatureParams::default(),
            preset: None,
        }
    }

    /// Its numeric fields that are parameters.
    pub fn fields(&self) -> Vec<(&'static str, Unit)> {
        let mut v = vec![("thickness", Unit::Mm)];
        if self.extent == RibExtent::Finite {
            v.push(("depth", Unit::Mm));
        }
        if self.direction == RibDirection::Normal {
            v.extend([("taper", Unit::Deg), ("draft", Unit::Deg)]);
            if !self.boss.centers.is_empty() {
                v.extend([
                    ("boss_diameter", Unit::Mm),
                    ("boss_offset", Unit::Mm),
                    ("boss_draft", Unit::Deg),
                ]);
            }
        }
        v
    }

    /// The value of field `f`.
    pub fn value(&self, f: &str) -> Option<Real> {
        Some(match f {
            "thickness" => self.thickness,
            "depth" => self.depth,
            "taper" => self.taper,
            "draft" => self.draft,
            "boss_diameter" => self.boss.diameter,
            "boss_offset" => self.boss.offset,
            "boss_draft" => self.boss.draft,
            _ => return None,
        })
    }

    /// Sets field `f`.
    pub fn set(&mut self, f: &str, v: Real) {
        match f {
            "thickness" => self.thickness = v,
            "depth" => self.depth = v,
            "taper" => self.taper = v,
            "draft" => self.draft = v,
            "boss_diameter" => self.boss.diameter = v,
            "boss_offset" => self.boss.offset = v,
            "boss_draft" => self.boss.draft = v,
            _ => {}
        }
    }
}

/// The curves a Rib starts with: every normal (not construction), not
/// projected curve of the sketch.
pub fn default_curves(ps: &super::PartSketch) -> Vec<EntityId> {
    ps.sketch
        .entities()
        .filter(|(id, e)| {
            ps.sketch.style(*id) == Style::Normal && !e.points().iter().all(|&p| ps.is_projected(p))
        })
        .map(|(id, _)| id)
        .collect()
}

/// A sketch curve as a polyline (start → end; a circle closed on itself).
fn polyline(sk: &Sketch, id: EntityId) -> Option<Vec<Point2>> {
    let pos = |p: PointId| sk.point(p).map(|q| q.position);
    let tol: Real = 0.01;
    Some(match sk.entity(id)? {
        Entity::Line { start, end } => vec![pos(*start)?, pos(*end)?],
        Entity::Arc { center, start, end } => {
            let (c, a, b) = (pos(*center)?, pos(*start)?, pos(*end)?);
            let r = (a - c).norm();
            let a0 = (a.y - c.y).atan2(a.x - c.x);
            let mut sw = (b.y - c.y).atan2(b.x - c.x) - a0;
            while sw <= 0.0 {
                sw += std::f64::consts::TAU;
            }
            arc_points(c, r, a0, sw, tol)
        }
        Entity::Circle { center, radius } => {
            arc_points(pos(*center)?, *radius, 0.0, std::f64::consts::TAU, tol)
        }
        Entity::Spline { .. } => {
            let (cv, knots) = sk.spline_def(id, &|p| {
                sk.point(p).map_or(Vector2::zeros(), |q| q.position.coords)
            });
            crate::sketch::ProfileCurve::Spline {
                points: cv.into_iter().map(Point2::from).collect(),
                reversed: false,
                knots,
            }
            .sample(tol)
        }
    })
}

fn arc_points(c: Point2, r: Real, a0: Real, sweep: Real, tol: Real) -> Vec<Point2> {
    let step = 2.0 * (1.0 - (tol / r.max(1e-9)).min(1.0)).acos();
    let n = ((sweep.abs() / step.max(1e-3)).ceil() as usize).clamp(2, 512);
    (0..=n)
        .map(|i| {
            let t = a0 + sweep * i as Real / n as Real;
            c + Vector2::new(t.cos(), t.sin()) * r
        })
        .collect()
}

/// The chains of `curves`: curves joined end to end (by position) are one
/// polyline; a chain that comes back to its start is closed.
pub fn chains(sk: &Sketch, curves: &[EntityId]) -> Result<Vec<RibChain>, String> {
    let mut pieces: Vec<Vec<Point2>> = Vec::new();
    for &id in curves {
        let pts = polyline(sk, id)
            .ok_or_else(|| "a profile curve no longer exists in the sketch".to_owned())?;
        if pts.len() >= 2 {
            pieces.push(pts);
        }
    }
    if pieces.is_empty() {
        return Err("select the profile curves".into());
    }
    let same = |a: Point2, b: Point2| (a - b).norm() < 1e-6;
    let mut out = Vec::new();
    while let Some(mut chain) = pieces.pop() {
        loop {
            let (s, e) = (chain[0], chain[chain.len() - 1]);
            if chain.len() > 2 && same(s, e) {
                break;
            }
            let Some(k) = pieces.iter().position(|p| {
                let (a, b) = (p[0], p[p.len() - 1]);
                same(a, e) || same(b, e) || same(a, s) || same(b, s)
            }) else {
                break;
            };
            let mut p = pieces.remove(k);
            let (a, b) = (p[0], p[p.len() - 1]);
            if same(a, e) {
                chain.extend(p.drain(1..));
            } else if same(b, e) {
                p.reverse();
                chain.extend(p.drain(1..));
            } else if same(b, s) {
                p.pop();
                p.extend(chain);
                chain = p;
            } else {
                p.reverse();
                p.pop();
                p.extend(chain);
                chain = p;
            }
        }
        let closed = chain.len() > 2 && same(chain[0], chain[chain.len() - 1]);
        if closed {
            chain.pop();
        }
        out.push(RibChain {
            points: chain,
            closed,
        });
    }
    out.reverse();
    Ok(out)
}

/// Points evenly spaced along a polyline (its corners kept), about `n`.
fn resample(pts: &[Point2], closed: bool, n: usize) -> Vec<Point2> {
    let mut ring = pts.to_vec();
    if closed && let Some(f) = pts.first() {
        ring.push(*f);
    }
    let len: Real = ring.windows(2).map(|w| (w[1] - w[0]).norm()).sum();
    if len <= 0.0 {
        return pts.to_vec();
    }
    let step = len / n.max(1) as Real;
    let mut out = vec![ring[0]];
    for w in ring.windows(2) {
        let l = (w[1] - w[0]).norm();
        let k = (l / step).ceil().max(1.0) as usize;
        for i in 1..=k {
            out.push(w[0] + (w[1] - w[0]) * (i as Real / k as Real));
        }
    }
    if closed {
        out.pop();
    }
    out
}

/// Douglas–Peucker simplification of an open polyline.
fn simplify(pts: &[Point2], tol: Real) -> Vec<Point2> {
    if pts.len() <= 2 {
        return pts.to_vec();
    }
    let (a, b) = (pts[0], pts[pts.len() - 1]);
    let d = b - a;
    let len = d.norm();
    let dist = |p: &Point2| {
        if len < 1e-12 {
            (p - a).norm()
        } else {
            (d.perp(&(p - a)) / len).abs()
        }
    };
    let (k, far) = pts
        .iter()
        .enumerate()
        .skip(1)
        .take(pts.len() - 2)
        .map(|(i, p)| (i, dist(p)))
        .fold((0, -1.0), |m, x| if x.1 > m.1 { x } else { m });
    if far <= tol {
        return vec![a, b];
    }
    let mut left = simplify(&pts[..=k], tol);
    let right = simplify(&pts[k..], tol);
    left.pop();
    left.extend(right);
    left
}

/// Offsets a polyline by `d` (positive: to its left), mitred at corners.
fn offset_polyline(pts: &[Point2], closed: bool, d: Real) -> Vec<Point2> {
    let n = pts.len();
    let seg_n = |i: usize| -> Option<Vector2> {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        let t = (b - a).try_normalize(1e-12)?;
        Some(Vector2::new(-t.y, t.x))
    };
    let segs = if closed { n } else { n - 1 };
    let normals: Vec<Option<Vector2>> = (0..segs).map(seg_n).collect();
    (0..n)
        .map(|i| {
            let before = if i > 0 {
                normals[i - 1]
            } else if closed {
                normals[segs - 1]
            } else {
                None
            };
            let after = if i < segs { normals[i] } else { None };
            let m = match (before, after) {
                (Some(a), Some(b)) => {
                    let s = a + b;
                    let c = s.norm_squared() / 2.0;
                    if c < 0.05 { b } else { s / c }
                }
                (Some(a), None) | (None, Some(a)) => a,
                (None, None) => Vector2::zeros(),
            };
            pts[i] + m * d
        })
        .collect()
}

fn signed_area(p: &[Point2]) -> Real {
    (0..p.len())
        .map(|i| {
            let (a, b) = (p[i], p[(i + 1) % p.len()]);
            a.x * b.y - b.x * a.y
        })
        .sum::<Real>()
        / 2.0
}

/// A closed polygon (sketch coordinates) as profile segments named
/// `names[i]` per edge.
fn segments(poly: &[Point2], names: &[String]) -> Vec<crate::kernel::ProfileSegment> {
    (0..poly.len())
        .filter(|&i| (poly[(i + 1) % poly.len()] - poly[i]).norm() > 1e-9)
        .map(|i| crate::kernel::ProfileSegment {
            geometry: crate::kernel::Segment2::Line {
                a: poly[i],
                b: poly[(i + 1) % poly.len()],
            },
            name: names.get(i).cloned().unwrap_or_else(|| format!("S{i}")),
        })
        .collect()
}

/// One tool of a rib: a profile on a plane extruded between two offsets.
pub(super) struct RibTool {
    pub plane: crate::kernel::Plane,
    pub outer: Vec<crate::kernel::ProfileSegment>,
    pub holes: Vec<Vec<crate::kernel::ProfileSegment>>,
    pub start: Real,
    pub end: Real,
    pub taper: Real,
    pub prefix: String,
}

impl RibFeature {
    /// Whether every open end of the profile reaches the body (then
    /// Extend Profile does nothing and Inventor hides it).
    pub fn ends_reach(&self, sketch: &Sketch, body: &BRep) -> bool {
        let rays = Rays::of(body, &self.solids);
        let Ok(chains) = chains(sketch, &self.curves) else {
            return true;
        };
        let dir = self.normal_dir(sketch, &rays, &chains);
        chains.iter().filter(|c| !c.closed).all(|c| {
            let n = c.points.len();
            [
                (c.points[0], c.points[1]),
                (c.points[n - 1], c.points[n - 2]),
            ]
            .iter()
            .all(|&(e, prev)| {
                self.end_gap(sketch, &rays, e, prev, dir)
                    .is_none_or(|g| g < 1e-3)
            })
        })
    }

    /// The side of the sketch plane the web goes to (+1 along the
    /// normal, −1 against), toward the material unless flipped.
    fn normal_dir(&self, sketch: &Sketch, rays: &Rays, chains: &[RibChain]) -> Real {
        let pl = &sketch.plane;
        let n = pl.normal.into_inner();
        let mut score = 0i32;
        for c in chains {
            for q in resample(&c.points, c.closed, 16) {
                let p = pl.point_at(q.x, q.y);
                if rays.entry(p, n).is_some() {
                    score += 1;
                }
                if rays.entry(p, -n).is_some() {
                    score -= 1;
                }
            }
        }
        let s = if score < 0 { -1.0 } else { 1.0 };
        if self.flip { -s } else { s }
    }

    /// How far an open end `e` (coming from `prev`) is from the body along
    /// its tangent (`None`: nothing there). For a web the ray runs a little
    /// below the sketch plane (side `dir`).
    fn end_gap(
        &self,
        sketch: &Sketch,
        rays: &Rays,
        e: Point2,
        prev: Point2,
        dir: Real,
    ) -> Option<Real> {
        let pl = &sketch.plane;
        let t = (e - prev).try_normalize(1e-12)?;
        let t3 = pl.x_axis.into_inner() * t.x + pl.y_axis().into_inner() * t.y;
        let below = if self.direction == RibDirection::Normal {
            // Halfway down the web (or 1 mm when To Next is not known yet).
            let d = match self.extent {
                RibExtent::Finite => self.depth / 2.0,
                RibExtent::ToNext => {
                    let p = pl.point_at(e.x, e.y);
                    rays.entry(p, pl.normal.into_inner() * dir)
                        .map_or(
                            0.5,
                            |(h, x)| if h > 1e-9 { h / 2.0 } else { x.min(1.0) / 2.0 },
                        )
                }
            };
            pl.normal.into_inner() * dir * d.max(1e-3)
        } else {
            Vector3::zeros()
        };
        let p = pl.point_at(e.x, e.y) + below;
        rays.entry(p, t3).map(|(h, _)| h)
    }

    /// The chains with Extend Profile applied: open ends run on along their
    /// tangent into the body.
    fn extended(
        &self,
        sketch: &Sketch,
        rays: &Rays,
        chains: &[RibChain],
        dir: Real,
        ov: Real,
    ) -> Vec<RibChain> {
        if !self.extend {
            return chains.to_vec();
        }
        chains
            .iter()
            .map(|c| {
                if c.closed || c.points.len() < 2 {
                    return c.clone();
                }
                let mut pts = c.points.clone();
                let n = pts.len();
                for (e, prev, front) in [(pts[0], pts[1], true), (pts[n - 1], pts[n - 2], false)] {
                    let Some(g) = self.end_gap(sketch, rays, e, prev, dir) else {
                        continue;
                    };
                    if g < 1e-3 || g > rays.size {
                        continue;
                    }
                    let Some(t) = (e - prev).try_normalize(1e-12) else {
                        continue;
                    };
                    let q = e + t * (g + ov);
                    if front {
                        pts.insert(0, q);
                    } else {
                        pts.push(q);
                    }
                }
                RibChain {
                    points: pts,
                    closed: false,
                }
            })
            .collect()
    }

    /// The tools of the rib on `sketch` against `body` (the body so far).
    pub(super) fn tools(&self, sketch: &Sketch, body: &BRep) -> Result<Vec<RibTool>, String> {
        let t = self.thickness;
        if !(t.is_finite() && t > 1e-6) {
            return Err("the thickness must be greater than 0".into());
        }
        if self.extent == RibExtent::Finite && !(self.depth.is_finite() && self.depth > 1e-6) {
            return Err("the depth must be greater than 0".into());
        }
        let rays = Rays::of(body, &self.solids);
        if rays.size <= 0.0 && self.extent == RibExtent::ToNext {
            return Err("there is no body for the rib to reach (use Finite)".into());
        }
        let chains = chains(sketch, &self.curves)?;
        let pl = sketch.plane.clone();
        let dir = self.normal_dir(sketch, &rays, &chains);
        let ov = (t / 4.0).clamp(0.05, 1.0);
        let chains = self.extended(sketch, &rays, &chains, dir, ov);
        let mut out = Vec::new();
        for (k, c) in chains.iter().enumerate() {
            let prefix = if k == 0 {
                self.id.clone()
            } else {
                format!("{}#{}", self.id, k + 1)
            };
            match self.direction {
                RibDirection::Parallel => out.push(self.parallel_tool(&pl, &rays, c, ov, prefix)?),
                RibDirection::Normal => {
                    out.extend(self.web_tools(sketch, &rays, c, dir, ov, prefix)?)
                }
            }
        }
        Ok(out)
    }

    /// A rib in the sketch plane: the curve and its shadow on the body.
    fn parallel_tool(
        &self,
        pl: &crate::kernel::Plane,
        rays: &Rays,
        c: &RibChain,
        ov: Real,
        prefix: String,
    ) -> Result<RibTool, String> {
        if c.closed {
            return Err("a rib parallel to the sketch plane needs an open profile".into());
        }
        let (a, b) = (c.points[0], c.points[c.points.len() - 1]);
        let chord = (b - a)
            .try_normalize(1e-12)
            .ok_or_else(|| "the rib profile has no length".to_owned())?;
        let to3 = |v: Vector2| pl.x_axis.into_inner() * v.x + pl.y_axis().into_inner() * v.y;
        let along = resample(&c.points, false, 64);
        // Toward the material: the side of the curve where the rays meet the body.
        let mut n = Vector2::new(-chord.y, chord.x);
        let probe = |side: Vector2| {
            along
                .iter()
                .filter(|q| rays.entry(pl.point_at(q.x, q.y), to3(side)).is_some())
                .count()
        };
        if probe(-n) > probe(n) {
            n = -n;
        }
        if self.flip {
            n = -n;
        }
        let mut far = Vec::with_capacity(along.len());
        let mut found = 0;
        for q in &along {
            let h = match (rays.entry(pl.point_at(q.x, q.y), to3(n)), self.extent) {
                (Some((h, x)), RibExtent::ToNext) => {
                    found += 1;
                    Some(h + ov.min((x - h) / 2.0))
                }
                (_, RibExtent::Finite) => {
                    found += 1;
                    Some(self.depth)
                }
                (None, RibExtent::ToNext) => None,
            };
            far.push(h);
        }
        if found == 0 {
            return Err("the rib profile does not face the body (use Finite)".into());
        }
        // Samples that miss take their nearest neighbour's reach.
        let known: Vec<(usize, Real)> = far
            .iter()
            .enumerate()
            .filter_map(|(i, h)| h.map(|h| (i, h)))
            .collect();
        let far: Vec<Point2> = along
            .iter()
            .enumerate()
            .map(|(i, q)| {
                let h = far[i].unwrap_or_else(|| {
                    known
                        .iter()
                        .min_by_key(|(j, _)| j.abs_diff(i))
                        .map_or(0.0, |(_, h)| *h)
                });
                q + n * h
            })
            .collect();
        let far = simplify(&far, ov / 4.0);
        let mut poly = simplify(&along, 1e-6);
        let mut names = vec!["Top".to_owned(); poly.len() - 1];
        names.push("End1".into());
        let fl = far.len();
        poly.extend(far.into_iter().rev());
        names.extend(std::iter::repeat_n("Root".to_owned(), fl - 1));
        names.push("End0".into());
        if signed_area(&poly) < 0.0 {
            poly.reverse();
            names.reverse();
            names.rotate_left(1);
        }
        let (start, end) = match self.side {
            RibSide::Symmetric => (-self.thickness / 2.0, self.thickness / 2.0),
            RibSide::Side1 => (0.0, self.thickness),
            RibSide::Side2 => (-self.thickness, 0.0),
        };
        Ok(RibTool {
            plane: pl.clone(),
            outer: segments(&poly, &names),
            holes: Vec::new(),
            start,
            end,
            taper: 0.0,
            prefix,
        })
    }

    /// A web: the band around the curve drawn along the sketch normal, and
    /// its bosses.
    fn web_tools(
        &self,
        sketch: &Sketch,
        rays: &Rays,
        c: &RibChain,
        dir: Real,
        ov: Real,
        prefix: String,
    ) -> Result<Vec<RibTool>, String> {
        let pl = &sketch.plane;
        let n3 = pl.normal.into_inner() * dir;
        let t = self.thickness;
        // Depth: To Next = the deepest entry under the band, a little into
        // the material; every sample must still be in material there.
        let samples = resample(&c.points, c.closed, 64);
        let (depth, root) = match self.extent {
            RibExtent::Finite => (self.depth, self.depth),
            RibExtent::ToNext => {
                let mut hmax: Option<Real> = None;
                let mut exits = Vec::new();
                for q in &samples {
                    if let Some((h, x)) = rays.entry(pl.point_at(q.x, q.y), n3) {
                        hmax = Some(hmax.map_or(h, |m: Real| m.max(h)));
                        exits.push(x);
                    }
                }
                let h = hmax.ok_or("the web does not reach a face (use Finite)")?;
                if h < 1e-6 {
                    return Err("the web starts inside the body: there is no next face".into());
                }
                let room = exits.iter().copied().fold(Real::INFINITY, Real::min) - h;
                if room <= 1e-6 {
                    return Err(
                        "the face under the web is not even (part of it is deeper than the material elsewhere); use Finite".into(),
                    );
                }
                (h + ov.min(room / 2.0), h)
            }
        };
        let angle = self.taper + self.draft;
        if !angle.is_finite() || angle.abs() >= 1.4 {
            return Err("the draft must be between -80 and 80 deg".into());
        }
        // Thickness at the sketch plane.
        let top = match self.hold {
            DraftHold::Top => t,
            DraftHold::Root => t - 2.0 * root * angle.tan(),
        };
        if top <= 1e-6 {
            return Err(
                "Draft: the web gets no thickness at the top (Hold Thickness At Root)".into(),
            );
        }
        let (l, r) = match self.side {
            RibSide::Symmetric => (top / 2.0, -top / 2.0),
            RibSide::Side1 => (top, 0.0),
            RibSide::Side2 => (0.0, -top),
        };
        let pts = simplify_keep(&c.points, c.closed);
        let left = offset_polyline(&pts, c.closed, l);
        let right = offset_polyline(&pts, c.closed, r);
        let (outer, holes) = if c.closed {
            let (mut a, mut b) = (left, right);
            if signed_area(&a).abs() < signed_area(&b).abs() {
                std::mem::swap(&mut a, &mut b);
            }
            if signed_area(&a) < 0.0 {
                a.reverse();
            }
            if signed_area(&b) > 0.0 {
                b.reverse();
            }
            let na = vec!["Outer".to_owned(); a.len()];
            let nb = vec!["Inner".to_owned(); b.len()];
            (segments(&a, &na), vec![segments(&b, &nb)])
        } else {
            let mut poly = left.clone();
            let mut names = vec!["Left".to_owned(); left.len() - 1];
            names.push("End1".into());
            let nr = right.len();
            poly.extend(right.into_iter().rev());
            names.extend(std::iter::repeat_n("Right".to_owned(), nr - 1));
            names.push("End0".into());
            if signed_area(&poly) < 0.0 {
                poly.reverse();
                names.reverse();
                names.rotate_left(1);
            }
            (segments(&poly, &names), Vec::new())
        };
        let end = depth * dir;
        let mut out = vec![RibTool {
            plane: pl.clone(),
            outer,
            holes,
            start: 0.0,
            end,
            taper: angle,
            prefix: prefix.clone(),
        }];
        // Bosses at the chosen sketch points that lie on this chain.
        let on_chain = |p: Point2| {
            let n = c.points.len();
            let segs = if c.closed { n } else { n - 1 };
            (0..segs).any(|i| {
                let (a, b) = (c.points[i], c.points[(i + 1) % n]);
                let d = b - a;
                let l2 = d.norm_squared();
                let s = if l2 > 0.0 {
                    ((p - a).dot(&d) / l2).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                (a + d * s - p).norm() < 1e-3
            })
        };
        let b = &self.boss;
        let mut k = 0;
        for &cp in &b.centers {
            let Some(p) = sketch.point(cp).map(|q| q.position) else {
                return Err("a boss centre no longer exists in the sketch".into());
            };
            if !on_chain(p) {
                continue;
            }
            if !(b.diameter.is_finite() && b.diameter > t) {
                return Err("Boss: the diameter must be larger than the thickness".into());
            }
            k += 1;
            let ring = arc_points(p, b.diameter / 2.0, 0.0, std::f64::consts::TAU, 0.005);
            let ring = &ring[..ring.len() - 1];
            let names = vec!["Wall".to_owned(); ring.len()];
            let mut poly = ring.to_vec();
            if signed_area(&poly) < 0.0 {
                poly.reverse();
            }
            out.push(RibTool {
                plane: pl.clone(),
                outer: segments(&poly, &names),
                holes: Vec::new(),
                start: -b.offset * dir,
                end,
                taper: b.draft,
                prefix: format!("{prefix}:Boss{k}"),
            });
        }
        Ok(out)
    }
}

impl RibFeature {
    /// The rib's exact tools (before they are joined to the body).
    pub(super) fn build_exact(
        &self,
        sketch: &Sketch,
        body: &BRep,
    ) -> Result<Vec<crate::kernel::exact::ExactSolid>, super::exact::Fail> {
        use super::exact::Fail::{Kernel, User};
        let tools = self.tools(sketch, body).map_err(User)?;
        tools
            .iter()
            .map(|t| {
                let prof = crate::kernel::exact::profile(&t.outer, &t.holes).map_err(Kernel)?;
                crate::kernel::exact::extrude(&t.plane, &prof, t.start, t.end, t.taper, &t.prefix)
                    .map_err(|e| User(format!("the geometry kernel cannot build the rib ({e})")))
            })
            .collect()
    }
}

impl super::PartDocument {
    /// **Rib** on curves of sketch `index` (id `Rib<N>`; name and sketch
    /// are assigned here). The sketch is consumed (hidden) or shared.
    /// Returns the id; on failure nothing changes.
    pub fn add_rib(
        &mut self,
        index: usize,
        mut def: RibFeature,
    ) -> Result<String, super::PartError> {
        let sketch_id = self
            .sketches
            .get(index)
            .ok_or(super::PartError::NoSketch(index))?
            .id
            .clone();
        if def.curves.is_empty() {
            return Err(super::PartError::Feature(
                "select the profile curves".into(),
            ));
        }
        let id = self.next_feature_id("Rib");
        (def.id, def.name, def.sketch) = (id.clone(), id.clone(), sketch_id.clone());
        self.add_consuming(super::Feature::Rib(def), &[sketch_id])?;
        Ok(id)
    }
}

/// A polyline without its collinear inner points.
fn simplify_keep(pts: &[Point2], closed: bool) -> Vec<Point2> {
    if closed {
        return pts.to_vec();
    }
    simplify(pts, 1e-7)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chains_join_curves_end_to_end() {
        let mut sk = Sketch::default();
        let a = sk.add_line(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
        let b = sk.add_line(Point2::new(10.0, 0.0), Point2::new(10.0, 5.0));
        let c = sk.add_line(Point2::new(20.0, 0.0), Point2::new(30.0, 0.0));
        let ch = chains(&sk, &[a, b, c]).unwrap();
        assert_eq!(ch.len(), 2);
        assert!(ch.iter().any(|c| c.points.len() == 3 && !c.closed));
        let sq = sk.add_rectangle_two_point(Point2::new(40.0, 0.0), Point2::new(50.0, 10.0));
        assert!(sq.is_ok());
    }

    #[test]
    fn offsets_and_simplify() {
        let p = vec![
            Point2::new(0.0, 0.0),
            Point2::new(10.0, 0.0),
            Point2::new(10.0, 10.0),
        ];
        let l = offset_polyline(&p, false, 1.0);
        assert!((l[1] - Point2::new(9.0, 1.0)).norm() < 1e-9, "{l:?}");
        let s = simplify(&resample(&p, false, 20), 1e-6);
        assert_eq!(s.len(), 3);
    }
}
