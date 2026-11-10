//! Shell, offset faces, draft and thicken through SMLib: the body goes to SMLib as its schema 4
//! record, tagged as operand 0, SMLib runs the operation, and the result comes back with SMLib's own
//! history, renumbered into the body's and the result's face and edge order (as
//! `crate::smlib_boolean` does for booleans). A result SMLib's own check refuses is an
//! error, never a body.
//!
//! SMLib's tags travel through the copies its offset makes, so a shell's inner wall and a
//! thickened sheet's moved copy carry the tag of the face they were made from, and SMLib reads
//! both that face and its copy as pieces of it (Split). Which is which is a matter of where they
//! lie: the copy is the operation's distance from the face, the face itself on it. So a result
//! face farther from its tagged face than half that distance is recorded as its offset
//! ([`FaceOrigin::Offset`]), and its edges as where its faces meet. A face SMLib makes new along
//! a sheet's open edge (a thickened sheet's wall) is recorded as made in place of that edge.

use solidcraft_geom::prelude::*;
use std::collections::{BTreeMap, BTreeSet};

use solidcore_record::Vec3;
use solidcraft_smlib::{Brep, CheckLevel, Error as SmError};

use crate::body::Body;
use crate::history::{Change, EdgeOrigin, FaceOrigin, History};
use crate::native::{body_from_smlib_ordered, smlib_body_ordered};
use crate::{KernelError, Result, record};

fn failed(op: &str, what: &str, e: impl crate::Cause) -> KernelError {
    KernelError::smlib(format!("SMLib {op}: {what}"), e)
}

/// The error for SMLib's refusal of `op`, in the words of the operation; `wall` is its wall's
/// thickness, where it has one.
fn refused(op: &str, wall: Option<f64>, e: SmError) -> KernelError {
    match e {
        SmError::NoRoom => KernelError::Invalid(format!("{op}: the wall is too thick: it leaves no room inside the body")),
        // The walls just meet: thinner they stay apart, thicker they cross and merge.
        SmError::WallsTouch => {
            let wall = wall.map(|t| format!(" {t} mm")).unwrap_or_default();
            KernelError::Invalid(format!(
                "{op}: the{wall} walls would meet along a line (between a hole and the side, for example), which leaves no solid there; \
                 use a thinner or thicker wall, or move the feature"
            ))
        }
        // solidcore's one construction: a face curves tighter than the wall is thick.
        SmError::WallFolds { largest } => {
            let largest = largest.map(|w| format!("; the largest wall that works is {} mm", crate::blend::trimmed_mm(w))).unwrap_or_default();
            KernelError::Invalid(format!("{op}: the wall is thicker than a face curves, so the face's offset would fold{largest}"))
        }
        SmError::NoMatch { point } => KernelError::Invalid(format!("{op}: no face at selection point {}", point + 1)),
        SmError::Ambiguous { point } => KernelError::Invalid(format!("{op}: selection point {} is as near to two faces", point + 1)),
        SmError::Unsupported => KernelError::Failed(format!("not supported yet: SMLib {op} for this geometry")),
        SmError::NoEffect => KernelError::Failed(format!("SMLib made no change: the {op} reported success but moved no face onto its new surface")),
        e => failed(op, "operation", e),
    }
}

/// How far a selection point may be from the face it picks: the kernel's own picking reach.
fn pick_tolerance(b: &Body) -> f64 {
    b.size() * 1e-3 + 1e-6
}

fn points(p: &[Vec3]) -> Vec<[f64; 3]> {
    p.iter().map(|v| [v.x, v.y, v.z]).collect()
}

/// Runs `op` on `b` in SMLib, tagged as operand 0, and brings the result back with SMLib's
/// history over `[b]` (none where an order cannot be matched). `wall`: the operation's wall
/// thickness, for its refusals.
pub(crate) fn run(
    name: &str,
    b: &Body,
    wall: Option<f64>,
    op: impl FnOnce(&Brep) -> std::result::Result<Brep, SmError>,
) -> Result<(Body, Option<History>)> {
    b.require_brep(name)?;
    let (sb, ob) = smlib_body_ordered(b)?;
    let tagged = sb.tagged(0).map_err(|e| failed(name, "tag", e))?;
    let r = op(&tagged).map_err(|e| refused(name, wall, e))?;
    let check = r.check(CheckLevel::All).map_err(|e| failed(name, "check", e))?;
    let mut straightened = None;
    if !check.is_valid() {
        straightened = straightened_export(&r)?;
        if straightened.is_none() {
            let input = sb.check(CheckLevel::All).is_ok_and(|c| !c.is_valid());
            let what = if input { "SMLib's check refuses the result (the body already fails it)" } else { "SMLib's check refuses the result" };
            return Err(failed(name, what, check));
        }
    }
    let (body, or) = match straightened {
        Some((record, order)) => (Body::from_smlib_record(record)?, Some(order)),
        None => body_from_smlib_ordered(&r)?,
    };
    let history = match (ob, or) {
        (Some(ob), Some(or)) => r.history().map_err(|e| failed(name, "history", e))?.history(&[ob], &or).map(Some).unwrap_or_else(|e| {
            log::debug!(target: "smlib", "{name} history: {e}");
            None
        }),
        _ => None,
    };
    Ok((body, history))
}

/// The widest gap at the seam of a closed row or column of a control net taken for a fitter's slip
/// (mm, [`straighten_nets`]): a thousandth of the finest fitting tolerance a thicken or offset
/// asks for (0.01 mm).
const NET_SLIP: f64 = 1e-5;

/// `r`'s export with its nets' seams closed ([`straighten_nets`]), with the order of SMLib's faces
/// and edges, where that changed something and SMLib's check accepts the result imported again
/// with its splines as they are (3DCadCraft addition). SMLib's fitted offset of a swept tube came
/// back with rows round the tube whose last control point overshoots the first by up to 2.6e-6 mm:
/// the step closing the row points back against the row's first step (0.037 mm), which its check
/// (test #5, a net that doubles back) refuses.
fn straightened_export(r: &Brep) -> Result<Option<(solidcore_record::brep::Brep, solidcraft_smlib::history::Order)>> {
    // A result that cannot be exported cannot be a body either: that is the error.
    let (mut record, order) = r.to_native_ordered().map_err(|e| KernelError::Failed(e.to_string()))?;
    if !straighten_nets(&mut record) {
        return Ok(None);
    }
    let Ok(again) = Brep::from_record_keeping_splines(&record, true) else { return Ok(None) };
    Ok(again.check(CheckLevel::All).is_ok_and(|c| c.is_valid()).then_some((record, order)))
}

/// Each B-spline net of `r` with the seams of its closed rows and columns closed: where a line's
/// two end points are less than [`NET_SLIP`] apart and the step from its last point to its first
/// turns back (more than 170 degrees, the angle SMLib's check refuses) against the line's first or
/// last step, at least a hundred times longer, both end points go to their middle. Each moves
/// less than half the gap, so the surface by less than that. Whether anything moved.
fn straighten_nets(r: &mut solidcore_record::brep::Brep) -> bool {
    use solidcore_record::brep::Surface;
    let mut moved = false;
    for f in &mut r.faces {
        match &mut f.surface {
            Surface::Spline { points, .. } => {
                let mut net: Vec<Vec<Vec3>> = points.iter().map(|row| row.iter().map(|p| Vec3::new(p[0], p[1], p[2])).collect()).collect();
                if straighten(&mut net) {
                    moved = true;
                    for (row, out) in net.iter().zip(points.iter_mut()) {
                        for (p, o) in row.iter().zip(out.iter_mut()) {
                            *o = [p.x, p.y, p.z];
                        }
                    }
                }
            }
            Surface::RationalSpline { homogeneous, .. } => {
                if homogeneous.iter().flatten().any(|h| !(h[3].is_finite() && h[3] != 0.0)) {
                    continue;
                }
                let mut net: Vec<Vec<Vec3>> = homogeneous.iter().map(|row| row.iter().map(|h| Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3])).collect()).collect();
                if straighten(&mut net) {
                    moved = true;
                    for (row, out) in net.iter().zip(homogeneous.iter_mut()) {
                        for (p, o) in row.iter().zip(out.iter_mut()) {
                            let w = o[3];
                            *o = [p.x * w, p.y * w, p.z * w, w];
                        }
                    }
                }
            }
            _ => {}
        }
    }
    moved
}

/// [`straighten_nets`] on one net of Euclidean points, rows then columns.
fn straighten(net: &mut [Vec<Vec3>]) -> bool {
    let cols = net.first().map_or(0, Vec::len);
    if net.iter().any(|row| row.len() != cols) {
        return false;
    }
    let mut moved = false;
    for row in net.iter_mut() {
        moved |= straighten_line(row);
    }
    for j in 0..cols {
        let mut line: Vec<Vec3> = net.iter().filter_map(|row| row.get(j).copied()).collect();
        if straighten_line(&mut line) {
            moved = true;
            for (row, q) in net.iter_mut().zip(line) {
                if let Some(p) = row.get_mut(j) {
                    *p = q;
                }
            }
        }
    }
    moved
}

/// [`straighten_nets`] on one row or column.
fn straighten_line(line: &mut [Vec3]) -> bool {
    let n = line.len();
    let (Some(&first), Some(&second), Some(&before), Some(&last)) = (line.first(), line.get(1), n.checked_sub(2).and_then(|i| line.get(i)), line.last()) else {
        return false;
    };
    if n < 3 {
        return false;
    }
    let gap = first - last;
    let lg = gap.len();
    if !(lg > 0.0 && lg < NET_SLIP) {
        return false;
    }
    let back = (170f64).to_radians().cos();
    let turns_back = |step: Vec3| {
        let ls = step.len();
        ls >= 100.0 * lg && gap.dot(step) < back * lg * ls
    };
    if !(turns_back(second - first) || turns_back(last - before)) {
        return false;
    }
    let mid = (first + last) * 0.5;
    if let Some(p) = line.first_mut() {
        *p = mid;
    }
    if let Some(p) = line.last_mut() {
        *p = mid;
    }
    true
}

/// [`crate::shell`] through SMLib: walls of `thickness` inside, open at the faces under the
/// points `open` (the face of the nearest triangle) and the faces
/// on the same surface joined to them: a round wall stored in pieces (a hole's two halves) opens
/// whole. SMLib is given one point inside each, never the pick itself, which may lie on the
/// line between two pieces.
pub(crate) fn shell(b: &Body, open: &[Vec3], thickness: f64) -> Result<Body> {
    if !(thickness.is_finite() && thickness > 1e-6) {
        return Err(KernelError::Invalid("shell thickness must be positive".into()));
    }
    let picked = faces_under(b, open)?;
    let opened = crate::offset::with_same_surface(b, &picked);
    let tol = pick_tolerance(b);
    let pts = points(&face_points(b, &opened)?);
    let (r, h) = run("shell", b, Some(thickness), |s| s.shell(&pts, tol, -thickness))?;
    let h = h.and_then(|h| with_offsets(&r, b, h, thickness));
    Ok(r.with_checked_history(h, &record::counts(&[b])))
}

/// [`crate::offset_faces`] through SMLib: the faces `chosen` (indices in `b`'s face order)
/// moved `distance` along their outward normals. The moved faces are the faces themselves,
/// moved (Modified).
pub(crate) fn offset_faces(b: &Body, chosen: &[usize], distance: f64) -> Result<Body> {
    let at = face_points(b, chosen)?;
    let tol = pick_tolerance(b);
    let pts = points(&at);
    let (r, h) = run("offset faces", b, None, |s| s.offset_faces(&pts, tol, distance))?;
    let h = h.map(|mut h| {
        for f in &mut h.faces {
            if f.change == Change::Kept && f.origins.iter().any(|o| matches!(o, FaceOrigin::Face { face, .. } if chosen.contains(face))) {
                f.change = Change::Modified;
            }
        }
        h
    });
    Ok(r.with_checked_history(h, &record::counts(&[b])))
}

/// How many times [`thicken`] halves a sheet's spline faces whose wall comes out too large.
const MAX_HALVINGS: usize = 2;

/// [`crate::thicken`] through SMLib: the sheet `b` grown by `thickness` along its faces'
/// normals, or half each way when `symmetric`. SMLib fits a free-form face's offset with a net
/// that can outgrow what the evaluator takes (a tube swept along a spline, 430 × 9 control points,
/// thickened outward: 864 × 415, past 2^18 points); then the sheet's spline faces are halved and
/// the halves thickened, each fitted on its own ([`thicken_halved`]).
pub(crate) fn thicken(b: &Body, thickness: f64, symmetric: bool) -> Result<Body> {
    thicken_within(b, thickness, symmetric, MAX_HALVINGS)
}

fn thicken_within(b: &Body, thickness: f64, symmetric: bool, halvings: usize) -> Result<Body> {
    match thicken_once(b, thickness, symmetric) {
        Err(e) if halvings > 0 && too_large(&e) => thicken_halved(b, thickness, symmetric, halvings - 1).map_err(|_| e),
        other => other,
    }
}

/// Whether `e` says a spline surface has more control points than the evaluator takes
/// (`solidcraft_geom`'s `TooLarge`, reported through the export of SMLib's result).
fn too_large(e: &KernelError) -> bool {
    let text = e.to_string();
    text.contains("a spline surface of") && text.contains("points")
}

/// [`thicken`] of `b` with each spline face cut in two by the plane of its iso curve half way
/// along the direction with more control points (that curve must be flat: a swept section), the
/// halves thickened, and the history composed back onto `b`'s faces and edges (3DCadCraft
/// addition).
fn thicken_halved(b: &Body, thickness: f64, symmetric: bool, halvings: usize) -> Result<Body> {
    let r = b.record();
    let mut cut = b.clone();
    let mut inner: Option<History> = None;
    for f in 0..r.faces.len() {
        let Some(plane) = r.faces.get(f).and_then(middle_plane) else { continue };
        // The faces of `cut` that are face f of `b`: before any split, f itself.
        let faces: Vec<usize> = match &inner {
            None => vec![f],
            Some(h) => h
                .faces
                .iter()
                .enumerate()
                .filter(|(_, p)| p.origins.iter().any(|o| matches!(o, FaceOrigin::Face { face, .. } if *face as usize == f)))
                .map(|(i, _)| i)
                .collect(),
        };
        let split = crate::splitface::split_faces(&cut, &faces, &crate::splitface::SplitTool::Plane(plane))?;
        let h = split.history().cloned().ok_or_else(|| KernelError::Failed("thicken: the halved sheet has no history".into()))?;
        inner = Some(match inner {
            None => h,
            Some(before) => h.substituted(0, &before).map_err(|e| KernelError::Failed(e.to_string()))?,
        });
        cut = split;
    }
    let Some(inner) = inner else { return Err(KernelError::Failed("thicken: no spline face to halve".into())) };
    let made = thicken_within(&cut, thickness, symmetric, halvings)?;
    let h = made.history().cloned().and_then(|h| h.substituted(0, &inner).ok());
    Ok(made.with_checked_history(h, &record::counts(&[b])))
}

/// The plane of the iso curve of `face`'s spline surface half way along the parameter with more
/// control points, where that curve is flat (within a millionth of its size).
fn middle_plane(face: &solidcore_record::brep::Face) -> Option<solidcore_record::Plane> {
    use solidcore_record::brep::Surface;
    let (nu, nv) = match &face.surface {
        Surface::Spline { points, .. } => (points.len(), points.first()?.len()),
        Surface::RationalSpline { homogeneous, .. } => (homogeneous.len(), homogeneous.first()?.len()),
        _ => return None,
    };
    let s = solidcraft_geom::brep::eval::SurfaceEval::new(face.parameters()).ok()?;
    let [[u0, u1], [v0, v1]] = face.domain;
    const SAMPLES: usize = 32;
    let pts: Vec<Vec3> = (0..SAMPLES)
        .map(|k| {
            let t = k as f64 / SAMPLES as f64;
            if nu >= nv { s.point([0.5 * (u0 + u1), v0 + t * (v1 - v0)]) } else { s.point([u0 + t * (u1 - u0), 0.5 * (v0 + v1)]) }
        })
        .collect::<Option<_>>()?;
    let centre = pts.iter().fold(Vec3::ZERO, |a, p| a + *p) * (1.0 / SAMPLES as f64);
    // Newell's normal of the closed polygon through the samples.
    let mut n = Vec3::ZERO;
    for (i, p) in pts.iter().enumerate() {
        let q = pts.get((i + 1) % SAMPLES)?;
        n = n + (*p - centre).cross(*q - centre);
    }
    let n = n.normalized()?;
    let size = pts.iter().map(|p| p.dist(centre)).fold(0.0, f64::max);
    if !(size > 0.0) || pts.iter().any(|p| (*p - centre).dot(n).abs() > 1e-6 * size.max(1.0)) {
        return None;
    }
    let x = (*pts.first()? - centre).normalized()?;
    let x = (x - n * x.dot(n)).normalized()?;
    Some(solidcore_record::Plane { origin: centre, x, y: n.cross(x) })
}

fn thicken_once(b: &Body, thickness: f64, symmetric: bool) -> Result<Body> {
    if !(thickness.is_finite() && thickness.abs() > 1e-9) {
        return Err(KernelError::Invalid("thickness must be non-zero".into()));
    }
    // SMLib's thicken takes open sheets only; a closed one (a sphere, an egg) is the hollow its
    // shell makes with no opening, from the surface moved half way back when symmetric.
    let tol = pick_tolerance(b);
    // One side: SMLib's thicken, or the hollow of a closed sheet.
    let one_side = |s: &Brep, t: f64| match s.thicken(t, false) {
        Err(SmError::Argument) => s.shell(&[], tol, t),
        other => other,
    };
    let (r, h) = run("thicken", b, Some(thickness.abs()), |s| {
        let made = match s.thicken(thickness, symmetric) {
            Err(SmError::Argument) if symmetric => s.offset_sheet(-0.5 * thickness).and_then(|m| m.shell(&[], tol, thickness)),
            Err(SmError::Argument) => s.shell(&[], tol, thickness),
            other => other,
        };
        match made {
            // A symmetric thicken moves the surface half way first; that fitted copy can fail
            // where the surface itself holds (an egg's pole: "the largest wall that works is 0").
            // Then the two halves of the surface itself, joined along it.
            Err(e) if symmetric => {
                let (Ok(out), Ok(inn)) = (one_side(s, 0.5 * thickness), one_side(s, -0.5 * thickness)) else { return Err(e) };
                out.boolean(&inn, solidcraft_smlib::BooleanOp::Union)?.ok_or(e)
            }
            other => other,
        }
    })?;
    let step = if symmetric { thickness.abs() / 2.0 } else { thickness.abs() };
    // The walls first, while the sheet's edges still carry their tags on both copies.
    let h = h.map(with_edge_walls).and_then(|h| with_offsets(&r, b, h, step));
    Ok(r.with_checked_history(h, &record::counts(&[b])))
}

/// [`crate::draft`] through SMLib: the faces under the points `at` (flat, parallel to `pull`)
/// tilted by `angle` about their lines on `neutral`, leaning in going along `pull`; the faces
/// around them extended or trimmed to meet them. A tilted face is the face itself (Modified).
/// A face SMLib can't draft (not flat, or not parallel to the pull) is "not supported yet".
pub(crate) fn draft(b: &Body, at: &[Vec3], neutral: &solidcore_record::Plane, pull: Vec3, angle: f64) -> Result<Body> {
    if !angle.is_finite() || angle.abs() >= std::f64::consts::FRAC_PI_2 - 1e-3 {
        return Err(KernelError::Invalid("draft angle must be between −90° and 90°".into()));
    }
    let pull = pull.normalized().ok_or_else(|| KernelError::Invalid("pull direction".into()))?;
    let normal = neutral.normal().normalized().ok_or_else(|| KernelError::Invalid("neutral plane".into()))?;
    if normal.dot(pull).abs() < 1e-9 {
        return Err(KernelError::Failed("the neutral plane is parallel to the pull direction".into()));
    }
    let chosen = faces_under(b, at)?;
    let tol = pick_tolerance(b);
    let pts = points(&face_points(b, &chosen)?);
    let o = neutral.origin;
    let (r, h) = run("draft", b, None, |s| s.draft(&pts, tol, [o.x, o.y, o.z], [normal.x, normal.y, normal.z], [pull.x, pull.y, pull.z], angle))?;
    let h = h.map(|mut h| {
        for f in &mut h.faces {
            if f.change == Change::Kept && f.origins.iter().any(|o| matches!(o, FaceOrigin::Face { face, .. } if chosen.contains(face))) {
                f.change = Change::Modified;
            }
        }
        h
    });
    Ok(r.with_checked_history(h, &record::counts(&[b])))
}

/// The face of `b` under each point: the face of the nearest triangle, within the picking reach.
fn faces_under(b: &Body, at: &[Vec3]) -> Result<Vec<usize>> {
    let reach = pick_tolerance(b);
    let mesh = b.mesh(&crate::MeshTolerance::chord((b.size() * 1e-3).max(1e-3)))?;
    let mut out: Vec<usize> = Vec::new();
    for p in at {
        let near = mesh
            .triangles
            .iter()
            .zip(&mesh.tri_face)
            .filter_map(|(t, f)| mesh.tri(t).map(|[x, y, z]| (solidcraft_geom::compare::closest_on_triangle(*p, x, y, z).dist(*p), *f as usize)))
            .min_by(|x, y| x.0.total_cmp(&y.0));
        match near {
            Some((d, f)) if d < reach => {
                if !out.contains(&f) {
                    out.push(f);
                }
            }
            _ => return Err(KernelError::Invalid(format!("no face at {:?}", [p.x, p.y, p.z]))),
        }
    }
    Ok(out)
}

/// One point inside each face `faces` of `b`: the centroid of its largest triangle.
pub(crate) fn face_points(b: &Body, faces: &[usize]) -> Result<Vec<Vec3>> {
    let mesh = b.mesh(&crate::MeshTolerance::chord((b.size() * 1e-3).max(1e-3)))?;
    faces
        .iter()
        .map(|f| {
            mesh.triangles
                .iter()
                .zip(&mesh.tri_face)
                .filter(|(_, t)| **t as usize == *f)
                .filter_map(|(t, _)| mesh.tri(t))
                .max_by(|x, y| area(x).total_cmp(&area(y)))
                .map(|[p, q, s]| (p + q + s) * (1.0 / 3.0))
                .ok_or_else(|| KernelError::Failed(format!("offset faces: face {f} has no triangles")))
        })
        .collect()
}

fn area([a, b, c]: &[Vec3; 3]) -> f64 {
    (*b - *a).cross(*c - *a).len()
}

/// `h` over `[input]` with each result face that lies `distance` (more than half of it) from
/// the input face it is tagged with recorded as that face's offset, the faces left on their
/// input face Split only where several still share it, and every edge of an offset face as
/// where its faces meet. `None` where a face cannot be measured.
fn with_offsets(result: &Body, input: &Body, mut h: History, distance: f64) -> Option<History> {
    let chord = (input.size().max(result.size()) * 1e-3).min(distance / 8.0).max(1e-6);
    let source = input.mesh(&crate::MeshTolerance::chord(chord)).ok()?;
    let made = result.mesh(&crate::MeshTolerance::chord(chord)).ok()?;
    let at = |mesh: &solidcraft_geom::Mesh, face: usize| -> Vec<[Vec3; 3]> {
        mesh.triangles.iter().zip(&mesh.tri_face).filter(|(_, t)| **t as usize == face).filter_map(|(t, _)| mesh.tri(t)).collect()
    };
    let mut offset = vec![false; h.faces.len()];
    for (i, f) in h.faces.iter_mut().enumerate() {
        let [FaceOrigin::Face { operand: 0, face }] = f.origins[..] else { continue };
        let tris = at(&made, i);
        let p = tris.iter().max_by(|x, y| area(x).total_cmp(&area(y))).map(|[a, b, c]| (*a + *b + *c) * (1.0 / 3.0))?;
        let under = at(&source, face);
        let d = under.iter().map(|[a, b, c]| solidcraft_geom::compare::closest_on_triangle(p, *a, *b, *c).dist(p)).fold(f64::INFINITY, f64::min);
        if !d.is_finite() {
            return None;
        }
        if d > distance / 2.0 {
            *f = record::generated(vec![FaceOrigin::Offset { operand: 0, face }]);
            offset[i] = true;
        }
    }
    // Faces still on an input face: Split only where several share it.
    let mut uses: BTreeMap<FaceOrigin, usize> = BTreeMap::new();
    for f in &h.faces {
        if let [o @ FaceOrigin::Face { .. }] = f.origins[..] {
            *uses.entry(o).or_default() += 1;
        }
    }
    for f in &mut h.faces {
        if f.change == Change::Split
            && let [o @ FaceOrigin::Face { .. }] = f.origins[..]
            && uses.get(&o) == Some(&1)
        {
            f.change = Change::Modified;
        }
    }
    let faces = h.faces.clone();
    for e in &mut h.edges {
        if e.faces.iter().any(|f| offset.get(*f).copied().unwrap_or(false)) {
            e.origins = record::meeting(&e.faces, &faces);
        }
    }
    Some(h)
}

/// `h` with each face new to the operation that stands on exactly one input edge (its edges
/// tagged with that edge, a thickened sheet's wall) recorded as made in place of that edge.
fn with_edge_walls(mut h: History) -> History {
    let mut along: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); h.faces.len()];
    for e in &h.edges {
        for o in &e.origins {
            if let EdgeOrigin::Edge { operand: 0, edge } = o {
                for f in &e.faces {
                    if let Some(s) = along.get_mut(*f) {
                        s.insert(*edge);
                    }
                }
            }
        }
    }
    for (f, edges) in h.faces.iter_mut().zip(&along) {
        if f.origins.is_empty()
            && let [edge] = edges.iter().copied().collect::<Vec<_>>()[..]
        {
            *f = record::generated(vec![FaceOrigin::Edge { operand: 0, edge }]);
        }
    }
    h
}
