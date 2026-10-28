//! The surface commands of 3D Model › Surface (`docs/spec/5.4-surfaces.md`) as
//! [`BodyOp`](super::BodyOp) variants (format revision 37): **Stitch**, **Boundary Patch**,
//! **Trim**, **Extend**, **Ruled Surface**, **Replace Face** and **Sculpt**.
//!
//! References follow rule 2: bodies by persistent name, faces as [`FaceRef`] (name + a point),
//! edges as [`SurfEdge`] (the name of a face along the edge + a point on the edge). New
//! surface bodies are named after the feature (`Stitch1`, `Patch1`); faces carried over keep
//! their names, new ones are `<id>:Face`, `<id>:Patch`, `<id>:Side:<k>`.

use serde::{Deserialize, Serialize};

use super::exact::Fail::{self, Kernel, User};
use super::extrude::ProfileRef;
use super::placement::{PlaneRef, WorkGeom};
use super::shell::FaceRef;
use super::work::LineRef;
use crate::kernel::exact::{self, ExactSolid, surface_ops};
use crate::kernel::{BRep, Plane, Point3, Real, Vector3};

/// An edge of the model: the name of a face along it and a point on it (model coordinates).
/// A surface's free edge has one face; that face's name and the point find it again.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SurfEdge {
    /// Persistent name of a face along the edge.
    pub face: String,
    /// A point on the edge when it was picked.
    pub hint: [Real; 3],
}

impl SurfEdge {
    fn point(&self) -> Point3 {
        Point3::new(self.hint[0], self.hint[1], self.hint[2])
    }
}

fn default_tolerance() -> Real {
    0.00025
}

/// **Stitch**: surface bodies sewn along the edges they share into one body (a solid when they
/// close a volume, unless Keep as Surface).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StitchOp {
    /// The surface bodies, by name.
    pub surfaces: Vec<String>,
    /// Maximum Tolerance (mm): edges closer than this are one.
    #[serde(default = "default_tolerance")]
    pub tolerance: Real,
    /// Keep as Surface: a closed result stays a surface body.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub keep_surface: bool,
}

/// **Boundary Patch**: a surface inside a closed loop of model edges, or a sketch region.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PatchOp {
    /// The loop of model edges (any order).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edges: Vec<SurfEdge>,
    /// A sketch (by id) and its region, instead of edges.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sketch: Option<String>,
    /// The region of `sketch`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<ProfileRef>,
    /// Output Solid: the patch is sewn to the surface whose edges bound it, and the result
    /// becomes a solid when that closes it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub solid: bool,
}

/// What trims a surface.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TrimTool {
    /// A work plane, an origin plane or a planar face.
    Plane(PlaneRef),
    /// Another surface body (its first face, extended), by name.
    Surface(String),
    /// The curves of a sketch (by id), swept along the sketch's normal.
    Sketch(String),
}

/// **Trim**: a surface body cut by a tool; the piece the user picked goes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrimOp {
    /// The surface body trimmed, by name.
    pub surface: String,
    /// The cutting tool.
    pub tool: TrimTool,
    /// A point on the piece to remove (model coordinates).
    pub remove: [Real; 3],
    /// Invert Selection: keep that piece and remove the rest.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub invert: bool,
}

/// **Extend**: edges of a planar surface moved outward in its plane.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExtendOp {
    /// The edges (straight edges of one planar surface).
    pub edges: Vec<SurfEdge>,
    /// Distance (mm).
    pub distance: Real,
    /// Extents To: each edge goes up to this plane instead (along the surface).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<PlaneRef>,
}

/// Ruled Surface types.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuledKind {
    /// Square to the face the edge bounds.
    #[default]
    Normal,
    /// In the face's plane, outward (tangent to a planar face).
    Tangent,
    /// Along a direction.
    Sweep,
}

/// **Ruled Surface**: a strip of `distance` from model edges.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RuledOp {
    /// Normal, Tangent or Sweep (stored as `ruled_type`: `kind` is the operation's tag).
    #[serde(default, rename = "ruled_type")]
    pub kind: RuledKind,
    /// The edges (each with the face whose side decides the direction).
    pub edges: Vec<SurfEdge>,
    /// Distance (mm).
    pub distance: Real,
    /// The other side.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub flip: bool,
    /// The direction of a Sweep.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vector: Option<LineRef>,
    /// Normal: the strip leans this far (radians) from the face's normal, outward.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub angle: Real,
    /// Alternate All Faces: each edge's other face decides its strip.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub alternate: bool,
}

fn is_zero(v: &Real) -> bool {
    *v == 0.0
}

/// What a replaced face becomes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ReplaceWith {
    /// A surface body, by name.
    Surface(String),
    /// A plane (work plane, origin plane, planar face).
    Plane(PlaneRef),
}

/// **Replace Face**: planar faces of a solid replaced by a surface; the body grows or shrinks
/// to it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReplaceFaceOp {
    /// The faces replaced.
    pub faces: Vec<FaceRef>,
    /// The new face.
    pub new: ReplaceWith,
}

/// Sculpt operations.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SculptOperation {
    /// Material is added (joined).
    #[default]
    Add,
    /// Material is removed.
    Remove,
    /// The bounded volume becomes a new solid.
    NewSolid,
}

/// **Sculpt**: material added or removed where surfaces bound a volume with the solid.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SculptOp {
    /// The surface bodies, by name.
    pub surfaces: Vec<String>,
    /// Add, Remove or New Solid.
    #[serde(default)]
    pub operation: SculptOperation,
    /// The solid (empty: the first one).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub solids: Vec<String>,
    /// Remove with a surface that crosses the solid: a point on the side that goes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<[Real; 3]>,
    /// Without `side`: the side behind the surface goes instead of the one it faces.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub flip: bool,
}

impl StitchOp {
    /// Refuses a definition that cannot be built.
    pub fn validate(&self) -> Result<(), String> {
        if self.surfaces.len() < 2 {
            return Err("select at least two surfaces to stitch".into());
        }
        if !(self.tolerance.is_finite() && self.tolerance > 0.0) {
            return Err("the tolerance must be positive".into());
        }
        Ok(())
    }
}

impl PatchOp {
    /// Refuses a definition that cannot be built.
    pub fn validate(&self) -> Result<(), String> {
        match (&self.sketch, self.edges.is_empty()) {
            (None, true) => Err("select a closed loop of edges or a sketch region".into()),
            (Some(_), _) if self.profile.is_none() => Err("select the sketch region".into()),
            _ => Ok(()),
        }
    }
}

impl TrimOp {
    /// The sketch it uses as the cutting tool.
    pub fn sketch(&self) -> Option<&str> {
        match &self.tool {
            TrimTool::Sketch(s) => Some(s),
            _ => None,
        }
    }

    /// Refuses a definition that cannot be built.
    pub fn validate(&self) -> Result<(), String> {
        if self.surface.is_empty() {
            return Err("select the piece of the surface to remove".into());
        }
        Ok(())
    }
}

impl ExtendOp {
    /// Refuses a definition that cannot be built.
    pub fn validate(&self) -> Result<(), String> {
        if self.edges.is_empty() {
            return Err("select the edges to extend".into());
        }
        if self.to.is_none() && !(self.distance.is_finite() && self.distance > 0.0) {
            return Err("the distance must be positive".into());
        }
        Ok(())
    }
}

impl RuledOp {
    /// Refuses a definition that cannot be built.
    pub fn validate(&self) -> Result<(), String> {
        if self.edges.is_empty() {
            return Err("select the edges".into());
        }
        if !(self.distance.is_finite() && self.distance > 0.0) {
            return Err("the distance must be positive".into());
        }
        if self.kind == RuledKind::Sweep && self.vector.is_none() {
            return Err("select the sweep direction".into());
        }
        Ok(())
    }
}

impl ReplaceFaceOp {
    /// Refuses a definition that cannot be built.
    pub fn validate(&self) -> Result<(), String> {
        if self.faces.is_empty() {
            return Err("select the faces to replace".into());
        }
        Ok(())
    }
}

impl SculptOp {
    /// Refuses a definition that cannot be built.
    pub fn validate(&self) -> Result<(), String> {
        if self.surfaces.is_empty() {
            return Err("select the surfaces that bound the volume".into());
        }
        Ok(())
    }
}

// ---------------------------------------------------------------- geometry ---

fn p(h: &[Real; 3]) -> Point3 {
    Point3::new(h[0], h[1], h[2])
}

/// The surface body called `name` (index in `sheets`).
fn sheet_named(sheets: &[ExactSolid], name: &str) -> Result<usize, Fail> {
    sheets
        .iter()
        .position(|s| s.name.as_deref() == Some(name))
        .ok_or_else(|| User(format!("the surface {name} no longer exists")))
}

/// The point of an edge of face `face` nearest `hint` (the edge the user picked, found again
/// on the current body).
fn on_edge(body: &BRep, e: &SurfEdge) -> Option<Point3> {
    let hint = e.point();
    let opts = crate::kernel::TessellationOptions::default();
    let mut best: Option<(Real, Point3)> = None;
    for (_, f) in body
        .faces()
        .filter(|(_, f)| f.name.as_deref() == Some(e.face.as_str()))
    {
        for lp in std::iter::once(&f.outer).chain(&f.inner) {
            for ce in &lp.coedges {
                let Ok(pts) = body.sample_edge(ce.edge, &opts) else {
                    continue;
                };
                for w in pts.windows(2) {
                    let (a, b) = (w[0], w[1]);
                    let ab = b - a;
                    let t = ((hint - a).dot(&ab) / ab.norm_squared().max(1e-24)).clamp(0.0, 1.0);
                    let q = a + ab * t;
                    let d = (q - hint).norm();
                    if best.is_none_or(|(x, _)| d < x) {
                        best = Some((d, q));
                    }
                }
            }
        }
    }
    best.map(|(_, q)| q)
}

/// The straight edge of a face named like `e`'s nearest `at` (its ends) and the centre of that
/// face's mesh.
fn edge_ends(body: &BRep, e: &SurfEdge, at: &Point3) -> Option<(Point3, Point3, Point3)> {
    let opts = crate::kernel::TessellationOptions::default();
    let mut best: Option<(Real, Point3, Point3, Point3)> = None;
    for (_, f) in body
        .faces()
        .filter(|(_, f)| f.name.as_deref() == Some(e.face.as_str()))
    {
        let m = f.mesh.as_ref()?;
        let c = m
            .positions
            .iter()
            .fold(Vector3::zeros(), |s, q| s + q.coords)
            / m.positions.len().max(1) as Real;
        for lp in std::iter::once(&f.outer).chain(&f.inner) {
            for ce in &lp.coedges {
                let Ok(pts) = body.sample_edge(ce.edge, &opts) else {
                    continue;
                };
                let (Some(a), Some(b)) = (pts.first(), pts.last()) else {
                    continue;
                };
                let ab = b - a;
                let t = ((at - a).dot(&ab) / ab.norm_squared().max(1e-24)).clamp(0.0, 1.0);
                let d = (a + ab * t - at).norm();
                if best.is_none_or(|x| d < x.0) {
                    best = Some((d, *a, *b, Point3::from(c)));
                }
            }
        }
    }
    best.map(|(_, a, b, c)| (a, b, c))
}

/// The body (solid or surface) holding the edges: the one with a face of the first edge's
/// name nearest its hint.
fn edge_body(all: &[ExactSolid], edges: &[SurfEdge]) -> Result<usize, Fail> {
    let first = edges
        .first()
        .ok_or_else(|| User("select the edges".into()))?;
    exact::faces_in_solids(all, &[(first.face.as_str(), first.point())])
        .pop()
        .flatten()
        .map(|(b, _)| b)
        .ok_or_else(|| User(format!("the face {} no longer exists", first.face)))
}

fn edge_points(body: &BRep, edges: &[SurfEdge]) -> Result<Vec<Point3>, Fail> {
    edges
        .iter()
        .map(|e| {
            on_edge(body, e).ok_or_else(|| User(format!("an edge of {} no longer exists", e.face)))
        })
        .collect()
}

/// Stitch: the surfaces sewn; the result replaces them.
pub(super) fn stitch(
    op: &StitchOp,
    id: &str,
    cur: &[ExactSolid],
    sheets: &[ExactSolid],
) -> Result<Vec<ExactSolid>, Fail> {
    op.validate().map_err(User)?;
    let idx: Vec<usize> = op
        .surfaces
        .iter()
        .map(|n| sheet_named(sheets, n))
        .collect::<Result<_, _>>()?;
    let parts: Vec<&ExactSolid> = idx.iter().map(|&i| &sheets[i]).collect();
    // The kernel puts surfaces that meet nowhere into one body too (side by side); Inventor
    // stitches only what meets within the tolerance.
    let edges: Vec<Vec<[Point3; 2]>> = parts.iter().map(|s| edge_segments(s)).collect();
    for (k, mine) in edges.iter().enumerate() {
        let meets = edges.iter().enumerate().any(|(j, other)| {
            j != k
                && mine.iter().any(|a| {
                    other.iter().any(|b| {
                        let d = |q: &Point3, s: &[Point3; 2]| {
                            let ab = s[1] - s[0];
                            let t = ((q - s[0]).dot(&ab) / ab.norm_squared().max(1e-24))
                                .clamp(0.0, 1.0);
                            (s[0] + ab * t - q).norm()
                        };
                        // Overlapping edges: an end or the middle of one lies on the other.
                        let m = nalgebra::center(&a[0], &a[1]);
                        [a[0], a[1], m]
                            .iter()
                            .filter(|q| d(q, b) <= op.tolerance)
                            .count()
                            >= 2
                    })
                })
        });
        if !meets {
            return Err(User(format!(
                "{} does not meet the other surfaces within the tolerance ({} mm)",
                op.surfaces[k], op.tolerance
            )));
        }
    }
    let sewn = surface_ops::stitch(&parts, op.tolerance, op.keep_surface, id).map_err(|e| {
        User(format!(
            "the surfaces do not meet within the tolerance ({e})"
        ))
    })?;
    let mut out: Vec<ExactSolid> = cur.to_vec();
    out.extend(
        sheets
            .iter()
            .enumerate()
            .filter(|(i, _)| !idx.contains(i))
            .map(|(_, s)| s.clone()),
    );
    out.push(sewn);
    Ok(out)
}

/// The edges of a body as straight pieces (the display samples of each edge).
fn edge_segments(s: &ExactSolid) -> Vec<[Point3; 2]> {
    let Ok(b) = exact::to_brep(std::slice::from_ref(s)) else {
        return Vec::new();
    };
    let opts = crate::kernel::TessellationOptions::default();
    b.edges()
        .filter_map(|(e, _)| b.sample_edge(e, &opts).ok())
        .flat_map(|pts| pts.windows(2).map(|w| [w[0], w[1]]).collect::<Vec<_>>())
        .collect()
}

/// Boundary Patch: a new surface body.
pub(super) fn patch(
    op: &PatchOp,
    id: &str,
    body: &BRep,
    sketches: &[super::PartSketch],
    cur: &[ExactSolid],
    sheets: &[ExactSolid],
) -> Result<Vec<ExactSolid>, Fail> {
    op.validate().map_err(User)?;
    let made = if let (Some(sid), Some(pr)) = (&op.sketch, &op.profile) {
        let ps = sketches
            .iter()
            .find(|s| &s.id == sid)
            .ok_or_else(|| User(format!("sketch {sid} is missing")))?;
        let regions = ps.sketch.regions();
        let region = pr
            .resolve(&regions)
            .ok_or_else(|| User("the sketch region no longer exists".into()))?;
        let (outer, holes) = super::extrude::kernel_profile(region);
        let prof = exact::profile(&outer, &holes).map_err(Kernel)?;
        surface_ops::patch_region(&ps.sketch.plane, &prof, id).map_err(User)?
    } else {
        let all: Vec<ExactSolid> = cur.iter().chain(sheets).cloned().collect();
        let b = edge_body(&all, &op.edges)?;
        let pts = edge_points(body, &op.edges)?;
        let patch = surface_ops::patch_edges(&all[b], &pts, id)
            .map_err(|e| User(format!("the edges do not bound a patch ({e})")))?;
        if op.solid {
            // Sewn to the surface it closes: that surface is replaced by the result.
            if !all[b].is_open() {
                return Err(User("Output Solid: the edges must be a surface's".into()));
            }
            let mut sewn =
                surface_ops::stitch(&[&all[b], &patch], 1e-4, false, id).map_err(User)?;
            if sewn.is_open() {
                return Err(User(
                    "Output Solid: the patch does not close the surface (other openings remain)"
                        .into(),
                ));
            }
            if sewn.volume_at(1e-3).is_some_and(|v| v < 0.0) {
                sewn = surface_ops::reversed(&sewn).map_err(User)?;
            }
            sewn.name = all[b].name.clone();
            let mut out = all;
            out[b] = sewn;
            // Solids before surface bodies.
            out.sort_by_key(|s| s.is_open());
            return Ok(out);
        }
        patch
    };
    let mut out: Vec<ExactSolid> = cur.iter().chain(sheets).cloned().collect();
    out.push(made);
    Ok(out)
}

/// Trim: the surface replaced by what is left of it.
pub(super) fn trim(
    op: &TrimOp,
    body: &BRep,
    works: &WorkGeom,
    sketches: &[super::PartSketch],
    cur: &[ExactSolid],
    sheets: &[ExactSolid],
) -> Result<Vec<ExactSolid>, Fail> {
    op.validate().map_err(User)?;
    let i = sheet_named(sheets, &op.surface)?;
    let remove = p(&op.remove);
    // The kernel keeps a side: the other side of the tool from the removed piece.
    let (trimmed, kept) = match &op.tool {
        TrimTool::Plane(r) => {
            let pl = super::placement::plane_of(r, body, works).map_err(|e| User(e.to_string()))?;
            let n = pl.normal.into_inner();
            let d = (remove - pl.origin).dot(&n);
            if d.abs() < 1e-9 {
                return Err(User("pick the piece to remove off the cutting tool".into()));
            }
            let keep = if op.invert {
                remove
            } else {
                remove - n * (2.0 * d)
            };
            (
                surface_ops::trim(&sheets[i], &surface_ops::TrimBy::Plane(&pl), keep),
                keep,
            )
        }
        TrimTool::Sketch(sid) => {
            let ps = sketches
                .iter()
                .find(|s| &s.id == sid)
                .ok_or_else(|| User(format!("sketch {sid} is missing")))?;
            let sk = &ps.sketch;
            // Every curve of the sketch (not construction, not projected), one chain each.
            let chains: Vec<Vec<crate::kernel::ProfileSegment>> = sk
                .entities()
                .filter(|(e, _)| {
                    !ps.is_reference(*e) && sk.style(*e) == crate::sketch::Style::Normal
                })
                .filter_map(|(e, _)| {
                    let op = super::OpenProfile {
                        curves: vec![e],
                        flip: false,
                        match_shape: false,
                    };
                    super::extrude::open_chain(&op, sk).ok().map(|c| c.0)
                })
                .collect();
            if chains.is_empty() {
                return Err(User("the sketch has no curve to cut with".into()));
            }
            let pl = &sk.plane;
            // The kept side: across the curve nearest the removed piece, in the sketch's plane.
            let q = pl.project(&remove);
            let mut best: Option<(Real, nalgebra::Vector2<Real>)> = None;
            for c in &chains {
                for s in c {
                    for k in 0..=16 {
                        let a = s.geometry.point(k as Real / 16.0);
                        let d = (q - a).norm();
                        if best.is_none_or(|(x, _)| d < x) {
                            best = Some((d, q - a));
                        }
                    }
                }
            }
            let (d, v) = best.ok_or_else(|| User("the sketch has no curve".into()))?;
            if d < 1e-9 {
                return Err(User(
                    "pick the piece to remove off the cutting curve".into(),
                ));
            }
            let w = pl.x_axis.into_inner() * v.x + pl.y_axis().into_inner() * v.y;
            let keep = if op.invert { remove } else { remove - w * 2.0 };
            (
                surface_ops::trim(&sheets[i], &surface_ops::TrimBy::Curves(pl, &chains), keep),
                keep,
            )
        }
        TrimTool::Surface(name) => {
            let t = sheet_named(sheets, name)?;
            if t == i {
                return Err(User("a surface cannot trim itself".into()));
            }
            let side = exact::SheetSide::new(sheets[t].triangles().map_err(Kernel)?);
            let d = side.signed(&remove);
            if d.abs() < 1e-9 {
                return Err(User("pick the piece to remove off the cutting tool".into()));
            }
            let n = side.nearest(&remove).map_or_else(Vector3::z, |(_, n)| n);
            let keep = if op.invert {
                remove
            } else {
                remove - n * (2.0 * d)
            };
            (
                surface_ops::trim(&sheets[i], &surface_ops::TrimBy::Face(&sheets[t], 0), keep),
                keep,
            )
        }
    };
    // A tool that runs exactly along the seams of the surface's pieces (a plane through a
    // cylinder's axis, where the kernel's two halves meet) cuts no face: the pieces on the
    // other side go whole.
    let trimmed = match trimmed {
        Ok(t) => t,
        Err(e) => {
            let side = |q: &Point3| -> Option<Real> {
                match &op.tool {
                    TrimTool::Plane(r) => {
                        let pl = super::placement::plane_of(r, body, works).ok()?;
                        Some((q - pl.origin).dot(&pl.normal.into_inner()))
                    }
                    TrimTool::Surface(name) => {
                        let t = sheet_named(sheets, name).ok()?;
                        let side = exact::SheetSide::new(sheets[t].triangles().ok()?);
                        Some(side.signed(q))
                    }
                    TrimTool::Sketch(_) => None,
                }
            };
            let fail = || User(format!("the tool does not cut the surface ({e})"));
            let keep_side = side(&kept).ok_or_else(fail)?;
            let s = &sheets[i];
            let size = s.size().max(1.0);
            let mut gone = Vec::new();
            for f in 0..surface_ops::face_count(s) {
                let q = exact::face_point(s, f).ok_or_else(fail)?;
                let x = side(&q).ok_or_else(fail)?;
                if x.abs() < 1e-6 * size {
                    return Err(fail()); // a face on the tool: really not cut
                }
                if (x > 0.0) != (keep_side > 0.0) {
                    gone.push(f);
                }
            }
            if gone.is_empty() || gone.len() == surface_ops::face_count(s) {
                return Err(fail());
            }
            exact::delete_faces(s, &gone, false).map_err(|_| fail())?
        }
    };
    let mut out: Vec<ExactSolid> = cur.to_vec();
    for (k, s) in sheets.iter().enumerate() {
        out.push(if k == i { trimmed.clone() } else { s.clone() });
    }
    Ok(out)
}

/// Extend: the surface replaced by its extended copy.
pub(super) fn extend(
    op: &ExtendOp,
    body: &BRep,
    works: &WorkGeom,
    cur: &[ExactSolid],
    sheets: &[ExactSolid],
) -> Result<Vec<ExactSolid>, Fail> {
    op.validate().map_err(User)?;
    let i = edge_body(sheets, &op.edges)?;
    let pts = edge_points(body, &op.edges)?;
    let fail = |e: String| {
        User(format!(
            "only straight edges of a planar surface extend yet ({e})"
        ))
    };
    let grown = match &op.to {
        None => surface_ops::extend(&sheets[i], &pts, op.distance).map_err(fail)?,
        Some(r) => {
            // Up to a plane: each edge by its own distance, along the surface away from it.
            let target =
                super::placement::plane_of(r, body, works).map_err(|e| User(e.to_string()))?;
            let tn = target.normal.into_inner();
            let mut s = sheets[i].clone();
            for (e, at) in op.edges.iter().zip(&pts) {
                let (a, b, centre) = edge_ends(body, e, at)
                    .ok_or_else(|| User(format!("an edge of {} no longer exists", e.face)))?;
                let mid = nalgebra::center(&a, &b);
                let along = (b - a)
                    .try_normalize(1e-12)
                    .ok_or_else(|| User("a degenerate edge".into()))?;
                let away = mid - centre;
                let out = (away - along * away.dot(&along))
                    .try_normalize(1e-12)
                    .ok_or_else(|| User("cannot tell the outward side of an edge".into()))?;
                let den = out.dot(&tn);
                if den.abs() < 1e-9 {
                    return Err(User(
                        "the edge runs parallel to the plane: it never reaches it".into(),
                    ));
                }
                let d = (target.origin - mid).dot(&tn) / den;
                if d <= 1e-9 {
                    return Err(User(
                        "the plane lies behind an edge: Extend lengthens only".into(),
                    ));
                }
                s = surface_ops::extend(&s, std::slice::from_ref(at), d).map_err(fail)?;
            }
            s
        }
    };
    let mut out: Vec<ExactSolid> = cur.to_vec();
    for (k, s) in sheets.iter().enumerate() {
        out.push(if k == i { grown.clone() } else { s.clone() });
    }
    Ok(out)
}

/// Ruled Surface: a new surface body from edges of planar faces.
pub(super) fn ruled(
    op: &RuledOp,
    id: &str,
    body: &BRep,
    works: &WorkGeom,
    cur: &[ExactSolid],
    sheets: &[ExactSolid],
) -> Result<Vec<ExactSolid>, Fail> {
    op.validate().map_err(User)?;
    let opts = crate::kernel::TessellationOptions::default();
    // Each edge with the face that decides its strip: the picked face, or the edge's other
    // face with Alternate All Faces.
    let mut items: Vec<(Plane, crate::kernel::EdgeId, Point3, Point3, Vector3)> = Vec::new();
    for e in &op.edges {
        let at = on_edge(body, e).ok_or_else(|| User("an edge no longer exists".into()))?;
        // The edge of the face nearest `at`.
        let mut best: Option<(Real, crate::kernel::EdgeId)> = None;
        for (_, f) in body
            .faces()
            .filter(|(_, f)| f.name.as_deref() == Some(e.face.as_str()))
        {
            for lp in std::iter::once(&f.outer).chain(&f.inner) {
                for ce in &lp.coedges {
                    if let Ok(pts) = body.sample_edge(ce.edge, &opts) {
                        // Distance to the edge's segments, not its samples: a
                        // straight edge samples as its two ends only.
                        let d = pts
                            .windows(2)
                            .map(|w| {
                                let ab = w[1] - w[0];
                                let t = ((at - w[0]).dot(&ab) / ab.norm_squared().max(1e-24))
                                    .clamp(0.0, 1.0);
                                (w[0] + ab * t - at).norm()
                            })
                            .fold(Real::MAX, Real::min);
                        if best.is_none_or(|(x, _)| d < x) {
                            best = Some((d, ce.edge));
                        }
                    }
                }
            }
        }
        let (_, eid) = best.ok_or_else(|| User("an edge no longer exists".into()))?;
        let edge = body
            .edge(eid)
            .ok_or_else(|| User("an edge no longer exists".into()))?;
        let (Some(a), Some(b)) = (
            body.vertex(edge.start).map(|v| v.point),
            body.vertex(edge.end).map(|v| v.point),
        ) else {
            return Err(User("an edge no longer exists".into()));
        };
        let face = body
            .edge_faces(eid)
            .into_iter()
            .filter_map(|f| body.face(f))
            .find(|f| (f.name.as_deref() == Some(e.face.as_str())) != op.alternate)
            .ok_or_else(|| User("an edge has no other face".into()))?;
        let plane = match body.surface(face.surface) {
            Some(crate::kernel::Surface::Plane(pl)) if face.facet_of.is_none() => pl.clone(),
            _ => {
                return Err(User("Ruled Surface: only edges of planar faces yet".into()));
            }
        };
        let n = plane.normal.into_inner();
        // Outward in the face's plane: away from the face's mesh centre.
        let centre = face
            .mesh
            .as_ref()
            .map(|m| {
                m.positions
                    .iter()
                    .fold(Vector3::zeros(), |s, q| s + q.coords)
                    / m.positions.len().max(1) as Real
            })
            .unwrap_or_else(|| plane.origin.coords);
        let mid3 = nalgebra::center(&a, &b);
        let out = (mid3.coords - centre) - n * (mid3.coords - centre).dot(&n);
        let out = out.try_normalize(1e-12).unwrap_or_else(Vector3::x);
        items.push((plane, eid, a, b, out));
    }
    // Edges of one plane make one strip; the strips of other planes are stitched to it.
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for (i, (pl, ..)) in items.iter().enumerate() {
        let same = |j: usize| {
            let q = &items[j].0;
            pl.normal.dot(&q.normal) > 1.0 - 1e-9
                && (pl.origin - q.origin).dot(&q.normal).abs()
                    <= 1e-6 * (1.0 + (pl.origin - q.origin).norm())
        };
        match groups.iter_mut().find(|g| same(g[0])) {
            Some(g) => g.push(i),
            None => groups.push(vec![i]),
        }
    }
    let mut parts = Vec::new();
    for g in &groups {
        let plane = &items[g[0]].0;
        let mut segs = Vec::new();
        let mut outward = Vec::new();
        for &i in g {
            let (_, eid, a, b, out) = &items[i];
            segs.push(crate::kernel::ProfileSegment {
                geometry: edge_in_plane(body, *eid, a, b, plane, &opts)?,
                name: format!("{i}"),
            });
            outward.push(*out);
        }
        match ruled_strip(op, id, body, works, plane, segs.clone(), &outward) {
            Ok(s) => parts.push(s),
            // Edges that are not one chain end to end: a strip each, stitched below.
            Err(_) if segs.len() > 1 => {
                for (s, o) in segs.into_iter().zip(outward) {
                    parts.push(ruled_strip(op, id, body, works, plane, vec![s], &[o])?);
                }
            }
            Err(e) => return Err(e),
        }
    }
    let made = if parts.len() == 1 {
        parts.pop().ok_or_else(|| User("no edges".into()))?
    } else {
        let refs: Vec<&ExactSolid> = parts.iter().collect();
        surface_ops::stitch(&refs, 1e-4, true, id).map_err(User)?
    };
    let mut out: Vec<ExactSolid> = cur.iter().chain(sheets).cloned().collect();
    out.push(made);
    Ok(out)
}

/// One edge as a sketch segment in `plane` (it lies in it).
fn edge_in_plane(
    body: &BRep,
    eid: crate::kernel::EdgeId,
    a: &Point3,
    b: &Point3,
    plane: &Plane,
    opts: &crate::kernel::TessellationOptions,
) -> Result<crate::kernel::Segment2, Fail> {
    use crate::kernel::{Curve, Segment2};
    let n = plane.normal.into_inner();
    for q in [a, b] {
        if (q - plane.origin).dot(&n).abs() > 1e-6 * (1.0 + (q - plane.origin).norm()) {
            return Err(User(
                "Ruled Surface: the edges must lie in one plane".into(),
            ));
        }
    }
    let to2 = |q: &Point3| {
        let v = q - plane.origin;
        nalgebra::Point2::new(
            v.dot(&plane.x_axis.into_inner()),
            v.dot(&plane.y_axis().into_inner()),
        )
    };
    let edge = body
        .edge(eid)
        .ok_or_else(|| User("an edge no longer exists".into()))?;
    Ok(match body.curve(edge.curve) {
        Some(Curve::Line(_)) => Segment2::Line {
            a: to2(a),
            b: to2(b),
        },
        Some(Curve::Circle(c)) => {
            let c2 = to2(&c.center);
            let (pa, pb) = (to2(a), to2(b));
            let a0 = (pa - c2).y.atan2((pa - c2).x);
            let a1 = (pb - c2).y.atan2((pb - c2).x);
            // The sweep that passes the edge's middle.
            let mid = body
                .sample_edge(eid, opts)
                .ok()
                .and_then(|pts| pts.get(pts.len() / 2).copied())
                .map(|q| to2(&q));
            let ccw = (a1 - a0).rem_euclid(std::f64::consts::TAU);
            let ccw = if ccw < 1e-9 {
                std::f64::consts::TAU
            } else {
                ccw
            };
            let sweep = match mid {
                Some(m) => {
                    let am = (m - c2).y.atan2((m - c2).x);
                    if (am - a0).rem_euclid(std::f64::consts::TAU) <= ccw + 1e-9 {
                        ccw
                    } else {
                        ccw - std::f64::consts::TAU
                    }
                }
                None => ccw,
            };
            Segment2::Arc {
                c: c2,
                r: (pa - c2).norm(),
                a0,
                sweep,
            }
        }
        _ => {
            return Err(User("Ruled Surface: lines and arcs only yet".into()));
        }
    })
}

/// The strip of the edges `segs` lying in `plane` (`outward`: each edge's outward direction).
fn ruled_strip(
    op: &RuledOp,
    id: &str,
    body: &BRep,
    works: &WorkGeom,
    plane: &Plane,
    segs: Vec<crate::kernel::ProfileSegment>,
    outward: &[Vector3],
) -> Result<ExactSolid, Fail> {
    use crate::kernel::{ProfileSegment, Segment2};
    let n = plane.normal.into_inner();
    let sign = if op.flip { -1.0 } else { 1.0 };
    Ok(match op.kind {
        RuledKind::Normal if op.angle == 0.0 => {
            exact::sweep_sheet_along(plane, &[segs], n * (op.distance * sign), id).map_err(User)?
        }
        RuledKind::Normal => {
            // Leaning outward: each edge along its own direction, the strips stitched.
            let (c, s) = (op.angle.cos(), op.angle.sin());
            let mut parts = Vec::new();
            for (g, o) in segs.iter().zip(outward) {
                let dir = (n * c + o * s) * (op.distance * sign);
                parts.push(
                    exact::sweep_sheet_along(plane, &[vec![g.clone()]], dir, id).map_err(User)?,
                );
            }
            if parts.len() == 1 {
                parts.pop().ok_or_else(|| User("no edges".into()))?
            } else {
                let refs: Vec<&ExactSolid> = parts.iter().collect();
                surface_ops::stitch(&refs, 1e-4, true, id).map_err(User)?
            }
        }
        RuledKind::Sweep => {
            let v = op
                .vector
                .as_ref()
                .ok_or_else(|| User("select the sweep direction".into()))?;
            let (_, d) = v.resolve(body, works).map_err(User)?;
            let d = d
                .try_normalize(1e-12)
                .ok_or_else(|| User("the direction has no length".into()))?;
            if d.dot(&n).abs() < 1e-6 {
                return Err(User("the sweep direction lies in the edges' plane".into()));
            }
            exact::sweep_sheet_along(plane, &[segs], d * (op.distance * sign), id).map_err(User)?
        }
        RuledKind::Tangent => {
            // In the plane, outward: one straight edge at a time (a strip).
            let mut parts = Vec::new();
            for (k, (s, o)) in segs.iter().zip(outward).enumerate() {
                let Segment2::Line { a, b } = s.geometry else {
                    return Err(User(
                        "Ruled Surface Tangent: straight edges only yet".into(),
                    ));
                };
                let o2 = nalgebra::Vector2::new(
                    o.dot(&plane.x_axis.into_inner()),
                    o.dot(&plane.y_axis().into_inner()),
                ) * (op.distance * sign);
                let quad = [a, b, b + o2, a + o2];
                let lp: Vec<ProfileSegment> = (0..4)
                    .map(|j| ProfileSegment {
                        geometry: Segment2::Line {
                            a: quad[j],
                            b: quad[(j + 1) % 4],
                        },
                        name: format!("{k}.{j}"),
                    })
                    .collect();
                // Counter-clockwise for the kernel's region.
                let area: Real = (0..4)
                    .map(|j| quad[j].x * quad[(j + 1) % 4].y - quad[(j + 1) % 4].x * quad[j].y)
                    .sum();
                let lp = if area < 0.0 {
                    let mut r: Vec<ProfileSegment> = lp
                        .into_iter()
                        .rev()
                        .map(|s| ProfileSegment {
                            geometry: super::extrude::reverse_segment(&s.geometry),
                            name: s.name,
                        })
                        .collect();
                    r.rotate_left(0);
                    r
                } else {
                    lp
                };
                let prof = exact::profile(&lp, &[]).map_err(Kernel)?;
                parts.push(surface_ops::patch_region(plane, &prof, id).map_err(User)?);
            }
            if parts.len() == 1 {
                parts.pop().ok_or_else(|| User("no edges".into()))?
            } else {
                let refs: Vec<&ExactSolid> = parts.iter().collect();
                surface_ops::stitch(&refs, 1e-4, true, id).map_err(User)?
            }
        }
    })
}

/// The body index in `cur` of the solid named in `solids` (or the first).
fn target_solid(cur: &[ExactSolid], solids: &[String]) -> Result<usize, Fail> {
    match solids.first() {
        Some(n) => cur
            .iter()
            .position(|s| s.name.as_deref() == Some(n.as_str()))
            .ok_or_else(|| User(format!("the body {n} no longer exists"))),
        None if !cur.is_empty() => Ok(0),
        None => Err(User("there is no solid".into())),
    }
}

/// Sculpt: the volume the surfaces close (alone, or with a planar patch of their open rim)
/// joined to / cut from the solid; a surface that crosses the solid instead removes the side
/// of `side`.
pub(super) fn sculpt(
    op: &SculptOp,
    id: &str,
    cur: &[ExactSolid],
    sheets: &[ExactSolid],
) -> Result<Vec<ExactSolid>, Fail> {
    op.validate().map_err(User)?;
    let idx: Vec<usize> = op
        .surfaces
        .iter()
        .map(|n| sheet_named(sheets, n))
        .collect::<Result<_, _>>()?;
    let rest: Vec<ExactSolid> = sheets
        .iter()
        .enumerate()
        .filter(|(i, _)| !idx.contains(i))
        .map(|(_, s)| s.clone())
        .collect();
    let parts: Vec<&ExactSolid> = idx.iter().map(|&i| &sheets[i]).collect();
    // One body of the surfaces; closed by patches of its open rims.
    let mut quilt = if parts.len() == 1 {
        parts[0].clone()
    } else {
        surface_ops::stitch(&parts, 1e-3, false, id).map_err(User)?
    };

    if quilt.is_open()
        && let Some(closed) = close_rims(&quilt, id)
    {
        quilt = closed;
    }
    let t = target_solid(cur, &op.solids);
    let mut out: Vec<ExactSolid> = cur.to_vec();
    if !quilt.is_open() {
        let mut tool = quilt;
        tool.name = Some(id.to_owned());
        match op.operation {
            SculptOperation::NewSolid => out.push(tool),
            SculptOperation::Add | SculptOperation::Remove => {
                let t = t?;
                let how = if op.operation == SculptOperation::Add {
                    exact::ExactOp::Union
                } else {
                    exact::ExactOp::Cut
                };
                let r = exact::boolean(&out[t], &tool, how)
                    .map_err(Kernel)?
                    .ok_or_else(|| User("nothing is left".into()))?;
                out[t] = r;
            }
        }
    } else {
        // An open surface across the solid: Remove takes the side of `side`.
        if op.operation != SculptOperation::Remove {
            return Err(User(
                "the surfaces do not close a volume: use Remove with a surface across the body, or close them"
                    .into(),
            ));
        }
        let t = t?;
        let side = exact::SheetSide::new(quilt.triangles().map_err(Kernel)?);
        let (back, front) = exact::split_by_sheet(
            &out[t],
            &quilt,
            &side,
            &format!("{id}#b"),
            &format!("{id}#f"),
        )
        .map_err(User)?;
        // The side the surface faces goes, unless a point says otherwise.
        let remove_front = match op.side {
            Some(h) => side.signed(&p(&h)) > 0.0,
            None => !op.flip,
        };
        let kept = if remove_front { back } else { front };
        let mut kept = kept.ok_or_else(|| User("nothing is left on the other side".into()))?;
        kept.name = out[t].name.clone();
        out[t] = kept;
    }
    out.extend(rest);
    Ok(out)
}

/// The point half way along a polyline (a straight edge samples as its two ends).
fn middle(pts: &[Point3]) -> Option<Point3> {
    let len: Real = pts.windows(2).map(|w| (w[1] - w[0]).norm()).sum();
    let mut run = 0.0;
    for w in pts.windows(2) {
        let l = (w[1] - w[0]).norm();
        if run + l >= len / 2.0 && l > 0.0 {
            return Some(w[0] + (w[1] - w[0]) * ((len / 2.0 - run) / l));
        }
        run += l;
    }
    pts.first().copied()
}

/// A surface body closed by planar patches over its open rims, when every rim is a planar
/// loop (a dome on a face); `None` otherwise.
fn close_rims(s: &ExactSolid, id: &str) -> Option<ExactSolid> {
    let b = exact::to_brep(std::slice::from_ref(s)).ok()?;
    let map = b.edge_face_map();
    let free: Vec<crate::kernel::EdgeId> = map
        .iter()
        .filter(|(_, fs)| fs.len() == 1)
        .map(|(e, _)| *e)
        .collect();
    if free.is_empty() {
        return None;
    }
    // The free edges in loops (chained through shared vertices).
    let ends = |e: crate::kernel::EdgeId| b.edge(e).map(|x| (x.start, x.end));
    let mut left = free;
    let mut loops: Vec<Vec<crate::kernel::EdgeId>> = Vec::new();
    while let Some(first) = left.pop() {
        let mut lp = vec![first];
        let (start, mut at) = ends(first)?;
        while at != start {
            let k = left
                .iter()
                .position(|&e| ends(e).is_some_and(|(a, c)| a == at || c == at));
            let k = k?;
            let e = left.remove(k);
            let (a, c) = ends(e)?;
            at = if a == at { c } else { a };
            lp.push(e);
        }
        loops.push(lp);
    }
    let opts = crate::kernel::TessellationOptions::default();
    let mut patches = Vec::new();
    for lp in &loops {
        let pts: Vec<Point3> = lp
            .iter()
            .filter_map(|e| b.sample_edge(*e, &opts).ok().and_then(|q| middle(&q)))
            .collect();
        let pr = surface_ops::patch_edges(s, &pts, id);
        patches.push(pr.ok()?);
    }
    let mut parts: Vec<&ExactSolid> = vec![s];
    parts.extend(patches.iter());
    let patch = patches.first()?.clone();
    let closed = surface_ops::stitch(&parts, 1e-4, false, id);
    let closed = closed.ok()?;
    if closed.is_open() {
        return None;
    }
    let v = closed.volume_at(1e-3).unwrap_or(0.0);
    // A flat sheet "closed" by a copy of itself encloses nothing.
    if v.abs() < 1e-4 * s.size().powi(3) {
        return None;
    }
    let closed = if v < 0.0 {
        surface_ops::reversed(&closed).ok()?
    } else {
        closed
    };
    // The patch lies on the face the surface stands on (a dome on a top): a thin slab under
    // it, glued to the patch, keeps the kernel's Boolean off that coplanar face.
    let v = v.abs();
    if patches.len() != 1 {
        return Some(closed);
    }
    let depth = (s.size() * 1e-2).max(1e-3);
    for d in [depth, -depth] {
        let Ok(slab) = exact::thicken_sheet(&patch, d, false, None, &format!("{id}~slab")) else {
            continue;
        };
        let Ok(Some(t)) = exact::boolean(&closed, &slab, exact::ExactOp::Union) else {
            continue;
        };
        // The slab on the far side of the patch adds its own volume.
        let slab_v = slab.volume_at(1e-3).unwrap_or(0.0).abs();
        if t.volume_at(1e-3)
            .is_some_and(|tv| (tv - v - slab_v).abs() < 0.05 * slab_v)
        {
            return Some(t);
        }
    }
    Some(closed)
}

/// Replace Face: each face replaced by the new surface — the material beyond it within the
/// face's outline goes, the space up to it within the outline is filled.
pub(super) fn replace_face(
    op: &ReplaceFaceOp,
    id: &str,
    body: &BRep,
    works: &WorkGeom,
    cur: &[ExactSolid],
    sheets: &[ExactSolid],
) -> Result<Vec<ExactSolid>, Fail> {
    op.validate().map_err(User)?;
    let refs: Vec<(&str, Point3)> = op
        .faces
        .iter()
        .map(|f| (f.face.as_str(), p(&f.hint)))
        .collect();
    let found = exact::faces_in_solids(cur, &refs);
    let mut out: Vec<ExactSolid> = cur.to_vec();
    // The new face as a cutting sheet.
    let sheet: ExactSolid = match &op.new {
        ReplaceWith::Surface(n) => sheets[sheet_named(sheets, n)?].clone(),
        ReplaceWith::Plane(r) => {
            let pl = super::placement::plane_of(r, body, works).map_err(|e| User(e.to_string()))?;
            // A large square of the plane.
            let size = body
                .bounding_box()
                .map_or(1000.0, |(lo, hi)| (hi - lo).norm() * 4.0 + 10.0);
            let h = size / 2.0;
            let c = [(-h, -h), (h, -h), (h, h), (-h, h)];
            let lp: Vec<crate::kernel::ProfileSegment> = (0..4)
                .map(|j| crate::kernel::ProfileSegment {
                    geometry: crate::kernel::Segment2::Line {
                        a: nalgebra::Point2::new(c[j].0, c[j].1),
                        b: nalgebra::Point2::new(c[(j + 1) % 4].0, c[(j + 1) % 4].1),
                    },
                    name: format!("{j}"),
                })
                .collect();
            let prof = exact::profile(&lp, &[]).map_err(Kernel)?;
            surface_ops::patch_region(&pl, &prof, &format!("{id}~plane")).map_err(Kernel)?
        }
    };
    let side = exact::SheetSide::new(sheet.triangles().map_err(Kernel)?);
    for (k, f) in op.faces.iter().enumerate() {
        let (b, face) =
            found[k].ok_or_else(|| User(format!("the face {} no longer exists", f.face)))?;
        let _ = face;
        let s = out[b].clone();
        let (pl, outer, holes) = face_profile(body, f)?;
        let outline = exact::profile(&outer, &holes).map_err(Kernel)?;
        // The prism of the face's outline, from deep inside to far beyond.
        let reach = body
            .bounding_box()
            .map_or(1000.0, |(lo, hi)| (hi - lo).norm() * 2.0 + 10.0);
        let prism = exact::extrude(&pl, &outline, -reach, reach, 0.0, &format!("{id}~prism"))
            .map_err(Kernel)?;
        let (back, front) = exact::split_by_sheet(
            &prism,
            &sheet,
            &side,
            &format!("{id}~b"),
            &format!("{id}~f"),
        )
        .map_err(User)?;
        // The part of the prism on the body's side of the new face (where the face's own
        // inside lies) is kept; the outer part is cut away.
        let inside = pl.origin - pl.normal.into_inner() * (reach * 0.5);
        let inside_front = side.signed(&inside) > 0.0;
        let (keep, beyond) = if inside_front {
            (front, back)
        } else {
            (back, front)
        };
        let mut r = s;
        if let Some(beyond) = beyond {
            r = match exact::boolean(&r, &beyond, exact::ExactOp::Cut).map_err(Kernel)? {
                Some(x) => x,
                None => return Err(User("the new face takes the whole body".into())),
            };
        }
        if let Some(keep) = keep {
            // Only the part beyond the old face: the prism above the face's plane.
            let above = exact::extrude(&pl, &outline, 0.0, reach, 0.0, &format!("{id}~up"))
                .map_err(Kernel)?;
            if let Some(fill) =
                exact::boolean(&keep, &above, exact::ExactOp::Intersect).map_err(Kernel)?
            {
                r = exact::boolean(&r, &fill, exact::ExactOp::Union)
                    .map_err(Kernel)?
                    .unwrap_or(r);
            }
        }
        r.name = out[b].name.clone();
        // The new face (a piece of the cutting prism) is the feature's face.
        let tmp = format!("{id}~");
        for n in r.face_names.iter_mut().flatten() {
            if n.starts_with(&tmp) {
                *n = format!("{id}:Face");
            }
        }
        out[b] = r;
    }
    let mut all = out;
    all.extend(sheets.iter().cloned());
    Ok(all)
}

/// A sketch segment in `plane` of edge `eid`, run the way `forward` says (lines and circle
/// arcs; a whole circle is one turn).
fn edge_segment(
    body: &BRep,
    eid: crate::kernel::EdgeId,
    plane: &crate::kernel::Plane,
    forward: bool,
) -> Result<crate::kernel::Segment2, Fail> {
    use crate::kernel::{Curve, Segment2};
    let to2 = |q: &Point3| {
        let v = q - plane.origin;
        nalgebra::Point2::new(
            v.dot(&plane.x_axis.into_inner()),
            v.dot(&plane.y_axis().into_inner()),
        )
    };
    let gone = || User("an edge of the face no longer exists".into());
    let edge = body.edge(eid).ok_or_else(gone)?;
    let a = body.vertex(edge.start).ok_or_else(gone)?.point;
    let b = body.vertex(edge.end).ok_or_else(gone)?.point;
    let seg = match body.curve(edge.curve) {
        Some(Curve::Line(_)) => Segment2::Line {
            a: to2(&a),
            b: to2(&b),
        },
        Some(Curve::Circle(c)) => {
            let c2 = to2(&c.center);
            let pa = to2(&a);
            let a0 = (pa - c2).y.atan2((pa - c2).x);
            let pts = body
                .sample_edge(eid, &crate::kernel::TessellationOptions::default())
                .map_err(|_| gone())?;
            // The signed sweep from the samples (start → end along the edge).
            let mut sweep = 0.0;
            for w in pts.windows(2) {
                let (u, v) = (to2(&w[0]) - c2, to2(&w[1]) - c2);
                sweep += u.perp(&v).atan2(u.dot(&v));
            }
            Segment2::Arc {
                c: c2,
                r: (pa - c2).norm(),
                a0,
                sweep,
            }
        }
        _ => {
            return Err(User(
                "Replace Face: faces bounded by lines and arcs only yet".into(),
            ));
        }
    };
    Ok(if forward {
        seg
    } else {
        super::extrude::reverse_segment(&seg)
    })
}

/// A plane and the outer loop and holes of a face in it.
type FaceLoops = (
    crate::kernel::Plane,
    Vec<crate::kernel::ProfileSegment>,
    Vec<Vec<crate::kernel::ProfileSegment>>,
);

/// A planar face of the display body (by name, the piece nearest the hint) as a plane whose
/// normal points out of the material and its loops in that plane (outer counter-clockwise).
fn face_profile(body: &BRep, f: &FaceRef) -> Result<FaceLoops, Fail> {
    use crate::kernel::{ProfileSegment, Sense, Surface};
    let hint = p(&f.hint);
    let face = body
        .faces()
        .filter(|(_, x)| x.name.as_deref() == Some(f.face.as_str()))
        .min_by(|a, b| {
            let d = |x: &crate::kernel::Face| {
                x.mesh.as_ref().map_or(Real::MAX, |m| {
                    m.positions
                        .iter()
                        .map(|q| (q - hint).norm())
                        .fold(Real::MAX, Real::min)
                })
            };
            d(a.1).total_cmp(&d(b.1))
        })
        .map(|(_, x)| x)
        .ok_or_else(|| User(format!("the face {} no longer exists", f.face)))?;
    let Some(Surface::Plane(pl)) = body.surface(face.surface) else {
        return Err(User("Replace Face: planar faces only yet".into()));
    };
    if face.facet_of.is_some() {
        return Err(User("Replace Face: planar faces only yet".into()));
    }
    let n = match face.sense {
        Sense::Same => pl.normal.into_inner(),
        Sense::Reversed => -pl.normal.into_inner(),
    };
    let plane = crate::kernel::Plane::new(pl.origin, n, pl.x_axis.into_inner())
        .map_err(|e| Kernel(e.to_string()))?;
    let lp = |l: &crate::kernel::Loop| -> Result<Vec<ProfileSegment>, Fail> {
        l.coedges
            .iter()
            .enumerate()
            .map(|(k, ce)| {
                Ok(ProfileSegment {
                    geometry: edge_segment(body, ce.edge, &plane, ce.sense == Sense::Same)?,
                    name: format!("{k}"),
                })
            })
            .collect()
    };
    let area = |l: &[ProfileSegment]| -> Real {
        let pts: Vec<nalgebra::Point2<Real>> = l
            .iter()
            .flat_map(|s| (0..8).map(move |k| s.geometry.point(k as Real / 8.0)))
            .collect();
        (0..pts.len())
            .map(|i| {
                let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                a.x * b.y - b.x * a.y
            })
            .sum::<Real>()
            / 2.0
    };
    let orient = |l: Vec<ProfileSegment>, ccw: bool| -> Vec<ProfileSegment> {
        if (area(&l) > 0.0) == ccw {
            l
        } else {
            l.into_iter()
                .rev()
                .map(|s| ProfileSegment {
                    geometry: super::extrude::reverse_segment(&s.geometry),
                    name: s.name,
                })
                .collect()
        }
    };
    let outer = orient(lp(&face.outer)?, true);
    let holes = face
        .inner
        .iter()
        .map(|l| lp(l).map(|x| orient(x, false)))
        .collect::<Result<Vec<_>, _>>()?;
    Ok((plane, outer, holes))
}
