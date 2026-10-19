//! **Split** (Inventor: 3D Model → Modify → Split,
//! `docs/spec/2.13-combine-split.md`): divides faces (Split Face), removes
//! one side of a solid (Trim Solid) or makes two solids of one (Split
//! Solid). The tool is an origin / work plane, a planar face's plane or a
//! 2D sketch; a surface body as tool needs exact surfaces (not yet).
//!
//! Names (rule 2): pieces of a divided face are `<face>~<id>.<k>` (`k` from
//! the back of the tool); Trim / Split Solid keep every face's name and
//! name the section `<id>:Cut` (the part behind the tool, or the kept part)
//! and `<id>#2:Cut` (the part in front). Split Solid's back part keeps the
//! body's name, the front part is the new body `<id>#2`.
//!
//! Not patternable on its own (Inventor repeats it only within a patterned
//! solid), so it records no [`super::pattern::ToolSet`].

use serde::{Deserialize, Serialize};

use crate::kernel::exact::{self, ExactOp, ExactSolid, SplitBy};
use crate::kernel::{BRep, Plane, Point3, ProfileSegment, Real, Segment2, Vector3};
use crate::sketch::{Entity, Style};

use super::exact::Fail;
use super::placement::{PlaneRef, WorkGeom, plane_of};
use super::shell::FaceRef;

/// What Split does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitMethod {
    /// Divide faces; the solid is unchanged.
    #[default]
    SplitFace,
    /// Remove one side of a solid.
    TrimSolid,
    /// Keep both sides as two solids.
    SplitSolid,
}

impl SplitMethod {
    /// Inventor's name.
    pub fn label(self) -> &'static str {
        match self {
            SplitMethod::SplitFace => "Split Face",
            SplitMethod::TrimSolid => "Trim Solid",
            SplitMethod::SplitSolid => "Split Solid",
        }
    }
}

/// The splitting tool.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SplitToolRef {
    /// An origin plane, a work plane or a planar face's plane.
    Plane(PlaneRef),
    /// A 2D sketch (by id): its curves, swept along its normal.
    Sketch(String),
    /// A surface body (by persistent name).
    Surface(String),
}

/// The Split feature.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SplitFeature {
    /// Immutable identity (`Split<N>`).
    pub id: String,
    /// Display name.
    pub name: String,
    /// Split Face, Trim Solid or Split Solid.
    #[serde(default)]
    pub method: SplitMethod,
    /// The tool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<SplitToolRef>,
    /// Faces to divide (Split Face).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub faces: Vec<FaceRef>,
    /// Split Face: every face of `solid` the tool meets.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub all_faces: bool,
    /// The body (persistent name): Trim / Split Solid, and All Faces. Empty:
    /// the only solid.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub solid: String,
    /// Trim Solid: keep the side the arrow points to (Inventor's Keep Flip
    /// side); by default that side is removed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub flip: bool,
    /// Keep the tool sketch visible after the feature (Inventor: Keep
    /// sketch visible on (+)).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub keep_sketch_visible: bool,
}

impl SplitFeature {
    /// An empty Split of the given method.
    pub fn new(method: SplitMethod) -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            method,
            tool: None,
            faces: Vec::new(),
            all_faces: false,
            solid: String::new(),
            flip: false,
            keep_sketch_visible: false,
        }
    }

    /// The tool sketch's id, if the tool is a sketch.
    pub fn sketch(&self) -> Option<&str> {
        match &self.tool {
            Some(SplitToolRef::Sketch(id)) => Some(id),
            _ => None,
        }
    }

    /// The new body of Split Solid (`<id>#2`).
    pub fn new_body(&self) -> String {
        format!("{}#2", self.id)
    }
}

/// A tool resolved against the body built so far.
#[derive(Clone, Debug)]
pub enum ResolvedTool {
    /// A plane; its normal points to the front side.
    Plane(Plane),
    /// Sketch curves (open or closed chains) on `plane`; `regions`: the
    /// closed regions, for solids.
    Sketch {
        /// The sketch plane.
        plane: Plane,
        /// Chains of curves (sketch coordinates).
        chains: Vec<Vec<Segment2>>,
        /// The plane through a single straight line (normal = sketch normal
        /// × line direction), when the sketch is one line.
        line: Option<Plane>,
    },
}

impl ResolvedTool {
    /// The plane whose front side the arrow shows, if the tool is planar.
    pub fn side_plane(&self) -> Option<&Plane> {
        match self {
            ResolvedTool::Plane(p) => Some(p),
            ResolvedTool::Sketch { line, .. } => line.as_ref(),
        }
    }
}

/// A point at `s` ∈ [0, 1] along a segment (a spline: along its control
/// polygon, which holds the curve — enough for boxes and orientation).
fn seg_point(g: &Segment2, s: Real) -> crate::sketch::Point2 {
    match g {
        Segment2::Line { a, b } => a + (b - a) * s,
        Segment2::Arc { c, r, a0, sweep } => {
            let t = a0 + sweep * s;
            c + nalgebra::Vector2::new(t.cos(), t.sin()) * *r
        }
        Segment2::Spline {
            points, reversed, ..
        } => {
            let s = if *reversed { 1.0 - s } else { s };
            let n = points.len().saturating_sub(1);
            if n == 0 {
                return points.first().copied().unwrap_or_else(crate::sketch::Point2::origin);
            }
            let x = (s.clamp(0.0, 1.0) * n as Real).min(n as Real - 1e-12);
            let k = x.floor() as usize;
            let (p, q) = (points[k], points[(k + 1).min(n)]);
            p + (q - p) * (x - k as Real)
        }
    }
}

fn reversed(s: &Segment2) -> Segment2 {
    match s {
        Segment2::Line { a, b } => Segment2::Line { a: *b, b: *a },
        Segment2::Arc { c, r, a0, sweep } => Segment2::Arc {
            c: *c,
            r: *r,
            a0: a0 + sweep,
            sweep: -sweep,
        },
        Segment2::Spline {
            points,
            reversed,
            knots,
        } => Segment2::Spline {
            points: points.clone(),
            reversed: !reversed,
            knots: knots.clone(),
        },
    }
}

/// The sketch's own curves (not construction, not projected edges) as
/// chains joined at shared end points.
pub fn sketch_chains(ps: &super::PartSketch) -> Vec<Vec<Segment2>> {
    use crate::sketch::PointId;
    let sk = &ps.sketch;
    let pos = |p: PointId| sk.point(p).map(|q| q.position);
    // (segment, start point id, end point id); closed curves have none.
    let mut open: Vec<(Segment2, PointId, PointId)> = Vec::new();
    let mut chains: Vec<Vec<Segment2>> = Vec::new();
    for (id, e) in sk.entities() {
        if sk.style(id) == Style::Construction || ps.reference_lines.contains(&id) {
            continue;
        }
        match e {
            Entity::Line { start, end } => {
                if let (Some(a), Some(b)) = (pos(*start), pos(*end)) {
                    open.push((Segment2::Line { a, b }, *start, *end));
                }
            }
            Entity::Circle { center, radius } => {
                if let Some(c) = pos(*center) {
                    chains.push(vec![Segment2::Arc {
                        c,
                        r: *radius,
                        a0: 0.0,
                        sweep: std::f64::consts::TAU,
                    }]);
                }
            }
            Entity::Arc { center, start, end } => {
                if let (Some(c), Some(a), Some(b)) = (pos(*center), pos(*start), pos(*end)) {
                    let a0 = (a - c).y.atan2((a - c).x);
                    let a1 = (b - c).y.atan2((b - c).x);
                    let mut sweep = a1 - a0;
                    if sweep <= 0.0 {
                        sweep += std::f64::consts::TAU;
                    }
                    open.push((
                        Segment2::Arc {
                            c,
                            r: (a - c).norm(),
                            a0,
                            sweep,
                        },
                        *start,
                        *end,
                    ));
                }
            }
            Entity::Spline { points } => {
                let (cv, knots) = sk.spline_def(id, &|p| {
                    sk.point(p)
                        .map_or(nalgebra::Vector2::zeros(), |q| q.position.coords)
                });
                let (Some(&s), Some(&t)) = (points.first(), points.last()) else {
                    continue;
                };
                let seg = Segment2::Spline {
                    points: cv.into_iter().map(crate::sketch::Point2::from).collect(),
                    reversed: false,
                    knots,
                };
                if s == t {
                    chains.push(vec![seg]);
                } else {
                    open.push((seg, s, t));
                }
            }
        }
    }
    // Chain the open pieces end to start (turning them round as needed).
    while let Some((seg, s, mut t)) = open.pop() {
        let mut chain = vec![seg];
        let start = s;
        loop {
            let Some(k) = open.iter().position(|(_, a, b)| *a == t || *b == t) else {
                break;
            };
            let (g, a, b) = open.remove(k);
            if a == t {
                chain.push(g);
                t = b;
            } else {
                chain.push(reversed(&g));
                t = a;
            }
            if t == start {
                break;
            }
        }
        // Extend backwards from the start too.
        let mut head = start;
        while head != t {
            let Some(k) = open.iter().position(|(_, a, b)| *a == head || *b == head) else {
                break;
            };
            let (g, a, b) = open.remove(k);
            if b == head {
                chain.insert(0, g);
                head = a;
            } else {
                chain.insert(0, reversed(&g));
                head = b;
            }
        }
        chains.push(chain);
    }
    chains
}

/// Resolves the tool against the body built so far.
pub fn resolve_tool(
    tool: Option<&SplitToolRef>,
    body: &BRep,
    works: &WorkGeom,
    sketches: &[super::PartSketch],
) -> Result<ResolvedTool, String> {
    match tool {
        None => Err("select the split tool".into()),
        Some(SplitToolRef::Plane(p)) => plane_of(p, body, works)
            .map(ResolvedTool::Plane)
            .map_err(|e| e.to_string()),
        Some(SplitToolRef::Sketch(id)) => {
            let ps = sketches
                .iter()
                .find(|s| s.id == *id)
                .ok_or_else(|| format!("the sketch {id} no longer exists"))?;
            if let Some(h) = &ps.health {
                return Err(format!("the sketch {} is sick ({h})", ps.name));
            }
            let chains = sketch_chains(ps);
            if chains.is_empty() {
                return Err(format!("the sketch {} has no curves to split with", ps.name));
            }
            let plane = ps.sketch.plane.clone();
            let line = match chains.as_slice() {
                [c] => match c.as_slice() {
                    [Segment2::Line { a, b }] => {
                        let (pa, pb) = (plane.point_at(a.x, a.y), plane.point_at(b.x, b.y));
                        let n = plane.normal.cross(&(pb - pa));
                        Plane::from_normal(pa, n).ok()
                    }
                    _ => None,
                },
                _ => None,
            };
            Ok(ResolvedTool::Sketch {
                plane,
                chains,
                line,
            })
        }
        Some(SplitToolRef::Surface(name)) => Err(format!(
            "a surface body ({name}) as the tool is not supported yet (surfaces are not exact yet)"
        )),
    }
}

fn solid_index(cur: &[ExactSolid], name: &str) -> Result<usize, String> {
    if name.is_empty() {
        return match cur.len() {
            0 => Err("there is no solid to split".into()),
            1 => Ok(0),
            _ => Err("select the solid".into()),
        };
    }
    cur.iter()
        .position(|s| s.name.as_deref() == Some(name))
        .ok_or_else(|| format!("the solid {name} no longer exists"))
}

/// Which faces (kernel indices) a plane crosses: corners on both sides.
fn crossed_by_plane(s: &ExactSolid, pl: &Plane) -> Vec<usize> {
    let size = s
        .measure_full()
        .map_or(1.0, |m| (m.bbox.1 - m.bbox.0).norm().max(1e-6));
    let eps = 1e-7 * size;
    exact::face_corners(s)
        .unwrap_or_default()
        .iter()
        .enumerate()
        .filter(|(_, pts)| {
            let d: Vec<Real> = pts.iter().map(|p| pl.signed_distance(p)).collect();
            d.iter().any(|&x| x > eps) && d.iter().any(|&x| x < -eps)
        })
        .map(|(i, _)| i)
        .collect()
}

/// Faces whose box (in the sketch plane) overlaps the curves' box.
fn near_curves(s: &ExactSolid, plane: &Plane, chains: &[Vec<Segment2>]) -> Vec<usize> {
    let mut lo = [Real::INFINITY; 2];
    let mut hi = [Real::NEG_INFINITY; 2];
    for c in chains {
        for g in c {
            for k in 0..=16 {
                let p = seg_point(g, k as Real / 16.0);
                lo = [lo[0].min(p.x), lo[1].min(p.y)];
                hi = [hi[0].max(p.x), hi[1].max(p.y)];
            }
        }
    }
    exact::face_corners(s)
        .unwrap_or_default()
        .iter()
        .enumerate()
        .filter(|(_, pts)| {
            let mut a = [Real::INFINITY; 2];
            let mut b = [Real::NEG_INFINITY; 2];
            for p in pts.iter() {
                let q = plane.project(p);
                a = [a[0].min(q.x), a[1].min(q.y)];
                b = [b[0].max(q.x), b[1].max(q.y)];
            }
            !pts.is_empty()
                && a[0] <= hi[0] + 1e-6
                && b[0] >= lo[0] - 1e-6
                && a[1] <= hi[1] + 1e-6
                && b[1] >= lo[1] - 1e-6
        })
        .map(|(i, _)| i)
        .collect()
}

fn split_by(t: &ResolvedTool) -> SplitBy {
    match t {
        ResolvedTool::Plane(p) => SplitBy::Plane(p.clone()),
        ResolvedTool::Sketch { line: Some(p), .. } => SplitBy::Plane(p.clone()),
        ResolvedTool::Sketch { plane, chains, .. } => SplitBy::Curves {
            plane: plane.clone(),
            curves: chains.clone(),
        },
    }
}

/// The exact build of a Split. User errors and kernel refusals are the
/// feature's health (no faceted rebuild could split either).
pub(super) fn build_exact(
    sp: &SplitFeature,
    body: &BRep,
    works: &WorkGeom,
    sketches: &[super::PartSketch],
    cur: &[ExactSolid],
) -> Result<Vec<ExactSolid>, Fail> {
    let tool = resolve_tool(sp.tool.as_ref(), body, works, sketches).map_err(Fail::User)?;
    let kernel = |e: String| Fail::User(format!("the geometry kernel cannot split it ({e})"));
    let mut out = cur.to_vec();
    match sp.method {
        SplitMethod::SplitFace => {
            let by = split_by(&tool);
            let side = tool.side_plane().cloned();
            if sp.all_faces {
                let i = solid_index(&out, &sp.solid).map_err(Fail::User)?;
                let faces = match &tool {
                    ResolvedTool::Plane(p) | ResolvedTool::Sketch { line: Some(p), .. } => {
                        crossed_by_plane(&out[i], p)
                    }
                    ResolvedTool::Sketch { plane, chains, .. } => {
                        near_curves(&out[i], plane, chains)
                    }
                };
                let r = exact::split_faces_lenient(&out[i], &faces, &by, &sp.id, side.as_ref())
                    .map_err(kernel)?
                    .ok_or_else(|| Fail::User("the tool does not cross the body".into()))?;
                out[i] = r;
                return Ok(out);
            }
            if sp.faces.is_empty() {
                return Err(Fail::User("select the faces to split".into()));
            }
            // Faces by solid (each solid split once).
            let mut groups: Vec<(usize, Vec<(usize, String)>)> = Vec::new();
            for r in &sp.faces {
                let hint = Point3::from(r.hint);
                let found = out.iter().enumerate().find_map(|(i, s)| {
                    exact::faces_named(s, &r.face, hint)
                        .ok()
                        .and_then(|v| v.first().map(|&f| (i, f)))
                });
                let Some((i, f)) = found else {
                    return Err(Fail::User(format!("the face {} no longer exists", r.face)));
                };
                match groups.iter_mut().find(|(g, _)| *g == i) {
                    Some((_, v)) if !v.iter().any(|(x, _)| *x == f) => v.push((f, r.face.clone())),
                    Some(_) => {}
                    None => groups.push((i, vec![(f, r.face.clone())])),
                }
            }
            for (i, faces) in groups {
                let idx: Vec<usize> = faces.iter().map(|(f, _)| *f).collect();
                out[i] = exact::split_faces(&out[i], &idx, &by, &sp.id, side.as_ref()).map_err(
                    |e| {
                        // Name the face the tool misses.
                        let missed = faces.iter().find(|(f, _)| {
                            e.contains(&format!("does not cross face {f}"))
                        });
                        match missed {
                            Some((_, n)) => {
                                Fail::User(format!("the tool does not cross the face {n}"))
                            }
                            None if e.contains("does not cross") => {
                                Fail::User("the tool does not cross the selected faces".into())
                            }
                            None => kernel(e),
                        }
                    },
                )?;
            }
            Ok(out)
        }
        SplitMethod::TrimSolid | SplitMethod::SplitSolid => {
            let i = solid_index(&out, &sp.solid).map_err(Fail::User)?;
            let (back, front) = halves(&out[i], &tool, &sp.id).map_err(|e| match e {
                Halves::User(m) => Fail::User(m),
                Halves::Kernel(m) => kernel(m),
            })?;
            let (Some(mut back), Some(mut front)) = (back, front) else {
                return Err(Fail::User("the tool does not split the body".into()));
            };
            let name = out[i].name.clone();
            match sp.method {
                SplitMethod::TrimSolid => {
                    // Default: the front (the arrow's side) goes.
                    let mut kept = if sp.flip {
                        front.rename_prefix(&format!("{}#2:", sp.id), &format!("{}:", sp.id));
                        front
                    } else {
                        back
                    };
                    kept.name = name;
                    out[i] = kept;
                }
                _ => {
                    back.name = name;
                    front.name = Some(sp.new_body());
                    out[i] = back;
                    out.insert(i + 1, front);
                }
            }
            Ok(out)
        }
    }
}

enum Halves {
    User(String),
    Kernel(String),
}

/// The parts of `s` behind and in front of the tool. Section faces:
/// `<id>:Cut…` behind, `<id>#2:Cut…` in front. A closed sketch profile:
/// outside (behind) and inside (in front).
fn halves(
    s: &ExactSolid,
    tool: &ResolvedTool,
    id: &str,
) -> Result<(Option<ExactSolid>, Option<ExactSolid>), Halves> {
    let front_id = format!("{id}#2");
    if let Some(pl) = tool.side_plane() {
        return exact::split_by_plane(s, pl, id, &front_id).map_err(Halves::Kernel);
    }
    let ResolvedTool::Sketch { plane, chains, .. } = tool else {
        return Err(Halves::User("the tool does not split the body".into()));
    };
    let closed = |c: &Vec<Segment2>| match (c.first(), c.last()) {
        (Some(a), Some(b)) => {
            let (p, q) = (seg_point(a, 0.0), seg_point(b, 1.0));
            (p - q).norm() < 1e-9 * (1.0 + p.coords.norm())
        }
        _ => false,
    };
    if !chains.iter().all(closed) {
        return Err(Halves::User(
            "a solid is split by one straight line or closed profiles of the sketch (other open curves: Split Face only)"
                .into(),
        ));
    }
    // The prism of the closed chains, through the whole body.
    let m = s.measure_full().map_err(Halves::Kernel)?;
    let corners = [m.bbox.0, m.bbox.1];
    let reach = corners
        .iter()
        .flat_map(|a| corners.iter().map(move |b| Point3::new(a.x, b.y, a.z)))
        .chain(corners.iter().copied())
        .map(|p| plane.signed_distance(&p).abs())
        .fold(0.0, Real::max)
        + (m.bbox.1 - m.bbox.0).norm()
        + 1.0;
    let mut prism: Option<ExactSolid> = None;
    for c in chains {
        let outer: Vec<ProfileSegment> = orient_ccw(c)
            .into_iter()
            .enumerate()
            .map(|(k, g)| ProfileSegment {
                geometry: g,
                name: format!("{k}"),
            })
            .collect();
        let prof = exact::profile(&outer, &[]).map_err(Halves::Kernel)?;
        let p = exact::extrude(plane, &prof, -reach, reach, 0.0, id).map_err(Halves::Kernel)?;
        prism = Some(match prism {
            None => p,
            // Nested profiles alternate (a ring: the outer less the inner).
            Some(q) if exact::contains(&q, centre_of(&p)) => exact::boolean(&q, &p, ExactOp::Cut)
                .map_err(Halves::Kernel)?
                .ok_or_else(|| Halves::User("the profiles leave nothing".into()))?,
            Some(q) => exact::boolean(&q, &p, ExactOp::Union)
                .map_err(Halves::Kernel)?
                .ok_or_else(|| Halves::User("the profiles leave nothing".into()))?,
        });
    }
    let prism = prism.ok_or_else(|| Halves::User("the sketch has no closed profile".into()))?;
    let name_cut = |r: Option<ExactSolid>, prefix: &str| {
        r.map(|mut r| {
            for n in r.face_names.iter_mut() {
                if let Some(x) = n.as_ref()
                    && let Some(rest) = x.strip_prefix(&format!("{id}:"))
                {
                    *n = Some(format!("{prefix}:Cut:{rest}"));
                }
            }
            r.name = s.name.clone();
            r
        })
    };
    let back = exact::boolean(s, &prism, ExactOp::Cut).map_err(Halves::Kernel)?;
    let front = if exact::meets(s, &prism) {
        exact::boolean(s, &prism, ExactOp::Intersect).map_err(Halves::Kernel)?
    } else {
        None
    };
    Ok((name_cut(back, id), name_cut(front, &front_id)))
}

/// A point inside the prism's profile (its centroid; the prisms are of
/// simple closed curves).
fn centre_of(p: &ExactSolid) -> Point3 {
    p.measure_full().map_or(Point3::origin(), |m| m.centroid)
}

/// The chain turned counter-clockwise (positive area), as a profile's
/// outer loop must run.
fn orient_ccw(c: &[Segment2]) -> Vec<Segment2> {
    let mut area = 0.0;
    for g in c {
        for k in 0..32 {
            let (p, q) = (seg_point(g, k as Real / 32.0), seg_point(g, (k + 1) as Real / 32.0));
            area += p.x * q.y - q.x * p.y;
        }
    }
    if area >= 0.0 {
        c.to_vec()
    } else {
        c.iter().rev().map(reversed).collect()
    }
}

/// Unit normal of the tool's front side for the preview arrow, and a point
/// to start it at (on the tool, nearest the body's centre).
pub fn arrow(tool: &ResolvedTool, centre: Point3) -> Option<(Point3, Vector3)> {
    let pl = tool.side_plane()?;
    let d = pl.signed_distance(&centre);
    Some((centre - pl.normal.into_inner() * d, pl.normal.into_inner()))
}
