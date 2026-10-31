//! Surface bodies (open shells) and the operations that turn them into solids: Patch (a face
//! filling a closed loop of edges — planar, or B-spline patches for a loop that is not — or a
//! planar sketch region), Stitch (surfaces sharing edges sewn
//! into one body, a solid when they close), Trim (a surface cut by a tool, one side kept),
//! Thicken (a surface offset along its normals into a solid), copying faces out of a body, and
//! sheets swept from sketch curves (surface extrude, revolve, sweep and loft).

use std::collections::HashMap;

use mt::{BoundedCurve, InnerSpace, MetricSpace, ParametricCurve, ParametricSurface3D, SearchNearestParameter};
use solidcraft_geom::{Plane, Region2, Seg2, Vec2, Vec3};
use truck_modeling as mt;

use crate::body::{Body, from_p3, p3};
use crate::history::{Change, EdgeOrigin, FaceOrigin, FaceProv};
use crate::record::{self, Rebuild, Side, rebuilt_body};
use crate::{KernelError, Result, guard};

impl Body {
    /// A surface body: some shell does not close.
    pub fn is_surface(&self) -> bool {
        !self.is_mesh() && self.solid.boundaries().iter().any(|s| s.shell_condition() != mt::ShellCondition::Closed)
    }
}

/// A body of faces as they are (no orientation fix: a surface has no inside).
pub(crate) fn surface_body(faces: Vec<mt::Face>) -> Body {
    surface_of(mt::Shell::from(faces))
}

/// A body of a shell as it is.
fn surface_of(shell: mt::Shell) -> Body {
    Body {
        solid: std::sync::Arc::new(mt::Solid::new_unchecked(vec![shell])),
        size: 0.0,
        volume: Some(std::sync::Arc::new(std::sync::Mutex::new(None))),
        coarse_missing: None,
        mesh: None,
        color: None,
        paint: None,
        splits: None,
        face_sources: None,
        history: None,
        regularization_protected: false,
    }
}

/// A body from faces: a solid when they close, else a surface; with the history of the
/// operation that made the faces from `operands` as `rec` records.
fn body_of(faces: Vec<mt::Face>, operands: &[&mt::Solid], rec: &Rebuild) -> Result<Body> {
    let shell = mt::Shell::from(faces);
    if shell.shell_condition() == mt::ShellCondition::Closed {
        crate::record::rebuilt(mt::Solid::new_unchecked(vec![shell]), operands, rec)
    } else {
        Ok(rebuilt_body(surface_of(shell), operands, rec))
    }
}

/// A planar surface filling a sketch region. Its history has no operands: the face is new.
pub fn patch_region(plane: &Plane, region: &Region2) -> Result<Body> {
    let f = crate::build::face(plane, region)?;
    let mut rec = Rebuild::default();
    rec.made(&f, Vec::new());
    Ok(rebuilt_body(surface_body(vec![f]), &[], &rec))
}

/// A chain's edge seeds numbered across all the chains: the segment at `i` of the chain that
/// starts at segment `offset` is segment `offset + i`.
fn numbered(seeds: &[(mt::EdgeID, usize)], offset: usize) -> Vec<(mt::EdgeID, Vec<usize>)> {
    seeds.iter().map(|(id, i)| (*id, vec![offset + i])).collect()
}

/// The faces truck swept from the edges `seeds`, each with the segments under the edge whose
/// column it lies in (none where the faces don't follow the pattern; see [`record::follow`]).
fn swept_sides(shell: &mt::Shell, seeds: &[(mt::EdgeID, Vec<usize>)], fixed: &[mt::VertexID]) -> Vec<Side> {
    let provs = record::columns(shell.face_iter(), seeds, None, record::Column::ToEnd, fixed);
    shell.face_iter().enumerate().map(|(k, f)| (f.clone(), provs.as_ref().and_then(|p| p.get(k).cloned().flatten()))).collect()
}

/// A body of a sheet builder's faces: a surface, or a solid where they close; with the history
/// of a body built from the chains' segments (none unless every face's is known).
fn sheet_body(sides: Vec<Side>) -> Result<Body> {
    let history: Option<Vec<FaceProv>> = sides.iter().map(|(_, p)| p.clone()).collect();
    let shell = mt::Shell::from(sides.into_iter().map(|(f, _)| f).collect::<Vec<_>>());
    if shell.shell_condition() == mt::ShellCondition::Closed {
        return match crate::body::Solid::try_new(vec![shell.clone()]) {
            Ok(solid) => record::attach(Body::new(solid)?, &[], history, Default::default()),
            Err(_) => crate::record::rebuilt(mt::Solid::new_unchecked(vec![shell]), &[], &Rebuild::default()),
        };
    }
    record::attach(surface_of(shell), &[], history, Default::default())
}

/// Most curve chains one surface extrude takes.
const MAX_CHAINS: usize = 10_000;

/// A surface extruded from chains of sketch segments on `plane`: each chain (open, or closed
/// such as a profile's boundary) moved straight from `start` to `end` along the plane normal
/// (offsets in mm). The walls it sweeps form one surface body with no caps. Its history has no
/// operands: every face is new.
pub fn extrude_sheet(plane: &Plane, chains: &[Vec<Seg2>], start: f64, end: f64) -> Result<Body> {
    if !crate::build::frame_ok(plane) {
        return Err(KernelError::Invalid("extrude frame is not finite and orthonormal".into()));
    }
    if !(start.is_finite() && end.is_finite() && start.abs() < 1e7 && end.abs() < 1e7) {
        return Err(KernelError::Invalid("extrude distance is not finite".into()));
    }
    let (lo, hi) = if end >= start { (start, end) } else { (end, start) };
    if hi - lo < 1e-6 {
        return Err(KernelError::Invalid("extrude distance is zero".into()));
    }
    if chains.is_empty() || chains.len() > MAX_CHAINS {
        return Err(KernelError::Invalid("pick the curves or profiles to extrude".into()));
    }
    let base = plane.offset(lo);
    let along = crate::body::v3(plane.normal() * (hi - lo));
    let mut sides = Vec::new();
    let mut offset = 0;
    for chain in chains {
        let (wire, seeds) = crate::build::chain_wire_seeds(&base, chain)?;
        let walls: mt::Shell = guard("surface extrude", || Ok(mt::builder::tsweep(&wire, along)))?;
        sides.extend(swept_sides(&walls, &numbered(&seeds, offset), &[]));
        offset += chain.len();
    }
    sheet_body(sides)
}

/// A surface revolved from chains of sketch segments on `plane` about the sketch line through
/// `axis_origin` along `axis_dir`: each chain turned from `start` through `sweep` radians (a
/// full turn at most), positive angles carrying it toward the sketch normal. A chain end on the
/// axis stays put there (a cone's apex, a sphere's pole). The walls form one surface body with
/// no caps, or a solid where they close (a full turn of a closed curve, or of a curve from the
/// axis back to it); its history has no operands: every face is new.
pub fn revolve_sheet(plane: &Plane, chains: &[Vec<Seg2>], axis_origin: Vec2, axis_dir: Vec2, start: f64, sweep: f64) -> Result<Body> {
    use std::f64::consts::TAU;
    if !crate::build::frame_ok(plane) || !axis_origin.is_finite() || !axis_dir.is_finite() {
        return Err(KernelError::Invalid("revolve frame is not finite and orthonormal".into()));
    }
    if !(start.is_finite() && sweep.is_finite() && start.abs() <= TAU && sweep.abs() <= TAU + 1e-9) {
        return Err(KernelError::Invalid("revolve angles must be finite, a full turn at most".into()));
    }
    if sweep.abs() < 1e-6 {
        return Err(KernelError::Invalid("revolve angle is zero".into()));
    }
    if chains.is_empty() || chains.len() > MAX_CHAINS {
        return Err(KernelError::Invalid("pick the curves or profiles to revolve".into()));
    }
    let d = axis_dir.normalized().ok_or_else(|| KernelError::Invalid("revolve axis has no direction".into()))?;
    let side = |p: Vec2| d.cross(p - axis_origin);
    let on_axis = |p: Vec2| side(p).abs() < 1e-7;
    let (origin, n) = (plane.to_world(axis_origin), plane.normal());
    let axis = plane.dir_to_world(d).normalized().ok_or_else(|| KernelError::Invalid("revolve frame has no axis".into()))?;
    let mut sides = Vec::new();
    let mut offset = 0;
    for chain in chains {
        // On one side of the axis; the point farthest from it fixes the turning sense.
        let pts: Vec<Vec2> = chain.iter().flat_map(|s| s.polyline(1e-3)).collect();
        if pts.iter().any(|p| side(*p) < -1e-7) && pts.iter().any(|p| side(*p) > 1e-7) {
            return Err(KernelError::Invalid("the curves cross the revolve axis".into()));
        }
        let far = pts.iter().copied().max_by(|a, b| side(*a).abs().total_cmp(&side(*b).abs())).filter(|p| !on_axis(*p));
        let Some(far) = far else { return Err(KernelError::Invalid("the curves lie on the revolve axis".into())) };
        let radial = plane.to_world(far) - origin;
        let a = if axis.cross(radial).dot(n) < 0.0 { -axis } else { axis };
        let (first, last) = match (chain.first(), chain.last()) {
            (Some(f), Some(l)) => (f.start(), l.end()),
            _ => return Err(KernelError::Invalid("an empty curve chain".into())),
        };
        let closed = first.dist(last) <= 1e-6;
        if closed && pts.iter().any(|p| on_axis(*p)) {
            return Err(KernelError::Invalid("not supported yet: a closed curve touching the revolve axis".into()));
        }
        // The chain starts `start` round: its sketch frame turned there, so the wire's own edges
        // are the ones swept (and its faces can be followed back to them).
        let turn = |v: Vec3| crate::build::rotate_about(v, Vec3::ZERO, a, start);
        let turned = Plane { origin: crate::build::rotate_about(plane.origin, origin, a, start), x: turn(plane.x), y: turn(plane.y) };
        let (wire, seeds) = crate::build::chain_wire_seeds(&turned, chain)?;
        let fixed: Vec<mt::VertexID> = [(on_axis(first), wire.front_vertex()), (on_axis(last), wire.back_vertex())]
            .into_iter()
            .filter_map(|(apex, v)| v.filter(|_| apex).map(|v| v.id()))
            .collect();
        let walls: mt::Shell = guard("surface revolve", || {
            Ok(match (on_axis(first), on_axis(last)) {
                (true, _) => mt::builder::cone(&wire, crate::body::v3(a), mt::Rad(sweep)),
                (false, true) => mt::builder::cone(&wire.inverse(), crate::body::v3(a), mt::Rad(sweep)),
                (false, false) => mt::builder::rsweep(&wire, p3(origin), crate::body::v3(a), mt::Rad(sweep)),
            })
        })?;
        sides.extend(swept_sides(&walls, &numbered(&seeds, offset), &fixed));
        offset += chain.len();
    }
    sheet_body(sides)
}

/// A surface swept from chains of sketch segments on `plane` along a path that starts near
/// them. Along a path whose direction runs on across every joint, lines slide the chains and
/// arcs turn them, exactly; any other path (curves followed as polylines) goes through
/// [`sweep_sheet_points`]. The walls form one surface body with no caps (a solid where they
/// close); its history has no operands: every face is new.
pub fn sweep_sheet(plane: &Plane, chains: &[Vec<Seg2>], path: &[crate::PathSeg]) -> Result<Body> {
    use crate::PathSeg;
    if !crate::build::frame_ok(plane) {
        return Err(KernelError::Invalid("sweep frame is not finite and orthonormal".into()));
    }
    if chains.is_empty() || chains.len() > MAX_CHAINS {
        return Err(KernelError::Invalid("pick the curves or profiles to sweep".into()));
    }
    if path.is_empty() || path.len() > 1000 {
        return Err(KernelError::Invalid("the path needs 1…1000 segments".into()));
    }
    let ends: Vec<(Vec3, Vec3)> =
        path.iter().map(PathSeg::tangents).collect::<Option<_>>().ok_or_else(|| KernelError::Invalid("a path segment has no direction".into()))?;
    let first = match path.first() {
        Some(PathSeg::Line { a, .. } | PathSeg::Arc { a, .. }) => *a,
        None => return Err(KernelError::Invalid("the path needs 1…1000 segments".into())),
    };
    if ends.windows(2).any(|w| w[0].1.dot(w[1].0) < (1e-3f64).cos()) {
        let mut pts = vec![first];
        for seg in path {
            pts.extend(seg.points_after_start(1e-2));
        }
        return sweep_sheet_points(plane, chains, &pts);
    }
    let mut sides = Vec::new();
    let mut offset = 0;
    for chain in chains {
        let (mut cur, seeds) = crate::build::chain_wire_seeds(plane, chain)?;
        // Each edge's segments: the next stretch of the path starts from the edges the last one
        // carried them onto, in the same order.
        let mut seeds = numbered(&seeds, offset);
        for seg in path {
            let (part, map): (mt::Shell, Box<dyn Fn(Vec3) -> Vec3>) = match *seg {
                PathSeg::Line { a, b } => {
                    (guard("surface sweep", || Ok(mt::builder::tsweep(&cur, crate::body::v3(b - a))))?, Box::new(move |p| p + (b - a)))
                }
                PathSeg::Arc { center, axis, angle, .. } => {
                    let k = axis.normalized().ok_or_else(|| KernelError::Invalid("a path arc has no axis".into()))?;
                    let part = guard("surface sweep", || Ok(mt::builder::rsweep(&cur, p3(center), crate::body::v3(k), mt::Rad(angle))))?;
                    (part, Box::new(move |p| crate::build::rotate_about(p, center, k, angle)))
                }
            };
            let next = far_wire(&part, &cur, map.as_ref())?;
            sides.extend(swept_sides(&part, &seeds, &[]));
            seeds = next.edge_iter().map(|e| e.id()).zip(seeds.into_iter().map(|(_, n)| n)).collect();
            cur = next;
        }
        offset += chain.len();
    }
    sheet_body(sides)
}

/// A surface swept from chains of sketch segments on `plane` along a 3D polyline path: one smooth
/// B-spline wall per segment where the path has no corners, else ruled walls between the path's
/// points, mitred at the corners (see [`sweep_sheet`]).
pub fn sweep_sheet_points(plane: &Plane, chains: &[Vec<Seg2>], path: &[Vec3]) -> Result<Body> {
    if !crate::build::frame_ok(plane) {
        return Err(KernelError::Invalid("sweep frame is not finite and orthonormal".into()));
    }
    if chains.is_empty() || chains.len() > MAX_CHAINS {
        return Err(KernelError::Invalid("pick the curves or profiles to sweep".into()));
    }
    let ruled = sheet_body(crate::sweep_path::ruled_sheet(plane, chains, path)?)?;
    // Smooth walls where the path has no corners, kept when their area agrees with the ruled
    // walls' (a fit that went wrong falls back to those).
    if let Ok(Some(sides)) = crate::sweep_path::smooth_sheet(plane, chains, path)
        && let Ok(smooth) = sheet_body(sides)
        && let (Ok(a), Ok(b)) = (crate::measure(&smooth), crate::measure(&ruled))
        && (a.area - b.area).abs() <= 0.02 * b.area.abs()
    {
        return Ok(smooth);
    }
    Ok(ruled)
}

/// The far side of a sweep of the wire `start`: for each of its edges, in order and the same
/// way round, the edge of `swept` that `map` carries it onto (matched by its ends and middle).
fn far_wire(swept: &mt::Shell, start: &mt::Wire, map: &dyn Fn(Vec3) -> Vec3) -> Result<mt::Wire> {
    let near = |p: Vec3, q: Vec3| p.dist(q) <= 1e-6 * (1.0 + p.len().max(q.len()));
    let mid = |e: &mt::Edge| {
        let c = e.oriented_curve();
        let (t0, t1) = c.range_tuple();
        from_p3(c.subs((t0 + t1) / 2.0))
    };
    let mut out = Vec::new();
    for e in start.edge_iter() {
        let (a, b, m) = (map(from_p3(e.front().point())), map(from_p3(e.back().point())), map(mid(e)));
        let found = swept.edge_iter().find_map(|f| {
            let (fa, fb) = (from_p3(f.front().point()), from_p3(f.back().point()));
            if !near(mid(&f), m) {
                None
            } else if near(fa, a) && near(fb, b) {
                Some(f)
            } else if near(fa, b) && near(fb, a) {
                Some(f.inverse())
            } else {
                None
            }
        });
        out.push(found.ok_or_else(|| KernelError::Failed("sweep: lost the curves along the path".into()))?);
    }
    Ok(out.into())
}

/// A surface lofted through two or more sections, each a chain of sketch segments on its plane,
/// all open or all closed: ruled between two sections, one smooth face per segment through
/// three or more. Closed sections are matched as the solid loft matches them; open ones are
/// split to the same number of segments (longest first) and each turned to run the way the one
/// before it does. Walls only, no caps; the history has no operands: every face is new.
pub fn loft_sheet(sections: &[(Plane, Vec<Seg2>)]) -> Result<Body> {
    if sections.len() < 2 || sections.len() > 50 {
        return Err(KernelError::Invalid("a loft needs 2…50 sections".into()));
    }
    let closed: Vec<bool> = sections
        .iter()
        .map(|(_, c)| match (c.first(), c.last()) {
            (Some(f), Some(l)) if c.len() <= 4096 => Ok(l.end().dist(f.start()) <= 1e-6),
            _ => Err(KernelError::Invalid("a loft section has no curves, or too many".into())),
        })
        .collect::<Result<_>>()?;
    let all_closed = closed.iter().all(|c| *c);
    if !all_closed && closed.iter().any(|c| *c) {
        return Err(KernelError::Invalid("the loft sections must be all open or all closed".into()));
    }
    for (plane, chain) in sections {
        // Checks the frame, the numbers and that each chain runs end to end.
        if !crate::build::frame_ok(plane) {
            return Err(KernelError::Invalid("a loft section's frame is not finite and orthonormal".into()));
        }
        crate::build::chain_wire(plane, chain)?;
    }
    let (wires, sources) = if all_closed {
        let loops: Vec<(Plane, solidcraft_geom::Loop2)> = sections.iter().map(|(p, c)| (*p, solidcraft_geom::Loop2 { segs: c.clone() })).collect();
        let (wires, sources, _) = crate::build::loft_wires(&loops)?;
        (wires, sources)
    } else {
        let n = sections.iter().map(|(_, c)| c.len()).max().unwrap_or(0);
        let mut wires = Vec::with_capacity(sections.len());
        let mut sources = Vec::with_capacity(sections.len());
        let mut prev: Option<(Vec3, Vec3)> = None;
        // Segments are numbered through the sections in the order given.
        let mut offset = 0;
        for (plane, chain) in sections {
            // Each piece with the segment it is (part of).
            let mut segs: Vec<(Seg2, usize)> = chain.iter().enumerate().map(|(i, s)| (*s, offset + i)).collect();
            offset += chain.len();
            // Split the longest segment in half until the counts match.
            while segs.len() < n {
                let Some(i) = segs.iter().enumerate().max_by(|a, b| a.1.0.length().total_cmp(&b.1.0.length())).map(|(i, _)| i) else { break };
                let (s, k) = segs[i];
                let (a, b) = s.split_half();
                segs.splice(i..=i, [(a, k), (b, k)]);
            }
            let ends = |c: &[(Seg2, usize)]| c.first().zip(c.last()).map(|(f, l)| (plane.to_world(f.0.start()), plane.to_world(l.0.end())));
            if let (Some((ps, pe)), Some((s, e))) = (prev, ends(&segs))
                && s.dist(ps) + e.dist(pe) > s.dist(pe) + e.dist(ps)
            {
                segs = segs.iter().rev().map(|(s, k)| (s.reversed(), *k)).collect();
            }
            prev = ends(&segs);
            let pieces: Vec<Seg2> = segs.iter().map(|(s, _)| *s).collect();
            wires.push(crate::build::uniform_chain_wire(plane, &pieces, false)?);
            sources.push(segs.iter().map(|(_, k)| vec![*k]).collect::<Vec<_>>());
        }
        (wires, sources)
    };
    let faces = guard("surface loft", || {
        let smooth = if wires.len() >= 3 { crate::build::smooth_loft_faces(&wires, &sources, all_closed).ok() } else { None };
        match smooth {
            Some(f) => Ok(f),
            None => crate::build::ruled_loft_faces(&wires, &sources),
        }
    })?;
    sheet_body(faces)
}

/// The edge of a body nearest a point (by samples along each edge).
pub(crate) fn edge_near(body: &Body, p: Vec3) -> Option<mt::Edge> {
    let mut best: Option<(f64, mt::Edge)> = None;
    for e in body.solid.edge_iter() {
        let c = e.curve();
        let (t0, t1) = c.range_tuple();
        for k in 0..=32 {
            let d = from_p3(c.subs(t0 + (t1 - t0) * k as f64 / 32.0)).dist(p);
            if best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                best = Some((d, e.clone()));
            }
        }
    }
    best.filter(|(d, _)| *d <= body.size() * 0.05 + 1e-6).map(|(_, e)| e)
}

/// A surface filling a closed loop of a body's edges (each picked by a point on it): one face on
/// the body's own edges, planar when the loop is certainly planar ([`crate::plane_fit`]), else a
/// B-spline height field over the loop's plane ([`crate::fill`]; loops that do not bound one are
/// refused). Its history is over `[body]`: the face is new, its edges the body's own.
pub fn patch_edges(body: &Body, at: &[Vec3]) -> Result<Body> {
    patch_edges_tangent(body, at, None)
}

/// [`patch_edges`] with the Tangent condition (`Some(weight)`): the patch leaves every edge of
/// the loop going on along the face it bounds (G1), from a Coons patch with cubic Hermite blends
/// whose cross-boundary derivatives are those faces' directions past their edges (`weight` × the
/// width across). Only a loop of four sides (four edges, or four runs between corners) for now.
/// (3DCadCraft addition.)
pub fn patch_edges_tangent(body: &Body, at: &[Vec3], tangent: Option<f64>) -> Result<Body> {
    body.require_brep("patch")?;
    if let Some(w) = tangent {
        if at.is_empty() || at.len() > 10_000 {
            return Err(KernelError::Invalid("pick the edges of a closed loop".into()));
        }
        return guard("patch", || {
            let chain = picked_loop(body, at)?;
            four_sided_patch_with(body, &chain, Some(w))
        });
    }
    if at.is_empty() || at.len() > 10_000 {
        return Err(KernelError::Invalid("pick the edges of a closed loop".into()));
    }
    guard("patch", || {
        let chain = picked_loop(body, at)?;
        let Some(face) = crate::plane_fit::planar_face(&chain, crate::fill::planarity_tolerance(body))? else {
            // Not certainly planar: a fitted surface; where the fit refuses the loop (a hole cut
            // out of a wave folds over its best-fit plane), a four-sided loop is filled from its
            // four sides (3DCadCraft addition).
            return crate::fill::fill_loop(body, &chain).or_else(|e| four_sided_patch(body, &chain).map_err(|_| e));
        };
        // A new face on the body's own edges.
        let mut rec = Rebuild::default();
        rec.made(&face, Vec::new());
        Ok(rebuilt_body(surface_body(vec![face]), &[&body.solid], &rec))
    })
}

/// A surface bridging two open chains of edges (of `a` and of `b`, each edge picked by a point on
/// it): the ruled surface between them — each chain interpolated as a cubic B-spline through
/// points along it by chord length, chain B run the way that pairs its ends with A's nearest,
/// the two joined by straight lines at the ends (G0 to both). Its edges are its own (within the
/// interpolation's error of the chains). Its history is over `[a, b]`: the face is new.
/// (3DCadCraft addition: Loft between surface edges.)
pub fn bridge_edges(a: &Body, at_a: &[Vec3], b: &Body, at_b: &[Vec3]) -> Result<Body> {
    bridge_edges_tangent(a, at_a, b, at_b, [None, None])
}

/// [`bridge_edges`] with the Tangent condition at either section: `weights[0]` (A) and
/// `weights[1]` (B), `Some(w)` = the bridge leaves that section's edges tangent to its surface
/// (G1), reaching on `w` times a third of the distance between the sections before it turns
/// (a cubic across: the rows A, A + reach, B + reach, B); `None` = G0 there. The ends across are
/// then the surface's own boundary curves (a whole boundary's corners share theirs, the
/// directions of both sides averaged there). (3DCadCraft addition: Loft conditions.)
pub fn bridge_edges_tangent(a: &Body, at_a: &[Vec3], b: &Body, at_b: &[Vec3], weights: [Option<f64>; 2]) -> Result<Body> {
    a.require_brep("bridge")?;
    b.require_brep("bridge")?;
    if at_a.is_empty() || at_b.is_empty() || at_a.len() + at_b.len() > 10_000 {
        return Err(KernelError::Invalid("pick the edges of both sections".into()));
    }
    guard("bridge", || {
        // Whole boundaries (closed loops): one bridge per pair of sides.
        if let (Some(la), Some(lb)) = (all_loops(a, at_a)?, all_loops(b, at_b)?) {
            // The outer boundaries, and the holes paired nearest (when both surfaces have as
            // many): a solid through the holes (3DCadCraft addition).
            let mut faces = bridge_loops(a, &la[0], b, &lb[0], weights)?;
            // With different numbers of holes, as many pairs as the fewer has (each with the
            // nearest hole of the other); the rest stay open.
            if la.len() > 1 && lb.len() > 1 {
                let centre = |lp: &[mt::Edge]| chain_points(lp).map(|p| centroid(&p)).unwrap_or(Vec3::new(0.0, 0.0, 0.0));
                let a_fewer = la.len() <= lb.len();
                let (few, many) = if a_fewer { (&la[1..], &lb[1..]) } else { (&lb[1..], &la[1..]) };
                let mut free: Vec<usize> = (0..many.len()).collect();
                for h in few {
                    let c = centre(h);
                    let Some(k) = (0..free.len()).min_by(|x, y| c.dist(centre(&many[free[*x]])).total_cmp(&c.dist(centre(&many[free[*y]])))) else { break };
                    let other = &many[free.remove(k)];
                    let (ha, hb) = if a_fewer { (h, other) } else { (other, h) };
                    faces.extend(bridge_loops(a, ha, b, hb, weights)?);
                }
            }
            let mut rec = Rebuild::default();
            for f in &faces {
                rec.made(f, Vec::new());
            }
            return Ok(rebuilt_body(surface_body(faces), &[&a.solid, &b.solid], &rec));
        }
        // Open chains: one bridge on the chains' own edges (as the sides of whole boundaries).
        let ea = picked_chain(a, at_a)?;
        let mut eb = picked_chain(b, at_b)?;
        let end = |c: &[mt::Edge], last: bool| -> Result<Vec3> {
            let e = if last { c.last() } else { c.first() };
            e.map(|e| from_p3(if last { e.back().point() } else { e.front().point() }))
                .ok_or_else(|| KernelError::Invalid("a section has no edges".into()))
        };
        let (a0, a1) = (end(&ea, false)?, end(&ea, true)?);
        let (b0, b1) = (end(&eb, false)?, end(&eb, true)?);
        if a0.dist(b1) + a1.dist(b0) < a0.dist(b0) + a1.dist(b1) {
            eb = eb.iter().rev().map(mt::Edge::inverse).collect();
        }
        let (b0, b1) = (end(&eb, false)?, end(&eb, true)?);
        let tol = solidcraft_geom::LINEAR_TOL;
        if a0.dist(b0) <= tol || a1.dist(b1) <= tol {
            return Err(KernelError::Invalid("the sections touch: use Boundary Patch".into()));
        }
        let corner = |c: &[mt::Edge], last: bool| -> Result<mt::Vertex> {
            let e = if last { c.last() } else { c.first() };
            e.map(|e| if last { e.back().clone() } else { e.front().clone() })
                .ok_or_else(|| KernelError::Invalid("a section has no edges".into()))
        };
        let (va0, va1, vb0, vb1) = (corner(&ea, false)?, corner(&ea, true)?, corner(&eb, false)?, corner(&eb, true)?);
        let (qa, qb) = (side_points(&ea)?, side_points(&eb)?);
        let (surface, start, end) = if weights.iter().any(Option::is_some) {
            let reach = Reach::new(a, &ea, &qa, b, &eb, &qb, weights)?;
            let surface = cubic_facing(&qa, &reach.a, &qb, &reach.b)?;
            let (la, lb) = (qa.len() - 1, qb.len() - 1);
            let start = across_edge(&va0, &vb0, reach.a[0], reach.b[0])?;
            let end = across_edge(&va1, &vb1, reach.a[la], reach.b[lb])?;
            (surface, start, end)
        } else {
            (ruled_facing(&qa, &qb)?, mt::builder::line(&va0, &vb0), mt::builder::line(&va1, &vb1))
        };
        // Along A, up the end line, back along B, down the start line.
        let mut wire: Vec<mt::Edge> = ea.clone();
        wire.push(end);
        wire.extend(eb.iter().rev().map(mt::Edge::inverse));
        wire.push(start.inverse());
        let face = mt::Face::try_new(vec![wire.into()], mt::Surface::BSplineSurface(surface))
            .map_err(|e| KernelError::Failed(format!("bridge: {e}")))?;
        let mut rec = Rebuild::default();
        rec.made(&face, Vec::new());
        Ok(rebuilt_body(surface_body(vec![face]), &[&a.solid, &b.solid], &rec))
    })
}

/// The picked edges chained into closed loops, the longest first; `None` when they do not all
/// close (an open chain).
fn all_loops(body: &Body, at: &[Vec3]) -> Result<Option<Vec<Vec<mt::Edge>>>> {
    let mut pool: Vec<mt::Edge> = Vec::new();
    for p in at {
        let e = edge_near(body, *p).ok_or_else(|| KernelError::Invalid(format!("no edge at {p:?}")))?;
        if !pool.iter().any(|x| x.id() == e.id()) {
            pool.push(e);
        }
    }
    let mut loops: Vec<Vec<mt::Edge>> = Vec::new();
    while let Some(first) = pool.pop() {
        let start = first.front().id();
        let mut at_v = first.back().clone();
        let mut chain = vec![first];
        while at_v.id() != start {
            let Some(k) = pool.iter().position(|e| e.front().id() == at_v.id() || e.back().id() == at_v.id()) else {
                return Ok(None);
            };
            let e = pool.remove(k);
            let e = if e.front().id() == at_v.id() { e } else { e.inverse() };
            at_v = e.back().clone();
            chain.push(e);
        }
        loops.push(chain);
    }
    let len = |c: &Vec<mt::Edge>| chain_points(c).map(|p| p.windows(2).map(|w| w[0].dist(w[1])).sum::<f64>()).unwrap_or(0.0);
    loops.sort_by(|x, y| len(y).total_cmp(&len(x)));
    Ok((!loops.is_empty()).then_some(loops))
}

/// The direction an edge runs at its start (`end`: at its end), from two close points.
fn edge_direction(e: &mt::Edge, end: bool) -> Option<Vec3> {
    let mut s = crate::plane_fit::EdgeSampler::new(e)?;
    let (p, q) = if end { (s.at(1.0 - 1e-3)?, s.at(1.0)?) } else { (s.at(0.0)?, s.at(1e-3)?) };
    (q - p).normalized()
}

/// A closed loop of edges cut into sides where it turns sharply (more than 10°; smooth joints of
/// split edges turn about 1°); the first side
/// starts at a corner. None for a loop without corners (a circle).
fn loop_sides(lp: &[mt::Edge]) -> Option<Vec<Vec<mt::Edge>>> {
    let n = lp.len();
    let corner = |k: usize| -> bool {
        // Between edge k - 1 and edge k.
        let prev = &lp[(k + n - 1) % n];
        match (edge_direction(prev, true), edge_direction(&lp[k], false)) {
            (Some(a), Some(b)) => a.dot(b) < (10f64).to_radians().cos(),
            _ => true,
        }
    };
    let first = (0..n).find(|&k| corner(k))?;
    let mut sides: Vec<Vec<mt::Edge>> = Vec::new();
    for i in 0..n {
        let k = (first + i) % n;
        if i == 0 || corner(k) {
            sides.push(Vec::new());
        }
        if let Some(last) = sides.last_mut() {
            last.push(lp[k].clone());
        }
    }
    Some(sides)
}

/// One bridge per pair of sides of two closed boundaries (each cut at its corners, B run and
/// turned the way that pairs the corners nearest A's): the faces share their corner lines, so
/// they come out as one surface body.
fn bridge_loops(a: &Body, la: &[mt::Edge], b: &Body, lb: &[mt::Edge], weights: [Option<f64>; 2]) -> Result<Vec<mt::Face>> {
    let none = || KernelError::Invalid("not supported yet: a closed boundary of one edge as a loft section".into());
    // A smooth boundary (a circle) has no corners: its edges' joints serve as corners, so it
    // pairs with the other boundary's corners (3DCadCraft addition).
    let sides = |lp: &[mt::Edge]| loop_sides(lp).or_else(|| (lp.len() >= 2).then(|| lp.iter().map(|e| vec![e.clone()]).collect()));
    let (mut sa, mut sb) = match (loop_sides(la), loop_sides(lb)) {
        // One smooth boundary against corners: it is cut where it comes nearest the corners, so
        // a circle to a square keeps the square's four corners.
        (None, Some(b)) => (cut_at(la, &b).ok_or_else(none)?, b),
        (Some(a), None) => {
            let b = cut_at(lb, &a).ok_or_else(none)?;
            (a, b)
        }
        _ => (sides(la).ok_or_else(none)?, sides(lb).ok_or_else(none)?),
    };
    // Different numbers of sides: the boundary with more keeps the corners nearest the other's
    // (in order round both) and its other sides are joined; a joined side keeps its corner
    // inside its bridge (3DCadCraft addition).
    if sa.len() > sb.len() {
        sa = merged_sides(&sa, &sb, false);
    } else if sb.len() > sa.len() {
        sb = merged_sides(&sb, &sa, true);
    }
    let n = sa.len();
    if sb.len() != n {
        return Err(KernelError::Invalid(format!(
            "the boundaries have {n} and {} sides (corners) that could not be paired",
            sb.len()
        )));
    }
    // B run either way: its sides in reverse order, each turned round.
    let turned: Vec<Vec<mt::Edge>> = sb.iter().rev().map(|side| side.iter().rev().map(mt::Edge::inverse).collect()).collect();
    let start = |side: &[mt::Edge]| side.first().map(|e| from_p3(e.front().point()));
    let mut best: Option<(f64, Vec<Vec<mt::Edge>>)> = None;
    for list in [&sb, &turned] {
        for r in 0..n {
            let cand: Vec<Vec<mt::Edge>> = (0..n).map(|k| list[(k + r) % n].clone()).collect();
            let cost: f64 = (0..n)
                .map(|k| match (start(&sa[k]), start(&cand[k])) {
                    (Some(p), Some(q)) => p.dist(q),
                    _ => f64::MAX,
                })
                .sum();
            if best.as_ref().is_none_or(|(c, _)| cost < *c) {
                best = Some((cost, cand));
            }
        }
    }
    let pb = best.map(|(_, c)| c).ok_or_else(|| KernelError::Invalid("no pairing of the sides".into()))?;
    let tol = solidcraft_geom::LINEAR_TOL;
    for k in 0..n {
        if let (Some(p), Some(q)) = (start(&sa[k]), start(&pb[k]))
            && p.dist(q) <= tol
        {
            return Err(KernelError::Invalid("the boundaries touch at a corner: use Boundary Patch".into()));
        }
    }
    // The corner lines, shared by the faces on either side of them.
    let corner = |side: &[mt::Edge]| -> Result<mt::Vertex> {
        side.first().map(|e| e.front().clone()).ok_or_else(|| KernelError::Invalid("an empty side".into()))
    };
    let tangent = weights.iter().any(Option::is_some);
    let rows: Vec<(Vec<Vec3>, Vec<Vec3>)> = (0..n).map(|k| Ok((side_points(&sa[k])?, side_points(&pb[k])?))).collect::<Result<_>>()?;
    // With a tangent condition: each side's reach, the two sides' reach averaged at the corner
    // they share, so both faces have the same corner curve.
    let mut reaches: Vec<(Vec<Vec3>, Vec<Vec3>)> = Vec::new();
    if tangent {
        let all_a: Vec<Vec3> = rows.iter().flat_map(|r| r.0.iter().copied()).collect();
        let all_b: Vec<Vec3> = rows.iter().flat_map(|r| r.1.iter().copied()).collect();
        let dist = centroid(&all_a).dist(centroid(&all_b));
        for k in 0..n {
            let r = Reach::with_distance(a, &sa[k], &rows[k].0, b, &pb[k], &rows[k].1, weights, dist)?;
            reaches.push((r.a, r.b));
        }
        for k in 0..n {
            let j = (k + 1) % n;
            // Corner j: the end of side k and the start of side j, on A and on B.
            let ma = match (reaches[k].0.last(), reaches[j].0.first()) {
                (Some(p), Some(q)) => (*p + *q) * 0.5,
                _ => continue,
            };
            let mb = match (reaches[k].1.last(), reaches[j].1.first()) {
                (Some(p), Some(q)) => (*p + *q) * 0.5,
                _ => continue,
            };
            // The change faded in over the last (first) 15 % of each row: moving the end point
            // alone kinked the row there and twisted the bridge next to the corner.
            let fade = |row: &mut Vec<Vec3>, to: Vec3, at_end: bool| {
                let n = row.len();
                if n == 0 {
                    return;
                }
                let end = if at_end { n - 1 } else { 0 };
                let delta = to - row[end];
                let reach = ((n as f64) * 0.15).ceil().max(1.0) as usize;
                for s in 0..reach.min(n) {
                    let i = if at_end { n - 1 - s } else { s };
                    let x = s as f64 / reach as f64;
                    row[i] = row[i] + delta * (1.0 - x).powi(3);
                }
            };
            fade(&mut reaches[k].0, ma, true);
            fade(&mut reaches[j].0, ma, false);
            fade(&mut reaches[k].1, mb, true);
            fade(&mut reaches[j].1, mb, false);
        }
    }
    let lines: Vec<mt::Edge> = (0..n)
        .map(|k| {
            let (va, vb) = (corner(&sa[k])?, corner(&pb[k])?);
            if tangent { across_edge(&va, &vb, reaches[k].0[0], reaches[k].1[0]) } else { Ok(mt::builder::line(&va, &vb)) }
        })
        .collect::<Result<_>>()?;
    let mut faces = Vec::with_capacity(n);
    for k in 0..n {
        let j = (k + 1) % n;
        // The surface between the sides (interpolated through points on them); its boundary
        // is the sides' own edges, so the bridge meets both surfaces exactly.
        let (qa, qb) = (&rows[k].0, &rows[k].1);
        let surface = if tangent { cubic_facing(qa, &reaches[k].0, qb, &reaches[k].1)? } else { ruled_facing(qa, qb)? };
        // The bridge's boundary is the surfaces' own edges (its rows run through 256 points of
        // each, close enough for the mesher), so it shares them exactly: Stitch joins the three
        // without any gap, and a solid closes.
        let mut wire: Vec<mt::Edge> = sa[k].clone();
        wire.push(lines[j].clone());
        wire.extend(pb[k].iter().rev().map(mt::Edge::inverse));
        wire.push(lines[k].inverse());
        let face = mt::Face::try_new(vec![wire.into()], mt::Surface::BSplineSurface(surface))
            .map_err(|e| KernelError::Failed(format!("bridge: {e}")))?;
        faces.push(face);
    }
    // The bridges run A's edges the other way from A's own faces, so stitched together they
    // close a consistently oriented shell.
    if let Some(e0) = sa.first().and_then(|side| side.first()) {
        let same_way = a.solid.face_iter().any(|f| {
            f.boundaries()
                .iter()
                .flat_map(|w| w.edge_iter())
                .any(|e| e.id() == e0.id() && e.front().id() == e0.front().id())
        });
        if same_way {
            faces = faces.iter().map(mt::Face::inverse).collect();
        }
    }
    Ok(faces)
}

/// The ruled surface between point rows `qa` (v = 0) and `qb` (v = 1) whose normal agrees with
/// the loop "along A, across, back along B": the face's triangles must face the way its wire
/// runs (a bridge whose surface faced the other way showed inside out).
fn ruled_facing(qa: &[Vec3], qb: &[Vec3]) -> Result<mt::BSplineSurface<mt::Point3>> {
    let (ca, cb) = (interpolated(qa)?, interpolated(qb)?);
    // The loop's own normal (Newell), in wire order: A forward, B backward.
    let ring: Vec<Vec3> = qa.iter().chain(qb.iter().rev()).copied().collect();
    let mut nl = Vec3::new(0.0, 0.0, 0.0);
    for i in 0..ring.len() {
        let (p, q) = (ring[i], ring[(i + 1) % ring.len()]);
        nl = nl + Vec3::new((p.y - q.y) * (p.z + q.z), (p.z - q.z) * (p.x + q.x), (p.x - q.x) * (p.y + q.y));
    }
    let surface = mt::BSplineSurface::homotopy(ca.clone(), cb.clone());
    let n = surface.normal(0.5, 0.5);
    if nl.dot(Vec3::new(n.x, n.y, n.z)) >= 0.0 {
        return Ok(surface);
    }
    // The other way: the same rows with u reversed, the wire then runs with the normal.
    let (mut ra, mut rb) = (ca, cb);
    ra.invert();
    rb.invert();
    Ok(mt::BSplineSurface::homotopy(ra, rb))
}

/// The Tangent condition's inner rows of a bridge: for each point of A's and of B's row, the
/// point the cubic across reaches towards (moved out of its surface past the edge).
struct Reach {
    a: Vec<Vec3>,
    b: Vec<Vec3>,
}

impl Reach {
    #[allow(clippy::too_many_arguments)]
    fn new(a: &Body, ea: &[mt::Edge], qa: &[Vec3], b: &Body, eb: &[mt::Edge], qb: &[Vec3], w: [Option<f64>; 2]) -> Result<Reach> {
        let dist = centroid(qa).dist(centroid(qb));
        Reach::with_distance(a, ea, qa, b, eb, qb, w, dist)
    }

    /// Reach `w · dist / 3` along each row's outward direction; a side without the condition
    /// (G0) reaches a third of the way straight towards the other row, as the ruled surface.
    #[allow(clippy::too_many_arguments)]
    fn with_distance(a: &Body, ea: &[mt::Edge], qa: &[Vec3], b: &Body, eb: &[mt::Edge], qb: &[Vec3], w: [Option<f64>; 2], dist: f64) -> Result<Reach> {
        if !(dist > solidcraft_geom::LINEAR_TOL) {
            return Err(KernelError::Invalid("the sections touch: use Boundary Patch".into()));
        }
        let row = |body: &Body, chain: &[mt::Edge], q: &[Vec3], other: &[Vec3], w: Option<f64>| -> Result<Vec<Vec3>> {
            match w {
                Some(w) => {
                    let out = outward_row(body, chain, q, BRIDGE_POINTS)?;
                    Ok(q.iter().zip(out).map(|(p, d)| *p + d * (w * dist / 3.0)).collect())
                }
                None => {
                    // Towards the other row's point at the same share of its points.
                    let n = other.len().saturating_sub(1);
                    let m = q.len().saturating_sub(1).max(1);
                    Ok(q.iter()
                        .enumerate()
                        .map(|(i, p)| {
                            let j = ((i * n + m / 2) / m).min(n);
                            *p + (other[j] - *p) * (1.0 / 3.0)
                        })
                        .collect())
                }
            }
        };
        Ok(Reach { a: row(a, ea, qa, qb, w[0])?, b: row(b, eb, qb, qa, w[1])? })
    }
}

fn centroid(p: &[Vec3]) -> Vec3 {
    let n = p.len().max(1) as f64;
    p.iter().fold(Vec3::new(0.0, 0.0, 0.0), |s, q| s + *q) / n
}

/// Unit directions out of the surface past its edge at each point of a chain's row (`pts`, as
/// [`side_points`] makes them: BRIDGE_POINTS per edge): the edge's direction as its face's
/// boundary runs it (the face on its left) crossed with the face's normal.
fn outward_row(body: &Body, chain: &[mt::Edge], pts: &[Vec3], per_edge: usize) -> Result<Vec<Vec3>> {
    let mut info: Vec<(mt::Surface, bool)> = Vec::new();
    for e in chain {
        let found = body.solid.face_iter().find_map(|f| {
            f.boundaries()
                .iter()
                .flat_map(|w| w.edge_iter())
                .find(|we| we.id() == e.id())
                .map(|we| (f.oriented_surface(), we.front().id() == e.front().id()))
        });
        // A piece cut from a body edge (a side split where it was needed): the face whose
        // boundary edge runs through the piece's middle, the same way or not by their directions.
        let found = found.or_else(|| {
            let c = e.oriented_curve();
            let (t0, t1) = c.range_tuple();
            let tm = 0.5 * (t0 + t1);
            let (mid, dm) = (c.subs(tm), c.der(tm));
            body.solid.face_iter().find_map(|f| {
                f.boundaries().iter().flat_map(|w| w.edge_iter()).find_map(|we| {
                    let wc = we.oriented_curve();
                    if !on_curve(&wc, mid, 1e-4) {
                        return None;
                    }
                    let (a, b) = wc.range_tuple();
                    let t = wc.search_nearest_parameter(mid, Some(0.5 * (a + b)), 100)?;
                    Some((f.oriented_surface(), wc.der(t).dot(dm) > 0.0))
                })
            })
        });
        info.push(found.ok_or_else(|| KernelError::Invalid("a section edge bounds no face".into()))?);
    }
    let n = info.len();
    let last = pts.len().saturating_sub(1);
    let edge_of = |i: usize| (i / per_edge.max(1)).min(n.saturating_sub(1));
    let on = |k: usize, uv: (f64, f64), p: &Vec3| -> bool {
        // A trim edge is a fitted curve, up to a few hundredths off its face.
        info.get(k).is_some_and(|(s, _)| mt::ParametricSurface::subs(s, uv.0, uv.1).distance(p3(*p)) <= 0.05)
    };
    let search = |k: usize, p: &Vec3, hint: Option<(f64, f64)>| -> Option<(f64, f64)> {
        let s = &info.get(k)?.0;
        match hint {
            Some(h) => s.search_nearest_parameter(p3(*p), Some(h), 30),
            None => s.search_nearest_parameter(p3(*p), None, 100),
        }
        .filter(|uv| on(k, *uv, p))
    };
    // Each search starts from the previous point's parameters (along one edge they move a
    // little): without a hint the kernel scans the whole surface first (1 s for a loft), and it
    // can also land on the wrong part of a wavy surface (the patch of a hole shot out as a spike)
    // — such a point is searched again from its neighbour's parameters.
    let mut uvs: Vec<Option<(f64, f64)>> = vec![None; pts.len()];
    let mut hint: Option<(usize, (f64, f64))> = None;
    for (i, p) in pts.iter().enumerate() {
        let k = edge_of(i);
        let start = hint.filter(|(e, _)| *e == k).map(|(_, uv)| uv);
        let found = start.and_then(|h| search(k, p, Some(h))).or_else(|| search(k, p, None));
        if let Some(uv) = found {
            hint = Some((k, uv));
        }
        uvs[i] = found;
    }
    for i in (0..pts.len()).rev() {
        if uvs[i].is_none() && i < last && edge_of(i + 1) == edge_of(i) {
            uvs[i] = uvs[i + 1].and_then(|h| search(edge_of(i), &pts[i], Some(h)));
        }
        if uvs[i].is_none() {
            // The nearest point whatever its distance (an edge further off its face).
            uvs[i] = info.get(edge_of(i)).and_then(|(s, _)| s.search_nearest_parameter(p3(pts[i]), None, 100));
        }
    }
    let mut out = Vec::with_capacity(pts.len());
    for (i, uv) in uvs.iter().enumerate() {
        let k = edge_of(i);
        let (surf, same) = info.get(k).ok_or_else(|| KernelError::Invalid("a section has no edges".into()))?;
        // The run's direction here from the points either side (one side at the ends). (The
        // edge curve's own derivative was tried: a curve's parameters run past its edge and the
        // nearest one landed elsewhere, turning a loft's bridge out into a sheet.)
        let (i0, i1) = (i.saturating_sub(1), (i + 1).min(last));
        let t = (pts[i1] - pts[i0]).normalized().ok_or_else(|| KernelError::Invalid("a section edge has no direction".into()))?;
        let t = if *same { t } else { -t };
        let (u, v) = uv.ok_or_else(|| KernelError::Failed("a point of a section edge is not on its surface".into()))?;
        let nn = surf.normal(u, v);
        let d = t
            .cross(Vec3::new(nn.x, nn.y, nn.z))
            .normalized()
            .ok_or_else(|| KernelError::Failed("no direction across a section edge".into()))?;
        out.push(d);
    }
    Ok(out)
}

/// The edge across a tangent bridge at one end: the cubic from A's corner `va` through the two
/// reach points to B's corner `vb` (the surface's own boundary curve there).
fn across_edge(va: &mt::Vertex, vb: &mt::Vertex, ra: Vec3, rb: Vec3) -> Result<mt::Edge> {
    let ctrl = vec![va.point(), p3(ra), p3(rb), vb.point()];
    let c = mt::BSplineCurve::try_new(mt::KnotVec::bezier_knot(3), ctrl).map_err(|e| KernelError::Failed(format!("bridge end: {e}")))?;
    Ok(mt::Edge::new(va, vb, mt::Curve::BSplineCurve(c)))
}

/// The cubic surface across from row `qa` (v = 0) to row `qb` (v = 1) through the reach rows
/// `ra` (interpolated at `qa`'s parameters) and `rb` (at `qb`'s), facing as [`ruled_facing`].
fn cubic_facing(qa: &[Vec3], ra: &[Vec3], qb: &[Vec3], rb: &[Vec3]) -> Result<mt::BSplineSurface<mt::Point3>> {
    let (pa, pb) = (chord_params(qa)?, chord_params(qb)?);
    let mut rows = [interpolated_at(qa, &pa)?, interpolated_at(ra, &pa)?, interpolated_at(rb, &pb)?, interpolated_at(qb, &pb)?];
    // One knot vector for all four rows (twice round: each sync can add knots to the first).
    for _ in 0..2 {
        let (first, rest) = rows.split_at_mut(1);
        for r in rest.iter_mut() {
            first[0].syncro_knots(r);
        }
    }
    let build = |rows: &[mt::BSplineCurve<mt::Point3>; 4]| -> Result<mt::BSplineSurface<mt::Point3>> {
        let m = rows[0].control_points().len();
        if rows.iter().any(|r| r.control_points().len() != m) {
            return Err(KernelError::Failed("the bridge's rows could not share knots".into()));
        }
        let cps: Vec<Vec<mt::Point3>> = (0..m).map(|i| rows.iter().map(|r| *r.control_point(i)).collect()).collect();
        mt::BSplineSurface::try_new((rows[0].knot_vec().clone(), mt::KnotVec::bezier_knot(3)), cps)
            .map_err(|e| KernelError::Failed(format!("bridge: {e}")))
    };
    let surface = build(&rows)?;
    let ring: Vec<Vec3> = qa.iter().chain(qb.iter().rev()).copied().collect();
    let mut nl = Vec3::new(0.0, 0.0, 0.0);
    for i in 0..ring.len() {
        let (p, q) = (ring[i], ring[(i + 1) % ring.len()]);
        nl = nl + Vec3::new((p.y - q.y) * (p.z + q.z), (p.z - q.z) * (p.x + q.x), (p.x - q.x) * (p.y + q.y));
    }
    let n = surface.normal(0.5, 0.5);
    if nl.dot(Vec3::new(n.x, n.y, n.z)) >= 0.0 {
        return Ok(surface);
    }
    for r in rows.iter_mut() {
        r.invert();
    }
    build(&rows)
}

/// Chord-length parameters of a row on [0, 1].
fn chord_params(pts: &[Vec3]) -> Result<Vec<f64>> {
    let mut t = vec![0.0];
    for w in pts.windows(2) {
        let last = t[t.len() - 1];
        t.push(last + w[0].dist(w[1]));
    }
    let len = t[t.len() - 1];
    if len <= solidcraft_geom::LINEAR_TOL {
        return Err(KernelError::Invalid("a section has no length".into()));
    }
    Ok(t.iter().map(|x| x / len).collect())
}

/// A cubic B-spline through `pts` at `params`; where the points turn sharply (a corner inside
/// a side: more than 20° between neighbouring steps) the runs either side are interpolated on
/// their own and joined with a triple knot (C0 there), as a smooth spline through a corner
/// rings past it.
fn interpolated_at(pts: &[Vec3], params: &[f64]) -> Result<mt::BSplineCurve<mt::Point3>> {
    let n = pts.len();
    let turn = |i: usize| -> bool {
        match ((pts[i] - pts[i - 1]).normalized(), (pts[i + 1] - pts[i]).normalized()) {
            (Some(a), Some(b)) => a.dot(b) < (20f64).to_radians().cos(),
            _ => false,
        }
    };
    let mut cuts: Vec<usize> = vec![0];
    for i in 1..n.saturating_sub(1) {
        if turn(i) && i - cuts[cuts.len() - 1] >= 3 && n - 1 - i >= 3 {
            cuts.push(i);
        }
    }
    cuts.push(n.saturating_sub(1));
    let piece = |a: usize, b: usize| -> Result<(Vec<f64>, Vec<mt::Point3>)> {
        let coords: Vec<[f64; 3]> = pts[a..=b].iter().map(|q| [q.x, q.y, q.z]).collect();
        let c = solidcraft_geom::bspline::CurveN::<3>::interpolate(&params[a..=b], &coords, 3)
            .map_err(|e| KernelError::Invalid(format!("a section cannot be interpolated: {e:?}")))?;
        Ok((c.knots, c.ctrl.iter().map(|p| mt::Point3::new(p[0], p[1], p[2])).collect()))
    };
    let (mut knots, mut ctrl) = piece(cuts[0], cuts[1])?;
    for w in cuts.windows(2).skip(1) {
        // Joined at the corner: one of the four end knots dropped (a triple knot stays), the
        // shared point once.
        let (k1, c1) = piece(w[0], w[1])?;
        knots.pop();
        knots.extend(k1.into_iter().skip(4));
        ctrl.extend(c1.into_iter().skip(1));
    }
    mt::BSplineCurve::try_new(mt::KnotVec::from(knots), ctrl).map_err(|e| KernelError::Invalid(format!("a section's curve: {e}")))
}

/// A smooth closed boundary `lp` cut into as many sides as `other` has, at its points nearest
/// `other`'s corners (an existing vertex within the tolerance is used as it is); the pieces of a
/// cut edge are new edges on its curve (Stitch cuts the surface's own edge there to match).
/// `None` when a cut fails or two corners fall on one point.
fn cut_at(lp: &[mt::Edge], other: &[Vec<mt::Edge>]) -> Option<Vec<Vec<mt::Edge>>> {
    let corners: Vec<mt::Point3> = other.iter().filter_map(|side| side.first().map(|e| e.front().point())).collect();
    cut_at_points(lp, &corners)
}

/// [`cut_at`] at the loop's points nearest `corners`.
fn cut_at_points(lp: &[mt::Edge], corners: &[mt::Point3]) -> Option<Vec<Vec<mt::Edge>>> {
    let tol = solidcraft_geom::LINEAR_TOL.max(1e-6);
    // The nearest point of the loop to each corner: (edge index, parameter on its absolute curve).
    let mut cuts: Vec<Vec<f64>> = vec![Vec::new(); lp.len()];
    let mut at_vertex: Vec<mt::VertexID> = Vec::new();
    for q in corners {
        let mut best: Option<(f64, usize, f64)> = None;
        for (k, e) in lp.iter().enumerate() {
            let abs = if e.orientation() { e.clone() } else { e.inverse() };
            let c = abs.curve();
            let (t0, t1) = c.range_tuple();
            let n = 64;
            let ts = (0..=n)
                .map(|i| t0 + (t1 - t0) * i as f64 / n as f64)
                .min_by(|a, b| c.subs(*a).distance(*q).total_cmp(&c.subs(*b).distance(*q)))?;
            let t = c.search_nearest_parameter(*q, Some(ts), 100).unwrap_or(ts).clamp(t0.min(t1), t0.max(t1));
            let d = c.subs(t).distance(*q);
            if best.is_none_or(|(b, _, _)| d < b) {
                best = Some((d, k, t));
            }
        }
        let (_, k, t) = best?;
        let abs = if lp[k].orientation() { lp[k].clone() } else { lp[k].inverse() };
        let p = abs.curve().subs(t);
        if let Some(v) = [abs.front(), abs.back()].into_iter().find(|v| v.point().distance(p) <= tol) {
            at_vertex.push(v.id());
        } else {
            cuts[k].push(t);
        }
    }
    // The loop with its edges cut; the corner vertices.
    let mut edges: Vec<mt::Edge> = Vec::new();
    let mut corner_ids: Vec<mt::VertexID> = at_vertex;
    for (k, e) in lp.iter().enumerate() {
        let mut ts = cuts[k].clone();
        if ts.is_empty() {
            edges.push(e.clone());
            continue;
        }
        let abs = if e.orientation() { e.clone() } else { e.inverse() };
        ts.sort_by(f64::total_cmp);
        let mut pieces: Vec<mt::Edge> = Vec::new();
        let mut rest = abs;
        // Cut from the far end back, so each parameter stays on the piece still to be cut.
        for t in ts.iter().rev() {
            let v = mt::Vertex::new(rest.curve().subs(*t));
            let (a, b) = rest.cut_with_parameter(&v, *t)?;
            corner_ids.push(v.id());
            pieces.push(b);
            rest = a;
        }
        pieces.push(rest);
        pieces.reverse();
        if e.orientation() {
            edges.extend(pieces);
        } else {
            edges.extend(pieces.iter().rev().map(mt::Edge::inverse));
        }
    }
    let mut unique: Vec<mt::VertexID> = Vec::new();
    for v in corner_ids {
        if !unique.contains(&v) {
            unique.push(v);
        }
    }
    let corner_ids = unique;
    if corner_ids.len() != corners.len() {
        return None;
    }
    // Sides from corner to corner.
    let n = edges.len();
    let first = (0..n).find(|&k| corner_ids.contains(&edges[k].front().id()))?;
    let mut sides: Vec<Vec<mt::Edge>> = Vec::new();
    for i in 0..n {
        let e = &edges[(first + i) % n];
        if corner_ids.contains(&e.front().id()) {
            sides.push(Vec::new());
        }
        sides.last_mut()?.push(e.clone());
    }
    Some(sides)
}

/// The sides of boundary `many` joined into as many as `few` has: the kept corners (starts of
/// sides) are the ones nearest `few`'s corners in order round both loops (dynamic programming
/// over every start and, with `either_way`, both directions of `few`), each kept side running on
/// over the sides up to the next kept corner.
fn merged_sides(many: &[Vec<mt::Edge>], few: &[Vec<mt::Edge>], either_way: bool) -> Vec<Vec<mt::Edge>> {
    let start = |side: &[mt::Edge]| side.first().map(|e| from_p3(e.front().point())).unwrap_or(Vec3::new(0.0, 0.0, 0.0));
    let m = many.len();
    let n = few.len();
    if n == 0 || n >= m {
        return many.to_vec();
    }
    let mc: Vec<Vec3> = many.iter().map(|s| start(s)).collect();
    let fc: Vec<Vec3> = few.iter().map(|s| start(s)).collect();
    // `few` turned round: its corners in reverse order (a turned side starts where it ended).
    let fr: Vec<Vec3> = (0..n).map(|k| fc[(n - k) % n]).collect();
    let mut best: Option<(f64, Vec<usize>)> = None;
    for f in if either_way { vec![&fc, &fr] } else { vec![&fc] } {
        for r in 0..m {
            // cost[k][j]: corners 0..=k of `few` on kept corners up to j (offset from r) of `many`.
            let inf = f64::INFINITY;
            let mut cost = vec![vec![inf; m]; n];
            let mut from = vec![vec![0usize; m]; n];
            cost[0][0] = f[0].dist(mc[r]);
            for k in 1..n {
                for j in k..m {
                    let d = f[k].dist(mc[(r + j) % m]);
                    for i in (k - 1)..j {
                        if cost[k - 1][i] + d < cost[k][j] {
                            cost[k][j] = cost[k - 1][i] + d;
                            from[k][j] = i;
                        }
                    }
                }
            }
            let (j, c) = (0..m).map(|j| (j, cost[n - 1][j])).fold((0, inf), |a, b| if b.1 < a.1 { b } else { a });
            if !c.is_finite() || best.as_ref().is_some_and(|(b, _)| c >= *b) {
                continue;
            }
            let mut keep = vec![0usize; n];
            let mut at = j;
            for k in (0..n).rev() {
                keep[k] = (r + at) % m;
                at = from[k][at];
            }
            best = Some((c, keep));
        }
    }
    let Some((_, keep)) = best else { return many.to_vec() };
    (0..n)
        .map(|k| {
            let (a, b) = (keep[k], keep[(k + 1) % n]);
            let len = (b + m - a) % m;
            let len = if len == 0 { m } else { len };
            (0..len).flat_map(|i| many[(a + i) % m].iter().cloned()).collect()
        })
        .collect()
}

/// Points per edge a bridge's rows are interpolated through: dense enough that the surfaces' own
/// edges lie on the bridge within the mesher's tolerance.
const BRIDGE_POINTS: usize = 256;

/// Points along a side (its chain of edges) whose ends are exactly its end vertices.
fn side_points(side: &[mt::Edge]) -> Result<Vec<Vec3>> {
    let mut p = chain_points_n(side, BRIDGE_POINTS)?;
    if let (Some(f), Some(l)) = (side.first(), side.last())
        && let Some(k) = p.len().checked_sub(1)
    {
        p[0] = from_p3(f.front().point());
        p[k] = from_p3(l.back().point());
    }
    Ok(p)
}

/// Points along a chain of edges (64 per edge, the joints once), in the chain's direction.
fn chain_points(chain: &[mt::Edge]) -> Result<Vec<Vec3>> {
    chain_points_n(chain, 64)
}

/// [`chain_points`] with `per_edge` points per edge.
fn chain_points_n(chain: &[mt::Edge], per_edge: usize) -> Result<Vec<Vec3>> {
    let mut pts: Vec<Vec3> = Vec::new();
    for e in chain {
        let mut s = crate::plane_fit::EdgeSampler::new(e)
            .ok_or_else(|| KernelError::Invalid("an edge has no parameter range".into()))?;
        for k in 0..=per_edge {
            if k == 0 && !pts.is_empty() {
                continue;
            }
            let q = s
                .at(k as f64 / per_edge as f64)
                .ok_or_else(|| KernelError::Invalid("a point of an edge could not be found".into()))?;
            pts.push(q);
        }
    }
    Ok(pts)
}

/// A cubic B-spline through `pts` at chord-length parameters on [0, 1].
fn interpolated(pts: &[Vec3]) -> Result<mt::BSplineCurve<mt::Point3>> {
    interpolated_at(pts, &chord_params(pts)?)
}

/// The edges of `body` picked by points on them, chained into one open chain (each turned to
/// run along it). Call it under [`guard`].
fn picked_chain(body: &Body, at: &[Vec3]) -> Result<Vec<mt::Edge>> {
    let mut pool: Vec<mt::Edge> = Vec::new();
    for p in at {
        let e = edge_near(body, *p).ok_or_else(|| KernelError::Invalid(format!("no edge at {p:?}")))?;
        if !pool.iter().any(|x| x.id() == e.id()) {
            pool.push(e);
        }
    }
    let uses = |v: mt::VertexID, pool: &[mt::Edge]| {
        pool.iter().filter(|e| e.front().id() == v || e.back().id() == v).count()
    };
    // Start at an edge with a free end (a closed loop has none: it is not a bridge section).
    let k = pool
        .iter()
        .position(|e| uses(e.front().id(), &pool) == 1 || uses(e.back().id(), &pool) == 1)
        .ok_or_else(|| KernelError::Invalid("a section's edges close a loop: use Loft or Boundary Patch".into()))?;
    let first = pool.remove(k);
    let first = if uses(first.front().id(), &pool) == 0 { first } else { first.inverse() };
    let mut at_v = first.back().clone();
    let mut chain = vec![first];
    while let Some(k) = pool.iter().position(|e| e.front().id() == at_v.id() || e.back().id() == at_v.id()) {
        let e = pool.remove(k);
        let e = if e.front().id() == at_v.id() { e } else { e.inverse() };
        at_v = e.back().clone();
        chain.push(e);
    }
    if !pool.is_empty() {
        return Err(KernelError::Invalid("a section's edges do not form one chain".into()));
    }
    Ok(chain)
}

/// A closed loop of four sides (four edges, or four runs between corners) filled by the Coons
/// patch of its sides, on edges of its own (its boundary curves, between the loop's vertices).
/// Its history is over `[body]`.
fn four_sided_patch(body: &Body, chain: &[mt::Edge]) -> Result<Body> {
    four_sided_patch_with(body, chain, None)
}

/// [`four_sided_patch`]; `tangent`: G1 to the faces the loop bounds (see
/// [`patch_edges_tangent`]).
fn four_sided_patch_with(body: &Body, chain: &[mt::Edge], tangent: Option<f64>) -> Result<Body> {
    // Tangent of a loop without real corners (a round hole): a four-sided patch must bend at
    // its corners there (the rim cannot be both its side and its cross direction) and came out
    // creased or wrinkled — refused until an n-sided G1 fill exists.
    if tangent.is_some() && smooth_loop(chain) {
        return Err(KernelError::Invalid(
            "not supported yet: the Tangent condition on a loop without corners (a round hole); use Contact".into(),
        ));
    }
    let sides: Vec<Vec<mt::Edge>> = if chain.len() == 4 {
        chain.iter().map(|e| vec![e.clone()]).collect()
    } else {
        loop_sides(chain).ok_or_else(|| KernelError::Invalid("the loop has no corners".into()))?
    };
    if sides.len() != 4 {
        return Err(KernelError::Invalid(format!(
            "a loop of {} sides is not filled from its sides{}",
            sides.len(),
            if tangent.is_some() { " (Tangent needs a loop of four sides yet)" } else { "" }
        )));
    }
    four_sided_from(body, sides, tangent)
}

/// The direction an edge runs over its first (`end`: last) 20 % (a chord): a hook a fitted trim
/// curve can have right at its end does not count.
fn span_direction(e: &mt::Edge, end: bool) -> Option<Vec3> {
    let mut s = crate::plane_fit::EdgeSampler::new(e)?;
    let (p, q) = if end { (s.at(0.8)?, s.at(1.0)?) } else { (s.at(0.0)?, s.at(0.2)?) };
    (q - p).normalized()
}

/// Whether a loop has no real corner: no two edges longer than 5 % of it meet at more than 45°
/// (chords over a fifth of each, so a hook a fitted trim curve has at its end does not count).
fn smooth_loop(chain: &[mt::Edge]) -> bool {
    let len = |e: &mt::Edge| chain_points(std::slice::from_ref(e)).map(|p| p.windows(2).map(|w| w[0].dist(w[1])).sum::<f64>()).unwrap_or(0.0);
    let lens: Vec<f64> = chain.iter().map(len).collect();
    let total: f64 = lens.iter().sum();
    let n = chain.len();
    !(0..n).any(|k| {
        let j = (k + 1) % n;
        lens[k] > 0.05 * total
            && lens[j] > 0.05 * total
            && matches!((span_direction(&chain[k], true), span_direction(&chain[j], false)), (Some(x), Some(y)) if x.dot(y) < (45f64).to_radians().cos())
    })
}

/// The patch of four sides (see [`four_sided_patch`]).
fn four_sided_from(body: &Body, sides: Vec<Vec<mt::Edge>>, tangent: Option<f64>) -> Result<Body> {
    // The Coons patch of the four sides (each side a spline through 48 of its points), sampled on
    // a grid and interpolated row by row, then column by column (a cubic tensor B-spline). Its
    // edges are the surface's own boundary curves, so they lie on it exactly (truck's
    // `by_boundary` only averages control points: a circle in four arcs collapsed to nothing).
    let per_edge = |s: &[mt::Edge]| (48 / s.len()).max(8);
    let rows: Vec<Vec<Vec3>> = sides
        .iter()
        .map(|s| {
            let mut p = chain_points_n(s, per_edge(s))?;
            if let (Some(f), Some(l), Some(k)) = (s.first(), s.last(), p.len().checked_sub(1)) {
                p[0] = from_p3(f.front().point());
                p[k] = from_p3(l.back().point());
            }
            Ok(p)
        })
        .collect::<Result<_>>()?;
    let c: Vec<mt::BSplineCurve<mt::Point3>> = rows.iter().map(|r| interpolated(r)).collect::<Result<_>>()?;
    let at = |k: usize, t: f64| -> Vec3 { let p = c[k].subs(t.clamp(0.0, 1.0)); Vec3::new(p.x, p.y, p.z) };
    let (p00, p10, p11, p01) = (at(0, 0.0), at(1, 0.0), at(2, 0.0), at(3, 0.0));
    // Tangent: the directions out of each bounding face past its side (as rows, interpolated
    // like the sides themselves), so the patch leaves each side along its neighbour.
    let outs: Option<Vec<mt::BSplineCurve<mt::Point3>>> = match tangent {
        None => None,
        Some(_) => Some(
            sides
                .iter()
                .zip(&rows)
                .map(|(s, r)| {
                    let d = outward_row(body, s, r, per_edge(s))?;
                    // Vectors interpolated as points, at the side's own parameters.
                    interpolated_at(&d, &chord_params(r)?)
                })
                .collect::<Result<_>>()?,
        ),
    };
    let out = |k: usize, t: f64| -> Vec3 {
        outs.as_ref().map_or(Vec3::new(0.0, 0.0, 0.0), |o| {
            let p = o[k].subs(t.clamp(0.0, 1.0));
            Vec3::new(p.x, p.y, p.z)
        })
    };
    let w = tangent.unwrap_or(0.0);
    let coons = |u: f64, v: f64| -> Vec3 {
        let (b, t) = (at(0, u), at(2, 1.0 - u));
        let (l, r) = (at(3, 1.0 - v), at(1, v));
        if tangent.is_none() {
            return b * (1.0 - v) + t * v + l * (1.0 - u) + r * u
                - (p00 * ((1.0 - u) * (1.0 - v)) + p10 * (u * (1.0 - v)) + p11 * (u * v) + p01 * ((1.0 - u) * v));
        }
        // Cubic Hermite blends: h0 / h1 weight the boundaries, g0 / g1 their cross derivatives.
        let h = |x: f64| [2.0 * x * x * x - 3.0 * x * x + 1.0, -2.0 * x * x * x + 3.0 * x * x, x * x * x - 2.0 * x * x + x, x * x * x - x * x];
        // Cross derivatives: into the patch from the bottom (v = 0) and left (u = 0) sides along
        // their faces' outward directions; at the top and right the patch arrives against them.
        // Near the corners each is blended to the neighbouring side's own derivative there, as
        // a bicubic Coons patch needs compatible corners (else the mismatch bends it all over).
        // Derivatives over 2 % of a side: a trimmed rim can have a tiny piece running back at a
        // corner, and a finer step there gave twists that shot the patch out as a spike.
        let e = 0.02;
        let d = |k: usize, t: f64| -> Vec3 {
            let (a, b) = ((t - e).max(0.0), (t + e).min(1.0));
            (at(k, b) - at(k, a)) / (b - a)
        };
        // Boundary derivatives in (u, v): B(u) = side 0, R(v) = side 1, T(u) = side 2 backwards,
        // L(v) = side 3 backwards.
        let (db, dr) = (|u: f64| d(0, u), |v: f64| d(1, v));
        let (dt, dl) = (|u: f64| -d(2, 1.0 - u), |v: f64| -d(3, 1.0 - v));
        let width_u = |v: f64| at(3, 1.0 - v).dist(at(1, v));
        let width_v = |u: f64| at(0, u).dist(at(2, 1.0 - u));
        let sv0r = |u: f64| out(0, u) * (w * width_v(u));
        let sv1r = |u: f64| -out(2, 1.0 - u) * (w * width_v(u));
        let su0r = |v: f64| out(3, 1.0 - v) * (w * width_u(v));
        let su1r = |v: f64| -out(1, v) * (w * width_u(v));
        // Fading away from the corners ((1 − x)⁴: 6 % at the middle), so the sides keep their
        // faces' directions over most of their length.
        let fix = |raw: &dyn Fn(f64) -> Vec3, x: f64, at0: Vec3, at1: Vec3| raw(x) + (at0 - raw(0.0)) * (1.0 - x).powi(4) + (at1 - raw(1.0)) * x.powi(4);
        let sv0 = |u: f64| fix(&sv0r, u, dl(0.0), dr(0.0));
        let sv1 = |u: f64| fix(&sv1r, u, dl(1.0), dr(1.0));
        let su0 = |v: f64| fix(&su0r, v, db(0.0), dt(0.0));
        let su1 = |v: f64| fix(&su1r, v, db(1.0), dt(1.0));
        // No corner twists: estimated from the blended cross derivatives they were steep
        // (≈ 150 on a 20 mm patch) and pressed the middle flat.
        let zero = Vec3::new(0.0, 0.0, 0.0);
        let (t00, t10, t01, t11) = (zero, zero, zero, zero);
        let (hu, hv) = (h(u), h(v));
        let lu = l * hu[0] + r * hu[1] + su0(v) * hu[2] + su1(v) * hu[3];
        let lv = b * hv[0] + t * hv[1] + sv0(u) * hv[2] + sv1(u) * hv[3];
        // The corner terms (positions, cross derivatives, twists).
        let m = [
            [p00, p01, sv0(0.0), sv1(0.0)],
            [p10, p11, sv0(1.0), sv1(1.0)],
            [su0(0.0), su0(1.0), t00, t01],
            [su1(0.0), su1(1.0), t10, t11],
        ];
        let mut tt = Vec3::new(0.0, 0.0, 0.0);
        for (i, row) in m.iter().enumerate() {
            for (j, q) in row.iter().enumerate() {
                tt = tt + *q * (hu[i] * hv[j]);
            }
        }
        lu + lv - tt
    };
    const N: usize = 24;
    let params: Vec<f64> = (0..=N).map(|k| k as f64 / N as f64).collect();
    let interp = |pts: &[[f64; 3]]| {
        solidcraft_geom::bspline::CurveN::<3>::interpolate(&params, pts, 3)
            .map_err(|e| KernelError::Invalid(format!("the patch cannot be fitted: {e:?}")))
    };
    // Rows along u (one per v), then the control points column by column along v.
    let mut row_ctrl: Vec<Vec<[f64; 3]>> = Vec::with_capacity(N + 1);
    let mut uknots = Vec::new();
    for &v in &params {
        let pts: Vec<[f64; 3]> = params.iter().map(|&u| { let q = coons(u, v); [q.x, q.y, q.z] }).collect();
        let cv = interp(&pts)?;
        uknots = cv.knots.clone();
        row_ctrl.push(cv.ctrl);
    }
    let nu = row_ctrl.first().map_or(0, Vec::len);
    let mut grid: Vec<Vec<mt::Point3>> = vec![Vec::with_capacity(N + 1); nu];
    let mut vknots = Vec::new();
    for (i, col) in grid.iter_mut().enumerate() {
        let pts: Vec<[f64; 3]> = row_ctrl.iter().map(|r| r[i]).collect();
        let cv = interp(&pts)?;
        vknots = cv.knots.clone();
        *col = cv.ctrl.iter().map(|q| mt::Point3::new(q[0], q[1], q[2])).collect();
    }
    let surface = mt::BSplineSurface::try_new((mt::KnotVec::from(uknots.clone()), mt::KnotVec::from(vknots.clone())), grid.clone())
        .map_err(|e| KernelError::Invalid(format!("the patch surface: {e}")))?;
    // Its four boundary curves: v = 0, u = 1, v = 1, u = 0.
    let curve = |knots: &[f64], pts: Vec<mt::Point3>| {
        mt::BSplineCurve::try_new(mt::KnotVec::from(knots.to_vec()), pts)
            .map_err(|e| KernelError::Invalid(format!("a patch edge: {e}")))
    };
    let m = grid.first().map_or(0, Vec::len);
    let bottom = curve(&uknots, grid.iter().map(|col| col[0]).collect())?;
    let right = curve(&vknots, grid[nu - 1].clone())?;
    let top = curve(&uknots, grid.iter().map(|col| col[m - 1]).collect())?;
    let left = curve(&vknots, grid[0].clone())?;
    let corner = |k: usize| sides[k].first().map(|e| e.front().clone()).ok_or_else(|| KernelError::Invalid("an empty side".into()));
    let (v00, v10, v11, v01) = (corner(0)?, corner(1)?, corner(2)?, corner(3)?);
    // Counter-clockwise in (u, v): the face's normal (Su × Sv) and its wire agree.
    let wire: mt::Wire = vec![
        mt::Edge::new(&v00, &v10, mt::Curve::BSplineCurve(bottom)),
        mt::Edge::new(&v10, &v11, mt::Curve::BSplineCurve(right)),
        mt::Edge::new(&v01, &v11, mt::Curve::BSplineCurve(top)).inverse(),
        mt::Edge::new(&v00, &v01, mt::Curve::BSplineCurve(left)).inverse(),
    ]
    .into();
    let face = mt::Face::try_new(vec![wire], mt::Surface::BSplineSurface(surface))
        .map_err(|e| KernelError::Failed(format!("patch: {e}")))?;
    let mut rec = Rebuild::default();
    rec.made(&face, Vec::new());
    Ok(rebuilt_body(surface_body(vec![face]), &[&body.solid], &rec))
}

/// A cubic B-spline sheet whose control net is `net` (rows along u, each the same length; at
/// least 2 × 2): clamped uniform knots, so the sheet ends on the net's border (its corners are
/// the net's corners); its four border curves are its edges. One face, a surface body.
/// (3DCadCraft addition: Freeform Plane, a control cage edited point by point.)
pub fn bspline_sheet(net: &[Vec<Vec3>]) -> Result<Body> {
    let nu = net.len();
    let nv = net.first().map_or(0, Vec::len);
    if nu < 2 || nv < 2 || net.iter().any(|r| r.len() != nv) || nu * nv > 100_000 {
        return Err(KernelError::Invalid("a control net of at least 2 × 2 equal rows".into()));
    }
    if net.iter().flatten().any(|p| !p.is_finite()) {
        return Err(KernelError::Invalid("the control net has a point that is not finite".into()));
    }
    guard("freeform", || {
        // Degree min(3, points - 1), clamped uniform knots.
        let knots = |n: usize| -> (usize, mt::KnotVec) {
            let d = (n - 1).min(3);
            let inner = n - d - 1;
            let mut k = vec![0.0; d + 1];
            k.extend((1..=inner).map(|i| i as f64 / (inner + 1) as f64));
            k.extend(vec![1.0; d + 1]);
            (d, mt::KnotVec::from(k))
        };
        let ((_, ku), (_, kv)) = (knots(nu), knots(nv));
        let cps: Vec<Vec<mt::Point3>> = net.iter().map(|r| r.iter().map(|q| p3(*q)).collect()).collect();
        let surface = mt::BSplineSurface::try_new((ku.clone(), kv.clone()), cps.clone())
            .map_err(|e| KernelError::Invalid(format!("the freeform surface: {e}")))?;
        let curve = |k: &mt::KnotVec, pts: Vec<mt::Point3>| {
            mt::BSplineCurve::try_new(k.clone(), pts).map_err(|e| KernelError::Invalid(format!("a freeform edge: {e}")))
        };
        // Borders: v = 0 (along u), u = 1 (along v), v = 1, u = 0.
        let bottom = curve(&ku, cps.iter().map(|r| r[0]).collect())?;
        let right = curve(&kv, cps[nu - 1].clone())?;
        let top = curve(&ku, cps.iter().map(|r| r[nv - 1]).collect())?;
        let left = curve(&kv, cps[0].clone())?;
        let v00 = mt::Vertex::new(cps[0][0]);
        let v10 = mt::Vertex::new(cps[nu - 1][0]);
        let v11 = mt::Vertex::new(cps[nu - 1][nv - 1]);
        let v01 = mt::Vertex::new(cps[0][nv - 1]);
        let wire: mt::Wire = vec![
            mt::Edge::new(&v00, &v10, mt::Curve::BSplineCurve(bottom)),
            mt::Edge::new(&v10, &v11, mt::Curve::BSplineCurve(right)),
            mt::Edge::new(&v01, &v11, mt::Curve::BSplineCurve(top)).inverse(),
            mt::Edge::new(&v00, &v01, mt::Curve::BSplineCurve(left)).inverse(),
        ]
        .into();
        let face = mt::Face::try_new(vec![wire], mt::Surface::BSplineSurface(surface)).map_err(|e| KernelError::Failed(format!("freeform: {e}")))?;
        let mut rec = Rebuild::default();
        rec.made(&face, Vec::new());
        Ok(rebuilt_body(surface_body(vec![face]), &[], &rec))
    })
}

/// A cubic B-spline tube: closed (periodic, C2) around through the rings of control points
/// `net[i]` (i around, at least 3; each ring along the height, at least 2 points, clamped there).
/// Built as two faces (halves of the period, cut with full knots) sharing the two seams, so no
/// edge meets itself; its open ends are its two rims. (3DCadCraft addition: Freeform Cylinder.)
pub fn bspline_tube(net: &[Vec<Vec3>]) -> Result<Body> {
    let n = net.len();
    let nv = net.first().map_or(0, Vec::len);
    if n < 3 || nv < 2 || net.iter().any(|r| r.len() != nv) || n * nv > 100_000 {
        return Err(KernelError::Invalid("a tube cage needs 3 rings or more of 2 points or more".into()));
    }
    if net.iter().flatten().any(|p| !p.is_finite()) {
        return Err(KernelError::Invalid("the control net has a point that is not finite".into()));
    }
    guard("freeform", || {
        // Periodic around: the first three rings again at the end, uniform knots 0 ..= n + 6; the
        // period is [3, n + 3].
        let mut rows: Vec<Vec<mt::Point3>> = net.iter().map(|r| r.iter().map(|q| p3(*q)).collect()).collect();
        for k in 0..3 {
            rows.push(rows[k].clone());
        }
        let ku = mt::KnotVec::from((0..rows.len() + 4).map(|k| k as f64).collect::<Vec<_>>());
        let dv = (nv - 1).min(3);
        let inner = nv - dv - 1;
        let mut kvv = vec![0.0; dv + 1];
        kvv.extend((1..=inner).map(|i| i as f64 / (inner + 1) as f64));
        kvv.extend(vec![1.0; dv + 1]);
        let kv = mt::KnotVec::from(kvv);
        let mut whole = mt::BSplineSurface::try_new((ku, kv), rows).map_err(|e| KernelError::Invalid(format!("the freeform tube: {e}")))?;
        let (t0, t1) = (3.0, (n + 3) as f64);
        let mid = 0.5 * (t0 + t1);
        let mut period = whole.ucut(t0);
        let _ = period.ucut(t1);
        let mut a = period;
        let b = a.ucut(mid);
        // Border curves of a clamped half: its first / last column (along v), its first / last
        // control row (along u).
        let col = |s: &mt::BSplineSurface<mt::Point3>, last: bool| -> Result<mt::BSplineCurve<mt::Point3>> {
            let cps = s.control_points();
            let c = if last { cps.last() } else { cps.first() };
            let c = c.ok_or_else(|| KernelError::Failed("an empty tube half".into()))?;
            mt::BSplineCurve::try_new(s.vknot_vec().clone(), c.clone()).map_err(|e| KernelError::Failed(format!("a tube seam: {e}")))
        };
        let rim = |s: &mt::BSplineSurface<mt::Point3>, top: bool| -> Result<mt::BSplineCurve<mt::Point3>> {
            let pts: Vec<mt::Point3> = s.control_points().iter().filter_map(|c| if top { c.last() } else { c.first() }).copied().collect();
            mt::BSplineCurve::try_new(s.uknot_vec().clone(), pts).map_err(|e| KernelError::Failed(format!("a tube rim: {e}")))
        };
        // Seam 0 (where the period starts and ends) and seam 1 (the middle), shared by both faces.
        let s0 = col(&a, false)?;
        let s1 = col(&a, true)?;
        let (p00, p01) = (s0.front(), s0.back());
        let (p10, p11) = (s1.front(), s1.back());
        let (v00, v01, v10, v11) = (mt::Vertex::new(p00), mt::Vertex::new(p01), mt::Vertex::new(p10), mt::Vertex::new(p11));
        let seam0 = mt::Edge::new(&v00, &v01, mt::Curve::BSplineCurve(s0));
        let seam1 = mt::Edge::new(&v10, &v11, mt::Curve::BSplineCurve(s1));
        let face = |s: mt::BSplineSurface<mt::Point3>, from: &mt::Edge, to: &mt::Edge| -> Result<mt::Face> {
            let (f0, f1) = (from.front().clone(), from.back().clone());
            let (g0, g1) = (to.front().clone(), to.back().clone());
            let bottom = mt::Edge::new(&f0, &g0, mt::Curve::BSplineCurve(rim(&s, false)?));
            let top = mt::Edge::new(&f1, &g1, mt::Curve::BSplineCurve(rim(&s, true)?));
            let wire: mt::Wire = vec![bottom, to.clone(), top.inverse(), from.inverse()].into();
            mt::Face::try_new(vec![wire], mt::Surface::BSplineSurface(s)).map_err(|e| KernelError::Failed(format!("freeform tube: {e}")))
        };
        let fa = face(a, &seam0, &seam1)?;
        let fb = face(b, &seam1, &seam0)?;
        let mut rec = Rebuild::default();
        rec.made(&fa, Vec::new());
        rec.made(&fb, Vec::new());
        Ok(rebuilt_body(surface_body(vec![fa, fb]), &[], &rec))
    })
}

/// The edges of `body` picked by points on them, chained into one closed loop (each turned to
/// run along it). Call it under [`guard`]: sampling an intersection curve can fail in truck.
pub(crate) fn picked_loop(body: &Body, at: &[Vec3]) -> Result<Vec<mt::Edge>> {
    let mut edges: Vec<mt::Edge> = Vec::new();
    for p in at {
        let e = edge_near(body, *p).ok_or_else(|| KernelError::Invalid(format!("no edge at {p:?}")))?;
        if !edges.iter().any(|x| x.id() == e.id()) {
            edges.push(e);
        }
    }
    let mut pool = edges;
    let first = pool.pop().ok_or_else(|| KernelError::Invalid("no edges".into()))?;
    let start = first.front().id();
    let mut at_v = first.back().clone();
    let mut chain = vec![first];
    while at_v.id() != start {
        let k = pool
            .iter()
            .position(|e| e.front().id() == at_v.id() || e.back().id() == at_v.id())
            .ok_or_else(|| KernelError::Invalid("the edges do not form a closed loop".into()))?;
        let e = pool.remove(k);
        let e = if e.front().id() == at_v.id() { e } else { e.inverse() };
        at_v = e.back().clone();
        chain.push(e);
    }
    if !pool.is_empty() {
        return Err(KernelError::Invalid("the edges form more than one loop".into()));
    }
    Ok(chain)
}

/// Faces of a body copied out as a surface body (faces picked by points on them). Its history is
/// over `[body]`: the faces and their edges are the body's.
pub fn copy_faces(body: &Body, at: &[Vec3]) -> Result<Body> {
    body.require_brep("copy faces")?;
    let faces: Vec<&mt::Face> = body.solid.face_iter().collect();
    let mut chosen = Vec::new();
    for p in at {
        // The face whose mesh passes nearest the point.
        let i = nearest_face(body, *p)?;
        if !chosen.contains(&i) {
            chosen.push(i);
        }
    }
    let copied = surface_body(chosen.into_iter().filter_map(|i| faces.get(i).map(|f| (*f).clone())).collect());
    Ok(rebuilt_body(copied, &[&body.solid], &Rebuild::default()))
}

/// The faces picked by points on them (each once, in the order picked); every face when no
/// point is given.
fn picked_faces(body: &Body, at: &[Vec3]) -> Result<Vec<usize>> {
    if at.len() > 10_000 {
        return Err(KernelError::Invalid("too many faces picked".into()));
    }
    if at.is_empty() {
        return Ok((0..body.face_count()).collect());
    }
    let mut chosen = Vec::new();
    for p in at {
        let i = nearest_face(body, *p)?;
        if !chosen.contains(&i) {
            chosen.push(i);
        }
    }
    Ok(chosen)
}

/// A body's faces taken apart: each picked face (by points on it; every face when none are
/// given) its own surface body, and the faces left, if any, one surface body. A solid comes
/// apart into surfaces. Histories are over `[body]`: each face is the body's.
pub fn unstitch(body: &Body, at: &[Vec3]) -> Result<(Vec<Body>, Option<Body>)> {
    body.require_brep("unstitch")?;
    let chosen = picked_faces(body, at)?;
    let faces: Vec<&mt::Face> = body.solid.face_iter().collect();
    if faces.len() < 2 {
        return Err(KernelError::Invalid("the body has one face: nothing to unstitch".into()));
    }
    let piece = |picked: Vec<mt::Face>| rebuilt_body(surface_body(picked), &[&body.solid], &Rebuild::default());
    let pieces: Vec<Body> = chosen.iter().filter_map(|i| faces.get(*i)).map(|f| piece(vec![(*f).clone()])).collect();
    let rest: Vec<mt::Face> = faces.iter().enumerate().filter(|(i, _)| !chosen.contains(i)).map(|(_, f)| (*f).clone()).collect();
    Ok((pieces, (!rest.is_empty()).then(|| piece(rest))))
}

/// A surface body with the picked faces (by points on them; every face when none are given)
/// turned over: their normals point the other way. A solid is refused (its faces face out).
/// The history is over `[body]`: each face is the body's, turned over or not.
pub fn reverse_faces(body: &Body, at: &[Vec3]) -> Result<Body> {
    body.require_brep("reverse normal")?;
    if !body.is_surface() {
        return Err(KernelError::Invalid("a solid's faces face out: unstitch it into surfaces first".into()));
    }
    let chosen = picked_faces(body, at)?;
    let mut rec = Rebuild::default();
    let faces: Vec<mt::Face> = body
        .solid
        .face_iter()
        .enumerate()
        .map(|(i, f)| {
            if !chosen.contains(&i) {
                return f.clone();
            }
            let turned = f.inverse();
            rec.whole(&turned, f);
            turned
        })
        .collect();
    Ok(rebuilt_body(surface_body(faces), &[&body.solid], &rec))
}

/// A body with some of its faces (indices in its face order) taken away and the gap left open:
/// a surface body of the faces left. Deleting every face is refused. The history is over
/// `[body]`: each face left is the body's.
pub fn remove_faces(body: &Body, faces: &[usize]) -> Result<Body> {
    body.require_brep("Delete Face")?;
    if faces.is_empty() {
        return Err(KernelError::Invalid("no faces given".into()));
    }
    let left: Vec<mt::Face> = body.solid.face_iter().enumerate().filter(|(i, _)| !faces.contains(i)).map(|(_, f)| f.clone()).collect();
    if left.is_empty() {
        return Err(KernelError::Invalid("that would delete every face of the body".into()));
    }
    Ok(rebuilt_body(surface_body(left), &[&body.solid], &Rebuild::default()))
}

/// Index of the face nearest a point (over its mesh triangles).
pub(crate) fn nearest_face(body: &Body, p: Vec3) -> Result<usize> {
    let m = body.tessellate(body.size() * 2e-3)?;
    let mut best: Option<(f64, u32)> = None;
    for (t, f) in m.triangles.iter().zip(&m.tri_face) {
        let Some([a, b, c]) = m.tri(t) else { continue };
        let d = ((a + b + c) / 3.0).dist(p).min(a.dist(p)).min(b.dist(p)).min(c.dist(p));
        if best.is_none_or(|(x, _)| d < x) {
            best = Some((d, *f));
        }
    }
    best.map(|(_, f)| f as usize).ok_or_else(|| KernelError::Invalid("no face there".into()))
}

/// Sew surface bodies along edges that coincide (within `tol`): one body, a solid when the
/// faces close up. Faces are turned so that neighbours agree. The result's history is over
/// `bodies`: each face the one it was sewn from, each edge the edges sewn into it.
/// Whether `q` lies within `tol` of curve `c` (nearest point searched from a coarse sampling).
fn on_curve(c: &mt::Curve, q: mt::Point3, tol: f64) -> bool {
    let (t0, t1) = c.range_tuple();
    let n = 32;
    let Some(ts) = (0..=n)
        .map(|k| t0 + (t1 - t0) * k as f64 / n as f64)
        .min_by(|a, b| c.subs(*a).distance(q).total_cmp(&c.subs(*b).distance(q)))
    else {
        return false;
    };
    // Within the curve's own range: the nearest parameter of an arc's circle can lie on the
    // other half, and two halves of a circle (the same ends) were taken for one edge. 3DCadCraft.
    let (lo, hi) = (t0.min(t1), t0.max(t1));
    c.search_nearest_parameter(q, Some(ts), 100).is_some_and(|t| t >= lo - 1e-9 * (1.0 + hi - lo) && t <= hi + 1e-9 * (1.0 + hi - lo) && c.subs(t).distance(q) <= tol)
}

/// The edges of `faces` (in their own direction, by id) that carry another edge's vertex within
/// `tol` of their inside, cut at those vertices into pieces (the curve cut at its nearest
/// parameter; the pieces end on that vertex).
fn t_junction_pieces(faces: &[mt::Face], tol: f64) -> HashMap<mt::EdgeID, Vec<mt::Edge>> {
    let mut edges: Vec<mt::Edge> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut verts: Vec<mt::Vertex> = Vec::new();
    let mut vseen = std::collections::HashSet::new();
    for f in faces {
        for w in f.absolute_boundaries() {
            for e in w.edge_iter() {
                let abs = if e.orientation() { e.clone() } else { e.inverse() };
                for v in [abs.front(), abs.back()] {
                    if vseen.insert(v.id()) {
                        verts.push(v.clone());
                    }
                }
                if seen.insert(abs.id()) {
                    edges.push(abs);
                }
            }
        }
    }
    let mut out = HashMap::new();
    for e in edges {
        let c = e.curve();
        let (t0, t1) = c.range_tuple();
        // A coarse polyline for the search's start and a quick distance test.
        let n = 32;
        let samples: Vec<(f64, mt::Point3)> = (0..=n).map(|k| {
            let t = t0 + (t1 - t0) * k as f64 / n as f64;
            (t, c.subs(t))
        }).collect();
        let (p0, p1) = (e.front().point(), e.back().point());
        let mut cuts: Vec<(f64, mt::Vertex)> = Vec::new();
        for v in &verts {
            if v.id() == e.front().id() || v.id() == e.back().id() {
                continue;
            }
            let q = v.point();
            if q.distance(p0) <= tol || q.distance(p1) <= tol {
                continue;
            }
            let Some((ts, d)) = samples.iter().map(|(t, p)| (*t, p.distance(q))).min_by(|a, b| a.1.total_cmp(&b.1)) else { continue };
            // Far from every sample by more than a chord's sag allows: not on this edge.
            let chord = samples.windows(2).map(|w| w[0].1.distance(w[1].1)).fold(0.0, f64::max);
            if d > tol + chord {
                continue;
            }
            let Some(t) = c.search_nearest_parameter(q, Some(ts), 100) else { continue };
            if !(t > t0 && t < t1) || c.subs(t).distance(q) > tol {
                continue;
            }
            cuts.push((t, v.clone()));
        }
        if cuts.is_empty() {
            continue;
        }
        cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
        cuts.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-12);
        let mut rest = c;
        let mut start = e.front().clone();
        let mut parts = Vec::with_capacity(cuts.len() + 1);
        for (t, v) in cuts {
            let tail = mt::Cut::cut(&mut rest, t);
            parts.push(mt::Edge::new(&start, &v, rest));
            rest = tail;
            start = v;
        }
        parts.push(mt::Edge::new(&start, e.back(), rest));
        out.insert(e.id(), parts);
    }
    out
}

pub fn stitch(bodies: &[&Body], tol: f64) -> Result<Body> {
    if bodies.is_empty() {
        return Err(KernelError::Invalid("nothing to stitch".into()));
    }
    for b in bodies {
        b.require_brep("stitch")?;
    }
    let size = bodies.iter().map(|b| b.size()).fold(0.0, f64::max);
    let tol = if tol.is_finite() && tol > 0.0 { tol } else { size * 1e-6 + 1e-6 };
    guard("stitch", || {
        let faces: Vec<mt::Face> = bodies.iter().flat_map(|b| b.solid.face_iter().cloned().collect::<Vec<_>>()).collect();
        // T-junctions: an edge with another edge's vertex lying on it (within the tolerance, off
        // its ends) is cut there first, so a long edge meets the pieces of its neighbour edge by
        // edge (3DCadCraft addition).
        let pieces = t_junction_pieces(&faces, tol);
        // Shared vertices by position, shared edges by their ends and middle.
        let mut verts: Vec<mt::Vertex> = Vec::new();
        let mut vmap: HashMap<mt::VertexID, mt::Vertex> = HashMap::new();
        let mut canon = |v: &mt::Vertex, verts: &mut Vec<mt::Vertex>| -> mt::Vertex {
            if let Some(c) = vmap.get(&v.id()) {
                return c.clone();
            }
            let p = v.point();
            let c = match verts.iter().find(|u| u.point().distance(p) <= tol) {
                Some(u) => u.clone(),
                None => {
                    verts.push(v.clone());
                    v.clone()
                }
            };
            vmap.insert(v.id(), c.clone());
            c
        };
        let mut edges: Vec<(mt::Edge, mt::Point3)> = Vec::new();
        let mut out = Vec::with_capacity(faces.len());
        let mut rec = Rebuild::default();
        for f in &faces {
            let mut wires = Vec::new();
            for w in f.absolute_boundaries() {
                let mut nw = Vec::new();
                for e0 in w.edge_iter() {
                    let abs0 = if e0.orientation() { e0.clone() } else { e0.inverse() };
                    let parts = pieces.get(&abs0.id()).cloned().unwrap_or_else(|| vec![abs0.clone()]);
                    let mut run: Vec<mt::Edge> = Vec::with_capacity(parts.len());
                    for abs in parts {
                    let (a, b) = (canon(abs.front(), &mut verts), canon(abs.back(), &mut verts));
                    let c = abs.curve();
                    let (t0, t1) = c.range_tuple();
                    let mid = c.subs(0.5 * (t0 + t1));
                    let found = edges.iter().find_map(|(x, m)| {
                        // Same ends; and the middle of one on the other — by geometry, not by
                        // parameter: two curves along one edge may be parametrised differently
                        // (a loft bridge's own spline against a surface's trim edge).
                        if m.distance(mid) > tol * 10.0 && !on_curve(&x.curve(), mid, tol * 10.0) {
                            return None;
                        }
                        if x.front().id() == a.id() && x.back().id() == b.id() {
                            Some(x.clone())
                        } else if x.front().id() == b.id() && x.back().id() == a.id() {
                            Some(x.inverse())
                        } else {
                            None
                        }
                    });
                    let ne = match found {
                        Some(x) => x,
                        None => {
                            let x = mt::Edge::new_unchecked(&a, &b, c);
                            edges.push((x.clone(), mid));
                            x
                        }
                    };
                    rec.piece(&ne, &abs0);
                    run.push(ne);
                    }
                    if e0.orientation() {
                        nw.extend(run);
                    } else {
                        nw.extend(run.iter().rev().map(mt::Edge::inverse));
                    }
                }
                wires.push(mt::Wire::from(nw));
            }
            let mut nf = mt::Face::new_unchecked(wires, f.surface());
            if !f.orientation() {
                nf.invert();
            }
            rec.whole(&nf, f);
            out.push(nf);
        }
        // Neighbours run their shared edges opposite ways.
        let mut uses: HashMap<mt::EdgeID, Vec<(usize, bool)>> = HashMap::new();
        for (i, f) in out.iter().enumerate() {
            for e in f.boundaries().iter().flat_map(|w| w.edge_iter()) {
                uses.entry(e.id()).or_default().push((i, e.orientation()));
            }
        }
        let mut flip: Vec<Option<bool>> = vec![None; out.len()];
        for s in 0..out.len() {
            if flip[s].is_some() {
                continue;
            }
            flip[s] = Some(false);
            let mut stack = vec![s];
            while let Some(i) = stack.pop() {
                let fl = flip[i].unwrap_or(false);
                let Some(f) = out.get(i) else { continue };
                for e in f.boundaries().iter().flat_map(|w| w.edge_iter()) {
                    let d = e.orientation() != fl;
                    for &(j, dj) in uses.get(&e.id()).into_iter().flatten() {
                        if j != i && flip[j].is_none() {
                            flip[j] = Some(dj == d);
                            stack.push(j);
                        }
                    }
                }
            }
        }
        let mut out: Vec<mt::Face> = out.into_iter().zip(flip).map(|(f, fl)| if fl == Some(true) { f.inverse() } else { f }).collect();
        // A closed result turned to face out here (see `orientation_volume`).
        let shell = mt::Shell::from(out.clone());
        if shell.shell_condition() == mt::ShellCondition::Closed
            && crate::body::orientation_volume(&mt::Solid::new_unchecked(vec![shell])).is_ok_and(|v| v < 0.0)
        {
            out = out.iter().map(mt::Face::inverse).collect();
        }
        let operands: Vec<&mt::Solid> = bodies.iter().map(|b| &*b.solid).collect();
        body_of(out, &operands, &rec)
    })
}

/// How a face moves along its normal: a plane slides, a circular cylinder changes radius.
#[derive(Clone, Copy)]
pub(crate) enum Offset {
    Plane { n: Vec3 },
    Cylinder { o: Vec3, axis: Vec3, r: f64, out: f64 },
}

impl Offset {
    pub(crate) fn of(f: &mt::Face) -> Option<Offset> {
        match crate::splitface::analytic_field(&f.oriented_surface())? {
            crate::splitface::Analytic::Plane { n } => Some(Offset::Plane { n }),
            crate::splitface::Analytic::Cylinder { o, axis, r, out } => Some(Offset::Cylinder { o, axis, r, out }),
        }
    }
    pub(crate) fn map(&self, p: Vec3, d: f64) -> Vec3 {
        match *self {
            Offset::Plane { n } => p + n * d,
            // Scaled about the axis (control points of a NURBS circle stay exact).
            Offset::Cylinder { o, axis, r, out } => {
                let v = p - o;
                let radial = v - axis * v.dot(axis);
                p + radial * (out * d / r)
            }
        }
    }
}

fn map_pt(off: &Offset, p: mt::Point3, d: f64) -> mt::Point3 {
    p3(off.map(from_p3(p), d))
}

/// A curve moved by an offset (control points mapped; exact for lines, and for circles square
/// to a cylinder's axis or in a plane).
pub(crate) fn map_curve(off: &Offset, c: &mt::Curve, d: f64) -> Result<mt::Curve> {
    Ok(match c {
        mt::Curve::Line(l) => mt::Curve::Line(mt::Line(map_pt(off, l.0, d), map_pt(off, l.1, d))),
        mt::Curve::BSplineCurve(b) => {
            let pts: Vec<mt::Point3> = b.control_points().iter().map(|p| map_pt(off, *p, d)).collect();
            mt::Curve::BSplineCurve(mt::BSplineCurve::new(b.knot_vec().clone(), pts))
        }
        mt::Curve::NurbsCurve(n) => {
            let nb = n.non_rationalized();
            let pts: Vec<mt::Vector4> = nb
                .control_points()
                .iter()
                .map(|h| {
                    let w = h.w;
                    let q = map_pt(off, mt::Point3::new(h.x / w, h.y / w, h.z / w), d);
                    mt::Vector4::new(q.x * w, q.y * w, q.z * w, w)
                })
                .collect();
            mt::Curve::NurbsCurve(mt::NurbsCurve::new(mt::BSplineCurve::new(nb.knot_vec().clone(), pts)))
        }
        other => {
            // Anything else: a cubic through moved samples.
            let (t0, t1) = other.range_tuple();
            let pts: Vec<Vec3> = (0..=32).map(|k| off.map(from_p3(other.subs(t0 + (t1 - t0) * k as f64 / 32.0)), d)).collect();
            mt::Curve::BSplineCurve(crate::build::interpolate_cubic(&pts).ok_or_else(|| KernelError::Failed("offset edge".into()))?)
        }
    })
}

/// A surface moved by an offset (control points mapped; exact for planes and NURBS cylinders).
pub(crate) fn map_surface(off: &Offset, s: &mt::Surface, d: f64) -> Result<mt::Surface> {
    Ok(match s {
        mt::Surface::Plane(pl) => {
            let (o, a, b) = (pl.origin(), pl.u_axis(), pl.v_axis());
            let o2 = map_pt(off, o, d);
            mt::Surface::Plane(mt::Plane::new(o2, o2 + a, o2 + b))
        }
        mt::Surface::BSplineSurface(b) => {
            let rows: Vec<Vec<mt::Point3>> = b.control_points().iter().map(|r| r.iter().map(|p| map_pt(off, *p, d)).collect()).collect();
            mt::Surface::BSplineSurface(mt::BSplineSurface::new((b.uknot_vec().clone(), b.vknot_vec().clone()), rows))
        }
        mt::Surface::NurbsSurface(n) => {
            let nb = n.non_rationalized();
            let rows: Vec<Vec<mt::Vector4>> = nb
                .control_points()
                .iter()
                .map(|r| {
                    r.iter()
                        .map(|h| {
                            let w = h.w;
                            let q = map_pt(off, mt::Point3::new(h.x / w, h.y / w, h.z / w), d);
                            mt::Vector4::new(q.x * w, q.y * w, q.z * w, w)
                        })
                        .collect()
                })
                .collect();
            mt::Surface::NurbsSurface(mt::NurbsSurface::new(mt::BSplineSurface::new((nb.uknot_vec().clone(), nb.vknot_vec().clone()), rows)))
        }
        _ => return Err(KernelError::Invalid("not supported yet: thickening this kind of surface".into())),
    })
}

/// How far Thicken's fitted faces may stray from the true offset (mm).
const THICKEN_TOLERANCE: f64 = 0.01;

/// A surface body thickened into a solid: offset along the faces' normals by `thickness`
/// (negative: against them), or by half each way when `symmetric`. Planes and circular
/// cylinders move exactly, other faces onto fitted surfaces (see `surface_offset`); a thickness
/// that would fold a face over is refused. Neighbouring faces must meet smoothly (their offsets
/// agree).
pub fn thicken(body: &Body, thickness: f64, symmetric: bool) -> Result<Body> {
    thicken_with_tolerance(body, thickness, symmetric, THICKEN_TOLERANCE)
}

/// [`thicken`] with the fitting tolerance of free-form faces given (3DCadCraft: Inventor's
/// Allow Approximation tolerance; a doubly curved spline face, such as a revolved egg, often
/// needs more than 0.01 mm). Planes and circular cylinders still move exactly.
pub fn thicken_with_tolerance(body: &Body, thickness: f64, symmetric: bool, tolerance: f64) -> Result<Body> {
    if !(tolerance.is_finite() && (1e-6..=10.0).contains(&tolerance)) {
        return Err(KernelError::Invalid("the thicken tolerance must be 0.000001…10 mm".into()));
    }
    body.require_brep("thicken")?;
    if !(thickness.is_finite() && thickness.abs() > 1e-9) {
        return Err(KernelError::Invalid("thickness must be non-zero".into()));
    }
    let (d0, d1) = if symmetric { (-thickness / 2.0, thickness / 2.0) } else { (0.0, thickness) };
    let (lo, hi) = (d0.min(d1), d0.max(d1));
    let size = body.size();
    let tol = size * 1e-7 + 1e-9;
    guard("thicken", || {
        let faces: Vec<mt::Face> = body.solid.face_iter().cloned().collect();
        if faces.is_empty() {
            return Err(KernelError::Invalid("nothing to thicken".into()));
        }
        // Planes and circular cylinders move exactly; other faces onto surfaces fitted within the
        // offset tolerance (Offset Surface's).
        let offs: Vec<crate::surface_offset::FaceMover> =
            faces.iter().map(|f| crate::surface_offset::FaceMover::new(f, &[lo, hi], tolerance)).collect::<Result<_>>()?;
        let near = |a: Vec3, b: Vec3| a.dist(b) <= tol * 1e3;
        // Each side's copies of every vertex and edge. Faces that move one to the same place share
        // the copy (they meet smoothly there); across a sharp edge each face has its own.
        let mut vcopies: [HashMap<mt::VertexID, Vec<mt::Vertex>>; 2] = [HashMap::new(), HashMap::new()];
        let mut ecopies: [HashMap<mt::EdgeID, Vec<mt::Edge>>; 2] = [HashMap::new(), HashMap::new()];
        // Each face's copies, by side: (face, vertex) → vertex, (face, edge) → edge.
        let mut vof: [HashMap<(usize, mt::VertexID), mt::Vertex>; 2] = [HashMap::new(), HashMap::new()];
        let mut eof: [HashMap<(usize, mt::EdgeID), mt::Edge>; 2] = [HashMap::new(), HashMap::new()];
        // The faces using each edge, in order.
        let mut users: HashMap<mt::EdgeID, Vec<usize>> = HashMap::new();
        let mut abs_edges: HashMap<mt::EdgeID, mt::Edge> = HashMap::new();
        for (fi, (f, off)) in faces.iter().zip(&offs).enumerate() {
            for e in f.absolute_boundaries().iter().flat_map(|w| w.edge_iter()) {
                let abs = if e.orientation() { e.clone() } else { e.inverse() };
                let list = users.entry(abs.id()).or_default();
                if !list.contains(&fi) {
                    list.push(fi);
                }
                abs_edges.entry(abs.id()).or_insert_with(|| abs.clone());
                for (side, d) in [lo, hi].into_iter().enumerate() {
                    for v in [abs.front(), abs.back()] {
                        if vof[side].contains_key(&(fi, v.id())) {
                            continue;
                        }
                        let at = off.point(from_p3(v.point()), d)?;
                        let list = vcopies[side].entry(v.id()).or_default();
                        let copy = match list.iter().find(|c| near(from_p3(c.point()), at)) {
                            Some(c) => c.clone(),
                            None => {
                                let c = mt::Vertex::new(p3(at));
                                list.push(c.clone());
                                c
                            }
                        };
                        vof[side].insert((fi, v.id()), copy);
                    }
                    let (Some(a), Some(b)) = (vof[side].get(&(fi, abs.front().id())).cloned(), vof[side].get(&(fi, abs.back().id())).cloned()) else {
                        continue;
                    };
                    let c = abs.curve();
                    let (t0, t1) = c.range_tuple();
                    let mid = off.point(from_p3(c.subs(0.5 * (t0 + t1))), d)?;
                    let list = ecopies[side].entry(abs.id()).or_default();
                    let shared = list.iter().find(|m| {
                        m.front().id() == a.id() && m.back().id() == b.id() && {
                            let mc = m.curve();
                            let (s0, s1) = mc.range_tuple();
                            near(from_p3(mc.subs(0.5 * (s0 + s1))), mid)
                        }
                    });
                    let copy = match shared {
                        Some(m) => m.clone(),
                        None => {
                            let m = mt::Edge::new_unchecked(&a, &b, off.curve(&c, d)?);
                            list.push(m.clone());
                            m
                        }
                    };
                    eof[side].insert((fi, abs.id()), copy);
                }
            }
        }
        // Lines joining a vertex's copy on one side to its copy on the other, once each.
        let mut risers: HashMap<(mt::VertexID, mt::VertexID), mt::Edge> = HashMap::new();
        let mut riser = |a: &mt::Vertex, b: &mt::Vertex| -> mt::Edge {
            risers.entry((a.id(), b.id())).or_insert_with(|| mt::Edge::new_unchecked(a, b, mt::Curve::Line(mt::Line(a.point(), b.point())))).clone()
        };
        let moved_face = |fi: usize, f: &mt::Face, off: &crate::surface_offset::FaceMover, side: usize, d: f64| -> Result<mt::Face> {
            let wires: Vec<mt::Wire> = f
                .absolute_boundaries()
                .iter()
                .map(|w| {
                    w.edge_iter()
                        .filter_map(|e| {
                            let abs_id = if e.orientation() { e.id() } else { e.inverse().id() };
                            let m = eof[side].get(&(fi, abs_id))?.clone();
                            Some(if e.orientation() { m } else { m.inverse() })
                        })
                        .collect::<Vec<_>>()
                        .into()
                })
                .collect();
            let mut nf = mt::Face::new_unchecked(wires, off.surface(d)?);
            if !f.orientation() {
                nf.invert();
            }
            Ok(nf)
        };
        let mut out = Vec::new();
        // History (one operand, the surface): a copy at no distance is the face itself, a copy
        // further out its offset, a wall is made along the edge it stands on, a corner patch from
        // the sharp edges that meet at its corner.
        let operand = crate::record::Operand::new(body);
        let edge_index = &operand.edges;
        let mut history = Vec::new();
        let copy = |i: usize, d: f64| {
            if d == 0.0 {
                crate::record::from_face(0, i, Change::Kept)
            } else {
                crate::record::generated(vec![FaceOrigin::Offset { operand: 0, face: i }])
            }
        };
        let edge_origin = |id: &mt::EdgeID| -> Result<FaceOrigin> {
            let edge = edge_index.get(id).copied().ok_or_else(|| KernelError::Failed("thicken: an edge outside the surface".into()))?;
            Ok(FaceOrigin::Edge { operand: 0, edge })
        };
        for (i, (f, off)) in faces.iter().zip(&offs).enumerate() {
            // The lower copy faces back, the upper one forward.
            out.push(moved_face(i, f, off, 0, lo)?.inverse());
            out.push(moved_face(i, f, off, 1, hi)?);
            history.extend([copy(i, lo), copy(i, hi)]);
            // Walls along free edges, outward across the edge.
            for w in f.boundaries() {
                for e in w.edge_iter() {
                    let abs_id = if e.orientation() { e.id() } else { e.inverse().id() };
                    if users.get(&abs_id).map_or(0, Vec::len) != 1 {
                        continue;
                    }
                    let (Some(le), Some(he)) = (eof[0].get(&(i, abs_id)), eof[1].get(&(i, abs_id))) else { continue };
                    let (le, he) = if e.orientation() { (le.clone(), he.clone()) } else { (le.inverse(), he.inverse()) };
                    let (sa, sb) = (riser(le.front(), he.front()), riser(le.back(), he.back()));
                    // A → B low, B → B' up, B' → A' high back, A' → A down.
                    let wire: mt::Wire = vec![le.clone(), sb, he.inverse(), sa.inverse()].into();
                    let ruled = mt::builder::homotopy(&le, &he);
                    let surf = ruled.surface();
                    // The wall's outward normal: across the edge, away from the face.
                    let c = le.oriented_curve();
                    let (t0, t1) = c.range_tuple();
                    let tm = 0.5 * (t0 + t1);
                    let p = c.subs(tm);
                    let tangent = c.der(tm);
                    let n =
                        f.oriented_surface().search_nearest_parameter(p, None, 50).map(|(u, v)| f.oriented_surface().normal(u, v)).unwrap_or(tangent);
                    let outward = tangent.cross(n);
                    let sn = surf.search_nearest_parameter(p, None, 50).map(|(u, v)| surf.normal(u, v)).unwrap_or(outward);
                    let wall = if sn.dot(outward) >= 0.0 {
                        mt::Face::new_unchecked(vec![wire], surf)
                    } else {
                        let mut x = mt::Face::new_unchecked(vec![wire.inverse()], surf);
                        x.invert();
                        x
                    };
                    out.push(wall);
                    history.push(crate::record::generated(vec![edge_origin(&abs_id)?]));
                }
            }
        }
        // Sharp edges: where the two faces' copies of an edge part on a side, a wall closes the
        // gap; where they would cross (an inside corner on that side), refused.
        let mut sharp_at: HashMap<mt::VertexID, Vec<mt::EdgeID>> = HashMap::new();
        let mut gaps = Vec::new();
        for (id, fs) in &users {
            let [fa, fb] = fs.as_slice() else { continue };
            let Some(abs) = abs_edges.get(id) else { continue };
            for (side, d) in [lo, hi].into_iter().enumerate() {
                let (Some(ea), Some(eb)) = (eof[side].get(&(*fa, *id)), eof[side].get(&(*fb, *id))) else { continue };
                if ea.id() == eb.id() {
                    continue;
                }
                // Which way the faces bend across the edge: each lies behind the other's normal
                // (convex toward the normals) or in front of it.
                let c = abs.curve();
                let (t0, t1) = c.range_tuple();
                let p = from_p3(c.subs(0.5 * (t0 + t1)));
                let normal = |fi: usize| -> Option<Vec3> {
                    let s = faces.get(fi)?.oriented_surface();
                    let (u, v) = s.search_nearest_parameter(p3(p), None, 50)?;
                    Some(from_p3(mt::Point3::new(0.0, 0.0, 0.0) + s.normal(u, v)))
                };
                let centre = |fi: usize| -> Option<Vec3> {
                    let pts: Vec<Vec3> =
                        faces.get(fi)?.absolute_boundaries().iter().flat_map(|w| w.vertex_iter().map(|v| from_p3(v.point()))).collect();
                    Some(pts.iter().fold(Vec3::ZERO, |a, q| a + *q) * (1.0 / pts.len().max(1) as f64))
                };
                let (Some(na), Some(ca), Some(cb)) = (normal(*fa), centre(*fa), centre(*fb)) else {
                    return Err(KernelError::Failed("thicken: the faces at a sharp edge".into()));
                };
                // Convex toward the normals: the other face falls away behind this one's normal.
                let convex = na.dot(cb - p) < 0.0 && normal(*fb).is_some_and(|nb| nb.dot(ca - p) < 0.0);
                if convex != (d > 0.0) {
                    return Err(KernelError::Invalid(
                        "not supported yet: thickening into an inside corner across a sharp edge (try the other side, or symmetric off)".into(),
                    ));
                }
                gaps.push((*id, side, ea.clone(), eb.clone()));
                for v in [abs.front(), abs.back()] {
                    let list = sharp_at.entry(v.id()).or_default();
                    if !list.contains(id) {
                        list.push(*id);
                    }
                }
            }
        }
        // Each gap wall runs along one face's copy of the edge and back along the other's, joined
        // at the ends by lines between the two copies of each end vertex. It is turned so that it
        // crosses the first face's copy the other way from that face.
        let mut links: HashMap<(mt::VertexID, mt::VertexID), mt::Edge> = HashMap::new();
        let mut link = |a: &mt::Vertex, b: &mt::Vertex| -> mt::Edge {
            if let Some(e) = links.get(&(b.id(), a.id())) {
                return e.inverse();
            }
            links.entry((a.id(), b.id())).or_insert_with(|| mt::Edge::new_unchecked(a, b, mt::Curve::Line(mt::Line(a.point(), b.point())))).clone()
        };
        let used_forward = |faces: &[mt::Face], e: &mt::Edge| -> Option<bool> {
            faces
                .iter()
                .flat_map(|f| f.boundaries().into_iter().flat_map(|w| w.edge_iter().cloned().collect::<Vec<_>>()))
                .find(|x| x.id() == e.id())
                .map(|x| x.front().id() == e.front().id())
        };
        for (id, _side, ea, eb) in gaps {
            let wire: mt::Wire = vec![ea.clone(), link(ea.back(), eb.back()), eb.inverse(), link(eb.front(), ea.front())].into();
            let surf = mt::builder::homotopy(&ea, &eb).surface();
            let wall = mt::Face::new_unchecked(vec![wire], surf);
            let wall = if used_forward(&out, &ea).unwrap_or(false) { wall.inverse() } else { wall };
            out.push(wall);
            history.push(crate::record::generated(vec![edge_origin(&id)?]));
        }
        // Corners: whatever is left open is closed by plane patches (where three faces meet at a
        // sharp corner, a triangle), each from the sharp edges meeting there.
        let open = mt::Shell::from(out.clone()).extract_boundaries();
        for w in open {
            let patch = mt::builder::try_attach_plane(&[w.inverse()])
                .map_err(|_| KernelError::Invalid("not supported yet: thickening a corner where the gap does not close flat".into()))?;
            let mut origins = Vec::new();
            for v in w.vertex_iter() {
                for (vid, list) in &sharp_at {
                    let at = vcopies.iter().flat_map(|m| m.get(vid).into_iter().flatten()).any(|c| c.id() == v.id());
                    if at {
                        for e in list {
                            let o = edge_origin(e)?;
                            if !origins.contains(&o) {
                                origins.push(o);
                            }
                        }
                    }
                }
            }
            if origins.is_empty() {
                return Err(KernelError::Failed("the thickened surface does not close".into()));
            }
            out.push(patch);
            history.push(crate::record::generated(origins));
        }
        let shell = mt::Shell::from(out);
        if shell.shell_condition() != mt::ShellCondition::Closed {
            return Err(KernelError::Failed("the thickened surface does not close".into()));
        }
        // The edges copied at no distance are the surface's own edges, rebuilt.
        let mut copies = HashMap::new();
        for (side, d) in [lo, hi].into_iter().enumerate() {
            if d != 0.0 {
                continue;
            }
            for (id, list) in &ecopies[side] {
                if let Some(&edge) = edge_index.get(id) {
                    for c in list {
                        copies.insert(c.id(), EdgeOrigin::Edge { operand: 0, edge });
                    }
                }
            }
        }
        crate::record::attach(Body::new(mt::Solid::new_unchecked(vec![shell]))?, &[operand], Some(history), copies)
    })
}

/// A surface body trimmed by a tool: its faces are cut where the tool meets them and the
/// pieces on the side of `keep` are kept. The result's history is over `[body]`.
pub fn trim(body: &Body, tool: &crate::SplitTool, keep: Vec3) -> Result<Body> {
    body.require_brep("trim")?;
    let total = body.solid.face_iter().count();
    let all: Vec<usize> = (0..total).collect();
    let (split, _) = crate::splitface::split_any(body, &all, tool)?;
    let side = crate::splitface::side_of_point(tool, keep)?;
    guard("trim", || {
        let mut kept = Vec::new();
        for f in split.solid.face_iter() {
            let x = crate::splitface::face_side(f, tool, split.size())?;
            if (x >= 0.0) == (side >= 0.0) {
                kept.push(f.clone());
            }
        }
        if kept.is_empty() {
            return Err(KernelError::Invalid("nothing is left on that side of the tool".into()));
        }
        // The kept pieces are the split's own; its history carries them back to `body`.
        let trimmed = rebuilt_body(surface_body(kept), &[&split.solid], &Rebuild::default());
        Ok(crate::record::after(trimmed, &split, &crate::record::counts(&[body])))
    })
}

/// A planar surface extended: the chosen straight edges (picked by points on them) move
/// outward in the face's plane by `distance`, their neighbours lengthened to meet them. The
/// result's history is over `[body]`: the face and each edge the one it was moved from.
pub fn extend(body: &Body, at: &[Vec3], distance: f64) -> Result<Body> {
    body.require_brep("extend")?;
    if !(distance.is_finite() && distance > 0.0) {
        return Err(KernelError::Invalid("the extension distance must be positive".into()));
    }
    let faces: Vec<mt::Face> = body.solid.face_iter().cloned().collect();
    let [face] = faces.as_slice() else { return Err(KernelError::Invalid("not supported yet: extending a surface of more than one face".into())) };
    let Some(crate::splitface::Analytic::Plane { n }) = crate::splitface::analytic_field(&face.oriented_surface()) else {
        return Err(KernelError::Invalid("not supported yet: extending a surface that is not planar".into()));
    };
    let chosen: Vec<mt::EdgeID> = at
        .iter()
        .map(|p| edge_near(body, *p).map(|e| e.id()).ok_or_else(|| KernelError::Invalid(format!("no edge at {p:?}"))))
        .collect::<Result<_>>()?;
    guard("extend", || {
        let bounds = face.boundaries();
        let [outer] = bounds.as_slice() else { return Err(KernelError::Invalid("not supported yet: extending a surface with holes".into())) };
        let edges: Vec<mt::Edge> = outer.edge_iter().cloned().collect();
        let m = edges.len();
        // Each edge as a line (point, direction) moved outward when chosen.
        let mut lines = Vec::with_capacity(m);
        for e in &edges {
            let (a, b) = (from_p3(e.front().point()), from_p3(e.back().point()));
            if !matches!(e.curve(), mt::Curve::Line(_)) {
                return Err(KernelError::Invalid("not supported yet: extending a surface with curved edges".into()));
            }
            let d = (b - a).normalized().ok_or_else(|| KernelError::Invalid("a zero-length edge".into()))?;
            // Outward: right of the edge seen along the normal (the face is on its left).
            let out = d.cross(n);
            let shift = if chosen.contains(&e.id()) || chosen.contains(&e.inverse().id()) { distance } else { 0.0 };
            lines.push((a + out * shift, d));
        }
        // Corners: where consecutive lines meet.
        let meet = |(p, d): (Vec3, Vec3), (q, e): (Vec3, Vec3)| -> Option<Vec3> {
            let w = q - p;
            let c = d.cross(e);
            let cc = c.dot(c);
            if cc < 1e-24 {
                return None;
            }
            Some(p + d * (w.cross(e).dot(c) / cc))
        };
        let mut corners = Vec::with_capacity(m);
        for i in 0..m {
            let (Some(prev), Some(cur)) = (lines.get((i + m - 1) % m), lines.get(i)) else { continue };
            let p = meet(*prev, *cur).unwrap_or(cur.0);
            corners.push(mt::Vertex::new(p3(p)));
        }
        let mut rec = Rebuild::default();
        let wire: mt::Wire = (0..m)
            .filter_map(|i| {
                let (a, b, old) = (corners.get(i)?, corners.get((i + 1) % m)?, edges.get(i)?);
                let e = mt::Edge::new_unchecked(a, b, mt::Curve::Line(mt::Line(a.point(), b.point())));
                rec.piece(&e, old);
                Some(e)
            })
            .collect::<Vec<_>>()
            .into();
        let mut f = mt::builder::try_attach_plane(&[wire]).map_err(|e| KernelError::Failed(format!("extend: {e}")))?;
        if f.oriented_surface().normal(0.0, 0.0).dot(crate::body::v3(n)) < 0.0 {
            f = f.inverse();
        }
        rec.face(&f, face);
        Ok(rebuilt_body(surface_body(vec![f]), &[&body.solid], &rec))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{box_solid, cylinder, measure};
    use solidcraft_geom::{Loop2, Vec2};

    fn rel(a: f64, b: f64) -> f64 {
        (a - b).abs() / b.abs().max(1e-9)
    }

    /// Walls swept from an open line, an open arc and a closed square: areas are length × height,
    /// no caps, and stitching two patches onto the square's walls closes the box.
    #[test]
    fn extrude_sheet_of_open_and_closed_chains() {
        let line = vec![Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(10.0, 0.0) }];
        let s = extrude_sheet(&Plane::XY, std::slice::from_ref(&line), 0.0, 5.0).unwrap();
        assert!(s.is_surface() && s.face_count() == 1);
        assert!(rel(measure(&s).unwrap().area, 50.0) < 1e-9);
        // A quarter arc of radius 4, swept from -1 to 2: area r θ h.
        let arc = vec![Seg2::Arc { center: Vec2::new(0.0, 0.0), radius: 4.0, start: 0.0, sweep: std::f64::consts::FRAC_PI_2 }];
        let a = measure(&extrude_sheet(&Plane::XY, &[arc], 2.0, -1.0).unwrap()).unwrap().area;
        let want = 4.0 * std::f64::consts::FRAC_PI_2 * 3.0;
        assert!(rel(a, want) < 1e-3, "{a} vs {want}"); // (measured on meshes)
        // A line then a line back up: one chain, two faces.
        let bent = vec![line[0], Seg2::Line { a: Vec2::new(10.0, 0.0), b: Vec2::new(10.0, 6.0) }];
        assert_eq!(extrude_sheet(&Plane::XY, &[bent], 0.0, 1.0).unwrap().face_count(), 2);
        // A closed square: four walls, open at both ends; with a patch on each end, a solid box.
        let sq = Region2 {
            outer: Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)]),
            holes: vec![],
        };
        let walls = extrude_sheet(&Plane::XY, std::slice::from_ref(&sq.outer.segs), 0.0, 4.0).unwrap();
        assert!(walls.is_surface() && walls.face_count() == 4);
        assert!(rel(measure(&walls).unwrap().area, 160.0) < 1e-9);
        let (bottom, top) = (patch_region(&Plane::XY, &sq).unwrap(), patch_region(&Plane::XY.offset(4.0), &sq).unwrap());
        let closed = stitch(&[&walls, &bottom, &top], 1e-6).unwrap();
        assert!(!closed.is_surface());
        assert!(rel(measure(&closed).unwrap().volume, 400.0) < 1e-9);
    }

    /// Sheets revolved about the sketch's y axis, against their analytic areas: a cylinder (and a
    /// quarter of one, started a quarter turn round), cones with the apex at either end, a sphere
    /// from a half circle, and a square's ring (Pappus: 2π × centroid distance × length).
    #[test]
    fn revolve_sheet_areas() {
        use std::f64::consts::{FRAC_PI_2, PI};
        let sheet = |chain: Vec<Seg2>, start: f64, sweep: f64| {
            revolve_sheet(&Plane::XY, &[chain], Vec2::new(0.0, 0.0), Vec2::new(0.0, 1.0), start, sweep).unwrap()
        };
        let rev = |chain: Vec<Seg2>, start: f64, sweep: f64| measure(&sheet(chain, start, sweep)).unwrap().area;
        let line = |a: (f64, f64), b: (f64, f64)| Seg2::Line { a: Vec2::new(a.0, a.1), b: Vec2::new(b.0, b.1) };
        let close = |a: f64, want: f64| assert!(rel(a, want) < 2e-3, "{a} vs {want}"); // (measured on meshes)
        close(rev(vec![line((3.0, 0.0), (3.0, 4.0))], 0.0, 2.0 * PI), 2.0 * PI * 3.0 * 4.0);
        close(rev(vec![line((3.0, 0.0), (3.0, 4.0))], FRAC_PI_2, FRAC_PI_2), FRAC_PI_2 * 3.0 * 4.0);
        close(rev(vec![line((0.0, 0.0), (3.0, 4.0))], 0.0, 2.0 * PI), PI * 3.0 * 5.0);
        close(rev(vec![line((3.0, 4.0), (0.0, 0.0))], 0.0, 2.0 * PI), PI * 3.0 * 5.0);
        assert!(sheet(vec![line((3.0, 0.0), (3.0, 4.0))], 0.0, 1.0).is_surface());
        // Closing shells are solids, outward: a sphere from a half circle, a square's ring.
        let half = Seg2::Arc { center: Vec2::new(0.0, 0.0), radius: 2.0, start: -FRAC_PI_2, sweep: PI };
        let ball = measure(&sheet(vec![half], 0.0, 2.0 * PI)).unwrap();
        close(ball.area, 4.0 * PI * 4.0);
        close(ball.volume, 4.0 / 3.0 * PI * 8.0);
        assert!(sheet(vec![half], 0.0, PI).is_surface());
        let sq = Loop2::polygon(&[Vec2::new(2.0, 0.0), Vec2::new(3.0, 0.0), Vec2::new(3.0, 1.0), Vec2::new(2.0, 1.0)]);
        let ring = measure(&sheet(sq.segs.clone(), 0.0, 2.0 * PI)).unwrap();
        close(ring.area, 2.0 * PI * (2.0 + 3.0 + 2.5 + 2.5));
        close(ring.volume, 2.0 * PI * 2.5);
    }

    #[test]
    fn revolve_sheet_refuses_bad_input() {
        let rev = |chain: Vec<Seg2>, sweep: f64| revolve_sheet(&Plane::XY, &[chain], Vec2::new(0.0, 0.0), Vec2::new(0.0, 1.0), 0.0, sweep);
        let line = |a: (f64, f64), b: (f64, f64)| Seg2::Line { a: Vec2::new(a.0, a.1), b: Vec2::new(b.0, b.1) };
        assert!(rev(vec![line((-1.0, 0.0), (1.0, 1.0))], 1.0).is_err(), "crosses the axis");
        assert!(rev(vec![line((0.0, 0.0), (0.0, 1.0))], 1.0).is_err(), "on the axis");
        assert!(rev(vec![line((1.0, 0.0), (1.0, 1.0))], 0.0).is_err(), "zero angle");
        assert!(rev(vec![line((1.0, 0.0), (1.0, 1.0))], 7.0).is_err(), "more than a turn");
        assert!(rev(vec![line((1.0, 0.0), (1.0, 1.0))], f64::NAN).is_err());
        let touching = Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0), Vec2::new(1.0, 1.0), Vec2::new(0.0, 1.0)]);
        assert!(rev(touching.segs, 1.0).is_err(), "a closed curve touching the axis");
        assert!(revolve_sheet(&Plane::XY, &[], Vec2::new(0.0, 0.0), Vec2::new(0.0, 1.0), 0.0, 1.0).is_err());
        assert!(revolve_sheet(&Plane::XY, &[vec![line((1.0, 0.0), (1.0, 1.0))]], Vec2::new(0.0, 0.0), Vec2::new(0.0, 0.0), 0.0, 1.0).is_err());
    }

    /// A line along x swept up z: straight (length × height), then on round a quarter turn
    /// (a quarter cylinder of radius 5 about an axis along x: 10 × 5π/2, the arc taking up the
    /// line's far edge), and along a path with a corner (mitred ruled walls, 10 × both legs); a
    /// closed square's four walls; a 3D point path.
    #[test]
    fn sweep_sheet_areas() {
        use crate::PathSeg;
        use std::f64::consts::FRAC_PI_2;
        let line = vec![Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(10.0, 0.0) }];
        let up = PathSeg::Line { a: Vec3::ZERO, b: Vec3::new(0.0, 0.0, 5.0) };
        let area = |b: &Body| measure(b).unwrap().area;
        let straight = sweep_sheet(&Plane::XY, std::slice::from_ref(&line), &[up]).unwrap();
        assert!(straight.is_surface() && rel(area(&straight), 50.0) < 1e-9);
        let bend = PathSeg::Arc { a: Vec3::new(0.0, 0.0, 5.0), center: Vec3::new(0.0, 5.0, 5.0), axis: -Vec3::X, angle: FRAC_PI_2 };
        let smooth = sweep_sheet(&Plane::XY, std::slice::from_ref(&line), &[up, bend]).unwrap();
        assert!(smooth.is_surface());
        let want = 50.0 + 10.0 * 5.0 * FRAC_PI_2;
        assert!(rel(area(&smooth), want) < 2e-3, "{} vs {want}", area(&smooth)); // (measured on meshes)
        let over = PathSeg::Line { a: Vec3::new(0.0, 0.0, 5.0), b: Vec3::new(0.0, 5.0, 5.0) };
        let cornered = sweep_sheet(&Plane::XY, std::slice::from_ref(&line), &[up, over]).unwrap();
        assert!(cornered.is_surface() && rel(area(&cornered), 100.0) < 1e-9, "{}", area(&cornered));
        let sq = Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(4.0, 0.0), Vec2::new(4.0, 4.0), Vec2::new(0.0, 4.0)]);
        let walls = sweep_sheet(&Plane::XY, std::slice::from_ref(&sq.segs), &[up]).unwrap();
        assert!(walls.is_surface() && walls.face_count() == 4 && rel(area(&walls), 80.0) < 1e-9);
        let pts: Vec<Vec3> = (0..=10).map(|i| Vec3::new(0.0, 0.0, i as f64)).collect();
        assert!(rel(area(&sweep_sheet_points(&Plane::XY, std::slice::from_ref(&line), &pts).unwrap()), 100.0) < 1e-6);
        // A smooth point path: a quarter circle of radius 30 in the YZ plane. One smooth wall (not
        // forty ruled strips), a piece of cylinder: 10 × 15π.
        let arc: Vec<Vec3> = (0..=40)
            .map(|i| {
                let t = i as f64 / 40.0 * FRAC_PI_2;
                Vec3::new(0.0, 30.0 * (1.0 - t.cos()), 30.0 * t.sin())
            })
            .collect();
        let curved = sweep_sheet_points(&Plane::XY, std::slice::from_ref(&line), &arc).unwrap();
        assert_eq!(curved.face_count(), 1, "smooth wall");
        let want = 10.0 * 30.0 * FRAC_PI_2;
        assert!(rel(area(&curved), want) < 2e-3, "{} vs {want}", area(&curved)); // (measured on meshes)
        // A bent chain along the same path: two smooth walls joined by one shared seam edge.
        let bent = vec![line[0], Seg2::Line { a: Vec2::new(10.0, 0.0), b: Vec2::new(10.0, 5.0) }];
        let two = sweep_sheet_points(&Plane::XY, &[bent], &arc).unwrap();
        assert_eq!(two.face_count(), 2);
        let edges: std::collections::HashSet<_> = two.solid.edge_iter().map(|e| e.id()).collect();
        assert_eq!(edges.len(), 7, "the walls share their seam");
    }

    #[test]
    fn sweep_sheet_refuses_bad_input() {
        use crate::PathSeg;
        let line = vec![Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(10.0, 0.0) }];
        let up = PathSeg::Line { a: Vec3::ZERO, b: Vec3::new(0.0, 0.0, 5.0) };
        assert!(sweep_sheet(&Plane::XY, std::slice::from_ref(&line), &[]).is_err());
        assert!(sweep_sheet(&Plane::XY, &[], &[up]).is_err());
        assert!(sweep_sheet(&Plane::XY, std::slice::from_ref(&line), &[PathSeg::Line { a: Vec3::ZERO, b: Vec3::ZERO }]).is_err());
        let no_axis = PathSeg::Arc { a: Vec3::ZERO, center: Vec3::new(0.0, 5.0, 0.0), axis: Vec3::ZERO, angle: 1.0 };
        assert!(sweep_sheet(&Plane::XY, std::slice::from_ref(&line), &[no_axis]).is_err());
        assert!(sweep_sheet_points(&Plane::XY, std::slice::from_ref(&line), &[Vec3::ZERO]).is_err());
        assert!(sweep_sheet_points(&Plane::XY, std::slice::from_ref(&line), &[Vec3::ZERO, Vec3::new(f64::NAN, 0.0, 0.0)]).is_err());
    }

    /// Sheets lofted between open lines and closed loops: a ruled strip (even when the second
    /// line runs the other way, or is in two pieces), a smooth strip through three, the walls
    /// between two squares, and a cylinder through three circles.
    #[test]
    fn loft_sheet_areas() {
        use std::f64::consts::PI;
        let at = |z: f64| Plane::XY.offset(z);
        let line = |a: f64, b: f64| Seg2::Line { a: Vec2::new(a, 0.0), b: Vec2::new(b, 0.0) };
        let area = |b: &Body| measure(b).unwrap().area;
        let strip = loft_sheet(&[(at(0.0), vec![line(0.0, 10.0)]), (at(5.0), vec![line(0.0, 10.0)])]).unwrap();
        assert!(strip.is_surface() && rel(area(&strip), 50.0) < 1e-6, "{}", area(&strip));
        let turned = loft_sheet(&[(at(0.0), vec![line(0.0, 10.0)]), (at(5.0), vec![line(10.0, 0.0)])]).unwrap();
        assert!(rel(area(&turned), 50.0) < 1e-6, "not twisted: {}", area(&turned));
        let pieces = loft_sheet(&[(at(0.0), vec![line(0.0, 10.0)]), (at(5.0), vec![line(0.0, 5.0), line(5.0, 10.0)])]).unwrap();
        assert!(rel(area(&pieces), 50.0) < 1e-6 && pieces.face_count() == 2, "{}", area(&pieces));
        let three = loft_sheet(&[(at(0.0), vec![line(0.0, 10.0)]), (at(5.0), vec![line(0.0, 10.0)]), (at(10.0), vec![line(0.0, 10.0)])]).unwrap();
        assert!(rel(area(&three), 100.0) < 1e-3, "{}", area(&three)); // (measured on meshes)
        let sq = Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)]);
        let walls = loft_sheet(&[(at(0.0), sq.segs.clone()), (at(4.0), sq.segs.clone())]).unwrap();
        assert!(walls.is_surface() && walls.face_count() == 4 && rel(area(&walls), 160.0) < 1e-6, "{}", area(&walls));
        let circle = Loop2::circle(Vec2::ZERO, 2.0).segs;
        let tube = loft_sheet(&[(at(0.0), circle.clone()), (at(5.0), circle.clone()), (at(10.0), circle)]).unwrap();
        assert!(tube.is_surface());
        let want = 2.0 * PI * 2.0 * 10.0;
        assert!(rel(area(&tube), want) < 2e-3, "{} vs {want}", area(&tube)); // (measured on meshes)
    }

    #[test]
    fn loft_sheet_refuses_bad_input() {
        let at = |z: f64| Plane::XY.offset(z);
        let line = vec![Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(10.0, 0.0) }];
        let sq = Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0), Vec2::new(1.0, 1.0)]).segs;
        assert!(loft_sheet(&[(at(0.0), line.clone())]).is_err(), "one section");
        assert!(loft_sheet(&[(at(0.0), line.clone()), (at(1.0), sq)]).is_err(), "open and closed");
        assert!(loft_sheet(&[(at(0.0), line.clone()), (at(1.0), vec![])]).is_err(), "an empty section");
        let gap = vec![line[0], Seg2::Line { a: Vec2::new(11.0, 0.0), b: Vec2::new(12.0, 0.0) }];
        assert!(loft_sheet(&[(at(0.0), line.clone()), (at(1.0), gap)]).is_err(), "a section with a gap");
        let nan = vec![Seg2::Line { a: Vec2::new(f64::NAN, 0.0), b: Vec2::new(1.0, 0.0) }];
        assert!(loft_sheet(&[(at(0.0), line), (at(1.0), nan)]).is_err());
    }

    #[test]
    fn extrude_sheet_refuses_bad_input() {
        let line = vec![Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(10.0, 0.0) }];
        assert!(extrude_sheet(&Plane::XY, std::slice::from_ref(&line), 0.0, 0.0).is_err());
        assert!(extrude_sheet(&Plane::XY, std::slice::from_ref(&line), 0.0, f64::NAN).is_err());
        assert!(extrude_sheet(&Plane::XY, &[], 0.0, 1.0).is_err());
        assert!(extrude_sheet(&Plane::XY, &[vec![]], 0.0, 1.0).is_err());
        let gap = vec![line[0], Seg2::Line { a: Vec2::new(11.0, 0.0), b: Vec2::new(12.0, 0.0) }];
        assert!(extrude_sheet(&Plane::XY, &[gap], 0.0, 1.0).is_err());
        let nan = vec![Seg2::Line { a: Vec2::new(0.0, f64::NAN), b: Vec2::new(1.0, 0.0) }];
        assert!(extrude_sheet(&Plane::XY, &[nan], 0.0, 1.0).is_err());
    }

    /// A box taken apart into its six faces (each a 10 × 10 surface) and stitched back into
    /// the same solid; two picked faces come off on their own, the other four stay together;
    /// a one-face body has nothing to take apart.
    #[test]
    fn unstitch_a_box_and_stitch_it_back() {
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
        let (pieces, rest) = unstitch(&b, &[]).unwrap();
        assert!(rest.is_none() && pieces.len() == 6);
        for p in &pieces {
            assert!(p.is_surface() && p.face_count() == 1 && rel(measure(p).unwrap().area, 100.0) < 1e-9);
        }
        let back = stitch(&pieces.iter().collect::<Vec<_>>(), 1e-6).unwrap();
        assert!(!back.is_surface() && rel(measure(&back).unwrap().volume, 1000.0) < 1e-9);
        let (two, rest) = unstitch(&b, &[Vec3::new(5.0, 5.0, 10.0), Vec3::new(5.0, 5.0, 0.0)]).unwrap();
        let rest = rest.unwrap();
        assert!(two.len() == 2 && rest.is_surface() && rest.face_count() == 4);
        assert!(unstitch(&two[0], &[]).is_err());
    }

    /// A patch thickened along its normal grows up; turned over, the same thickness grows down.
    /// A solid's faces are not turned over.
    #[test]
    fn reverse_faces_turns_the_thicken_side() {
        let r = Region2 {
            outer: Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)]),
            holes: vec![],
        };
        let s = patch_region(&Plane::XY, &r).unwrap();
        let up = measure(&thicken(&s, 2.0, false).unwrap()).unwrap().bbox;
        let turned = reverse_faces(&s, &[]).unwrap();
        assert!(turned.is_surface() && turned.face_count() == 1);
        let down = measure(&thicken(&turned, 2.0, false).unwrap()).unwrap().bbox;
        assert!((up.min.z - down.min.z - 2.0).abs() < 1e-9 && (up.max.z - down.max.z - 2.0).abs() < 1e-9, "{up:?} vs {down:?}");
        assert!(reverse_faces(&box_solid(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)).unwrap(), &[]).is_err());
    }

    /// A box's top taken away without healing: a five-face surface; patching the opening and
    /// stitching closes the same box. Every face, or none, is refused.
    #[test]
    fn remove_faces_leaves_the_gap_open() {
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
        let top = nearest_face(&b, Vec3::new(5.0, 5.0, 10.0)).unwrap();
        let open = remove_faces(&b, &[top]).unwrap();
        assert!(open.is_surface() && open.face_count() == 5);
        assert!(rel(measure(&open).unwrap().area, 500.0) < 1e-9);
        let lid = patch_edges(&open, &[Vec3::new(5.0, 0.0, 10.0), Vec3::new(10.0, 5.0, 10.0), Vec3::new(5.0, 10.0, 10.0), Vec3::new(0.0, 5.0, 10.0)])
            .unwrap();
        let back = stitch(&[&open, &lid], 1e-6).unwrap();
        assert!(!back.is_surface() && rel(measure(&back).unwrap().volume, 1000.0) < 1e-9);
        assert!(remove_faces(&b, &(0..6).collect::<Vec<_>>()).is_err());
        assert!(remove_faces(&b, &[]).is_err());
    }

    /// Faces that are not planes or cylinders thicken onto fitted surfaces. A sixth of a sphere
    /// (one of its six patches) thickened out by 1 is a sixth of the shell between radii 10 and
    /// 11; a torus patch (a symmetric share of the torus, by its area) thickened out by 1 is that
    /// share of the shell 2π²R((r+1)² − r²); thickened in past the sphere's centre, refused.
    #[test]
    fn thicken_curved_faces_onto_fitted_surfaces() {
        use crate::{sphere, torus};
        use std::f64::consts::PI;
        let ball = sphere(Vec3::ZERO, 10.0).unwrap();
        let patch = copy_faces(&ball, &[Vec3::new(0.0, 0.0, 10.0)]).unwrap();
        assert!(rel(measure(&patch).unwrap().area, 4.0 * PI * 100.0 / 6.0) < 2e-3, "one sixth");
        let shell = thicken(&patch, 1.0, false).unwrap();
        assert!(!shell.is_surface());
        let want = 4.0 / 3.0 * PI * (1331.0 - 1000.0) / 6.0;
        let v = measure(&shell).unwrap().volume;
        assert!(rel(v, want) < 3e-3, "{v} vs {want}"); // (measured on meshes)
        assert!(thicken(&patch, -10.0, false).is_err());
        // A torus patch (R 20, r 5): between the tube radii r and r + 1 over its own angles, θ round
        // the tube and φ round the axis (read off its mesh): the volume is
        // Δφ·[R((r+1)² − r²)/2·Δθ + ((r+1)³ − r³)/3·(sin θ1 − sin θ0)].
        let ring = torus(Vec3::ZERO, 20.0, 5.0).unwrap();
        let face = copy_faces(&ring, &[Vec3::new(25.0, 0.0, 0.0)]).unwrap();
        let mesh = face.tessellate(0.01).unwrap();
        let angles: Vec<(f64, f64)> = mesh.positions.iter().map(|p| (p.z.atan2((p.x * p.x + p.y * p.y).sqrt() - 20.0), p.y.atan2(p.x))).collect();
        let span = |f: &dyn Fn(&(f64, f64)) -> f64| {
            let (lo, hi) = angles.iter().map(f).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| (a.min(x), b.max(x)));
            (lo, hi)
        };
        let ((t0, t1), (p0, p1)) = (span(&|a| a.0), span(&|a| a.1));
        assert!(t1 - t0 < PI && p1 - p0 < PI, "a patch within half a turn each way: {t0}..{t1}, {p0}..{p1}");
        let (r, rr, big) = (5.0f64, 6.0f64, 20.0);
        let want = (p1 - p0) * (big * (rr * rr - r * r) / 2.0 * (t1 - t0) + (rr.powi(3) - r.powi(3)) / 3.0 * (t1.sin() - t0.sin()));
        let v = measure(&thicken(&face, 1.0, false).unwrap()).unwrap().volume;
        assert!(rel(v, want) < 3e-3, "{v} vs {want}"); // (measured on meshes)
    }

    /// Three faces of a box meeting at a corner (sharp edges), thickened out by 1: three slabs,
    /// a wedge along each sharp edge and a corner tetrahedron, 3·100 + 3·(½·1²·10) + 1/6; a closed
    /// solid that every check accepts. Thickened into the inside corner, refused.
    #[test]
    fn thicken_a_box_corner_across_its_sharp_edges() {
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
        let corner = copy_faces(&b, &[Vec3::new(5.0, 5.0, 10.0), Vec3::new(10.0, 5.0, 5.0), Vec3::new(5.0, 10.0, 5.0)]).unwrap();
        assert!(corner.is_surface() && corner.face_count() == 3);
        let t = thicken(&corner, 1.0, false).unwrap();
        assert!(!t.is_surface());
        let want = 300.0 + 15.0 + 1.0 / 6.0;
        let v = measure(&t).unwrap().volume;
        assert!(rel(v, want) < 1e-9, "{v} vs {want}");
        assert!(Body::new(t.solid.as_ref().clone()).is_ok());
        assert!(thicken(&corner, -1.0, false).is_err());
        assert!(thicken(&corner, 1.0, true).is_err(), "half of it goes into the inside corner");
    }

    /// A saddle B-spline patch thickened by 1.5 holds the volume its offsets sweep: by Steiner,
    /// the offset's area is quadratic in the distance, so Simpson's rule over sampled offsets
    /// (Offset Surface at 0, ¾ and 1.5) is exact.
    #[test]
    fn thicken_a_bspline_patch_by_its_sampled_offsets() {
        let b = box_solid(Vec3::new(-14.0, -14.0, 0.0), Vec3::new(14.0, 14.0, 20.0)).unwrap();
        let tool = crate::cylinder(Vec3::new(0.0, -60.0, 50.0), Vec3::Y, 40.0, 120.0).unwrap();
        let cup = crate::boolean(&b, &tool, crate::BoolOp::Cut).unwrap().unwrap();
        let z = |x: f64| 50.0 - (1600.0 - x * x).sqrt();
        let rim = [Vec3::new(14.0, 0.0, z(14.0)), Vec3::new(0.0, 14.0, z(0.0)), Vec3::new(-14.0, 0.0, z(-14.0)), Vec3::new(0.0, -14.0, z(0.0))];
        let cover = patch_edges(&cup, &rim).unwrap();
        let t = 1.5;
        let area = |d: f64| measure(&crate::offset_surface(&cover, &[0], d, 0.01).unwrap()).unwrap().area;
        let want = t / 6.0 * (area(0.0) + 4.0 * area(t / 2.0) + area(t));
        let v = measure(&thicken(&cover, t, false).unwrap()).unwrap().volume;
        assert!(rel(v, want) < 2e-3, "{v} vs {want}"); // (measured on meshes)
    }

    /// A cone frustum's side (radii 10 and 5, 10 high) thickened out by 1: a cone has no Gaussian
    /// curvature, so its offset's area grows linearly and the volume is t·π·L·(r1 + r2 + t·cos β),
    /// L the slant length and β the angle between the normal and the radius (cos β = 10/L).
    #[test]
    fn thicken_a_cone_frustum_side() {
        let region = Region2 {
            outer: Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(5.0, 10.0), Vec2::new(0.0, 10.0)]),
            holes: vec![],
        };
        let xz = Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Z };
        let frustum = crate::revolve(&xz, &[region], Vec2::new(0.0, 0.0), Vec2::new(0.0, 1.0), 2.0 * std::f64::consts::PI).unwrap().remove(0);
        // The side comes in several pieces round the axis: picked all the way round.
        let round: Vec<Vec3> =
            (0..8).map(|k| f64::from(k) * std::f64::consts::FRAC_PI_4).map(|a| Vec3::new(7.5 * a.cos(), 7.5 * a.sin(), 5.0)).collect();
        let side = copy_faces(&frustum, &round).unwrap();
        let l = 125f64.sqrt();
        assert!(rel(measure(&side).unwrap().area, std::f64::consts::PI * 15.0 * l) < 2e-3, "the whole side");
        let v = measure(&thicken(&side, 1.0, false).unwrap()).unwrap().volume;
        let want = std::f64::consts::PI * l * (15.0 + 10.0 / l);
        assert!(rel(v, want) < 3e-3, "{v} vs {want}"); // (measured on meshes)
    }

    #[test]
    fn patch_and_thicken_a_square() {
        let r = Region2 {
            outer: Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)]),
            holes: vec![],
        };
        let s = patch_region(&Plane::XY, &r).unwrap();
        assert!(s.is_surface());
        let t = thicken(&s, 2.0, false).unwrap();
        assert!(!t.is_surface());
        assert!(rel(measure(&t).unwrap().volume, 200.0) < 1e-9);
        let t = thicken(&s, 2.0, true).unwrap();
        let m = measure(&t).unwrap();
        assert!(rel(m.volume, 200.0) < 1e-9 && (m.bbox.min.z + 1.0).abs() < 1e-9, "{:?}", m.bbox);
    }

    #[test]
    fn thicken_a_cylinder_side_and_a_box_top() {
        let c = cylinder(Vec3::ZERO, Vec3::Z, 10.0, 20.0).unwrap();
        // One half of the side, offset outward by 1: half of a ring.
        let half = copy_faces(&c, &[Vec3::new(7.0, 7.0, 10.0)]).unwrap();
        let v = measure(&thicken(&half, 1.0, false).unwrap()).unwrap().volume;
        let want = std::f64::consts::PI * (121.0 - 100.0) / 2.0 * 20.0;
        assert!(rel(v, want) < 3e-4, "{v} vs {want}"); // (measured on meshes)
        // Both halves (meeting smoothly): the whole ring, inward.
        let d = 10.0 / 2f64.sqrt();
        let side = copy_faces(&c, &[Vec3::new(d, d, 10.0), Vec3::new(-d, -d, 10.0)]).unwrap();
        assert_eq!(side.face_count(), 2);
        let v = measure(&thicken(&side, -1.0, false).unwrap()).unwrap().volume;
        let want = std::f64::consts::PI * (100.0 - 81.0) * 20.0;
        assert!(rel(v, want) < 3e-4, "{v} vs {want}"); // (measured on meshes)
        // Two faces meeting at a sharp edge: two slabs and the wedge closing the gap between
        // them, its ends patched: 2·100 + ½·1²·10.
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
        let two = copy_faces(&b, &[Vec3::new(5.0, 5.0, 10.0), Vec3::new(5.0, 0.0, 5.0)]).unwrap();
        let v = measure(&thicken(&two, 1.0, false).unwrap()).unwrap().volume;
        assert!(rel(v, 205.0) < 1e-9, "{v}");
    }

    #[test]
    fn stitch_six_faces_into_a_box_and_patch_a_hole() {
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap();
        let pts = [
            Vec3::new(5.0, 10.0, 0.0),
            Vec3::new(5.0, 10.0, 30.0),
            Vec3::new(0.0, 10.0, 15.0),
            Vec3::new(10.0, 10.0, 15.0),
            Vec3::new(5.0, 0.0, 15.0),
            Vec3::new(5.0, 20.0, 15.0),
        ];
        let parts: Vec<Body> = pts.iter().map(|p| copy_faces(&b, &[*p]).unwrap()).collect();
        let refs: Vec<&Body> = parts.iter().collect();
        let s = stitch(&refs, 0.0).unwrap();
        assert!(!s.is_surface());
        assert!(rel(measure(&s).unwrap().volume, 6000.0) < 1e-9);
        // Five faces leave an open box; patching the top's edges closes it again.
        let open = stitch(&[refs[0], refs[2], refs[3], refs[4], refs[5]], 0.0).unwrap();
        assert!(open.is_surface());
        let lid =
            patch_edges(&open, &[Vec3::new(5.0, 0.0, 30.0), Vec3::new(10.0, 10.0, 30.0), Vec3::new(5.0, 20.0, 30.0), Vec3::new(0.0, 10.0, 30.0)])
                .unwrap();
        let closed = stitch(&[&open, &lid], 0.0).unwrap();
        assert!(!closed.is_surface());
        assert!(rel(measure(&closed).unwrap().volume, 6000.0) < 1e-9);
    }

    #[test]
    fn extend_a_planar_surface() {
        let r = Region2 {
            outer: Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)]),
            holes: vec![],
        };
        let s = patch_region(&Plane::XY, &r).unwrap();
        let e = extend(&s, &[Vec3::new(10.0, 5.0, 0.0)], 5.0).unwrap();
        assert!(rel(measure(&e).unwrap().area, 150.0) < 1e-9);
        let e = extend(&s, &[Vec3::new(10.0, 5.0, 0.0), Vec3::new(5.0, 10.0, 0.0)], 5.0).unwrap();
        assert!(rel(measure(&e).unwrap().area, 225.0) < 1e-9);
        assert!(extend(&s, &[Vec3::new(10.0, 5.0, 0.0)], -1.0).is_err());
    }

    #[test]
    fn trim_keeps_one_side() {
        let r = Region2 {
            outer: Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)]),
            holes: vec![],
        };
        let s = patch_region(&Plane::XY, &r).unwrap();
        let pl = Plane::new(Vec3::new(3.0, 0.0, 0.0), Vec3::Y, Vec3::Z).unwrap();
        let t = trim(&s, &crate::SplitTool::Plane(pl), Vec3::new(1.0, 5.0, 0.0)).unwrap();
        assert!(rel(measure(&t).unwrap().area, 30.0) < 1e-9);
        let t = trim(&s, &crate::SplitTool::Plane(pl), Vec3::new(8.0, 5.0, 0.0)).unwrap();
        assert!(rel(measure(&t).unwrap().area, 70.0) < 1e-9);
    }
}
