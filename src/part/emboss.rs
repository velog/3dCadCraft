//! Emboss / Engrave (`docs/spec/2.17-emboss.md`): a [`BodyOp`](super::BodyOp) variant.
//!
//! *From Face*: the profile, seen along the sketch's normal, raises or recesses the first face
//! the direction reaches by the depth; the raised top is that face offset, the walls run along
//! the direction. *From Plane*: no depth; where the sketch lies outside the body the profile
//! grows from the plane to the body, where it lies inside it cuts away everything on that side.
//! A positive taper widens the tool away from the sketch.
//!
//! Faces: tools `<id>:<k>:…` (`k` = the profile, from 1), so `<id>:<k>:End` is a top or a floor.

use serde::{Deserialize, Serialize};

use super::boolean::Operation;
use super::exact::{Fail, combine};
use super::extrude::{ExtrudeFeature, ProfileRef, default_profiles};
use super::rib::Rays;
use super::shell::{FaceRef, face_near};
use crate::kernel::exact::{self, ExactOp, ExactSolid};
use crate::kernel::{BRep, FaceId, Point3, Real, Surface, Vector3};
use crate::sketch::{ProfileCurve, Region, Sketch};

/// The three types of the command.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum EmbossKind {
    /// Raises the profile above the face.
    #[default]
    EmbossFromFace,
    /// Recesses the profile into the face.
    EngraveFromFace,
    /// Adds and removes material from the sketch plane to the body.
    FromPlane,
}

/// Which way it goes from the sketch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum EmbossSide {
    /// Along the sketch's normal.
    #[default]
    Direction1,
    /// Against it.
    Direction2,
    /// Both ways (From Plane only).
    Symmetric,
}

/// Emboss / Engrave.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EmbossOp {
    /// The profile's sketch.
    pub sketch: String,
    /// The regions; empty = every region of the sketch.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profiles: Vec<ProfileRef>,
    /// Emboss, Engrave or From Plane.
    #[serde(default)]
    pub style: EmbossKind,
    /// Raised or recessed by (mm; parameter `depth`; the face types).
    pub depth: Real,
    /// Direction.
    #[serde(default)]
    pub side: EmbossSide,
    /// Taper (radians, positive = wider away from the sketch; parameter `taper`; From Plane).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub taper: Real,
    /// Wrap to Face: the face the profile is rolled onto (the face types).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wrap: Option<FaceRef>,
}

fn is_zero(v: &Real) -> bool {
    *v == 0.0
}

impl EmbossOp {
    /// An Emboss from Face of every region of `sketch`.
    pub fn new(sketch: impl Into<String>, depth: Real) -> Self {
        Self {
            sketch: sketch.into(),
            profiles: Vec::new(),
            style: EmbossKind::default(),
            depth,
            side: EmbossSide::default(),
            taper: 0.0,
            wrap: None,
        }
    }

    /// Refuses a definition that cannot be built (before any geometry).
    pub fn validate(&self) -> Result<(), String> {
        if self.sketch.is_empty() {
            return Err("select the profile".into());
        }
        if self.style != EmbossKind::FromPlane {
            if self.depth <= 0.0 {
                return Err("the depth must be positive".into());
            }
            if self.side == EmbossSide::Symmetric {
                return Err("Symmetric belongs to Emboss/Engrave from Plane".into());
            }
        }
        if self.taper.abs() >= 89f64.to_radians() {
            return Err("the taper must stay below 89 degrees".into());
        }
        Ok(())
    }

    /// The browser's tooltip.
    pub fn describe(&self) -> String {
        match self.style {
            EmbossKind::EmbossFromFace => format!("Emboss of {} by {} mm", self.sketch, self.depth),
            EmbossKind::EngraveFromFace => {
                format!("Engrave of {} by {} mm", self.sketch, self.depth)
            }
            EmbossKind::FromPlane => format!("Emboss/Engrave from the plane of {}", self.sketch),
        }
    }

    fn directions(&self) -> Vec<Real> {
        match self.side {
            EmbossSide::Direction1 => vec![1.0],
            EmbossSide::Direction2 => vec![-1.0],
            EmbossSide::Symmetric => vec![1.0, -1.0],
        }
    }
}

/// One profile's points in space: the inside point first, then the outline.
fn samples(sketch: &Sketch, r: &Region) -> Vec<Point3> {
    let mut uv = vec![r.interior_point()];
    uv.extend(r.outer.polygon(0.05));
    uv.iter().map(|p| sketch.plane.point_at(p.x, p.y)).collect()
}

/// Where the ray from `p` along `d` meets the body's skin: the point itself when it lies on it,
/// else the first crossing. `None`: it never does.
fn first_skin(rays: &Rays, p: Point3, d: Vector3) -> Option<Real> {
    let (ahead, behind) = (rays.entry(p, d), rays.entry(p, -d));
    let inside = |x: &Option<(Real, Real)>| x.is_some_and(|v| v.0 == 0.0);
    // On the skin: inside one way, nothing the other way.
    if (ahead.is_none() && inside(&behind)) || (inside(&ahead) && behind.is_none()) {
        return Some(0.0);
    }
    // Inside the material: where the ray leaves it; outside: where it enters.
    ahead.map(|(entry, exit)| if entry == 0.0 { exit } else { entry })
}

/// The body's display triangles with their faces (`pick_face` knows only planar faces).
fn skin(body: &BRep) -> Vec<(FaceId, [Point3; 3])> {
    let opts = crate::kernel::TessellationOptions::default();
    let mut out = Vec::new();
    for (sid, s) in body.solids() {
        if s.open {
            continue;
        }
        let Ok(m) = body.tessellate_solid(sid, &opts) else {
            continue;
        };
        for (f, range) in &m.face_triangles {
            for t in &m.faces.triangles[range.clone()] {
                out.push((*f, t.map(|i| m.faces.positions[i as usize])));
            }
        }
    }
    out
}

/// The face under `q`, looking along `d`: the nearest triangle the ray crosses.
fn face_under(tris: &[(FaceId, [Point3; 3])], q: Point3, d: Vector3) -> Option<FaceId> {
    let from = q - d;
    let mut best: Option<(Real, FaceId)> = None;
    for (f, [a, b, c]) in tris {
        let (e1, e2) = (b - a, c - a);
        let h = d.cross(&e2);
        let det = e1.dot(&h);
        if det.abs() < 1e-14 {
            continue;
        }
        let s = from - a;
        let u = s.dot(&h) / det;
        let w = s.cross(&e1);
        let v = d.dot(&w) / det;
        if !(-1e-9..=1.0 + 1e-9).contains(&u) || v < -1e-9 || u + v > 1.0 + 1e-9 {
            continue;
        }
        let t = e2.dot(&w) / det;
        // Around `q`: the ray starts 1 mm before it.
        if (t - 1.0).abs() < 0.5 && best.is_none_or(|x| (t - 1.0).abs() < (x.0 - 1.0).abs()) {
            best = Some((t, *f));
        }
    }
    best.map(|x| x.1)
}

/// The solid holding the face at `q` and the kernel face: `(solid, face, a plane square to d)`.
fn face_at(
    body: &BRep,
    tris: &[(FaceId, [Point3; 3])],
    cur: &[ExactSolid],
    q: Point3,
    d: Vector3,
) -> Option<(usize, usize, bool)> {
    let f = body.face(face_under(tris, q, d)?)?;
    let square = matches!(
        body.surface(f.surface),
        Some(Surface::Plane(pl)) if pl.normal.dot(&d).abs() > 1.0 - 1e-9
    );
    let name = f.name.as_deref()?;
    let (s, k) = exact::faces_in_solids(cur, &[(name, q)]).pop().flatten()?;
    Some((s, k, square))
}

/// The sign along `d` that leaves the material at `q`.
fn outward(rays: &Rays, q: Point3, d: Vector3) -> Real {
    let inside = matches!(rays.entry(q + d * 1e-3, d), Some((t, _)) if t == 0.0);
    if inside { -1.0 } else { 1.0 }
}

/// How far past a curved face a tool stands (into the body for an emboss, into the air for an
/// engrave), so its inner surface does not run almost on the face: 0.02 mm took the kernel 7.5 s
/// on a cylinder, 0.5 mm 0.5 s. Never more than half the wall at `on` looking along `inward`, or
/// an emboss on a tube would reach into its hollow.
fn reach_in(rays: &Rays, on: Point3, inward: Vector3) -> Real {
    let start = on + inward * 1e-3;
    let wall = rays
        .entry(start, inward)
        .filter(|x| x.0 == 0.0)
        .map_or(1.0, |x| x.1);
    (wall / 2.0).clamp(1e-3, 0.5)
}

/// The profile of one region, its segments named as an Extrude's.
fn piece_profile(
    e: &ExtrudeFeature,
    body: &BRep,
    works: &super::placement::WorkGeom,
    sketch: &Sketch,
) -> Result<exact::ExactProfile, Fail> {
    let pieces = e.pieces(body, works, sketch).map_err(Fail::User)?;
    let p = pieces
        .first()
        .ok_or_else(|| Fail::User("the profile no longer exists".into()))?;
    exact::profile(&p.outer, &p.holes).map_err(Fail::Kernel)
}

/// The tool of a rectangle wrapped onto a cylinder: arc length along the sketch = arc length on
/// the face, walls radial.
fn wrapped_tool(
    op: &EmbossOp,
    body: &BRep,
    rays: &Rays,
    sketch: &Sketch,
    r: &Region,
    prefix: &str,
) -> Result<ExactSolid, Fail> {
    let user = |m: &str| Fail::User(format!("Wrap to Face: {m}"));
    let wrap = op.wrap.as_ref().ok_or_else(|| user("select the face"))?;
    let f = face_near(body, &wrap.face, Point3::from(wrap.hint))
        .and_then(|id| body.face(id))
        .ok_or_else(|| user("the face no longer exists"))?;
    let Some(Surface::Cylinder(c)) = body.surface(f.surface) else {
        return Err(user("only cylindrical faces so far"));
    };
    let (axis, rad, n) = (
        c.axis.into_inner(),
        c.radius,
        sketch.plane.normal.into_inner(),
    );
    if axis.dot(&n).abs() > 1e-6 {
        return Err(user("the sketch must be parallel to the cylinder's axis"));
    }
    // The side of the cylinder the sketch stands on (a plane through the axis: the direction).
    let dist = (sketch.plane.origin - c.origin).dot(&n);
    let radial = if dist.abs() > 1e-9 {
        n * dist.signum()
    } else if op.side == EmbossSide::Direction2 {
        -n
    } else {
        n
    };
    let tangent = axis.cross(&radial);
    const SHAPE: &str = "only rectangles along the axis and round the cylinder so far";
    let lines = r
        .outer
        .edges
        .iter()
        .all(|e| matches!(e.curve, ProfileCurve::Line { .. }));
    if !r.holes.is_empty() || r.outer.edges.len() != 4 || !lines {
        return Err(user(SHAPE));
    }
    let (mut s0, mut s1, mut z0, mut z1) = (
        Real::INFINITY,
        Real::NEG_INFINITY,
        Real::INFINITY,
        Real::NEG_INFINITY,
    );
    let pts: Vec<Point3> = r
        .outer
        .polygon(1.0)
        .iter()
        .map(|p| sketch.plane.point_at(p.x, p.y))
        .collect();
    for (k, a) in pts.iter().enumerate() {
        let b = pts[(k + 1) % pts.len()];
        let (ds, dz) = ((b - a).dot(&tangent), (b - a).dot(&axis));
        if ds.abs() > 1e-6 && dz.abs() > 1e-6 {
            return Err(user(SHAPE));
        }
        let (s, z) = ((a - c.origin).dot(&tangent), (a - c.origin).dot(&axis));
        (s0, s1, z0, z1) = (s0.min(s), s1.max(s), z0.min(z), z1.max(z));
    }
    let angle = (s1 - s0) / rad;
    if angle >= std::f64::consts::TAU {
        return Err(user(
            "the profile is longer than the way round the cylinder",
        ));
    }
    let mid = (s0 + s1) / 2.0 / rad;
    let centre = radial * mid.cos() + tangent * mid.sin();
    let air = reach_in(
        rays,
        c.origin + radial * rad + axis * ((z0 + z1) / 2.0),
        -radial,
    );
    let radii = if op.style == EmbossKind::EngraveFromFace {
        (rad - op.depth, rad + air)
    } else {
        (rad - air, rad + op.depth)
    };
    exact::emboss::ring_sector(c.origin, axis, centre, radii, (z0, z1), angle, prefix)
        .map_err(Fail::Kernel)
}

/// Builds Emboss / Engrave (`cur`: the solids before it; surface bodies stay outside).
pub(super) fn emboss(
    op: &EmbossOp,
    id: &str,
    body: &BRep,
    works: &super::placement::WorkGeom,
    sketches: &[super::PartSketch],
    cur: &[ExactSolid],
) -> Result<Vec<ExactSolid>, Fail> {
    let ps = sketches
        .iter()
        .find(|s| s.id == op.sketch)
        .ok_or_else(|| Fail::User("the sketch no longer exists".into()))?;
    let sketch = &ps.sketch;
    let regions = sketch.regions();
    let profiles = if op.profiles.is_empty() {
        default_profiles(ps, &regions)
    } else {
        op.profiles.clone()
    };
    if profiles.is_empty() {
        return Err(Fail::User("the sketch has no closed profile".into()));
    }
    let reach = match body.bounding_box() {
        Some((lo, hi)) => (hi - lo).norm() + 1.0,
        None => return Err(Fail::User("there is no body to emboss".into())),
    };
    let rays = Rays::of(body, &[]);
    let tris = skin(body);
    let n = sketch.plane.normal.into_inner();
    let (mut joined, mut cut) = (Vec::new(), Vec::new());
    let (mut join_in, mut cut_in) = (Vec::new(), Vec::new());
    let name_of = |s: usize| cur.get(s).and_then(|x| x.name.clone());
    for (k, pr) in profiles.iter().enumerate() {
        let region = pr
            .resolve(&regions)
            .ok_or_else(|| Fail::User("the profile no longer exists".into()))?;
        let prefix = format!("{id}:{}", k + 1);
        let mut e = ExtrudeFeature::new(vec![pr.clone()], 1.0);
        e.id = prefix.clone();
        let pts = samples(sketch, region);
        if op.style == EmbossKind::FromPlane {
            let prof = piece_profile(&e, body, works, sketch)?;
            let mut any = false;
            for sign in op.directions() {
                let d = n * sign;
                let seed = pts[0];
                let (ahead, behind) = (rays.entry(seed, d), rays.entry(seed, -d));
                let zero = |x: &Option<(Real, Real)>| x.is_some_and(|v| v.0 == 0.0);
                // On the skin: nothing to add or remove on this side.
                if (ahead.is_none() && zero(&behind)) || (zero(&ahead) && behind.is_none()) {
                    return Err(Fail::User(
                        "the sketch lies on a face of the body: nothing to add or remove \
                         (use Emboss or Engrave from Face)"
                            .into(),
                    ));
                }
                let Some((entry, _)) = ahead else {
                    continue;
                };
                any = true;
                if entry == 0.0 {
                    let far = pts
                        .iter()
                        .filter_map(|p| rays.last_hit(*p, d))
                        .fold(0.0, Real::max);
                    cut.push(
                        exact::extrude(
                            &sketch.plane,
                            &prof,
                            0.0,
                            sign * (far + 1.0),
                            op.taper,
                            &prefix,
                        )
                        .map_err(Fail::Kernel)?,
                    );
                    if let Some(q) = rays.last_hit(seed, d).map(|t| seed + d * t)
                        && let Some((s, _, _)) = face_at(body, &tris, cur, q, d)
                    {
                        cut_in.extend(name_of(s));
                    }
                } else {
                    let deep = pts
                        .iter()
                        .filter_map(|p| rays.entry(*p, d).map(|x| x.0))
                        .fold(entry, Real::max);
                    joined.push(
                        exact::extrude(
                            &sketch.plane,
                            &prof,
                            0.0,
                            sign * (deep + 1e-2),
                            op.taper,
                            &prefix,
                        )
                        .map_err(Fail::Kernel)?,
                    );
                    if let Some((s, _, _)) = face_at(body, &tris, cur, seed + d * entry, d) {
                        join_in.extend(name_of(s));
                    }
                }
            }
            if !any {
                return Err(Fail::User("the direction points away from the body".into()));
            }
            continue;
        }

        // The face types.
        let engrave = op.style == EmbossKind::EngraveFromFace;
        if op.wrap.is_some() {
            let tool = wrapped_tool(op, body, &rays, sketch, region, &prefix)?;
            let owner = op.wrap.as_ref().and_then(|w| {
                exact::faces_in_solids(cur, &[(w.face.as_str(), Point3::from(w.hint))])
                    .pop()
                    .flatten()
                    .and_then(|(s, _)| name_of(s))
            });
            if engrave {
                cut.push(tool);
                cut_in.extend(owner);
            } else {
                joined.push(tool);
                join_in.extend(owner);
            }
            continue;
        }
        let d = n * op.directions()[0];
        let over = || Fail::User("the profile must lie entirely over a face of the body".into());
        let mut targets: Vec<(usize, usize, bool)> = Vec::new();
        let mut levels: Vec<Real> = Vec::new();
        let mut first = None;
        for p in &pts {
            let t = first_skin(&rays, *p, d).ok_or_else(over)?;
            let q = *p + d * t;
            first.get_or_insert(q);
            levels.push((q - sketch.plane.origin).dot(&n));
            let found = face_at(body, &tris, cur, q, d).ok_or_else(over)?;
            if !targets.iter().any(|x| x.0 == found.0 && x.1 == found.1) {
                targets.push(found);
            }
        }
        let (solid, q) = match (targets.first(), first) {
            (Some(t), Some(q)) => (t.0, q),
            _ => return Err(over()),
        };
        if targets.iter().any(|t| t.0 != solid) {
            return Err(Fail::User("the profile spans several bodies".into()));
        }
        let flat =
            targets.iter().all(|t| t.2) && levels.iter().all(|l| (l - levels[0]).abs() < 1e-6);
        let prof = piece_profile(&e, body, works, sketch)?;
        let tool = if flat {
            // A flat face square to the sketch: a plain prism, reaching a hair into the air
            // (engrave) or the body (emboss), so no face of the tool lies on the body's.
            let out = outward(&rays, q, n);
            let air = (op.depth * 0.01).clamp(1e-3, 0.05);
            let (start, end) = if engrave {
                (levels[0] + out * air, levels[0] - out * op.depth)
            } else {
                (levels[0] - out * air, levels[0] + out * op.depth)
            };
            exact::extrude(&sketch.plane, &prof, start, end, 0.0, &prefix).map_err(Fail::Kernel)?
        } else {
            // A curved face: the shell between the face and its offset, cut to the profile.
            let faces: Vec<usize> = targets.iter().map(|t| t.1).collect();
            let air = reach_in(&rays, q, -d * outward(&rays, q, d));
            let (from, thick) = if engrave {
                (air, -(op.depth + air))
            } else {
                (-air, op.depth + air)
            };
            let sheet = exact::offset_sheet(&cur[solid], &faces, from, None, &prefix)
                .map_err(Fail::User)?;
            let slab =
                exact::thicken_sheet(&sheet, thick, false, None, &prefix).map_err(Fail::User)?;
            let prism = exact::extrude(
                &sketch.plane,
                &prof,
                -reach * 2.0,
                reach * 2.0,
                0.0,
                &prefix,
            )
            .map_err(Fail::Kernel)?;
            exact::boolean(&slab, &prism, ExactOp::Intersect)
                .map_err(Fail::Kernel)?
                .ok_or_else(|| Fail::User("the profile misses the face".into()))?
        };
        if engrave {
            cut.push(tool);
            cut_in.extend(name_of(solid));
        } else {
            joined.push(tool);
            join_in.extend(name_of(solid));
        }
    }
    let mut out = cur.to_vec();
    if !joined.is_empty() {
        join_in.dedup();
        out = combine(out, joined, Operation::Join, &join_in, false)?;
    }
    if !cut.is_empty() {
        cut_in.dedup();
        out = combine(out, cut, Operation::Cut, &cut_in, false)?;
    }
    Ok(out)
}
