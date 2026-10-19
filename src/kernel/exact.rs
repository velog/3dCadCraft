//! The exact kernel boundary: solids built by SolidCraft's kernel
//! (`solidcraft-kernel`, MIT OR Apache-2.0, on truck), see `docs/SOLIDCRAFT.md`.
//!
//! Every call goes through `guarded` (a panic in the third-party code
//! becomes an error) and no type of that kernel leaves this module: the
//! rest of the program sees [`ExactSolid`] and the [`BRep`] that
//! [`to_brep`] makes from it — named faces, edges and vertices, exact
//! planes, cylinders, cones, spheres and tori, and the exact kernel's own
//! display triangles per face.
//!
//! Names: faces get theirs from the operation's exact history
//! (`<feature>:Top`, `<feature>:Side:<curve>`, `<feature>:Start` / `End`,
//! inherited through Booleans, `<feature>:Round:<edge>` for blends). Edges
//! and vertices are named after the faces they bound (a box keeps its corner
//! codes, `Box1:V111`, `Box1:E000-100`).

use std::collections::HashMap;
use std::sync::Arc;

use solidcraft_geom as sg;
use solidcraft_kernel as sk;
use solidcraft_kernel::history::{FaceOrigin, History};

use super::extrude::Segment2;
use super::mesh::TriangleMesh;
use super::{
    BRep, Circle, Coedge, Cone, Curve, Cylinder, FaceId, Line, Loop, Plane, Point3, Real, Sense,
    Shell, Solid, Sphere, Surface, Torus, Vector3, VertexId,
};

/// A solid of the exact kernel with the persistent names of its faces (in
/// the kernel's face order).
#[derive(Clone)]
pub struct ExactSolid {
    /// Body name (the creating feature's id).
    pub name: Option<String>,
    body: sk::Body,
    /// Face names, by face index.
    pub face_names: Vec<Option<String>>,
    /// The profiles of the extrusions in its history, for vertex and edge
    /// names (`<prefix>:VS:<segment>`, `<prefix>:StartEdge:<segment>`).
    profiles: Vec<ProfileFrame>,
}

/// An extruded profile: its feature prefix, sketch plane and the start
/// point of each named segment (sketch coordinates).
#[derive(Clone, Debug)]
struct ProfileFrame {
    prefix: String,
    plane: Plane,
    starts: Vec<(String, nalgebra::Point2<Real>)>,
}

impl std::fmt::Debug for ExactSolid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExactSolid")
            .field("name", &self.name)
            .field("faces", &self.face_names.len())
            .finish()
    }
}

/// Runs a call into the exact kernel; a panic becomes an error.
fn guarded<T>(what: &str, f: impl FnOnce() -> sk::Result<T>) -> Result<T, String> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err(format!("{what}: internal error in the geometry kernel")),
    }
}

fn v3(p: &Point3) -> sg::Vec3 {
    sg::Vec3::new(p.x, p.y, p.z)
}

fn d3(v: &Vector3) -> sg::Vec3 {
    sg::Vec3::new(v.x, v.y, v.z)
}

fn p3(v: sg::Vec3) -> Point3 {
    Point3::new(v.x, v.y, v.z)
}

fn w3(v: sg::Vec3) -> Vector3 {
    Vector3::new(v.x, v.y, v.z)
}

fn plane_of(p: &Plane) -> Result<sg::Plane, String> {
    sg::Plane::new(v3(&p.origin), d3(&p.x_axis), d3(&p.y_axis()))
        .ok_or_else(|| "degenerate sketch plane".to_owned())
}

/// Boolean operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExactOp {
    /// Union.
    Union,
    /// `a − b`.
    Cut,
    /// Common part.
    Intersect,
}

/// Measurements of an exact solid (from the kernel's own tessellation).
#[derive(Clone, Debug, PartialEq)]
pub struct ExactMeasure {
    /// Volume (mm³).
    pub volume: Real,
    /// Surface area (mm²).
    pub area: Real,
    /// Centre of mass at uniform density.
    pub centroid: Point3,
    /// Axis-aligned bounding box (min, max).
    pub bbox: (Point3, Point3),
    /// Faces after merging pieces of one surface (as a user counts them).
    pub faces: usize,
    /// Connected pieces (lumps).
    pub lumps: usize,
}

impl ExactSolid {
    /// Volume, area, centre, box, merged face count and lumps.
    pub fn measure_full(&self) -> Result<ExactMeasure, String> {
        let m = guarded("measure", || sk::measure(&self.body))?;
        let lumps = guarded("lumps", || self.body.lumps()).map_or(1, |v| v.len().max(1));
        Ok(ExactMeasure {
            volume: m.volume,
            area: m.area,
            centroid: p3(m.centroid),
            bbox: (p3(m.bbox.min), p3(m.bbox.max)),
            faces: m.merged.faces,
            lumps,
        })
    }

    /// Faces (name, area, centroid, planar normal) and edge midpoints with
    /// their lengths, for diagnosing kernel results in tests.
    #[doc(hidden)]
    pub fn debug_topology(&self) -> String {
        let mut out = String::new();
        if let Ok(fs) = guarded("faces", || self.body.faces(1e-3)) {
            for f in fs {
                out += &format!(
                    "F{} {:?} area {:.2} c {:?} n {:?}\n",
                    f.index,
                    self.face_names.get(f.index).cloned().flatten(),
                    f.area,
                    f.centroid,
                    f.plane_normal
                );
            }
        }
        if let Ok(es) = guarded("edges", || self.body.edges(1e-3)) {
            for e in es {
                out += &format!("E{} mid {:?} len {:.2}\n", e.index, e.mid, e.length);
            }
        }
        out
    }

    /// Volume, area and face count (for tests and properties).
    pub fn measure(&self) -> Result<(Real, Real, usize), String> {
        let m = guarded("measure", || sk::measure(&self.body))?;
        Ok((m.volume, m.area, m.faces))
    }

    /// The faces' names, in face order.
    pub fn names(&self) -> &[Option<String>] {
        &self.face_names
    }
}

/// Names of a result's faces from the history (or, without one, from the
/// geometric trace) against its operands' names. Generated faces get
/// `generated(origin)`.
fn names_from(
    result: &sk::Body,
    operands: &[&ExactSolid],
    generated: &dyn Fn(&FaceOrigin) -> Option<String>,
) -> Vec<Option<String>> {
    let traced;
    let hist: Option<&History> = match result.history() {
        Some(h) => Some(h),
        None => {
            let bodies: Vec<&sk::Body> = operands.iter().map(|o| &o.body).collect();
            traced = guarded("trace", || sk::provenance::trace(result, &bodies)).ok();
            traced.as_ref()
        }
    };
    let n = result.face_count();
    let mut out = vec![None; n];
    if let Some(h) = hist {
        for (i, fp) in h.faces.iter().enumerate().take(n) {
            // The first named origin wins (merged faces keep one name).
            out[i] = fp.origins.iter().find_map(|o| match o {
                FaceOrigin::Face { operand, face } => operands
                    .get(*operand)
                    .and_then(|s| s.face_names.get(*face).cloned().flatten()),
                other => generated(other),
            });
        }
    }
    out
}

// ------------------------------------------------------------- builders ---

/// A box between two corners, faces named `<prefix>:Top` … like
/// [`BRep::make_box_named`].
pub fn box_named(a: Point3, b: Point3, prefix: &str) -> Result<ExactSolid, String> {
    let body = guarded("box", || sk::box_solid(v3(&a), v3(&b)))?;
    let (lo, hi) = (a.inf(&b), a.sup(&b));
    let mid = nalgebra::center(&lo, &hi);
    let faces = guarded("faces", || body.faces(1e-3))?;
    let face_names = faces
        .iter()
        .map(|f| {
            let c = f.centroid;
            let d = Vector3::new(c.x - mid.x, c.y - mid.y, c.z - mid.z);
            let s = hi - lo;
            let rel = [d.x / s.x, d.y / s.y, d.z / s.z];
            let k = (0..3)
                .max_by(|&i, &j| rel[i].abs().total_cmp(&rel[j].abs()))
                .unwrap_or(2);
            let role = match (k, rel[k] > 0.0) {
                (0, false) => "Left",
                (0, true) => "Right",
                (1, false) => "Front",
                (1, true) => "Back",
                (2, false) => "Bottom",
                _ => "Top",
            };
            Some(format!("{prefix}:{role}"))
        })
        .collect();
    Ok(ExactSolid {
        name: Some(prefix.to_owned()),
        body,
        face_names,
        profiles: Vec::new(),
    })
}

/// A profile region in the exact kernel's form, with its segments' names
/// (outer loop first, then holes, as the kernel numbers them).
pub struct ExactProfile {
    region: sg::Region2,
    names: Vec<String>,
    starts: Vec<(String, nalgebra::Point2<Real>)>,
}

/// Converts a profile (our segments) to the exact kernel's region.
pub fn profile(
    outer: &[super::extrude::ProfileSegment],
    holes: &[Vec<super::extrude::ProfileSegment>],
) -> Result<ExactProfile, String> {
    let mut names = Vec::new();
    let mut starts = Vec::new();
    let mut conv = |lp: &[super::extrude::ProfileSegment]| -> Result<sg::Loop2, String> {
        let mut segs = Vec::new();
        for s in lp {
            starts.push((s.name.clone(), s.geometry.start()));
            let pieces = seg2(&s.geometry)?;
            for _ in &pieces {
                names.push(s.name.clone());
            }
            segs.extend(pieces);
        }
        Ok(sg::Loop2 { segs })
    };
    let outer = conv(outer)?;
    let holes = holes.iter().map(|h| conv(h)).collect::<Result<_, _>>()?;
    Ok(ExactProfile {
        region: sg::Region2 { outer, holes },
        names,
        starts,
    })
}

fn v2(p: &nalgebra::Point2<Real>) -> sg::Vec2 {
    sg::Vec2::new(p.x, p.y)
}

/// One of our profile segments as exact-kernel segments (a B-spline as its
/// Bézier pieces).
fn seg2(g: &Segment2) -> Result<Vec<sg::Seg2>, String> {
    Ok(match g {
        Segment2::Line { a, b } => vec![sg::Seg2::Line { a: v2(a), b: v2(b) }],
        Segment2::Arc { c, r, a0, sweep } => vec![sg::Seg2::Arc {
            center: v2(c),
            radius: *r,
            start: *a0,
            sweep: *sweep,
        }],
        Segment2::Spline {
            points,
            reversed,
            knots,
        } => {
            let n = points.len();
            let deg = if knots.is_empty() {
                3.min(n.saturating_sub(1)).max(1)
            } else {
                knots.len().saturating_sub(n + 1).max(1)
            };
            let knots: Vec<Real> = if knots.is_empty() {
                // Clamped uniform.
                let inner = n.saturating_sub(deg + 1);
                let mut k = vec![0.0; deg + 1];
                for i in 1..=inner {
                    k.push(i as Real / (inner + 1) as Real);
                }
                k.extend(std::iter::repeat_n(1.0, deg + 1));
                k
            } else {
                knots.clone()
            };
            let mut pieces = bezier_pieces(deg, points, &knots)?
                .into_iter()
                .map(|cp| {
                    let pts: Vec<sg::Vec2> = cp.iter().map(v2).collect();
                    sg::bezier(&pts).ok_or_else(|| "spline degree too high".to_owned())
                })
                .collect::<Result<Vec<_>, _>>()?;
            if *reversed {
                pieces.reverse();
                pieces = pieces.into_iter().map(reverse_seg).collect();
            }
            pieces
        }
    })
}

fn reverse_seg(s: sg::Seg2) -> sg::Seg2 {
    match s {
        sg::Seg2::Line { a, b } => sg::Seg2::Line { a: b, b: a },
        sg::Seg2::Arc {
            center,
            radius,
            start,
            sweep,
        } => sg::Seg2::Arc {
            center,
            radius,
            start: start + sweep,
            sweep: -sweep,
        },
        sg::Seg2::Cubic { p0, p1, p2, p3 } => sg::Seg2::Cubic {
            p0: p3,
            p1: p2,
            p2: p1,
            p3: p0,
        },
        sg::Seg2::Conic { a, apex, b, w } => sg::Seg2::Conic {
            a: b,
            apex,
            b: a,
            w,
        },
        sg::Seg2::Bezier { n, p } => {
            let mut q = p;
            let k = n as usize;
            q[..=k].reverse();
            sg::Seg2::Bezier { n, p: q }
        }
    }
}

/// Bézier control polygons of a clamped B-spline (Boehm knot insertion until
/// every inner knot has multiplicity `deg`).
fn bezier_pieces(
    deg: usize,
    cps: &[nalgebra::Point2<Real>],
    knots: &[Real],
) -> Result<Vec<Vec<nalgebra::Point2<Real>>>, String> {
    let mut p: Vec<nalgebra::Point2<Real>> = cps.to_vec();
    let mut t: Vec<Real> = knots.to_vec();
    if t.len() != p.len() + deg + 1 {
        return Err("spline knots do not match its points".into());
    }
    let (lo, hi) = (t[deg], t[t.len() - deg - 1]);
    // Distinct inner knots.
    let mut inner: Vec<Real> = t[deg + 1..t.len() - deg - 1].to_vec();
    inner.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
    for u in inner {
        loop {
            let mult = t.iter().filter(|&&x| (x - u).abs() < 1e-12).count();
            if mult >= deg {
                break;
            }
            // Insert u once.
            let k = t.iter().rposition(|&x| x <= u + 1e-12).unwrap_or(deg);
            let mut q = Vec::with_capacity(p.len() + 1);
            for i in 0..=p.len() {
                if i + deg <= k {
                    q.push(p[i]);
                } else if i > k {
                    q.push(p[i - 1]);
                } else {
                    let a = (u - t[i]) / (t[i + deg] - t[i]).max(1e-300);
                    q.push(nalgebra::Point2::from(
                        p[i - 1].coords * (1.0 - a) + p[i].coords * a,
                    ));
                }
            }
            t.insert(k + 1, u);
            p = q;
        }
    }
    let _ = (lo, hi);
    Ok(p.windows(deg + 1)
        .step_by(deg)
        .map(|w| w.to_vec())
        .collect())
}

/// Extrudes `prof` on `plane` between offsets `start` and `end` along the
/// normal, or (with `taper`, radians, positive growing) by `length` from
/// `start` in direction `sign`. Faces: `<prefix>:Side:<segment>`,
/// `<prefix>:Start`, `<prefix>:End`.
pub fn extrude(
    plane: &Plane,
    prof: &ExactProfile,
    start: Real,
    end: Real,
    taper: Real,
    prefix: &str,
) -> Result<ExactSolid, String> {
    let pl = plane_of(plane)?;
    let body = if taper.abs() < 1e-12 {
        let mut v = guarded("extrude", || {
            sk::extrude(&pl, std::slice::from_ref(&prof.region), start, end)
        })?;
        if v.len() != 1 {
            return Err("extrude made more than one body".into());
        }
        v.remove(0)
    } else {
        let base = sg::Plane::new(
            v3(&(plane.origin + plane.normal.into_inner() * start)),
            d3(&plane.x_axis),
            d3(&plane.y_axis()),
        )
        .ok_or("degenerate plane")?;
        let len = (end - start).abs();
        let sign = (end - start).signum();
        guarded("extrude", || {
            sk::extrude_tapered(&base, &prof.region, len, sign, taper)
        })?
    };
    let names = prof.names.clone();
    let face_names = names_from(&body, &[], &|o| match o {
        FaceOrigin::Profile { segment } => {
            names.get(*segment).map(|n| format!("{prefix}:Side:{n}"))
        }
        FaceOrigin::Cap { end: e } => {
            Some(format!("{prefix}:{}", if *e { "End" } else { "Start" }))
        }
        _ => None,
    });
    let face_names = if face_names.iter().all(Option::is_none) {
        cap_side_names(&body, plane, start, end, &names, prefix)?
    } else {
        face_names
    };
    // The kernel's start cap lies at the lower offset; ours at `start`.
    let face_names = if end < start {
        let (s0, e0) = (format!("{prefix}:Start"), format!("{prefix}:End"));
        face_names
            .into_iter()
            .map(|n| match n {
                Some(n) if n == s0 => Some(e0.clone()),
                Some(n) if n == e0 => Some(s0.clone()),
                n => n,
            })
            .collect()
    } else {
        face_names
    };
    Ok(ExactSolid {
        name: Some(prefix.to_owned()),
        body,
        face_names,
        profiles: vec![ProfileFrame {
            prefix: prefix.to_owned(),
            plane: plane.clone(),
            starts: prof.starts.clone(),
        }],
    })
}

/// Names for an extrusion without history: caps by their plane offset,
/// sides by order.
fn cap_side_names(
    body: &sk::Body,
    plane: &Plane,
    start: Real,
    end: Real,
    names: &[String],
    prefix: &str,
) -> Result<Vec<Option<String>>, String> {
    let n = plane.normal.into_inner();
    let faces = guarded("faces", || body.faces(1e-3))?;
    let mut side = 0;
    Ok(faces
        .iter()
        .map(|f| {
            let c = w3(f.centroid);
            let h = (c - plane.origin.coords).dot(&n);
            let cap = f
                .plane_normal
                .is_some_and(|pn| w3(pn).cross(&n).norm() < 1e-6);
            if cap && (h - start).abs() < 1e-6 {
                Some(format!("{prefix}:Start"))
            } else if cap && (h - end).abs() < 1e-6 {
                Some(format!("{prefix}:End"))
            } else {
                let nm = names.get(side).cloned().unwrap_or_else(|| side.to_string());
                side += 1;
                Some(format!("{prefix}:Side:{nm}"))
            }
        })
        .collect())
}

/// Revolves `prof` about the axis (sketch coordinates) by `angle`
/// (radians, full turn when ≥ 2π), starting at the sketch plane rotated by
/// `start`.
pub fn revolve(
    plane: &Plane,
    prof: &ExactProfile,
    axis_origin: nalgebra::Point2<Real>,
    axis_dir: nalgebra::Vector2<Real>,
    start: Real,
    angle: Real,
    prefix: &str,
) -> Result<ExactSolid, String> {
    let mut pl = plane_of(plane)?;
    if start.abs() > 1e-12 {
        // Turn the sketch frame about the axis first.
        let o = plane.point_at(axis_origin.x, axis_origin.y);
        let d = (plane.x_axis.into_inner() * axis_dir.x + plane.y_axis().into_inner() * axis_dir.y)
            .normalize();
        let rot = nalgebra::Rotation3::from_axis_angle(&nalgebra::Unit::new_normalize(d), start);
        let org = o + rot * (plane.origin - o);
        pl = sg::Plane::new(
            v3(&org),
            d3(&(rot * plane.x_axis.into_inner())),
            d3(&(rot * plane.y_axis().into_inner())),
        )
        .ok_or("degenerate plane")?;
    }
    let mut v = guarded("revolve", || {
        sk::revolve(
            &pl,
            std::slice::from_ref(&prof.region),
            sg::Vec2::new(axis_origin.x, axis_origin.y),
            sg::Vec2::new(axis_dir.x, axis_dir.y),
            angle,
        )
    })?;
    if v.len() != 1 {
        return Err("revolve made more than one body".into());
    }
    let body = v.remove(0);
    let names = prof.names.clone();
    let face_names = names_from(&body, &[], &|o| match o {
        FaceOrigin::Profile { segment } => {
            names.get(*segment).map(|n| format!("{prefix}:Side:{n}"))
        }
        FaceOrigin::Cap { end } => Some(format!("{prefix}:{}", if *end { "End" } else { "Start" })),
        _ => None,
    });
    Ok(ExactSolid {
        name: Some(prefix.to_owned()),
        body,
        face_names,
        profiles: Vec::new(),
    })
}

/// `a op b`; the result keeps `a`'s body name. `None`: nothing is left.
pub fn boolean(a: &ExactSolid, b: &ExactSolid, op: ExactOp) -> Result<Option<ExactSolid>, String> {
    let kop = match op {
        ExactOp::Union => sk::BoolOp::Union,
        ExactOp::Cut => sk::BoolOp::Cut,
        ExactOp::Intersect => sk::BoolOp::Intersect,
    };
    let r = guarded("boolean", || sk::boolean(&a.body, &b.body, kop))?;
    Ok(r.map(|body| {
        let face_names = names_from(&body, &[a, b], &|_| None);
        ExactSolid {
            name: a.name.clone(),
            body,
            face_names,
            profiles: a.profiles.iter().chain(&b.profiles).cloned().collect(),
        }
    }))
}

/// Solids whose boxes are clearly apart as one solid of several pieces,
/// without any intersection work (SolidCraft's `union_apart`, made for many
/// pattern tools). `Ok(None)`: some may meet — union them the usual way.
/// Face names are kept; the body name is the first one's.
pub fn union_apart(solids: &[ExactSolid]) -> Result<Option<ExactSolid>, String> {
    if solids.len() < 2 {
        return Ok(None);
    }
    let bodies: Vec<sk::Body> = solids.iter().map(|s| s.body.clone()).collect();
    let Some(body) = guarded("union apart", || sk::union_apart(&bodies))? else {
        return Ok(None);
    };
    let refs: Vec<&ExactSolid> = solids.iter().collect();
    let face_names = names_from(&body, &refs, &|_| None);
    Ok(Some(ExactSolid {
        name: solids[0].name.clone(),
        body,
        face_names,
        profiles: solids
            .iter()
            .flat_map(|s| s.profiles.iter().cloned())
            .collect(),
    }))
}

/// Whether `p` lies inside the solid's material.
pub fn contains(s: &ExactSolid, p: Point3) -> bool {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| s.body.contains(v3(&p))))
        .unwrap_or(false)
}

/// Whether two solids share material or touch.
pub fn meets(a: &ExactSolid, b: &ExactSolid) -> bool {
    guarded("meets", || a.body.meets(&b.body)).unwrap_or(false)
}

/// Splits a solid into its connected pieces (same body name).
pub fn lumps(s: &ExactSolid) -> Vec<ExactSolid> {
    match guarded("lumps", || s.body.lumps()) {
        Ok(v) if v.len() > 1 => v
            .into_iter()
            .map(|body| {
                let face_names = names_from(&body, &[s], &|_| None);
                ExactSolid {
                    name: s.name.clone(),
                    body,
                    face_names,
                    profiles: s.profiles.clone(),
                }
            })
            .collect(),
        _ => vec![s.clone()],
    }
}

/// Fillet (`radius`) of the edges nearest `points`; new faces
/// `<prefix>:Round:<edge>`.
pub fn fillet(
    s: &ExactSolid,
    points: &[Point3],
    radius: Real,
    prefix: &str,
) -> Result<ExactSolid, String> {
    let pts: Vec<sg::Vec3> = points.iter().map(v3).collect();
    let body = guarded("fillet", || sk::fillet(&s.body, &pts, radius))?;
    Ok(blend_result(s, body, prefix, "Round"))
}

/// Fillet of the edges nearest `points`, each at its own radius (several
/// edge sets of one feature).
pub fn fillet_each(
    s: &ExactSolid,
    points: &[Point3],
    radii: &[Real],
    prefix: &str,
) -> Result<ExactSolid, String> {
    let pts: Vec<sg::Vec3> = points.iter().map(v3).collect();
    let body = guarded("fillet", || sk::fillet_each(&s.body, &pts, radii))?;
    Ok(blend_result(s, body, prefix, "Round"))
}

/// Variable-radius fillet of the straight edge nearest `at`: (position,
/// radius) points from its end nearest `start`, straight or `smooth`
/// between them.
pub fn fillet_variable(
    s: &ExactSolid,
    at: Point3,
    start: Point3,
    points: &[(Real, Real)],
    smooth: bool,
    prefix: &str,
) -> Result<ExactSolid, String> {
    let body = guarded("variable fillet", || {
        sk::fillet_variable_points(&s.body, v3(&at), v3(&start), points, smooth)
    })?;
    Ok(blend_result(s, body, prefix, "Round"))
}

/// Chord-width fillet of the edges nearest `points`: each blend is `chord`
/// wide across its edge.
pub fn fillet_chord(
    s: &ExactSolid,
    points: &[Point3],
    chord: Real,
    prefix: &str,
) -> Result<ExactSolid, String> {
    let pts: Vec<sg::Vec3> = points.iter().map(v3).collect();
    let body = guarded("chord fillet", || sk::fillet_chord(&s.body, &pts, chord))?;
    Ok(blend_result(s, body, prefix, "Round"))
}

/// Colours of one body for STEP: the body's (sRGB 0–1) and faces of their
/// own (kernel face index, colour, opacity).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StepPaint {
    /// The body's colour.
    pub color: Option<[f32; 3]>,
    /// Faces with another colour.
    pub faces: Vec<(usize, [f32; 3], f32)>,
}

/// STEP (ISO 10303-21, AP214/AP242 geometry) text of `solids`, one body
/// each, named after them; `product` names the part.
pub fn step_text(solids: &[ExactSolid], product: &str) -> Result<String, String> {
    step_text_painted(solids, product, &[])
}

/// [`step_text`] with colours (`paint[i]` for `solids[i]`; missing entries
/// keep the kernel's own colour).
pub fn step_text_painted(
    solids: &[ExactSolid],
    product: &str,
    paint: &[StepPaint],
) -> Result<String, String> {
    let names: Vec<String> = solids
        .iter()
        .enumerate()
        .map(|(i, s)| s.name.clone().unwrap_or_else(|| format!("Body{}", i + 1)))
        .collect();
    let painted: Vec<sk::Body> = solids
        .iter()
        .enumerate()
        .map(|(i, s)| match paint.get(i) {
            None => s.body.clone(),
            Some(p) => {
                let faces = (!p.faces.is_empty()).then(|| sk::Paint {
                    opacity: 1.0,
                    faces: p
                        .faces
                        .iter()
                        .map(|&(face, color, opacity)| sk::FacePaint {
                            face,
                            color,
                            opacity,
                        })
                        .collect(),
                });
                s.body
                    .clone()
                    .with_color(p.color.or(s.body.color()))
                    .with_paint(faces)
            }
        })
        .collect();
    let list: Vec<sk::ExportBody> = painted
        .iter()
        .zip(&names)
        .map(|(b, n)| sk::ExportBody {
            name: n.clone(),
            body: b,
            color: b.color(),
        })
        .collect();
    let header = sk::StepHeader {
        file_name: format!("{product}.step"),
        organization: "3DCadCraft".into(),
        ..Default::default()
    };
    guarded("STEP export", || sk::step_export_bodies(&list, &header))
}

/// Volumes of the bodies in STEP `text` (reads it back with the kernel's
/// importer; for round-trip checks).
pub fn step_volumes(text: &str) -> Result<Vec<Real>, String> {
    let imp = guarded("STEP import", || sk::step_import(text))?;
    imp.bodies
        .iter()
        .map(|b| guarded("measure", || sk::measure(&b.body)).map(|m| m.volume))
        .collect()
}

/// Chamfer of the edges nearest `points`: `d1` on the first face and `d2`
/// (or an angle) on the second.
pub fn chamfer(
    s: &ExactSolid,
    points: &[Point3],
    d1: Real,
    second: ChamferSecond,
    flip: bool,
    prefix: &str,
) -> Result<ExactSolid, String> {
    let pts: Vec<sg::Vec3> = points.iter().map(v3).collect();
    let body = match second {
        ChamferSecond::Same => guarded("chamfer", || sk::chamfer(&s.body, &pts, d1))?,
        ChamferSecond::Distance(d) => guarded("chamfer", || {
            sk::chamfer_sides(&s.body, &pts, d1, sk::ChamferSide::Distance(d), flip)
        })?,
        ChamferSecond::Angle(a) => guarded("chamfer", || {
            sk::chamfer_sides(&s.body, &pts, d1, sk::ChamferSide::Angle(a), flip)
        })?,
    };
    Ok(blend_result(s, body, prefix, "Bevel"))
}

/// The second size of a chamfer.
#[derive(Clone, Copy, Debug)]
pub enum ChamferSecond {
    /// Equal distances.
    Same,
    /// A second distance.
    Distance(Real),
    /// An angle from the first face (radians).
    Angle(Real),
}

fn blend_result(s: &ExactSolid, body: sk::Body, prefix: &str, role: &str) -> ExactSolid {
    // The input's edges as the names of the faces they separate: a blend
    // face is named after the edge it replaces, so adding an edge to the
    // feature never renames the others.
    let tol = s.body.size().max(1e-9) * 2e-4;
    let edge_faces = guarded("tessellate", || s.body.tessellate(tol))
        .map(|m| m.edge_faces)
        .unwrap_or_default();
    let edge_name = |e: usize| -> Option<String> {
        let mut names: Vec<String> = edge_faces
            .get(e)?
            .iter()
            .filter_map(|&f| s.face_names.get(f as usize).cloned().flatten())
            .collect();
        names.sort();
        names.dedup();
        (!names.is_empty()).then(|| names.join("|"))
    };
    let face_names = names_from(&body, &[s], &|o| match o {
        FaceOrigin::Edge { edge, .. } => Some(match edge_name(*edge) {
            Some(n) => format!("{prefix}:{role}[{n}]"),
            None => format!("{prefix}:{role}:{edge}"),
        }),
        _ => None,
    });
    // Unnamed generated faces (corner patches): numbered in face order.
    let mut n = 0usize;
    let face_names = face_names
        .into_iter()
        .map(|f| {
            f.or_else(|| {
                n += 1;
                Some(format!("{prefix}:Corner:{n}"))
            })
        })
        .collect();
    ExactSolid {
        name: s.name.clone(),
        body,
        face_names,
        profiles: s.profiles.clone(),
    }
}

// ---------------------------------------------------------------- faces ---

/// A point on every face of `s` (kernel face order): the centre of the
/// face's largest triangle at a fine tolerance, so it lies on the face (its
/// sag is far below the kernel's face-picking distance) and away from its
/// edges. `None` for a face without triangles.
fn face_samples(s: &ExactSolid) -> Result<Vec<Option<Point3>>, String> {
    let tol = (s.body.size() * 1e-4).max(1e-4);
    let mesh = guarded("tessellate", || s.body.tessellate(tol))?;
    let n = s.face_names.len().max(s.body.face_count());
    let mut best: Vec<Option<(Real, Point3)>> = vec![None; n];
    for (t, &f) in mesh.triangles.iter().zip(&mesh.tri_face) {
        let Some([a, b, c]) = mesh.tri(t) else {
            continue;
        };
        let (a, b, c) = (p3(a), p3(b), p3(c));
        let area = (b - a).cross(&(c - a)).norm();
        let slot = best.get_mut(f as usize);
        if let Some(slot) = slot
            && slot.is_none_or(|(x, _)| area > x)
        {
            *slot = Some((area, Point3::from((a.coords + b.coords + c.coords) / 3.0)));
        }
    }
    Ok(best.into_iter().map(|b| b.map(|(_, p)| p)).collect())
}

/// The faces of `s` named `name`, nearest to `hint` first (a face a later
/// Boolean split in pieces keeps its name on each piece).
pub fn faces_named(s: &ExactSolid, name: &str, hint: Point3) -> Result<Vec<usize>, String> {
    let samples = face_samples(s)?;
    let mut v: Vec<(Real, usize)> = s
        .face_names
        .iter()
        .enumerate()
        .filter(|(_, n)| n.as_deref() == Some(name))
        .filter_map(|(i, _)| {
            samples
                .get(i)
                .copied()
                .flatten()
                .map(|p| ((p - hint).norm(), i))
        })
        .collect();
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    Ok(v.into_iter().map(|(_, i)| i).collect())
}

/// Which way a shell's walls grow from the faces (Inventor: Inside,
/// Outside, Both).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellSide {
    /// Into the material: the outside stays where it is.
    Inside,
    /// Out of it: the old faces become the inner walls.
    Outside,
    /// Half each way.
    Both,
}

/// Hollows `s` with walls `thickness` thick; the faces `open` (kernel face
/// indices) are removed. Inner walls are named `<prefix>:Inner[<face>]`
/// after the face they line; the outside keeps its names. With no open
/// face the body gets a closed cavity (Inventor allows that).
pub fn shell(
    s: &ExactSolid,
    open: &[usize],
    thickness: Real,
    side: ShellSide,
    prefix: &str,
) -> Result<ExactSolid, String> {
    if !(thickness.is_finite() && thickness > 1e-6) {
        return Err("the thickness must be positive".into());
    }
    let samples = face_samples(s)?;
    let pts = |want: &dyn Fn(usize) -> bool| -> Vec<sg::Vec3> {
        samples
            .iter()
            .enumerate()
            .filter(|(i, _)| want(*i))
            .filter_map(|(_, p)| p.as_ref().map(v3))
            .collect()
    };
    let open_pts = pts(&|i| open.contains(&i));
    if open_pts.len() < open.len() {
        return Err("a face to remove has no area".into());
    }
    let inner_name = |face: usize| -> String {
        match s.face_names.get(face).cloned().flatten() {
            Some(n) => format!("{prefix}:Inner[{n}]"),
            None => format!("{prefix}:Inner:{face}"),
        }
    };
    // Inside with faces to remove: the kernel's own shell (convex bodies
    // and walls of any kind); the general way below if it gives up.
    let first = if side == ShellSide::Inside && !open.is_empty() {
        match guarded("shell", || sk::shell(&s.body, &open_pts, thickness)) {
            Ok(body) => {
                let face_names = names_from(&body, &[s], &|o| match o {
                    FaceOrigin::Offset { face, .. } => Some(inner_name(*face)),
                    _ => None,
                });
                // Walls the history does not trace (a non-convex body's
                // cavity): numbered in face order, never unnamed.
                let mut n = 0usize;
                let face_names = face_names
                    .into_iter()
                    .map(|f| {
                        f.or_else(|| {
                            n += 1;
                            Some(format!("{prefix}:Inner:{n}"))
                        })
                    })
                    .collect();
                return Ok(ExactSolid {
                    name: s.name.clone(),
                    body,
                    face_names,
                    profiles: s.profiles.clone(),
                });
            }
            // A round at an open face at least as thick as the wall: the
            // kernel refuses the rim (SolidCraft #342). Inventor still makes
            // it: a closed cavity, the opening cut as a prism of the face.
            // The general path below only spent 7 s failing here.
            Err(e) if e.contains("rim would have no thickness") => {
                return shell_with_openings(s, open, thickness, prefix).map_err(|_| e);
            }
            Err(e) => Some(e),
        }
    } else {
        None
    };
    // An offset body less a cavity (SolidCraft's Outside / Both): the walls
    // moved out by `out`, the cavity the body moved in by `inn`, its open
    // faces pushed out through the walls.
    let (out, inn) = match side {
        ShellSide::Inside => (0.0, thickness),
        ShellSide::Outside => (thickness, 0.0),
        ShellSide::Both => (thickness / 2.0, thickness / 2.0),
    };
    let walls = pts(&|i| !open.contains(&i));
    let general = || -> Result<ExactSolid, String> {
        let outer = if out > 0.0 {
            let body = guarded("shell", || sk::offset_faces(&s.body, &walls, out))?;
            let face_names = names_from(&body, &[s], &|_| None);
            ExactSolid {
                name: s.name.clone(),
                body,
                face_names,
                profiles: s.profiles.clone(),
            }
        } else {
            s.clone()
        };
        let lined = ExactSolid {
            name: s.name.clone(),
            body: s.body.clone(),
            face_names: (0..s.face_names.len())
                .map(|i| Some(inner_name(i)))
                .collect(),
            profiles: Vec::new(),
        };
        let mut cavity = if inn > 0.0 {
            let body = guarded("shell", || sk::offset_faces(&lined.body, &walls, -inn))?;
            let face_names = names_from(&body, &[&lined], &|_| None);
            ExactSolid {
                face_names,
                body,
                ..lined
            }
        } else {
            lined
        };
        if !open_pts.is_empty() {
            let push = 2.0 * thickness + 1.0;
            let body = guarded("shell", || sk::offset_faces(&cavity.body, &open_pts, push))?;
            let face_names = names_from(&body, &[&cavity], &|_| None);
            cavity = ExactSolid {
                face_names,
                body,
                ..cavity
            };
        }
        boolean(&outer, &cavity, ExactOp::Cut)?.ok_or_else(|| "the shell removed everything".into())
    };
    general().map_err(|e| first.unwrap_or(e))
}

/// Shell Inside where an open face meets rounds at least as large as the
/// wall: the closed cavity, then each open planar face cut through as a
/// prism of its outline (straight edges only), from outside to 1.5 walls
/// deep. The rim faces are named `<prefix>:Rim:Side:<n>`.
fn shell_with_openings(
    s: &ExactSolid,
    open: &[usize],
    thickness: Real,
    prefix: &str,
) -> Result<ExactSolid, String> {
    let grow = thickness * 1e-3;
    openings_cut(s, open, thickness, prefix, grow)
        .or_else(|_| openings_cut(s, open, thickness, prefix, -grow))
}

fn openings_cut(
    s: &ExactSolid,
    open: &[usize],
    thickness: Real,
    prefix: &str,
    grow: Real,
) -> Result<ExactSolid, String> {
    use super::extrude::ProfileSegment;
    // The prisms first: an opening the cut cannot make fails before the
    // (slower) cavity is built.
    let mut prisms = Vec::new();
    let size = s.body.size().max(1e-9);
    let mesh = guarded("tessellate", || s.body.tessellate((size * 1e-4).max(1e-4)))?;
    let infos = guarded("faces", || s.body.faces(1e-3))?;
    let samples = face_samples(s)?;
    let key = |p: &Point3| {
        let q = |x: Real| (x / (size * 1e-6)).round() as i64;
        (q(p.x), q(p.y), q(p.z))
    };
    for &f in open {
        let info = infos.iter().find(|i| i.index == f).ok_or("no such face")?;
        let n = info
            .plane_normal
            .map(w3)
            .ok_or("not supported yet: a curved open face next to a round")?;
        let origin = samples
            .get(f)
            .copied()
            .flatten()
            .ok_or("face without area")?;
        let plane = Plane::from_normal(origin, n).map_err(|e| e.to_string())?;
        // The face's boundary edges, each straight: (start, end).
        let mut segs: Vec<(Point3, Point3)> = Vec::new();
        for (pts, faces) in mesh.edges.iter().zip(&mesh.edge_faces) {
            if !faces.contains(&(f as u32)) {
                continue;
            }
            let pts: Vec<Point3> = pts.iter().map(|v| p3(*v)).collect();
            let (Some(a), Some(b)) = (pts.first().copied(), pts.last().copied()) else {
                continue;
            };
            let d = b - a;
            let l = d.norm();
            if l < size * 1e-9 {
                continue;
            }
            if pts
                .iter()
                .any(|p| (p - a).cross(&d).norm() / l > size * 1e-6)
            {
                return Err(
                    "not supported yet: an opening with curved edges next to a round".into(),
                );
            }
            segs.push((a, b));
        }
        // Chain into loops.
        let mut loops: Vec<Vec<nalgebra::Point2<Real>>> = Vec::new();
        while let Some((a, b)) = segs.pop() {
            let start = key(&a);
            let mut lp = vec![a];
            let mut end = b;
            while key(&end) != start {
                lp.push(end);
                let i = segs
                    .iter()
                    .position(|(x, y)| key(x) == key(&end) || key(y) == key(&end))
                    .ok_or("the opening's outline is not closed")?;
                let (x, y) = segs.swap_remove(i);
                end = if key(&x) == key(&end) { y } else { x };
            }
            loops.push(lp.iter().map(|p| plane.project(p)).collect());
        }
        let area = |l: &[nalgebra::Point2<Real>]| {
            (0..l.len())
                .map(|i| {
                    let (a, b) = (l[i], l[(i + 1) % l.len()]);
                    a.x * b.y - b.x * a.y
                })
                .sum::<Real>()
                / 2.0
        };
        let k = (0..loops.len())
            .max_by(|&i, &j| area(&loops[i]).abs().total_cmp(&area(&loops[j]).abs()))
            .ok_or("the opening has no outline")?;
        // The opening grows by a hair (`grow` > 0): its sides then cross the
        // rounds next to the face instead of running along the edges where
        // they touch it (the kernel cannot cut faces that only touch). When
        // that cut grazes them (small rounds), it shrinks by a hair instead:
        // its sides then meet only the flat face and the cavity's ceiling.

        let mut n_seg = 0usize;
        let mut seg_loop = |l: &[nalgebra::Point2<Real>], ccw: bool| -> Vec<ProfileSegment> {
            let mut v: Vec<nalgebra::Point2<Real>> = l.to_vec();
            if (area(&v) > 0.0) != ccw {
                v.reverse();
            }
            let v = offset_polygon(&v, grow);
            (0..v.len())
                .map(|i| {
                    n_seg += 1;
                    ProfileSegment {
                        geometry: Segment2::Line {
                            a: v[i],
                            b: v[(i + 1) % v.len()],
                        },
                        name: n_seg.to_string(),
                    }
                })
                .collect()
        };
        let outer = seg_loop(&loops[k], true);
        let holes: Vec<Vec<ProfileSegment>> = loops
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != k)
            .map(|(_, l)| seg_loop(l, false))
            .collect();
        let prof = profile(&outer, &holes)?;
        let prism = extrude(
            &plane,
            &prof,
            -1.5 * thickness,
            thickness + 1.0,
            0.0,
            &format!("{prefix}:Rim"),
        )?;
        prisms.push(prism);
    }
    let mut out = shell(s, &[], thickness, ShellSide::Inside, prefix)?;
    for prism in prisms {
        out = boolean(&out, &prism, ExactOp::Cut)?.ok_or("the shell removed everything")?;
    }
    Ok(out)
}

/// Polygon `v` with every edge moved `d` to its right (outward for a
/// counter-clockwise loop, into the hole for a clockwise one); corners where
/// the moved edges meet, collinear edges just shifted.
fn offset_polygon(v: &[nalgebra::Point2<Real>], d: Real) -> Vec<nalgebra::Point2<Real>> {
    let n = v.len();
    let shifted = |i: usize| {
        let (a, b) = (v[i], v[(i + 1) % n]);
        let t = (b - a).normalize();
        let nr = nalgebra::Vector2::new(t.y, -t.x) * d;
        (a + nr, t)
    };
    (0..n)
        .map(|i| {
            let (p0, t0) = shifted((i + n - 1) % n);
            let (p1, t1) = shifted(i);
            let cross = t0.x * t1.y - t0.y * t1.x;
            if cross.abs() < 1e-12 {
                p1
            } else {
                let s = ((p1 - p0).x * t1.y - (p1 - p0).y * t1.x) / cross;
                p0 + t0 * s
            }
        })
        .collect()
}

/// A point on face `face` of `s` (see [`faces_named`]).
pub fn face_point(s: &ExactSolid, face: usize) -> Option<Point3> {
    face_samples(s).ok()?.get(face).copied().flatten()
}

/// Face Draft about a neutral plane: the faces `faces` (kernel indices) of
/// `s` tilt by `angle` (radians) about their line on `neutral`, leaning in
/// going along `pull` for a positive angle. The faces keep their names.
pub fn draft(
    s: &ExactSolid,
    faces: &[usize],
    neutral: &Plane,
    pull: Vector3,
    angle: Real,
) -> Result<ExactSolid, String> {
    let samples = face_samples(s)?;
    let pts: Vec<sg::Vec3> = faces
        .iter()
        .filter_map(|&f| samples.get(f).copied().flatten())
        .map(|p| v3(&p))
        .collect();
    if pts.len() < faces.len() || pts.is_empty() {
        return Err("no faces to draft".into());
    }
    let pl = plane_of(neutral)?;
    let body = guarded("draft", || sk::draft(&s.body, &pts, &pl, d3(&pull), angle))?;
    let face_names = names_from(&body, &[s], &|_| None);
    Ok(ExactSolid {
        name: s.name.clone(),
        body,
        face_names,
        profiles: s.profiles.clone(),
    })
}

/// A copy of `s` placed by the rigid motion `m` (rotation, translation or a
/// reflection, determinant −1; `p' = m · p` in homogeneous coordinates).
/// Faces keep their order; each name becomes `<rename>:<name>` (pattern
/// copies: `RectangularPattern1#2:Extrusion2:Side`), and the body is named
/// `body` (or keeps its name). The copy carries no profile frames, so its
/// vertices and edges are not named after a sketch.
pub fn transformed(
    s: &ExactSolid,
    m: &nalgebra::Matrix4<Real>,
    rename: &str,
    body: Option<&str>,
) -> Result<ExactSolid, String> {
    let lin = m.fixed_view::<3, 3>(0, 0).into_owned();
    let ortho = (lin.transpose() * lin - nalgebra::Matrix3::identity())
        .abs()
        .max();
    let bottom = m[(3, 0)].abs() + m[(3, 1)].abs() + m[(3, 2)].abs() + (m[(3, 3)] - 1.0).abs();
    if !(ortho < 1e-9 && bottom < 1e-12) {
        return Err("the placement is not a rigid motion".into());
    }
    // SolidCraft's matrices are column-major: c[col][row].
    let mut c = [[0.0; 4]; 4];
    for (col, cc) in c.iter_mut().enumerate() {
        for (row, x) in cc.iter_mut().enumerate() {
            *x = m[(row, col)];
        }
    }
    let placed = guarded("transform", || sk::transform_matrix(&s.body, c))?;
    let mut face_names = names_from(&placed, &[s], &|_| None);
    // Without a history the order is the original's (affine placement).
    if face_names.iter().all(Option::is_none) && placed.face_count() == s.face_names.len() {
        face_names = s.face_names.clone();
    }
    let face_names = face_names
        .into_iter()
        .map(|n| n.map(|n| format!("{rename}:{n}")))
        .collect();
    Ok(ExactSolid {
        name: body.map(str::to_owned).or_else(|| s.name.clone()),
        body: placed,
        face_names,
        profiles: Vec::new(),
    })
}
// ---------------------------------------------------------- hole tools ---

/// The frame of a tool about an axis: origin on the axis, `x` radial (the
/// profile's seam turned by `seam` radians about the axis), `y` along it.
fn axial_frame(origin: Point3, axis: Vector3, seam: Real) -> Result<sg::Plane, String> {
    let k = axis
        .try_normalize(1e-12)
        .ok_or_else(|| "the hole axis has no direction".to_owned())?;
    let any = if k.x.abs() < 0.9 {
        Vector3::x()
    } else {
        Vector3::y()
    };
    let u = (any - k * any.dot(&k)).normalize();
    let w = k.cross(&u);
    let x = u * seam.cos() + w * seam.sin();
    sg::Plane::new(v3(&origin), d3(&x), d3(&k)).ok_or_else(|| "degenerate hole axis".to_owned())
}

/// A tool of revolution (a drilled hole with its seats, after SolidCraft's
/// `hole_tools_and_prisms`): `profile` is a closed polygon `[r, z]` in the
/// half plane through the axis (z along `axis` from `origin`, r ≥ 0), its
/// segment `i` (from point `i` to `i + 1`) makes the face named
/// `<prefix>:<names[i]>`. `seam` turns the profile plane (seams of tools
/// side by side should not line up).
pub fn revolved_tool(
    origin: Point3,
    axis: Vector3,
    seam: Real,
    profile: &[[Real; 2]],
    names: &[&str],
    prefix: &str,
) -> Result<ExactSolid, String> {
    if profile.len() < 3 || names.len() != profile.len() {
        return Err("the hole profile is incomplete".into());
    }
    if profile.iter().flatten().any(|x| !x.is_finite()) || profile.iter().any(|p| p[0] < -1e-9) {
        return Err("the hole profile is not valid".into());
    }
    let pl = axial_frame(origin, axis, seam)?;
    let pts: Vec<sg::Vec2> = profile
        .iter()
        .map(|p| sg::Vec2::new(p[0].max(0.0), p[1]))
        .collect();
    let region = sg::Region2 {
        outer: sg::Loop2::polygon(&pts),
        holes: Vec::new(),
    };
    let mut v = guarded("hole", || {
        sk::revolve(
            &pl,
            std::slice::from_ref(&region),
            sg::Vec2::new(0.0, 0.0),
            sg::Vec2::new(0.0, 1.0),
            std::f64::consts::TAU,
        )
    })?;
    if v.len() != 1 {
        return Err("the hole tool is not one solid".into());
    }
    let body = v.remove(0);
    let face_names = names_from(&body, &[], &|o| match o {
        FaceOrigin::Profile { segment } => names.get(*segment).map(|n| format!("{prefix}:{n}")),
        _ => None,
    });
    Ok(ExactSolid {
        name: Some(prefix.to_owned()),
        body,
        face_names,
        profiles: Vec::new(),
    })
}

/// A helical thread groove: the closed polygon `section` (`[r, z]` in the
/// half plane through the axis, z from `origin`) screwed `turns` turns of
/// `pitch` along `axis` (right-handed about it; `left_hand` turns the other
/// way). Smooth B-spline walls through SolidCraft's `sweep_helix` (never its
/// faceted mesh: SolidCraft #2038 faceted the whole body). Every face is
/// named `<prefix>:<name>`.
#[allow(clippy::too_many_arguments)]
pub fn helix_tool(
    origin: Point3,
    axis: Vector3,
    seam: Real,
    section: &[[Real; 2]],
    pitch: Real,
    turns: Real,
    left_hand: bool,
    prefix: &str,
    name: &str,
) -> Result<ExactSolid, String> {
    if section.len() < 3 || section.iter().flatten().any(|x| !x.is_finite()) {
        return Err("the thread section is not valid".into());
    }
    if !(pitch.is_finite() && pitch > 1e-3 && turns.is_finite() && turns > 0.0) {
        return Err("the thread pitch and length must be positive".into());
    }
    let pl = axial_frame(origin, axis, seam)?;
    let pts: Vec<sg::Vec2> = section.iter().map(|p| sg::Vec2::new(p[0], p[1])).collect();
    let region = sg::Region2 {
        outer: sg::Loop2::polygon(&pts),
        holes: Vec::new(),
    };
    let k = d3(&axis.normalize());
    // A left-handed helix turns about −axis and climbs along +axis.
    let (ax, p) = if left_hand {
        (k * -1.0, -pitch)
    } else {
        (k, pitch)
    };
    let body = guarded("thread", || {
        sk::sweep_helix(&pl, &region, v3(&origin), ax, p, turns)
    })?;
    let n = body.face_count();
    Ok(ExactSolid {
        name: Some(prefix.to_owned()),
        body,
        face_names: vec![Some(format!("{prefix}:{name}")); n],
        profiles: Vec::new(),
    })
}

/// A modeled thread groove for [`cut_threads`]: a closed convex polygon
/// (`[r, z]` about the axis) screwed along the axis, and the zone around it
/// that gets the faceted cut.
#[derive(Clone, Debug, PartialEq)]
pub struct ThreadGroove {
    /// A point on the axis (z = 0).
    pub origin: Point3,
    /// Axis direction (+z).
    pub axis: Vector3,
    /// The section at the helix start (z measured from `base`).
    pub section: Vec<[Real; 2]>,
    /// Where the helix starts (z).
    pub base: Real,
    /// Pitch (mm per turn).
    pub pitch: Real,
    /// Number of turns.
    pub turns: Real,
    /// Left-handed thread.
    pub left_hand: bool,
    /// The zone `[r_in, r_out, z_lo, z_hi]` holding the groove: only the
    /// material inside it is faceted.
    pub zone: [Real; 4],
    /// Face name prefix (the feature id).
    pub prefix: String,
    /// Name of the groove walls after the prefix (`Thread:1`).
    pub name: String,
}

/// Stations per turn of a modeled thread's helix (10° apart: the chords
/// sag 0.4 % of the radius; SolidCraft's `thread_groove` uses 48, which
/// took twice as long).
const THREAD_STATIONS: usize = 36;

/// Cuts modeled threads into `s`. The exact kernel's Boolean with a smooth
/// helical sweep took 5 s per turn crossing a bore and over a minute
/// crossing a plane, so the grooves are cut as triangle meshes
/// (SolidCraft's `faceted_boolean`) — but only into the material of their
/// zones: `s` is split into the exact rest (`s` less the zones) and the
/// zones' material, and only the latter is faceted (SolidCraft #2038
/// faceted the whole body). Separate zones are cut on threads of their own.
/// Returns the pieces: the exact rest (if any) and the threaded zones
/// (planar faces), touching each other along the zones' walls. The groove
/// walls are named `<prefix>:<name>`.
pub fn cut_threads(s: &ExactSolid, grooves: &[ThreadGroove]) -> Result<Vec<ExactSolid>, String> {
    if grooves.is_empty() {
        return Ok(vec![s.clone()]);
    }
    let mut zones = Vec::new();
    let mut meshes: Vec<(Point3, sg::Mesh, String)> = Vec::new();
    for (k, g) in grooves.iter().enumerate() {
        let [r0, r1, z0, z1] = g.zone;
        if !(r0 >= 0.0 && r1 > r0 && z1 > z0) {
            return Err("the thread zone is empty".into());
        }
        zones.push(revolved_tool(
            g.origin,
            g.axis,
            0.4 + 0.613 * k as Real,
            &[[r0, z0], [r1, z0], [r1, z1], [r0, z1]],
            &["ThreadZone"; 4],
            &g.prefix,
        )?);
        if g.section.len() < 3 || g.section.len() > 64 {
            return Err("the thread section is not valid".into());
        }
        if !(g.pitch > 1e-3 && g.turns > 0.0 && g.turns <= 200.0) {
            return Err(format!(
                "a modeled thread is limited to 200 turns ({:.0} mm at this pitch); use a cosmetic one",
                200.0 * g.pitch
            ));
        }
        let k3 = g
            .axis
            .try_normalize(1e-12)
            .ok_or_else(|| "the thread axis has no direction".to_owned())?;
        let base = g.origin + k3 * g.base;
        let pl = axial_frame(base, k3, 0.0)?;
        let poly: Vec<sg::Vec2> = g
            .section
            .iter()
            .map(|p| sg::Vec2::new(p[0], p[1]))
            .collect();
        // A left-handed helix turns about −axis and climbs along +axis.
        let (ax, p) = if g.left_hand {
            (d3(&-k3), -g.pitch)
        } else {
            (d3(&k3), g.pitch)
        };
        let mut m = guarded("thread", || {
            sk::sweep_helix_mesh(&pl, &poly, v3(&base), ax, p, g.turns, THREAD_STATIONS)
        })?;
        m.tri_face = vec![0; m.triangles.len()];
        let mid = g.origin + k3 * ((z0 + z1) / 2.0);
        meshes.push((mid, m, format!("{}:{}", g.prefix, g.name)));
    }
    let zone = match union_apart(&zones)? {
        Some(z) => z,
        None => {
            let mut it = zones.into_iter();
            let mut z = it.next().ok_or("no thread zone")?;
            for t in it {
                z = boolean(&z, &t, ExactOp::Union)?.ok_or("thread zones")?;
            }
            z
        }
    };
    let inside = boolean(s, &zone, ExactOp::Intersect)?
        .ok_or_else(|| "the thread is not in the material".to_owned())?;
    let rest = boolean(s, &zone, ExactOp::Cut)?;
    // Each piece of material with the grooves inside its box.
    let mut jobs: Vec<(ExactSolid, sg::Mesh, Vec<String>)> = Vec::new();
    for piece in lumps(&inside) {
        let Ok(m) = piece.measure_full() else {
            continue;
        };
        let (lo, hi) = m.bbox;
        let pad = 1e-6 * (hi - lo).norm() + 1e-9;
        let mut mesh = sg::Mesh::default();
        let mut names = Vec::new();
        for (mid, g, name) in &meshes {
            if !(0..3).all(|i| mid[i] >= lo[i] - pad && mid[i] <= hi[i] + pad) {
                continue;
            }
            let off = u32::try_from(mesh.positions.len()).map_err(|_| "thread mesh too large")?;
            mesh.positions.extend(g.positions.iter().copied());
            mesh.triangles.extend(
                g.triangles
                    .iter()
                    .map(|t| [t[0] + off, t[1] + off, t[2] + off]),
            );
            names.extend(std::iter::repeat_n(name.clone(), g.triangles.len()));
        }
        mesh.tri_face = vec![0; mesh.triangles.len()];
        jobs.push((piece, mesh, names));
    }
    let results: Vec<Result<Option<ExactSolid>, String>> = std::thread::scope(|sc| {
        let handles: Vec<_> = jobs
            .iter()
            .map(|(piece, mesh, names)| sc.spawn(move || threaded_piece(s, piece, mesh, names)))
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join()
                    .unwrap_or_else(|_| Err("thread: internal error in the geometry kernel".into()))
            })
            .collect()
    });
    let mut out: Vec<ExactSolid> = rest.into_iter().collect();
    for r in results {
        if let Some(p) = r? {
            out.push(p);
        }
    }
    Ok(out)
}

/// One zone's material less its grooves (faceted), named.
fn threaded_piece(
    s: &ExactSolid,
    piece: &ExactSolid,
    mesh: &sg::Mesh,
    groove_names: &[String],
) -> Result<Option<ExactSolid>, String> {
    if mesh.triangles.is_empty() {
        return Ok(Some(piece.clone()));
    }
    let tol = (piece.body.size() * 5e-4).clamp(2e-3, 1e-2);
    let Some(body) = guarded("thread", || {
        sk::faceted_boolean(&piece.body, mesh, sk::BoolOp::Cut, tol)
    })?
    else {
        return Ok(None);
    };
    // Faces on the zone's material keep their names; faces on the mesh are
    // the groove's.
    let n = body.face_count();
    let mut face_names = vec![None; n];
    if let Some(h) = body.history() {
        for (i, fp) in h.faces.iter().enumerate().take(n) {
            face_names[i] = fp.origins.iter().find_map(|o| match o {
                FaceOrigin::Face { operand: 0, face } => {
                    piece.face_names.get(*face).cloned().flatten()
                }
                FaceOrigin::Face { operand: 1, face } => groove_names.get(*face).cloned(),
                _ => None,
            });
        }
    }
    Ok(Some(ExactSolid {
        name: s.name.clone(),
        body,
        face_names,
        profiles: Vec::new(),
    }))
}

/// `s` moved by the rigid motion `m` (a pattern copy of a tool); face names
/// are kept.
pub fn moved(s: &ExactSolid, m: &nalgebra::Matrix4<Real>) -> Result<ExactSolid, String> {
    // The kernel takes the matrix column by column.
    let mut a = [[0.0; 4]; 4];
    for (c, col) in a.iter_mut().enumerate() {
        for (r, x) in col.iter_mut().enumerate() {
            *x = m[(r, c)];
        }
    }
    let body = guarded("transform", || sk::transform_matrix(&s.body, a))?;
    let face_names = names_from(&body, &[s], &|_| None);
    let face_names = if face_names.iter().all(Option::is_none) {
        s.face_names.clone()
    } else {
        face_names
    };
    Ok(ExactSolid {
        name: s.name.clone(),
        body,
        face_names,
        profiles: Vec::new(),
    })
}

// --------------------------------------------------------- to our B-rep ---

/// Display tolerance (mm) of the exact kernel's triangles.
const DISPLAY_TOL: Real = 0.01;

/// A point snapped to a 1 nm grid, for welding.
type Key = (i64, i64, i64);

fn key(p: &Point3) -> Key {
    let q = |x: Real| (x * 1e6).round() as i64;
    (q(p.x), q(p.y), q(p.z))
}

/// Our B-rep of the solids: named topology, exact analytic surfaces where
/// the kernel reports them, and its display triangles per face.
pub fn to_brep(solids: &[ExactSolid]) -> Result<BRep, String> {
    let mut out = BRep::new();
    for s in solids {
        add_solid(&mut out, s)?;
    }
    Ok(out)
}

/// The exact kernel's surface of face `i` in our form.
fn surface_of(body: &sk::Body, i: usize) -> Option<Surface> {
    use sk::FaceSurface as F;
    if let Some(p) = body.face_plane(i) {
        let n = w3(p.x.cross(p.y));
        return Plane::new(p3(p.origin), n, w3(p.x))
            .ok()
            .map(Surface::Plane);
    }
    let any_x = |a: &Vector3| {
        let t = if a.x.abs() < 0.9 {
            Vector3::x()
        } else {
            Vector3::y()
        };
        t - a * t.dot(a)
    };
    Some(match sk::face_surface(body, i)? {
        F::Plane { normal, point } => {
            let n = w3(normal);
            Surface::Plane(Plane::new(p3(point), n, any_x(&n)).ok()?)
        }
        F::Cylinder {
            origin,
            axis,
            radius,
        } => {
            let a = w3(axis);
            Surface::Cylinder(Cylinder::new(p3(origin), a, any_x(&a), radius).ok()?)
        }
        F::Cone {
            apex,
            axis,
            half_angle,
        } => {
            let a = w3(axis);
            Surface::Cone(Cone::new(p3(apex), a, any_x(&a), 0.0, half_angle).ok()?)
        }
        F::Sphere { center, radius } => Surface::Sphere(Sphere {
            center: p3(center),
            radius,
            axis: nalgebra::Unit::new_normalize(Vector3::z()),
            x_axis: nalgebra::Unit::new_normalize(Vector3::x()),
        }),
        F::Torus {
            center,
            axis,
            major,
            minor,
        } => {
            let a = w3(axis);
            Surface::Torus(Torus {
                center: p3(center),
                axis: nalgebra::Unit::new_normalize(a),
                x_axis: nalgebra::Unit::new_normalize(any_x(&a)),
                major,
                minor,
            })
        }
    })
}

/// The curve through a polyline: a line, a circle (all points on one), or
/// the polyline as a degree-1 B-spline.
fn curve_of(pts: &[Point3]) -> Curve {
    let n = pts.len();
    if n >= 2 {
        let (a, b) = (pts[0], pts[n - 1]);
        let d = b - a;
        let straight = d.norm() > 1e-9
            && pts
                .iter()
                .all(|p| (p - a).cross(&d).norm() <= 1e-7 * d.norm() * (1.0 + d.norm()));
        if straight && let Ok(l) = Line::new(a, d) {
            return Curve::Line(l);
        }
        if n >= 4
            && let Some(c) = circle_through(pts)
        {
            return Curve::Circle(c);
        }
    }
    let k = n.max(2);
    let mut knots = vec![0.0; 2];
    for i in 1..k - 1 {
        knots.push(i as Real);
    }
    knots.extend([(k - 1) as Real; 2]);
    let cps = if n >= 2 {
        pts.to_vec()
    } else {
        vec![pts[0], pts[0]]
    };
    match super::BSplineCurve::new(1, cps, knots, None) {
        Ok(b) => Curve::BSpline(b),
        Err(_) => {
            Curve::Line(Line::new(pts[0], Vector3::x()).unwrap_or_else(|_| unreachable_line()))
        }
    }
}

fn unreachable_line() -> Line {
    Line {
        origin: Point3::origin(),
        direction: nalgebra::Unit::new_normalize(Vector3::x()),
    }
}

/// The circle through all of `pts` (within 1e-6 relative), if there is one.
fn circle_through(pts: &[Point3]) -> Option<Circle> {
    let n = pts.len();
    let (a, b, c) = (pts[0], pts[n / 3], pts[2 * n / 3]);
    let (ab, ac) = (b - a, c - a);
    let nrm = ab.cross(&ac);
    if nrm.norm() < 1e-12 {
        return None;
    }
    let center = a
        + (ac.norm_squared() * nrm.cross(&ab) + ab.norm_squared() * ac.cross(&nrm))
            / (2.0 * nrm.norm_squared());
    let r = (a - center).norm();
    let tol = 1e-6 * r.max(1.0);
    let plane_n = nrm.normalize();
    if pts
        .iter()
        .any(|p| ((p - center).norm() - r).abs() > tol || (p - center).dot(&plane_n).abs() > tol)
    {
        return None;
    }
    Circle::new(center, plane_n, a - center, r).ok()
}

/// The sense in which face `m` runs an edge through `pts`: the face lies to
/// the left of the edge seen from outside (normal × direction points into
/// it). Taken at the edge's middle segment, against the face's triangle
/// nearest to it.
fn sense_by_geometry(m: &TriangleMesh, pts: &[Point3]) -> Option<Sense> {
    let i = pts.len() / 2;
    let (a, b) = (pts[i.saturating_sub(1)], pts[i.max(1)]);
    let d = b - a;
    let mid = nalgebra::center(&a, &b);
    let (_, c, n) = m
        .triangles
        .iter()
        .map(|t| {
            let [p, q, r] = t.map(|k| m.positions[k as usize]);
            let c = Point3::from((p.coords + q.coords + r.coords) / 3.0);
            let n = t
                .iter()
                .map(|&k| m.normals[k as usize])
                .fold(Vector3::zeros(), |s, v| s + v);
            ((c - mid).norm(), c, n)
        })
        .min_by(|x, y| x.0.total_cmp(&y.0))?;
    let side = n.cross(&d).dot(&(c - mid));
    if side.abs() < 1e-15 {
        return None;
    }
    Some(if side > 0.0 {
        Sense::Same
    } else {
        Sense::Reversed
    })
}

fn add_solid(out: &mut BRep, s: &ExactSolid) -> Result<(), String> {
    let body = &s.body;
    let mesh = guarded("display mesh", || body.display_mesh(DISPLAY_TOL))?;
    let nf = body.face_count();
    // Triangles per face (our mesh type), and the directed triangle edges of
    // each face (for the boundary orientation).
    let mut meshes: Vec<TriangleMesh> = vec![TriangleMesh::default(); nf];
    let mut directed: Vec<std::collections::HashSet<(Key, Key)>> = vec![Default::default(); nf];
    for (t, tri) in mesh.triangles.iter().enumerate() {
        let f = *mesh.tri_face.get(t).unwrap_or(&0) as usize;
        let Some(m) = meshes.get_mut(f) else { continue };
        let base = m.positions.len() as u32;
        let mut ks = [(0, 0, 0); 3];
        for (j, &vi) in tri.iter().enumerate() {
            let p = mesh
                .positions
                .get(vi as usize)
                .copied()
                .unwrap_or(sg::Vec3::ZERO);
            let nv = mesh
                .normals
                .get(vi as usize)
                .copied()
                .unwrap_or(sg::Vec3::Z);
            m.positions.push(p3(p));
            m.normals.push(w3(nv));
            ks[j] = key(&p3(p));
        }
        m.triangles.push([base, base + 1, base + 2]);
        for j in 0..3 {
            directed[f].insert((ks[j], ks[(j + 1) % 3]));
        }
    }

    // Vertices: edge polyline ends, welded.
    let mut vid: HashMap<Key, VertexId> = HashMap::new();
    let mut vertex = |out: &mut BRep, p: Point3| -> VertexId {
        *vid.entry(key(&p)).or_insert_with(|| out.add_vertex(p))
    };
    // Edges and the faces each one bounds.
    let mut edges = Vec::new();
    let mut by_face: Vec<Vec<(usize, Sense)>> = vec![Vec::new(); nf];
    for (ei, poly) in mesh.edges.iter().enumerate() {
        let pts: Vec<Point3> = poly.iter().map(|p| p3(*p)).collect();
        if pts.len() < 2 {
            continue;
        }
        let (a, b) = (vertex(out, pts[0]), vertex(out, pts[pts.len() - 1]));
        let curve = curve_of(&pts);
        let e = match &curve {
            Curve::Circle(_) if a == b => {
                let cid = out.add_curve(curve.clone());
                out.add_edge(
                    cid,
                    a,
                    b,
                    super::Interval::new(0.0, std::f64::consts::TAU).map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?
            }
            _ => {
                let range = curve_range(&curve, &pts)?;
                let cid = out.add_curve(curve);
                out.add_edge(cid, a, b, range).map_err(|e| e.to_string())?
            }
        };
        edges.push((e, pts.clone()));
        let faces: Vec<usize> = mesh
            .edge_faces
            .get(ei)
            .map(|v| v.iter().map(|&f| f as usize).collect())
            .unwrap_or_default();
        let (k0, k1) = (key(&pts[0]), key(&pts[1]));
        let mut distinct = faces.clone();
        distinct.sort_unstable();
        distinct.dedup();
        // Two faces must run the edge in opposite directions. When the
        // triangle winding says the same for both (a sphere split in three
        // pieces met by a disc, FreeCAD #19184), the geometry decides.
        if let [f, g] = distinct[..]
            && faces.len() == 2
            && f < nf
            && g < nf
            && directed[f].contains(&(k0, k1)) == directed[g].contains(&(k0, k1))
            && let (Some(sf), Some(sg_)) = (
                sense_by_geometry(&meshes[f], &pts),
                sense_by_geometry(&meshes[g], &pts),
            )
            && sf != sg_
        {
            by_face[f].push((edges.len() - 1, sf));
            by_face[g].push((edges.len() - 1, sg_));
            continue;
        }
        for &f in &distinct {
            if f >= nf {
                continue;
            }
            // A seam: the face meets itself, once in each direction.
            if faces.iter().filter(|&&g| g == f).count() > 1 || distinct.len() == 1 {
                by_face[f].push((edges.len() - 1, Sense::Same));
                by_face[f].push((edges.len() - 1, Sense::Reversed));
                continue;
            }
            let sense = if directed[f].contains(&(k0, k1)) {
                Sense::Same
            } else {
                Sense::Reversed
            };
            by_face[f].push((edges.len() - 1, sense));
        }
    }

    // The faces' surfaces in parallel: recognising a fillet's torus samples
    // the face (72 hole rounds: 470 ms in one thread).
    let surfaces: Vec<Option<Surface>> = {
        let threads = std::thread::available_parallelism()
            .map_or(1, |n| n.get())
            .min(nf.max(1));
        let chunk = nf.div_ceil(threads.max(1)).max(1);
        let mut out_s: Vec<Option<Surface>> = vec![None; nf];
        std::thread::scope(|sc| {
            let handles: Vec<_> = (0..nf)
                .step_by(chunk)
                .map(|start| {
                    let end = (start + chunk).min(nf);
                    sc.spawn(move || {
                        (
                            start,
                            (start..end)
                                .map(|f| surface_of(body, f))
                                .collect::<Vec<_>>(),
                        )
                    })
                })
                .collect();
            for h in handles {
                if let Ok((start, v)) = h.join() {
                    for (k, sfc) in v.into_iter().enumerate() {
                        out_s[start + k] = sfc;
                    }
                }
            }
        });
        out_s
    };
    // Faces: loops chained from their coedges.
    let mut face_ids = Vec::with_capacity(nf);
    for f in 0..nf {
        let coedges: Vec<Coedge> = by_face[f]
            .iter()
            .map(|&(i, sense)| Coedge {
                edge: edges[i].0,
                sense,
            })
            .collect();
        let loops = chain_loops(out, coedges);
        let Some((outer, inner)) = split_outer(out, loops) else {
            continue;
        };
        let surface = surfaces[f]
            .clone()
            .unwrap_or_else(|| Surface::Plane(Plane::xy()));
        let sid = out.add_surface(surface);
        let fid = out.faces.insert(super::Face {
            surface: sid,
            outer,
            inner,
            sense: Sense::Same,
            name: s.face_names.get(f).cloned().flatten(),
            facet_of: None,
            mesh: Some(Arc::new(std::mem::take(&mut meshes[f]))),
        });
        face_ids.push(fid);
    }
    // One shell per connected set of faces; a shell inside another one's
    // box is a cavity of it, every other shell a solid of its own (same
    // body name). Putting them all into one shell broke Euler's formula.
    for (outer, cavities) in shells_of(out, &face_ids) {
        let mut shells = vec![outer];
        shells.extend(cavities);
        let solid = out.solids.insert(Solid {
            shells,
            open: false,
            name: s.name.clone(),
        });
        name_edges_and_vertices(out, solid, &s.profiles);
    }
    Ok(())
}

/// Splits faces into connected shells (sharing edges) and groups them as
/// (outer shell, its cavities).
fn shells_of(out: &BRep, faces: &[FaceId]) -> Vec<(Shell, Vec<Shell>)> {
    let n = faces.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut [usize], i: usize) -> usize {
        let mut r = i;
        while p[r] != r {
            r = p[r];
        }
        let mut j = i;
        while p[j] != r {
            let next = p[j];
            p[j] = r;
            j = next;
        }
        r
    }
    let mut by_edge: HashMap<super::EdgeId, usize> = HashMap::new();
    for (i, &f) in faces.iter().enumerate() {
        let Some(face) = out.face(f) else { continue };
        for lp in std::iter::once(&face.outer).chain(&face.inner) {
            for ce in &lp.coedges {
                match by_edge.get(&ce.edge) {
                    Some(&j) => {
                        let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                        parent[a] = b;
                    }
                    None => {
                        by_edge.insert(ce.edge, i);
                    }
                }
            }
        }
    }
    let mut groups: HashMap<usize, Vec<FaceId>> = HashMap::new();
    for (i, &f) in faces.iter().enumerate() {
        let r = find(&mut parent, i);
        groups.entry(r).or_default().push(f);
    }
    let mut groups: Vec<Vec<FaceId>> = groups.into_values().collect();
    // Deterministic order: by the smallest face position in `faces`.
    let pos = |f: &FaceId| faces.iter().position(|g| g == f).unwrap_or(usize::MAX);
    groups.sort_by_key(|g| g.iter().map(pos).min());
    if groups.len() == 1 {
        return vec![(
            Shell {
                faces: groups.remove(0),
            },
            Vec::new(),
        )];
    }
    // Boxes of the groups from their vertices.
    let bbox = |g: &[FaceId]| -> (Point3, Point3) {
        let mut lo = Point3::new(Real::MAX, Real::MAX, Real::MAX);
        let mut hi = Point3::new(Real::MIN, Real::MIN, Real::MIN);
        for f in g {
            if let Some(m) = out.face(*f).and_then(|x| x.mesh.as_ref()) {
                for p in &m.positions {
                    lo = lo.inf(p);
                    hi = hi.sup(p);
                }
            }
        }
        (lo, hi)
    };
    let boxes: Vec<(Point3, Point3)> = groups.iter().map(|g| bbox(g)).collect();
    let inside = |a: usize, b: usize| {
        let ((la, ha), (lb, hb)) = (boxes[a], boxes[b]);
        a != b
            && (0..3).all(|k| la[k] >= lb[k] - 1e-9 && ha[k] <= hb[k] + 1e-9)
            && (0..3).any(|k| la[k] > lb[k] + 1e-9 || ha[k] < hb[k] - 1e-9)
    };
    let mut out_groups: Vec<(Shell, Vec<Shell>)> = Vec::new();
    // The smallest box that contains a group owns it as a cavity.
    let owner: Vec<Option<usize>> = (0..groups.len())
        .map(|i| {
            (0..groups.len())
                .filter(|&j| inside(i, j))
                .min_by(|&a, &b| {
                    let v = |k: usize| (boxes[k].1 - boxes[k].0).product();
                    v(a).total_cmp(&v(b))
                })
        })
        .collect();
    let mut index: HashMap<usize, usize> = HashMap::new();
    for (i, g) in groups.iter().enumerate() {
        if owner[i].is_none() {
            index.insert(i, out_groups.len());
            out_groups.push((Shell { faces: g.clone() }, Vec::new()));
        }
    }
    for (i, g) in groups.iter().enumerate() {
        if let Some(o) = owner[i] {
            match index.get(&o) {
                Some(&k) => out_groups[k].1.push(Shell { faces: g.clone() }),
                // A cavity of a cavity (a part inside a hollow): a solid.
                None => out_groups.push((Shell { faces: g.clone() }, Vec::new())),
            }
        }
    }
    out_groups
}

/// The parameter range of a polyline's curve (line: its length; spline:
/// its domain).
fn curve_range(c: &Curve, pts: &[Point3]) -> Result<super::Interval, String> {
    let r = match c {
        Curve::Line(_) => super::Interval::new(0.0, (pts[pts.len() - 1] - pts[0]).norm()),
        Curve::Circle(k) => {
            let ang = |p: &Point3| {
                let v = p - k.center;
                v.dot(&k.y_axis()).atan2(v.dot(&k.x_axis))
            };
            let a0 = ang(&pts[0]);
            // Accumulate the turn along the polyline.
            let mut t = a0;
            for w in pts.windows(2) {
                let (a, b) = (ang(&w[0]), ang(&w[1]));
                let mut d = b - a;
                while d > std::f64::consts::PI {
                    d -= std::f64::consts::TAU;
                }
                while d < -std::f64::consts::PI {
                    d += std::f64::consts::TAU;
                }
                t += d;
            }
            if t >= a0 {
                super::Interval::new(a0, t)
            } else {
                // The circle runs the other way: flip its frame.
                super::Interval::new(t, a0)
            }
        }
        Curve::BSpline(b) => {
            let d = b.domain();
            super::Interval::new(d.min, d.max)
        }
    };
    r.map_err(|e| e.to_string())
}

/// Orders coedges into closed loops (best effort: leftovers form loops of
/// their own).
fn chain_loops(out: &BRep, mut rest: Vec<Coedge>) -> Vec<Loop> {
    let ends = |ce: &Coedge| -> Option<(VertexId, VertexId)> {
        let e = out.edge(ce.edge)?;
        Some(match ce.sense {
            Sense::Same => (e.start, e.end),
            Sense::Reversed => (e.end, e.start),
        })
    };
    let mut loops = Vec::new();
    while !rest.is_empty() {
        let first = rest.remove(0);
        let Some((start, mut at)) = ends(&first) else {
            continue;
        };
        let mut lp = vec![first];
        while at != start {
            let Some(k) = rest
                .iter()
                .position(|ce| ends(ce).is_some_and(|(s, _)| s == at))
            else {
                break;
            };
            let ce = rest.remove(k);
            at = ends(&ce).map_or(at, |(_, e)| e);
            lp.push(ce);
        }
        loops.push(Loop { coedges: lp });
    }
    loops
}

/// The outer loop (the longest) and the holes.
fn split_outer(out: &BRep, mut loops: Vec<Loop>) -> Option<(Loop, Vec<Loop>)> {
    if loops.is_empty() {
        return None;
    }
    let len = |lp: &Loop| -> Real {
        lp.coedges
            .iter()
            .filter_map(|ce| out.edge(ce.edge))
            .map(|e| e.range.length().abs())
            .sum()
    };
    let k = (0..loops.len())
        .max_by(|&a, &b| len(&loops[a]).total_cmp(&len(&loops[b])))
        .unwrap_or(0);
    let outer = loops.remove(k);
    Some((outer, loops))
}

/// A vertex of an extrusion: `<prefix>:VS:<segment>` (`VE` on the end cap)
/// for the segment that starts there.
fn extrusion_vertex(names: &[String], p: &Point3, profiles: &[ProfileFrame]) -> Option<String> {
    for pf in profiles {
        let pre = &pf.prefix;
        let level = if names.iter().any(|n| *n == format!("{pre}:Start")) {
            "S"
        } else if names.iter().any(|n| *n == format!("{pre}:End")) {
            "E"
        } else {
            continue;
        };
        let side = format!("{pre}:Side:");
        let segs: Vec<&str> = names.iter().filter_map(|n| n.strip_prefix(&side)).collect();
        if segs.is_empty() {
            continue;
        }
        let q = pf.plane.project(p);
        let best = pf
            .starts
            .iter()
            .filter(|(n, _)| segs.contains(&n.as_str()))
            .min_by(|a, b| (a.1 - q).norm().total_cmp(&(b.1 - q).norm()))?;
        return Some(format!("{pre}:V{level}:{}", best.0));
    }
    None
}

/// An edge of an extrusion: `<prefix>:StartEdge:<segment>` / `EndEdge`
/// between a side and a cap, `<prefix>:Seam:<segment>` between two sides
/// (the segment starting at its end vertices `va`, `vb`).
fn extrusion_edge(names: &[String], va: &str, vb: &str) -> Option<String> {
    let [a, b] = names else { return None };
    for (x, y) in [(a, b), (b, a)] {
        let Some((pre, seg)) = x.split_once(":Side:") else {
            continue;
        };
        if *y == format!("{pre}:Start") {
            return Some(format!("{pre}:StartEdge:{seg}"));
        }
        if *y == format!("{pre}:End") {
            return Some(format!("{pre}:EndEdge:{seg}"));
        }
        if y.starts_with(&format!("{pre}:Side:")) {
            for v in [va, vb] {
                for lvl in ["VS:", "VE:"] {
                    if let Some(seg) = v.strip_prefix(&format!("{pre}:{lvl}")) {
                        return Some(format!("{pre}:Seam:{seg}"));
                    }
                }
            }
        }
    }
    None
}

/// Edge and vertex names from the faces around them (box corners keep
/// their codes).
fn name_edges_and_vertices(out: &mut BRep, solid: super::SolidId, profiles: &[ProfileFrame]) {
    let Some(s) = out.solid(solid).cloned() else {
        return;
    };
    let mut edge_faces: HashMap<super::EdgeId, Vec<String>> = HashMap::new();
    for &f in s.shells.iter().flat_map(|sh| sh.faces.iter()) {
        let Some(face) = out.face(f) else { continue };
        let name = face.name.clone().unwrap_or_default();
        for lp in std::iter::once(&face.outer).chain(&face.inner) {
            for ce in &lp.coedges {
                let v = edge_faces.entry(ce.edge).or_default();
                if !v.contains(&name) {
                    v.push(name.clone());
                }
            }
        }
    }
    let mut vertex_faces: HashMap<VertexId, Vec<String>> = HashMap::new();
    for (e, names) in &edge_faces {
        if let Some(x) = out.edge(*e) {
            for v in [x.start, x.end] {
                let list = vertex_faces.entry(v).or_default();
                for n in names {
                    if !list.contains(n) {
                        list.push(n.clone());
                    }
                }
            }
        }
    }
    // Box roles: Left/Right → x bit, Front/Back → y, Bottom/Top → z.
    let box_code = |names: &[String]| -> Option<(String, String)> {
        let mut prefix = None;
        let mut bits = [None; 3];
        for n in names {
            let (p, role) = n.rsplit_once(':')?;
            if prefix.get_or_insert(p) != &p {
                return None;
            }
            let (i, b) = match role {
                "Left" => (0, 0),
                "Right" => (0, 1),
                "Front" => (1, 0),
                "Back" => (1, 1),
                "Bottom" => (2, 0),
                "Top" => (2, 1),
                _ => return None,
            };
            bits[i] = Some(b);
        }
        Some((
            prefix?.to_owned(),
            bits.iter()
                .map(|b| b.map_or("x".to_owned(), |b: u8| b.to_string()))
                .collect(),
        ))
    };
    let mut vnames: HashMap<VertexId, String> = HashMap::new();
    let mut used: HashMap<String, usize> = HashMap::new();
    let mut verts: Vec<(VertexId, Vec<String>)> = vertex_faces.into_iter().collect();
    verts.sort_by(|a, b| {
        let (pa, pb) = (
            out.vertex(a.0).map(|v| v.point),
            out.vertex(b.0).map(|v| v.point),
        );
        let k = |p: Option<Point3>| p.map(|p| key(&p)).unwrap_or_default();
        k(pa).cmp(&k(pb))
    });
    for (v, mut names) in verts {
        names.sort();
        let point = out.vertex(v).map(|x| x.point);
        let base = match box_code(&names) {
            Some((p, code)) if !code.contains('x') => format!("{p}:V{code}"),
            _ => point
                .and_then(|pt| extrusion_vertex(&names, &pt, profiles))
                .unwrap_or_else(|| format!("V[{}]", names.join("|"))),
        };
        let k = used.entry(base.clone()).or_insert(0);
        *k += 1;
        let name = if *k == 1 { base } else { format!("{base}#{k}") };
        vnames.insert(v, name.clone());
        if let Some(x) = out.vertices.get_mut(v) {
            x.name = Some(name);
        }
    }
    let mut used: HashMap<String, usize> = HashMap::new();
    let mut es: Vec<(super::EdgeId, Vec<String>)> = edge_faces.into_iter().collect();
    es.sort_by_key(|(e, _)| {
        out.edge(*e)
            .and_then(|x| out.vertex(x.start).map(|v| key(&v.point)))
            .unwrap_or_default()
    });
    for (e, mut names) in es {
        names.sort();
        let Some(x) = out.edge(e).cloned() else {
            continue;
        };
        let (va, vb) = (
            vnames.get(&x.start).cloned().unwrap_or_default(),
            vnames.get(&x.end).cloned().unwrap_or_default(),
        );
        // A box edge: `<prefix>:E<a>-<b>` with the corner codes sorted.
        let boxed = match (va.rsplit_once(":V"), vb.rsplit_once(":V")) {
            (Some((p, a)), Some((q, b))) if p == q && box_code(&names).is_some() => {
                let (a, b) = if a <= b { (a, b) } else { (b, a) };
                Some(format!("{p}:E{a}-{b}"))
            }
            _ => None,
        };
        let base = boxed
            .or_else(|| extrusion_edge(&names, &va, &vb))
            .unwrap_or_else(|| format!("E[{}]", names.join("|")));
        let k = used.entry(base.clone()).or_insert(0);
        *k += 1;
        let name = if *k == 1 { base } else { format!("{base}#{k}") };
        if let Some(x) = out.edges.get_mut(e) {
            x.name = Some(name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rounded_box() -> ExactSolid {
        let b = box_named(
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(40.0, 30.0, 20.0),
            "Box1",
        )
        .unwrap();
        // The four vertical edges.
        let pts: Vec<Point3> = [(0.0, 0.0), (40.0, 0.0), (40.0, 30.0), (0.0, 30.0)]
            .iter()
            .map(|&(x, y)| Point3::new(x, y, 10.0))
            .collect();
        fillet(&b, &pts, 5.0, "Fillet1").unwrap()
    }

    #[test]
    fn box_faces_are_named_and_valid() {
        let b = box_named(Point3::origin(), Point3::new(40.0, 30.0, 20.0), "Box1").unwrap();
        let (vol, _, faces) = b.measure().unwrap();
        assert!((vol - 24_000.0).abs() < 1e-6);
        assert_eq!(faces, 6);
        let mut names: Vec<String> = b.names().iter().flatten().cloned().collect();
        names.sort();
        assert_eq!(
            names,
            [
                "Box1:Back",
                "Box1:Bottom",
                "Box1:Front",
                "Box1:Left",
                "Box1:Right",
                "Box1:Top"
            ]
        );
        let brep = to_brep(&[b]).unwrap();
        let (sid, _) = brep.solids().next().unwrap();
        let stats = brep.validate_solid(sid).unwrap();
        assert_eq!(stats.euler_characteristic(), 2, "{stats:?}");
        assert!(
            brep.vertices()
                .any(|(_, v)| v.name.as_deref() == Some("Box1:V111"))
        );
        assert!(
            brep.edges()
                .any(|(_, e)| e.name.as_deref() == Some("Box1:E000-100"))
        );
    }

    #[test]
    fn fillet_keeps_names_and_converts() {
        let r = rounded_box();
        let (vol, _, faces) = r.measure().unwrap();
        let expect = 24_000.0 - 4.0 * (25.0 - std::f64::consts::PI * 25.0 / 4.0) * 20.0;
        assert!((vol - expect).abs() / expect < 1e-4, "{vol} vs {expect}");
        assert_eq!(faces, 10);
        assert!(r.names().iter().any(|n| n.as_deref() == Some("Box1:Top")));
        assert!(
            r.names()
                .iter()
                .any(|n| n.as_deref() == Some("Fillet1:Round[Box1:Front|Box1:Right]")),
            "{:?}",
            r.names()
        );
        assert_eq!(
            r.names()
                .iter()
                .filter(|n| n
                    .as_deref()
                    .is_some_and(|n| n.starts_with("Fillet1:Round[")))
                .count(),
            4
        );
        let brep = to_brep(&[r]).unwrap();
        let (sid, _) = brep.solids().next().unwrap();
        let stats = brep.validate_solid(sid).unwrap();
        assert_eq!(stats.euler_characteristic(), 2, "{stats:?}");
        let curved = brep
            .faces()
            .filter(|(_, f)| matches!(brep.surface(f.surface), Some(Surface::Cylinder(_))))
            .count();
        assert_eq!(curved, 4);
    }

    #[test]
    fn extrude_with_hole_names_sides_and_caps() {
        use super::super::extrude::ProfileSegment;
        let line = |a: (f64, f64), b: (f64, f64), n: &str| ProfileSegment {
            geometry: Segment2::Line {
                a: nalgebra::Point2::new(a.0, a.1),
                b: nalgebra::Point2::new(b.0, b.1),
            },
            name: n.to_owned(),
        };
        let outer = vec![
            line((0.0, 0.0), (20.0, 0.0), "L1:0"),
            line((20.0, 0.0), (20.0, 10.0), "L2:0"),
            line((20.0, 10.0), (0.0, 10.0), "L3:0"),
            line((0.0, 10.0), (0.0, 0.0), "L4:0"),
        ];
        let hole = vec![ProfileSegment {
            geometry: Segment2::Arc {
                c: nalgebra::Point2::new(10.0, 5.0),
                r: 2.0,
                a0: 0.0,
                sweep: -std::f64::consts::TAU,
            },
            name: "C1:0".into(),
        }];
        let prof = profile(&outer, &[hole]).unwrap();
        let s = extrude(&Plane::xy(), &prof, 0.0, 5.0, 0.0, "Extrusion1").unwrap();
        let (vol, _, _) = s.measure().unwrap();
        let expect = (200.0 - std::f64::consts::PI * 4.0) * 5.0;
        assert!((vol - expect).abs() / expect < 1e-4, "{vol} vs {expect}");
        let names: Vec<String> = s.names().iter().flatten().cloned().collect();
        for n in [
            "Extrusion1:Start",
            "Extrusion1:End",
            "Extrusion1:Side:L1:0",
            "Extrusion1:Side:C1:0",
        ] {
            assert!(names.iter().any(|x| x == n), "{n} missing in {names:?}");
        }
        let brep = to_brep(&[s]).unwrap();
        let (sid, _) = brep.solids().next().unwrap();
        brep.validate_solid(sid).unwrap();
    }

    /// A union of bodies apart is one kernel body with two shells: our
    /// B-rep gets two valid solids (one shell put them into one and broke
    /// Euler's formula; found by tests/boolean_identity.rs).
    #[test]
    fn union_of_bodies_apart_gives_valid_solids() {
        let a = box_named(Point3::origin(), Point3::new(10.0, 10.0, 10.0), "A").unwrap();
        let b = box_named(
            Point3::new(20.0, 0.0, 0.0),
            Point3::new(30.0, 10.0, 10.0),
            "B",
        )
        .unwrap();
        let u = boolean(&a, &b, ExactOp::Union).unwrap().unwrap();
        let brep = to_brep(&[u]).unwrap();
        assert_eq!(brep.solids().count(), 2);
        for (id, _) in brep.solids() {
            let st = brep.validate_solid(id).unwrap();
            assert_eq!(st.euler_characteristic(), 2, "{st:?}");
        }
    }

    #[test]
    fn panics_become_errors() {
        let r: Result<(), String> = guarded("test", || panic!("boom"));
        assert!(r.is_err());
    }

    // ------------------------------------------------------------ shell ---

    use std::f64::consts::PI;

    /// A prism of the closed polygon `pts` (counter-clockwise), `h` high.
    fn prism(pts: &[(f64, f64)], h: f64, id: &str) -> ExactSolid {
        use super::super::extrude::ProfileSegment;
        let segs: Vec<ProfileSegment> = (0..pts.len())
            .map(|i| {
                let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                ProfileSegment {
                    geometry: Segment2::Line {
                        a: nalgebra::Point2::new(a.0, a.1),
                        b: nalgebra::Point2::new(b.0, b.1),
                    },
                    name: format!("L{i}:0"),
                }
            })
            .collect();
        let prof = profile(&segs, &[]).unwrap();
        extrude(&Plane::xy(), &prof, 0.0, h, 0.0, id).unwrap()
    }

    fn cylinder(r: f64, h: f64, id: &str) -> ExactSolid {
        use super::super::extrude::ProfileSegment;
        let c = vec![ProfileSegment {
            geometry: Segment2::Arc {
                c: nalgebra::Point2::new(0.0, 0.0),
                r,
                a0: 0.0,
                sweep: std::f64::consts::TAU,
            },
            name: "C1:0".into(),
        }];
        let prof = profile(&c, &[]).unwrap();
        extrude(&Plane::xy(), &prof, 0.0, h, 0.0, id).unwrap()
    }

    fn face(s: &ExactSolid, name: &str, hint: (f64, f64, f64)) -> usize {
        faces_named(s, name, Point3::new(hint.0, hint.1, hint.2)).unwrap()[0]
    }

    /// Volume within 3e-4 (relative: the kernel measures curved faces on
    /// its triangles) and a valid B-rep.
    fn check_shell(s: &ExactSolid, expect: f64) {
        let (vol, _, _) = s.measure().unwrap();
        assert!((vol - expect).abs() / expect < 3e-4, "{vol} vs {expect}");
        let brep = to_brep(std::slice::from_ref(s)).unwrap();
        for (id, _) in brep.solids() {
            brep.validate_solid(id).unwrap();
        }
    }

    fn box_40() -> ExactSolid {
        box_named(Point3::origin(), Point3::new(40.0, 30.0, 20.0), "Box1").unwrap()
    }

    #[test]
    fn shell_inside_open_top_of_a_box() {
        let b = box_40();
        let top = face(&b, "Box1:Top", (20.0, 15.0, 20.0));
        let s = shell(&b, &[top], 2.0, ShellSide::Inside, "Shell1").unwrap();
        check_shell(&s, 24_000.0 - 36.0 * 26.0 * 18.0);
        // The rim round the opening is what is left of the top: it keeps
        // the top's name; 6 outer + 5 inner faces.
        let names: Vec<&str> = s.names().iter().flatten().map(String::as_str).collect();
        assert_eq!(names.len(), 11, "{names:?}");
        for n in [
            "Box1:Top",
            "Box1:Front",
            "Shell1:Inner[Box1:Front]",
            "Shell1:Inner[Box1:Bottom]",
        ] {
            assert!(names.contains(&n), "{n} missing in {names:?}");
        }
        assert!(!names.iter().any(|n| n.contains("Inner[Box1:Top]")));
    }

    #[test]
    fn shell_outside_and_both_of_a_box() {
        let b = box_40();
        let top = face(&b, "Box1:Top", (20.0, 15.0, 20.0));
        let out = shell(&b, &[top], 2.0, ShellSide::Outside, "Shell1").unwrap();
        // The walls grow out (the open top stays at z = 20); the old box is
        // the cavity.
        check_shell(&out, 44.0 * 34.0 * 22.0 - 24_000.0);
        let m = out.measure_full().unwrap();
        assert!((m.bbox.0.z + 2.0).abs() < 1e-6 && (m.bbox.1.z - 20.0).abs() < 1e-6);
        let names: Vec<&str> = out.names().iter().flatten().map(String::as_str).collect();
        assert!(names.contains(&"Shell1:Inner[Box1:Front]"), "{names:?}");
        let both = shell(&b, &[top], 2.0, ShellSide::Both, "Shell1").unwrap();
        check_shell(&both, 42.0 * 32.0 * 21.0 - 38.0 * 28.0 * 19.0);
    }

    #[test]
    fn shell_of_a_cylinder_open_top() {
        let c = cylinder(10.0, 20.0, "Extrusion1");
        let top = face(&c, "Extrusion1:End", (0.0, 0.0, 20.0));
        let s = shell(&c, &[top], 2.0, ShellSide::Inside, "Shell1").unwrap();
        check_shell(&s, PI * 100.0 * 20.0 - PI * 64.0 * 18.0);
        let out = shell(&c, &[top], 2.0, ShellSide::Outside, "Shell1").unwrap();
        check_shell(&out, PI * 144.0 * 22.0 - PI * 100.0 * 20.0);
    }

    #[test]
    fn shell_of_a_non_convex_prism() {
        // An L: 30 × 30 less the 20 × 20 corner, 10 high.
        let l = prism(
            &[
                (0.0, 0.0),
                (30.0, 0.0),
                (30.0, 10.0),
                (10.0, 10.0),
                (10.0, 30.0),
                (0.0, 30.0),
            ],
            10.0,
            "Extrusion1",
        );
        let top = face(&l, "Extrusion1:End", (5.0, 5.0, 10.0));
        let s = shell(&l, &[top], 1.0, ShellSide::Inside, "Shell1").unwrap();
        // Cavity: the L offset in by 1 (area 28·8 + 8·20), 9 deep.
        let cavity = (28.0 * 8.0 + 8.0 * 20.0) * 9.0;
        check_shell(&s, 500.0 * 10.0 - cavity);
    }

    #[test]
    fn shell_with_two_open_faces_and_rounded_walls() {
        // A box open at the top and the front.
        let b = box_40();
        let top = face(&b, "Box1:Top", (20.0, 15.0, 20.0));
        let front = face(&b, "Box1:Front", (20.0, 0.0, 10.0));
        let s = shell(&b, &[top, front], 2.0, ShellSide::Inside, "Shell1").unwrap();
        check_shell(&s, 24_000.0 - 36.0 * 28.0 * 18.0);
        // Rounded vertical edges (r 5): the inner rounds have radius 3.
        let r = rounded_box();
        let top = face(&r, "Box1:Top", (20.0, 15.0, 20.0));
        let s = shell(&r, &[top], 2.0, ShellSide::Inside, "Shell1").unwrap();
        let outer = 24_000.0 - 4.0 * (25.0 - PI * 25.0 / 4.0) * 20.0;
        let inner = (36.0 * 26.0 - 4.0 * (9.0 - PI * 9.0 / 4.0)) * 18.0;
        check_shell(&s, outer - inner);
        // Every face named (rule 2), also walls the history does not trace.
        assert!(s.names().iter().all(Option::is_some), "{:?}", s.names());
        assert!(
            s.names()
                .iter()
                .flatten()
                .any(|n| n.starts_with("Shell1:Inner"))
        );
    }

    #[test]
    fn shell_without_open_faces_leaves_a_closed_cavity() {
        let b = box_40();
        let s = shell(&b, &[], 2.0, ShellSide::Inside, "Shell1").unwrap();
        check_shell(&s, 24_000.0 - 36.0 * 26.0 * 16.0);
    }

    // ------------------------------------------------------------ draft ---

    fn simpson(f: &dyn Fn(f64) -> f64, a: f64, b: f64) -> f64 {
        (b - a) / 6.0 * (f(a) + 4.0 * f((a + b) / 2.0) + f(b))
    }

    fn sides(b: &ExactSolid) -> Vec<usize> {
        ["Box1:Front", "Box1:Back", "Box1:Left", "Box1:Right"]
            .iter()
            .map(|n| face(b, n, (20.0, 15.0, 10.0)))
            .collect()
    }

    #[test]
    fn draft_box_sides_about_the_bottom() {
        let b = box_40();
        let k = 5f64.to_radians().tan();
        let up = Vector3::new(0.0, 0.0, 1.0);
        let d = draft(&b, &sides(&b), &Plane::xy(), up, 5f64.to_radians()).unwrap();
        check_shell(
            &d,
            simpson(&|z| (40.0 - 2.0 * z * k) * (30.0 - 2.0 * z * k), 0.0, 20.0),
        );
        // The faces keep their names.
        let names: Vec<&str> = d.names().iter().flatten().map(String::as_str).collect();
        for n in ["Box1:Front", "Box1:Top", "Box1:Bottom"] {
            assert!(names.contains(&n), "{n} in {names:?}");
        }
        // About the top (the faces lean out below it).
        let top = Plane::from_normal(Point3::new(0.0, 0.0, 20.0), up).unwrap();
        let d = draft(&b, &sides(&b), &top, up, 5f64.to_radians()).unwrap();
        check_shell(
            &d,
            simpson(
                &|z| (40.0 + 2.0 * (20.0 - z) * k) * (30.0 + 2.0 * (20.0 - z) * k),
                0.0,
                20.0,
            ),
        );
    }

    #[test]
    fn draft_one_face_and_a_negative_angle() {
        let b = box_40();
        let k = 5f64.to_radians().tan();
        let up = Vector3::new(0.0, 0.0, 1.0);
        let front = face(&b, "Box1:Front", (20.0, 0.0, 10.0));
        let d = draft(&b, &[front], &Plane::xy(), up, 5f64.to_radians()).unwrap();
        check_shell(&d, 40.0 * (600.0 - 200.0 * k));
        let d = draft(&b, &[front], &Plane::xy(), up, -(5f64.to_radians())).unwrap();
        check_shell(&d, 40.0 * (600.0 + 200.0 * k));
    }

    #[test]
    fn draft_of_a_cylinder_wall() {
        // SolidCraft #547: R10 × 20 at 5° gives a frustum.
        let c = cylinder(10.0, 20.0, "Extrusion1");
        let wall = face(&c, "Extrusion1:Side:C1:0", (10.0, 0.0, 10.0));
        let up = Vector3::new(0.0, 0.0, 1.0);
        let d = draft(&c, &[wall], &Plane::xy(), up, 5f64.to_radians()).unwrap();
        let r = 10.0 - 20.0 * 5f64.to_radians().tan();
        check_shell(&d, PI * 20.0 / 3.0 * (100.0 + 10.0 * r + r * r));
    }

    #[test]
    fn draft_of_a_non_convex_prism() {
        // The L's two outer long walls (x = 0 and y = 0), 3° about the bottom.
        let l = prism(
            &[
                (0.0, 0.0),
                (30.0, 0.0),
                (30.0, 10.0),
                (10.0, 10.0),
                (10.0, 30.0),
                (0.0, 30.0),
            ],
            10.0,
            "Extrusion1",
        );
        let a = face(&l, "Extrusion1:Side:L0:0", (15.0, 0.0, 5.0));
        let b = face(&l, "Extrusion1:Side:L5:0", (0.0, 15.0, 5.0));
        let k = 3f64.to_radians().tan();
        let up = Vector3::new(0.0, 0.0, 1.0);
        let d = draft(&l, &[a, b], &Plane::xy(), up, 3f64.to_radians()).unwrap();
        // Each wall loses a wedge 30 long, 10 high, 10k deep at the top;
        // they overlap in the corner pyramid (k²·h³/3).
        let lost = 2.0 * 30.0 * 0.5 * 10.0 * 10.0 * k - k * k * 1000.0 / 3.0;
        check_shell(&d, 500.0 * 10.0 - lost);
    }

    #[test]
    fn draft_errors() {
        let b = box_40();
        let up = Vector3::new(0.0, 0.0, 1.0);
        let s = sides(&b);
        assert!(draft(&b, &s, &Plane::xy(), up, 90f64.to_radians()).is_err());
        assert!(draft(&b, &[], &Plane::xy(), up, 0.1).is_err());
    }

    /// A box with all 12 edges rounded `r`.
    fn all_rounded(r: f64) -> ExactSolid {
        let b = box_40();
        let mut pts = Vec::new();
        for &(x, y) in &[(20.0, 0.0), (40.0, 15.0), (20.0, 30.0), (0.0, 15.0)] {
            pts.push(Point3::new(x, y, 0.0));
            pts.push(Point3::new(x, y, 20.0));
        }
        for &(x, y) in &[(0.0, 0.0), (40.0, 0.0), (40.0, 30.0), (0.0, 30.0)] {
            pts.push(Point3::new(x, y, 10.0));
        }
        fillet(&b, &pts, r, "Fillet1").unwrap()
    }

    /// Volume of a box `a × b × c` with every edge rounded `r` (Steiner:
    /// the inner box grown by a ball).
    fn rounded_volume(a: f64, b: f64, c: f64, r: f64) -> f64 {
        let (p, q, s) = (a - 2.0 * r, b - 2.0 * r, c - 2.0 * r);
        p * q * s
            + 2.0 * (p * q + q * s + s * p) * r
            + PI * (p + q + s) * r * r
            + 4.0 / 3.0 * PI * r * r * r
    }

    #[test]
    fn shell_closed_of_a_fully_rounded_box() {
        let b = all_rounded(3.0);
        check_shell(&b, rounded_volume(40.0, 30.0, 20.0, 3.0));
        let s = shell(&b, &[], 2.0, ShellSide::Inside, "Shell1").unwrap();
        check_shell(
            &s,
            rounded_volume(40.0, 30.0, 20.0, 3.0) - rounded_volume(36.0, 26.0, 16.0, 1.0),
        );
    }

    /// Your rounded box: every edge rounded `r`, the top removed, walls 2.
    /// With r ≥ the wall the kernel refuses the rim; the opening is cut as a
    /// prism of the top face (rim strip square to the top, walls 2 thick).
    #[test]
    fn shell_open_top_of_a_fully_rounded_box() {
        for r in [1.0, 3.0, 5.0, 2.0] {
            let b = all_rounded(r);
            let top = face(&b, "Box1:Top", (20.0, 15.0, 20.0));
            let s = shell(&b, &[top], 2.0, ShellSide::Inside, "Shell1")
                .unwrap_or_else(|e| panic!("r {r}: {e}"));
            let outer = rounded_volume(40.0, 30.0, 20.0, r);
            let expect = if r < 2.0 {
                // The kernel's own shell (inner corners sharp where r < t):
                // checked for validity only.
                s.measure().unwrap().0
            } else {
                outer
                    - rounded_volume(36.0, 26.0, 16.0, r - 2.0)
                    - (40.0 - 2.0 * r) * (30.0 - 2.0 * r) * 2.0
            };
            check_shell(&s, expect);
        }
    }

    #[test]
    fn shell_errors() {
        let b = box_40();
        let top = face(&b, "Box1:Top", (20.0, 15.0, 20.0));
        assert!(shell(&b, &[top], 0.0, ShellSide::Inside, "Shell1").is_err());
        assert!(shell(&b, &[top], -1.0, ShellSide::Inside, "Shell1").is_err());
        // Walls thicker than half the body: nothing sensible is left.
        assert!(shell(&b, &[top], 16.0, ShellSide::Inside, "Shell1").is_err());
    }

    fn check_valid(s: &ExactSolid, vol: Real) {
        let (v, _, _) = s.measure().unwrap();
        assert!((v - vol).abs() < 1e-6 * vol.max(1.0), "volume {v} vs {vol}");
        let brep = to_brep(std::slice::from_ref(s)).unwrap();
        for (sid, _) in brep.solids() {
            let st = brep.validate_solid(sid).unwrap();
            assert_eq!(st.euler_characteristic(), 2, "{st:?}");
        }
    }

    #[test]
    fn translated_copy_keeps_volume_and_prefixes_names() {
        let b = box_named(Point3::origin(), Point3::new(10.0, 20.0, 5.0), "Box1").unwrap();
        let m = nalgebra::Matrix4::new_translation(&Vector3::new(30.0, 0.0, 0.0));
        let c = transformed(&b, &m, "RectangularPattern1#2", Some("P#2")).unwrap();
        check_valid(&c, 1000.0);
        assert_eq!(c.name.as_deref(), Some("P#2"));
        assert!(
            c.names()
                .iter()
                .flatten()
                .any(|n| n == "RectangularPattern1#2:Box1:Top")
        );
        let mm = c.measure_full().unwrap();
        assert!((mm.bbox.0.x - 30.0).abs() < 1e-9 && (mm.bbox.1.x - 40.0).abs() < 1e-9);
    }

    #[test]
    fn rotated_and_mirrored_copies_are_valid() {
        let b = box_named(
            Point3::new(10.0, 0.0, 0.0),
            Point3::new(20.0, 5.0, 4.0),
            "Box1",
        )
        .unwrap();
        let rot = nalgebra::Rotation3::from_axis_angle(&Vector3::z_axis(), 1.0);
        let c = transformed(&b, &rot.to_homogeneous(), "C#2", None).unwrap();
        check_valid(&c, 200.0);
        // Reflection in the YZ plane (determinant −1): still a solid with
        // positive volume, on the other side.
        let mut m = nalgebra::Matrix4::identity();
        m[(0, 0)] = -1.0;
        let r = transformed(&b, &m, "Mirror1#2", None).unwrap();
        check_valid(&r, 200.0);
        let mm = r.measure_full().unwrap();
        assert!((mm.bbox.0.x + 20.0).abs() < 1e-9 && (mm.bbox.1.x + 10.0).abs() < 1e-9);
        assert!(contains(&r, Point3::new(-15.0, 2.5, 2.0)));
        // Union with the original: twice the volume.
        let u = boolean(&b, &r, ExactOp::Union).unwrap().unwrap();
        assert!((u.measure().unwrap().0 - 400.0).abs() < 1e-6);
        // Scaling is refused.
        let s = nalgebra::Matrix4::new_scaling(2.0);
        assert!(transformed(&b, &s, "X", None).is_err());
    }
}

#[cfg(test)]
mod sphere_tests {
    use super::*;

    /// A quarter disc revolved about its edge on the axis: the kernel
    /// splits the sphere in three faces; the disc's arcs must still be run
    /// once each way (they were not: a non-manifold edge, FreeCAD #19184).
    #[test]
    fn revolved_hemisphere_converts_to_a_valid_solid() {
        use super::super::extrude::ProfileSegment;
        let p = |x: f64, y: f64| nalgebra::Point2::new(x, y);
        let line = |a, b, n: &str| ProfileSegment {
            geometry: Segment2::Line { a, b },
            name: n.into(),
        };
        let segs = vec![
            line(p(0.0, 0.0), p(10.0, 0.0), "a"),
            ProfileSegment {
                geometry: Segment2::Arc {
                    c: p(0.0, 0.0),
                    r: 10.0,
                    a0: 0.0,
                    sweep: std::f64::consts::FRAC_PI_2,
                },
                name: "b".into(),
            },
            line(p(0.0, 10.0), p(0.0, 0.0), "c"),
        ];
        let prof = profile(&segs, &[]).unwrap();
        let plane =
            Plane::new(Point3::origin(), Vector3::new(0.0, -1.0, 0.0), Vector3::x()).unwrap();
        let s = revolve(
            &plane,
            &prof,
            p(0.0, 0.0),
            nalgebra::Vector2::new(0.0, 1.0),
            0.0,
            std::f64::consts::TAU,
            "Revolution1",
        )
        .unwrap();
        let (vol, _, _) = s.measure().unwrap();
        let want = 2.0 / 3.0 * std::f64::consts::PI * 1000.0;
        assert!((vol - want).abs() / want < 3e-4, "{vol} vs {want}");
        let b = to_brep(&[s]).unwrap();
        for (id, _) in b.solids() {
            let st = b.validate_solid(id).unwrap();
            assert_eq!(st.euler_characteristic(), 2, "{st:?}");
        }
    }
}

#[cfg(test)]
mod hole_tests;
