//! Snap Fit (`docs/spec/5.8-plastic-parts.md`): a cantilever hook or a loop on each point. The
//! beam runs along the direction from the point; its root goes on to the next face of the body
//! or stops at the point. Faces: `<id>:<k>:Beam…`, `<id>:<k>:Hook…`.

use serde::{Deserialize, Serialize};

use super::{Centers, is_false};
use crate::kernel::exact::{self, ExactOp, ExactSolid, PrismSeg};
use crate::kernel::{BRep, Plane, Point3, ProfileSegment, Real, Segment2, Vector3};
use crate::part::boolean::Operation;
use crate::part::exact::{Fail, combine};
use crate::part::placement::WorkGeom;
use crate::part::rib::Rays;
use crate::sketch::Point2;

/// The hook's beam and head (lengths in mm, angles in radians).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnapHook {
    /// Beam thickness at the wall.
    pub thickness_wall: Real,
    /// Beam thickness at the hook.
    pub thickness_hook: Real,
    /// Beam width at the wall.
    pub width_wall: Real,
    /// Beam width at the hook.
    pub width_hook: Real,
    /// Beam length from the point to its end.
    pub length: Real,
    /// The straight tip beyond the insertion face.
    pub tip_length: Real,
    /// How far the hook stands out.
    pub undercut: Real,
    /// From the retention face to the end.
    pub hook_length: Real,
    /// Retention face from the beam (90° = square).
    pub retention: Real,
    /// Insertion face from the beam.
    pub insertion: Real,
}

impl Default for SnapHook {
    fn default() -> Self {
        Self {
            thickness_wall: 1.5,
            thickness_hook: 1.0,
            width_wall: 4.0,
            width_hook: 4.0,
            length: 10.0,
            tip_length: 0.5,
            undercut: 1.0,
            hook_length: 4.0,
            retention: 90f64.to_radians(),
            insertion: 30f64.to_radians(),
        }
    }
}

/// The loop (lengths in mm).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnapLoop {
    /// Length from the point to the top.
    pub length: Real,
    /// Width.
    pub width: Real,
    /// Thickness at the wall.
    pub thickness_wall: Real,
    /// Thickness at the top.
    pub thickness_top: Real,
    /// Each side beside the opening.
    pub catch_side: Real,
    /// Above the opening.
    pub catch_top: Real,
    /// The opening's length.
    pub opening: Real,
}

impl Default for SnapLoop {
    fn default() -> Self {
        Self {
            length: 10.0,
            width: 8.0,
            thickness_wall: 1.5,
            thickness_top: 1.0,
            catch_side: 1.5,
            catch_top: 1.5,
            opening: 4.0,
        }
    }
}

/// Hook or Loop.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SnapKind {
    /// A cantilever hook.
    Hook(SnapHook),
    /// A cantilever loop.
    Loop(SnapLoop),
}

/// Snap Fit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnapFitOp {
    /// Where.
    pub centers: Centers,
    /// Target bodies; empty = whichever it reaches.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub solids: Vec<String>,
    /// The beam against the sketch's normal or the direction.
    #[serde(default, skip_serializing_if = "is_false")]
    pub flip: bool,
    /// Which of the four arrows the hook faces (0 = the sketch's X, then a quarter turn each).
    #[serde(default)]
    pub hook_arrow: u8,
    /// Extend to next: the root goes on to the next face; else it stops at the point.
    pub extend: bool,
    /// Hook or Loop.
    #[serde(rename = "style")]
    pub kind: SnapKind,
}

/// Height along the beam a face at `angle` from it gains over `undercut`; a square face none.
fn rise(undercut: Real, angle: Real) -> Real {
    if (angle - std::f64::consts::FRAC_PI_2).abs() < 1e-9 {
        0.0
    } else {
        undercut / angle.tan()
    }
}

/// The straight land of a hook between its two faces.
fn hook_land(h: &SnapHook) -> Real {
    h.hook_length - h.tip_length - rise(h.undercut, h.retention) - rise(h.undercut, h.insertion)
}

impl SnapFitOp {
    /// A hook on the Center Points of `sketch`.
    pub fn new(sketch: impl Into<String>) -> Self {
        Self {
            centers: Centers::Sketch {
                sketch: sketch.into(),
                points: Vec::new(),
            },
            solids: Vec::new(),
            flip: false,
            hook_arrow: 0,
            extend: true,
            kind: SnapKind::Hook(SnapHook::default()),
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
            SnapKind::Hook(h) => {
                pos(h.thickness_wall, "the thickness at the wall")?;
                pos(h.thickness_hook, "the thickness at the hook")?;
                pos(h.width_wall, "the width at the wall")?;
                pos(h.width_hook, "the width at the hook")?;
                pos(h.length, "the beam length")?;
                pos(h.undercut, "the undercut depth")?;
                pos(h.hook_length, "the hook length")?;
                if h.tip_length < 0.0 {
                    return Err("the tip length cannot be negative".into());
                }
                if h.hook_length > h.length {
                    return Err("the hook is longer than the beam".into());
                }
                for (a, name) in [(h.retention, "retention"), (h.insertion, "insertion")] {
                    if !(a > 0.0 && a < std::f64::consts::PI - 1e-3) {
                        return Err(format!(
                            "the {name} angle must lie between 0 and 180 degrees"
                        ));
                    }
                }
                if hook_land(h) < 0.0 {
                    return Err("the hook is too short for its faces and tip".into());
                }
            }
            SnapKind::Loop(l) => {
                pos(l.length, "the clip length")?;
                pos(l.width, "the clip width")?;
                pos(l.thickness_wall, "the thickness at the wall")?;
                pos(l.thickness_top, "the thickness at the top")?;
                pos(l.catch_side, "the catch's side width")?;
                pos(l.catch_top, "the catch's top width")?;
                pos(l.opening, "the opening length")?;
                if 2.0 * l.catch_side >= l.width {
                    return Err("the sides leave no opening".into());
                }
                if l.catch_top + l.opening >= l.length {
                    return Err("the opening does not fit the clip's length".into());
                }
            }
        }
        Ok(())
    }

    /// The browser's tooltip.
    pub fn describe(&self) -> String {
        match &self.kind {
            SnapKind::Hook(h) => format!("Snap fit hook, beam {} mm", h.length),
            SnapKind::Loop(l) => format!("Snap fit loop, {} × {} mm", l.width, l.length),
        }
    }
}

fn seg_rect(x0: Real, x1: Real, y0: Real, y1: Real) -> Vec<PrismSeg> {
    let p = Point2::new;
    vec![
        PrismSeg::Line(p(x0, y0), p(x1, y0)),
        PrismSeg::Line(p(x1, y0), p(x1, y1)),
        PrismSeg::Line(p(x1, y1), p(x0, y1)),
        PrismSeg::Line(p(x0, y1), p(x0, y0)),
    ]
}

/// A closed polygon as a profile (counter-clockwise).
fn polygon_profile(pts: &[Point2]) -> Result<exact::ExactProfile, Fail> {
    let area: Real = (0..pts.len())
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            a.x * b.y - b.x * a.y
        })
        .sum();
    let order: Vec<Point2> = if area >= 0.0 {
        pts.to_vec()
    } else {
        pts.iter().rev().copied().collect()
    };
    let segs: Vec<ProfileSegment> = (0..order.len())
        .map(|i| ProfileSegment {
            geometry: Segment2::Line {
                a: order[i],
                b: order[(i + 1) % order.len()],
            },
            name: format!("S{i}"),
        })
        .collect();
    exact::profile(&segs, &[]).map_err(Fail::Kernel)
}

/// How far behind the point the root starts: into the wall when it extends.
fn root(rays: &Rays, op: &SnapFitOp, p: Point3, b: Vector3) -> Result<Real, Fail> {
    if !op.extend {
        return Ok(0.0);
    }
    let (enter, exit) = rays
        .entry(p, -b)
        .ok_or_else(|| Fail::User("a snap fit reaches no face of the body".into()))?;
    // A point in the material has its root there already.
    Ok(if enter == 0.0 {
        0.0
    } else {
        enter + ((exit - enter) / 2.0).clamp(1e-3, 2.0)
    })
}

fn kern(e: crate::kernel::KernelError) -> Fail {
    Fail::Kernel(e.to_string())
}

fn hook(
    h: &SnapHook,
    p: Point3,
    b: Vector3,
    u: Vector3,
    back: Real,
    prefix: &str,
) -> Result<ExactSolid, Fail> {
    // Section planes square to the beam: x = the hook's way, y across.
    let start = Plane::new(p - b * back, b, u).map_err(kern)?;
    let end = Plane {
        origin: p + b * h.length,
        normal: start.normal,
        x_axis: start.x_axis,
    };
    let beam = exact::prism_between(
        &[(
            seg_rect(
                -h.thickness_wall / 2.0,
                h.thickness_wall / 2.0,
                -h.width_wall / 2.0,
                h.width_wall / 2.0,
            ),
            seg_rect(
                -h.thickness_hook / 2.0,
                h.thickness_hook / 2.0,
                -h.width_hook / 2.0,
                h.width_hook / 2.0,
            ),
        )],
        &start,
        &end,
        &format!("{prefix}:Beam"),
    )
    .map_err(Fail::Kernel)?;
    // The head, in the plane of the hook's way and the beam, from inside the beam.
    let total = h.length + back;
    let half = |z: Real| {
        (h.thickness_wall + (h.thickness_hook - h.thickness_wall) * (z + back) / total) / 2.0
    };
    let z_r = h.length - h.hook_length;
    let x_r = half(z_r);
    let z_land = z_r + rise(h.undercut, h.retention);
    let x_top = x_r + h.undercut;
    let z_tip = h.length - h.tip_length;
    let z_ramp = z_tip - rise(h.undercut, h.insertion);
    let inner = -0.25 * h.thickness_hook;
    let pts = [
        Point2::new(inner, z_r),
        Point2::new(x_r, z_r),
        Point2::new(x_top, z_land),
        Point2::new(x_top, z_ramp),
        Point2::new(half(z_tip), z_tip),
        Point2::new(inner, z_tip),
    ];
    // x = u; the plane's y must be the beam's way: its normal is u × b.
    let side = Plane::new(p, u.cross(&b), u).map_err(kern)?;
    let head = exact::extrude(
        &side,
        &polygon_profile(&pts)?,
        -h.width_hook / 2.0,
        h.width_hook / 2.0,
        0.0,
        &format!("{prefix}:Hook"),
    )
    .map_err(Fail::Kernel)?;
    exact::boolean(&beam, &head, ExactOp::Union)
        .map_err(Fail::Kernel)?
        .ok_or_else(|| Fail::Kernel("the hook misses its beam".into()))
}

fn clip(
    l: &SnapLoop,
    p: Point3,
    b: Vector3,
    u: Vector3,
    back: Real,
    prefix: &str,
) -> Result<ExactSolid, Fail> {
    let start = Plane::new(p - b * back, b, u).map_err(kern)?;
    let end = Plane {
        origin: p + b * l.length,
        normal: start.normal,
        x_axis: start.x_axis,
    };
    let frame = exact::prism_between(
        &[(
            seg_rect(
                -l.thickness_wall / 2.0,
                l.thickness_wall / 2.0,
                -l.width / 2.0,
                l.width / 2.0,
            ),
            seg_rect(
                -l.thickness_top / 2.0,
                l.thickness_top / 2.0,
                -l.width / 2.0,
                l.width / 2.0,
            ),
        )],
        &start,
        &end,
        &format!("{prefix}:Clip"),
    )
    .map_err(Fail::Kernel)?;
    // The opening, through the thickness: x across, y along the beam.
    let (z1, z0) = (l.length - l.catch_top, l.length - l.catch_top - l.opening);
    let half = l.width / 2.0 - l.catch_side;
    let plane = Plane::new(p, b.cross(&u).cross(&b).normalize(), b.cross(&u)).map_err(kern)?;
    let s = plane.y_axis().into_inner().dot(&b).signum();
    let reach = l.thickness_wall.max(l.thickness_top) + 1.0;
    let pts = [
        Point2::new(-half, z0 * s),
        Point2::new(half, z0 * s),
        Point2::new(half, z1 * s),
        Point2::new(-half, z1 * s),
    ];
    let opening = exact::extrude(
        &plane,
        &polygon_profile(&pts)?,
        -reach,
        reach,
        0.0,
        &format!("{prefix}:Opening"),
    )
    .map_err(Fail::Kernel)?;
    exact::boolean(&frame, &opening, ExactOp::Cut)
        .map_err(Fail::Kernel)?
        .ok_or_else(|| Fail::Kernel("the opening leaves nothing of the clip".into()))
}

/// Builds Snap Fit (`cur`: the solids before it; surface bodies stay outside).
pub(in crate::part) fn snap_fit(
    op: &SnapFitOp,
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
    let b = if op.flip { -n } else { n };
    let turn = op.hook_arrow as Real * std::f64::consts::FRAC_PI_2;
    // The arrows count in the sketch, so Flip leaves the hook facing the way it did.
    let u = x * turn.cos() + n.cross(&x) * turn.sin();
    let mut tools = Vec::new();
    for (k, p) in pts.iter().enumerate() {
        let back = root(&rays, op, *p, b)?;
        let prefix = format!("{id}:{}", k + 1);
        tools.push(match &op.kind {
            SnapKind::Hook(h) => hook(h, *p, b, u, back, &prefix)?,
            SnapKind::Loop(l) => clip(l, *p, b, u, back, &prefix)?,
        });
    }
    combine(cur.to_vec(), tools, Operation::Join, &op.solids, false)
}
