//! Boss (`docs/spec/5.8-plastic-parts.md`): a screw post from the mating plane to the next face
//! of the body, the head or the thread side, with stiffening ribs. Each centre is a body of
//! revolution (`<id>:<k>:…`) and its ribs are prisms (`<id>:<k>:Rib<n>:…`).

use serde::{Deserialize, Serialize};

use super::common;
use super::{Centers, is_false, is_zero};
use crate::kernel::exact::{self, ExactSolid, PrismSeg};
use crate::kernel::{BRep, Plane, Point3, Real, Vector3};
use crate::part::boolean::Operation;
use crate::part::exact::{Fail, combine};
use crate::part::placement::WorkGeom;
use crate::part::rib::Rays;
use crate::sketch::Point2;

/// The head side's seat.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum HeadStyle {
    /// A flat seat.
    #[default]
    Counterbore,
    /// A cone.
    Countersink,
}

/// The head side of a screw joint (lengths in mm, angles in radians).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BossHead {
    /// Counterbore or Countersink.
    #[serde(default)]
    pub style: HeadStyle,
    /// Wall round the head's recess.
    pub wall: Real,
    /// Plate between the mating plane and the seat.
    pub shank_height: Real,
    /// Seat ring raised from that plate.
    pub clamp_height: Real,
    /// The screw's hole.
    pub shank_diameter: Real,
    /// The seat ring.
    pub clamp_diameter: Real,
    /// The recess.
    pub head_diameter: Real,
    /// Countersink's included angle.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub countersink: Real,
    /// Draft of the recess.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub inner_draft: Real,
    /// Draft of the screw's hole.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub shaft_draft: Real,
    /// Draft of the seat ring.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub clamp_draft: Real,
}

impl Default for BossHead {
    fn default() -> Self {
        Self {
            style: HeadStyle::Counterbore,
            wall: 1.5,
            shank_height: 1.5,
            clamp_height: 0.5,
            shank_diameter: 3.0,
            clamp_diameter: 7.0,
            head_diameter: 8.0,
            countersink: 0.0,
            inner_draft: 0.0,
            shaft_draft: 0.0,
            clamp_draft: 0.0,
        }
    }
}

/// The thread side's hole.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum ThreadHole {
    /// A solid post (an ejector pin).
    None,
    /// The given depth from the mating plane.
    Depth(Real),
    /// To the wall's inner face.
    #[default]
    FullDepth,
    /// Through the wall.
    Through,
}

/// The thread side of a screw joint.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BossThread {
    /// The post.
    pub diameter: Real,
    /// The hole.
    #[serde(default)]
    pub hole: ThreadHole,
    /// Its diameter.
    pub hole_diameter: Real,
    /// Its draft.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub inner_draft: Real,
}

impl Default for BossThread {
    fn default() -> Self {
        Self {
            diameter: 6.0,
            hole: ThreadHole::FullDepth,
            hole_diameter: 2.5,
            inner_draft: 0.0,
        }
    }
}

/// Head or Thread.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum BossKind {
    /// The screw head's side.
    Head(BossHead),
    /// The thread's side.
    Thread(BossThread),
}

/// Stiffening ribs round each boss.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BossRibs {
    /// Ribs per boss.
    pub count: usize,
    /// Thickness at the top.
    pub thickness: Real,
    /// Draft of its sides.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub draft: Real,
    /// Reach from the boss at the top.
    pub shoulder: Real,
    /// The top's distance from the boss's start.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub top_offset: Real,
    /// Round of the shoulder.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub shoulder_radius: Real,
    /// Outer edge's slope from the axis.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub flare: Real,
    /// Round where the rib stands on the body.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rib_radius: Real,
    /// Round where the rib meets the boss.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub blend_radius: Real,
    /// First rib from the sketch's X axis.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub start: Real,
    /// The angle the ribs share.
    pub space: Real,
}

impl Default for BossRibs {
    fn default() -> Self {
        Self {
            count: 4,
            thickness: 1.0,
            draft: 0.0,
            shoulder: 3.0,
            top_offset: 1.0,
            shoulder_radius: 0.0,
            flare: 0.0,
            rib_radius: 0.0,
            blend_radius: 0.0,
            start: 0.0,
            space: std::f64::consts::TAU,
        }
    }
}

/// Boss.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BossOp {
    /// Where.
    pub centers: Centers,
    /// Target bodies; empty = whichever it reaches.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub solids: Vec<String>,
    /// Against the default direction.
    #[serde(default, skip_serializing_if = "is_false")]
    pub flip: bool,
    /// Start from the sketch plane, positive against the boss's direction.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub offset: Real,
    /// Head or Thread.
    #[serde(rename = "style")]
    pub kind: BossKind,
    /// Draft of the outside.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub outer_draft: Real,
    /// Round where it meets the body.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub fillet: Real,
    /// Stiffening ribs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ribs: Option<BossRibs>,
}

impl BossOp {
    /// A thread boss with a full-depth hole on the Center Points of `sketch`.
    pub fn new(sketch: impl Into<String>) -> Self {
        Self {
            centers: Centers::Sketch {
                sketch: sketch.into(),
                points: Vec::new(),
            },
            solids: Vec::new(),
            flip: false,
            offset: 0.0,
            kind: BossKind::Thread(BossThread::default()),
            outer_draft: 0.0,
            fillet: 0.0,
            ribs: None,
        }
    }

    /// The radius of the outside at its start.
    pub fn outer_radius(&self) -> Real {
        match &self.kind {
            BossKind::Head(h) => h.head_diameter / 2.0 + h.wall,
            BossKind::Thread(t) => t.diameter / 2.0,
        }
    }

    /// Refuses a definition that cannot be built (before any geometry).
    pub fn validate(&self) -> Result<(), String> {
        self.centers.validate()?;
        let pos = |v: Real, name: &str| {
            if v > 0.0 {
                Ok(())
            } else {
                Err(format!("{name} must be positive"))
            }
        };
        match &self.kind {
            BossKind::Head(h) => {
                pos(h.wall, "the wall thickness")?;
                pos(h.shank_height, "the shank height")?;
                pos(h.shank_diameter, "the shank diameter")?;
                pos(h.head_diameter, "the head diameter")?;
                if h.clamp_height < 0.0 {
                    return Err("the clamp height cannot be negative".into());
                }
                if !(h.shank_diameter < h.clamp_diameter && h.clamp_diameter <= h.head_diameter) {
                    return Err(
                        "the diameters must grow from the shank over the clamp to the head".into(),
                    );
                }
                if h.style == HeadStyle::Countersink
                    && !(h.countersink > 0.0 && h.countersink < std::f64::consts::PI)
                {
                    return Err("the countersink angle must lie between 0 and 180 degrees".into());
                }
            }
            BossKind::Thread(t) => {
                pos(t.diameter, "the thread diameter")?;
                if t.hole != ThreadHole::None
                    && !(t.hole_diameter > 0.0 && t.hole_diameter < t.diameter)
                {
                    return Err("the hole must be smaller than the boss".into());
                }
                if let ThreadHole::Depth(d) = t.hole {
                    pos(d, "the hole depth")?;
                }
            }
        }
        if self.fillet < 0.0 {
            return Err("the fillet cannot be negative".into());
        }
        if self.outer_draft.abs() >= 45f64.to_radians() {
            return Err("the draft must stay below 45 degrees".into());
        }
        if let Some(r) = &self.ribs {
            if r.count == 0 {
                return Err("there must be at least one rib".into());
            }
            pos(r.thickness, "the rib thickness")?;
            pos(r.shoulder, "the shoulder length")?;
            if r.top_offset < 0.0 {
                return Err("the shoulder's top offset cannot be negative".into());
            }
        }
        Ok(())
    }

    /// The browser's tooltip.
    pub fn describe(&self) -> String {
        match &self.kind {
            BossKind::Head(h) => format!("Head boss Ø{} mm", h.head_diameter + 2.0 * h.wall),
            BossKind::Thread(t) => format!("Thread boss Ø{} mm", t.diameter),
        }
    }

    /// The boss's direction from each point: towards the nearest body along the normal; Flip
    /// turns it round.
    fn direction(&self, rays: &Rays, pts: &[Point3], n: Vector3) -> Result<Vector3, String> {
        let toward = |d: Vector3| {
            pts.iter()
                .filter_map(|p| rays.entry(*p, d).map(|x| x.0))
                .fold(None, |a: Option<Real>, t| Some(a.map_or(t, |x| x.min(t))))
        };
        let d = match (toward(n), toward(-n)) {
            (Some(a), Some(b)) if b < a => -n,
            (None, Some(_)) => -n,
            (None, None) => return Err("no body lies along the boss's direction".into()),
            _ => n,
        };
        Ok(if self.flip { -d } else { d })
    }
}

/// How far a tool goes on into a wall `thick` deep.
fn embed(thick: Real) -> Real {
    (thick / 2.0).clamp(1e-3, 2.0)
}

/// The cut of one boss as a profile (radius, distance along the axis) closed on the axis.
type CutProfile = (Vec<[Real; 2]>, Vec<&'static str>);

fn cut_profile(op: &BossOp, enter: Real, exit: Real) -> Result<Option<CutProfile>, Fail> {
    let user = |m: &str| Fail::User(m.to_owned());
    let air = 1.0;
    let end_air = exit + 1.0;
    Ok(Some(match &op.kind {
        BossKind::Thread(t) => {
            let r = t.hole_diameter / 2.0;
            let far = match t.hole {
                ThreadHole::None => return Ok(None),
                ThreadHole::Depth(v) => v,
                ThreadHole::FullDepth => enter,
                ThreadHole::Through => end_air,
            };
            let tan = t.inner_draft.tan();
            (
                vec![
                    [0.0, -air],
                    [r + (far + air) * tan, -air],
                    [r, far],
                    [0.0, far],
                ],
                vec!["Air", "Hole", "Floor", "Axis"],
            )
        }
        BossKind::Head(h) => {
            let rs = h.shank_diameter / 2.0;
            let (rc, rh) = (h.clamp_diameter / 2.0, h.head_diameter / 2.0);
            let (s, top) = (h.shank_height, h.shank_height + h.clamp_height);
            if top >= exit {
                return Err(user("the shank and clamp are taller than the boss"));
            }
            let rh_far = rh + (end_air - s) * h.inner_draft.tan();
            let rs_air = rs + (top + air) * h.shaft_draft.tan();
            match h.style {
                HeadStyle::Counterbore if h.clamp_height > 0.0 => {
                    let rc_top = rc - h.clamp_height * h.clamp_draft.tan();
                    if rc_top <= rs {
                        return Err(user("the clamp's draft reaches the shank's hole"));
                    }
                    (
                        vec![
                            [0.0, -air],
                            [rs_air, -air],
                            [rs, top],
                            [rc_top, top],
                            [rc, s],
                            [rh, s],
                            [rh_far, end_air],
                            [0.0, end_air],
                        ],
                        vec![
                            "Air",
                            "Shank",
                            "Clamp",
                            "ClampSide",
                            "Seat",
                            "Recess",
                            "Out",
                            "Axis",
                        ],
                    )
                }
                HeadStyle::Counterbore => (
                    vec![
                        [0.0, -air],
                        [rs_air, -air],
                        [rs, s],
                        [rh, s],
                        [rh_far, end_air],
                        [0.0, end_air],
                    ],
                    vec!["Air", "Shank", "Seat", "Recess", "Out", "Axis"],
                ),
                HeadStyle::Countersink => {
                    let rise = (rh - rs) / (h.countersink / 2.0).tan();
                    (
                        vec![
                            [0.0, -air],
                            [rs_air, -air],
                            [rs, s],
                            [rh, s + rise],
                            [rh_far, end_air],
                            [0.0, end_air],
                        ],
                        vec!["Air", "Shank", "Cone", "Recess", "Out", "Axis"],
                    )
                }
            }
        }
    }))
}

/// What one boss adds: its post, its ribs and what is cut out.
struct Tools {
    post: ExactSolid,
    ribs: Vec<ExactSolid>,
    cut: Option<ExactSolid>,
}

/// One boss's tools.
fn one(
    op: &BossOp,
    rays: &Rays,
    o: Point3,
    d: Vector3,
    x: Vector3,
    prefix: &str,
) -> Result<Tools, Fail> {
    let user = |m: &str| Fail::User(m.to_owned());
    let y = d.cross(&x);
    let r0 = op.outer_radius();
    let tan_o = op.outer_draft.tan();
    // The wall under the axis, and how deep it lies round the outside.
    let (enter, exit) = rays
        .entry(o, d)
        .ok_or_else(|| user("a boss reaches no face of the body"))?;
    if enter <= 0.0 {
        return Err(user("a boss starts inside the material"));
    }
    let mut deep = enter;
    for k in 0..16 {
        let a = k as Real * std::f64::consts::TAU / 16.0;
        let q = o + (x * a.cos() + y * a.sin()) * (r0 + enter * tan_o);
        if let Some((e, _)) = rays.entry(q, d) {
            deep = deep.max(e);
        }
    }
    let z1 = deep + embed(exit - enter);
    let r1 = r0 + z1 * tan_o;
    let seam = 0.31;
    let post = exact::revolved_tool(
        o,
        d,
        seam,
        &[[0.0, 0.0], [r0, 0.0], [r1, z1], [0.0, z1]],
        &["Start", "Outer", "Foot", "Axis"],
        prefix,
    )
    .map_err(Fail::Kernel)?;
    let mut ribs = Vec::new();
    // Ribs: from inside the boss to its reach, widening to the wall; the hole is cut afterwards.
    if let Some(r) = &op.ribs {
        let step = r.space / r.count as Real;
        let z0 = r.top_offset;
        if z0 >= enter {
            return Err(user("the shoulder's top offset is below the wall"));
        }
        let ri = r0 * 0.5;
        for k in 0..r.count {
            let a = r.start + step * k as Real;
            let u = x * a.cos() + y * a.sin();
            let plane = Plane::new(o + d * z0, d, u).map_err(|e| Fail::Kernel(e.to_string()))?;
            let far = Plane {
                origin: o + d * z1,
                normal: plane.normal,
                x_axis: plane.x_axis,
            };
            let rect = |z: Real| {
                let h = r.thickness / 2.0 + (z - z0) * r.draft.tan();
                let out = r0 + z * tan_o + r.shoulder + (z - z0) * r.flare.tan();
                let p = Point2::new;
                vec![
                    PrismSeg::Line(p(ri, -h), p(out, -h)),
                    PrismSeg::Line(p(out, -h), p(out, h)),
                    PrismSeg::Line(p(out, h), p(ri, h)),
                    PrismSeg::Line(p(ri, h), p(ri, -h)),
                ]
            };
            ribs.push(
                exact::prism_between(
                    &[(rect(z0), rect(z1))],
                    &plane,
                    &far,
                    &format!("{prefix}:Rib{}", k + 1),
                )
                .map_err(Fail::Kernel)?,
            );
        }
    }
    let cut = match cut_profile(op, enter, exit)? {
        Some((pts, names)) => Some(
            exact::revolved_tool(o, d, seam, &pts, &names, &format!("{prefix}:Cut"))
                .map_err(Fail::Kernel)?,
        ),
        None => None,
    };
    Ok(Tools { post, ribs, cut })
}

/// Builds Boss (`cur`: the solids before it; surface bodies stay outside).
pub(in crate::part) fn boss(
    op: &BossOp,
    id: &str,
    body: &BRep,
    works: &WorkGeom,
    sketches: &[crate::part::PartSketch],
    cur: &[ExactSolid],
) -> Result<Vec<ExactSolid>, Fail> {
    let (pts, n, x) = op
        .centers
        .resolve(body, works, sketches)
        .map_err(Fail::User)?;
    let rays = Rays::of(body, &op.solids);
    let d = op.direction(&rays, &pts, n).map_err(Fail::User)?;
    let (mut posts, mut ribs, mut cut) = (Vec::new(), Vec::new(), Vec::new());
    for (k, p) in pts.iter().enumerate() {
        let start = *p - d * op.offset;
        let t = one(op, &rays, start, d, x, &format!("{id}:{}", k + 1))?;
        posts.push(t.post);
        ribs.push(t.ribs);
        cut.extend(t.cut);
    }
    let own = format!("{id}:");
    let mut out = combine(cur.to_vec(), posts, Operation::Join, &op.solids, false)?;
    // The kernel cannot carry a round through the corners where ribs meet the post and the body,
    // so the post's foot is rounded while it stands alone and the ribs come after it.
    if op.fillet > 0.0 {
        out = common::fillet_where(out, id, |_, _, a, b| {
            let (x, y) = (a.starts_with(&own), b.starts_with(&own));
            (x != y).then_some(op.fillet)
        })?;
    }
    let ribs: Vec<ExactSolid> = ribs.into_iter().flatten().collect();
    if !ribs.is_empty() {
        out = combine(out, ribs, Operation::Join, &op.solids, false)?;
    }
    if !cut.is_empty() {
        out = combine(out, cut, Operation::Cut, &op.solids, false)?;
    }
    rib_fillets(op, id, out)
}

/// The ribs' rounds. Each set goes on alone: a set the kernel refuses says which it was.
fn rib_fillets(op: &BossOp, id: &str, mut out: Vec<ExactSolid>) -> Result<Vec<ExactSolid>, Fail> {
    let Some(r) = &op.ribs else {
        return Ok(out);
    };
    let own = format!("{id}:");
    let rib = |s: &str| s.starts_with(&own) && s.contains(":Rib");
    let side = |s: &str| rib(s) && s.contains(":Side:");
    let shoulder = |a: &str, b: &str| {
        let (top, outer) = (|s: &str| rib(s) && s.ends_with(":Start"), |s: &str| {
            rib(s) && s.ends_with(":Side:1")
        });
        (top(a) && outer(b)) || (top(b) && outer(a))
    };
    let blend = |a: &str, b: &str| {
        let on_post = |s: &str| s.starts_with(&own) && !s.contains(":Rib");
        (side(a) && on_post(b)) || (side(b) && on_post(a))
    };
    let foot = |a: &str, b: &str| (rib(a) && !b.starts_with(&own)) || (rib(b) && !a.starts_with(&own));
    let sets: [(Real, &str, &dyn Fn(&str, &str) -> bool); 3] = [
        (r.shoulder_radius, "Shoulder Radius", &shoulder),
        (r.blend_radius, "Blend Radius", &blend),
        (r.rib_radius, "Rib Radius", &foot),
    ];
    for (radius, name, takes) in sets {
        if radius <= 0.0 {
            continue;
        }
        out = common::fillet_where(out, id, |_, _, a, b| takes(a, b).then_some(radius)).map_err(
            |e| match e {
                Fail::User(m) => Fail::User(format!("{name}: {m}")),
                other => other,
            },
        )?;
    }
    Ok(out)
}
