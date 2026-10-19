//! Coils (`docs/spec/5.3-coil.md`): a profile screwed about an axis, with a
//! pitch that may change along the way (closed ends) and a radius that may
//! grow with the height (taper).
//!
//! The coil is built in pieces: constant-pitch stretches without taper go
//! through SolidCraft's helical sweep (`sk::sweep_helix`, smooth B-spline
//! walls), the others (pitch transitions, tapered turns) are smooth lofts
//! through the profile placed at stations 15° apart (`sk::loft`, at most 48
//! steps each). Pieces meet on identical planar sections and are joined.
//!
//! Booleans of a coil with a body go through a faceted zone (as modeled
//! threads, see [`super::cut_threads`]): the exact kernel's Boolean with
//! helical walls took seconds per turn. Only the material inside the coil's
//! annular zone is faceted; the rest of the body stays exact.

use super::{
    ExactOp, ExactProfile, ExactSolid, FaceOrigin, Point3, Real, Vector3, boolean, d3, guarded,
    lumps, names_from, revolved_tool, sg, sk, v3,
};
use crate::kernel::Plane;

/// The screw motion of a coil: at turn angle `a` the profile is turned by
/// `a` about the axis (the other way for a left-hand coil), lifted by
/// `height(a)` along it and pushed out from it by `grow(a)`.
pub struct CoilMotion<'a> {
    /// A point on the axis.
    pub origin: Point3,
    /// Unit axis direction; the coil climbs along it.
    pub axis: Vector3,
    /// Left-hand coil (turns clockwise about the axis).
    pub left_hand: bool,
    /// Unit direction from the axis to the profile's centre at the start
    /// (the direction the taper pushes the profile).
    pub radial: Vector3,
    /// `(height, growth)` at a turn angle.
    pub at: &'a (dyn Fn(Real) -> (Real, Real) + Sync),
}

impl CoilMotion<'_> {
    /// The rigid motion (plus radial push) of the profile at angle `a`.
    fn apply(&self, a: Real, p: Point3) -> Point3 {
        let (h, g) = (self.at)(a);
        let rot = self.rotation(a);
        self.origin + rot * (p - self.origin + self.radial * g) + self.axis * h
    }

    fn rotation(&self, a: Real) -> nalgebra::Rotation3<Real> {
        let s = if self.left_hand { -1.0 } else { 1.0 };
        nalgebra::Rotation3::from_axis_angle(&nalgebra::Unit::new_normalize(self.axis), s * a)
    }

    /// The profile plane at angle `a`.
    fn plane_at(&self, plane: &Plane, a: Real) -> Result<sg::Plane, String> {
        let rot = self.rotation(a);
        let o = self.apply(a, plane.origin);
        sg::Plane::new(
            v3(&o),
            d3(&(rot * plane.x_axis.into_inner())),
            d3(&(rot * plane.y_axis().into_inner())),
        )
        .ok_or_else(|| "degenerate coil station".to_owned())
    }
}

/// Finest station spacing of a lofted coil (15°).
const LOFT_STEP: Real = std::f64::consts::PI / 12.0;
/// Coarsest station spacing a loft may use (45°: the cubic sections stay
/// within about 1e-4 of the radius); longer coils are faceted.
const LOFT_COARSE: Real = std::f64::consts::FRAC_PI_4;
/// Most steps of one loft (the kernel takes at most 50 sections).
const LOFT_MAX: usize = 48;
/// Stations per turn of a faceted coil body.
pub const MESH_STATIONS: usize = 36;

/// A solid coil of `prof` (on `plane`) moved by `m` from angle 0 to
/// `total`. `pitch`: the coil has one constant pitch and no taper — the
/// kernel's helical sweep builds it; otherwise a smooth loft through
/// stations (15° to 45° apart). When neither works (too many turns for one
/// loft, a kernel failure) the coil is a faceted body of `polygon` (the
/// profile's outer loop sampled, sketch coordinates). Walls are named
/// `<prefix>:Side:<segment>` (`<prefix>:Side` when faceted), the first
/// section `<prefix>:Start`, the last `<prefix>:End`.
#[allow(clippy::too_many_arguments)]
pub fn coil(
    plane: &Plane,
    prof: &ExactProfile,
    polygon: &[nalgebra::Point2<Real>],
    m: &CoilMotion,
    total: Real,
    pitch: Option<Real>,
    prefix: &str,
) -> Result<ExactSolid, String> {
    if !prof.region.holes.is_empty() {
        return Err("a coil profile with holes is not supported yet".into());
    }
    if !(total.is_finite() && total > 1e-9) {
        return Err("the coil has no turns".into());
    }
    // The sweeps need distinct segment ends: a full circle becomes halves.
    let mut names = Vec::new();
    let mut segs = Vec::new();
    for (i, s) in prof.region.outer.segs.iter().enumerate() {
        let name = prof.names.get(i).cloned().unwrap_or_else(|| i.to_string());
        match *s {
            sg::Seg2::Arc {
                center,
                radius,
                start,
                sweep,
            } if sweep.abs() > std::f64::consts::PI + 1e-9 => {
                for k in 0..2 {
                    segs.push(sg::Seg2::Arc {
                        center,
                        radius,
                        start: start + sweep * k as Real / 2.0,
                        sweep: sweep / 2.0,
                    });
                    names.push(name.clone());
                }
            }
            s => {
                segs.push(s);
                names.push(name);
            }
        }
    }
    let region = sg::Region2 {
        outer: sg::Loop2 { segs },
        holes: Vec::new(),
    };
    let smooth: Result<sk::Body, String> = match pitch {
        Some(pitch) => {
            let pl = m.plane_at(plane, 0.0)?;
            let k = d3(&m.axis);
            let (ax, pp) = if m.left_hand {
                (k * -1.0, -pitch)
            } else {
                (k, pitch)
            };
            let turns = total / std::f64::consts::TAU;
            let origin = v3(&m.origin);
            guarded("coil", || {
                sk::sweep_helix(&pl, &region, origin, ax, pp, turns)
            })
        }
        None => {
            let nseg = region.outer.segs.len();
            // Smooth walls only: a ruled result (one face per segment and
            // station) cuts the corners — 2.5 % of the volume at 22.5°.
            let smooth_only = |b: sk::Body| {
                if b.face_count() <= nseg + 2 {
                    Ok(b)
                } else {
                    Err("the kernel made ruled walls".to_owned())
                }
            };
            // A round section looks the same however it turns: a sweep
            // along the centre's path (a rotation-minimising frame) is
            // smooth for any number of turns.
            let round = round_section(&region.outer);
            let swept = if let Some(c) = round {
                let pl = m.plane_at(plane, 0.0)?;
                let steps = ((total / (std::f64::consts::PI / 36.0)).ceil() as usize).max(8);
                let centre = plane.point_at(c.x, c.y);
                let path: Vec<sg::Vec3> = (0..=steps)
                    .map(|k| v3(&m.apply(total * k as Real / steps as Real, centre)))
                    .collect();
                guarded("coil", || sk::sweep_path(&pl, &region, &path)).and_then(smooth_only)
            } else {
                Err("not a round section".into())
            };
            swept.or_else(|_| {
                let steps = ((total / LOFT_STEP).ceil() as usize).clamp(2, LOFT_MAX);
                if total / steps as Real > LOFT_COARSE + 1e-9 {
                    Err("too many turns for one loft".into())
                } else {
                    let mut secs = Vec::with_capacity(steps + 1);
                    for k in 0..=steps {
                        let a = total * k as Real / steps as Real;
                        secs.push((m.plane_at(plane, a)?, region.outer.clone()));
                    }
                    guarded("coil", || sk::loft(&secs)).and_then(smooth_only)
                }
            })
        }
    };
    let start = m.plane_at(plane, 0.0)?;
    let end = m.plane_at(plane, total)?;
    let mut out = match smooth {
        Ok(body) => {
            let face_names = names_from(&body, &[], &|o| match o {
                FaceOrigin::Profile { segment } => {
                    names.get(*segment).map(|n| format!("{prefix}:Side:{n}"))
                }
                _ => None,
            });
            ExactSolid {
                name: Some(prefix.to_owned()),
                body,
                face_names,
                profiles: Vec::new(),
            }
        }
        Err(e) => {
            log::debug!("coil {prefix}: faceted body ({e})");
            let mesh = coil_mesh(plane, polygon, m, 0.0, total, MESH_STATIONS)?;
            let body = guarded("coil", || {
                sk::mesh_body(&mesh.0.positions, &mesh.0.triangles)
            })?;
            let n = body.face_count();
            ExactSolid {
                name: Some(prefix.to_owned()),
                body,
                face_names: vec![None; n],
                profiles: Vec::new(),
            }
        }
    };
    let c = polygon
        .iter()
        .fold(nalgebra::Vector2::zeros(), |acc, p| acc + p.coords)
        / polygon.len().max(1) as Real;
    let c = plane.point_at(c.x, c.y);
    name_caps(
        &mut out,
        [&start, &end],
        [m.apply(0.0, c), m.apply(total, c)],
        prefix,
    )?;
    Ok(out)
}

/// The centre of a loop made only of arcs of one circle.
fn round_section(lp: &sg::Loop2) -> Option<nalgebra::Point2<Real>> {
    let mut c: Option<(sg::Vec2, Real)> = None;
    for s in &lp.segs {
        let sg::Seg2::Arc { center, radius, .. } = *s else {
            return None;
        };
        match c {
            None => c = Some((center, radius)),
            Some((c0, r0)) => {
                if (c0 - center).len() > 1e-9 || (r0 - radius).abs() > 1e-9 {
                    return None;
                }
            }
        }
    }
    c.map(|(p, _)| nalgebra::Point2::new(p.x, p.y))
}

/// Names the planar faces lying on the first and last sections (on their
/// planes, nearer the section's centre) `Start` / `End`, and any wall left
/// without a name `Side`. Both sections can lie in one plane (whole turns
/// about an axis in the profile plane).
fn name_caps(
    s: &mut ExactSolid,
    planes: [&sg::Plane; 2],
    centres: [Point3; 2],
    prefix: &str,
) -> Result<(), String> {
    let tol = (s.body.size() * 1e-3).max(1e-3);
    let faces = guarded("faces", || s.body.faces(tol))?;
    let on = |pl: &sg::Plane, c: sg::Vec3, n: Option<sg::Vec3>| {
        let nn = pl.normal();
        n.is_some_and(|n| n.cross(nn).len() < 1e-6) && (c - pl.origin).dot(nn).abs() < 1e-5
    };
    if s.face_names.len() < s.body.face_count() {
        s.face_names.resize(s.body.face_count(), None);
    }
    for f in faces {
        let Some(slot) = s.face_names.get_mut(f.index) else {
            continue;
        };
        let c = Point3::new(f.centroid.x, f.centroid.y, f.centroid.z);
        let d = [(c - centres[0]).norm(), (c - centres[1]).norm()];
        let k = usize::from(d[1] < d[0]);
        // A round section swept along its centre's path ends turned a
        // little out of the station plane, centred on the station.
        let centred = f.plane_normal.is_some() && d[k] < 1e-6 * (1.0 + centres[k].coords.norm());
        if centred || on(planes[k], f.centroid, f.plane_normal) {
            *slot = Some(format!("{prefix}:{}", ["Start", "End"][k]));
        } else if slot.is_none() {
            *slot = Some(format!("{prefix}:Side"));
        }
    }
    Ok(())
}

/// A closed triangle mesh of the coil (outward), for the faceted zone
/// Booleans: `polygon` (the profile's outer loop on `plane`, sketch
/// coordinates) at `stations_per_turn` stations per turn from angle `a0` to
/// `a1`, capped at both ends.
pub fn coil_mesh(
    plane: &Plane,
    polygon: &[nalgebra::Point2<Real>],
    m: &CoilMotion,
    a0: Real,
    a1: Real,
    stations_per_turn: usize,
) -> Result<CoilMesh, String> {
    let n = polygon.len();
    if !(3..=4096).contains(&n) {
        return Err("the coil section is not valid".into());
    }
    let span = a1 - a0;
    if !(span.is_finite() && span > 0.0) {
        return Err("the coil has no turns".into());
    }
    let steps = ((span / std::f64::consts::TAU * stations_per_turn as Real).ceil() as usize)
        .clamp(2, 200_000);
    let mut poly = polygon.to_vec();
    let area: Real = (0..n)
        .map(|i| {
            let (p, q) = (poly[i], poly[(i + 1) % n]);
            p.x * q.y - q.x * p.y
        })
        .sum();
    if area < 0.0 {
        poly.reverse();
    }
    let mut mesh = sg::Mesh::default();
    for i in 0..=steps {
        let a = a0 + span * i as Real / steps as Real;
        for q in &poly {
            mesh.positions
                .push(v3(&m.apply(a, plane.point_at(q.x, q.y))));
        }
    }
    let id = |i: usize, j: usize| u32::try_from(i * n + j % n).unwrap_or(0);
    for i in 0..steps {
        for j in 0..n {
            mesh.triangles
                .push([id(i, j), id(i + 1, j), id(i + 1, j + 1)]);
            mesh.triangles
                .push([id(i, j), id(i + 1, j + 1), id(i, j + 1)]);
        }
    }
    // Caps: counter-clockwise triangles of the (counter-clockwise) section
    // run with the walls' edges at the start and against them at the end.
    let (_, tris) = crate::kernel::triangulate_with_holes(&poly, &[]);
    for t in &tris {
        mesh.triangles.push([id(0, t[0]), id(0, t[1]), id(0, t[2])]);
        mesh.triangles
            .push([id(steps, t[0]), id(steps, t[2]), id(steps, t[1])]);
    }
    mesh.tri_face = vec![0; mesh.triangles.len()];
    if mesh.measure().volume < 0.0 {
        for t in &mut mesh.triangles {
            t.swap(1, 2);
        }
    }
    Ok(CoilMesh(mesh))
}

/// A coil's closed triangle mesh (see [`coil_mesh`]).
#[derive(Clone)]
pub struct CoilMesh(sg::Mesh);

impl CoilMesh {
    /// Enclosed volume (mm³).
    pub fn volume(&self) -> Real {
        self.0.measure().volume
    }
}

/// The annular zone `[r_in, r_out, z_lo, z_hi]` about an axis that holds a
/// coil, for [`coil_boolean`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CoilZone {
    /// A point on the axis (z = 0).
    pub origin: Point3,
    /// Unit axis.
    pub axis: Vector3,
    /// Radii and heights.
    pub zone: [Real; 4],
}

/// `op` of the coil with the solid `s` through the faceted zone: `s` is
/// split into its exact rest (outside the zone) and the zone's material,
/// and only the latter meets the coil's mesh. Returns the pieces (the
/// exact rest and the faceted zone part, touching along the zone's walls),
/// or `Ok(None)` when the coil does not reach into the material (a Join is
/// then exact: the coil apart from the body or sitting on a face). The
/// coil's faces are named `name`.
pub fn coil_boolean(
    s: &ExactSolid,
    mesh: &CoilMesh,
    zone: &CoilZone,
    op: ExactOp,
    name: &str,
) -> Result<Option<Vec<ExactSolid>>, String> {
    let [r0, r1, z0, z1] = zone.zone;
    if !(r0 >= 0.0 && r1 > r0 && z1 > z0) {
        return Err("the coil zone is empty".into());
    }
    let prefix = s.name.as_deref().unwrap_or("Coil");
    let ring = |a: Real, b: Real, k: usize| {
        revolved_tool(
            zone.origin,
            zone.axis,
            0.37 + 0.613 * k as Real,
            &[[r0, a], [r1, a], [r1, b], [r0, b]],
            &["CoilZone"; 4],
            prefix,
        )
    };
    let whole = ring(z0, z1, 0)?;
    let kop = match op {
        ExactOp::Union => sk::BoolOp::Union,
        ExactOp::Cut => sk::BoolOp::Cut,
        ExactOp::Intersect => sk::BoolOp::Intersect,
    };
    let facet = |piece: &ExactSolid| -> Result<Option<ExactSolid>, String> {
        let tol = (piece.body.size() * 5e-4).clamp(2e-3, 1e-2);
        let r = guarded("coil", || {
            sk::faceted_boolean(&piece.body, &mesh.0, kop, tol)
        })?;
        let Some(body) = r else {
            return Ok(None);
        };
        let n = body.face_count();
        let mut face_names = vec![None; n];
        if let Some(h) = body.history() {
            for (i, fp) in h.faces.iter().enumerate().take(n) {
                face_names[i] = fp.origins.iter().find_map(|o| match o {
                    FaceOrigin::Face { operand: 0, face } => {
                        piece.face_names.get(*face).cloned().flatten()
                    }
                    FaceOrigin::Face { operand: 1, .. } => Some(name.to_owned()),
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
    };
    if op == ExactOp::Union {
        // One piece: the coil joins the zone's material once.
        let Some(inside) = boolean(s, &whole, ExactOp::Intersect)? else {
            return Ok(None);
        };
        let vol = inside.measure_full().map(|m| m.volume).unwrap_or(0.0);
        if vol <= 1e-9 * zone_volume(zone).max(1.0) {
            return Ok(None);
        }
        let rest = boolean(s, &whole, ExactOp::Cut)?;
        let mut out: Vec<ExactSolid> = rest.into_iter().collect();
        out.extend(facet(&inside)?);
        return Ok(Some(out));
    }
    if !super::meets(s, &whole) {
        return Ok(None);
    }
    // Cut / Intersect: the zone in slabs across the axis (about 4 mm high,
    // one per core, at most 8), each slab's material cut on a thread of its
    // own — the exact Booleans with the rings and the faceted Booleans both
    // take seconds; the rest (outside the zone) is made meanwhile.
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    let n = (((z1 - z0) / 4.0).ceil() as usize).clamp(1, cores.clamp(1, 8));
    let rings: Vec<ExactSolid> = (0..n)
        .map(|k| {
            let a = z0 + (z1 - z0) * k as Real / n as Real;
            let b = z0 + (z1 - z0) * (k + 1) as Real / n as Real;
            ring(a, b, k + 1)
        })
        .collect::<Result<_, _>>()?;
    type Slab = Result<Vec<ExactSolid>, String>;
    let (rest, slabs): (Result<Option<ExactSolid>, String>, Vec<Slab>) = std::thread::scope(|sc| {
        let rest = (op == ExactOp::Cut).then(|| sc.spawn(|| boolean(s, &whole, ExactOp::Cut)));
        let jobs: Vec<_> = rings
            .iter()
            .map(|z| {
                let facet = &facet;
                sc.spawn(move || -> Slab {
                    let Some(inside) = boolean(s, z, ExactOp::Intersect)? else {
                        return Ok(Vec::new());
                    };
                    let mut out = Vec::new();
                    for piece in lumps(&inside) {
                        out.extend(facet(&piece)?);
                    }
                    Ok(out)
                })
            })
            .collect();
        let lost = "coil: internal error in the geometry kernel";
        let rest = match rest {
            Some(h) => h.join().unwrap_or_else(|_| Err(lost.to_owned())),
            None => Ok(None),
        };
        let slabs = jobs
            .into_iter()
            .map(|h| h.join().unwrap_or_else(|_| Err(lost.to_owned())))
            .collect();
        (rest, slabs)
    });
    let mut out: Vec<ExactSolid> = rest?.into_iter().collect();
    for sl in slabs {
        out.extend(sl?);
    }
    Ok(Some(out))
}

fn zone_volume(z: &CoilZone) -> Real {
    let [r0, r1, z0, z1] = z.zone;
    std::f64::consts::PI * (r1 * r1 - r0 * r0) * (z1 - z0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::extrude::{ProfileSegment, Segment2};
    use std::f64::consts::{PI, TAU};

    fn circle_profile(cx: Real, cy: Real, r: Real) -> (ExactProfile, Vec<nalgebra::Point2<Real>>) {
        let seg = ProfileSegment {
            geometry: Segment2::Arc {
                c: nalgebra::Point2::new(cx, cy),
                r,
                a0: 0.0,
                sweep: TAU,
            },
            name: "C".into(),
        };
        let poly = (0..48)
            .map(|k| {
                let t = TAU * k as Real / 48.0;
                nalgebra::Point2::new(cx + r * t.cos(), cy + r * t.sin())
            })
            .collect();
        (super::super::profile(&[seg], &[]).unwrap(), poly)
    }

    fn brep_volume(s: &ExactSolid) -> Real {
        let b = super::super::to_brep(std::slice::from_ref(s)).unwrap();
        b.faces()
            .filter_map(|(_, f)| f.mesh.as_ref())
            .flat_map(|m| {
                m.triangles.iter().map(|t| {
                    let [p, q, r] = t.map(|i| m.positions[i as usize].coords);
                    p.dot(&q.cross(&r)) / 6.0
                })
            })
            .sum()
    }

    /// Helical sweep, round-section sweep and faceted body: the same volume
    /// (a meridian section screwed about the axis: area × centre circle ×
    /// turns), the caps named apart although they lie in one plane.
    #[test]
    fn coil_bodies_and_names() {
        let plane = Plane::zx();
        let (prof, poly) = circle_profile(0.0, 10.0, 1.0);
        let lin = |a: Real| (5.0 * a / TAU, 0.0);
        let m = CoilMotion {
            origin: Point3::origin(),
            axis: Vector3::z(),
            left_hand: false,
            radial: Vector3::x(),
            at: &lin,
        };
        let want = PI * TAU * 10.0 * 3.0;
        for pitch in [Some(5.0), None] {
            let s = coil(&plane, &prof, &poly, &m, 3.0 * TAU, pitch, "Coil1").unwrap();
            let v = brep_volume(&s);
            // Display triangles (0.01 mm) sit inside the round walls.
            assert!((v - want).abs() < 0.012 * want, "{pitch:?}: {v} vs {want}");
            let names: Vec<_> = s.face_names.iter().flatten().cloned().collect();
            assert!(names.contains(&"Coil1:Start".to_owned()), "{names:?}");
            assert!(
                names.contains(&"Coil1:End".to_owned()),
                "{pitch:?} {names:?}
{}",
                s.debug_topology()
            );
            assert!(
                names.iter().any(|n| n.starts_with("Coil1:Side:C")),
                "{names:?}"
            );
        }
        // The mesh: closed and outward, the polygon's area × path.
        let mesh = coil_mesh(&plane, &poly, &m, 0.0, 3.0 * TAU, 36).unwrap();
        let area = 0.5 * 48.0 * (TAU / 48.0).sin();
        let mv = area * TAU * 10.0 * 3.0;
        assert!(
            (mesh.volume() - mv).abs() < 0.01 * mv,
            "{} vs {mv}",
            mesh.volume()
        );
    }
}
