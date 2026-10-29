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
    /// A surface body (Output Surface), even where its sheet closes.
    sheet: bool,
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
    match crate::diagnostics::crash::guard(f) {
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
    /// The size the kernel scales its tolerances with (the box diagonal).
    pub fn size(&self) -> Real {
        guarded("size", || Ok(self.body.size())).unwrap_or(1.0)
    }

    /// Volume from one mesh at chord tolerance `tol`, cached in the body (a
    /// clone shares it): cheap enough to compare two solids measured at the
    /// same `tol`. `measure_full` meshes three times, finely (4 s of a
    /// 400-hole pattern went into "did the cut remove anything").
    pub fn volume_at(&self, tol: Real) -> Option<Real> {
        guarded("volume", || Ok(self.body.volume_at(tol)))
            .ok()
            .flatten()
    }

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
        sheet: false,
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
        let closed = closed_loop(lp.iter().map(|s| s.geometry.clone()).collect());
        for (s, g) in lp.iter().zip(&closed) {
            starts.push((s.name.clone(), g.start()));
            let pieces = seg2(g)?;
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

/// The largest gap between neighbouring segments [`closed_loop`] closes:
/// the sketch solver leaves its points up to ~1e-6 mm off (a projected arc's
/// ends from the model, an offset loop after a solve), and the exact kernel
/// refuses a loop whose ends miss by more than 1e-6 ("profile loop is not
/// closed"). A bigger gap is a real one and stays.
const LOOP_SNAP: Real = 1e-4;

/// A closed loop with every joint made exact: neighbours meet at the middle
/// of their two ends (when they are less than [`LOOP_SNAP`] apart); a line
/// takes the joints as its ends, an arc keeps its sweep's sense and goes
/// through both joints (its centre moved onto their bisector), a spline's
/// end control points move to them.
fn closed_loop(mut segs: Vec<super::extrude::Segment2>) -> Vec<super::extrude::Segment2> {
    use super::extrude::Segment2 as S;
    let n = segs.len();
    if n < 2 {
        return segs;
    }
    // Joint i: between segment i's end and segment i + 1's start.
    let joints: Vec<Option<nalgebra::Point2<Real>>> = (0..n)
        .map(|i| {
            let (e, s) = (segs[i].end(), segs[(i + 1) % n].start());
            ((e - s).norm() <= LOOP_SNAP).then(|| nalgebra::center(&e, &s))
        })
        .collect();
    for i in 0..n {
        let start = joints[(i + n - 1) % n];
        let end = joints[i];
        match &mut segs[i] {
            S::Line { a, b } => {
                if let Some(p) = start {
                    *a = p;
                }
                if let Some(p) = end {
                    *b = p;
                }
            }
            S::Arc { c, r, a0, sweep } => {
                let (Some(s), Some(e)) = (start, end) else {
                    continue;
                };
                let chord = e - s;
                if chord.norm() <= 1e-12 {
                    continue;
                }
                let m = nalgebra::center(&s, &e);
                let nrm = nalgebra::Vector2::new(-chord.y, chord.x).normalize();
                let cc = m + nrm * (*c - m).dot(&nrm);
                let ang = |p: nalgebra::Point2<Real>| (p.y - cc.y).atan2(p.x - cc.x);
                let (b0, b1) = (ang(s), ang(e));
                // The same sense and (nearly) the same turn as before.
                let mut sw = b1 - b0;
                let tau = std::f64::consts::TAU;
                while sw - *sweep > std::f64::consts::PI {
                    sw -= tau;
                }
                while *sweep - sw > std::f64::consts::PI {
                    sw += tau;
                }
                *c = cc;
                *r = (s - cc).norm();
                *a0 = b0;
                *sweep = sw;
            }
            S::Spline {
                points, reversed, ..
            } => {
                let k = points.len();
                if k < 2 {
                    continue;
                }
                let (first, last) = if *reversed { (k - 1, 0) } else { (0, k - 1) };
                if let Some(p) = start {
                    points[first] = p;
                }
                if let Some(p) = end {
                    points[last] = p;
                }
            }
        }
    }
    segs
}

fn v2(p: &nalgebra::Point2<Real>) -> sg::Vec2 {
    sg::Vec2::new(p.x, p.y)
}

/// One of our profile segments as exact-kernel segments (a B-spline as its
/// Bézier pieces).
pub(crate) fn seg2(g: &Segment2) -> Result<Vec<sg::Seg2>, String> {
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
        sheet: false,
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
        // The kernel sweeps a positive angle clockwise about `d` (seen
        // from its tip): turn the start the same way.
        let rot = nalgebra::Rotation3::from_axis_angle(&nalgebra::Unit::new_normalize(d), -start);
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
        sheet: false,
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
            sheet: false,
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
        sheet: false,
        name: solids[0].name.clone(),
        body,
        face_names,
        profiles: solids
            .iter()
            .flat_map(|s| s.profiles.iter().cloned())
            .collect(),
    }))
}

/// Solids the caller knows are apart (no two meet or touch) as one solid
/// of several pieces: [`union_apart`] without its check, which meshes every
/// piece (2500 pattern copies: 6 s). Face names are kept.
pub fn join_apart(solids: &[ExactSolid]) -> Result<Option<ExactSolid>, String> {
    if solids.len() < 2 {
        return Ok(None);
    }
    let bodies: Vec<sk::Body> = solids.iter().map(|s| s.body.clone()).collect();
    let Some(body) = guarded("join apart", || sk::join_apart(&bodies))? else {
        return Ok(None);
    };
    let refs: Vec<&ExactSolid> = solids.iter().collect();
    let face_names = names_from(&body, &refs, &|_| None);
    Ok(Some(ExactSolid {
        sheet: false,
        name: solids[0].name.clone(),
        body,
        face_names,
        profiles: solids
            .iter()
            .flat_map(|s| s.profiles.iter().cloned())
            .collect(),
    }))
}

/// A box around the solid from a mesh, grown by twice its chord tolerance
/// so the true surface is inside (as `union_apart` measures). The
/// tolerance follows the box's smallest side, not the solid's size: a long
/// thin tool (a Through All hole r0.4, 800 mm long) meshed at its size's
/// 2 % grew by 32 mm, and copies 1 mm apart looked as if they met.
pub fn bounds(s: &ExactSolid) -> Option<(Point3, Point3)> {
    let mesh_box = |tol: Real| {
        let bb = guarded("bounds", || s.body.tessellate(tol).map(|m| m.bounds())).ok()?;
        (!bb.is_empty()).then_some((p3(bb.min), p3(bb.max)))
    };
    let coarse = (s.size() * 0.02).max(1e-3);
    let (lo, hi) = mesh_box(coarse)?;
    let side = (hi - lo).min();
    let tol = (side * 1e-2).clamp(1e-4, coarse);
    let (lo, hi) = if tol < coarse {
        mesh_box(tol)?
    } else {
        (lo, hi)
    };
    let pad = Vector3::repeat(2.0 * tol);
    Some((lo - pad, hi + pad))
}

/// Whether `p` lies inside the solid's material.
pub fn contains(s: &ExactSolid, p: Point3) -> bool {
    crate::diagnostics::crash::guard(|| s.body.contains(v3(&p))).unwrap_or(false)
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
                    sheet: false,
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

/// A body read from a STEP file by [`step_bodies`].
#[derive(Clone, Debug)]
pub struct StepBody {
    /// The body: named `<prefix>` (then `<prefix>#2` …), faces `<body>:F<entity>`.
    pub solid: ExactSolid,
    /// The file's label of the solid, else its product's name.
    pub label: String,
    /// Products from the root to the part holding it.
    pub path: Vec<String>,
    /// Surface colour from the file (RGB 0..1).
    pub color: Option<[f32; 3]>,
    /// A closed solid (false: a surface body).
    pub closed: bool,
}

/// What [`step_bodies`] read.
#[derive(Clone, Debug, Default)]
pub struct StepRead {
    /// The bodies, in the file's order.
    pub bodies: Vec<StepBody>,
    /// What the reader could not read.
    pub warnings: Vec<String>,
    /// The file's originating system.
    pub system: String,
}

/// The bodies of STEP `text` (mm, placed by the file's assembly structure), named after
/// `prefix`. A face is named by its STEP face entity (`<body>:F123`): fixed for one text, so
/// the names survive every rebuild; pieces of a face the kernel splits share it.
pub fn step_bodies(text: &str, prefix: &str) -> Result<StepRead, String> {
    let imp = guarded("STEP import", || sk::step_import_shared(text))?;
    let mut out = StepRead {
        warnings: imp.warnings.clone(),
        system: imp.system.clone(),
        ..Default::default()
    };
    for (k, b) in imp.bodies.iter().enumerate() {
        let name = if k == 0 {
            prefix.to_owned()
        } else {
            format!("{prefix}#{}", k + 1)
        };
        let n = b.body.face_count();
        let face_names = (0..n)
            .map(|i| b.face_origin.get(i).map(|e| format!("{name}:F{e}")))
            .collect();
        out.bodies.push(StepBody {
            solid: ExactSolid {
                sheet: !b.closed,
                name: Some(name),
                body: b.body.clone(),
                face_names,
                profiles: Vec::new(),
            },
            label: b.name.clone(),
            path: b.path.clone(),
            color: b.color,
            closed: b.closed,
        });
    }
    Ok(out)
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

/// One circular edge for [`chamfer_circles`].
#[derive(Clone, Debug)]
pub struct CircleChamfer {
    /// The circle's centre.
    pub center: Point3,
    /// A point on the circle (picks the edge).
    pub at: Point3,
    /// Equal setback on both faces (mm).
    pub distance: Real,
    /// The name of the new face (`Hole1:CSink:1`).
    pub name: String,
}

/// Equal chamfers of circular edges, each new face named after its circle
/// (90° countersinks of holes: the kernel's chamfer of a bore's rim is ~10×
/// faster than cutting a cone tool). Circles of one distance go in one call.
pub fn chamfer_circles(
    s: &ExactSolid,
    circles: &[CircleChamfer],
    prefix: &str,
) -> Result<ExactSolid, String> {
    let mut cur = s.clone();
    let mut done = vec![false; circles.len()];
    for i in 0..circles.len() {
        if done[i] {
            continue;
        }
        let d = circles[i].distance;
        let group: Vec<usize> = (i..circles.len())
            .filter(|&j| !done[j] && (circles[j].distance - d).abs() <= 1e-9 * d.max(1.0))
            .collect();
        let pts: Vec<sg::Vec3> = group.iter().map(|&j| v3(&circles[j].at)).collect();
        let body = guarded("chamfer", || sk::chamfer(&cur.body, &pts, d))?;
        let mut next = blend_result(&cur, body, prefix, "Bevel");
        let new: Vec<usize> = next
            .face_names
            .iter()
            .enumerate()
            .filter(|(_, n)| {
                n.as_deref().is_some_and(|n| {
                    n.starts_with(&format!("{prefix}:Bevel"))
                        || n.starts_with(&format!("{prefix}:Corner"))
                })
            })
            .map(|(f, _)| f)
            .collect();
        if !new.is_empty() {
            let samples = face_samples(&next)?;
            for f in new {
                let Some(p) = samples.get(f).copied().flatten() else {
                    continue;
                };
                let near = group.iter().copied().min_by(|&a, &b| {
                    let da = (circles[a].center - p).norm();
                    let db = (circles[b].center - p).norm();
                    da.total_cmp(&db)
                });
                if let Some(j) = near {
                    next.face_names[f] = Some(circles[j].name.clone());
                }
            }
        }
        for &j in &group {
            done[j] = true;
        }
        cur = next;
    }
    Ok(cur)
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
        sheet: false,
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
/// Boolean split in pieces keeps its name on each piece; the sectors of a
/// full revolve share one name). Nearest = the distance from `hint` to the
/// face itself (its triangles); on a tie (the hint on the edge between two
/// pieces) the piece whose sample point is nearer.
pub fn faces_named(s: &ExactSolid, name: &str, hint: Point3) -> Result<Vec<usize>, String> {
    Ok(faces_named_many(s, &[(name, hint)])?
        .pop()
        .unwrap_or_default())
}

/// [`faces_named`] for many references at once: the samples and the mesh
/// are made once (only when a name is shared), not once per reference
/// (Delete Face of 36 hole walls took 9 s).
pub fn faces_named_many(
    s: &ExactSolid,
    refs: &[(&str, Point3)],
) -> Result<Vec<Vec<usize>>, String> {
    let named: Vec<Vec<usize>> = refs
        .iter()
        .map(|(name, _)| {
            s.face_names
                .iter()
                .enumerate()
                .filter(|(_, n)| n.as_deref() == Some(*name))
                .map(|(i, _)| i)
                .collect()
        })
        .collect();
    if named.iter().all(|v| v.len() <= 1) {
        return Ok(named);
    }
    let samples = face_samples(s)?;
    let tol = (s.body.size() * 1e-4).max(1e-4);
    let mesh = guarded("tessellate", || s.body.tessellate(tol))?;
    let tie = tol * 10.0;
    // Distances in steps of `tie` (a total order), then the sample's.
    let step = |d: Real| {
        if d.is_finite() {
            (d / tie).round()
        } else {
            Real::MAX
        }
    };
    let mut out = Vec::with_capacity(refs.len());
    for (named, (_, hint)) in named.into_iter().zip(refs) {
        if named.len() <= 1 {
            out.push(named);
            continue;
        }
        let mut near = vec![Real::INFINITY; s.face_names.len().max(s.body.face_count())];
        for (t, &f) in mesh.triangles.iter().zip(&mesh.tri_face) {
            if !named.contains(&(f as usize)) {
                continue;
            }
            let Some([a, b, c]) = mesh.tri(t) else {
                continue;
            };
            if let Some(d) = near.get_mut(f as usize) {
                let q = sheet::closest_on_triangle(hint, &p3(a), &p3(b), &p3(c));
                *d = d.min((q - hint).norm());
            }
        }
        let mut v: Vec<(Real, Real, usize)> = named
            .into_iter()
            .map(|i| {
                let d = near.get(i).copied().unwrap_or(Real::INFINITY);
                let sd = samples
                    .get(i)
                    .copied()
                    .flatten()
                    .map_or(Real::INFINITY, |p| (p - hint).norm());
                (d, sd, i)
            })
            .collect();
        v.sort_by(|a, b| step(a.0).total_cmp(&step(b.0)).then(a.1.total_cmp(&b.1)));
        out.push(v.into_iter().map(|(_, _, i)| i).collect());
    }
    Ok(out)
}

/// The distance from each `(face, point)` to that face of `s` (its display
/// triangles; one mesh for all of them).
pub fn face_distances(s: &ExactSolid, asks: &[(usize, Point3)]) -> Result<Vec<Real>, String> {
    let tol = (s.body.size() * 1e-4).max(1e-4);
    let mesh = guarded("tessellate", || s.body.tessellate(tol))?;
    let mut out = vec![Real::INFINITY; asks.len()];
    for (t, &f) in mesh.triangles.iter().zip(&mesh.tri_face) {
        let Some([a, b, c]) = mesh.tri(t) else {
            continue;
        };
        for (d, (face, p)) in out.iter_mut().zip(asks) {
            if *face == f as usize {
                let q = sheet::closest_on_triangle(p, &p3(a), &p3(b), &p3(c));
                *d = d.min((q - p).norm());
            }
        }
    }
    Ok(out)
}

/// Face references `(name, hint)` resolved among several bodies:
/// `(body index, kernel face)` of each. A name held by more than one body
/// (a Split Solid leaves the names of the faces it cuts on both parts)
/// means the piece nearest the hint over all of them, never the first
/// body's.
pub fn faces_in_solids(
    solids: &[ExactSolid],
    refs: &[(&str, Point3)],
) -> Vec<Option<(usize, usize)>> {
    // Every body's best piece of each reference.
    let mut cands: Vec<Vec<(usize, usize)>> = vec![Vec::new(); refs.len()];
    for (i, s) in solids.iter().enumerate() {
        let asks: Vec<usize> = (0..refs.len())
            .filter(|&k| s.face_names.iter().any(|n| n.as_deref() == Some(refs[k].0)))
            .collect();
        if asks.is_empty() {
            continue;
        }
        let these: Vec<(&str, Point3)> = asks.iter().map(|&k| refs[k]).collect();
        let Ok(found) = faces_named_many(s, &these) else {
            continue;
        };
        for (k, v) in asks.into_iter().zip(found) {
            if let Some(&f) = v.first() {
                cands[k].push((i, f));
            }
        }
    }
    // A name on several bodies: the nearest piece (one mesh per body).
    let mut dist: Vec<Vec<Real>> = cands.iter().map(|c| vec![0.0; c.len()]).collect();
    for (i, s) in solids.iter().enumerate() {
        let mut asks = Vec::new();
        let mut at = Vec::new();
        for (k, c) in cands.iter().enumerate() {
            if c.len() < 2 {
                continue;
            }
            for (j, &(b, f)) in c.iter().enumerate() {
                if b == i {
                    asks.push((f, refs[k].1));
                    at.push((k, j));
                }
            }
        }
        if asks.is_empty() {
            continue;
        }
        let d = face_distances(s, &asks).unwrap_or_else(|_| vec![Real::INFINITY; asks.len()]);
        for ((k, j), d) in at.into_iter().zip(d) {
            dist[k][j] = d;
        }
    }
    cands
        .into_iter()
        .zip(dist)
        .map(|(c, d)| {
            c.into_iter()
                .zip(d)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(x, _)| x)
        })
        .collect()
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
    shell_with(s, open, thickness, side, false, prefix)
}

/// [`shell`] with Inventor's shell type: `rounded` (Rounded Corners) rounds
/// the corners the offset makes, as a true offset would: the outer walls'
/// sharp convex edges by the outward offset, the cavity's sharp concave
/// edges by the inward one (edges on removed faces stay sharp). Rounds are
/// named `<prefix>:Corner:Round:<edge>`.
pub fn shell_with(
    s: &ExactSolid,
    open: &[usize],
    thickness: Real,
    side: ShellSide,
    rounded: bool,
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
    let first = if side == ShellSide::Inside && !open.is_empty() && !rounded {
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
                    sheet: false,
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
    // The removed faces' planes: their edges stay sharp (Rounded Corners).
    let open_planes: Vec<(Point3, Vector3)> = if rounded {
        let tol = (s.body.size() * 1e-3).max(1e-4);
        guarded("faces", || s.body.faces(tol))?
            .into_iter()
            .filter(|f| open.contains(&f.index))
            .filter_map(|f| f.plane_normal.map(|n| (p3(f.centroid), w3(n))))
            .collect()
    } else {
        Vec::new()
    };
    let general = || -> Result<ExactSolid, String> {
        let outer = if out > 0.0 {
            let body = guarded("shell", || sk::offset_faces(&s.body, &walls, out))?;
            let face_names = names_from(&body, &[s], &|_| None);
            let o = ExactSolid {
                sheet: false,
                name: s.name.clone(),
                body,
                face_names,
                profiles: s.profiles.clone(),
            };
            if rounded {
                round_offset_corners(&o, &open_planes, true, out, prefix)?
            } else {
                o
            }
        } else {
            s.clone()
        };
        let lined = ExactSolid {
            sheet: false,
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
            let c = ExactSolid {
                sheet: false,
                face_names,
                body,
                ..lined
            };
            if rounded {
                round_offset_corners(&c, &open_planes, false, inn, prefix)?
            } else {
                c
            }
        } else {
            lined
        };
        if !open_pts.is_empty() {
            let push = 2.0 * thickness + 1.0;
            let body = guarded("shell", || sk::offset_faces(&cavity.body, &open_pts, push))?;
            let face_names = names_from(&body, &[&cavity], &|_| None);
            cavity = ExactSolid {
                sheet: false,
                face_names,
                body,
                ..cavity
            };
        }
        boolean(&outer, &cavity, ExactOp::Cut)?.ok_or_else(|| "the shell removed everything".into())
    };
    general().map_err(|e| first.unwrap_or(e))
}

/// Rounds the sharp edges of `s` that are convex (`convex`) or concave by
/// `r`, except those lying on one of `skip` (point, normal): Rounded
/// Corners of a shell. Nothing to round: `s` as it is.
fn round_offset_corners(
    s: &ExactSolid,
    skip: &[(Point3, Vector3)],
    convex: bool,
    r: Real,
    prefix: &str,
) -> Result<ExactSolid, String> {
    let size = s.body.size().max(1e-9);
    let edges = guarded("edges", || s.body.edges(size * 1e-3))?;
    let eps = (size * 1e-3).max(1e-4);
    let mut mids = Vec::new();
    for e in &edges {
        let m = p3(e.mid);
        if skip
            .iter()
            .any(|(c, n)| (m - c).dot(n).abs() < size * 1e-6 + 1e-7)
        {
            continue;
        }
        // The edge's direction at its middle.
        let k = e
            .points
            .iter()
            .enumerate()
            .min_by(|a, b| (p3(*a.1) - m).norm().total_cmp(&(p3(*b.1) - m).norm()))
            .map_or(0, |(k, _)| k);
        let (Some(a), Some(b)) = (
            e.points.get(k.saturating_sub(1)),
            e.points.get((k + 1).min(e.points.len().saturating_sub(1))),
        ) else {
            continue;
        };
        let Some(d) = (p3(*b) - p3(*a)).try_normalize(1e-12) else {
            continue;
        };
        // Material around the edge: 16 directions square to it.
        let u = d.cross(&if d.x.abs() < 0.9 {
            Vector3::x()
        } else {
            Vector3::y()
        });
        let Some(u) = u.try_normalize(1e-12) else {
            continue;
        };
        let v = d.cross(&u);
        let inside = (0..16)
            .filter(|i| {
                let t = std::f64::consts::TAU * (*i as Real + 0.5) / 16.0;
                contains(s, m + (u * t.cos() + v * t.sin()) * eps)
            })
            .count();
        let take = if convex { inside < 7 } else { inside > 9 };
        if take {
            mids.push(m);
        }
    }
    if mids.is_empty() {
        return Ok(s.clone());
    }
    fillet(s, &mids, r, &format!("{prefix}:Corner"))
        .map_err(|e| format!("the corners cannot be rounded ({e})"))
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
        sheet: false,
        name: s.name.clone(),
        body,
        face_names,
        profiles: s.profiles.clone(),
    })
}

/// The input face each face of `result` came from: `(operand, face)` of its
/// first face origin (the history, or a trace of the operands).
fn face_origins(result: &sk::Body, operands: &[&ExactSolid]) -> Vec<Option<(usize, usize)>> {
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
            out[i] = fp.origins.iter().find_map(|o| match o {
                FaceOrigin::Face { operand, face } => Some((*operand, *face)),
                _ => None,
            });
        }
    }
    out
}

/// Two-sided draft about a plane square to `pull` (Face Draft Symmetric /
/// Asymmetric, Parting Line; `docs/spec/2.8-draft.md`): the solid is split
/// by `neutral`, the pieces of `faces` above it are drafted by `above`
/// (its own neutral plane and angle) leaning in along `pull`, those below
/// by `below` leaning in along `−pull`, and the halves are joined again.
/// `None` leaves that side as it is. Every piece keeps its face's name; the
/// section is not in the result.
pub fn draft_two_sided(
    s: &ExactSolid,
    faces: &[usize],
    neutral: &Plane,
    pull: Vector3,
    above: Option<(&Plane, Real)>,
    below: Option<(&Plane, Real)>,
) -> Result<ExactSolid, String> {
    let pull = pull
        .try_normalize(1e-12)
        .ok_or_else(|| "the pull direction has no length".to_owned())?;
    if neutral.normal.dot(&pull).abs() < 1.0 - 1e-9 {
        return Err(
            "the parting plane is not square to the pull direction (not supported yet)".into(),
        );
    }
    if faces.is_empty() {
        return Err("no faces to draft".into());
    }
    let plane = Plane::from_normal(neutral.origin, pull).map_err(|e| e.to_string())?;
    let h0 = plane.origin.coords.dot(&pull);
    let kplane = plane_of(&plane)?;
    let parts = guarded("split", || sk::split_by_plane(&s.body, &kplane))?;
    // Each part with its side (by the height of its face samples).
    let mut up: Option<ExactSolid> = None;
    let mut down: Option<ExactSolid> = None;
    for body in parts {
        let face_names = names_from(&body, &[s], &|_| None);
        let origin = face_origins(&body, &[s]);
        let part = ExactSolid {
            sheet: false,
            name: s.name.clone(),
            body,
            face_names,
            profiles: s.profiles.clone(),
        };
        let samples = face_samples(&part)?;
        let (sum, n) = samples.iter().flatten().fold((0.0, 0usize), |(a, n), p| {
            (a + p.coords.dot(&pull) - h0, n + 1)
        });
        if n == 0 {
            continue;
        }
        let chosen: Vec<usize> = origin
            .iter()
            .enumerate()
            .filter(|(_, o)| matches!(o, Some((0, f)) if faces.contains(f)))
            .map(|(i, _)| i)
            .collect();
        let (slot, angle, dir) = if sum > 0.0 {
            (&mut up, above, pull)
        } else {
            (&mut down, below, -pull)
        };
        let drafted = match angle {
            Some((pl, a)) if !chosen.is_empty() => {
                if pl.normal.dot(&pull).abs() < 1.0 - 1e-9 {
                    return Err(
                        "the pivot plane is not square to the pull direction (not supported yet)"
                            .into(),
                    );
                }
                draft(&part, &chosen, pl, dir, a)?
            }
            _ => part,
        };
        if slot.is_some() {
            return Err(
                "the plane splits the solid in more than two pieces (not supported yet)".into(),
            );
        }
        *slot = Some(drafted);
    }
    let joined = match (up, down) {
        (Some(a), Some(b)) => {
            let r = guarded("boolean", || {
                sk::boolean(&a.body, &b.body, sk::BoolOp::Union)
            })?
            .ok_or_else(|| "the drafted halves do not join".to_owned())?;
            let face_names = names_from(&r, &[&a, &b], &|_| None);
            ExactSolid {
                sheet: false,
                name: s.name.clone(),
                body: r,
                face_names,
                profiles: s.profiles.clone(),
            }
        }
        (Some(a), None) | (None, Some(a)) => a,
        (None, None) => return Err("the plane leaves nothing of the solid".into()),
    };
    Ok(joined)
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
        sheet: false,
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

/// A cone (a countersink) as SolidCraft builds it: a disc of radius `r0` at
/// `z0` along `axis` from `origin`, extruded to `z1` with a taper that
/// ends at radius `r1`. A tool of revolution with the same cone took about
/// 1 s per hole in the Boolean (the kernel meets a revolved line and a
/// plane on its general path); this one a few ms. Faces:
/// `<prefix>:<names[0]>` at `z0`, `names[1]` the cone, `names[2]` at `z1`.
#[allow(clippy::too_many_arguments)]
pub fn cone_tool(
    origin: Point3,
    axis: Vector3,
    seam: Real,
    (r0, z0): (Real, Real),
    (r1, z1): (Real, Real),
    names: [&str; 3],
    prefix: &str,
) -> Result<ExactSolid, String> {
    let len = (z1 - z0).abs();
    if !(r0 > 0.0 && r1 > 0.0 && len > 1e-9) {
        return Err("the countersink is not valid".into());
    }
    let k = axis
        .try_normalize(1e-12)
        .ok_or_else(|| "the hole axis has no direction".to_owned())?;
    // The disc's plane: square to the axis at z0 (its normal is the axis);
    // `seam` turns its x like a tool of revolution's profile plane.
    let any = if k.x.abs() < 0.9 {
        Vector3::x()
    } else {
        Vector3::y()
    };
    let u0 = (any - k * any.dot(&k)).normalize();
    let u = u0 * seam.cos() + k.cross(&u0) * seam.sin();
    let w = k.cross(&u);
    let base = sg::Plane::new(v3(&(origin + k * z0)), d3(&u), d3(&w))
        .ok_or("degenerate countersink plane")?;
    let region = sg::Region2 {
        outer: sg::Loop2::circle(sg::Vec2::new(0.0, 0.0), r0),
        holes: Vec::new(),
    };
    let taper = ((r1 - r0) / len).atan();
    let sign = (z1 - z0).signum();
    let body = guarded("countersink", || {
        sk::extrude_tapered(&base, &region, len, sign, taper)
    })?;
    let face_names = names_from(&body, &[], &|o| match o {
        FaceOrigin::Profile { .. } => Some(format!("{prefix}:{}", names[1])),
        FaceOrigin::Cap { end } => Some(format!(
            "{prefix}:{}",
            if *end { names[2] } else { names[0] }
        )),
        _ => None,
    });
    Ok(ExactSolid {
        sheet: false,
        name: None,
        body,
        face_names,
        profiles: Vec::new(),
    })
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
        sheet: false,
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
        sheet: false,
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
    /// Where (z) the body has faces square to the axis in the zone (the
    /// start face, the far face, a seat's floor): the slabs of
    /// [`cut_threads`] keep their planes away from them (a plane 0.04 mm
    /// under the start face left a sliver whose faceted cut failed).
    pub avoid: Vec<Real>,
}

impl ThreadGroove {
    /// The axis-aligned box around the zone (a cylinder of its outer radius
    /// between its two heights). Use it to find the solid or piece a groove
    /// belongs to: the zone's middle can lie in the air (a thin plate under
    /// a large start margin), which once skipped grooves without a word.
    pub fn zone_box(&self) -> (Point3, Point3) {
        let k = self.axis.try_normalize(1e-12).unwrap_or_else(Vector3::z);
        let r = self.zone[1];
        let (a, b) = (
            self.origin + k * self.zone[2],
            self.origin + k * self.zone[3],
        );
        // A disc of radius r square to k reaches r·sqrt(1 − k_i²) along i.
        let ext = Vector3::new(
            r * (1.0 - k.x * k.x).max(0.0).sqrt(),
            r * (1.0 - k.y * k.y).max(0.0).sqrt(),
            r * (1.0 - k.z * k.z).max(0.0).sqrt(),
        );
        (a.inf(&b) - ext, a.sup(&b) + ext)
    }
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
    let mut meshes: Vec<((Point3, Point3), sg::Mesh, String)> = Vec::new();
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
        let zbox = g.zone_box();
        meshes.push((zbox, m, format!("{}:{}", g.prefix, g.name)));
    }
    // Zones apart from each other (the usual case): each one in slabs.
    let apart = if zones.len() == 1 {
        Some(zones[0].clone())
    } else {
        union_apart(&zones)?
    };
    if let Some(whole) = apart {
        return slab_threads(s, grooves, &whole);
    }
    // Zones that overlap (threads closer than their zones): one piece of
    // material for all of them.
    let mut it = zones.into_iter();
    let mut zone = it.next().ok_or("no thread zone")?;
    for t in it {
        zone = boolean(&zone, &t, ExactOp::Union)?.ok_or("thread zones")?;
    }
    let inside = boolean(&zone, s, ExactOp::Intersect)?
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
        for ((zlo, zhi), g, name) in &meshes {
            // The zone overlaps this piece (its middle may lie in the air).
            if !(0..3).all(|i| zlo[i] <= hi[i] + pad && zhi[i] >= lo[i] - pad) {
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
            .map(|(piece, mesh, names)| {
                sc.spawn(move || threaded_piece(s, piece, mesh, names, None))
            })
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

/// Turns of a groove per slab of its zone in [`slab_threads`].
const SLAB_TURNS: Real = 2.0;

/// Rounds of [`slab_threads`]: the first cut, then repairs of failed slabs.
const SLAB_ROUNDS: usize = 6;

/// A groove's helix as stations `THREAD_STATIONS` per turn from its start
/// (the last one at its end), so a run of stations is the same triangles
/// as in the whole groove: neighbouring slabs cut the groove along their
/// common plane identically.
struct GrooveHelix {
    /// The section's corners at station 0 (world).
    corners: Vec<Point3>,
    /// A point on the axis and the unit axis the section turns about.
    origin: Point3,
    turn: Vector3,
    /// Rise per radian along `turn` (negative for a left-handed thread).
    rise: Real,
    /// Total angle and the number of the last station.
    total: Real,
    last: usize,
    /// Axial extent of the section about its station (along the groove axis).
    lo: Real,
    hi: Real,
    /// Where the section's middle starts along the groove axis.
    base: Real,
    pitch: Real,
}

impl GrooveHelix {
    fn new(g: &ThreadGroove) -> Result<Self, String> {
        let k3 = g
            .axis
            .try_normalize(1e-12)
            .ok_or_else(|| "the thread axis has no direction".to_owned())?;
        let base = g.origin + k3 * g.base;
        let pl = axial_frame(base, k3, 0.0)?;
        let corners = g
            .section
            .iter()
            .map(|q| {
                let w = pl.to_world(sg::Vec2::new(q[0], q[1]));
                Point3::new(w.x, w.y, w.z)
            })
            .collect();
        let (turn, rise) = if g.left_hand {
            (-k3, -g.pitch / std::f64::consts::TAU)
        } else {
            (k3, g.pitch / std::f64::consts::TAU)
        };
        let total = std::f64::consts::TAU * g.turns;
        // The last station is at the end (never a sliver of a station after
        // the one before it).
        let last = (g.turns * THREAD_STATIONS as Real - 1e-3).ceil().max(1.0) as usize;
        let lo = g
            .section
            .iter()
            .map(|q| q[1])
            .fold(Real::INFINITY, Real::min);
        let hi = g
            .section
            .iter()
            .map(|q| q[1])
            .fold(Real::NEG_INFINITY, Real::max);
        Ok(Self {
            corners,
            origin: base,
            turn,
            rise,
            total,
            last,
            lo,
            hi,
            base: g.base,
            pitch: g.pitch,
        })
    }

    fn angle(&self, i: usize) -> Real {
        if i >= self.last {
            self.total
        } else {
            (std::f64::consts::TAU * i as Real / THREAD_STATIONS as Real).min(self.total)
        }
    }

    /// The section's axial extent at station `i` (from the groove origin).
    fn extent(&self, i: usize) -> (Real, Real) {
        let z = self.base + self.pitch * self.angle(i) / std::f64::consts::TAU;
        (z + self.lo, z + self.hi)
    }

    /// The closed mesh of stations `i0..=i1`, wound outward.
    fn mesh(&self, i0: usize, i1: usize) -> sg::Mesh {
        let m = self.corners.len();
        let mut mesh = sg::Mesh::default();
        let axis = nalgebra::Unit::new_unchecked(self.turn);
        for i in i0..=i1 {
            let a = self.angle(i);
            let rot = nalgebra::Rotation3::from_axis_angle(&axis, a);
            let lift = self.turn * (self.rise * a);
            for c in &self.corners {
                let p = self.origin + rot * (c - self.origin) + lift;
                mesh.positions.push(v3(&p));
            }
        }
        let n = i1 - i0;
        let id = |i: usize, j: usize| u32::try_from(i * m + j % m).unwrap_or(0);
        for i in 0..n {
            for j in 0..m {
                mesh.triangles
                    .push([id(i, j), id(i + 1, j), id(i + 1, j + 1)]);
                mesh.triangles
                    .push([id(i, j), id(i + 1, j + 1), id(i, j + 1)]);
            }
        }
        for j in 1..m - 1 {
            mesh.triangles.push([id(0, 0), id(0, j), id(0, j + 1)]);
            mesh.triangles.push([id(n, 0), id(n, j + 1), id(n, j)]);
        }
        if mesh.measure().volume < 0.0 {
            for t in &mut mesh.triangles {
                t.swap(1, 2);
            }
        }
        mesh.tri_face = vec![0; mesh.triangles.len()];
        mesh
    }

    /// The stations whose groove reaches into `[a, b]` along the axis, with
    /// one more at each end (its end caps lie outside the band).
    fn stations(&self, a: Real, b: Real) -> Option<(usize, usize)> {
        let i0 = (0..=self.last)
            .rev()
            .find(|&i| self.extent(i).1 < a)
            .unwrap_or(0);
        let i1 = (0..=self.last)
            .find(|&i| self.extent(i).0 > b)
            .unwrap_or(self.last);
        (i1 > i0 && self.extent(i0).0 <= b && self.extent(i1).1 >= a).then_some((i0, i1))
    }
}

/// [`cut_threads`] for zones apart from each other: every zone in slabs
/// across its axis ([`SLAB_TURNS`] turns high, after the Coil's slabs),
/// each slab's material cut with only the part of its groove that reaches
/// into it. The faceted Boolean grows faster than the groove's length
/// (owner's M8 × 10 in a 283 × 136 × 10 plate: 13.6 s in one zone, ~1.5 s
/// in slabs), so short pieces on parallel workers are much faster; the rest
/// of the body (outside the zones) is made meanwhile.
///
/// A slab's faceted Boolean sometimes fails ("shell is not oriented and
/// closed", a negative intersection volume, a result our B-rep refuses),
/// depending on where its planes lie (a plane near a row of the body's
/// facets makes slivers). A failed slab is cut again with its planes moved,
/// then merged with a neighbour (see [`SLAB_ROUNDS`]); at worst the zone is
/// one piece, as it was cut before the slabs.
fn slab_threads(
    s: &ExactSolid,
    grooves: &[ThreadGroove],
    whole: &ExactSolid,
) -> Result<Vec<ExactSolid>, String> {
    type Slab = Result<(bool, Vec<ExactSolid>), String>;
    /// One groove's slabs: the planes between them (along the axis) and the
    /// latest result of each.
    struct Plan<'a> {
        k: usize,
        g: &'a ThreadGroove,
        helix: GrooveHelix,
        planes: Vec<Real>,
        moved: Vec<bool>,
        tol: Real,
        name: String,
        done: Vec<Option<Slab>>,
        /// The body near the zone, once a slab cut out of `s` came out
        /// imprecise (see [`near_zone`]).
        local: Option<ExactSolid>,
    }
    let mut plans = Vec::new();
    for (k, g) in grooves.iter().enumerate() {
        let [r1, z0, z1] = [g.zone[1], g.zone[2], g.zone[3]];
        let n = ((z1 - z0) / (SLAB_TURNS * g.pitch))
            .ceil()
            .clamp(1.0, 400.0) as usize;
        // One facet tolerance for every slab of a zone (the zone's size, as
        // the whole zone was faceted), so the slabs facet its walls alike.
        let tol = ((4.0 * r1 * r1 + (z1 - z0) * (z1 - z0)).sqrt() * 5e-4).clamp(2e-3, 1e-2);
        // Inner planes off the even division (a symmetric zone put one in
        // the middle of the plate, at a row of the bore's facets, and both
        // slabs beside it failed) and away from the body's square faces.
        let h = (z1 - z0) / n as Real;
        let mut planes: Vec<Real> = (0..=n)
            .map(|j| {
                let z = z0 + h * j as Real;
                if j == 0 || j == n { z } else { z + 0.137 * h }
            })
            .collect();
        for z in planes.iter_mut().take(n).skip(1) {
            *z = clear_of(*z, &g.avoid, 0.3 * h);
        }
        for j in 1..n {
            // Still in order, and no slab thinner than a tenth.
            let (lo, hi) = (planes[j - 1] + 0.1 * h, planes[j + 1] - 0.1 * h);
            planes[j] = planes[j].clamp(lo, hi.max(lo));
        }
        plans.push(Plan {
            k,
            g,
            helix: GrooveHelix::new(g)?,
            planes,
            moved: vec![false; n + 1],
            tol,
            name: format!("{}:{}", g.prefix, g.name),
            done: (0..n).map(|_| None).collect(),
            local: None,
        });
    }
    // A slab's material (an exact Boolean with the body), then the faceted
    // cut of every piece of it, which our B-rep must take as well (one slab
    // in 54 of nine M6 holes had a vertex off its edge).
    let slab = |p: &Plan, j: usize| -> Slab {
        let (g, a, b) = (p.g, p.planes[j], p.planes[j + 1]);
        let Some((i0, i1)) = p.helix.stations(a, b) else {
            return Ok((false, Vec::new()));
        };
        let ring = revolved_tool(
            g.origin,
            g.axis,
            0.4 + 0.613 * p.k as Real,
            &[
                [g.zone[0], a],
                [g.zone[1], a],
                [g.zone[1], b],
                [g.zone[0], b],
            ],
            &["ThreadZone"; 4],
            &g.prefix,
        )?;
        let Some(inside) = boolean(p.local.as_ref().unwrap_or(s), &ring, ExactOp::Intersect)?
        else {
            return Ok((false, Vec::new()));
        };
        if p.local.is_none() && !square_to(&inside, &g.axis) {
            return Err(IMPRECISE.into());
        }
        let mesh = p.helix.mesh(i0, i1);
        let names = vec![p.name.clone(); mesh.triangles.len()];
        let mut out = Vec::new();
        for piece in lumps(&inside) {
            let Some(cut) = threaded_piece(s, &piece, &mesh, &names, Some(p.tol))? else {
                continue;
            };
            let shown = to_brep(std::slice::from_ref(&cut))?;
            for (sid, _) in shown.solids() {
                shown
                    .validate_solid(sid)
                    .map_err(|e| format!("a threaded slab is not a valid solid ({e:?})"))?;
            }
            out.push(cut);
        }
        Ok((true, out))
    };
    let lost = "thread: internal error in the geometry kernel";
    let workers = std::thread::available_parallelism().map_or(1, |n| n.get());
    let mut rest = None;
    for round in 0..SLAB_ROUNDS {
        // Every slab without a good result, on parallel workers (the rest
        // of the body beside them, the first time).
        let todo: Vec<(usize, usize)> = plans
            .iter()
            .enumerate()
            .flat_map(|(pi, p)| {
                (0..p.done.len())
                    .filter(|&j| !matches!(p.done[j], Some(Ok(_))))
                    .map(move |j| (pi, j))
            })
            .collect();
        if todo.is_empty() {
            break;
        }
        let next = std::sync::atomic::AtomicUsize::new(0);
        let slots: Vec<std::sync::Mutex<Option<Slab>>> =
            todo.iter().map(|_| std::sync::Mutex::new(None)).collect();
        let plans_ref = &plans;
        let made = std::thread::scope(|sc| {
            let r = (round == 0).then(|| sc.spawn(|| boolean(s, whole, ExactOp::Cut)));
            for _ in 0..workers.clamp(1, todo.len()) {
                sc.spawn(|| {
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(&(pi, j)) = todo.get(i) else { break };
                        let out = slab(&plans_ref[pi], j);
                        if let Some(slot) = slots.get(i)
                            && let Ok(mut slot) = slot.lock()
                        {
                            *slot = Some(out);
                        }
                    }
                });
            }
            r.map(|h| h.join().unwrap_or_else(|_| Err(lost.to_owned())))
        });
        if let Some(r) = made {
            rest = Some(r?);
        }
        for (&(pi, j), slot) in todo.iter().zip(slots) {
            let r = slot
                .into_inner()
                .map_err(|_| lost.to_owned())?
                .unwrap_or_else(|| Err(lost.to_owned()));
            if let Err(e) = &r {
                log::debug!(
                    "thread {}: slab {j} of {} failed (round {round}): {e}",
                    plans[pi].name,
                    plans[pi].done.len()
                );
            }
            plans[pi].done[j] = Some(r);
        }
        if round + 1 == SLAB_ROUNDS {
            break;
        }
        // Imprecise slabs: their zone is cut out of the body near it from
        // now on (as they were, without moving planes).
        let want: Vec<usize> = (0..plans.len())
            .filter(|&pi| {
                plans[pi].local.is_none()
                    && plans[pi]
                        .done
                        .iter()
                        .any(|d| matches!(d, Some(Err(e)) if e == IMPRECISE))
            })
            .collect();
        let near: Vec<Result<Option<ExactSolid>, String>> = std::thread::scope(|sc| {
            let jobs: Vec<_> = want
                .iter()
                .map(|&pi| {
                    let g = plans[pi].g;
                    sc.spawn(move || near_zone(s, g))
                })
                .collect();
            jobs.into_iter()
                .map(|h| h.join().unwrap_or_else(|_| Err(lost.to_owned())))
                .collect()
        });
        for (&pi, l) in want.iter().zip(near) {
            let p = &mut plans[pi];
            let Some(l) = l? else {
                return Err("the thread is not in the material".into());
            };
            p.local = Some(l);
            for d in &mut p.done {
                if matches!(d, Some(Err(e)) if e == IMPRECISE) {
                    *d = None;
                }
            }
        }
        // The inner planes beside a failed slab move by about half a slab
        // (once each: small steps did not get off a row of facets); a
        // failed slab whose planes have moved already is merged with its
        // upper neighbour (the lower one for the last slab), one merge per
        // zone and round. The slabs beside a changed plane are redone.
        for p in &mut plans {
            let n = p.done.len();
            if n == 1 {
                continue;
            }
            let failed: Vec<usize> = (0..n)
                .filter(|&j| matches!(p.done[j], Some(Err(_))))
                .collect();
            let mut merge = None;
            let mut moves = Vec::new();
            for &j in &failed {
                let fresh: Vec<usize> = [j, j + 1]
                    .into_iter()
                    .filter(|&q| q > 0 && q < n && !p.moved[q])
                    .collect();
                if fresh.is_empty() {
                    merge.get_or_insert(j);
                }
                moves.extend(fresh);
            }
            moves.sort_unstable();
            moves.dedup();
            for q in moves {
                p.moved[q] = true;
                let (lo, hi) = (p.planes[q - 1], p.planes[q + 1]);
                let z = clear_of(p.planes[q] + 0.23 * (hi - lo), &p.g.avoid, 0.15 * (hi - lo));
                if z > lo + 0.05 * (hi - lo) && z < hi - 0.05 * (hi - lo) {
                    p.planes[q] = z;
                }
                p.done[q - 1] = None;
                p.done[q] = None;
            }
            if let Some(j) = merge {
                let q = if j + 1 < n { j + 1 } else { j };
                p.planes.remove(q);
                p.moved.remove(q);
                p.done.remove(q);
                p.done[q - 1] = None;
            }
        }
    }
    let mut out: Vec<ExactSolid> = rest.ok_or(lost)?.into_iter().collect();
    let mut any = false;
    for p in plans {
        for r in p.done {
            let (hit, pieces) = r.unwrap_or_else(|| Err(lost.to_owned()))?;
            any |= hit;
            out.extend(pieces);
        }
    }
    if !any {
        return Err("the thread is not in the material".into());
    }
    Ok(out)
}

/// `z` moved `gap` away from the nearest of `avoid` it is closer than
/// `gap` to (upwards when on it).
fn clear_of(z: Real, avoid: &[Real], gap: Real) -> Real {
    let mut z = z;
    for &a in avoid {
        if (z - a).abs() < gap {
            z = if z < a { a - gap } else { a + gap };
        }
    }
    z
}

/// A slab out of a large body that is not as exact as the ring it was cut
/// with (see [`square_to`]); it is cut again out of [`near_zone`].
const IMPRECISE: &str = "the slab of the thread zone is not precise";

/// Whether the faces of `piece` that should be square to `axis` (the
/// slab's planes, within 1e-4 rad) are square to it to 1e-9. Out of a
/// 300 mm plate an M3 slab came out with its planes tilted 1.7e-6 rad and
/// its circles 1 µm off (the kernel's tolerances follow the operands'
/// size), and its faceted cut failed every time; out of a 150 mm plate it
/// was exact.
fn square_to(piece: &ExactSolid, axis: &Vector3) -> bool {
    let Some(k) = axis.try_normalize(1e-12) else {
        return true;
    };
    let Ok(faces) = guarded("faces", || piece.body.faces(1e-2)) else {
        return true;
    };
    faces.iter().filter_map(|f| f.plane_normal).all(|n| {
        let off = Vector3::new(n.x, n.y, n.z).cross(&k).norm();
        !(1e-9..1e-4).contains(&off)
    })
}

/// `s` in a box around the zone of `g`: the box's planes meet only the
/// body's own planar faces near the zone (the bore's cylinder lies inside
/// it), so slabs cut out of it are as exact as out of a small part. Slower
/// to cut than `s` itself, so only for slabs that came out imprecise.
fn near_zone(s: &ExactSolid, g: &ThreadGroove) -> Result<Option<ExactSolid>, String> {
    let (lo, hi) = g.zone_box();
    let pad = Vector3::repeat(0.37 * g.pitch + 0.05);
    let b = box_named(lo - pad, hi + pad, &g.prefix)?;
    boolean(s, &b, ExactOp::Intersect)
}

/// One zone's material less its grooves (faceted), named.
fn threaded_piece(
    s: &ExactSolid,
    piece: &ExactSolid,
    mesh: &sg::Mesh,
    groove_names: &[String],
    tol: Option<Real>,
) -> Result<Option<ExactSolid>, String> {
    if mesh.triangles.is_empty() {
        return Ok(Some(piece.clone()));
    }
    // A slab (`tol` given) is not retried finer here: a failure is about
    // where its planes cut the groove, and `slab_threads` moves them.
    let (tol, factors): (Real, &[Real]) = match tol {
        Some(t) => (t, &[1.0]),
        None => ((piece.body.size() * 5e-4).clamp(2e-3, 1e-2), &[1.0, 0.5]),
    };
    let mut last = String::new();
    let mut made = None;
    for &f in factors {
        match guarded("thread", || {
            sk::faceted_boolean(&piece.body, mesh, sk::BoolOp::Cut, tol * f)
        }) {
            Ok(r) => {
                made = Some(r);
                break;
            }
            Err(e) => last = e,
        }
    }
    let Some(made) = made else {
        return Err(last);
    };
    let Some(body) = made else {
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
        sheet: false,
        name: s.name.clone(),
        body,
        face_names,
        profiles: Vec::new(),
    }))
}

/// `s` moved by the rigid motion `m` (a pattern copy of a tool, Move
/// Bodies); face names are kept, a surface body stays one.
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
        sheet: s.sheet,
        name: s.name.clone(),
        body,
        face_names,
        profiles: Vec::new(),
    })
}

// ---------------------------------------------------------------- split ---

/// What divides faces (Split Face, `docs/spec/2.13-combine-split.md`).
#[derive(Clone, Debug)]
pub enum SplitBy {
    /// An unbounded plane.
    Plane(Plane),
    /// Sketch curves (chains of our segments in `plane`) swept along the
    /// plane's normal, each extended past its ends.
    Curves {
        /// The sketch plane.
        plane: Plane,
        /// The chains, in sketch coordinates.
        curves: Vec<Vec<Segment2>>,
    },
    /// A surface body of one face: the face's surface, extended.
    Sheet(ExactSolid),
}

/// Split Face (SolidCraft's `split_faces_with_map`): divides the faces
/// `faces` (kernel indices) of `s` where the tool meets them; the solid is
/// unchanged. Every piece of a divided face is named
/// `<name>~<prefix>.<k>`, `k` counting the pieces from the back of `side`
/// (signed distance of a point on the piece; without `side`, in face order).
/// A listed face the tool misses is an error.
pub fn split_faces(
    s: &ExactSolid,
    faces: &[usize],
    tool: &SplitBy,
    prefix: &str,
    side: Option<&dyn Fn(&Point3) -> Real>,
) -> Result<ExactSolid, String> {
    let pieces_of = |segs: &[Segment2]| -> Result<Vec<sg::Seg2>, String> {
        let mut v = Vec::new();
        for g in segs {
            v.extend(seg2(g)?);
        }
        Ok(v)
    };
    let (body, origin) = match tool {
        SplitBy::Plane(p) => {
            let pl = plane_of(p)?;
            guarded("split face", || {
                sk::split_faces_with_map(&s.body, faces, &sk::SplitTool::Plane(pl))
            })?
        }
        SplitBy::Curves { plane, curves } => {
            let pl = plane_of(plane)?;
            let curves = curves
                .iter()
                .map(|c| pieces_of(c))
                .collect::<Result<Vec<_>, _>>()?;
            guarded("split face", || {
                sk::split_faces_with_map(
                    &s.body,
                    faces,
                    &sk::SplitTool::Curves { plane: pl, curves },
                )
            })?
        }
        SplitBy::Sheet(sheet) => {
            // One surface (a full revolve's sectors share it): its first face.
            let [face] = sheet::distinct_faces(sheet)?[..] else {
                return Err(
                    "not supported yet: a surface of several faces as the tool of Split Face"
                        .into(),
                );
            };
            sheet::split_faces_by(&s.body, faces, sheet, face)?
        }
    };
    let mut out = ExactSolid {
        sheet: false,
        name: s.name.clone(),
        body,
        face_names: Vec::new(),
        profiles: s.profiles.clone(),
    };
    let n = out.body.face_count();
    let samples = face_samples(&out).unwrap_or_default();
    // Pieces of each source face, ordered along the side plane.
    let mut by_source: HashMap<usize, Vec<(Real, usize)>> = HashMap::new();
    for (i, &src) in origin.iter().enumerate().take(n) {
        let h = match (side, samples.get(i).copied().flatten()) {
            (Some(pl), Some(p)) => pl(&p),
            _ => i as Real,
        };
        by_source.entry(src).or_default().push((h, i));
    }
    let mut names: Vec<Option<String>> = vec![None; n];
    for (src, mut v) in by_source {
        let orig = s.face_names.get(src).cloned().flatten();
        v.sort_by(|a, b| a.0.total_cmp(&b.0));
        let several = v.len() > 1;
        for (k, (_, i)) in v.into_iter().enumerate() {
            names[i] = match (&orig, several) {
                (Some(o), true) => Some(format!("{o}~{prefix}.{}", k + 1)),
                (Some(o), false) => Some(o.clone()),
                (None, true) => Some(format!("{prefix}:Face.{}", k + 1)),
                (None, false) => None,
            };
        }
    }
    out.face_names = names;
    Ok(out)
}

/// Split Solid / Trim Solid by a plane (SolidCraft's `split_by_plane`):
/// the parts behind (`.0`) and in front of (`.1`) the plane, along its
/// normal; `None` where nothing is. The section faces are named
/// `<back>:Cut` and `<front>:Cut`; every other face keeps its name (the
/// pieces of one face in each part). Both parts keep the body's name.
pub fn split_by_plane(
    s: &ExactSolid,
    plane: &Plane,
    back: &str,
    front: &str,
) -> Result<(Option<ExactSolid>, Option<ExactSolid>), String> {
    let pl = plane_of(plane)?;
    // SolidCraft's `split_body` imprints the section (no Boolean: ~10× faster
    // through rounds); bodies with voids or a plane along a face go through
    // the half-space Booleans of `split_by_plane`.
    let parts = match guarded("split", || {
        sk::split_body(&s.body, &sk::SplitTool::Plane(pl))
    }) {
        Ok(v) if v.len() > 1 => v,
        _ => guarded("split", || sk::split_by_plane(&s.body, &pl))?,
    };
    let (mut b, mut f) = (None, None);
    for body in parts {
        // The side: the edge midpoint farthest from the plane (a part lies
        // on one side; measuring its volume took 0.7 s through rounds).
        let tol = (body.size() * 1e-2).max(1e-3);
        let edges = guarded("edges", || body.edges(tol))?;
        let ahead = edges
            .iter()
            .map(|e| plane.signed_distance(&p3(e.mid)))
            .max_by(|x, y| x.abs().total_cmp(&y.abs()))
            .is_some_and(|d| d > 0.0);
        let cut = if ahead { front } else { back };
        let face_names = names_from(&body, &[s], &|_| None)
            .into_iter()
            .map(|n| n.or_else(|| Some(format!("{cut}:Cut"))))
            .collect();
        let part = ExactSolid {
            sheet: false,
            name: s.name.clone(),
            body,
            face_names,
            profiles: s.profiles.clone(),
        };
        let slot = if ahead { &mut f } else { &mut b };
        *slot = Some(match slot.take() {
            None => part,
            // Several pieces on one side (a U cut across its legs): one
            // body of several lumps.
            Some(prev) => {
                let both = [prev, part];
                match union_apart(&both)? {
                    Some(u) => u,
                    None => boolean(&both[0], &both[1], ExactOp::Union)?
                        .ok_or_else(|| "split: a part came out empty".to_owned())?,
                }
            }
        });
    }
    Ok((b, f))
}

/// The display-triangle corners of every face (kernel face order), at a
/// coarse tolerance: enough to tell which side of a tool a face lies on.
pub fn face_corners(s: &ExactSolid) -> Result<Vec<Vec<Point3>>, String> {
    let tol = (s.body.size() * 2e-3).max(1e-3);
    let mesh = guarded("tessellate", || s.body.tessellate(tol))?;
    let n = s.face_names.len().max(s.body.face_count());
    let mut out: Vec<Vec<Point3>> = vec![Vec::new(); n];
    for (t, &f) in mesh.triangles.iter().zip(&mesh.tri_face) {
        if let (Some([a, b, c]), Some(v)) = (mesh.tri(t), out.get_mut(f as usize)) {
            v.extend([p3(a), p3(b), p3(c)]);
        }
    }
    Ok(out)
}

/// Like [`split_faces`], but faces the tool misses are left out (All
/// Faces): the kernel names the first one it misses, which is dropped and
/// the split tried again. `Ok(None)`: the tool misses every face.
pub fn split_faces_lenient(
    s: &ExactSolid,
    faces: &[usize],
    tool: &SplitBy,
    prefix: &str,
    side: Option<&dyn Fn(&Point3) -> Real>,
) -> Result<Option<ExactSolid>, String> {
    let mut left = faces.to_vec();
    while !left.is_empty() {
        match split_faces(s, &left, tool, prefix, side) {
            Ok(r) => return Ok(Some(r)),
            Err(e) => {
                let missed = e.split("does not cross face ").nth(1).and_then(|t| {
                    t.chars()
                        .take_while(char::is_ascii_digit)
                        .collect::<String>()
                        .parse::<usize>()
                        .ok()
                });
                match missed {
                    Some(f) if left.contains(&f) => left.retain(|&x| x != f),
                    _ if e.contains("does not cross the selected faces") => return Ok(None),
                    _ => return Err(e),
                }
            }
        }
    }
    Ok(None)
}

impl ExactSolid {
    /// Renames the faces whose names start with `from` to start with `to`
    /// (a section face of the second part of a split).
    pub fn rename_prefix(&mut self, from: &str, to: &str) {
        for n in self.face_names.iter_mut().flatten() {
            if let Some(rest) = n.strip_prefix(from) {
                *n = format!("{to}{rest}");
            }
        }
    }
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
    to_brep_with(solids, false)
}

/// [`to_brep`]; `lenient`: an edge whose fitted line or circle misses an
/// end takes the polyline through its samples instead of failing (a
/// twisted sweep cut leaves nearly circular intersection curves). Only for
/// Sweep / Loft steps: elsewhere the strict failure sends the part to the
/// faceted kernel, which some blends still need.
pub fn to_brep_with(solids: &[ExactSolid], lenient: bool) -> Result<BRep, String> {
    let mut out = BRep::new();
    for s in solids {
        add_solid(&mut out, s, lenient)?;
    }
    Ok(out)
}

/// The exact kernel's surface of face `i` in our form; `kind` is the
/// face's analytic surface (from [`sk::face_surfaces`], all faces at once).
fn surface_of(body: &sk::Body, i: usize, kind: Option<sk::FaceSurface>) -> Option<Surface> {
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
    Some(match kind? {
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
            // Our cone is given by a reference circle (radius > 0): take
            // the one a unit along the axis from the apex, on the side
            // where the cone opens.
            let a = w3(axis).try_normalize(1e-12)?;
            let side = if half_angle >= 0.0 { 1.0 } else { -1.0 };
            let r = half_angle.tan().abs();
            Surface::Cone(Cone::new(p3(apex) + a * side, a, any_x(&a), r, half_angle).ok()?)
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
    polyline_curve(pts)
}

/// The degree-1 B-spline through `pts` (passes through every sample).
fn polyline_curve(pts: &[Point3]) -> Curve {
    let n = pts.len();
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
fn sense_by_geometry(
    m: &TriangleMesh,
    pts: &[Point3],
    inside: &dyn Fn(Point3) -> bool,
    eps: Real,
) -> Option<Sense> {
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
            // The triangle's own normal, turned outward by its vertex
            // normals (one of them can be meaningless at a cone's tip).
            let vn = t
                .iter()
                .map(|&k| m.normals[k as usize])
                .fold(Vector3::zeros(), |s, v| s + v);
            let g = (q - p).cross(&(r - p));
            let n = if g.dot(&vn) < 0.0 { -g } else { g };
            ((c - mid).norm(), c, n)
        })
        // Zero-area triangles (at a cone's tip) give no direction.
        .filter(|(_, _, n)| n.norm() > 1e-12 * (1.0 + mid.coords.norm()).powi(2))
        .min_by(|x, y| x.0.total_cmp(&y.0))?;
    // A face whose triangles (and normals) came out inside-out: the body
    // decides which side is outside.
    let n = match n.try_normalize(1e-30) {
        Some(u) if inside(c + u * eps) && !inside(c - u * eps) => -u,
        Some(u) => u,
        None => return None,
    };
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

fn add_solid(out: &mut BRep, s: &ExactSolid, lenient: bool) -> Result<(), String> {
    let body = &s.body;
    // Once: it walks every shell.
    let open = s.is_open();
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

    // Inside test for the orientation fallback below.
    let eps = body.size().max(1e-9) * 1e-5;
    let inside =
        |p: Point3| crate::diagnostics::crash::guard(|| body.contains(v3(&p))).unwrap_or(false);
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
                // The range runs between the edge's *vertices*: an end shared
                // with a neighbour that the kernel left up to its tolerance
                // away (1.75e-6 mm at a lip's rounded corner on a downward
                // face) was merged into one vertex, and the circle's angle
                // must reach that vertex, not this edge's own last sample.
                let mut ends = pts.clone();
                let last = ends.len() - 1;
                if let (Some(va), Some(vb)) = (out.vertex(a), out.vertex(b)) {
                    ends[0] = va.point;
                    ends[last] = vb.point;
                }
                let range = curve_range(&curve, &ends)?;
                let cid = out.add_curve(curve);
                match out.add_edge(cid, a, b, range) {
                    Ok(e) => e,
                    // A line or circle fitted to the samples that misses an
                    // end (an intersection curve of a lofted face, nearly
                    // circular): the polyline through them.
                    Err(_) if lenient => {
                        let poly = polyline_curve(&pts);
                        let range = curve_range(&poly, &pts)?;
                        let cid = out.add_curve(poly);
                        out.add_edge(cid, a, b, range).map_err(|e| e.to_string())?
                    }
                    Err(e) => return Err(e.to_string()),
                }
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
                sense_by_geometry(&meshes[f], &pts, &inside, eps),
                sense_by_geometry(&meshes[g], &pts, &inside, eps),
            )
            && sf != sg_
        {
            by_face[f].push((edges.len() - 1, sf));
            by_face[g].push((edges.len() - 1, sg_));
            continue;
        }
        // Two faces of which one does not run along this edge's samples (it was meshed with
        // its own points on an edge it borrows — a bridge between surfaces): it runs the edge
        // the other way from the face that does.
        let known = |f: usize| {
            if directed[f].contains(&(k0, k1)) {
                Some(Sense::Same)
            } else if directed[f].contains(&(k1, k0)) {
                Some(Sense::Reversed)
            } else {
                None
            }
        };
        let other = |s: Sense| match s {
            Sense::Same => Sense::Reversed,
            Sense::Reversed => Sense::Same,
        };
        if let [f, g] = distinct[..]
            && faces.len() == 2
            && f < nf
            && g < nf
        {
            let pair = match (known(f), known(g)) {
                (Some(a), None) => Some((a, other(a))),
                (None, Some(b)) => Some((other(b), b)),
                _ => None,
            };
            if let Some((a, b)) = pair {
                by_face[f].push((edges.len() - 1, a));
                by_face[g].push((edges.len() - 1, b));
                continue;
            }
        }
        for &f in &distinct {
            if f >= nf {
                continue;
            }
            // A seam: the face meets itself, once in each direction. An edge of one face that
            // its triangles run one way only is a free edge (a surface body's rim), not a seam
            // (Surface bodies only: in a solid every edge has two sides, so one face there is a
            // seam the mesh lists once.)
            let both_ways = directed[f].contains(&(k0, k1)) && directed[f].contains(&(k1, k0));
            let free = open && !both_ways && faces.len() == 1;
            if faces.iter().filter(|&&g| g == f).count() > 1 || (distinct.len() == 1 && !free) {
                by_face[f].push((edges.len() - 1, Sense::Same));
                by_face[f].push((edges.len() - 1, Sense::Reversed));
                continue;
            }
            let sense = if directed[f].contains(&(k0, k1)) {
                Sense::Same
            } else if directed[f].contains(&(k1, k0)) {
                Sense::Reversed
            } else {
                // The face's triangles do not run along this edge's samples (a face meshed
                // with its own points on a borrowed edge, a bridge between surfaces): its
                // geometry decides.
                sense_by_geometry(&meshes[f], &pts, &inside, eps).unwrap_or(Sense::Reversed)
            };
            by_face[f].push((edges.len() - 1, sense));
        }
    }

    // The faces' surfaces: recognised in one walk over the kernel's faces
    // (in parallel there: a fillet's torus samples the face; asking face by
    // face walked to each one, 5 s for a plate with 5000 holes).
    let kinds = guarded("face surfaces", || Ok(sk::face_surfaces(body)))?;
    let surfaces: Vec<Option<Surface>> = (0..nf)
        .map(|f| surface_of(body, f, kinds.get(f).copied().flatten()))
        .collect();
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
            open: s.is_open(),
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
    let index: HashMap<FaceId, usize> = faces.iter().enumerate().map(|(i, f)| (*f, i)).collect();
    let pos = |f: &FaceId| index.get(f).copied().unwrap_or(usize::MAX);
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
            // The fitted circle's x axis runs through the first sample; the
            // start is a vertex shared with a neighbour (a hair off it), so
            // its angle can come out a hair below 0, outside the circle's
            // domain [0, 2π].
            let a0 = match ang(&pts[0]) {
                a if a.abs() < 1e-9 => 0.0,
                a => a,
            };
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
/// their own). Each loop starts at the first coedge not used yet and goes on
/// with the first unused one starting where it ends; the coedges are found
/// by their start vertex (a plate with 5000 holes has one face of 10000
/// coedges: a linear search per step took seconds).
fn chain_loops(out: &BRep, all: Vec<Coedge>) -> Vec<Loop> {
    let ends = |ce: &Coedge| -> Option<(VertexId, VertexId)> {
        let e = out.edge(ce.edge)?;
        Some(match ce.sense {
            Sense::Same => (e.start, e.end),
            Sense::Reversed => (e.end, e.start),
        })
    };
    let ends: Vec<Option<(VertexId, VertexId)>> = all.iter().map(ends).collect();
    // Coedges by start vertex, in their order; `next[v]` skips used ones.
    let mut starting: HashMap<VertexId, Vec<usize>> = HashMap::new();
    for (i, e) in ends.iter().enumerate() {
        if let Some((s, _)) = e {
            starting.entry(*s).or_default().push(i);
        }
    }
    let mut next: HashMap<VertexId, usize> = HashMap::new();
    let mut used = vec![false; all.len()];
    let mut take = |v: VertexId, used: &mut [bool]| -> Option<usize> {
        let list = starting.get(&v)?;
        let k = next.entry(v).or_insert(0);
        while *k < list.len() && used[list[*k]] {
            *k += 1;
        }
        let i = *list.get(*k)?;
        used[i] = true;
        Some(i)
    };
    let mut loops = Vec::new();
    for first in 0..all.len() {
        if used[first] {
            continue;
        }
        used[first] = true;
        let Some((start, mut at)) = ends[first] else {
            continue;
        };
        let mut lp = vec![all[first]];
        while at != start {
            let Some(k) = take(at, &mut used) else {
                break;
            };
            at = ends[k].map_or(at, |(_, e)| e);
            lp.push(all[k]);
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

    /// Volume of the box's sides drafted from z = `h` (`a` above, `b`
    /// below): two frusta of a 40 × 30 section.
    fn two_sided_box(h: f64, a: f64, b: f64) -> f64 {
        let (ka, kb) = (a.to_radians().tan(), b.to_radians().tan());
        simpson(
            &|z| (40.0 - 2.0 * z * ka) * (30.0 - 2.0 * z * ka),
            0.0,
            20.0 - h,
        ) + simpson(&|z| (40.0 - 2.0 * z * kb) * (30.0 - 2.0 * z * kb), 0.0, h)
    }

    #[test]
    fn draft_two_sided_box() {
        let b = box_40();
        let up = Vector3::new(0.0, 0.0, 1.0);
        for (h, a, bb) in [
            (10.0, 5.0f64, 5.0f64),
            (10.0, 5.0, 3.0),
            (5.0, 4.0, 4.0),
            (15.0, 2.0, 6.0),
            (10.0, -3.0, -3.0),
        ] {
            let mid = Plane::from_normal(Point3::new(0.0, 0.0, h), up).unwrap();
            let d = draft_two_sided(
                &b,
                &sides(&b),
                &mid,
                up,
                Some((&mid, a.to_radians())),
                Some((&mid, bb.to_radians())),
            )
            .unwrap();
            check_shell(&d, two_sided_box(h, a, bb));
            let names: Vec<&str> = d.names().iter().flatten().map(String::as_str).collect();
            // Each side in two pieces, both named after it.
            assert_eq!(
                names.iter().filter(|n| **n == "Box1:Front").count(),
                2,
                "{names:?}"
            );
        }
        // One side only: the other half stays.
        let mid = Plane::from_normal(Point3::new(0.0, 0.0, 10.0), up).unwrap();
        let d = draft_two_sided(
            &b,
            &sides(&b),
            &mid,
            up,
            Some((&mid, 5f64.to_radians())),
            None,
        )
        .unwrap();
        check_shell(&d, two_sided_box(10.0, 5.0, 0.0));
        // Move Parting Line: the top half about its top edges (z = 20), the
        // bottom half left; the upper part overhangs at the parting line.
        let top = Plane::from_normal(Point3::new(0.0, 0.0, 20.0), up).unwrap();
        let k = 3f64.to_radians().tan();
        let d = draft_two_sided(
            &b,
            &sides(&b),
            &mid,
            up,
            Some((&top, 3f64.to_radians())),
            None,
        )
        .unwrap();
        check_shell(
            &d,
            12_000.0
                + simpson(
                    &|z| (40.0 + 2.0 * (20.0 - z) * k) * (30.0 + 2.0 * (20.0 - z) * k),
                    10.0,
                    20.0,
                ),
        );
        // The plane must be square to the pull.
        let tilted =
            Plane::from_normal(Point3::new(0.0, 0.0, 10.0), Vector3::new(0.0, 0.3, 1.0)).unwrap();
        assert!(draft_two_sided(&b, &sides(&b), &tilted, up, Some((&tilted, 0.1)), None).is_err());
    }

    #[test]
    fn draft_two_sided_cylinder_and_l_prism() {
        // A cylinder R10 × 20 split at z = 8: two cones meeting at R10.
        let c = cylinder(10.0, 20.0, "Extrusion1");
        let wall = face(&c, "Extrusion1:Side:C1:0", (10.0, 0.0, 10.0));
        let up = Vector3::new(0.0, 0.0, 1.0);
        let mid = Plane::from_normal(Point3::new(0.0, 0.0, 8.0), up).unwrap();
        let k = 5f64.to_radians().tan();
        let d = draft_two_sided(
            &c,
            &[wall],
            &mid,
            up,
            Some((&mid, 5f64.to_radians())),
            Some((&mid, 5f64.to_radians())),
        )
        .unwrap();
        let frustum = |h: f64| {
            let r = 10.0 - h * k;
            PI * h / 3.0 * (100.0 + 10.0 * r + r * r)
        };
        check_shell(&d, frustum(12.0) + frustum(8.0));
        // The L's outer long walls, 3° both ways about z = 5.
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
        let bw = face(&l, "Extrusion1:Side:L5:0", (0.0, 15.0, 5.0));
        let k = 3f64.to_radians().tan();
        let mid = Plane::from_normal(Point3::new(0.0, 0.0, 5.0), up).unwrap();
        let d = draft_two_sided(
            &l,
            &[a, bw],
            &mid,
            up,
            Some((&mid, 3f64.to_radians())),
            Some((&mid, 3f64.to_radians())),
        )
        .unwrap();
        // Per half (height 5): two wedges 30 long, less the corner pyramid.
        let lost = 2.0 * (2.0 * 30.0 * 0.5 * 5.0 * 5.0 * k - k * k * 125.0 / 3.0);
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

mod coil;
pub use coil::{CoilMesh, CoilMotion, CoilZone, coil, coil_boolean, coil_mesh};

mod loft;
pub use loft::{
    GuidedLoft, LoftEnd, LoftPlaced, StationOutline, loft_guided, loft_stations, profile_area,
    profile_outline,
};
mod sheet;
pub use sheet::{
    SheetSide, closest_on_triangle, extrude_sheet, revolve_sheet, sheet_nearest, split_by_sheet,
    sweep_sheet_along,
};
pub mod surface_ops;

pub mod sheet_metal;

mod delete_face;
mod sweep;
mod thicken;
pub use delete_face::{delete_faces, delete_shell};
pub use sweep::{
    Frame, PathPiece, check_bend, frame_point, loft, loft_frames, loft_surface, loft_to_point,
    path_points, sweep, sweep_surface,
};
pub use thicken::{face_sheet, offset_sheet, offset_solid, thicken_faces, thicken_sheet};
