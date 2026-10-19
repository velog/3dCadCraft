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

/// One stretch of a coil, between two turn angles (radians from the
/// start). `pitch`: a constant pitch without radial growth (the helical
/// sweep builds it); `None`: built through stations.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CoilPiece {
    /// Start angle.
    pub from: Real,
    /// End angle.
    pub to: Real,
    /// Constant pitch (mm per turn), if the stretch has one.
    pub pitch: Option<Real>,
}

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

/// Stations per radian of the lofted pieces (15° apart).
const LOFT_STEP: Real = std::f64::consts::PI / 12.0;
/// Most steps of one loft (the kernel takes at most 50 sections).
const LOFT_MAX: usize = 48;

/// A solid coil of `prof` (on `plane`) moved by `m` through `pieces` (in
/// order, each starting where the one before ends). Walls are named
/// `<prefix>:Side:<segment>`, the first section `<prefix>:Start` and the
/// last `<prefix>:End`.
pub fn coil(
    plane: &Plane,
    prof: &ExactProfile,
    m: &CoilMotion,
    pieces: &[CoilPiece],
    prefix: &str,
) -> Result<ExactSolid, String> {
    if !prof.region.holes.is_empty() {
        return Err("a coil profile with holes is not supported yet".into());
    }
    let (Some(first), Some(last)) = (pieces.first(), pieces.last()) else {
        return Err("the coil has no turns".into());
    };
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
    let mut bodies: Vec<sk::Body> = Vec::new();
    for p in pieces {
        let span = p.to - p.from;
        if !(span.is_finite() && span > 1e-9) {
            continue;
        }
        match p.pitch {
            Some(pitch) => {
                let pl = m.plane_at(plane, p.from)?;
                let k = d3(&m.axis);
                let (ax, pp) = if m.left_hand {
                    (k * -1.0, -pitch)
                } else {
                    (k, pitch)
                };
                let turns = span / std::f64::consts::TAU;
                let origin = v3(&m.apply(p.from, m.origin));
                let body = guarded("coil", || {
                    sk::sweep_helix(&pl, &region, origin, ax, pp, turns)
                })?;
                bodies.push(body);
            }
            None => {
                let steps = ((span / LOFT_STEP).ceil() as usize).max(2);
                let mut k0 = 0;
                while k0 < steps {
                    let k1 = (k0 + LOFT_MAX).min(steps);
                    // At least two steps per loft (three sections: smooth).
                    let k1 = if steps - k1 == 1 { k1 + 1 } else { k1 };
                    let mut secs = Vec::with_capacity(k1 - k0 + 1);
                    for k in k0..=k1 {
                        let a = p.from + span * k as Real / steps as Real;
                        secs.push((m.plane_at(plane, a)?, region.outer.clone()));
                    }
                    bodies.push(guarded("coil", || sk::loft(&secs))?);
                    k0 = k1;
                }
            }
        }
    }
    if bodies.is_empty() {
        return Err("the coil has no turns".into());
    }
    let start = m.plane_at(plane, first.from)?;
    let end = m.plane_at(plane, last.to)?;
    let pieces: Vec<ExactSolid> = bodies
        .into_iter()
        .map(|body| {
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
        })
        .collect();
    let mut it = pieces.into_iter();
    let mut out = it.next().ok_or("the coil has no turns")?;
    for p in it {
        out = boolean(&out, &p, ExactOp::Union)?
            .ok_or_else(|| "the coil pieces do not join".to_owned())?;
    }
    name_caps(&mut out, &start, &end, prefix)?;
    Ok(out)
}

/// Names the planar faces on the first and last sections `Start` / `End`
/// and any wall left without a name `Side`.
fn name_caps(s: &mut ExactSolid, start: &sg::Plane, end: &sg::Plane, prefix: &str) -> Result<(), String> {
    let faces = guarded("faces", || s.body.faces(1e-3))?;
    let on = |pl: &sg::Plane, c: sg::Vec3, n: Option<sg::Vec3>| {
        let nn = pl.normal();
        n.is_some_and(|n| n.cross(nn).len() < 1e-6) && (c - pl.origin).dot(nn).abs() < 1e-6
    };
    if s.face_names.len() < s.body.face_count() {
        s.face_names.resize(s.body.face_count(), None);
    }
    for f in faces {
        let Some(slot) = s.face_names.get_mut(f.index) else {
            continue;
        };
        if on(start, f.centroid, f.plane_normal) {
            *slot = Some(format!("{prefix}:Start"));
        } else if on(end, f.centroid, f.plane_normal) {
            *slot = Some(format!("{prefix}:End"));
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
            mesh.positions.push(v3(&m.apply(a, plane.point_at(q.x, q.y))));
        }
    }
    let id = |i: usize, j: usize| u32::try_from(i * n + j % n).unwrap_or(0);
    for i in 0..steps {
        for j in 0..n {
            mesh.triangles.push([id(i, j), id(i + 1, j), id(i + 1, j + 1)]);
            mesh.triangles.push([id(i, j), id(i + 1, j + 1), id(i, j + 1)]);
        }
    }
    // Caps: counter-clockwise triangles of the (counter-clockwise) section
    // run with the walls' edges at the start and against them at the end.
    let (_, tris) = crate::kernel::triangulate_with_holes(&poly, &[]);
    for t in &tris {
        mesh.triangles.push([id(0, t[0]), id(0, t[1]), id(0, t[2])]);
        mesh.triangles.push([id(steps, t[0]), id(steps, t[2]), id(steps, t[1])]);
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
    let z = revolved_tool(
        zone.origin,
        zone.axis,
        0.37,
        &[[r0, z0], [r1, z0], [r1, z1], [r0, z1]],
        &["CoilZone"; 4],
        s.name.as_deref().unwrap_or("Coil"),
    )?;
    let Some(inside) = boolean(s, &z, ExactOp::Intersect)? else {
        return Ok(None);
    };
    let vol = inside.measure_full().map(|m| m.volume).unwrap_or(0.0);
    if vol <= 1e-9 * zone_volume(zone).max(1.0) {
        return Ok(None);
    }
    let rest = if op == ExactOp::Intersect {
        None
    } else {
        boolean(s, &z, ExactOp::Cut)?
    };
    let kop = match op {
        ExactOp::Union => sk::BoolOp::Union,
        ExactOp::Cut => sk::BoolOp::Cut,
        ExactOp::Intersect => sk::BoolOp::Intersect,
    };
    let tol = (inside.body.size() * 5e-4).clamp(2e-3, 1e-2);
    let mut out: Vec<ExactSolid> = rest.into_iter().collect();
    let parts = if op == ExactOp::Union {
        vec![inside]
    } else {
        lumps(&inside)
    };
    for piece in parts {
        let r = guarded("coil", || sk::faceted_boolean(&piece.body, &mesh.0, kop, tol))?;
        let Some(body) = r else {
            continue;
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
        out.push(ExactSolid {
            name: s.name.clone(),
            body,
            face_names,
            profiles: Vec::new(),
        });
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

    fn circle_profile(cx: Real, cy: Real, r: Real) -> ExactProfile {
        let seg = ProfileSegment {
            geometry: Segment2::Arc {
                c: nalgebra::Point2::new(cx, cy),
                r,
                a0: 0.0,
                sweep: TAU,
            },
            name: "C".into(),
        };
        super::super::profile(&[seg], &[]).unwrap()
    }

    #[test]
    fn prototype_timings() {
        let plane = Plane::zx();
        let prof = circle_profile(0.0, 10.0, 1.0);
        let at = |a: Real| (5.0 * a / TAU, 0.0);
        let m = CoilMotion {
            origin: Point3::origin(),
            axis: Vector3::z(),
            left_hand: false,
            radial: Vector3::x(),
            at: &at,
        };
        let t = std::time::Instant::now();
        let s = coil(&plane, &prof, &m, &[CoilPiece { from: 0.0, to: 3.0 * TAU, pitch: Some(5.0) }], "Coil1").unwrap();
        let v = s.measure().unwrap();
        eprintln!("helix: {:?} in {:?} expect {}", v, t.elapsed(), PI * 2.0 * PI * 10.0 * 3.0 * (1.0f64 + (5.0 / (TAU * 10.0)).powi(2)).sqrt());
        eprintln!("{:?}", s.face_names);
        let t = std::time::Instant::now();
        let s = coil(&plane, &prof, &m, &[CoilPiece { from: 0.0, to: TAU, pitch: Some(5.0) }, CoilPiece { from: TAU, to: 2.0 * TAU, pitch: None }], "Coil1").unwrap();
        let v = s.measure().unwrap();
        eprintln!("helix+loft: {:?} in {:?}", v, t.elapsed());
        eprintln!("{:?}", s.face_names);
    }
}
