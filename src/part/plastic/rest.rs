//! Rest (`docs/spec/5.8-plastic-parts.md`): a flat landing on a slanted or curved wall. The space
//! above the landing is cut through the body; thin walls join the landing to the body's wall on
//! both sides of it (platform walls below, clearance walls above). The walls reach the middle of
//! the wall, taken as a plane fitted under the outline: a wall that bends further than half its
//! thickness from that plane is refused.

use serde::{Deserialize, Serialize};

use super::common::{self, Piece, wall_at};
use super::{is_false, is_zero};
use crate::kernel::exact::{self, ExactSolid};
use crate::kernel::{BRep, Plane, Real};
use crate::part::boolean::Operation;
use crate::part::exact::{Fail, combine};
use crate::part::extrude::{ProfileRef, default_profiles};
use crate::part::placement::{PlaneRef, WorkGeom, plane_of};
use crate::part::rib::Rays;

/// Which side of the profile the thin wall lies on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RestSide {
    /// Inside the profile.
    #[default]
    Inside,
    /// Outside it.
    Outside,
    /// Half each way.
    BothSides,
}

/// How far the platform walls go from the landing.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum RestExtent {
    /// To the wall of the body.
    #[default]
    ThroughAll,
    /// A height (mm).
    Distance(Real),
    /// To a plane parallel to the sketch, with an offset along the rest's direction.
    ToSurface(PlaneRef, Real),
}

/// Where the landing lies.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum RestLanding {
    /// Its top this far from the sketch plane, out of the body.
    Distance(Real),
    /// On a plane parallel to the sketch, with an offset out of the body.
    ToSurface(PlaneRef, Real),
}

impl Default for RestLanding {
    fn default() -> Self {
        Self::Distance(0.0)
    }
}

/// Rest.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RestOp {
    /// The profile's sketch.
    pub sketch: String,
    /// The regions; empty = every region of the sketch.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profiles: Vec<ProfileRef>,
    /// Target bodies; empty = whichever it reaches.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub solids: Vec<String>,
    /// Against the default direction (into the nearest body).
    #[serde(default, skip_serializing_if = "is_false")]
    pub flip: bool,
    /// The thin wall.
    pub thickness: Real,
    /// Its side.
    #[serde(default)]
    pub side: RestSide,
    /// The platform walls.
    #[serde(default)]
    pub extent: RestExtent,
    /// The landing.
    #[serde(default)]
    pub landing: RestLanding,
    /// Draft of the platform walls (radians).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub landing_taper: Real,
    /// Draft of the clearance walls (radians).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub clearance_taper: Real,
}

impl RestOp {
    /// A rest on every region of `sketch`, its landing on the sketch's plane.
    pub fn new(sketch: impl Into<String>, thickness: Real) -> Self {
        Self {
            sketch: sketch.into(),
            profiles: Vec::new(),
            solids: Vec::new(),
            flip: false,
            thickness,
            side: RestSide::default(),
            extent: RestExtent::default(),
            landing: RestLanding::default(),
            landing_taper: 0.0,
            clearance_taper: 0.0,
        }
    }

    /// Refuses a definition that cannot be built (before any geometry).
    pub fn validate(&self) -> Result<(), String> {
        if self.sketch.is_empty() {
            return Err("select the profile".into());
        }
        if self.thickness <= 0.0 {
            return Err("the thickness must be positive".into());
        }
        if matches!(self.extent, RestExtent::Distance(d) if d <= 0.0) {
            return Err("the platform's height must be positive".into());
        }
        for t in [self.landing_taper, self.clearance_taper] {
            if t.abs() >= 45f64.to_radians() {
                return Err("the tapers must stay below 45 degrees".into());
            }
        }
        Ok(())
    }

    /// The browser's tooltip.
    pub fn describe(&self) -> String {
        format!("Rest on {}, walls {} mm", self.sketch, self.thickness)
    }

    /// The outer and inner outline of the thin wall.
    fn outlines(&self, outline: &[Piece]) -> Result<(Vec<Piece>, Vec<Piece>), String> {
        // A counter-clockwise outline has its inside on the left.
        let turn = if common::area(outline) >= 0.0 {
            1.0
        } else {
            -1.0
        };
        let at = |inward: Real| common::offset(outline, true, inward * turn);
        Ok(match self.side {
            RestSide::Inside => (at(0.0)?, at(self.thickness)?),
            RestSide::Outside => (at(-self.thickness)?, at(0.0)?),
            RestSide::BothSides => (at(-self.thickness / 2.0)?, at(self.thickness / 2.0)?),
        })
    }
}

/// A level given by a plane parallel to the sketch.
fn level_of(r: &PlaneRef, plane: &Plane, body: &BRep, works: &WorkGeom) -> Result<Real, Fail> {
    let q = plane_of(r, body, works).map_err(|e| Fail::User(format!("the surface: {e}")))?;
    let n = plane.normal.into_inner();
    if q.normal.dot(&n).abs() < 1.0 - 1e-9 {
        return Err(Fail::User(
            "the surface must be parallel to the sketch".into(),
        ));
    }
    Ok((q.origin - plane.origin).dot(&n))
}

/// The ring between two outlines from level `a0` to `a1` along the direction (`dn` = its sign on
/// the sketch's normal), widened by `tan` per unit away from `a0`.
fn ring(
    plane: &Plane,
    (outer, inner): (&[Piece], &[Piece]),
    (a0, a1): (Real, Real),
    dn: Real,
    tan: Real,
    prefix: &str,
) -> Result<ExactSolid, Fail> {
    let grow = (a1 - a0).abs() * tan;
    let turn = if common::area(outer) >= 0.0 {
        1.0
    } else {
        -1.0
    };
    let out1 = common::offset(outer, true, -grow * turn).map_err(Fail::User)?;
    let in_turn = if common::area(inner) >= 0.0 {
        1.0
    } else {
        -1.0
    };
    let in1 = common::offset(inner, true, -grow * in_turn).map_err(Fail::User)?;
    common::prism(
        plane,
        (a0 * dn, outer),
        (a1 * dn, &out1),
        &[(inner.to_vec(), in1)],
        prefix,
    )
}

/// One region's tools: what is cut out, what is joined.
#[allow(clippy::too_many_arguments)]
fn tools(
    op: &RestOp,
    plane: &Plane,
    outline: &[Piece],
    rays: &Rays,
    d: crate::kernel::Vector3,
    dn: Real,
    landing: Real,
    platform: Option<Real>,
    prefix: &str,
) -> Result<(Vec<ExactSolid>, Vec<ExactSolid>), Fail> {
    let (outer, inner) = op.outlines(outline).map_err(Fail::User)?;
    // The wall under the outline.
    let mut mids = Vec::new();
    let (mut high, mut low): (Real, Real) = (Real::INFINITY, Real::NEG_INFINITY);
    let mut thin = Real::INFINITY;
    let samples = common::polygon(&outer);
    let step = (samples.len() / 64).max(1);
    for q in samples.iter().step_by(step) {
        let p = plane.point_at(q.x, q.y);
        let Some((enter, exit)) = wall_at(rays, p, d) else {
            return Err(Fail::User(
                "the rest's outline must lie entirely over the body".into(),
            ));
        };
        mids.push((q.x, q.y, (enter + exit) / 2.0));
        high = high.min(enter);
        low = low.max(exit);
        thin = thin.min(exit - enter);
    }
    let Some((c0, cu, cv)) = common::fit_heights(&mids) else {
        return Err(Fail::User("the outline has no area".into()));
    };
    let mid_at = |u: Real, v: Real| c0 + cu * u + cv * v;
    if mids
        .iter()
        .any(|(u, v, m)| (mid_at(*u, *v) - m).abs() > thin * 0.45)
    {
        return Err(Fail::User(
            "the wall bends too much under the outline for walls to the next face; \
             use a smaller profile or a Distance"
                .into(),
        ));
    }
    let t = op.thickness;
    let margin = 1.0 + t;
    let top = high.min(landing) - margin;
    let bottom = low.max(landing + t) + margin;
    // The space above the landing and under it, inside the thin wall.
    let cut_up = common::prism(
        plane,
        (landing * dn, &inner),
        (
            top * dn,
            &common::offset(
                &inner,
                true,
                -(landing - top)
                    * op.clearance_taper.tan()
                    * if common::area(&inner) >= 0.0 {
                        1.0
                    } else {
                        -1.0
                    },
            )
            .map_err(Fail::User)?,
        ),
        &[],
        &format!("{prefix}:Clear"),
    )?;
    let cut_down = common::prism(
        plane,
        ((landing + t) * dn, &inner),
        (low.max(landing + t) * dn + margin * dn, &inner),
        &[],
        &format!("{prefix}:Under"),
    )?;
    // The landing.
    let plate = common::prism(
        plane,
        (landing * dn, &outer),
        ((landing + t) * dn, &outer),
        &[],
        &format!("{prefix}:Landing"),
    )?;
    let mut joined = vec![plate];
    // Mid-wall plane: points of it are `plane(u, v) + d * mid(u, v)`.
    let (ux, uy) = (plane.x_axis.into_inner(), plane.y_axis().into_inner());
    let (e1, e2) = (ux + d * cu, uy + d * cv);
    let mid_normal = e1.cross(&e2);
    let mid_normal = if mid_normal.dot(&d) < 0.0 {
        -mid_normal
    } else {
        mid_normal
    };
    let mid_plane = Plane::new(plane.origin + d * c0, mid_normal, ux)
        .map_err(|e| Fail::Kernel(e.to_string()))?;
    // Platform walls: below the landing, down to the wall or to the height asked for.
    let below_to = platform.unwrap_or(bottom);
    if below_to > landing + 1e-9 {
        let down = ring(
            plane,
            (&outer, &inner),
            (landing, below_to),
            dn,
            op.landing_taper.tan(),
            &format!("{prefix}:Platform"),
        )?;
        if platform.is_some() {
            joined.push(down);
        } else {
            // Keep what lies above the wall's middle.
            let (back, _) =
                exact::split_by_plane(&down, &mid_plane, "Mid", "Mid").map_err(Fail::Kernel)?;
            joined.extend(back);
        }
    }
    // Clearance walls: above the landing, up to the wall where it stands over the landing.
    if top < landing - 1e-9 {
        let up = ring(
            plane,
            (&outer, &inner),
            (landing, top),
            dn,
            op.clearance_taper.tan(),
            &format!("{prefix}:Clearance"),
        )?;
        let (_, front) =
            exact::split_by_plane(&up, &mid_plane, "Mid", "Mid").map_err(Fail::Kernel)?;
        joined.extend(front);
    }
    Ok((vec![cut_up, cut_down], joined))
}

/// Builds Rest (`cur`: the solids before it; surface bodies stay outside).
pub(in crate::part) fn rest(
    op: &RestOp,
    id: &str,
    body: &BRep,
    works: &WorkGeom,
    sketches: &[crate::part::PartSketch],
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
    let plane = &sketch.plane;
    let n = plane.normal.into_inner();
    let rays = Rays::of(body, &op.solids);
    // Into the body: the way its nearest wall lies under the first region.
    let seed = profiles
        .first()
        .and_then(|p| p.resolve(&regions))
        .map(|r| r.interior_point())
        .ok_or_else(|| Fail::User("the profile no longer exists".into()))?;
    let at = plane.point_at(seed.x, seed.y);
    let dist = |d| rays.entry(at, d).map(|x: (Real, Real)| x.0);
    let inward = match (dist(n), dist(-n)) {
        (Some(a), Some(b)) => a <= b,
        (Some(_), None) => true,
        (None, Some(_)) => false,
        (None, None) => {
            return Err(Fail::User(
                "the rest's outline must lie entirely over the body".into(),
            ));
        }
    };
    let dn = if inward != op.flip { 1.0 } else { -1.0 };
    let d = n * dn;
    let landing = match &op.landing {
        RestLanding::Distance(v) => -*v,
        RestLanding::ToSurface(r, off) => level_of(r, plane, body, works)? * dn - *off,
    };
    let platform = match &op.extent {
        RestExtent::ThroughAll => None,
        RestExtent::Distance(v) => Some(landing + *v),
        RestExtent::ToSurface(r, off) => Some(level_of(r, plane, body, works)? * dn + *off),
    };
    let (mut cut, mut joined) = (Vec::new(), Vec::new());
    for (k, pr) in profiles.iter().enumerate() {
        let region = pr
            .resolve(&regions)
            .ok_or_else(|| Fail::User("the profile no longer exists".into()))?;
        let outline = common::pieces_of(&region.outer);
        let (c, j) = tools(
            op,
            plane,
            &outline,
            &rays,
            d,
            dn,
            landing,
            platform,
            &format!("{id}:{}", k + 1),
        )?;
        cut.extend(c);
        joined.extend(j);
    }
    let out = combine(cur.to_vec(), cut, Operation::Cut, &op.solids, true)?;
    combine(out, joined, Operation::Join, &op.solids, false)
}
