//! Regeneration through the exact kernel (`kernel::exact`, see
//! `docs/SOLIDCRAFT.md`): each body-making feature becomes exact solids, and
//! the part's [`BRep`] is converted from them after every feature.
//!
//! A builder returns `Ok(None)` for an option the exact path does not cover
//! yet; regeneration then goes on with the faceted kernel from that feature.

use crate::kernel::exact::{self, ChamferSecond, ExactOp, ExactSolid};
use crate::kernel::{BRep, Curve, Plane, Point3, Real, TessellationOptions};
use crate::sketch::Sketch;

use super::blend::{BlendFeature, BlendKind, ChamferMethod, EdgeRef, FilletType};
use super::boolean::{Operation, Output};
use super::extrude::{ExtrudeFeature, kernel_profile};
use super::revolve::RevolveFeature;

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
pub(super) fn prefix(id: &str, n: usize) -> String {
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
    works: &super::placement::WorkGeom,
    sketch: &Sketch,
) -> Result<Option<Vec<ExactSolid>>, Fail> {
    extrude_from(e, body, works, sketch, 0.0, 0.0)
}

/// [`extrude`] whose tools start `overlap` behind the start level (away
/// from the extrusion): a Join whose tool only touches the body on a face
/// (a lip on a rim, its wall flush with the body's wall) gives the kernel's
/// Boolean nothing to intersect ("no result"); reaching a little into the
/// body it does, and the result is the same solid. `grow` offsets every
/// loop of the profile outward (holes shrink): a Cut whose walls are the
/// body's own walls (a section from Project Cut Edges) reaches a little into
/// the air beyond them.
pub(super) fn extrude_from(
    e: &ExtrudeFeature,
    body: &BRep,
    works: &super::placement::WorkGeom,
    sketch: &Sketch,
    overlap: Real,
    grow: Real,
) -> Result<Option<Vec<ExactSolid>>, Fail> {
    if e.output == Output::Surface {
        return extrude_surface(e, body, works, sketch);
    }
    let plane = &sketch.plane;
    let mut out = Vec::new();
    for piece in e.pieces(body, works, sketch).map_err(User)? {
        let levels = &piece.span.levels;
        let prof = if grow != 0.0 {
            let grown = |l: &[crate::kernel::ProfileSegment]| {
                crate::kernel::offset_loop(l, grow).map_err(|x| Kernel(x.to_string()))
            };
            let outer = grown(&piece.outer)?;
            let holes = piece
                .holes
                .iter()
                .map(|h| grown(h))
                .collect::<Result<Vec<_>, _>>()?;
            exact::profile(&outer, &holes)
        } else {
            exact::profile(&piece.outer, &piece.holes)
        }
        .map_err(Kernel)?;
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
        if overlap != 0.0 && taper != 0.0 {
            return Err(Kernel("no overlap for a tapered extrusion".into()));
        }
        let back = overlap * (last.offset - first.offset).signum();
        let mut tool = exact::extrude(
            plane,
            &prof,
            first.offset - back,
            last.offset,
            taper,
            &piece.prefix,
        )
        .map_err(Kernel)?;
        // A size beyond everything involved (for the clipping solids).
        let reach = {
            let (lo, hi) = body.bounding_box().unwrap_or((plane.origin, plane.origin));
            let far = lo.coords.amax().max(hi.coords.amax()) + plane.origin.coords.norm();
            2.0 * (far
                + piece.extent()
                + levels.iter().map(|l| l.offset.abs()).fold(0.0, Real::max))
                + 100.0
        };
        if let Some(clip) = &piece.span.clip {
            tool = clip_tool(&tool, clip, &piece.prefix, reach)?;
        }
        if let Some((lo, hi)) = piece.bounds {
            let b =
                exact::box_named(lo, hi, &format!("{}:Bounds", piece.prefix)).map_err(Kernel)?;
            tool = exact::boolean(&tool, &b, ExactOp::Intersect)
                .map_err(Kernel)?
                .ok_or_else(|| {
                    User("Match Shape: the open profile does not meet the body".into())
                })?;
        }
        out.push(tool);
    }
    Ok(Some(out))
}

/// A Join on a face whose profile's outer loop is the whole outline of the
/// face (a lip round a rim, its outer wall flush with the body's walls): the
/// kernel's Union of a tool that only touches the body along those walls
/// found no result after a minute (owner's box, 2026-10-27), with or without
/// the tool reaching into the body. Here the face itself is thickened into a
/// slab (its walls are the body's own surfaces, so the Union is a gluing:
/// 30 ms) and the profile's holes are cut back out of it. `Ok(None)`: not
/// this case. Faces: the slab's moved face `<id>:End`, its walls (where they
/// do not merge into the body's) `<id>:Wall:<k>`, the holes' walls
/// `<id>:Side:<segment>`, their floors keep the face's name.
pub(super) fn face_slab_join(
    e: &ExtrudeFeature,
    body: &BRep,
    works: &super::placement::WorkGeom,
    ps: &super::PartSketch,
    cur: &[ExactSolid],
    chosen: &[String],
) -> Result<Option<Vec<ExactSolid>>, Fail> {
    let super::PlaneRef::Face(face) = &ps.placement.plane else {
        return Ok(None);
    };
    if e.operation != Operation::Join || e.output != Output::Solid || e.profiles.len() != 1 {
        return Ok(None);
    }
    let regions = ps.sketch.regions();
    let Some(region) = e.profiles[0].resolve(&regions) else {
        return Ok(None);
    };
    if !region.outer.edges.iter().all(|x| ps.is_reference(x.source)) {
        return Ok(None);
    }
    let Ok(pieces) = e.pieces(body, works, &ps.sketch) else {
        return Ok(None);
    };
    let [piece] = pieces.as_slice() else {
        return Ok(None);
    };
    let levels = &piece.span.levels;
    let (Some(first), Some(last)) = (levels.first(), levels.last()) else {
        return Ok(None);
    };
    if levels.len() != 2
        || first.offset.abs() > 1e-12
        || levels.iter().any(|l| l.inset.abs() > 1e-12)
        || piece.span.clip.is_some()
        || piece.bounds.is_some()
    {
        return Ok(None);
    }
    let depth = last.offset;
    let plane = &ps.sketch.plane;
    let inside = region.interior_point();
    let at = plane.point_at(inside.x, inside.y);
    // The solid holding the face there.
    let Some((k, f)) = cur.iter().enumerate().find_map(|(k, s)| {
        if s.is_open()
            || !(chosen.is_empty() || s.name.as_ref().is_some_and(|n| chosen.contains(n)))
        {
            return None;
        }
        let f = *exact::faces_named(s, face, at).ok()?.first()?;
        let d = *exact::face_distances(s, &[(f, at)]).ok()?.first()?;
        (d <= 1e-6 * (1.0 + at.coords.norm())).then_some((k, f))
    }) else {
        return Ok(None);
    };
    let solid = &cur[k];
    // Only out of the body (into the air).
    let n = plane.normal.into_inner() * depth.signum();
    if exact::contains(solid, at + n * (1e-3 * depth.abs()).max(1e-4)) {
        return Ok(None);
    }
    let prefix = format!("{}:Lip", e.id);
    let Ok(sheet) = exact::face_sheet(solid, &[f]) else {
        return Ok(None);
    };
    let Ok(slab) = exact::thicken_sheet(&sheet, depth.abs(), false, None, &prefix) else {
        return Ok(None);
    };
    // The slab is the face's whole outline: the profile's outer loop must be
    // it (a slab of the wrong size would add material the user did not draw).
    let want = region.outer.area().abs() * depth.abs();
    match slab.volume_at(1e-4) {
        Some(v) if (v - want).abs() <= 1e-5 * want => {}
        _ => return Ok(None),
    }
    let Ok(Some(mut joined)) = exact::boolean(solid, &slab, ExactOp::Union) else {
        return Ok(None);
    };
    // The holes, cut back out of the slab.
    for (i, h) in piece.holes.iter().enumerate() {
        let lp: Vec<crate::kernel::ProfileSegment> = h
            .iter()
            .rev()
            .map(|s| crate::kernel::ProfileSegment {
                geometry: super::extrude::reverse_segment(&s.geometry),
                name: s.name.clone(),
            })
            .collect();
        let prof = exact::profile(&lp, &[]).map_err(Kernel)?;
        let tool = exact::extrude(plane, &prof, 0.0, depth, 0.0, &format!("{}#h{i}", e.id))
            .map_err(Kernel)?;
        joined = exact::boolean(&joined, &tool, ExactOp::Cut)
            .map_err(Kernel)?
            .ok_or_else(|| User("Join: nothing is left of the solid".into()))?;
    }
    // Names as a plain Join would give them: the moved face is the End, the
    // pockets' walls are the profile's sides, their floors stay the face.
    let p = &piece.prefix;
    for n in joined.face_names.iter_mut().flatten() {
        if n.starts_with(&format!("{prefix}:Offset:")) {
            *n = format!("{p}:End");
        } else if let Some(k) = n.strip_prefix(&format!("{prefix}:Wall:")) {
            *n = format!("{p}:Wall:{k}");
        } else if let Some((h, rest)) = n.split_once(':')
            && h.starts_with(&format!("{}#h", e.id))
        {
            *n = if rest == "Start" {
                face.clone()
            } else {
                format!("{p}:{rest}")
            };
        }
    }
    joined.name = solid.name.clone();
    let mut out = cur.to_vec();
    out[k] = joined;
    Ok(Some(out))
}

/// Extrude with Output Surface: the walls of each profile (its loops) and
/// of the open curves, as exact sheets (`exact::extrude_sheet`), one surface
/// body per profile. `Ok(None)` for what sheets do not cover yet (a taper, a
/// termination that is not parallel to the sketch).
fn extrude_surface(
    e: &ExtrudeFeature,
    body: &BRep,
    works: &super::placement::WorkGeom,
    sketch: &Sketch,
) -> Result<Option<Vec<ExactSolid>>, Fail> {
    use crate::kernel::ProfileSegment;
    let plane = &sketch.plane;
    // (prefix, levels, chains)
    let mut parts: Vec<(
        String,
        Vec<crate::kernel::ExtrudeLevel>,
        Vec<Vec<ProfileSegment>>,
    )> = Vec::new();
    if !e.profiles.is_empty() {
        let mut closed = e.clone();
        closed.open = None;
        for piece in closed.pieces(body, works, sketch).map_err(User)? {
            if piece.span.clip.is_some() || piece.bounds.is_some() {
                return Ok(None);
            }
            let chains = std::iter::once(piece.outer.clone())
                .chain(piece.holes.iter().cloned())
                .collect();
            parts.push((piece.prefix.clone(), piece.span.levels.clone(), chains));
        }
    }
    if let Some(op) = &e.open {
        let (chain, ..) = super::extrude::open_chain(op, sketch).map_err(User)?;
        let probes = super::extrude::segments_polygon(&chain);
        let span = e.span(body, works, plane, &probes).map_err(User)?;
        if span.clip.is_some() {
            return Ok(None);
        }
        parts.push((prefix(&e.id, parts.len()), span.levels, vec![chain]));
    }
    if parts.is_empty() {
        return Err(User("select at least one profile or open curve".into()));
    }
    let mut out = Vec::new();
    for (pre, levels, chains) in parts {
        let (Some(first), Some(last)) = (levels.first(), levels.last()) else {
            return Err(User("the extrusion has no depth".into()));
        };
        if levels.iter().any(|l| l.inset.abs() > 1e-12) {
            // A tapered surface: not with sheets yet.
            return Ok(None);
        }
        out.push(
            exact::extrude_sheet(plane, &chains, first.offset, last.offset, &pre)
                .map_err(Kernel)?,
        );
    }
    Ok(Some(out))
}

/// A square slab of half size `half` on `plane` (normal = its normal),
/// between offsets `from` and `to`; every face is named `name`.
fn slab(plane: &Plane, half: Real, from: Real, to: Real, name: &str) -> Result<ExactSolid, Fail> {
    use crate::kernel::{ProfileSegment, Segment2};
    let c = [
        nalgebra::Point2::new(-half, -half),
        nalgebra::Point2::new(half, -half),
        nalgebra::Point2::new(half, half),
        nalgebra::Point2::new(-half, half),
    ];
    let outer: Vec<ProfileSegment> = (0..4)
        .map(|k| ProfileSegment {
            geometry: Segment2::Line {
                a: c[k],
                b: c[(k + 1) % 4],
            },
            name: format!("S{k}"),
        })
        .collect();
    let prof = exact::profile(&outer, &[]).map_err(Kernel)?;
    let mut s = exact::extrude(plane, &prof, from, to, 0.0, name).map_err(Kernel)?;
    for n in &mut s.face_names {
        *n = Some(name.to_owned());
    }
    Ok(s)
}

/// A solid cylinder around `cyl`'s axis, `2·len` long, its round face
/// named `name`.
fn cylinder_solid(
    cyl: &crate::kernel::Cylinder,
    len: Real,
    name: &str,
) -> Result<ExactSolid, Fail> {
    use crate::kernel::{ProfileSegment, Segment2};
    let plane = Plane::new(cyl.origin, cyl.axis.into_inner(), cyl.x_axis.into_inner())
        .map_err(|e| Kernel(e.to_string()))?;
    let outer = vec![ProfileSegment {
        geometry: Segment2::Arc {
            c: nalgebra::Point2::origin(),
            r: cyl.radius,
            a0: 0.0,
            sweep: std::f64::consts::TAU,
        },
        name: "C".into(),
    }];
    let prof = exact::profile(&outer, &[]).map_err(Kernel)?;
    let mut s = exact::extrude(&plane, &prof, -len, len, 0.0, name).map_err(Kernel)?;
    for n in &mut s.face_names {
        *n = Some(name.to_owned());
    }
    Ok(s)
}

/// Ends an extrusion made past its termination (`docs/spec/2.1-extrude.md`
/// §3.3 To): cut by an inclined plane, or stopped at a cylinder's near or
/// far side. The termination face is named `<prefix>:End`.
fn clip_tool(
    tool: &ExactSolid,
    clip: &super::extrude::Clip,
    prefix: &str,
    reach: Real,
) -> Result<ExactSolid, Fail> {
    use super::extrude::Clip;
    let end = format!("{prefix}:End");
    let gone = || User("To: nothing of the extrusion is left before the termination".into());
    match clip {
        Clip::Plane { plane, keep } => {
            // The kept side at negative offsets of the slab's plane.
            let m = plane.normal.into_inner();
            let m = if (keep - plane.origin).dot(&m) > 0.0 {
                -m
            } else {
                m
            };
            let x = if m.x.abs() < 0.9 {
                crate::kernel::Vector3::x()
            } else {
                crate::kernel::Vector3::y()
            };
            let pl = Plane::new(plane.origin, m, x).map_err(|e| Kernel(e.to_string()))?;
            let s = slab(&pl, reach, -reach, 0.0, &end)?;
            exact::boolean(tool, &s, ExactOp::Intersect)
                .map_err(Kernel)?
                .ok_or_else(gone)
        }
        Clip::Cylinder { cyl, dir, near } => {
            let a = cyl.axis.into_inner();
            let dp = (dir - a * dir.dot(&a))
                .try_normalize(1e-12)
                .ok_or_else(|| User("To: the extrusion runs along the cylinder's axis".into()))?;
            let c = cylinder_solid(cyl, reach, &end)?;
            // The half strip from the axis plane on along the travel
            // direction, wider than the cylinder (never tangent to it).
            let w = a.cross(&dp);
            let pl = Plane::new(cyl.origin, a, w).map_err(|e| Kernel(e.to_string()))?;
            // In `pl`, x = w and y = a × w = −dp·…: find dp's sign in y.
            let y = pl.y_axis().into_inner();
            let (v0, v1) = if y.dot(&dp) > 0.0 {
                (0.0, reach)
            } else {
                (-reach, 0.0)
            };
            let r = cyl.radius * 1.5;
            let strip = {
                use crate::kernel::{ProfileSegment, Segment2};
                let c4 = [
                    nalgebra::Point2::new(-r, v0),
                    nalgebra::Point2::new(r, v0),
                    nalgebra::Point2::new(r, v1),
                    nalgebra::Point2::new(-r, v1),
                ];
                let outer: Vec<ProfileSegment> = (0..4)
                    .map(|k| ProfileSegment {
                        geometry: Segment2::Line {
                            a: c4[k],
                            b: c4[(k + 1) % 4],
                        },
                        name: format!("S{k}"),
                    })
                    .collect();
                let prof = exact::profile(&outer, &[]).map_err(Kernel)?;
                exact::extrude(&pl, &prof, -reach, reach, 0.0, &format!("{prefix}:Strip"))
                    .map_err(Kernel)?
            };
            if *near {
                let t = exact::boolean(tool, &c, ExactOp::Cut)
                    .map_err(Kernel)?
                    .ok_or_else(gone)?;
                exact::boolean(&t, &strip, ExactOp::Cut)
                    .map_err(Kernel)?
                    .ok_or_else(gone)
            } else {
                let beyond = exact::boolean(&strip, &c, ExactOp::Cut)
                    .map_err(Kernel)?
                    .ok_or_else(gone)?;
                exact::boolean(tool, &beyond, ExactOp::Cut)
                    .map_err(Kernel)?
                    .ok_or_else(gone)
            }
        }
    }
}

/// The solids of a Revolve (one per region), before its operation.
pub(super) fn revolve(
    r: &RevolveFeature,
    body: &BRep,
    works: &super::placement::WorkGeom,
    sketch: &Sketch,
) -> Result<Option<Vec<ExactSolid>>, Fail> {
    let axis = super::revolve::resolve_axis_in(&r.axis, sketch, works).map_err(User)?;
    let (start, sweep) = r.sweep_in(body, works, sketch).map_err(User)?;
    let regions = sketch.regions();
    let mut out = Vec::new();
    for (n, pr) in r.profiles.iter().enumerate() {
        let region = pr
            .resolve(&regions)
            .ok_or_else(|| User(format!("profile #{} no longer exists in the sketch", n + 1)))?;
        let (outer, holes) = kernel_profile(region);
        if r.output == Output::Surface {
            // The walls only, as an exact sheet (`exact::revolve_sheet`).
            let chains: Vec<_> = std::iter::once(outer)
                .chain(holes)
                .flat_map(|lp| off_axis(lp, axis.origin, axis.dir))
                .collect();
            out.push(
                exact::revolve_sheet(
                    &sketch.plane,
                    &chains,
                    axis.origin,
                    axis.dir,
                    start,
                    sweep,
                    REVOLVE_SHEET_SENSE,
                    &prefix(&r.id, n),
                )
                .map_err(Kernel)?,
            );
            continue;
        }
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
    // Open curves (Output Surface): one chain, without its lines on the axis.
    if !r.open.is_empty() {
        if r.output != Output::Surface {
            return Err(User(
                "open curves give a surface: choose Output Surface".into(),
            ));
        }
        let op = super::OpenProfile {
            curves: r.open.clone(),
            flip: false,
            match_shape: false,
        };
        let (chain, ..) = super::extrude::open_chain(&op, sketch)
            .map_err(|e| User(e.replace("Match Shape: ", "")))?;
        let chains = open_off_axis(chain, axis.origin, axis.dir);
        if chains.is_empty() {
            return Err(User("the open curves lie on the axis".into()));
        }
        out.push(
            exact::revolve_sheet(
                &sketch.plane,
                &chains,
                axis.origin,
                axis.dir,
                start,
                sweep,
                REVOLVE_SHEET_SENSE,
                &prefix(&r.id, r.profiles.len()),
            )
            .map_err(Kernel)?,
        );
    }
    Ok(Some(out))
}

/// An open chain without its lines on the revolve axis: the runs between
/// them (each sweeps a surface; a line on the axis sweeps nothing).
fn open_off_axis(
    chain: Vec<crate::kernel::ProfileSegment>,
    origin: nalgebra::Point2<Real>,
    dir: nalgebra::Vector2<Real>,
) -> Vec<Vec<crate::kernel::ProfileSegment>> {
    use crate::kernel::Segment2;
    let Some(d) = dir.try_normalize(1e-15) else {
        return vec![chain];
    };
    let size = chain
        .iter()
        .map(|s| s.geometry.point(0.0).coords.norm())
        .fold(1.0, Real::max);
    let on = |p: nalgebra::Point2<Real>| {
        let v = p - origin;
        (v.x * d.y - v.y * d.x).abs() < 1e-9 * size
    };
    let mut out = Vec::new();
    let mut cur = Vec::new();
    for s in chain {
        if matches!(&s.geometry, Segment2::Line { a, b } if on(*a) && on(*b)) {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
        } else {
            cur.push(s);
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Applies a feature's operation: `added` (its solids) against the bodies
/// so far (all of them, or those named in `solids`). `lenient` (pattern
/// copies): a cutting tool that misses the body is skipped; only when none
/// hits is it an error.
pub(super) fn combine(
    mut cur: Vec<ExactSolid>,
    added: Vec<ExactSolid>,
    op: Operation,
    solids: &[String],
    lenient: bool,
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
    let mut any_hit = false;
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
                // Did the tool remove anything? A tool that only touches the body
                // (a cut pointing away from it) leaves it as it was: new faces
                // say yes at once; with as many faces as before, the volumes
                // decide, meshed fine enough for the tool (a small hole in a
                // large plate) at the same tolerance before and after.
                for &i in &targets {
                    // Pattern copies (`lenient`): the tool is one compound of
                    // many pieces, nearly always over the body; `meets` would
                    // mesh all of it finely (2500 copies: 2.5 s) only to say
                    // so. A copy clear of the body leaves it as it was.
                    if !lenient && !exact::meets(&cur[i], &t) {
                        continue;
                    }
                    let tol = (cur[i].size().min(t.size()) * 1e-3).max(1e-4);
                    let vol = |s: &ExactSolid| s.volume_at(tol).unwrap_or(0.0);
                    match exact::boolean(&cur[i], &t, ExactOp::Cut)
                        .map_err(|e| Kernel(format!("Cut failed: {e}")))?
                    {
                        Some(r) => {
                            let changed = r.face_names.len() != cur[i].face_names.len();
                            if changed || {
                                let before = vol(&cur[i]);
                                before - vol(&r) > 1e-9 * before.max(1.0)
                            } {
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
                if !hit && !lenient {
                    return Err(User("Cut: the feature does not intersect the body".into()));
                }
                any_hit |= hit;
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
    if lenient && op == Operation::Cut && !any_hit {
        return Err(User("Cut: none of the copies intersects the body".into()));
    }
    Ok(cur)
}

/// A kernel failure of a pattern is the feature's health (a faceted
/// rebuild could not build a pattern either).
pub(super) fn as_user(f: Fail) -> Fail {
    match f {
        Kernel(e) => User(format!("the geometry kernel cannot build it ({e})")),
        u => u,
    }
}

/// Rectangular / Circular Pattern and Mirror (`docs/spec/2.11-patterns.md`,
/// `docs/spec/2.12-mirror.md`). Feature mode: the sources' tools are copied
/// to every placement and applied with their own operation (all copies of
/// one tool set at once: `combine` joins the ones apart into one tool).
/// Solid mode: whole solids are copied and joined to the original, added as
/// new solids, or (Remove Original) replace it. Returns the solids and the
/// tools the pattern applied (originals and copies, so a later pattern or
/// mirror of this one copies every instance).
pub(super) fn pattern(
    p: &super::PatternFeature,
    body: &BRep,
    works: &super::placement::WorkGeom,
    cur: &[ExactSolid],
    sources: &[&super::pattern::ToolSet],
) -> Result<(Vec<ExactSolid>, Vec<super::pattern::ToolSet>), Fail> {
    use super::pattern::ToolSet;
    let places = p.placements(body, works).map_err(User)?;
    let tag = |k: usize| format!("{}#{k}", p.id);
    let copy = |s: &ExactSolid, k: usize, m: &nalgebra::Matrix4<Real>, name: &str| {
        exact::transformed(s, m, &tag(k), Some(name))
            .map_err(|e| User(format!("the geometry kernel cannot copy it ({e})")))
    };
    let mut out = cur.to_vec();
    if p.solid_mode {
        let targets: Vec<usize> = (0..cur.len())
            .filter(|&i| {
                p.solids.is_empty() || cur[i].name.as_ref().is_some_and(|n| p.solids.contains(n))
            })
            .collect();
        if targets.is_empty() {
            return Err(User(if p.solids.is_empty() {
                "there is no solid to copy".into()
            } else {
                "the chosen solid no longer exists".into()
            }));
        }
        let several = targets.len() > 1;
        let mut extra = Vec::new();
        for &i in &targets {
            let src = &cur[i];
            let orig = src.name.clone().unwrap_or_default();
            let mut copies = Vec::new();
            for (k, m) in &places {
                let name = if several {
                    format!("{}[{orig}]", tag(*k))
                } else {
                    tag(*k)
                };
                copies.push(copy(src, *k, m, &name)?);
            }
            if copies.is_empty() {
                continue;
            }
            if p.remove_original {
                // Only the copy stays, under the original's body name (later
                // features keep finding their body).
                if let Some(mut c) = copies.into_iter().next() {
                    c.name = src.name.clone();
                    out[i] = c;
                }
                continue;
            }
            match p.operation {
                Operation::NewSolid => extra.extend(copies),
                _ => {
                    let tools = match exact::union_apart(&copies) {
                        Ok(Some(one)) => vec![one],
                        _ => copies,
                    };
                    for t in tools {
                        out[i] = exact::boolean(&out[i], &t, ExactOp::Union)
                            .map_err(|e| User(format!("Join failed: {e}")))?
                            .ok_or_else(|| User("Join failed: the result is empty".into()))?;
                    }
                }
            }
        }
        out.extend(extra);
        return Ok((out, Vec::new()));
    }
    if sources.is_empty() {
        return Err(User("select the features to copy".into()));
    }
    let mut made = Vec::new();
    for set in sources {
        if set.op == Operation::Intersect {
            return Err(User(
                "an Intersect feature cannot be copied yet (pattern the solid)".into(),
            ));
        }
        let mut copies = Vec::new();
        for (k, m) in &places {
            for t in &set.tools {
                copies.push(copy(t, *k, m, &tag(*k))?);
            }
        }
        if !copies.is_empty() {
            // Copies apart from each other (checked on the tools' boxes moved
            // to each place, no mesh per copy) go in as one tool.
            let tool = match apart(&set.tools, &places) {
                true => exact::join_apart(&copies).ok().flatten(),
                false => None,
            };
            let tools = match tool {
                Some(one) => vec![one],
                None => copies.clone(),
            };
            out = combine(out, tools, set.op, &set.solids, true).map_err(as_user)?;
        }
        let mut all = set.tools.clone();
        all.extend(copies);
        made.push(ToolSet {
            tools: all,
            op: set.op,
            solids: set.solids.clone(),
        });
    }
    Ok((out, made))
}

/// Whether every copy of `tools` at `places` stays clear of every other:
/// each tool's box is measured once and moved to each place as an oriented
/// box; a sweep along x over their axis-aligned hulls finds the candidate
/// pairs, which are then tested exactly as oriented boxes (separating axes).
/// The hulls alone are too coarse for rotated copies: a circular pattern of
/// 1000 holes r0.4 on a ring of 180 (1.13 apart) has hulls that touch.
fn apart(tools: &[ExactSolid], places: &[(usize, nalgebra::Matrix4<Real>)]) -> bool {
    use nalgebra::{Matrix3, Vector3};
    let mut local = Vec::with_capacity(tools.len());
    for t in tools {
        match exact::bounds(t) {
            Some(b) => local.push(b),
            None => return false,
        }
    }
    // (centre, axes as columns, half sizes) and the hull (lo, hi).
    struct Obb {
        c: Vector3<Real>,
        r: Matrix3<Real>,
        h: Vector3<Real>,
        lo: Point3,
        hi: Point3,
    }
    let mut boxes: Vec<Obb> = Vec::with_capacity(local.len() * places.len());
    for (_, m) in places {
        // A rigid placement (moves, turns, mirrors): the moved axes stay
        // unit and square.
        let r: Matrix3<Real> = m.fixed_view::<3, 3>(0, 0).into_owned();
        for (lo, hi) in &local {
            let c = m.transform_point(&nalgebra::center(lo, hi)).coords;
            let h = (hi - lo) * 0.5;
            let ext = Vector3::new(
                r.row(0).abs().dot(&h.transpose()),
                r.row(1).abs().dot(&h.transpose()),
                r.row(2).abs().dot(&h.transpose()),
            );
            boxes.push(Obb {
                c,
                r,
                h,
                lo: Point3::from(c - ext),
                hi: Point3::from(c + ext),
            });
        }
    }
    // Separating axis test of two oriented boxes (Gottschalk et al.).
    let meet = |a: &Obb, b: &Obb| -> bool {
        let rot = a.r.transpose() * b.r;
        let t = a.r.transpose() * (b.c - a.c);
        let abs = rot.abs().add_scalar(1e-12);
        for i in 0..3 {
            let ra = a.h[i];
            let rb = (0..3).map(|j| b.h[j] * abs[(i, j)]).sum::<Real>();
            if t[i].abs() > ra + rb {
                return false;
            }
        }
        for j in 0..3 {
            let ra = (0..3).map(|i| a.h[i] * abs[(i, j)]).sum::<Real>();
            let rb = b.h[j];
            let tj = (0..3).map(|i| t[i] * rot[(i, j)]).sum::<Real>();
            if tj.abs() > ra + rb {
                return false;
            }
        }
        for i in 0..3 {
            let (i1, i2) = ((i + 1) % 3, (i + 2) % 3);
            for j in 0..3 {
                let (j1, j2) = ((j + 1) % 3, (j + 2) % 3);
                let ra = a.h[i1] * abs[(i2, j)] + a.h[i2] * abs[(i1, j)];
                let rb = b.h[j1] * abs[(i, j2)] + b.h[j2] * abs[(i, j1)];
                let d = t[i2] * rot[(i1, j)] - t[i1] * rot[(i2, j)];
                if d.abs() > ra + rb {
                    return false;
                }
            }
        }
        true
    };
    boxes.sort_by(|p, q| p.lo.x.total_cmp(&q.lo.x));
    for (k, a) in boxes.iter().enumerate() {
        for b in &boxes[k + 1..] {
            if b.lo.x > a.hi.x {
                break;
            }
            if a.lo.y <= b.hi.y
                && b.lo.y <= a.hi.y
                && a.lo.z <= b.hi.z
                && b.lo.z <= a.hi.z
                && meet(a, b)
            {
                return false;
            }
        }
    }
    true
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
    // Partial chamfer: one straight edge, a wedge only along its part.
    if bl.partial.is_some() {
        if bl.edges.len() != 1 {
            return Err(User("a partial chamfer takes one edge".into()));
        }
        return wedge_blend(bl, body, cur).map_err(User);
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

/// Shell (`docs/spec/2.6-shell.md`): the removed faces are found by name
/// (the piece nearest the hint); the solid is the one holding them, the one
/// named in `solids`, or the only one. A kernel failure is the feature's
/// health (no faceted rebuild, as for blends).
pub(super) fn shell(sh: &super::ShellFeature, cur: &[ExactSolid]) -> Result<Vec<ExactSolid>, Fail> {
    use exact::ShellSide;
    if !(sh.thickness.is_finite() && sh.thickness > 0.0) {
        return Err(User("the thickness must be greater than 0".into()));
    }
    let mut target: Option<usize> = None;
    let mut open = Vec::new();
    for (r, found) in sh.remove.iter().zip(resolve_faces(cur, &sh.remove)) {
        let Some((i, f)) = found else {
            return Err(User(format!("the face {} no longer exists", r.face)));
        };
        match target {
            Some(t) if t != i => {
                return Err(User("the removed faces belong to different solids".into()));
            }
            _ => target = Some(i),
        }
        if !open.contains(&f) {
            open.push(f);
        }
    }
    let named = |s: &ExactSolid| s.name.as_ref().is_some_and(|n| sh.solids.contains(n));
    let target = match (target, sh.solids.is_empty()) {
        (Some(t), _) => t,
        (None, false) => cur
            .iter()
            .position(named)
            .ok_or_else(|| User("the chosen solid no longer exists".into()))?,
        (None, true) if cur.len() == 1 => 0,
        (None, true) if cur.is_empty() => return Err(User("there is no solid to shell".into())),
        (None, true) => return Err(User("select the solid to shell".into())),
    };
    if !sh.solids.is_empty() && !named(&cur[target]) {
        return Err(User("the removed faces are not on the chosen solid".into()));
    }
    let side = match sh.direction {
        super::ShellDirection::Inside => ShellSide::Inside,
        super::ShellDirection::Outside => ShellSide::Outside,
        super::ShellDirection::Center => ShellSide::Both,
    };
    let rounded = sh.corners == super::ShellCorners::Rounded;
    let r = exact::shell_with(&cur[target], &open, sh.thickness, side, rounded, &sh.id).map_err(
        |e| {
            // The kernel offsets analytic faces only; a sweep or loft along
            // a curve has free-form (B-spline) walls.
            if e.contains("only bodies with planes") {
                User(format!(
                    "free-form faces (a sweep or loft along a curve) cannot be offset yet ({e})"
                ))
            } else {
                User(format!("the geometry kernel cannot build it ({e})"))
            }
        },
    )?;
    // Walls as thick as the part leave no cavity (a cylinder 10 high shelled
    // 10 or 12 from the top, FreeCAD #25050): no inner wall was made. (By
    // names: measuring the volumes took 1.4 s on a rounded disc.)
    let inner = format!("{}:Inner", sh.id);
    if !r.names().iter().flatten().any(|n| n.starts_with(&inner)) {
        return Err(User(
            "the walls are as thick as the part: nothing is left to hollow".into(),
        ));
    }
    let mut out = cur.to_vec();
    out[target] = r;
    Ok(out)
}

/// A body operation (`docs/spec/2.14-thicken-offset.md`). `cur`: the solids,
/// `sheets`: the surface bodies (only these operations see them). Returns
/// the solids and the new surface bodies; kernel failures are health.
pub(super) fn body_op(
    f: &super::BodyOpFeature,
    body: &BRep,
    works: &super::placement::WorkGeom,
    sketches: &[super::PartSketch],
    cur: &[ExactSolid],
    sheets: &[ExactSolid],
) -> Result<Vec<ExactSolid>, Fail> {
    use super::surface_op as so;
    match &f.op {
        super::BodyOp::Thicken(t) => thicken(t, &f.id, cur, sheets).map(|mut out| {
            out.extend(sheets.iter().cloned());
            out
        }),
        super::BodyOp::DeleteFace(d) => delete_face(d, cur, sheets),
        super::BodyOp::MoveBodies(m) => move_bodies(m, body, works, cur, sheets),
        super::BodyOp::Stitch(o) => so::stitch(o, &f.id, cur, sheets),
        super::BodyOp::Patch(o) => so::patch(o, &f.id, body, sketches, cur, sheets),
        super::BodyOp::Trim(o) => so::trim(o, body, works, sketches, cur, sheets),
        super::BodyOp::Extend(o) => so::extend(o, body, works, cur, sheets),
        super::BodyOp::Ruled(o) => so::ruled(o, &f.id, body, works, cur, sheets),
        super::BodyOp::ReplaceFace(o) => so::replace_face(o, &f.id, body, works, cur, sheets),
        super::BodyOp::Sculpt(o) => so::sculpt(o, &f.id, cur, sheets),
    }
}

/// The rigid motion of Move Bodies' steps, in order (lines resolved on the
/// body before the feature).
pub(super) fn move_matrix(
    m: &super::body_op::MoveBodiesOp,
    body: &BRep,
    works: &super::placement::WorkGeom,
) -> Result<nalgebra::Matrix4<Real>, String> {
    use super::body_op::MoveStep;
    let mut total = nalgebra::Matrix4::<Real>::identity();
    for s in &m.steps {
        let step = match s {
            MoveStep::FreeDrag { x, y, z } => {
                nalgebra::Translation3::new(*x, *y, *z).to_homogeneous()
            }
            MoveStep::AlongRay {
                direction,
                flip,
                offset,
            } => {
                let (_, d) = direction.resolve(body, works)?;
                let d = if *flip { -d } else { d };
                nalgebra::Translation3::from(d * *offset).to_homogeneous()
            }
            MoveStep::RotateAboutLine { axis, flip, angle } => {
                let (p, d) = axis.resolve(body, works)?;
                let d = if *flip { -d } else { d };
                let axis = nalgebra::Unit::try_new(d, 1e-12)
                    .ok_or_else(|| "the axis has no direction".to_owned())?;
                let r = nalgebra::Rotation3::from_axis_angle(&axis, *angle).to_homogeneous();
                nalgebra::Translation3::from(p.coords).to_homogeneous()
                    * r
                    * nalgebra::Translation3::from(-p.coords).to_homogeneous()
            }
        };
        total = step * total;
    }
    Ok(total)
}

/// Move Bodies: each chosen body (solid or surface) moved by the steps;
/// faces keep their names. Returns the solids and the surface bodies.
fn move_bodies(
    m: &super::body_op::MoveBodiesOp,
    body: &BRep,
    works: &super::placement::WorkGeom,
    cur: &[ExactSolid],
    sheets: &[ExactSolid],
) -> Result<Vec<ExactSolid>, Fail> {
    m.validate().map_err(User)?;
    let t = move_matrix(m, body, works).map_err(User)?;
    let mut all: Vec<ExactSolid> = cur.iter().chain(sheets).cloned().collect();
    for name in &m.bodies {
        let i = all
            .iter()
            .position(|s| s.name.as_deref() == Some(name.as_str()))
            .ok_or_else(|| User(format!("the body {name} no longer exists")))?;
        all[i] = exact::moved(&all[i], &t)
            .map_err(|e| User(format!("the geometry kernel cannot move it ({e})")))?;
    }
    Ok(all)
}

/// Each face reference resolved among the bodies of `all`: `(body index,
/// kernel face)`, the piece nearest the hint when several bodies hold the
/// name ([`exact::faces_in_solids`]).
fn resolve_faces(all: &[ExactSolid], refs: &[super::FaceRef]) -> Vec<Option<(usize, usize)>> {
    let asks: Vec<(&str, Point3)> = refs
        .iter()
        .map(|r| (r.face.as_str(), Point3::from(r.hint)))
        .collect();
    exact::faces_in_solids(all, &asks)
}

/// Delete Face: the picked faces of one body (solid or surface) deleted —
/// healed, opened into a surface body, or whole shells (Lump or Void).
/// Returns the solids and the surface bodies.
fn delete_face(
    d: &super::body_op::DeleteFaceOp,
    cur: &[ExactSolid],
    sheets: &[ExactSolid],
) -> Result<Vec<ExactSolid>, Fail> {
    use super::body_op::DeleteSelect;
    d.validate().map_err(User)?;
    let mut all: Vec<ExactSolid> = cur.iter().chain(sheets).cloned().collect();
    let mut source: Option<usize> = None;
    let mut faces: Vec<usize> = Vec::new();
    let found_in = resolve_faces(&all, &d.faces);
    for (r, found) in d.faces.iter().zip(found_in) {
        let Some((i, f)) = found else {
            return Err(User(format!("the face {} no longer exists", r.face)));
        };
        match source {
            Some(s) if s != i => {
                return Err(User("the faces belong to different bodies".into()));
            }
            _ => source = Some(i),
        }
        if !faces.contains(&f) {
            faces.push(f);
        }
    }
    let Some(i) = source else {
        return Err(User("select the faces to delete".into()));
    };
    let kernel = |e: String| User(e.replace("Delete Face: ", ""));
    let r = match d.select {
        DeleteSelect::Faces => exact::delete_faces(&all[i], &faces, d.heal).map_err(kernel)?,
        DeleteSelect::LumpOrVoid => {
            // One shell at a time: indices change once a shell goes, so
            // each later face is found again by its name (a face whose shell
            // went with an earlier pick is skipped).
            let mut s = all[i].clone();
            for r in &d.faces {
                let f = exact::faces_named(&s, &r.face, Point3::from(r.hint))
                    .ok()
                    .and_then(|v| v.first().copied());
                if let Some(f) = f {
                    s = exact::delete_shell(&s, f).map_err(kernel)?;
                }
            }
            s
        }
    };
    all[i] = r;
    Ok(all)
}

/// Thicken/Offset: the picked faces (or quilts) as a slab joined to, cut
/// from, intersected with a solid or a new solid; or offset into a surface.
fn thicken(
    t: &super::body_op::ThickenOp,
    id: &str,
    cur: &[ExactSolid],
    sheets: &[ExactSolid],
) -> Result<Vec<ExactSolid>, Fail> {
    use super::body_op::{ThickenOutput, ThickenSelect};
    t.validate().map_err(User)?;
    let (d, symmetric) = t.signed();
    let kernel = |e: String| {
        // An edge sharp along part of its length only (a loft through a
        // rectangle's corner between circles): the wall Thicken stands there
        // shrinks to nothing at its ends.
        if e.contains("Two same vertices") {
            User(
                concat!(
                    "an edge or a point of the faces is sharp in places and smooth in others ",
                    "(a cone tip, a loft through a corner): Thicken needs every edge smooth or ",
                    "sharp along its whole length"
                )
                .into(),
            )
        } else {
            User(format!("the geometry kernel cannot build it ({e})"))
        }
    };
    // Every body that can hold a picked face: solids, then surface bodies.
    let all: Vec<&ExactSolid> = cur.iter().chain(sheets).collect();
    let mut source: Option<usize> = None;
    // Face mode: the body index and its kernel faces (for the overlapped
    // retry below).
    let mut picked: Option<(usize, Vec<usize>)> = None;
    let piece = match t.select {
        ThickenSelect::Face => {
            let mut faces = Vec::new();
            let owned: Vec<ExactSolid> = all.iter().map(|s| (*s).clone()).collect();
            let found_in = resolve_faces(&owned, &t.faces);
            for (r, found) in t.faces.iter().zip(found_in) {
                let Some((i, f)) = found else {
                    return Err(User(format!("the face {} no longer exists", r.face)));
                };
                match source {
                    Some(s) if s != i => {
                        return Err(User("the faces belong to different bodies".into()));
                    }
                    _ => source = Some(i),
                }
                if !faces.contains(&f) {
                    faces.push(f);
                }
            }
            let Some(i) = source else {
                return Err(User("select the faces".into()));
            };
            picked = Some((i, faces.clone()));
            match t.output {
                ThickenOutput::Surface => {
                    exact::offset_sheet(all[i], &faces, d, t.tolerance, id).map_err(kernel)?
                }
                ThickenOutput::Solid => {
                    exact::thicken_faces(all[i], &faces, d, symmetric, t.tolerance, id)
                        .map_err(kernel)?
                }
            }
        }
        ThickenSelect::Quilt => {
            let mut out: Option<ExactSolid> = None;
            for q in &t.quilts {
                let s = sheets
                    .iter()
                    .find(|s| s.name.as_deref() == Some(q.as_str()))
                    .ok_or_else(|| User(format!("the surface body {q} no longer exists")))?;
                let slab =
                    exact::thicken_sheet(s, d, symmetric, t.tolerance, id).map_err(kernel)?;
                out = Some(match out {
                    None => slab,
                    Some(prev) => exact::boolean(&prev, &slab, ExactOp::Union)
                        .map_err(kernel)?
                        .ok_or_else(|| User("the thickened quilts leave nothing".into()))?,
                });
            }
            out.ok_or_else(|| User("select the surface bodies (quilts)".into()))?
        }
    };
    let mut out = cur.to_vec();
    if t.output == ThickenOutput::Surface || t.operation == Operation::NewSolid {
        out.push(piece);
        return Ok(out);
    }
    // The solid it changes: the chosen one, else the solid of the faces, else
    // the only one.
    let target = if let Some(name) = t.solids.first() {
        cur.iter()
            .position(|s| s.name.as_deref() == Some(name.as_str()))
            .ok_or_else(|| User(format!("the solid {name} no longer exists")))?
    } else if let Some(i) = source.filter(|&i| i < cur.len()) {
        i
    } else if cur.len() == 1 {
        0
    } else if cur.is_empty() {
        return Err(User("there is no solid to join to: use New Solid".into()));
    } else {
        return Err(User("select the solid".into()));
    };
    let name = cur[target].name.clone();
    let op = match t.operation {
        Operation::Join => ExactOp::Union,
        Operation::Cut => ExactOp::Cut,
        Operation::Intersect => ExactOp::Intersect,
        Operation::NewSolid => ExactOp::Union,
    };
    // Every face of the solid, outward Join or inward Cut: the moved faces
    // are the new solid.
    let whole = picked.as_ref().is_some_and(|(i, faces)| {
        *i == target && !symmetric && faces.len() == cur[target].face_names.len()
    });
    let grows =
        (t.operation == Operation::Join && d > 0.0) || (t.operation == Operation::Cut && d < 0.0);
    if whole && grows {
        let mut r = exact::offset_solid(&cur[target], d, t.tolerance).map_err(kernel)?;
        r.name = cur[target].name.clone();
        out[target] = r;
        return Ok(out);
    }
    let first = exact::boolean(&cur[target], &piece, op);
    let first = match first {
        Ok(Some(r)) => Ok(r),
        Ok(None) => Err(format!("{:?}: nothing is left of the solid", t.operation)),
        Err(e) => Err(e),
    };
    // A slab that only touches the solid along the picked faces (curved
    // faces: the kernel's Boolean finds no result on coincident spheres or
    // splines): built again reaching a little past the faces on the other
    // side — into the material for a Join, out of it for a Cut — so the two
    // overlap. The extra part lies inside (Join) or outside (Cut) the solid,
    // so the result is the same.
    let mut r = match (first, &picked) {
        (Ok(r), _) => r,
        (Err(e), Some((i, faces))) if !symmetric => {
            let eps = (0.1 * d.abs()).clamp(0.02, 0.5) * -d.signum();
            let sheet = exact::offset_sheet(all[*i], faces, eps, t.tolerance, id)
                .map_err(|_| User(format!("the geometry kernel cannot build it ({e})")))?;
            let slab = exact::thicken_sheet(&sheet, d - eps, false, t.tolerance, id)
                .map_err(|_| User(format!("the geometry kernel cannot build it ({e})")))?;
            exact::boolean(&cur[target], &slab, op)
                .map_err(kernel)?
                .ok_or_else(|| User(format!("{:?}: nothing is left of the solid", t.operation)))?
        }
        (Err(e), _) => return Err(kernel(e)),
    };
    r.name = name;
    out[target] = r;
    Ok(out)
}

/// Face Draft (`docs/spec/2.8-draft.md`): the faces (with their rounds,
/// Automatic Blending) are found by name on the solid holding them. One
/// way: faces sharing a neutral plane are drafted in one kernel call,
/// groups one after another (faces keep their names, so each group is found
/// again on the result). Two-sided (Symmetric / Asymmetric, Parting Line):
/// one split–draft–join. `sketch_lines`: the parting sketch's lines.
/// Kernel failures are health.
pub(super) fn draft(
    dr: &super::DraftFeature,
    body: &BRep,
    works: &super::placement::WorkGeom,
    cur: &[ExactSolid],
    sketch_lines: Option<&[(Point3, Point3, crate::kernel::Vector3)]>,
) -> Result<Vec<ExactSolid>, Fail> {
    if dr.faces.is_empty() {
        return Err(User("select the faces to draft".into()));
    }
    if !(dr.angle.is_finite() && dr.angle.abs() < std::f64::consts::FRAC_PI_2 - 1e-3) {
        return Err(User("the draft angle must be between −90° and 90°".into()));
    }
    let pull = dr.pull_direction(body, works).map_err(User)?;
    let refs = dr.faces_with_blends(body, pull);
    let kernel = |e: String| User(format!("the geometry kernel cannot build it ({e})"));
    let mut out = cur.to_vec();
    if let Some(two) = dr
        .two_sided(body, works, pull, sketch_lines)
        .map_err(User)?
    {
        let all: Vec<&super::FaceRef> = refs.iter().collect();
        let (t, faces) = draft_targets(&out, &all)?;
        out[t] = exact::draft_two_sided(
            &out[t],
            &faces,
            &two.plane,
            pull,
            two.above.as_ref().map(|s| (&s.neutral, s.angle)),
            two.below.as_ref().map(|s| (&s.neutral, s.angle)),
        )
        .map_err(kernel)?;
        return Ok(out);
    }
    let planes = dr.neutral_planes(body, works, pull, &refs).map_err(User)?;
    // Groups of faces with the same neutral plane (height along the pull).
    let mut groups: Vec<(crate::kernel::Plane, Vec<&super::FaceRef>)> = Vec::new();
    for (r, pl) in refs.iter().zip(planes) {
        let h = pl.origin.coords.dot(&pull);
        match groups
            .iter_mut()
            .find(|(g, _)| (g.origin.coords.dot(&pull) - h).abs() < 1e-9)
        {
            Some((_, v)) => v.push(r),
            None => groups.push((pl, vec![r])),
        }
    }
    for (pl, refs) in groups {
        let (t, faces) = draft_targets(&out, &refs)?;
        // Pieces of split faces (Split Face): drafted piece by piece.
        out[t] = if refs.iter().any(|r| r.face.contains('~')) {
            super::split::draft_pieces(&out[t], &faces, &pl, pull, dr.angle, &dr.id)
                .map_err(kernel)?
        } else {
            exact::draft(&out[t], &faces, &pl, pull, dr.angle).map_err(kernel)?
        };
    }
    Ok(out)
}

/// The solid holding the referenced faces and their kernel faces (all on
/// one solid).
fn draft_targets(
    out: &[ExactSolid],
    refs: &[&super::FaceRef],
) -> Result<(usize, Vec<usize>), Fail> {
    let mut target: Option<usize> = None;
    let mut faces = Vec::new();
    let asks: Vec<(&str, Point3)> = refs
        .iter()
        .map(|r| (r.face.as_str(), Point3::from(r.hint)))
        .collect();
    for (r, found) in refs.iter().zip(exact::faces_in_solids(out, &asks)) {
        let Some((i, f)) = found else {
            return Err(User(format!("the face {} no longer exists", r.face)));
        };
        match target {
            Some(t) if t != i => {
                return Err(User("the faces belong to different solids".into()));
            }
            _ => target = Some(i),
        }
        if !faces.contains(&f) {
            faces.push(f);
        }
    }
    target
        .map(|t| (t, faces))
        .ok_or_else(|| User("select the faces to draft".into()))
}

// What a Hole or Thread feature leaves for later: the holes it made
/// (drawing data), cosmetic threads (display) and modeled threads (cut
/// after every feature).
#[derive(Clone, Debug, Default)]
pub(super) struct HoleOutput {
    pub holes: Vec<super::hole::HoleInstance>,
    pub cosmetic: Vec<super::hole::CosmeticThread>,
    pub grooves: Vec<exact::ThreadGroove>,
}

impl HoleOutput {
    /// Adds a later step's output.
    pub fn append(&mut self, mut o: HoleOutput) {
        self.holes.append(&mut o.holes);
        self.cosmetic.append(&mut o.cosmetic);
        self.grooves.append(&mut o.grooves);
    }
}

/// The centres of a Hole feature: (point on the start plane, unit drilling
/// direction, start face name).
pub(super) fn hole_placements(
    h: &super::hole::HoleFeature,
    ps: Option<&super::PartSketch>,
    body: &BRep,
    works: &super::placement::WorkGeom,
) -> Result<Vec<(Point3, crate::kernel::Vector3, Option<String>)>, String> {
    use super::draft::PullRef;
    use super::hole::HoleCenter;
    if h.centers.is_empty() {
        return Err("select the hole centres".into());
    }
    // A picked direction (face / plane normal, edge / axis).
    let picked = match &h.direction {
        None => None,
        Some(PullRef::Plane(p)) => Some(
            super::placement::plane_of(p, body, works)
                .map_err(|e| format!("the direction: {e}"))?
                .normal
                .into_inner(),
        ),
        Some(PullRef::Line(l)) => Some(
            l.resolve(body, works)
                .map_err(|e| format!("the direction: {e}"))?
                .1,
        ),
    };
    let mut out = Vec::new();
    for c in &h.centers {
        let (o, mut dir, face) = match c {
            HoleCenter::Point(pid) => {
                let ps = ps.ok_or_else(|| format!("sketch {} is missing", h.sketch))?;
                // A deleted centre point removes its hole (Inventor).
                let Some(p) = ps.sketch.point(*pid) else {
                    continue;
                };
                let face = match &ps.placement.plane {
                    super::PlaneRef::Face(n) => Some(n.clone()),
                    _ => None,
                };
                (
                    ps.sketch.to_world(&p.position),
                    -ps.sketch.plane.normal.into_inner(),
                    face,
                )
            }
            HoleCenter::Concentric(e) => concentric_center(body, e)?,
            HoleCenter::Axis(fr) => {
                let cf = super::hole::cylinder_face(body, fr)?;
                (cf.start, cf.axis, None)
            }
        };
        if let Some(p) = picked {
            let p = p
                .try_normalize(1e-12)
                .ok_or_else(|| "the direction has no length".to_owned())?;
            // The picked line's sense on the side the hole drilled to.
            dir = if p.dot(&dir) < 0.0 { -p } else { p };
        }
        if h.spec.flip {
            dir = -dir;
        }
        out.push((o, dir, face));
    }
    if out.is_empty() {
        return Err("every centre point of the hole was deleted".into());
    }
    Ok(out)
}

/// The centre of the circular edge `e` and the drilling direction into the
/// planar face it bounds.
fn concentric_center(
    body: &BRep,
    e: &EdgeRef,
) -> Result<(Point3, crate::kernel::Vector3, Option<String>), String> {
    use crate::kernel::{Sense, Surface};
    let hint = Point3::from(e.hint);
    let mut best: Option<(Real, Real, Point3, crate::kernel::Vector3, String)> = None;
    for (fid, f) in body.faces() {
        let Some(name) = f.name.as_deref() else {
            continue;
        };
        if !e.faces.iter().any(|n| n == name) {
            continue;
        }
        let Some(Surface::Plane(pl)) = body.surface(f.surface) else {
            continue;
        };
        let out = match f.sense {
            Sense::Same => pl.normal.into_inner(),
            Sense::Reversed => -pl.normal.into_inner(),
        };
        for eid in super::select::edges_of_face(body, fid) {
            let Some(Curve::Circle(c)) = body.edge(eid).and_then(|x| body.curve(x.curve)) else {
                continue;
            };
            // The hint lies on the circle's plane, on the circle or inside
            // it (an edge's hint may be the middle of its chord).
            let v = hint - c.center;
            let off = v.dot(&c.normal).abs();
            let rho = (v - c.normal.into_inner() * v.dot(&c.normal)).norm();
            if rho > c.radius * 1.05 + 1e-6 {
                continue;
            }
            let d = off + (rho - c.radius).abs();
            if best.as_ref().is_none_or(|b| d < b.0) {
                best = Some((d, off, c.center, -out, name.to_owned()));
            }
        }
    }
    let (_, off, c, dir, face) = best.ok_or_else(|| {
        format!(
            "the circular edge between {} and {} no longer exists",
            e.faces[0], e.faces[1]
        )
    })?;
    let size = body
        .bounding_box()
        .map_or(1.0, |(lo, hi)| (hi - lo).norm().max(1.0));
    if off > 1e-3 * size {
        return Err("the circular edge no longer exists".into());
    }
    Ok((c, dir, Some(face)))
}

/// Whether the point `q` (on the face's plane) lies on face `f` (its
/// display triangles).
fn face_contains(body: &BRep, f: crate::kernel::FaceId, q: Point3) -> bool {
    let Some(m) = body.face(f).and_then(|x| x.mesh.clone()) else {
        return true;
    };
    m.triangles.iter().any(|t| {
        let [a, b, c] = t.map(|i| m.positions[i as usize]);
        let n = (b - a).cross(&(c - a));
        let inside = |p: Point3, r: Point3| (r - p).cross(&(q - p)).dot(&n) >= -1e-12 * n.norm();
        inside(a, b) && inside(b, c) && inside(c, a)
    })
}

/// How far along `dir` from `o` a hole of radius `r` must go until its
/// axis and its rim (16 points) have all met face `f` (its display
/// triangles); `None` when none of them meets it ahead.
fn curved_reach(
    body: &BRep,
    f: crate::kernel::FaceId,
    o: Point3,
    dir: crate::kernel::Vector3,
    r: Real,
) -> Option<Real> {
    let m = body.face(f).and_then(|x| x.mesh.clone())?;
    let d = dir.normalize();
    let side = perpendicular(d);
    let other = d.cross(&side);
    let hit = |p: Point3| -> Option<Real> {
        m.triangles
            .iter()
            .filter_map(|t| {
                let [a, b, c] = t.map(|i| m.positions[i as usize]);
                // Möller–Trumbore.
                let (e1, e2) = (b - a, c - a);
                let h = d.cross(&e2);
                let det = e1.dot(&h);
                if det.abs() < 1e-14 {
                    return None;
                }
                let s = p - a;
                let u = s.dot(&h) / det;
                let q = s.cross(&e1);
                let v = d.dot(&q) / det;
                let t = e2.dot(&q) / det;
                (u >= -1e-9 && v >= -1e-9 && u + v <= 1.0 + 1e-9 && t > 1e-9).then_some(t)
            })
            .fold(None, |acc: Option<Real>, t| {
                Some(acc.map_or(t, |a| a.min(t)))
            })
    };
    let mut far: Option<Real> = hit(o);
    for k in 0..16 {
        let a = std::f64::consts::TAU * k as Real / 16.0;
        let p = o + (side * a.cos() + other * a.sin()) * r;
        if let Some(t) = hit(p) {
            far = Some(far.map_or(t, |x: Real| x.max(t)));
        }
    }
    far
}

/// Whether the ring of radius `rad` about `c` (square to `dir`) lies on one
/// planar face of `body` square to `dir` — where a 90° countersink can be the
/// kernel's chamfer of the bore's rim.
fn ring_on_square_face(
    body: &BRep,
    c: Point3,
    dir: crate::kernel::Vector3,
    rad: Real,
    size: Real,
) -> bool {
    use crate::kernel::Surface;
    let u = perpendicular(dir);
    let v = dir.cross(&u);
    body.faces().any(|(fid, f)| {
        let Some(Surface::Plane(pl)) = body.surface(f.surface) else {
            return false;
        };
        let n = pl.normal.into_inner();
        if n.dot(&dir).abs() < 1.0 - 1e-9 || (c - pl.origin).dot(&n).abs() > 1e-6 * size {
            return false;
        }
        (0..8).all(|k| {
            let a = k as Real * std::f64::consts::FRAC_PI_4;
            face_contains(body, fid, c + (u * a.cos() + v * a.sin()) * rad)
        })
    })
}

/// A unit vector square to `d`.
fn perpendicular(d: crate::kernel::Vector3) -> crate::kernel::Vector3 {
    let a = if d.x.abs() < 0.9 {
        crate::kernel::Vector3::x()
    } else {
        crate::kernel::Vector3::y()
    };
    d.cross(&a).normalize()
}

/// How far the body reaches from `o` along `dir` (mm, ≥ 0).
fn reach(points: &[Point3], o: Point3, dir: crate::kernel::Vector3) -> Real {
    points
        .iter()
        .map(|p| (p - o).dot(&dir))
        .fold(0.0, Real::max)
}

/// Every point of the body's display triangles and vertices (for
/// [`reach`]).
fn body_points(body: &BRep) -> Vec<Point3> {
    let mut v: Vec<Point3> = body.vertices().map(|(_, x)| x.point).collect();
    for (_, f) in body.faces() {
        if let Some(m) = &f.mesh {
            v.extend(m.positions.iter().copied());
        }
    }
    v
}

/// Hole (`docs/spec/2.3-hole.md`): tools of revolution per centre (seat,
/// bore), cut in two Booleans — the seats, then the bores (`combine` unions tools
/// apart from each other with `union_apart`; after SolidCraft's
/// `hole_tools_and_prisms`). Modeled
/// threads are only recorded here (cut after every feature).
pub(super) fn hole(
    h: &super::hole::HoleFeature,
    ps: Option<&super::PartSketch>,
    body: &BRep,
    works: &super::placement::WorkGeom,
    cur: &[ExactSolid],
    tap: super::hole::TapDiameter,
) -> Result<(Vec<ExactSolid>, HoleOutput, Vec<super::pattern::ToolSet>), Fail> {
    use super::hole::{CosmeticThread, HoleInstance, SeatType, Termination};
    // A tapped hole is cut at the document's Tapped Hole Diameter.
    let spec = &h.spec.with_tap_diameter(tap);
    spec.validate().map_err(User)?;
    let places = hole_placements(h, ps, body, works).map_err(User)?;
    let size = body
        .bounding_box()
        .map_or(1.0, |(lo, hi)| (hi - lo).norm().max(1.0));
    let margin = (size * 0.01).max(0.1);
    let points = body_points(body);
    let mut rays: Option<super::rib::Rays> = None;
    let (mut seats, mut bores) = (Vec::new(), Vec::new());
    // 90° countersinks on a face square to the hole: the kernel's chamfer of
    // the bore's rim after the bores (36 holes 5 s instead of ~40 s with cone
    // tools, ROADMAP K4). Their cone tools are kept for Pattern / Mirror and
    // as the fallback when the chamfer fails.
    let (mut deferred, mut rims) = (Vec::new(), Vec::new());
    let right_angle = |a: Real| (a - std::f64::consts::FRAC_PI_2).abs() < 1e-9;
    let r_bore = spec.bore_radius();
    let mut out = HoleOutput::default();
    for (k, (o, dir, face)) in places.iter().enumerate() {
        let n = k + 1;
        let up = -*dir;
        // Above the start plane: air (the plane is a face, or outside the
        // body) → the tool starts above it, so no cap lies on a face.
        // A centre on a face has air on its outward side (the sketch normal),
        // material behind it (Direction 2 drills into the air); elsewhere ask.
        let in_material = if face.is_some() {
            spec.flip
        } else {
            let probe = *o + up * (1e-4 * size);
            cur.iter().any(|s| exact::contains(s, probe))
        };
        let top = if spec.symmetric {
            // Through all the other way too.
            reach(&points, *o, up) + margin
        } else if in_material && !spec.extend_start {
            0.0
        } else {
            margin
        };
        let far = reach(&points, *o, *dir);
        let (depth, point) = match &spec.termination {
            Termination::Distance => (spec.depth, true),
            Termination::ThroughAll => (far + margin, false),
            Termination::To(fr) => {
                let fid = super::shell::face_near(body, &fr.face, Point3::from(fr.hint))
                    .ok_or_else(|| User(format!("the face {} no longer exists", fr.face)))?;
                match body.face(fid).and_then(|f| body.surface(f.surface)) {
                    Some(crate::kernel::Surface::Plane(pl)) => {
                        let nrm = pl.normal.into_inner();
                        let den = dir.dot(&nrm);
                        if den.abs() < 1e-9 {
                            return Err(User(
                                "the termination face is parallel to the hole".into(),
                            ));
                        }
                        let d = (pl.origin - o).dot(&nrm) / den;
                        if !(d.is_finite() && d > 1e-6) {
                            return Err(User(
                                "the termination face is not ahead of the hole".into(),
                            ));
                        }
                        if !spec.extend_to && !face_contains(body, fid, *o + *dir * d) {
                            return Err(User(
                                "the hole does not reach the termination face (turn Extend To on)"
                                    .into(),
                            ));
                        }
                        (d, false)
                    }
                    _ => {
                        // A curved face (a bore, a shaft): the hole goes on
                        // until its whole section has met the face, so it
                        // breaks fully through into the space beyond it.
                        let d = curved_reach(body, fid, *o, *dir, r_bore).ok_or_else(|| {
                            User("the hole does not reach the termination face".into())
                        })?;
                        (d + (0.02 * r_bore).max(1e-4 * size), false)
                    }
                }
            }
        };
        // A bore whose wall reaches the far side of the body (a blind depth
        // equal to or past a plate's thickness): its bottom and drill point
        // lie in the air and cut nothing, but a cap on the far face or a
        // cone crossing nothing sent the kernel down its slow general path
        // (283 × 136 × 10 plate, depth 10, 118°: 6.1 s, sometimes a
        // non-manifold edge). The tool runs out past the body as Through
        // All does; the thread keeps the given depth.
        let reaches = |d: Real| d >= far - 1e-6 * size;
        let (tool_depth, tool_point) = if point && reaches(spec.wall_bottom(depth)) {
            (far + margin, false)
        } else {
            (depth, point)
        };
        let mut parts = spec.tools(tool_depth, top, tool_point);
        let side = perpendicular(*dir);
        let mut start_rim = (spec.seat == SeatType::Countersink
            && right_angle(spec.csink_angle)
            && face.is_some()
            && !spec.flip
            && !spec.symmetric
            && ring_on_square_face(body, *o, *dir, spec.csink_diameter / 2.0, size))
        .then(|| exact::CircleChamfer {
            center: *o,
            at: *o + side * r_bore,
            distance: spec.csink_diameter / 2.0 - r_bore,
            name: format!("{}:CSink:{n}", h.id),
        });
        let mut far_rim = None;
        // Far side countersink: where the bore last leaves the material
        // (within its depth), cut with the seats.
        if spec.far_csink {
            let rays = rays.get_or_insert_with(|| super::rib::Rays::of(body, &h.solids));
            let start = *o + up * (1e-4 * size);
            let exits = rays.exits(start, *dir);
            let reach_to = if tool_point { depth } else { tool_depth };
            let exit = exits
                .into_iter()
                .map(|t| t - 1e-4 * size)
                .filter(|&t| t > 1e-6 && t <= reach_to + 1e-6 * size)
                .fold(None, |a: Option<Real>, t| Some(a.map_or(t, |a| a.max(t))));
            match exit {
                Some(e) => {
                    // `e` counts from `o` (the ray started 1e-4·size above it).
                    let c = *o + *dir * e;
                    let rf = spec.far_diameter() / 2.0;
                    if right_angle(spec.far_angle()) && ring_on_square_face(body, c, *dir, rf, size)
                    {
                        far_rim = Some(exact::CircleChamfer {
                            center: c,
                            at: c + side * r_bore,
                            distance: rf - r_bore,
                            name: format!("{}:FarCSink:{n}", h.id),
                        });
                    }
                    parts.insert(parts.len() - 1, spec.far_tool(e, margin));
                }
                None => {
                    return Err(User(
                        "the far side countersink needs a hole that comes out of the body".into(),
                    ));
                }
            }
        }
        let last = parts.len().saturating_sub(1);
        for (j, prof) in parts.iter().enumerate() {
            let pts: Vec<[Real; 2]> = prof.iter().map(|x| x.0).collect();
            let names: Vec<String> = prof.iter().map(|x| format!("{}:{n}", x.1)).collect();
            let refs: Vec<&str> = names.iter().map(String::as_str).collect();
            let seam = 0.2 + 0.613 * k as Real + 0.29 * j as Real;
            // Countersinks (start and far side) as tapered discs: a cone of
            // revolution sent the Boolean down a slow path (~1 s a hole).
            let cone = prof.len() == 4
                && prof[0].0[0] == 0.0
                && prof[3].0[0] == 0.0
                && matches!(prof[1].1, "CSink" | "FarCSink");
            let tool = if cone {
                // Start: wide end at the top (point 1), narrow inside the bore
                // (point 2). Far side: wide below the far face (point 2).
                let n: Vec<&str> = names.iter().map(String::as_str).collect();
                let (wide, narrow, caps) = if prof[1].1 == "CSink" {
                    (prof[1].0, prof[2].0, [n[0], n[1], n[2]])
                } else {
                    (prof[2].0, prof[1].0, [n[2], n[1], n[0]])
                };
                exact::cone_tool(
                    *o,
                    up,
                    seam,
                    (wide[0], wide[1]),
                    (narrow[0], narrow[1]),
                    caps,
                    &h.id,
                )
                .or_else(|_| exact::revolved_tool(*o, up, seam, &pts, &refs, &h.id))
                .map_err(Kernel)?
            } else {
                exact::revolved_tool(*o, up, seam, &pts, &refs, &h.id).map_err(Kernel)?
            };
            let rim = match prof[1].1 {
                "CSink" if cone => start_rim.take(),
                "FarCSink" if cone => far_rim.take(),
                _ => None,
            };
            if j == last {
                bores.push(tool);
            } else if let Some(rim) = rim {
                deferred.push(tool);
                rims.push(rim);
            } else {
                seats.push(tool);
            }
        }
        out.holes.push(HoleInstance {
            feature: h.id.clone(),
            occurrence: n,
            spec: spec.clone(),
            origin: *o,
            direction: *dir,
            start_face: face.clone(),
            copy: Vec::new(),
        });
        if let Some(t) = spec.tapped_thread() {
            // The threaded length, never past the material.
            let through = !matches!(spec.termination, Termination::Distance);
            let len = spec.thread_length(depth).min(far);
            if !(len.is_finite() && len > 1e-6) {
                continue;
            }
            if t.modeled {
                let section = t.internal_groove(spec.bore_radius()).map_err(User)?;
                let w0 = section[0][1].abs().max(section[3][1].abs());
                let p = t.pitch;
                // Through (or a blind bore reaching the far side): the groove
                // runs out below the body. Stopping square there put the
                // groove's lowest corners exactly on the far face, and the
                // faceted cut failed after 150 s ("shell is not oriented
                // and closed").
                let open_end = through || reaches(spec.wall_bottom(depth));
                let base = if open_end && reaches(len) {
                    -len - p
                } else {
                    -len + w0
                };
                // One pitch past the start plane is enough to run out into the air
                // (the tool's start margin grows with the part: 3.6 mm above a 3 mm
                // plate of a 300 mm part made the faceted cut fail).
                let end = if top > 0.0 { p } else { -w0 };
                let turns = (end - base) / p;
                if !(turns.is_finite() && turns > 0.0) {
                    continue;
                }
                let r_out = t.major / 2.0 + t.clearance + 0.3 * p;
                out.grooves.push(exact::ThreadGroove {
                    origin: *o,
                    axis: up,
                    section: section.clone(),
                    base,
                    pitch: p,
                    turns,
                    left_hand: !t.right_hand,
                    zone: [
                        (spec.bore_radius() - 0.2 * p).max(0.0),
                        r_out,
                        base - w0 - 0.25 * p,
                        end + w0 + 0.25 * p,
                    ],
                    prefix: h.id.clone(),
                    name: format!("Thread:{n}"),
                    // The start face, the far side, a seat's floor, the
                    // bore's bottom.
                    avoid: vec![0.0, -far, -spec.seat_depth, -spec.wall_bottom(depth)],
                });
            } else {
                out.cosmetic.push(CosmeticThread {
                    feature: h.id.clone(),
                    start: *o,
                    axis: *dir,
                    radius: t.major / 2.0,
                    length: len,
                    pitch: t.pitch,
                    shown: true,
                    face_radius: spec.bore_radius(),
                    internal: true,
                    right_hand: t.right_hand,
                });
            }
        }
    }
    // Seats first, then the bores through their floors; each stage is one
    // Boolean (`combine` joins tools apart from each other). Both stages are
    // kept as tool sets, in that order, so Pattern / Mirror can copy holes.
    let all_seats: Vec<ExactSolid> = seats.iter().chain(&deferred).cloned().collect();
    let sets: Vec<super::pattern::ToolSet> = [&all_seats, &bores]
        .into_iter()
        .filter(|t| !t.is_empty())
        .map(|t| super::pattern::ToolSet {
            tools: t.clone(),
            op: Operation::Cut,
            solids: h.solids.clone(),
        })
        .collect();
    let next = if seats.is_empty() {
        cur.to_vec()
    } else {
        combine(cur.to_vec(), seats, Operation::Cut, &h.solids, false)?
    };
    let next = combine(next, bores.clone(), Operation::Cut, &h.solids, false)?;
    if rims.is_empty() {
        return Ok((next, out, sets));
    }
    match chamfer_rims(&next, &rims, &h.id) {
        Ok(done) => Ok((done, out, sets)),
        Err(e) => {
            log::debug!("{}: countersink chamfer failed ({e}), cutting cones", h.id);
            let next = combine(cur.to_vec(), all_seats, Operation::Cut, &h.solids, false)?;
            let next = combine(next, bores, Operation::Cut, &h.solids, false)?;
            Ok((next, out, sets))
        }
    }
}

/// The chamfers of [`hole`]'s rims, each on the solid whose box holds its
/// circle.
fn chamfer_rims(
    solids: &[ExactSolid],
    rims: &[exact::CircleChamfer],
    prefix: &str,
) -> Result<Vec<ExactSolid>, String> {
    let mut left: Vec<&exact::CircleChamfer> = rims.iter().collect();
    let mut out = Vec::with_capacity(solids.len());
    for s in solids {
        let mine: Vec<exact::CircleChamfer> = match exact::bounds(s) {
            Some((lo, hi)) if !s.is_open() => {
                let inside = |p: &Point3| (0..3).all(|i| p[i] >= lo[i] && p[i] <= hi[i]);
                let (a, b): (Vec<_>, Vec<_>) = left.into_iter().partition(|c| inside(&c.at));
                left = b;
                a.into_iter().cloned().collect()
            }
            _ => Vec::new(),
        };
        out.push(if mine.is_empty() {
            s.clone()
        } else {
            exact::chamfer_circles(s, &mine, prefix)?
        });
    }
    if !left.is_empty() {
        return Err("a countersink rim is on no solid".into());
    }
    Ok(out)
}

/// Thread (`docs/spec/2.9-thread.md`): a cosmetic thread changes nothing;
/// a modeled one records its groove (cut after every feature). A modeled
/// external thread with a printing clearance also returns the ring that
/// shrinks the shaft to `major − 2·clearance` (cut exactly, at once).
pub(super) fn thread(
    th: &super::hole::ThreadFeature,
    body: &BRep,
) -> Result<(HoleOutput, Option<ExactSolid>), Fail> {
    use super::hole::CosmeticThread;
    let (cf, a, b) = th.stretch(body).map_err(User)?;
    let t = &th.spec;
    let mut out = HoleOutput::default();
    let len = b - a;
    let start = cf.start + cf.axis * a;
    if !t.modeled {
        out.cosmetic.push(CosmeticThread {
            feature: th.id.clone(),
            start,
            axis: cf.axis,
            radius: if cf.internal {
                t.major / 2.0
            } else {
                t.minor / 2.0
            },
            length: len,
            pitch: t.pitch,
            shown: th.display,
            face_radius: cf.radius,
            internal: cf.internal,
            right_hand: t.right_hand,
        });
        return Ok((out, None));
    }
    let p = t.pitch;
    let mut ring = None;
    let (section, zone_r) = if cf.internal {
        let s = t.internal_groove(cf.radius).map_err(User)?;
        let r0 = (cf.radius - 0.2 * p).max(0.0);
        (s, [r0, t.major / 2.0 + t.clearance + 0.3 * p])
    } else {
        let shrunk = t.major / 2.0 - t.clearance;
        if t.clearance > 0.0 && shrunk < cf.radius {
            let r_out = cf.radius + 0.25 * p;
            let end = if b >= cf.length - 1e-6 { len + p } else { len };
            ring = Some(
                exact::revolved_tool(
                    start,
                    cf.axis,
                    0.31,
                    &[[shrunk, -p], [r_out, -p], [r_out, end], [shrunk, end]],
                    &["Crest", "Clear", "Clear", "Crest"],
                    &th.id,
                )
                .map_err(Kernel)?,
            );
        }
        let s = t.external_groove(cf.radius.min(shrunk)).map_err(User)?;
        let root = s[0][0];
        (s, [(root - 0.25 * p).max(0.0), cf.radius + 0.25 * p])
    };
    let w = section.iter().map(|q| q[1].abs()).fold(0.0, Real::max);
    // The thread runs out past the start end of the face (into the air at
    // a free end) and stops square at its other end, unless it reaches it.
    let base = -p;
    let z1 = len + if b >= cf.length - 1e-6 { p } else { -w };
    out.grooves.push(exact::ThreadGroove {
        origin: start,
        axis: cf.axis,
        section,
        base,
        pitch: p,
        turns: (z1 - base) / p,
        left_hand: !t.right_hand,
        zone: [zone_r[0], zone_r[1], base - w - 0.25 * p, z1 + w + 0.25 * p],
        prefix: th.id.clone(),
        name: "Thread".into(),
        // The face's ends and the thread's.
        avoid: vec![0.0, len, -a, cf.length - a],
    });
    Ok((out, ring))
}

// Cuts the modeled threads into the solids they lie in (after every
/// feature, so no later Boolean meets their faceted zones). A failure is
/// the owning feature's health; the solids then stay as they were.
pub(super) fn finish_threads(
    solids: Vec<ExactSolid>,
    grooves: &[exact::ThreadGroove],
) -> (Vec<ExactSolid>, Vec<(String, String)>) {
    let mut failed = Vec::new();
    if grooves.is_empty() {
        return (solids, failed);
    }
    // Every groove must land in some solid; one that does not is the
    // owner's health (it was skipped silently when its zone's middle lay
    // above the face: a thin plate under a large part's start margin).
    let mut hit = vec![false; grooves.len()];
    let mut out = Vec::new();
    for s in solids {
        let Ok(m) = s.measure_full() else {
            out.push(s);
            continue;
        };
        let (lo, hi) = m.bbox;
        let mine: Vec<usize> = (0..grooves.len())
            .filter(|&i| {
                let (zlo, zhi) = grooves[i].zone_box();
                (0..3).all(|k| zlo[k] <= hi[k] + 1e-6 && zhi[k] >= lo[k] - 1e-6)
            })
            .collect();
        if mine.is_empty() {
            out.push(s);
            continue;
        }
        let set: Vec<exact::ThreadGroove> = mine.iter().map(|&i| grooves[i].clone()).collect();
        match exact::cut_threads(&s, &set) {
            Ok(pieces) => {
                for &i in &mine {
                    hit[i] = true;
                }
                out.extend(pieces);
            }
            // The zones miss this solid: another one may hold them.
            Err(e) if e.contains("not in the material") => out.push(s),
            Err(e) => {
                for &i in &mine {
                    hit[i] = true;
                    failed.push((
                        grooves[i].prefix.clone(),
                        format!(
                            "the geometry kernel cannot cut this modeled thread ({e}); use a cosmetic thread"
                        ),
                    ));
                }
                out.push(s);
            }
        }
    }
    for (g, h) in grooves.iter().zip(hit) {
        if !h {
            failed.push((
                g.prefix.clone(),
                "the modeled thread is not in the material".to_owned(),
            ));
        }
    }
    (out, failed)
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
            let mut back = if goes_on(a, -t) { ext } else { 0.0 };
            let mut ahead = if goes_on(b, t) { ext } else { 0.0 };
            // Partial: only `length` of the edge, `to_start` from the end
            // nearest the start point, ends square to the edge.
            let (a, b, len) = match &bl.partial {
                Some(p) => {
                    let s = Point3::from(p.start);
                    let from_a = (a - s).norm() <= (b - s).norm();
                    if !(p.to_start >= 0.0 && p.length > 1e-9) {
                        return Err(
                            "the partial chamfer needs a length and a start distance".into()
                        );
                    }
                    if p.to_start + p.length > len + 1e-9 {
                        return Err("the partial chamfer runs past the end of the edge".into());
                    }
                    back = 0.0;
                    ahead = 0.0;
                    if from_a {
                        let a2 = a + t * p.to_start;
                        (a2, a2 + t * p.length, p.length)
                    } else {
                        let b2 = b - t * p.to_start;
                        (b2 - t * p.length, b2, p.length)
                    }
                }
                None => (a, b, len),
            };
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
            assert!(
                t.elapsed().as_secs_f64() < 8.0
                    || std::env::var_os("CADCRAFT3D_NO_BUDGETS").is_some()
            );
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

/// The turning sense of a surface revolve against a solid one (checked by
/// `revolve_surface_lies_where_the_solid_does`).
const REVOLVE_SHEET_SENSE: Real = 1.0;

/// A profile loop without its lines on the revolve axis, as the open chains
/// between them (a surface revolve: an edge on the axis sweeps no face, and
/// the kernel takes a curve from the axis back to it, not a closed one
/// touching it). A loop off the axis stays as it is.
fn off_axis(
    lp: Vec<crate::kernel::ProfileSegment>,
    origin: nalgebra::Point2<Real>,
    dir: nalgebra::Vector2<Real>,
) -> Vec<Vec<crate::kernel::ProfileSegment>> {
    use crate::kernel::Segment2;
    let Some(d) = dir.try_normalize(1e-15) else {
        return vec![lp];
    };
    let size = lp
        .iter()
        .map(|s| s.geometry.point(0.0).coords.norm())
        .fold(1.0, Real::max);
    let on = |p: nalgebra::Point2<Real>| {
        let v = p - origin;
        (v.x * d.y - v.y * d.x).abs() < 1e-9 * size
    };
    let on_axis = |s: &crate::kernel::ProfileSegment| matches!(&s.geometry, Segment2::Line { a, b } if on(*a) && on(*b));
    let Some(first) = lp.iter().position(on_axis) else {
        return vec![lp];
    };
    // Start right after an axis line, then cut at every other one.
    let n = lp.len();
    let mut out = Vec::new();
    let mut cur = Vec::new();
    for k in 1..=n {
        let s = &lp[(first + k) % n];
        if on_axis(s) {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
        } else {
            cur.push(s.clone());
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Sheet metal Face (`docs/spec/5.6-sheet-metal.md`): the first profile of
/// the sketch as a plate of the rule's thickness; the sketch's lines name
/// its edges. Returns the solid and the sheet's state.
pub(super) fn sheet_face(
    sf: &super::sheet_metal::SheetFaceFeature,
    sketch: &Sketch,
    rule: &super::sheet_metal::SheetMetalDefaults,
) -> Result<(ExactSolid, super::sheet_metal::SheetState), Fail> {
    use crate::kernel::exact::sheet_metal as ksm;
    use crate::sketch::Entity;
    rule.validate().map_err(User)?;
    let regions = sketch.regions();
    let pr = sf
        .profiles
        .first()
        .ok_or_else(|| User("select the profile".into()))?;
    let region = pr
        .resolve(&regions)
        .ok_or_else(|| User("the profile no longer exists in the sketch".into()))?;
    let (outer, holes) = kernel_profile(region);
    let prof = exact::profile(&outer, &holes).map_err(User)?;
    let curves: Vec<ksm::BaseCurve> = sketch
        .entities()
        .filter_map(|(id, e)| match e {
            Entity::Line { start, end } => {
                let (a, b) = (sketch.point(*start)?, sketch.point(*end)?);
                Some((
                    super::extrude::entity_key(id),
                    [a.position.x, a.position.y],
                    [b.position.x, b.position.y],
                ))
            }
            _ => None,
        })
        .collect();
    let (solid, model) = ksm::face(
        &sf.id,
        &rule.values(),
        &sketch.plane,
        &prof,
        &curves,
        sf.flip,
    )
    .map_err(User)?;
    let faces = solid.face_names.len();
    Ok((solid, super::sheet_metal::SheetState::new(model, faces)))
}

/// Sheet metal Flange: bends on the named edges of its sheet; the sheet's
/// solid is rebuilt from its model.
/// The sheet `body` and its state, refused when another feature changed
/// it since the last sheet step (that change is not in the model).
fn sheet_of<'a>(
    body: &str,
    solids: &[ExactSolid],
    metal: &'a std::collections::HashMap<String, super::sheet_metal::SheetState>,
) -> Result<(usize, &'a super::sheet_metal::SheetState), Fail> {
    let k = solids
        .iter()
        .position(|s| s.name.as_deref() == Some(body))
        .ok_or_else(|| User(format!("the sheet {body} no longer exists")))?;
    let state = metal
        .get(body)
        .ok_or_else(|| User(format!("{body} is not a sheet metal body")))?;
    if solids[k].face_names.len() != state.faces {
        return Err(User(
            "a feature after the sheet changed it (holes and cuts other than the sheet metal Cut are not in the sheet model yet): put this feature before them"
                .into(),
        ));
    }
    Ok((k, state))
}

/// The open chain of a sketch's own straight lines, in order (Contour Flange).
fn open_chain(ps: &super::PartSketch) -> Result<Vec<[Real; 2]>, String> {
    use crate::sketch::{Entity, Style};
    let sk = &ps.sketch;
    let mut segs: Vec<([Real; 2], [Real; 2])> = Vec::new();
    for (id, e) in sk.entities() {
        if sk.style(id) != Style::Normal || ps.is_reference(id) {
            continue;
        }
        match e {
            Entity::Line { start, end } => {
                let (Some(a), Some(b)) = (sk.point(*start), sk.point(*end)) else {
                    continue;
                };
                segs.push(([a.position.x, a.position.y], [b.position.x, b.position.y]));
            }
            _ => return Err("only lines: the bends are made at their corners".into()),
        }
    }
    if segs.is_empty() {
        return Err("the sketch has no lines".into());
    }
    let same = |p: [Real; 2], q: [Real; 2]| (p[0] - q[0]).hypot(p[1] - q[1]) < 1e-6;
    let degree = |p: [Real; 2]| {
        segs.iter()
            .filter(|(a, b)| same(*a, p) || same(*b, p))
            .count()
    };
    let start = segs
        .iter()
        .flat_map(|(a, b)| [*a, *b])
        .find(|p| degree(*p) == 1)
        .ok_or_else(|| "the lines must form an open chain".to_owned())?;
    let mut pts = vec![start];
    let mut used = vec![false; segs.len()];
    let mut at = start;
    while let Some(k) =
        (0..segs.len()).find(|&k| !used[k] && (same(segs[k].0, at) || same(segs[k].1, at)))
    {
        used[k] = true;
        at = if same(segs[k].0, at) {
            segs[k].1
        } else {
            segs[k].0
        };
        pts.push(at);
    }
    if used.iter().any(|u| !u) {
        return Err("the lines must form one open chain (no branches, no gaps)".into());
    }
    Ok(pts)
}

/// A sheet operation's solids and, when the sheet model follows it, the
/// sheet's new state (a Cut of a sheet no longer in the model has none).
pub(super) type SheetStep = (
    Vec<ExactSolid>,
    Option<(String, super::sheet_metal::SheetState)>,
);

/// Hem, Fold, Contour Flange, Unfold / Refold, Cut. Returns the solids, the
/// sheet body changed (or made) and its new state.
pub(super) fn sheet_op(
    op: &super::sheet_metal::SheetOpFeature,
    solids: &[ExactSolid],
    metal: &std::collections::HashMap<String, super::sheet_metal::SheetState>,
    rule: &super::sheet_metal::SheetMetalDefaults,
    sketches: &[super::PartSketch],
) -> Result<SheetStep, Fail> {
    use super::sheet_metal::{ContourDirection, SheetOp, SheetState};
    use crate::kernel::exact::sheet_metal as ksm;
    let sketch_of = |id: &str| {
        sketches
            .iter()
            .find(|s| s.id == id)
            .ok_or_else(|| User(format!("sketch {id} is missing")))
    };
    let replace = |k: usize, solid: ExactSolid, model: ksm::SheetModel, body: &str| {
        let faces = solid.face_names.len();
        let mut next = solids.to_vec();
        next[k] = solid;
        (next, Some((body.to_owned(), SheetState::new(model, faces))))
    };
    let rv = rule.values();
    match &op.op {
        SheetOp::Hem {
            body,
            edges,
            length,
            gap,
            flip,
        } => {
            let (k, state) = sheet_of(body, solids, metal)?;
            if edges.is_empty() {
                return Err(User("select the edges".into()));
            }
            let mut pts = Vec::new();
            for e in edges {
                pts.push(
                    ksm::edge_point(&state.model, &e.name, Point3::from(e.hint))
                        .ok_or_else(|| User(format!("the edge {} no longer exists", e.name)))?,
                );
            }
            let gap = gap.unwrap_or(rv.thickness * 0.5);
            let (solid, model) =
                ksm::hem(&state.model, body, &op.id, &pts, *length, gap, *flip).map_err(User)?;
            Ok(replace(k, solid, model, body))
        }
        SheetOp::Fold {
            body,
            sketch,
            line,
            angle,
            radius,
            location,
            flip,
            flip_side,
        } => {
            let (k, state) = sheet_of(body, solids, metal)?;
            let ps = sketch_of(sketch)?;
            let sk = &ps.sketch;
            let (a, b) = sk
                .entities()
                .find_map(|(id, e)| match e {
                    crate::sketch::Entity::Line { start, end }
                        if super::extrude::entity_key(id) == *line =>
                    {
                        Some((sk.point(*start)?.position, sk.point(*end)?.position))
                    }
                    _ => None,
                })
                .ok_or_else(|| User(format!("the fold line {line} is gone from {sketch}")))?;
            let pl = &sk.plane;
            let (wa, wb) = (pl.point_at(a.x, a.y), pl.point_at(b.x, b.y));
            // The side that stays: left of the line, or right with Flip Side.
            let d = (b - a).normalize();
            let left = crate::sketch::Point2::new(-d.y, d.x);
            let s = if *flip_side { -1.0 } else { 1.0 };
            let mid = (a.coords + b.coords) * 0.5;
            let off = (rv.thickness * 2.0).max(1.0) * s;
            let fixed = pl.point_at(mid.x + left.x * off, mid.y + left.y * off);
            let r = radius.unwrap_or(rv.bend_radius);
            let (solid, model, notes) = ksm::fold(
                &state.model,
                body,
                &op.id,
                wa,
                wb,
                *angle,
                r,
                location.kernel(),
                *flip,
                Some(fixed),
            )
            .map_err(User)?;
            for n in notes {
                log::info!("{}: {n}", op.id);
            }
            Ok(replace(k, solid, model, body))
        }
        SheetOp::Contour {
            sketch,
            width,
            flip,
            direction,
            width2,
            centered,
        } => {
            rule.validate().map_err(User)?;
            let ps = sketch_of(sketch)?;
            let pts = open_chain(ps).map_err(User)?;
            if !(width.is_finite() && *width > 0.0) {
                return Err(User("the width must be positive".into()));
            }
            if *direction == ContourDirection::Asymmetric && !(width2.is_finite() && *width2 >= 0.0)
            {
                return Err(User("the second width must not be negative".into()));
            }
            let span = direction.span(*width, *width2);
            let (solid, model) =
                ksm::contour(&op.id, &rv, &ps.sketch.plane, &pts, span, *flip, *centered)
                    .map_err(User)?;
            let faces = solid.face_names.len();
            let mut next = solids.to_vec();
            next.push(solid);
            Ok((next, Some((op.id.clone(), SheetState::new(model, faces)))))
        }
        SheetOp::Unfold { body } | SheetOp::Refold { body } => {
            let (k, state) = sheet_of(body, solids, metal)?;
            let flat = matches!(op.op, SheetOp::Unfold { .. });
            let (solid, model) = ksm::set_flat(&state.model, body, flat).map_err(User)?;
            Ok(replace(k, solid, model, body))
        }
        SheetOp::CornerSeam {
            body,
            a,
            b,
            seam,
            gap,
            ratio,
        } => {
            let (k, state) = sheet_of(body, solids, metal)?;
            let gap = gap.unwrap_or(rv.gap);
            let (solid, model) =
                ksm::seam(&state.model, body, a, b, seam.kernel(), gap, *ratio).map_err(User)?;
            Ok(replace(k, solid, model, body))
        }
        SheetOp::Cut {
            sketch,
            profiles,
            extent,
            depth,
            across,
        } => {
            use super::sheet_metal::CutExtent;
            let mode = match (extent, across) {
                (_, true) => ksm::CutMode::AcrossBends,
                (CutExtent::Thickness, false) => ksm::CutMode::Face,
                (CutExtent::Distance, false) => ksm::CutMode::Depth(*depth),
                (CutExtent::ThroughAll, false) => ksm::CutMode::ThroughAll,
            };
            let ps = sketch_of(sketch)?;
            let regions = ps.own_regions();
            let mut profs = Vec::new();
            for pr in profiles {
                let region = pr
                    .resolve(&regions)
                    .ok_or_else(|| User("a profile no longer exists in the sketch".into()))?;
                let (outer, holes) = kernel_profile(region);
                profs.push(exact::profile(&outer, &holes).map_err(User)?);
            }
            if profs.is_empty() {
                return Err(User("select the profiles".into()));
            }
            // The first sheet (by name) the profiles cut.
            let mut names: Vec<&String> = metal.keys().collect();
            names.sort();
            let mut why = String::from("there is no sheet metal body");
            let mut modelled = false;
            for body in names {
                let Ok((k, state)) = sheet_of(body, solids, metal) else {
                    continue;
                };
                modelled = true;
                match ksm::cut(&state.model, body, &ps.sketch.plane, &profs, mode) {
                    Ok((solid, model)) => return Ok(replace(k, solid, model, body)),
                    Err(e) => why = e,
                }
            }
            if modelled || *across {
                return Err(User(why));
            }
            // A sheet another feature changed (a Fillet, a Hole) is no longer
            // in the sheet model: cut it as a solid, square to the sketch, as
            // deep as asked on both sides of the plane (the sketch lies on a
            // face, so one side is air). The flat pattern does not see it.
            let d = match mode {
                ksm::CutMode::Depth(d) => d,
                ksm::CutMode::ThroughAll => 1e4,
                _ => rv.thickness,
            };
            let mut next = solids.to_vec();
            let mut cut_any = false;
            for (n, prof) in profs.iter().enumerate() {
                let tool = exact::extrude(
                    &ps.sketch.plane,
                    prof,
                    -d,
                    d,
                    0.0,
                    &format!("{}#{n}", op.id),
                )
                .map_err(User)?;
                for s in next.iter_mut() {
                    if s.is_open() || !metal.contains_key(s.name.as_deref().unwrap_or("")) {
                        continue;
                    }
                    if !exact::meets(s, &tool) {
                        continue;
                    }
                    if let Some(r) = exact::boolean(s, &tool, exact::ExactOp::Cut).map_err(User)? {
                        *s = r;
                        cut_any = true;
                    }
                }
            }
            if !cut_any {
                return Err(User(why));
            }
            Ok((next, None))
        }
        SheetOp::CornerRound { body, sets } => {
            let (k, state) = sheet_of(body, solids, metal)?;
            let mut cuts = Vec::new();
            for s in sets {
                for c in &s.corners {
                    cuts.push((corner_at(body, c), ksm::CornerSize::Round(s.radius)));
                }
            }
            if cuts.is_empty() {
                return Err(User("select the corner edges".into()));
            }
            let (solid, model) =
                ksm::corner_cuts(&state.model, body, &op.id, &cuts).map_err(User)?;
            Ok(replace(k, solid, model, body))
        }
        SheetOp::CornerChamfer {
            body,
            corners,
            method,
            distance,
            angle,
            distance2,
            flip,
            edge,
        } => {
            use super::sheet_metal::ChamferMethod;
            let (k, state) = sheet_of(body, solids, metal)?;
            if corners.is_empty() {
                return Err(User("select the corner edges".into()));
            }
            let cuts: Vec<_> = corners
                .iter()
                .map(|c| {
                    let size = match method {
                        ChamferMethod::Distance => ksm::CornerSize::Chamfer(*distance, *distance),
                        ChamferMethod::TwoDistances if *flip => {
                            ksm::CornerSize::Chamfer(*distance2, *distance)
                        }
                        ChamferMethod::TwoDistances => {
                            ksm::CornerSize::Chamfer(*distance, *distance2)
                        }
                        ChamferMethod::DistanceAngle => {
                            // From the given face, else the first (the second with Flip).
                            let on_b = match edge {
                                Some(e) => *e == c.faces[1],
                                None => *flip,
                            };
                            ksm::CornerSize::DistanceAngle(*distance, *angle, on_b)
                        }
                    };
                    (corner_at(body, c), size)
                })
                .collect();
            let (solid, model) =
                ksm::corner_cuts(&state.model, body, &op.id, &cuts).map_err(User)?;
            Ok(replace(k, solid, model, body))
        }
    }
}

/// A Corner Round / Corner Chamfer corner for the sheet model: its rim faces
/// without the body's prefix.
fn corner_at(
    body: &str,
    c: &super::sheet_metal::CornerEdge,
) -> crate::kernel::exact::sheet_metal::CornerAt {
    let local = |n: &str| {
        n.strip_prefix(body)
            .and_then(|r| r.strip_prefix(':'))
            .unwrap_or(n)
            .to_owned()
    };
    crate::kernel::exact::sheet_metal::CornerAt {
        a: local(&c.faces[0]),
        b: local(&c.faces[1]),
        near: Point3::from(c.hint),
    }
}

/// Convert to Sheet Metal of the solid `sc.body`.
pub(super) fn sheet_convert(
    sc: &super::sheet_metal::SheetConvertFeature,
    solids: &[ExactSolid],
    rule: &super::sheet_metal::SheetMetalDefaults,
) -> Result<(Vec<ExactSolid>, super::sheet_metal::SheetState), Fail> {
    use crate::kernel::exact::sheet_metal as ksm;
    let k = solids
        .iter()
        .position(|s| s.name.as_deref() == Some(sc.body.as_str()))
        .ok_or_else(|| User(format!("the solid {} no longer exists", sc.body)))?;
    if sc.bends.is_empty() {
        return Err(User("select the edges that become bends".into()));
    }
    let hint = Point3::from(sc.face.hint);
    let faces = exact::faces_named(&solids[k], &sc.face.face, hint).map_err(User)?;
    let face = faces
        .first()
        .and_then(|&f| exact::face_point(&solids[k], f))
        .ok_or_else(|| User(format!("the face {} no longer exists", sc.face.face)))?;
    let body = exact::to_brep(solids).map_err(User)?;
    let index: Vec<crate::kernel::SolidId> = body.solids().map(|(id, _)| id).collect();
    let mut bends = Vec::with_capacity(sc.bends.len());
    for r in &sc.bends {
        let (_, pts) = edge_points(&body, &index, r).map_err(User)?;
        let p = pts.first().map(|(_, p)| *p).ok_or_else(|| {
            User(format!(
                "the edge between {} and {} no longer exists",
                r.faces[0], r.faces[1]
            ))
        })?;
        bends.push(p);
    }
    let (solid, model) =
        ksm::convert(&solids[k], &sc.id, &rule.values(), face, &bends).map_err(User)?;
    let faces = solid.face_names.len();
    let mut next = solids.to_vec();
    next[k] = solid;
    Ok((next, super::sheet_metal::SheetState::new(model, faces)))
}

/// A Flange's further bends: bend `k + 2` stands on the far edge of every
/// panel of the bend before it (on the same face as the edge that bend
/// stands on, the other face when it turns the other way), all panels of one
/// bend in one call so their corners
/// close as the first bend's do. Panels of bend `k` are tagged `<id>-<k>.<n>`
/// (the first bend's stay `<id>.<n>`).
fn flange_bends(
    fl: &super::sheet_metal::SheetFlangeFeature,
    mut model: crate::kernel::exact::sheet_metal::SheetModel,
    first: &[crate::kernel::exact::sheet_metal::FlangeEdge],
    radius: Real,
) -> Result<crate::kernel::exact::sheet_metal::SheetModel, Fail> {
    use crate::kernel::exact::sheet_metal as ksm;
    let mut prev = fl.id.clone();
    let mut near: Vec<Point3> = first.iter().map(|e| e.at).collect();
    // Each panel's side: a bend on the same face's edge as the bend before
    // turns the same way (a U), on the other face's the other way (a Z).
    let mut top: Vec<bool> = fl
        .edges
        .iter()
        .map(|e| !e.name.ends_with(":bottom"))
        .collect();
    for (j, b) in fl.bends.iter().enumerate() {
        let n = j + 2;
        if !(b.height.is_finite() && b.height > 0.0) {
            return Err(User(format!("bend {n}: the height must be positive")));
        }
        for t in &mut top {
            *t ^= b.flip;
        }
        let mut edges = Vec::with_capacity(near.len());
        for (k, p) in near.iter().enumerate() {
            let side = if top[k] { "top" } else { "bottom" };
            let name = format!("{prev}.{}:far:{side}", k + 1);
            let at = ksm::edge_point(&model, &name, *p)
                .ok_or_else(|| User(format!("bend {n}: bend {} has no far edge", n - 1)))?;
            edges.push(ksm::FlangeEdge {
                at,
                height: b.height,
                angle: b.angle,
                offsets: (0.0, 0.0),
            });
        }
        let tag = format!("{}-{n}", fl.id);
        let (m, notes) = ksm::flanges_model(
            &model,
            &tag,
            &edges,
            radius,
            fl.position.kernel_pub(),
            false,
            &ksm::FlangeCorners {
                shape: fl.corner_shape.map(|c| c.kernel_pub()),
                size: fl.corner_size,
                gap: fl.corner_gap,
                at_vertex: fl.relief_placement == super::sheet_metal::ReliefPlacement::Vertex,
                laser_corners: fl.laser_corners,
                seams: Vec::new(),
            },
        )
        .map_err(|e| User(format!("bend {n}: {e}")))?;
        for note in notes {
            log::info!("{tag}: {note}");
        }
        model = m;
        near = edges.iter().map(|e| e.at).collect();
        prev = tag;
    }
    Ok(model)
}

pub(super) fn sheet_flange(
    fl: &super::sheet_metal::SheetFlangeFeature,
    solids: &[ExactSolid],
    metal: &std::collections::HashMap<String, super::sheet_metal::SheetState>,
    rule: &super::sheet_metal::SheetMetalDefaults,
) -> Result<(Vec<ExactSolid>, super::sheet_metal::SheetState), Fail> {
    use crate::kernel::exact::sheet_metal as ksm;
    let k = solids
        .iter()
        .position(|s| s.name.as_deref() == Some(fl.body.as_str()))
        .ok_or_else(|| User(format!("the sheet {} no longer exists", fl.body)))?;
    let state = metal
        .get(&fl.body)
        .ok_or_else(|| User(format!("{} is not a sheet metal body", fl.body)))?;
    if solids[k].face_names.len() != state.faces {
        return Err(User(
            "a feature after the sheet changed it (holes and cuts are not in the sheet model yet): put the flanges before them"
                .into(),
        ));
    }
    if fl.edges.is_empty() {
        return Err(User("select the edges".into()));
    }
    if !(fl.height.is_finite() && fl.height > 0.0) {
        return Err(User("the height must be positive".into()));
    }
    let mut edges = Vec::with_capacity(fl.edges.len());
    for e in &fl.edges {
        let at = ksm::edge_point(&state.model, &e.name, Point3::from(e.hint))
            .ok_or_else(|| User(format!("the edge {} no longer exists", e.name)))?;
        let (a, b, _) = ksm::edge_ends(&state.model, at)
            .ok_or_else(|| User(format!("the edge {} no longer exists", e.name)))?;
        let offsets = fl.spans((b - a).norm()).map_err(User)?;
        edges.push(ksm::FlangeEdge {
            at,
            height: fl.height,
            angle: fl.angle,
            offsets,
        });
    }
    let radius = fl.radius.unwrap_or(rule.values().bend_radius);
    let (model, notes) = ksm::flanges_model(
        &state.model,
        &fl.id,
        &edges,
        radius,
        fl.position.kernel_pub(),
        fl.flip,
        &ksm::FlangeCorners {
            shape: fl.corner_shape.map(|c| c.kernel_pub()),
            size: fl.corner_size,
            gap: fl.corner_gap,
            at_vertex: fl.relief_placement == super::sheet_metal::ReliefPlacement::Vertex,
            laser_corners: fl.laser_corners,
            seams: fl
                .seams
                .iter()
                .map(|o| (o.a, o.b, o.seam.kernel(), o.gap, o.ratio))
                .collect(),
        },
    )
    .map_err(User)?;
    for n in notes {
        log::info!("{}: {n}", fl.id);
    }
    let model = flange_bends(fl, model, &edges, radius)?;
    let solid = ksm::sheet_solid(&model, &fl.body).map_err(User)?;
    let faces = solid.face_names.len();
    let mut next = solids.to_vec();
    next[k] = solid;
    Ok((next, super::sheet_metal::SheetState::new(model, faces)))
}
