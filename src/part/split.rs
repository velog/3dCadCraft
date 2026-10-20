//! **Split** (Inventor: 3D Model → Modify → Split,
//! `docs/spec/2.13-combine-split.md`): divides faces (Split Face), removes
//! one side of a solid (Trim Solid) or makes two solids of one (Split
//! Solid). The tool is an origin / work plane, a planar face's plane, a 2D
//! sketch or a surface body (Extrude / Revolve with Output Surface): each of
//! its faces' surfaces, extended (the kernel's `SplitTool::Face`).
//!
//! Names (rule 2): pieces of a divided face are `<face>~<id>.<k>` (`k` from
//! the back of the tool); Trim / Split Solid keep every face's name and
//! name the section `<id>:Cut` (the part behind the tool, or the kept part)
//! and `<id>#2:Cut` (the part in front). Split Solid's back part keeps the
//! body's name, the front part is the new body `<id>#2`.
//!
//! Not patternable on its own (Inventor repeats it only within a patterned
//! solid), so it records no `pattern::ToolSet`.

use serde::{Deserialize, Serialize};

use crate::kernel::exact::{self, ExactOp, ExactSolid, SheetSide, SplitBy};
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
    /// A surface body (persistent name) and its sides (its triangles in
    /// the part's B-rep; the front is along their normals).
    Surface {
        /// The body's persistent name.
        name: String,
        /// Which side of it a point is on.
        side: SheetSide,
    },
}

impl ResolvedTool {
    /// The plane whose front side the arrow shows, if the tool is planar.
    pub fn side_plane(&self) -> Option<&Plane> {
        match self {
            ResolvedTool::Plane(p) => Some(p),
            ResolvedTool::Sketch { line, .. } => line.as_ref(),
            ResolvedTool::Surface { .. } => None,
        }
    }

    /// Signed distance of `p` from the tool's front side (planes and
    /// surfaces; `None` for curves).
    pub fn signed(&self, p: &Point3) -> Option<Real> {
        match self {
            ResolvedTool::Surface { side, .. } => Some(side.signed(p)),
            _ => self.side_plane().map(|pl| pl.signed_distance(p)),
        }
    }
}

/// The triangles of the surface body `name` in `body` (an open solid).
fn sheet_side(body: &BRep, name: &str) -> Result<SheetSide, String> {
    let opts = crate::kernel::TessellationOptions::default();
    let mut tris = Vec::new();
    let mut found = false;
    for (_, s) in body.solids() {
        if s.name.as_deref() != Some(name) {
            continue;
        }
        if !s.open {
            return Err(format!("{name} is a solid body, not a surface"));
        }
        found = true;
        for sh in &s.shells {
            for &f in &sh.faces {
                let Ok(m) = body.face_mesh(f, &opts) else {
                    continue;
                };
                for t in &m.triangles {
                    let p = |i: u32| m.positions.get(i as usize).copied();
                    if let (Some(a), Some(b), Some(c)) = (p(t[0]), p(t[1]), p(t[2])) {
                        tris.push([a, b, c]);
                    }
                }
            }
        }
    }
    if !found {
        return Err(format!("the surface {name} no longer exists"));
    }
    let side = SheetSide::new(tris);
    if side.is_empty() {
        return Err(format!("the surface {name} has no area"));
    }
    Ok(side)
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
                return points
                    .first()
                    .copied()
                    .unwrap_or_else(crate::sketch::Point2::origin);
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
        if sk.style(id) == Style::Construction || ps.is_reference(id) {
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
        while let Some(k) = open.iter().position(|(_, a, b)| *a == t || *b == t) {
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
                return Err(format!(
                    "the sketch {} has no curves to split with",
                    ps.name
                ));
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
        Some(SplitToolRef::Surface(name)) => Ok(ResolvedTool::Surface {
            name: name.clone(),
            side: sheet_side(body, name)?,
        }),
    }
}

fn solid_index(cur: &[ExactSolid], name: &str) -> Result<usize, String> {
    if name.is_empty() {
        let solids: Vec<usize> = (0..cur.len()).filter(|&i| !cur[i].is_open()).collect();
        return match solids.as_slice() {
            [] => Err("there is no solid to split".into()),
            [i] => Ok(*i),
            _ => Err("select the solid".into()),
        };
    }
    cur.iter()
        .position(|s| !s.is_open() && s.name.as_deref() == Some(name))
        .ok_or_else(|| format!("the solid {name} no longer exists"))
}

/// The surface body `name` among the exact bodies.
fn sheet_of<'a>(cur: &'a [ExactSolid], name: &str) -> Result<&'a ExactSolid, String> {
    cur.iter()
        .find(|s| s.is_open() && s.name.as_deref() == Some(name))
        .ok_or_else(|| format!("the surface {name} no longer exists"))
}

/// Faces whose box overlaps the surface's box (All Faces with a surface;
/// the kernel then drops those it does not cross).
fn near_sheet(s: &ExactSolid, sheet: &ExactSolid) -> Vec<usize> {
    let Some((lo, hi)) = exact::bounds(sheet) else {
        return Vec::new();
    };
    let eps = 1e-6 * (1.0 + (hi - lo).norm());
    exact::face_corners(s)
        .unwrap_or_default()
        .iter()
        .enumerate()
        .filter(|(_, pts)| {
            !pts.is_empty()
                && (0..3).all(|k| {
                    let a = pts.iter().map(|p| p[k]).fold(Real::INFINITY, Real::min);
                    let b = pts.iter().map(|p| p[k]).fold(Real::NEG_INFINITY, Real::max);
                    a <= hi[k] + eps && b >= lo[k] - eps
                })
        })
        .map(|(i, _)| i)
        .collect()
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

fn split_by(t: &ResolvedTool, cur: &[ExactSolid]) -> Result<SplitBy, String> {
    Ok(match t {
        ResolvedTool::Plane(p) => SplitBy::Plane(p.clone()),
        ResolvedTool::Sketch { line: Some(p), .. } => SplitBy::Plane(p.clone()),
        ResolvedTool::Sketch { plane, chains, .. } => SplitBy::Curves {
            plane: plane.clone(),
            curves: chains.clone(),
        },
        ResolvedTool::Surface { name, .. } => SplitBy::Sheet(sheet_of(cur, name)?.clone()),
    })
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
            let by = split_by(&tool, &out).map_err(Fail::User)?;
            let signed = |p: &Point3| tool.signed(p).unwrap_or(0.0);
            let side: Option<&dyn Fn(&Point3) -> Real> = match &tool {
                ResolvedTool::Sketch { line: None, .. } => None,
                _ => Some(&signed),
            };
            if sp.all_faces {
                let i = solid_index(&out, &sp.solid).map_err(Fail::User)?;
                let faces = match (&tool, &by) {
                    (_, SplitBy::Sheet(sheet)) => near_sheet(&out[i], sheet),
                    (ResolvedTool::Plane(p) | ResolvedTool::Sketch { line: Some(p), .. }, _) => {
                        crossed_by_plane(&out[i], p)
                    }
                    (ResolvedTool::Sketch { plane, chains, .. }, _) => {
                        near_curves(&out[i], plane, chains)
                    }
                    (ResolvedTool::Surface { .. }, _) => Vec::new(),
                };
                let r = exact::split_faces_lenient(&out[i], &faces, &by, &sp.id, side)
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
                out[i] = exact::split_faces(&out[i], &idx, &by, &sp.id, side).map_err(|e| {
                    // Name the face the tool misses.
                    let missed = faces
                        .iter()
                        .find(|(f, _)| e.contains(&format!("does not cross face {f}")));
                    match missed {
                        Some((_, n)) => Fail::User(format!("the tool does not cross the face {n}")),
                        None if e.contains("does not cross") => {
                            Fail::User("the tool does not cross the selected faces".into())
                        }
                        None => kernel(e),
                    }
                })?;
            }
            Ok(out)
        }
        SplitMethod::TrimSolid | SplitMethod::SplitSolid => {
            let i = solid_index(&out, &sp.solid).map_err(Fail::User)?;
            let (back, front) = halves(&out[i], &tool, &out, &sp.id).map_err(|e| match e {
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
    cur: &[ExactSolid],
    id: &str,
) -> Result<(Option<ExactSolid>, Option<ExactSolid>), Halves> {
    let front_id = format!("{id}#2");
    if let ResolvedTool::Surface { name, side } = tool {
        let sheet = sheet_of(cur, name).map_err(Halves::User)?;
        return exact::split_by_sheet(s, sheet, side, id, &front_id).map_err(Halves::Kernel);
    }
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
    let m = s.measure_full().map_err(Halves::Kernel)?;
    // Closed chains as they are; one open chain closed on its front side
    // (left of its direction: sketch normal × tangent) by its tangent
    // extensions and a frame round the body.
    let open: Vec<&Vec<Segment2>> = chains.iter().filter(|c| !closed(c)).collect();
    let loops: Vec<Vec<Segment2>> = match open.as_slice() {
        [] => {
            for c in chains {
                if crosses_itself(&polyline(c, false), true) {
                    return Err(Halves::User(
                        "a closed curve of the sketch crosses itself (it bounds no region to split by)"
                            .into(),
                    ));
                }
            }
            chains.clone()
        }
        [c] if chains.len() == 1 => vec![open_region(plane, c, m.bbox).map_err(Halves::User)?],
        _ => {
            return Err(Halves::User(
                "a solid is split by one open curve chain or by closed profiles (not several open chains, nor both)"
                    .into(),
            ));
        }
    };
    // The prism of the closed chains, through the whole body.
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
    for c in &loops {
        let outer: Vec<ProfileSegment> = orient_ccw(c)
            .into_iter()
            .enumerate()
            .map(|(k, g)| ProfileSegment {
                geometry: g,
                name: format!("{k}"),
            })
            .collect();
        // A loop the kernel finds no area in crosses itself (a figure eight).
        let no_area = |e: String| {
            if e.contains("no area") {
                Halves::User(
                    "a curve of the sketch crosses itself (it bounds no region to split by)".into(),
                )
            } else {
                Halves::Kernel(e)
            }
        };
        let prof = exact::profile(&outer, &[]).map_err(no_area)?;
        let p = exact::extrude(plane, &prof, -reach, reach, 0.0, id).map_err(no_area)?;
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

/// Points on a chain (16 per line or arc, 64 per spline), closed or not.
fn polyline(c: &[Segment2], with_end: bool) -> Vec<crate::sketch::Point2> {
    let mut v = Vec::new();
    for g in c {
        let n = if matches!(g, Segment2::Spline { .. }) {
            64
        } else {
            16
        };
        for k in 0..n {
            v.push(g.point(k as Real / n as Real));
        }
    }
    if with_end && let Some(g) = c.last() {
        v.push(g.point(1.0));
    }
    v
}

/// Whether two segments meet (cross, touch or overlap), `eps` the length
/// below which points coincide.
fn segs_meet(
    a: crate::sketch::Point2,
    b: crate::sketch::Point2,
    c: crate::sketch::Point2,
    d: crate::sketch::Point2,
    eps: Real,
) -> bool {
    let cross = |u: nalgebra::Vector2<Real>, v: nalgebra::Vector2<Real>| u.x * v.y - u.y * v.x;
    // Distance from p to segment qr.
    let dist = |p: crate::sketch::Point2, q: crate::sketch::Point2, r: crate::sketch::Point2| {
        let e = r - q;
        let t = if e.norm_squared() > 0.0 {
            ((p - q).dot(&e) / e.norm_squared()).clamp(0.0, 1.0)
        } else {
            0.0
        };
        (p - (q + e * t)).norm()
    };
    let (d1, d2) = (cross(b - a, c - a), cross(b - a, d - a));
    let (d3, d4) = (cross(d - c, a - c), cross(d - c, b - c));
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
    {
        return true;
    }
    // Touching or overlapping: an end on the other segment.
    dist(a, c, d) < eps || dist(b, c, d) < eps || dist(c, a, b) < eps || dist(d, a, b) < eps
}

/// Whether a polyline (closed: its last point joined to the first) crosses
/// or touches itself away from its neighbouring segments.
fn crosses_itself(pts: &[crate::sketch::Point2], closed: bool) -> bool {
    let n = pts.len();
    if n < 4 {
        return false;
    }
    let size = pts.iter().map(|p| p.coords.norm()).fold(1.0, Real::max);
    let eps = 1e-7 * size;
    let segs = if closed { n } else { n - 1 };
    for i in 0..segs {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        for j in i + 2..segs {
            if closed && (j + 1) % n == i {
                continue;
            }
            let (c, d) = (pts[j], pts[(j + 1) % n]);
            if segs_meet(a, b, c, d, eps) {
                return true;
            }
        }
    }
    false
}

/// The unit direction of a segment leaving its start (`end` false) or
/// arriving at its end (along the control polygon for a spline: a clamped
/// spline's end tangent).
fn seg_dir(g: &Segment2, end: bool) -> Option<nalgebra::Vector2<Real>> {
    let h = 1e-6;
    let (p, q) = if end {
        (g.point(1.0 - h), g.point(1.0))
    } else {
        (g.point(0.0), g.point(h))
    };
    (q - p).try_normalize(1e-15)
}

/// One open chain closed into a region on its front side (on its left, the
/// side of sketch normal × direction, as for a single line): its ends
/// extended along their tangents to a frame round the body (`bbox`, seen in
/// the sketch plane, with a margin), and the frame walked counter-clockwise
/// back to the start. The region is counter-clockwise.
fn open_region(
    plane: &Plane,
    chain: &[Segment2],
    bbox: (Point3, Point3),
) -> Result<Vec<Segment2>, String> {
    use crate::sketch::Point2;
    let (first, last) = match (chain.first(), chain.last()) {
        (Some(a), Some(b)) => (a, b),
        _ => return Err("the sketch has no curves to split with".into()),
    };
    let (p0, p1) = (seg_point(first, 0.0), seg_point(last, 1.0));
    let (Some(t0), Some(t1)) = (seg_dir(first, false), seg_dir(last, true)) else {
        return Err("a curve of the sketch has no length".into());
    };
    let pts = polyline(chain, true);
    let mut lo = Point2::new(Real::INFINITY, Real::INFINITY);
    let mut hi = Point2::new(Real::NEG_INFINITY, Real::NEG_INFINITY);
    for i in 0..8 {
        let c = Point3::new(
            if i & 1 == 0 { bbox.0.x } else { bbox.1.x },
            if i & 2 == 0 { bbox.0.y } else { bbox.1.y },
            if i & 4 == 0 { bbox.0.z } else { bbox.1.z },
        );
        let q = plane.project(&c);
        lo = lo.inf(&q);
        hi = hi.sup(&q);
    }
    for p in &pts {
        lo = lo.inf(p);
        hi = hi.sup(p);
    }
    let margin = 1.0 + 0.1 * (hi - lo).norm();
    lo -= nalgebra::Vector2::new(margin, margin);
    hi += nalgebra::Vector2::new(margin, margin);
    // Where a ray from inside leaves the frame.
    let exit = |p: Point2, d: nalgebra::Vector2<Real>| -> Point2 {
        let mut t = Real::INFINITY;
        for (pc, dc, l, h) in [(p.x, d.x, lo.x, hi.x), (p.y, d.y, lo.y, hi.y)] {
            if dc > 1e-12 {
                t = t.min((h - pc) / dc);
            } else if dc < -1e-12 {
                t = t.min((l - pc) / dc);
            }
        }
        p + d * t.max(0.0)
    };
    let e_out = exit(p1, t1);
    let s_out = exit(p0, -t0);
    // The extended chain must not cross itself (a curve that turns back
    // across its own extension, or a self-crossing curve).
    let mut ext = vec![s_out];
    ext.extend(pts.iter().copied());
    ext.push(e_out);
    ext.dedup_by(|a, b| (*a - *b).norm() < 1e-12);
    let open_crossing = crosses_itself(&ext, false);
    if open_crossing {
        return Err(
            "the split curve crosses itself or its own extension (it cannot divide the body in two)"
                .into(),
        );
    }
    // Perimeter parameter (0..4, counter-clockwise from the low corner).
    let (w, h) = (hi.x - lo.x, hi.y - lo.y);
    let param = |p: Point2| -> Real {
        let eps = 1e-9 * (w + h);
        if (p.y - lo.y).abs() < eps {
            (p.x - lo.x) / w
        } else if (p.x - hi.x).abs() < eps {
            1.0 + (p.y - lo.y) / h
        } else if (p.y - hi.y).abs() < eps {
            2.0 + (hi.x - p.x) / w
        } else {
            3.0 + (hi.y - p.y) / h
        }
    };
    let corner = |j: i64| match j.rem_euclid(4) {
        0 => Point2::new(lo.x, lo.y),
        1 => Point2::new(hi.x, lo.y),
        2 => Point2::new(hi.x, hi.y),
        _ => Point2::new(lo.x, hi.y),
    };
    let (ue, us) = (param(e_out), param(s_out));
    let gap = (us - ue).rem_euclid(4.0);
    let mut walk = vec![e_out];
    let mut j = ue.floor() as i64 + 1;
    while (j as Real) < ue + gap - 1e-9 {
        walk.push(corner(j));
        j += 1;
    }
    walk.push(s_out);
    let mut out: Vec<Segment2> = Vec::new();
    let line = |a: Point2, b: Point2| Segment2::Line { a, b };
    if (s_out - p0).norm() > 1e-9 {
        out.push(line(s_out, p0));
    }
    out.extend(chain.iter().cloned());
    let mut prev = p1;
    for q in walk {
        if (q - prev).norm() > 1e-9 {
            out.push(line(prev, q));
        }
        prev = q;
    }
    // `out` starts at s_out (or p0) and ends there: closed.
    let poly = polyline(&out, false);
    let area: Real = (0..poly.len())
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            a.x * b.y - b.x * a.y
        })
        .sum();
    if area <= 0.0 {
        return Err("the split curve cannot divide the body in two".into());
    }
    Ok(out)
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
            let (p, q) = (
                seg_point(g, k as Real / 32.0),
                seg_point(g, (k + 1) as Real / 32.0),
            );
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
    if let ResolvedTool::Surface { side, .. } = tool {
        return side.nearest(&centre);
    }
    // One open chain: at its middle, towards its front (left) side.
    if let ResolvedTool::Sketch {
        plane,
        chains,
        line: None,
    } = tool
        && let [c] = chains.as_slice()
        && let Some(g) = c.get(c.len() / 2)
    {
        let at = g.point(0.5);
        let t = g.point(0.5 + 1e-6) - at;
        let left = nalgebra::Vector2::new(-t.y, t.x).try_normalize(1e-15)?;
        let p = plane.point_at(at.x, at.y);
        let q = plane.point_at(at.x + left.x, at.y + left.y);
        return Some((p, q - p));
    }
    let pl = tool.side_plane()?;
    let d = pl.signed_distance(&centre);
    Some((centre - pl.normal.into_inner() * d, pl.normal.into_inner()))
}

/// A planar face piece to draft: its name, outward normal, centre and
/// triangle corners.
struct Piece {
    name: Option<String>,
    n: Vector3,
    c: Point3,
    pts: Vec<Point3>,
}

fn piece_of(
    s: &ExactSolid,
    corners: &[Vec<Point3>],
    f: usize,
    pull: Vector3,
) -> Result<Piece, String> {
    let pts = corners
        .get(f)
        .filter(|p| p.len() >= 3)
        .ok_or_else(|| format!("face {f} has no geometry"))?
        .clone();
    let mut n = Vector3::zeros();
    for t in pts.chunks(3) {
        if let [a, b, c] = t {
            n += (b - a).cross(&(c - a));
        }
    }
    let n = n
        .try_normalize(1e-12)
        .ok_or_else(|| "a face to draft is degenerate".to_owned())?;
    let c = Point3::from(pts.iter().map(|p| p.coords).sum::<Vector3>() / pts.len() as Real);
    let size = pts
        .iter()
        .map(|p| (p - c).norm())
        .fold(0.0, Real::max)
        .max(1e-6);
    if pts
        .iter()
        .any(|p| (p - c).dot(&n).abs() > 1e-6 * size.max(1.0))
    {
        return Err("not supported yet: drafting a piece of a curved face".into());
    }
    // The normal pointing out of the material.
    let n = if exact::contains(s, c + n * size * 1e-3) {
        -n
    } else {
        n
    };
    if n.dot(&pull).abs() > 1e-6 {
        return Err("a chosen face isn't parallel to the pull direction".into());
    }
    Ok(Piece {
        name: s.face_names.get(f).cloned().flatten(),
        n,
        c,
        pts,
    })
}

/// Face Draft of pieces of split faces (`<face>~<split>.<k>`). The kernel's
/// draft of a convex body works on whole planes, so it tilted every piece
/// of the face (the volume came out unchanged: one half in, the other out).
/// Each planar piece is drafted on its own instead: a prism along the hinge
/// line (the piece's line on `neutral`) between the old and the tilted
/// plane, over the piece's height along `pull`, is cut (leaning in) or
/// joined (leaning out). The tilted face keeps the piece's name.
pub(super) fn draft_pieces(
    s: &ExactSolid,
    faces: &[usize],
    neutral: &Plane,
    pull: Vector3,
    angle: Real,
    prefix: &str,
) -> Result<ExactSolid, String> {
    let pull = pull
        .try_normalize(1e-12)
        .ok_or_else(|| "the pull direction has no length".to_owned())?;
    let corners = exact::face_corners(s)?;
    // The pieces first (the solid changes with each one).
    let pieces = faces
        .iter()
        .map(|&f| piece_of(s, &corners, f, pull))
        .collect::<Result<Vec<_>, _>>()?;
    let mut out = s.clone();
    let tmp = format!("{prefix}~piece");
    let slant_name = format!("{tmp}:Side:slant");
    for p in pieces {
        let denom = neutral.normal.dot(&pull);
        if denom.abs() < 1e-9 {
            return Err("the neutral plane is parallel to the pull direction".into());
        }
        let hinge = p.c + pull * ((neutral.origin - p.c).dot(&neutral.normal) / denom);
        // Frame: x along the outward normal, y along the pull, normal along
        // the hinge line.
        let z = p.n.cross(&pull);
        let frame = Plane::new(hinge, z, p.n).map_err(|e| e.to_string())?;
        let (mut y0, mut y1, mut z0, mut z1) = (
            Real::INFINITY,
            Real::NEG_INFINITY,
            Real::INFINITY,
            Real::NEG_INFINITY,
        );
        for q in &p.pts {
            let d = q - hinge;
            (y0, y1) = (y0.min(d.dot(&pull)), y1.max(d.dot(&pull)));
            (z0, z1) = (z0.min(d.dot(&z)), z1.max(d.dot(&z)));
        }
        // A positive angle leans in going along the pull: x = −y tan θ.
        let k = angle.tan();
        let xl = |y: Real| -y * k;
        let eps = 1e-9 * (y1 - y0).max(1.0);
        let lean_in = xl(y0) < -eps || xl(y1) < -eps;
        let lean_out = xl(y0) > eps || xl(y1) > eps;
        if lean_in && lean_out {
            return Err(
                "not supported yet: a piece crossing the neutral plane (split it there)".into(),
            );
        }
        if !lean_in && !lean_out {
            continue;
        }
        let reach = (y1 - y0).max(z1 - z0) * 2.0 + 1.0;
        let pt = crate::sketch::Point2::new;
        let quad = if lean_in {
            [pt(xl(y0), y0), pt(reach, y0), pt(reach, y1), pt(xl(y1), y1)]
        } else {
            [pt(0.0, y0), pt(xl(y0), y0), pt(xl(y1), y1), pt(0.0, y1)]
        };
        // The slanted side: from (xl(y1), y1) to (xl(y0), y0) when cutting,
        // (xl(y0), y0) → (xl(y1), y1) when adding.
        let slant = if lean_in { 3 } else { 1 };
        let mut segs: Vec<ProfileSegment> = (0..4)
            .map(|i| ProfileSegment {
                geometry: Segment2::Line {
                    a: quad[i],
                    b: quad[(i + 1) % 4],
                },
                name: if i == slant {
                    "slant".into()
                } else {
                    format!("e{i}")
                },
            })
            .filter(|s| match &s.geometry {
                Segment2::Line { a, b } => (b - a).norm() > 1e-9,
                _ => true,
            })
            .collect();
        let area: Real = (0..4)
            .map(|i| {
                let (a, b) = (quad[i], quad[(i + 1) % 4]);
                a.x * b.y - b.x * a.y
            })
            .sum();
        if area < 0.0 {
            segs = segs
                .into_iter()
                .rev()
                .map(|s| ProfileSegment {
                    geometry: reversed(&s.geometry),
                    name: s.name,
                })
                .collect();
        }
        let prof = exact::profile(&segs, &[])?;
        // Cuts reach past the piece sideways (a tool face never ends on an
        // edge of the result); additions stay within it.
        let (za, zb) = if lean_in {
            (z0 - reach, z1 + reach)
        } else {
            (z0, z1)
        };
        let tool = exact::extrude(&frame, &prof, za, zb, 0.0, &tmp)?;
        let op = if lean_in {
            ExactOp::Cut
        } else {
            ExactOp::Union
        };
        let mut r = exact::boolean(&out, &tool, op)?
            .ok_or_else(|| "the draft leaves nothing".to_owned())?;
        for n in r.face_names.iter_mut() {
            if n.as_deref() == Some(slant_name.as_str()) {
                *n = p.name.clone();
            } else if let Some(x) = n.as_ref().filter(|x| x.starts_with(&tmp)) {
                *n = Some(x.replacen(&tmp, prefix, 1));
            }
        }
        r.name = out.name.clone();
        out = r;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sketch::Point2;

    fn spline(pts: &[(Real, Real)]) -> Segment2 {
        Segment2::Spline {
            points: pts.iter().map(|p| Point2::new(p.0, p.1)).collect(),
            reversed: false,
            knots: Vec::new(),
        }
    }

    #[test]
    fn a_figure_eight_crosses_itself() {
        let g = spline(&[
            (14.0, 0.0),
            (10.0, -8.0),
            (-10.0, 8.0),
            (-14.0, 0.0),
            (-10.0, -8.0),
            (10.0, 8.0),
            (14.0, 0.0),
        ]);
        let pts = polyline(std::slice::from_ref(&g), false);
        assert!(crosses_itself(&pts, true), "{pts:?}");
        // A circle does not.
        let c = Segment2::Arc {
            c: Point2::origin(),
            r: 5.0,
            a0: 0.0,
            sweep: std::f64::consts::TAU,
        };
        assert!(!crosses_itself(&polyline(&[c], false), true));
    }

    #[test]
    fn an_open_line_closes_on_its_left() {
        // Along +X at y = 0 across a 40 × 30 box: the region is above.
        let line = Segment2::Line {
            a: Point2::new(-5.0, 0.0),
            b: Point2::new(5.0, 0.0),
        };
        let bbox = (
            Point3::new(-20.0, -15.0, 0.0),
            Point3::new(20.0, 15.0, 20.0),
        );
        let r = open_region(&Plane::xy(), &[line], bbox).unwrap();
        let pts = polyline(&r, false);
        assert!(pts.iter().all(|p| p.y >= -1e-9), "{pts:?}");
        assert!(pts.iter().any(|p| p.y > 15.0));
        assert!(!crosses_itself(&pts, true));
    }
}
