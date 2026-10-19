//! Regeneration through the exact kernel (`kernel::exact`, see
//! `docs/SOLIDCRAFT.md`): each body-making feature becomes exact solids, and
//! the part's [`BRep`] is converted from them after every feature.
//!
//! A builder returns `Ok(None)` for an option the exact path does not cover
//! yet; regeneration then goes on with the faceted kernel from that feature.

use crate::kernel::exact::{self, ChamferSecond, ExactOp, ExactSolid};
use crate::kernel::{BRep, Curve, Point3, Real, TessellationOptions};
use crate::sketch::Sketch;

use super::blend::{BlendFeature, BlendKind, ChamferMethod, EdgeRef, FilletType};
use super::boolean::{Operation, Output};
use super::extrude::{ExtrudeFeature, kernel_profile};
use super::revolve::{RevolveFeature, resolve_axis};

/// Why an exact build failed.
#[derive(Debug)]
pub(super) enum Fail {
    /// The feature's own problem (missing profile, nothing to cut): shown
    /// as its health, whatever kernel builds the part.
    User(String),
    /// The exact kernel could not do it: the part is rebuilt with the
    /// faceted kernel.
    Kernel(String),
}

use Fail::{Kernel, User};

/// Whether regeneration may use the exact kernel (`CADCRAFT3D_KERNEL=facet`
/// switches it off, for comparison).
pub(super) fn enabled() -> bool {
    std::env::var("CADCRAFT3D_KERNEL").map_or(true, |v| v != "facet")
}

/// The prefix of region `n` of a feature (`Extrusion1`, `Extrusion1#2`).
fn prefix(id: &str, n: usize) -> String {
    if n == 0 {
        id.to_owned()
    } else {
        format!("{id}#{}", n + 1)
    }
}

/// The solids of an Extrude (one per region), before its operation.
pub(super) fn extrude(
    e: &ExtrudeFeature,
    body: &BRep,
    sketch: &Sketch,
) -> Result<Option<Vec<ExactSolid>>, Fail> {
    if e.output == Output::Surface {
        return Ok(None);
    }
    let regions = sketch.regions();
    let plane = &sketch.plane;
    let mut out = Vec::new();
    for (n, pr) in e.profiles.iter().enumerate() {
        let region = pr
            .resolve(&regions)
            .ok_or_else(|| User(format!("profile #{} no longer exists in the sketch", n + 1)))?;
        let levels = e.levels(body, plane, region).map_err(User)?;
        let (outer, holes) = kernel_profile(region);
        let prof = exact::profile(&outer, &holes).map_err(Kernel)?;
        let (first, last) = match (levels.first(), levels.last()) {
            (Some(a), Some(b)) => (a, b),
            _ => return Err(User("the extrusion has no depth".into())),
        };
        let straight = levels.iter().all(|l| l.inset.abs() < 1e-12);
        let taper = if straight {
            0.0
        } else if levels.len() == 2 && first.inset.abs() < 1e-12 {
            (last.inset / (last.offset - first.offset).abs()).atan()
        } else {
            // A two-sided taper (kinked at the sketch plane): not yet.
            return Ok(None);
        };
        out.push(
            exact::extrude(
                plane,
                &prof,
                first.offset,
                last.offset,
                taper,
                &prefix(&e.id, n),
            )
            .map_err(Kernel)?,
        );
    }
    Ok(Some(out))
}

/// The solids of a Revolve (one per region), before its operation.
pub(super) fn revolve(
    r: &RevolveFeature,
    sketch: &Sketch,
) -> Result<Option<Vec<ExactSolid>>, Fail> {
    if r.output == Output::Surface {
        return Ok(None);
    }
    let axis = resolve_axis(&r.axis, sketch).map_err(User)?;
    let (start, sweep) = r.sweep().map_err(User)?;
    let regions = sketch.regions();
    let mut out = Vec::new();
    for (n, pr) in r.profiles.iter().enumerate() {
        let region = pr
            .resolve(&regions)
            .ok_or_else(|| User(format!("profile #{} no longer exists in the sketch", n + 1)))?;
        let (outer, holes) = kernel_profile(region);
        let prof = exact::profile(&outer, &holes).map_err(Kernel)?;
        out.push(
            exact::revolve(
                &sketch.plane,
                &prof,
                axis.origin,
                axis.dir,
                start,
                sweep,
                &prefix(&r.id, n),
            )
            .map_err(Kernel)?,
        );
    }
    Ok(Some(out))
}

/// Applies a feature's operation: `added` (its solids) against the bodies
/// so far (all of them, or those named in `solids`).
pub(super) fn combine(
    mut cur: Vec<ExactSolid>,
    added: Vec<ExactSolid>,
    op: Operation,
    solids: &[String],
) -> Result<Vec<ExactSolid>, Fail> {
    if op == Operation::NewSolid {
        cur.extend(added);
        return Ok(cur);
    }
    // Many tools apart from each other (a sketch full of holes): one
    // compound tool, so the body is cut once instead of once per tool on an
    // ever larger body (36 holes: 30 s → see docs/PERF.md).
    let added = match exact::union_apart(&added) {
        Ok(Some(one)) => vec![one],
        _ => added,
    };
    let chosen =
        |s: &ExactSolid| solids.is_empty() || s.name.as_ref().is_some_and(|n| solids.contains(n));
    for t in added {
        let targets: Vec<usize> = (0..cur.len()).filter(|&i| chosen(&cur[i])).collect();
        if targets.is_empty() {
            return Err(User(if solids.is_empty() {
                format!("{}: there is no solid body to operate on", op.label())
            } else {
                format!("{}: the chosen body no longer exists", op.label())
            }));
        }
        match op {
            Operation::Join => {
                // With the first chosen body; a separate lump when apart.
                let i = targets[0];
                let r = exact::boolean(&cur[i], &t, ExactOp::Union)
                    .map_err(|e| Kernel(format!("Join failed: {e}")))?
                    .ok_or_else(|| Kernel("Join failed: the result is empty".into()))?;
                cur[i] = r;
            }
            Operation::Cut => {
                let mut hit = false;
                let mut gone = Vec::new();
                // Volume before and after: a tool that only touches the body
                // (a cut pointing away from it) removes nothing.
                let vol = |s: &ExactSolid| s.measure_full().map(|m| m.volume).unwrap_or(0.0);
                for &i in &targets {
                    if !exact::meets(&cur[i], &t) {
                        continue;
                    }
                    let before = vol(&cur[i]);
                    match exact::boolean(&cur[i], &t, ExactOp::Cut)
                        .map_err(|e| Kernel(format!("Cut failed: {e}")))?
                    {
                        Some(r) => {
                            if before - vol(&r) > 1e-9 * before.max(1.0) {
                                hit = true;
                            }
                            cur[i] = r;
                        }
                        None => {
                            hit = true;
                            gone.push(i);
                        }
                    }
                }
                if !hit {
                    return Err(User("Cut: the feature does not intersect the body".into()));
                }
                for i in gone.into_iter().rev() {
                    cur.remove(i);
                }
            }
            Operation::Intersect => {
                let mut kept = false;
                let mut gone = Vec::new();
                for &i in &targets {
                    let r = if exact::meets(&cur[i], &t) {
                        exact::boolean(&cur[i], &t, ExactOp::Intersect)
                            .map_err(|e| Kernel(format!("Intersect failed: {e}")))?
                    } else {
                        None
                    };
                    match r {
                        Some(r) => {
                            cur[i] = r;
                            kept = true;
                        }
                        None => gone.push(i),
                    }
                }
                if !kept {
                    return Err(User(
                        "Intersect: the feature does not intersect the body".into(),
                    ));
                }
                for i in gone.into_iter().rev() {
                    cur.remove(i);
                }
            }
            Operation::NewSolid => {}
        }
    }
    Ok(cur)
}

/// Points on referenced edges for the kernel, per solid: a straight edge a
/// Boolean split in two (an imprinted vertex nothing else uses) is one edge
/// to the kernel, which heals it before blending — one point for the joined
/// pieces, or the second point lands on the new round ("the edge is
/// smooth"); pieces of one circle (a full circle comes as two halves) are
/// one closed edge — one point each (72 points for 36 holes made it blend
/// twice the edges). Edges already taken (by another set) are skipped.
struct EdgePoints<'a> {
    body: &'a BRep,
    solids: Vec<crate::kernel::SolidId>,
    taken: Vec<(usize, crate::kernel::EdgeId)>,
    circles: Vec<(usize, Point3, Real)>,
}

impl<'a> EdgePoints<'a> {
    fn new(body: &'a BRep) -> Self {
        Self {
            body,
            solids: body.solids().map(|(id, _)| id).collect(),
            taken: Vec::new(),
            circles: Vec::new(),
        }
    }

    fn straight(&self, e: crate::kernel::EdgeId) -> bool {
        matches!(
            self.body.edge(e).and_then(|x| self.body.curve(x.curve)),
            Some(Curve::Line(_))
        )
    }

    fn joined(&self, a: crate::kernel::EdgeId, b: crate::kernel::EdgeId) -> bool {
        let body = self.body;
        let (Some(x), Some(y)) = (body.edge(a), body.edge(b)) else {
            return false;
        };
        let mut fa = body.edge_faces(a);
        let mut fb = body.edge_faces(b);
        fa.sort();
        fb.sort();
        self.straight(a)
            && self.straight(b)
            && fa == fb
            && [x.start, x.end]
                .iter()
                .any(|v| *v == y.start || *v == y.end)
    }

    /// (solid index, point) for each new edge of `refs`.
    fn points(&mut self, refs: &[EdgeRef]) -> Result<Vec<(usize, Point3)>, String> {
        let mut out = Vec::new();
        for r in refs {
            let (i, pts) = edge_points(self.body, &self.solids, r)?;
            for (e, p) in pts {
                if self
                    .taken
                    .iter()
                    .any(|&(j, t)| j == i && (t == e || self.joined(t, e)))
                {
                    continue;
                }
                self.taken.push((i, e));
                let circle = match self.body.edge(e).and_then(|x| self.body.curve(x.curve)) {
                    Some(Curve::Circle(c)) => Some((c.center, c.radius)),
                    _ => None,
                };
                if let Some((c, rad)) = circle {
                    if self.circles.iter().any(|&(j, cc, rr)| {
                        j == i && (cc - c).norm() < 1e-6 && (rr - rad).abs() < 1e-6
                    }) {
                        continue;
                    }
                    self.circles.push((i, c, rad));
                }
                out.push((i, p));
            }
        }
        Ok(out)
    }
}

/// Fillet or Chamfer of the referenced edges. `body` is the B-rep converted
/// from `cur` (same solid order); the edges are found there by their faces'
/// names and passed to the kernel as points on them.
pub(super) fn blend(
    bl: &BlendFeature,
    body: &BRep,
    cur: &[ExactSolid],
) -> Result<Vec<ExactSolid>, Fail> {
    if !bl.has_input() {
        return Err(User("no edges selected".into()));
    }
    if matches!(bl.kind, BlendKind::Fillet { .. })
        && (bl.fillet_type != FilletType::Constant || !bl.sets.is_empty())
    {
        return special_fillet(bl, body, cur);
    }
    let mut points: Vec<Vec<Point3>> = vec![Vec::new(); cur.len()];
    let mut picker = EdgePoints::new(body);
    for (i, p) in picker.points(&bl.edges).map_err(User)? {
        points[i].push(p);
    }
    match kernel_blend(bl, cur, &points) {
        // A size larger than an arc of the loop (a chamfer wider than the
        // fillet before it): the kernel's blend cannot shrink the arc away.
        // No faceted rebuild for a blend the exact kernel cannot make: it
        // took 15–20 s per preview (the UI froze) and could overflow the
        // stack in the BSP Booleans (a crash). The feature reports why.
        Err(Kernel(msg)) => match loop_blend(bl, body, cur) {
            Err(Kernel(e)) => {
                log::debug!("loop blend: {e}");
                wedge_blend(bl, body, cur).map_err(|e| {
                    log::debug!("wedge blend: {e}");
                    User(format!("the geometry kernel cannot build it ({msg})"))
                })
            }
            r => r,
        },
        r => r,
    }
}

/// Fillets beyond one constant set: several sets with their own radii,
/// variable radius, chord width, Full Round (`docs/spec/2.4-fillet.md`).
/// The kernel builds them or the feature reports why (no faceted rebuild).
fn special_fillet(
    bl: &BlendFeature,
    body: &BRep,
    cur: &[ExactSolid],
) -> Result<Vec<ExactSolid>, Fail> {
    let BlendKind::Fillet { radius } = bl.kind else {
        return Err(User("not a fillet".into()));
    };
    let cannot = |e: String| User(format!("the geometry kernel cannot build it ({e})"));
    let mut out = cur.to_vec();
    let mut picker = EdgePoints::new(body);
    // Constant sets: one point and one radius per edge, per solid.
    let constant = |out: &mut Vec<ExactSolid>,
                    picker: &mut EdgePoints,
                    sets: &[(&[EdgeRef], Real)],
                    chord: bool|
     -> Result<(), Fail> {
        let mut pts: Vec<Vec<Point3>> = vec![Vec::new(); cur.len()];
        let mut radii: Vec<Vec<Real>> = vec![Vec::new(); cur.len()];
        for (edges, r) in sets {
            if r.is_nan() || *r <= 0.0 {
                return Err(User("the radius must be positive".into()));
            }
            for (i, p) in picker.points(edges).map_err(User)? {
                pts[i].push(p);
                radii[i].push(*r);
            }
        }
        for i in 0..cur.len() {
            if pts[i].is_empty() {
                continue;
            }
            out[i] = if chord {
                exact::fillet_chord(&out[i], &pts[i], radii[i][0], &bl.id).map_err(cannot)?
            } else {
                exact::fillet_each(&out[i], &pts[i], &radii[i], &bl.id).map_err(cannot)?
            };
        }
        Ok(())
    };
    match bl.fillet_type {
        FilletType::Constant => {
            let mut sets: Vec<(&[EdgeRef], Real)> = vec![(&bl.edges, radius)];
            sets.extend(bl.sets.iter().map(|s| (&s.edges[..], s.radius)));
            constant(&mut out, &mut picker, &sets, false)?;
        }
        FilletType::Chord => constant(&mut out, &mut picker, &[(&bl.edges, radius)], true)?,
        FilletType::FullRound => {
            let center = bl
                .full_round
                .as_deref()
                .ok_or(User("no middle face".into()))?;
            let (edges, r) = super::select::full_round_edges(body, center).map_err(User)?;
            constant(&mut out, &mut picker, &[(&edges, r)], false)?;
        }
        FilletType::Variable => {
            for v in &bl.variable {
                let at = picker.points(std::slice::from_ref(&v.edge)).map_err(User)?;
                let Some(&(i, p)) = at.first() else {
                    continue;
                };
                let pts: Vec<(Real, Real)> = v.points.iter().map(|q| (q[0], q[1])).collect();
                out[i] = exact::fillet_variable(
                    &out[i],
                    p,
                    Point3::from(v.start),
                    &pts,
                    bl.smooth,
                    &bl.id,
                )
                .map_err(cannot)?;
            }
        }
    }
    Ok(out)
}

/// Chamfer of straight edges between perpendicular planar faces from
/// wedge tools, when the kernel's chamfer fails (it cannot close a concave
/// corner where three chamfered edges meet): along every edge a prism of
/// the chamfer's triangle (the bevel line run on past both faces) is cut
/// away at a convex edge and added at a concave one. A prism runs on past
/// an edge's end only where that is harmless (empty space for a cut,
/// material for an addition), so neighbouring prisms meet in mitred corners.
fn wedge_blend(
    bl: &BlendFeature,
    body: &BRep,
    cur: &[ExactSolid],
) -> Result<Vec<ExactSolid>, String> {
    use crate::kernel::{FaceId, Plane, Vector3};
    if !matches!(bl.kind, BlendKind::Chamfer { .. }) {
        return Err("only chamfers".into());
    }
    let solid_index: Vec<crate::kernel::SolidId> = body.solids().map(|(id, _)| id).collect();
    let (d1, d2) = bl.wall_setbacks();
    let mut cuts: Vec<(usize, ExactSolid)> = Vec::new();
    let mut joins: Vec<(usize, ExactSolid)> = Vec::new();
    // Straight runs: pieces of one edge a Boolean split (same faces, a
    // shared vertex) are joined into one, so one prism covers them.
    struct Run {
        k: usize,
        si: usize,
        e: crate::kernel::EdgeId,
        faces: Vec<FaceId>,
        ends: [crate::kernel::VertexId; 2],
    }
    let mut runs: Vec<Run> = Vec::new();
    for (k, r) in bl.edges.iter().enumerate() {
        let (si, pts) = edge_points(body, &solid_index, r)?;
        for (e, _) in pts {
            if runs.iter().any(|x| x.e == e) {
                continue;
            }
            let x = body.edge(e).ok_or("edge")?;
            if !matches!(body.curve(x.curve), Some(Curve::Line(_))) {
                return Err("an edge is not straight".into());
            }
            let mut faces = body.edge_faces(e);
            faces.sort();
            runs.push(Run {
                k,
                si,
                e,
                faces,
                ends: [x.start, x.end],
            });
        }
    }
    loop {
        let pair = (0..runs.len()).find_map(|i| {
            (i + 1..runs.len()).find_map(|j| {
                let (p, q) = (&runs[i], &runs[j]);
                let shared = p.ends.iter().find(|v| q.ends.contains(v)).copied()?;
                (p.si == q.si && p.faces == q.faces).then_some((i, j, shared))
            })
        });
        let Some((i, j, v)) = pair else { break };
        let q = runs.remove(j);
        let p = &mut runs[i];
        let far = |r: [crate::kernel::VertexId; 2]| if r[0] == v { r[1] } else { r[0] };
        p.ends = [far(p.ends), far(q.ends)];
    }
    for Run { k, si, e, ends, .. } in runs {
        let r = &bl.edges[k];
        let solid = cur.get(si).ok_or("no solid")?;
        {
            let (a, b) = (
                body.vertex(ends[0]).ok_or("vertex")?.point,
                body.vertex(ends[1]).ok_or("vertex")?.point,
            );
            let len = (b - a).norm();
            if len < 1e-9 {
                continue;
            }
            let t = (b - a) / len;
            let mid = nalgebra::center(&a, &b);
            // The two faces in the reference's order, and the direction from
            // the edge into each.
            let fs = body.edge_faces(e);
            let face_named = |n: &str| -> Option<FaceId> {
                let m = super::blend::face_matcher(body, n);
                fs.iter()
                    .copied()
                    .find(|&f| m(body.face(f).and_then(|x| x.name.as_deref())))
            };
            let (f1, f2) = (
                face_named(&r.faces[0]).ok_or("face 1")?,
                face_named(&r.faces[1]).ok_or("face 2")?,
            );
            let into = |f: FaceId| -> Result<Vector3, String> {
                let pl = body.face_plane(f).ok_or("a face is not planar")?;
                let u = pl.normal.into_inner().cross(&t).normalize();
                let eps = 1e-3 * len.min(1.0);
                if body.face_contains(f, &(mid + u * eps)) {
                    Ok(u)
                } else if body.face_contains(f, &(mid - u * eps)) {
                    Ok(-u)
                } else {
                    Err("cannot tell the face's side".into())
                }
            };
            let (u1, u2) = (into(f1)?, into(f2)?);
            if u1.dot(&u2).abs() > 1e-6 {
                return Err("the faces are not perpendicular".into());
            }
            // Convex: the material fills the corner between the faces.
            let probe = (d1.min(d2) * 0.25).min(0.1);
            let convex = exact::contains(solid, mid + (u1 + u2) * probe);
            // Run on past an end only where that is harmless: into empty
            // space for a cut, into material for an addition (past an open
            // end it would cut a neighbour / grow a spike).
            // A cut runs on far (its neighbours' corners need the whole
            // bevel); an addition only a little, so that it never reaches
            // through a thin wall or floor to the far side.
            let ext = if convex {
                d1 + d2 + 1.0
            } else {
                0.05 * d1.min(d2).min(1.0)
            };
            let goes_on = |p: Point3, dir: Vector3| {
                let q = p + dir * probe.min(ext) + (u1 + u2) * probe;
                exact::contains(solid, q) != convex
            };
            let back = if goes_on(a, -t) { ext } else { 0.0 };
            let ahead = if goes_on(b, t) { ext } else { 0.0 };
            let mut pair = [
                body.face(f1)
                    .and_then(|x| x.name.clone())
                    .unwrap_or_default(),
                body.face(f2)
                    .and_then(|x| x.name.clone())
                    .unwrap_or_default(),
            ];
            pair.sort();
            let section = chamfer_wedge(d1, d2, &format!("Bevel[{}]", pair.join("|")))?;
            let prof = exact::profile(&section, &[])?;
            let n = u1.cross(&u2);
            // Extrude along n from the end it starts at.
            let (start, l0, l1) = if n.dot(&t) > 0.0 {
                (a, back, ahead)
            } else {
                (b, ahead, back)
            };
            let plane = Plane::new(start - n * l0, n, u1).map_err(|e| e.to_string())?;
            let tool = exact::extrude(
                &plane,
                &prof,
                0.0,
                len + l0 + l1,
                0.0,
                &format!("{}#w{k}", bl.id),
            )?;
            if convex {
                cuts.push((si, tool));
            } else {
                joins.push((si, tool));
            }
        }
    }
    let mut out = cur.to_vec();
    for (si, t) in &cuts {
        out[*si] = exact::boolean(&out[*si], t, ExactOp::Cut)?.ok_or("nothing is left")?;
    }
    for (si, t) in &joins {
        out[*si] = exact::boolean(&out[*si], t, ExactOp::Union)?.ok_or("empty")?;
    }
    // Tool faces `<id>#w<k>:Side:<face>` become `<id>:<face>`.
    let head = format!("{}#w", bl.id);
    for s in &mut out {
        for n in s.face_names.iter_mut().flatten() {
            if let Some(rest) = n.strip_prefix(&head)
                && let Some((_, face)) = rest.split_once(":Side:")
            {
                *n = format!("{}:{face}", bl.id);
            }
        }
    }
    Ok(out)
}

/// The kernel's own fillet / chamfer of the edges through `points` (per
/// solid).
fn kernel_blend(
    bl: &BlendFeature,
    cur: &[ExactSolid],
    points: &[Vec<Point3>],
) -> Result<Vec<ExactSolid>, Fail> {
    let mut out = cur.to_vec();
    for (i, pts) in points.iter().enumerate() {
        if pts.is_empty() {
            continue;
        }
        out[i] = match bl.kind {
            BlendKind::Fillet { radius } => {
                exact::fillet(&cur[i], pts, radius, &bl.id).map_err(Kernel)?
            }
            BlendKind::Chamfer {
                method,
                distance,
                distance2,
                angle,
                flip,
            } => {
                let second = match method {
                    ChamferMethod::Distance => ChamferSecond::Same,
                    ChamferMethod::TwoDistances => ChamferSecond::Distance(distance2),
                    ChamferMethod::DistanceAngle => ChamferSecond::Angle(angle),
                };
                exact::chamfer(&cur[i], pts, distance, second, flip, &bl.id).map_err(Kernel)?
            }
        };
    }
    Ok(out)
}

/// Fillet / Chamfer of the whole outer loop of a planar face ("cap") whose
/// walls are perpendicular to it, the loop convex, built from cutting tools:
/// a wedge along every straight edge, extended past its ends, and a
/// revolved wedge along every arc wider than the blend. An arc the blend
/// does not fit in gets no tool: the wedges of its neighbours meet in a
/// sharp corner, as the loop offset inward by the blend's size does (the
/// arc shrinks to a point). `Err`: not such a loop.
fn loop_blend(bl: &BlendFeature, body: &BRep, cur: &[ExactSolid]) -> Result<Vec<ExactSolid>, Fail> {
    let mut cone_needed = false;
    loop_tools(bl, body, cur, &mut cone_needed).map_err(|e| {
        if cone_needed {
            User(
                "Cone at small rounds is not supported yet for a blend larger than the round"
                    .into(),
            )
        } else {
            Kernel(e)
        }
    })
}

fn loop_tools(
    bl: &BlendFeature,
    body: &BRep,
    cur: &[ExactSolid],
    cone_needed: &mut bool,
) -> Result<Vec<ExactSolid>, String> {
    use crate::kernel::{FaceId, Plane, Vector3};
    let opts = TessellationOptions::default();
    let name_of = |f: FaceId| body.face(f).and_then(|x| x.name.clone());
    // The cap: the face named in every reference.
    let cap_name = bl.edges[0]
        .faces
        .iter()
        .find(|n| bl.edges.iter().all(|r| r.faces.contains(n)))
        .ok_or("the edges do not share a face")?
        .clone();
    let caps: Vec<FaceId> = body
        .faces()
        .filter(|(_, f)| f.name.as_deref() == Some(cap_name.as_str()))
        .map(|(id, _)| id)
        .collect();
    let [cap] = caps[..] else {
        return Err("the cap face is not unique".into());
    };
    // Outward normal of a face from its triangles (counter-clockwise seen
    // from outside).
    let normal_of = |f: FaceId| -> Option<Vector3> {
        let m = body.face_mesh(f, &opts).ok()?;
        let n: Vector3 = m
            .triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| m.positions[i as usize]);
                (b - a).cross(&(c - a))
            })
            .sum();
        (n.norm() > 1e-12).then(|| n.normalize())
    };
    let up = normal_of(cap).ok_or("the cap has no area")?;
    let down = -up;
    if body.face_plane(cap).is_none() {
        return Err("the cap is not planar".into());
    }
    let face = body.face(cap).ok_or("no cap")?;
    let others = |r: &EdgeRef| r.faces.iter().find(|n| **n != cap_name).cloned();
    // Every edge of the cap's outer loop must be in the feature.
    let loop_edges: Vec<crate::kernel::EdgeId> =
        face.outer.coedges.iter().map(|c| c.edge).collect();
    let mut wall_of = Vec::new();
    for &e in &loop_edges {
        let wall = body
            .edge_faces(e)
            .into_iter()
            .find(|&f| f != cap)
            .ok_or("an edge of the loop has one face")?;
        let wn = name_of(wall).unwrap_or_default();
        let covered = bl.edges.iter().any(|r| {
            others(r).is_some_and(|o| super::blend::face_matcher(body, &o)(Some(wn.as_str())))
        });
        if !covered {
            return Err("the edges are not a whole loop".into());
        }
        wall_of.push((wall, wn));
    }
    if loop_edges.len() < bl.edges.len() {
        return Err("edges outside the loop".into());
    }
    // Loop points, for the convexity test.
    let samples: Vec<Point3> = loop_edges
        .iter()
        .filter_map(|&e| body.sample_edge(e, &opts).ok())
        .flatten()
        .collect();
    let size = samples
        .iter()
        .map(|p| (p - samples[0]).norm())
        .fold(0.0, Real::max);
    let (setback, wall_setback) = bl.wall_setbacks();
    let ext = size + setback + 1.0;
    let role = match bl.kind {
        BlendKind::Fillet { .. } => "Round",
        BlendKind::Chamfer { .. } => "Bevel",
    };
    let solid = body
        .solids()
        .position(|(_, x)| x.shells.iter().any(|sh| sh.faces.contains(&cap)))
        .ok_or("the cap has no solid")?;
    let mut tools = Vec::new();
    for (k, &e) in loop_edges.iter().enumerate() {
        let x = body.edge(e).ok_or("edge missing")?;
        let (a, b) = (
            body.vertex(x.start).ok_or("vertex")?.point,
            body.vertex(x.end).ok_or("vertex")?.point,
        );
        let (wall, wn) = &wall_of[k];
        let mut pair = [cap_name.clone(), wn.clone()];
        pair.sort();
        let face_name = format!("{role}[{}]", pair.join("|"));
        let section = match bl.kind {
            // A triangle whose long side is the bevel line, run on past the
            // cap and the wall: no tool face ends on the bevel's edges.
            BlendKind::Chamfer { .. } => chamfer_wedge(setback, wall_setback, &face_name)?,
            BlendKind::Fillet { .. } => bl.wall_section(&face_name)?,
        };
        let prof = exact::profile(&section, &[])?;
        let prefix = format!("{}#t{k}", bl.id);
        match body.curve(x.curve) {
            Some(Curve::Line(_)) => {
                let t = b - a;
                let len = t.norm();
                if len < 1e-9 {
                    continue;
                }
                let t = t / len;
                let wall_n = normal_of(*wall).ok_or("the wall has no area")?;
                if wall_n.dot(&up).abs() > 1e-6 {
                    return Err("a wall is not perpendicular to the cap".into());
                }
                let inward = -(wall_n - t * wall_n.dot(&t)).normalize();
                // Convex: every loop point on the inner side of the edge.
                if samples
                    .iter()
                    .any(|p| (p - a).dot(&inward) < -1e-6 * size.max(1.0))
                {
                    return Err("the loop is not convex".into());
                }
                let n = inward.cross(&down);
                let start = if n.dot(&t) > 0.0 { a } else { b };
                let plane = Plane::new(start - n * ext, n, inward).map_err(|e| e.to_string())?;
                tools.push((
                    true,
                    exact::extrude(&plane, &prof, 0.0, len + 2.0 * ext, 0.0, &prefix)?,
                ));
            }
            Some(Curve::Circle(c)) => {
                let r = c.radius;
                let mid = body
                    .sample_edge(e, &opts)
                    .ok()
                    .and_then(|v| v.get(v.len() / 2).copied())
                    .ok_or("arc samples")?;
                // Convex: the wall's outward normal points away from the axis.
                let wall_n = normal_of(*wall).ok_or("the wall has no area")?;
                if wall_n.dot(&(mid - c.center)) <= 0.0 {
                    return Err("a concave arc in the loop".into());
                }
                if (c.normal.into_inner().cross(&up)).norm() > 1e-6 {
                    return Err("an arc is not parallel to the cap".into());
                }
                if setback >= r - 1e-9 {
                    if !bl.cone_corners {
                        // The neighbours' wedges run on through the corner and
                        // cut the round; they meet in a sharp edge.
                        continue;
                    }
                    *cone_needed = true;
                }
                // The section cut off at the axis (x = r): past the cone's apex
                // the whole corner sector goes, and the neighbours' wedges meet
                // in a sharp corner above it.
                let section = clip_section(&section, r)?;
                let prof = exact::profile(&section, &[])?;
                let sweep = x.range.length().abs();
                let inward = (c.center - a).normalize();
                let n = inward.cross(&down);
                let plane = Plane::new(a, n, inward).map_err(|e| e.to_string())?;
                // Right-handed about `down` from a towards the arc's middle.
                let sign = if (a - c.center).cross(&(mid - c.center)).dot(&down) >= 0.0 {
                    1.0
                } else {
                    -1.0
                };
                tools.push((
                    false,
                    exact::revolve(
                        &plane,
                        &prof,
                        nalgebra::Point2::new(r, 0.0),
                        nalgebra::Vector2::new(0.0, 1.0),
                        0.0,
                        sign * sweep,
                        &prefix,
                    )?,
                ));
            }
            _ => return Err("the loop has an edge that is not a line or an arc".into()),
        }
    }
    let mut out = cur.to_vec();
    let mut s = out[solid].clone();
    // Arcs first: their cones touch the bevel planes along a line, which
    // the Booleans resolve on the original walls but not on a cut plane.
    tools.sort_by_key(|(line, _)| *line);
    let tools: Vec<ExactSolid> = tools.into_iter().map(|(_, t)| t).collect();
    for t in &tools {
        s = exact::boolean(&s, t, ExactOp::Cut)?.ok_or("nothing is left")?;
    }
    // Tool faces `<id>#t<k>:Side:<face>` become `<id>:<face>`.
    let head = format!("{}#t", bl.id);
    for n in s.face_names.iter_mut().flatten() {
        if let Some(rest) = n.strip_prefix(&head)
            && let Some((_, face)) = rest.split_once(":Side:")
        {
            *n = format!("{}:{face}", bl.id);
        }
    }
    out[solid] = s;
    Ok(out)
}

/// A straight-sided section cut to `x <= xmax` (Sutherland-Hodgman); the
/// new side on `x = xmax` is named `Axis`.
fn clip_section(
    section: &[crate::kernel::ProfileSegment],
    xmax: Real,
) -> Result<Vec<crate::kernel::ProfileSegment>, String> {
    use crate::kernel::{ProfileSegment, Segment2};
    use nalgebra::Point2;
    let inside = |p: &Point2<Real>| p.x <= xmax + 1e-12;
    let all_in = section.iter().all(|s| match &s.geometry {
        Segment2::Line { a, b } => inside(a) && inside(b),
        Segment2::Arc { c, r, .. } => c.x + r <= xmax + 1e-12,
        Segment2::Spline { .. } => false,
    });
    if all_in {
        return Ok(section.to_vec());
    }
    let line = |a: Point2<Real>, b: Point2<Real>, name: &str| ProfileSegment {
        geometry: Segment2::Line { a, b },
        name: name.to_owned(),
    };
    let cut = |p: Point2<Real>, q: Point2<Real>| p + (q - p) * ((xmax - p.x) / (q.x - p.x));
    let mut out: Vec<ProfileSegment> = Vec::new();
    let mut exit: Option<Point2<Real>> = None;
    let mut first_entry: Option<usize> = None;
    for s in section {
        let Segment2::Line { a, b } = s.geometry else {
            return Err("cannot cut a curved section at the axis".into());
        };
        match (inside(&a), inside(&b)) {
            (true, true) => out.push(s.clone()),
            (true, false) => {
                let m = cut(a, b);
                out.push(line(a, m, &s.name));
                exit = Some(m);
            }
            (false, true) => {
                let m = cut(a, b);
                match exit.take() {
                    Some(e) => out.push(line(e, m, "Axis")),
                    None => first_entry = Some(out.len()),
                }
                out.push(line(m, b, &s.name));
            }
            (false, false) => {}
        }
    }
    // The loop left the half-plane before its first entry: close over the
    // start.
    if let (Some(e), Some(k)) = (exit, first_entry)
        && let Segment2::Line { a, .. } = out[k].geometry
    {
        out.insert(k, line(e, a, "Axis"));
    }
    if out.len() < 3 {
        return Err("nothing of the section is left".into());
    }
    Ok(out)
}

/// The chamfer's cutting triangle across an edge (x inward along the cap,
/// y down the wall, the edge at the origin): the bevel from (`d1`, 0) to
/// (0, `d2`), extended past both faces, closed outside the body.
fn chamfer_wedge(
    d1: Real,
    d2: Real,
    face: &str,
) -> Result<Vec<crate::kernel::ProfileSegment>, String> {
    use crate::kernel::{ProfileSegment, Segment2};
    use nalgebra::{Point2, Vector2};
    if !(d1 > 0.0 && d2 > 0.0) {
        return Err("the distances must be positive".into());
    }
    let (t1, t2) = (Point2::new(d1, 0.0), Point2::new(0.0, d2));
    let e = 0.1 * (d1 + d2) + 0.01;
    let along: Vector2<Real> = (t1 - t2).normalize();
    let a = t1 + along * e;
    let b = t2 - along * e;
    let c = Point2::new(-e, -e);
    let seg = |a: Point2<Real>, b: Point2<Real>, n: &str| ProfileSegment {
        geometry: Segment2::Line { a, b },
        name: n.to_owned(),
    };
    // Counter-clockwise in (x, y): a → b along the bevel is clockwise here,
    // so go a → c → b → a.
    Ok(vec![seg(a, c, "S0"), seg(c, b, "S1"), seg(b, a, face)])
}

/// The solid (index) an edge reference lies on and points on its edges:
/// the shared edge nearest the hint and, for a circle split in pieces, the
/// other pieces of that circle.
fn edge_points(
    body: &BRep,
    solids: &[crate::kernel::SolidId],
    r: &EdgeRef,
) -> Result<(usize, Vec<(crate::kernel::EdgeId, Point3)>), String> {
    let m0 = super::blend::face_matcher(body, &r.faces[0]);
    let m1 = super::blend::face_matcher(body, &r.faces[1]);
    let named = |f: crate::kernel::FaceId, n: &str| {
        let name = body.face(f).and_then(|x| x.name.as_deref());
        if n == r.faces[0] { m0(name) } else { m1(name) }
    };
    let hint = Point3::from(r.hint);
    let opts = TessellationOptions::default();
    let mut shared = Vec::new();
    for (e, _) in body.edges() {
        let fs = body.edge_faces(e);
        let (Some(a), Some(_)) = (
            fs.iter().copied().find(|&f| named(f, &r.faces[0])),
            fs.iter().copied().find(|&f| named(f, &r.faces[1])),
        ) else {
            continue;
        };
        let Ok(pts) = body.sample_edge(e, &opts) else {
            continue;
        };
        // Distance to the polyline's segments (its points alone tie for a
        // straight edge split in two: both pieces end 2 mm from a hint in
        // the middle of the short one).
        let seg = |p: &Point3, q: &Point3| {
            let d = q - p;
            let l2 = d.norm_squared();
            let s = if l2 > 0.0 {
                ((hint - p).dot(&d) / l2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            (p + d * s - hint).norm()
        };
        let d = if pts.len() == 1 {
            (pts[0] - hint).norm()
        } else {
            pts.windows(2)
                .map(|w| seg(&w[0], &w[1]))
                .fold(Real::INFINITY, Real::min)
        };
        shared.push((d, e, a, pts));
    }
    shared.sort_by(|x, y| x.0.total_cmp(&y.0));
    let Some((_, e0, f0, _)) = shared.first() else {
        return Err("the edge no longer exists".into());
    };
    let solid = solids
        .iter()
        .position(|&s| {
            body.solid(s)
                .is_some_and(|x| x.shells.iter().any(|sh| sh.faces.contains(f0)))
        })
        .ok_or("the edge has no solid")?;
    let circle = |e: crate::kernel::EdgeId| match body.edge(e).and_then(|x| body.curve(x.curve)) {
        Some(Curve::Circle(c)) => Some((c.center, c.radius)),
        _ => None,
    };
    let c0 = circle(*e0);
    // The point halfway along a sampled edge.
    let mid = |pts: &Vec<Point3>| {
        let total: Real = pts.windows(2).map(|w| (w[1] - w[0]).norm()).sum();
        let mut left = total / 2.0;
        for w in pts.windows(2) {
            let l = (w[1] - w[0]).norm();
            if l >= left && l > 0.0 {
                return w[0] + (w[1] - w[0]) * (left / l);
            }
            left -= l;
        }
        pts[pts.len() / 2]
    };
    let pts = shared
        .iter()
        .filter(|(_, e, _, _)| {
            e == e0
                || match (c0, circle(*e)) {
                    (Some((ca, ra)), Some((cb, rb))) => {
                        (ca - cb).norm() < 1e-6 && (ra - rb).abs() < 1e-6
                    }
                    _ => false,
                }
        })
        .filter(|(_, _, _, pts)| !pts.is_empty())
        .map(|(_, e, _, pts)| (*e, mid(pts)))
        .collect();
    Ok((solid, pts))
}

#[cfg(test)]
mod tests {
    use std::f64::consts::PI;

    use super::super::blend::{edge_ref, tangent_chain};
    use super::super::{BlendFeature, EdgeRef, PartDocument};
    use crate::kernel::Real;

    /// The acceptance part (box, eight r5 fillets, a chamfer 2 of the top
    /// loop picked by Automatic Edge Chain) is built exactly: few faces,
    /// exact volume, valid topology, persistent names.
    #[test]
    fn rounded_box_with_chamfered_top_is_exact() {
        let (a, b, h, r, d) = (40.0, 30.0, 20.0, 5.0, 2.0);
        let mut part = PartDocument::with_box("Box1", [a, b, h]).unwrap();
        assert!(part.exact);
        let f = |x: &str, y: &str, hint: [Real; 3]| EdgeRef {
            faces: [format!("Box1:{x}"), format!("Box1:{y}")],
            hint,
        };
        let edges = vec![
            f("Front", "Right", [a, 0.0, h / 2.0]),
            f("Right", "Back", [a, b, h / 2.0]),
            f("Back", "Left", [0.0, b, h / 2.0]),
            f("Left", "Front", [0.0, 0.0, h / 2.0]),
            f("Bottom", "Front", [a / 2.0, 0.0, 0.0]),
            f("Bottom", "Right", [a, b / 2.0, 0.0]),
            f("Bottom", "Back", [a / 2.0, b, 0.0]),
            f("Bottom", "Left", [0.0, b / 2.0, 0.0]),
        ];
        part.add_blend(BlendFeature::fillet(edges, r)).unwrap();
        assert!(part.exact, "{:?}", part.feature_health);
        let top_edge = part
            .body
            .edges()
            .find(|(e, _)| {
                edge_ref(&part.body, *e).is_some_and(|x| {
                    x.faces.contains(&"Box1:Top".to_owned())
                        && x.faces.contains(&"Box1:Front".to_owned())
                })
            })
            .map(|(e, _)| e)
            .unwrap();
        let chain = tangent_chain(&part.body, top_edge);
        assert_eq!(chain.len(), 8, "4 straight parts and 4 arcs: {chain:?}");
        part.add_blend(BlendFeature::chamfer(chain, d)).unwrap();
        assert!(part.exact, "{:?}", part.feature_health);
        assert!(
            part.body.faces().count() < 40,
            "{}",
            part.body.faces().count()
        );
        for (s, _) in part.body.solids() {
            part.body.validate_solid(s).unwrap();
        }
        let v_fillet = a * b * h
            - (4.0 - PI) * r * r * h
            - (1.0 - PI / 4.0) * r * r * 2.0 * (a + b - 4.0 * r)
            - 4.0 * PI * r * r * r / 12.0;
        let removed = d * d / 2.0 * 2.0 * (a + b - 4.0 * r)
            + 4.0 * (d * d / 2.0) * (PI / 2.0) * (r - d / 3.0);
        let mesh = part
            .body
            .tessellate_solid(
                part.body.solids().next().unwrap().0,
                &crate::kernel::TessellationOptions::default(),
            )
            .unwrap();
        let v = mesh_volume(&mesh.faces);
        let exact = v_fillet - removed;
        assert!((v - exact).abs() / exact < 2e-3, "{v} vs {exact}");
    }

    fn mesh_volume(m: &crate::kernel::TriangleMesh) -> Real {
        m.triangles
            .iter()
            .map(|t| {
                let (p, q, r) = (
                    m.positions[t[0] as usize].coords,
                    m.positions[t[1] as usize].coords,
                    m.positions[t[2] as usize].coords,
                );
                p.dot(&q.cross(&r)) / 6.0
            })
            .sum()
    }
}

#[cfg(test)]
mod big_chamfer {
    use super::super::blend::{edge_ref, tangent_chain};
    use super::super::{BlendFeature, EdgeRef, PartDocument};
    use crate::kernel::Real;

    const A: Real = 40.0;
    const B: Real = 30.0;
    const H: Real = 20.0;
    const R: Real = 5.0;

    /// A box with its vertical edges rounded (R) and the top loop chain.
    fn rounded() -> (PartDocument, Vec<EdgeRef>) {
        let mut part = PartDocument::with_box("Box1", [A, B, H]).unwrap();
        let f = |x: &str, y: &str, hint: [Real; 3]| EdgeRef {
            faces: [format!("Box1:{x}"), format!("Box1:{y}")],
            hint,
        };
        part.add_blend(BlendFeature::fillet(
            vec![
                f("Front", "Right", [A, 0.0, H / 2.0]),
                f("Right", "Back", [A, B, H / 2.0]),
                f("Back", "Left", [0.0, B, H / 2.0]),
                f("Left", "Front", [0.0, 0.0, H / 2.0]),
            ],
            R,
        ))
        .unwrap();
        let e = part
            .body
            .edges()
            .find(|(e, _)| {
                edge_ref(&part.body, *e).is_some_and(|x| {
                    x.faces.contains(&"Box1:Top".to_owned())
                        && x.faces.contains(&"Box1:Front".to_owned())
                })
            })
            .map(|(e, _)| e)
            .unwrap();
        let chain = tangent_chain(&part.body, e);
        (part, chain)
    }

    fn volume(part: &PartDocument) -> Real {
        let (sid, _) = part.body.solids().next().unwrap();
        let m = part
            .body
            .tessellate_solid(sid, &crate::kernel::TessellationOptions::default())
            .unwrap();
        m.faces
            .triangles
            .iter()
            .map(|t| {
                let [p, q, s] = t.map(|i| m.faces.positions[i as usize].coords);
                p.dot(&q.cross(&s)) / 6.0
            })
            .sum()
    }

    /// Chamfer d > R without the cone: the bevel planes run on through the
    /// corners, cut the rounds and meet in sharp edges; the top face is a
    /// sharp rectangle.
    #[test]
    fn chamfer_larger_than_the_round_cuts_it() {
        for d in [6.0, 8.0] {
            let (mut part, chain) = rounded();
            part.add_blend(BlendFeature::chamfer(chain, d)).unwrap();
            assert!(part.exact, "{:?}", part.feature_health);
            assert_eq!(part.body.faces().count(), 14);
            for (sid, _) in part.body.solids() {
                part.body.validate_solid(sid).unwrap();
            }
            let names: Vec<String> = part
                .body
                .faces()
                .filter_map(|(_, f)| f.name.clone())
                .collect();
            assert!(
                names.iter().all(|n| !n.contains("#t") && !n.contains(":S")),
                "{names:?}"
            );
            assert!(names.contains(&"Chamfer1:Bevel[Box1:Front|Box1:Top]".to_owned()));
            let top = part.body.find_face("Box1:Top").unwrap();
            let lp = &part.body.face(top).unwrap().outer;
            assert_eq!(lp.coedges.len(), 4, "a sharp rectangle");
            // Cross-section at erosion t: the sharp rectangle (a−2t)×(b−2t)
            // minus what of its corners lies outside the round.
            let deficit = |u: Real| -> Real {
                if u <= 0.0 {
                    return 0.0;
                }
                let n = 400;
                (0..n)
                    .map(|k| {
                        let x = u * (k as Real + 0.5) / n as Real;
                        let y0 = (R * R - x * x).max(0.0).sqrt();
                        (u - y0).max(0.0) * u / n as Real
                    })
                    .sum()
            };
            let n = 2000;
            let mut want = (A * B - (4.0 - std::f64::consts::PI) * R * R) * (H - d);
            for k in 0..n {
                let t = d * (k as Real + 0.5) / n as Real;
                want += ((A - 2.0 * t) * (B - 2.0 * t) - 4.0 * deficit(R - t)) * d / n as Real;
            }
            let v = volume(&part);
            assert!((v - want).abs() / want < 2e-3, "d={d}: {v} vs {want}");
        }
    }

    /// With the cone asked for (not built yet), the feature fails with a
    /// clear reason and the part stays as it was.
    #[test]
    fn cone_at_small_rounds_is_reported() {
        let (mut part, chain) = rounded();
        let before = part.body.faces().count();
        let mut c = BlendFeature::chamfer(chain, 6.0);
        c.cone_corners = true;
        let err = part.add_blend(c).unwrap_err().to_string();
        assert!(err.contains("Cone at small rounds"), "{err}");
        assert_eq!(part.features.len(), 2);
        assert_eq!(part.body.faces().count(), before);
    }
}

#[cfg(test)]
mod hollow_box {
    use super::super::blend::edge_ref;
    use super::super::{
        BlendFeature, EdgeRef, ExtentDirection, ExtrudeFeature, Operation, PartDocument,
        ProfileRef, SketchOptions, SketchPlacement,
    };
    use crate::kernel::{Point3, Real};
    use std::f64::consts::PI;

    const S: Real = 40.0;

    /// A 40 mm cube with a 32×32 pocket 36 deep from the top (4 mm walls).
    fn hollow() -> PartDocument {
        let mut part = PartDocument::with_box("Box1", [S, S, S]).unwrap();
        let opts = SketchOptions {
            auto_project_origin: false,
            ..Default::default()
        };
        let i = part
            .create_sketch(SketchPlacement::on_face("Box1:Top"), &opts)
            .unwrap();
        let plane = part.sketches[i].sketch.plane.clone();
        let a = plane.project(&Point3::new(4.0, 4.0, S));
        let b = plane.project(&Point3::new(36.0, 36.0, S));
        let sk = &mut part.sketches[i].sketch;
        sk.add_rectangle_two_point(a, b).unwrap();
        let r = sk.regions();
        let pocket = r.iter().find(|x| x.holes.is_empty()).unwrap();
        let mut ex =
            ExtrudeFeature::new(vec![ProfileRef::of(pocket, pocket.interior_point())], 36.0);
        ex.direction = ExtentDirection::Flipped;
        ex.operation = Operation::Cut;
        part.add_extrude(i, ex).unwrap();
        part
    }

    fn edges(part: &PartDocument, keep: impl Fn([Real; 3]) -> bool) -> Vec<EdgeRef> {
        part.body
            .edges()
            .filter_map(|(e, _)| edge_ref(&part.body, e))
            .filter(|r| keep(r.hint))
            .collect()
    }

    fn inner(h: [Real; 3]) -> bool {
        let ins = |v: Real| v > 3.0 && v < 37.0;
        h[2] < S - 1e-6 && ins(h[0]) && ins(h[1])
    }

    fn volume(part: &PartDocument) -> Real {
        let (sid, _) = part.body.solids().next().unwrap();
        let m = part
            .body
            .tessellate_solid(sid, &crate::kernel::TessellationOptions::default())
            .unwrap();
        m.faces
            .triangles
            .iter()
            .map(|t| {
                let [p, q, s] = t.map(|i| m.faces.positions[i as usize].coords);
                p.dot(&q.cross(&s)) / 6.0
            })
            .sum()
    }

    /// The 8 inner edges rounded r: the pocket becomes the core square
    /// (a − 2r) grown by a ball of r — exact volume by cross-sections.
    #[test]
    fn inner_fillets_have_the_right_volume() {
        let mut part = hollow();
        assert!(part.exact);
        let e = edges(&part, inner);
        assert_eq!(e.len(), 8);
        let r = 2.0;
        part.add_blend(BlendFeature::fillet(e, r)).unwrap();
        assert!(part.exact);
        let (a, h) = (32.0, 36.0);
        let n = 4000;
        let mut void = (a * a - (4.0 - PI) * r * r) * (h - r);
        for k in 0..n {
            let t = r * (k as Real + 0.5) / n as Real;
            let rho = (r * r - (r - t) * (r - t)).sqrt();
            let c = a - 2.0 * r;
            void += (c * c + 4.0 * c * rho + PI * rho * rho) * r / n as Real;
        }
        let want = S * S * S - void;
        let v = volume(&part);
        assert!((v - want).abs() / want < 2e-3, "{v} vs {want}");
    }

    /// Chamfer d on the 8 inner edges (the kernel cannot close the concave
    /// corners where three meet): wedges added along the edges, mitred.
    /// Pocket section at height t over the floor: (a − 2s)² with s = d − t,
    /// less 4 corner triangles of leg d − 2s where the vertical bevels reach.
    #[test]
    fn inner_chamfers_close_the_corners() {
        let d = 2.0;
        let mut part = hollow();
        let e = edges(&part, inner);
        part.add_blend(BlendFeature::chamfer(e, d)).unwrap();
        assert!(part.exact);
        for (sid, _) in part.body.solids() {
            part.body.validate_solid(sid).unwrap();
        }
        let (a, h) = (32.0, 36.0);
        let n = 4000;
        let mut void = (a * a - 2.0 * d * d) * (h - d);
        for k in 0..n {
            let t = d * (k as Real + 0.5) / n as Real;
            let s = d - t;
            let tri = (d - 2.0 * s).max(0.0);
            void += ((a - 2.0 * s).powi(2) - 4.0 * tri * tri / 2.0) * d / n as Real;
        }
        let want = S * S * S - void;
        let v = volume(&part);
        assert!((v - want).abs() / want < 1e-3, "{v} vs {want}");
        let names: Vec<String> = part
            .body
            .faces()
            .filter_map(|(_, f)| f.name.clone())
            .collect();
        assert!(names.iter().all(|n| !n.contains("#w")), "{names:?}");
    }

    /// The owner's case with a chamfer: outer edges (not the top ones) and
    /// inner edges. Outside, the three convex bevels at a bottom corner meet
    /// like the concave ones inside, so both use one section formula.
    #[test]
    fn outer_and_inner_chamfers_have_the_right_volume() {
        let d = 2.0;
        let mut part = hollow();
        let mut e = edges(&part, |h| {
            let side = |v: Real| v.abs() < 1e-6 || (v - S).abs() < 1e-6;
            h[2] < S - 1e-6 && (side(h[0]) || side(h[1]))
        });
        e.extend(edges(&part, inner));
        part.add_blend(BlendFeature::chamfer(e, d)).unwrap();
        assert!(part.exact);
        for (sid, _) in part.body.solids() {
            part.body.validate_solid(sid).unwrap();
        }
        let solid = |c: Real, h: Real| {
            let n = 4000;
            let mut v = (c * c - 2.0 * d * d) * (h - d);
            for k in 0..n {
                let t = d * (k as Real + 0.5) / n as Real;
                let s = d - t;
                let tri = (d - 2.0 * s).max(0.0);
                v += ((c - 2.0 * s).powi(2) - 2.0 * tri * tri) * d / n as Real;
            }
            v
        };
        let want = solid(S, S) - solid(32.0, 36.0);
        let v = volume(&part);
        assert!((v - want).abs() / want < 1e-3, "{v} vs {want}");
    }

    /// Outer edges (not the top ones) and inner edges together, as the
    /// owner did in the GUI: outer = box with vertical and bottom edges
    /// rounded, minus the rounded pocket.
    #[test]
    fn outer_and_inner_fillets_have_the_right_volume() {
        let mut part = hollow();
        let r = 2.0;
        let mut e = edges(&part, |h| {
            let side = |v: Real| v.abs() < 1e-6 || (v - S).abs() < 1e-6;
            h[2] < S - 1e-6 && (side(h[0]) || side(h[1]))
        });
        e.extend(edges(&part, inner));
        part.add_blend(BlendFeature::fillet(e, r)).unwrap();
        assert!(part.exact, "{:?}", part.feature_health);
        let grown = |c: Real, h: Real| {
            let n = 4000;
            let mut v = (c * c - (4.0 - PI) * r * r) * (h - r);
            for k in 0..n {
                let t = r * (k as Real + 0.5) / n as Real;
                let rho = (r * r - (r - t) * (r - t)).sqrt();
                let core = c - 2.0 * r;
                v += (core * core + 4.0 * core * rho + PI * rho * rho) * r / n as Real;
            }
            v
        };
        let want = grown(S, S) - grown(32.0, 36.0);
        let v = volume(&part);
        assert!((v - want).abs() / want < 2e-3, "{v} vs {want}");
        for (sid, _) in part.body.solids() {
            part.body.validate_solid(sid).unwrap();
        }
    }

    /// Picking the edges one by one (as in the GUI): every step is built
    /// exactly or refused quickly — never the faceted rebuild (15–20 s per
    /// preview, the UI froze, the BSP Booleans could crash).
    #[test]
    fn picking_edges_one_by_one_is_exact_or_refused() {
        let part = hollow();
        let outer = edges(&part, |h| {
            let side = |v: Real| v.abs() < 1e-6 || (v - S).abs() < 1e-6;
            h[2] < S - 1e-6 && (side(h[0]) || side(h[1]))
        });
        let mut picked = outer;
        for r in edges(&part, inner) {
            picked.push(r);
            let mut trial = part.clone();
            let t = std::time::Instant::now();
            let _ = trial.add_blend(BlendFeature::fillet(picked.clone(), 2.0));
            assert!(trial.exact, "fell back to the faceted kernel");
            assert!(t.elapsed().as_secs_f64() < 8.0);
        }
    }
}

#[cfg(test)]
mod edit_feature {
    use super::super::{BlendFeature, EdgeRef, Feature, PartDocument};
    use crate::kernel::Real;

    fn f(x: &str, y: &str, hint: [Real; 3]) -> EdgeRef {
        EdgeRef {
            faces: [format!("Box1:{x}"), format!("Box1:{y}")],
            hint,
        }
    }

    /// Edit Feature: the part rolls back to the feature, the fillet gets new
    /// edges in place; a failing edit leaves the part as it was.
    #[test]
    fn rollback_and_replace_a_fillet() {
        let mut part = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
        part.add_blend(BlendFeature::fillet(
            vec![f("Front", "Right", [40.0, 0.0, 10.0])],
            3.0,
        ))
        .unwrap();
        let filleted = part.body.faces().count();
        assert_eq!(filleted, 7);
        // Rolled back: the box as it was before the fillet.
        part.rollback = Some(1);
        part.regenerate().unwrap();
        assert_eq!(part.body.faces().count(), 6);
        // Two edges instead of one, same id and place.
        let Some(Feature::Blend(old)) = part.features.get(1).cloned() else {
            panic!("no blend")
        };
        let mut def = old.clone();
        def.edges.push(f("Back", "Left", [0.0, 30.0, 10.0]));
        let id = part.replace_feature(1, Feature::Blend(def)).unwrap();
        assert_eq!(id, old.id);
        assert_eq!(part.rollback, None);
        assert_eq!(part.body.faces().count(), 8);
        // Too large a radius: refused, nothing changes.
        let mut bad = old.clone();
        bad.kind = super::super::BlendKind::Fillet { radius: 50.0 };
        assert!(part.replace_feature(1, Feature::Blend(bad)).is_err());
        assert_eq!(part.body.faces().count(), 8);
        let Some(Feature::Blend(now)) = part.features.get(1) else {
            panic!("no blend")
        };
        assert_eq!(now.edges.len(), 2);
    }
}
