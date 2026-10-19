//! The **Coil** feature (Inventor: 3D Model → Create → Coil), see
//! `docs/spec/5.3-coil.md`.
//!
//! Closed sketch regions are screwed about an axis: springs, helical
//! grooves, threads made by hand (Coil + Cut). The pitch can run down to
//! zero at the ends (Close Start / Close End with a transition and a flat
//! angle) and the radius can grow with the height (Taper).
//!
//! The exact kernel builds the coil (`kernel::exact::coil`); Booleans with a
//! body go through a faceted zone around the coil (`exact::coil_boolean`,
//! like modeled threads). Surface output and the faceted kernel build it
//! from polygons ([`CoilFeature::build_faceted`]).

use std::f64::consts::TAU;

use serde::{Deserialize, Serialize};

use crate::kernel::csg::CsgPolygon;
use crate::kernel::FaceTag;
use crate::kernel::{BRep, Point3, Real, Vector3};
use crate::sketch::{EntityId, Point2, Sketch};

use super::expr::Unit;
use super::extrude::{ProfileRef, kernel_profile};
use super::params::FeatureParams;
use super::placement::WorkGeom;
use super::work::LineRef;
use super::{Operation, Output};

/// The axis of a coil.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum CoilAxis {
    /// A line of the profile sketch (usually a centerline).
    Sketch(EntityId),
    /// An origin axis, a work axis or a straight edge.
    Line(LineRef),
}

/// Which two values define the coil (Inventor's Coil Type).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CoilMethod {
    /// Pitch and number of revolutions.
    #[default]
    PitchAndRevolution,
    /// Number of revolutions and height; the pitch follows.
    RevolutionAndHeight,
    /// Pitch and height; the revolutions follow.
    PitchAndHeight,
}

impl CoilMethod {
    /// Every method, in Inventor's order.
    pub const ALL: [CoilMethod; 3] = [
        CoilMethod::PitchAndRevolution,
        CoilMethod::RevolutionAndHeight,
        CoilMethod::PitchAndHeight,
    ];

    /// Its name as Inventor shows it.
    pub fn label(self) -> &'static str {
        match self {
            CoilMethod::PitchAndRevolution => "Pitch and Revolution",
            CoilMethod::RevolutionAndHeight => "Revolution and Height",
            CoilMethod::PitchAndHeight => "Pitch and Height",
        }
    }
}

/// One end of a coil: natural (cut off square to the helix) or closed (the
/// pitch runs down to zero over `transition`, then `flat` without rise).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CoilEnd {
    /// Close Start / Close End.
    #[serde(default)]
    pub closed: bool,
    /// Transition Angle (radians).
    #[serde(default)]
    pub transition: Real,
    /// Flat Angle (radians).
    #[serde(default)]
    pub flat: Real,
}

impl CoilEnd {
    /// `(transition, flat)` that count (zero when natural).
    fn angles(&self) -> (Real, Real) {
        if self.closed {
            (self.transition.max(0.0), self.flat.max(0.0))
        } else {
            (0.0, 0.0)
        }
    }
}

/// The Coil feature.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CoilFeature {
    /// Immutable identity, the prefix of its topology names.
    pub id: String,
    /// Display name.
    pub name: String,
    /// `id` of the profile sketch.
    pub sketch: String,
    /// The chosen regions (the coil's sections).
    pub profiles: Vec<ProfileRef>,
    /// The axis.
    pub axis: CoilAxis,
    /// Flip: the coil climbs the other way along the axis.
    #[serde(default)]
    pub flip: bool,
    /// Which values define it.
    #[serde(default)]
    pub method: CoilMethod,
    /// Pitch (mm per revolution).
    pub pitch: Real,
    /// Revolutions (including the ends).
    pub revolutions: Real,
    /// Height (mm, from the first section's centre to the last one's).
    pub height: Real,
    /// Taper (radians; positive: the coil widens as it climbs).
    #[serde(default)]
    pub taper: Real,
    /// Rotation Left (clockwise about the axis) instead of Right.
    #[serde(default)]
    pub left_hand: bool,
    /// Close Start.
    #[serde(default)]
    pub start: CoilEnd,
    /// Close End.
    #[serde(default)]
    pub end: CoilEnd,
    /// Boolean operation with the body.
    #[serde(default)]
    pub operation: Operation,
    /// Solid or surface.
    #[serde(default)]
    pub output: Output,
    /// Bodies the operation works on, by body name (empty: the default).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub solids: Vec<String>,
    /// Parameter names and equations of its values.
    #[serde(default, skip_serializing_if = "FeatureParams::is_empty")]
    pub params: FeatureParams,
    /// The preset it was made with (name only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
}

/// The resolved screw motion of one coil.
#[derive(Clone, Debug, PartialEq)]
pub struct CoilGeometry {
    /// A point on the axis.
    pub origin: Point3,
    /// Unit axis (the climb direction).
    pub axis: Vector3,
    /// Total turn angle (radians).
    pub total: Real,
    /// The main pitch (mm per turn).
    pub pitch: Real,
    /// Height from the first section to the last.
    pub height: Real,
    /// Rise per radian of the main stretch.
    rate: Real,
    /// Start flat, start transition, main, end transition, end flat (radians).
    spans: [Real; 5],
    /// tan(taper).
    grow: Real,
    /// Left hand.
    pub left_hand: bool,
}

impl CoilGeometry {
    /// `(height, radial growth)` at turn angle `a`.
    pub fn at(&self, a: Real) -> (Real, Real) {
        let [fs, ts, mid, te, _] = self.spans;
        let c = self.rate;
        let a = a.clamp(0.0, self.total);
        let h = if a <= fs {
            0.0
        } else if a <= fs + ts {
            let b = a - fs;
            c * b * b / (2.0 * ts.max(1e-12))
        } else if a <= fs + ts + mid {
            c * (ts / 2.0 + (a - fs - ts))
        } else if a <= fs + ts + mid + te {
            let b = a - fs - ts - mid;
            c * (ts / 2.0 + mid + b - b * b / (2.0 * te.max(1e-12)))
        } else {
            c * (ts / 2.0 + mid + te / 2.0)
        };
        (h, h * self.grow)
    }

    /// One constant pitch without taper (the kernel's helical sweep).
    pub fn constant_pitch(&self) -> Option<Real> {
        let [fs, ts, _, te, fe] = self.spans;
        (fs + ts + te + fe < 1e-12 && self.grow.abs() < 1e-12).then_some(self.pitch)
    }
}

/// One coil to build: a region's outer loop and the motion.
pub(super) struct CoilPlan {
    pub prefix: String,
    pub outer: Vec<crate::kernel::ProfileSegment>,
    pub holes: usize,
    pub polygon: Vec<Point2>,
    pub geom: CoilGeometry,
    pub radial: Vector3,
}

fn quarter() -> Real {
    TAU / 4.0
}

impl CoilFeature {
    /// A coil of `profiles` about `axis` with Inventor's defaults (Pitch and
    /// Revolution, 10 mm pitch, 2 revolutions, right hand, natural ends).
    pub fn new(profiles: Vec<ProfileRef>, axis: CoilAxis) -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            sketch: String::new(),
            profiles,
            axis,
            flip: false,
            method: CoilMethod::PitchAndRevolution,
            pitch: 10.0,
            revolutions: 2.0,
            height: 20.0,
            taper: 0.0,
            left_hand: false,
            start: CoilEnd {
                closed: false,
                transition: quarter(),
                flat: quarter(),
            },
            end: CoilEnd {
                closed: false,
                transition: quarter(),
                flat: quarter(),
            },
            operation: Operation::NewSolid,
            output: Output::Solid,
            solids: Vec::new(),
            params: FeatureParams::default(),
            preset: None,
        }
    }

    /// Its numeric fields that are parameters (those the method shows).
    pub fn fields(&self) -> Vec<(&'static str, Unit)> {
        let mut v = match self.method {
            CoilMethod::PitchAndRevolution => {
                vec![("pitch", Unit::Mm), ("revolutions", Unit::Ul)]
            }
            CoilMethod::RevolutionAndHeight => {
                vec![("revolutions", Unit::Ul), ("height", Unit::Mm)]
            }
            CoilMethod::PitchAndHeight => vec![("pitch", Unit::Mm), ("height", Unit::Mm)],
        };
        v.push(("taper", Unit::Deg));
        if self.start.closed {
            v.extend([("start_transition", Unit::Deg), ("start_flat", Unit::Deg)]);
        }
        if self.end.closed {
            v.extend([("end_transition", Unit::Deg), ("end_flat", Unit::Deg)]);
        }
        v
    }

    /// The value of field `f`.
    pub fn value(&self, f: &str) -> Option<Real> {
        Some(match f {
            "pitch" => self.pitch,
            "revolutions" => self.revolutions,
            "height" => self.height,
            "taper" => self.taper,
            "start_transition" => self.start.transition,
            "start_flat" => self.start.flat,
            "end_transition" => self.end.transition,
            "end_flat" => self.end.flat,
            _ => return None,
        })
    }

    /// Sets field `f`.
    pub fn set(&mut self, f: &str, v: Real) {
        match f {
            "pitch" => self.pitch = v,
            "revolutions" => self.revolutions = v,
            "height" => self.height = v,
            "taper" => self.taper = v,
            "start_transition" => self.start.transition = v,
            "start_flat" => self.start.flat = v,
            "end_transition" => self.end.transition = v,
            "end_flat" => self.end.flat = v,
            _ => {}
        }
    }

    /// The axis in model space: a point on it and the unit climb direction
    /// (after Flip).
    pub fn world_axis(
        &self,
        sketch: &Sketch,
        body: &BRep,
        works: &WorkGeom,
    ) -> Result<(Point3, Vector3), String> {
        let (o, d) = match &self.axis {
            CoilAxis::Sketch(id) => {
                let Some(crate::sketch::Entity::Line { start, end }) = sketch.entity(*id) else {
                    return Err("the coil axis line no longer exists".into());
                };
                let (Some(a), Some(b)) = (sketch.point(*start), sketch.point(*end)) else {
                    return Err("the coil axis line no longer exists".into());
                };
                let pl = &sketch.plane;
                let (a, b) = (
                    pl.point_at(a.position.x, a.position.y),
                    pl.point_at(b.position.x, b.position.y),
                );
                (a, b - a)
            }
            CoilAxis::Line(l) => l.resolve(body, works)?,
        };
        let d = d
            .try_normalize(1e-12)
            .ok_or_else(|| "the coil axis has no direction".to_owned())?;
        Ok((o, if self.flip { -d } else { d }))
    }

    /// Main pitch, total turn angle and height from the method and the ends.
    pub fn sizes(&self) -> Result<(Real, Real, Real), String> {
        let (ts, fs) = self.start.angles();
        let (te, fe) = self.end.angles();
        for (v, what) in [
            (ts, "the start transition angle"),
            (fs, "the start flat angle"),
            (te, "the end transition angle"),
            (fe, "the end flat angle"),
        ] {
            if !(v.is_finite() && v <= TAU + 1e-9) {
                return Err(format!("{what} must be between 0 and 360 deg"));
            }
        }
        let ends = fs + ts + te + fe;
        // Height per unit pitch of a coil turning `a` in all: the
        // transitions rise half as much, the flats not at all.
        let rise = |a: Real| (a - fs - fe - (ts + te) / 2.0) / TAU;
        let positive = |v: Real, what: &str| {
            if v.is_finite() && v > 1e-9 {
                Ok(v)
            } else {
                Err(format!("{what} must be greater than 0"))
            }
        };
        let (pitch, total) = match self.method {
            CoilMethod::PitchAndRevolution => (
                positive(self.pitch, "the pitch")?,
                TAU * positive(self.revolutions, "the revolutions")?,
            ),
            CoilMethod::RevolutionAndHeight => {
                let total = TAU * positive(self.revolutions, "the revolutions")?;
                let h = positive(self.height, "the height")?;
                let r = rise(total);
                if r <= 1e-9 {
                    return Err("the closed ends take all the revolutions".into());
                }
                (h / r, total)
            }
            CoilMethod::PitchAndHeight => {
                let p = positive(self.pitch, "the pitch")?;
                let h = positive(self.height, "the height")?;
                (p, TAU * h / p + fs + fe + (ts + te) / 2.0)
            }
        };
        if total < ends - 1e-9 {
            return Err("the closed ends take more than all the revolutions".into());
        }
        if total / TAU > 200.0 + 1e-9 {
            return Err("a coil is limited to 200 revolutions".into());
        }
        Ok((pitch, total, pitch * rise(total)))
    }

    /// The coils to build, one per region (checked: axis off the profile,
    /// turns that clear each other).
    pub(super) fn plans(
        &self,
        sketch: &Sketch,
        body: &BRep,
        works: &WorkGeom,
    ) -> Result<Vec<CoilPlan>, String> {
        if self.profiles.is_empty() {
            return Err("select a profile".into());
        }
        let (o, k) = self.world_axis(sketch, body, works)?;
        let (pitch, total, height) = self.sizes()?;
        let (ts, fs) = self.start.angles();
        let (te, fe) = self.end.angles();
        if !self.taper.is_finite() || self.taper.abs() >= std::f64::consts::FRAC_PI_2 - 1e-6 {
            return Err("the taper must be between -90 and 90 deg".into());
        }
        let geom = CoilGeometry {
            origin: o,
            axis: k,
            total,
            pitch,
            height,
            rate: pitch / TAU,
            spans: [fs, ts, (total - fs - ts - te - fe).max(0.0), te, fe],
            grow: self.taper.tan(),
            left_hand: self.left_hand,
        };
        let regions = sketch.regions();
        let pl = &sketch.plane;
        let n = pl.normal.into_inner();
        let mut out = Vec::new();
        for (j, pr) in self.profiles.iter().enumerate() {
            let region = pr
                .resolve(&regions)
                .ok_or_else(|| format!("profile #{} no longer exists in the sketch", j + 1))?;
            let polygon = region.outer.polygon(0.002);
            let world: Vec<Point3> = polygon.iter().map(|q| pl.point_at(q.x, q.y)).collect();
            // The axis must not cross the profile.
            if k.dot(&n).abs() < 1e-9 {
                // In (or parallel to) the sketch plane: every point on one side.
                let side = |p: &Point3| (p - o).cross(&k).dot(&n);
                let s: Vec<Real> = world.iter().map(side).collect();
                let pos = s.iter().all(|&x| x > 1e-9);
                let neg = s.iter().all(|&x| x < -1e-9);
                if !(pos || neg) {
                    return Err("the axis must not intersect the profile".into());
                }
            } else {
                let t = (pl.origin - o).dot(&n) / k.dot(&n);
                let hit = pl.project(&(o + k * t));
                if region.contains(hit) {
                    return Err("the axis must not intersect the profile".into());
                }
            }
            let c = world.iter().fold(Vector3::zeros(), |acc, p| acc + p.coords)
                / world.len().max(1) as Real;
            let rel = c - o.coords;
            let radial = (rel - k * rel.dot(&k))
                .try_normalize(1e-12)
                .ok_or_else(|| "the profile is centred on the axis".to_owned())?;
            // The taper must not bring the profile across the axis.
            let r_min = world
                .iter()
                .map(|p| {
                    let d = p - o;
                    (d - k * d.dot(&k)).norm()
                })
                .fold(Real::INFINITY, Real::min);
            if r_min + height * geom.grow <= 1e-6 {
                return Err("the taper brings the coil to its axis".into());
            }
            // Turns one revolution apart must clear each other.
            let along: Vec<Real> = world.iter().map(|p| (p - o).dot(&k)).collect();
            let extent = along.iter().copied().fold(Real::NEG_INFINITY, Real::max)
                - along.iter().copied().fold(Real::INFINITY, Real::min);
            if total > TAU + 1e-9 {
                let steps = 720;
                let gap = (0..=steps)
                    .map(|i| {
                        let a = (total - TAU) * i as Real / steps as Real;
                        geom.at(a + TAU).0 - geom.at(a).0
                    })
                    .fold(Real::INFINITY, Real::min);
                if gap <= extent + 1e-6 {
                    return Err(format!(
                        "the coil runs into itself: the rise per revolution ({gap:.3} mm) must exceed the section ({extent:.3} mm)"
                    ));
                }
            }
            let (outer, holes) = kernel_profile(region);
            let prefix = if j == 0 {
                self.id.clone()
            } else {
                format!("{}#{}", self.id, j + 1)
            };
            out.push(CoilPlan {
                prefix,
                outer,
                holes: holes.len(),
                polygon,
                geom: geom.clone(),
                radial,
            });
        }
        Ok(out)
    }

    /// Builds the coil with the faceted kernel (Surface output, or a part
    /// the exact kernel gave up on): polygons between stations 10° apart.
    pub fn build_faceted(
        &self,
        body: &mut BRep,
        sketch: &Sketch,
        works: &WorkGeom,
    ) -> Result<(), String> {
        let plans = self.plans(sketch, &body.clone(), works)?;
        let surface = self.output == Output::Surface;
        let pl = &sketch.plane;
        for plan in plans {
            let g = &plan.geom;
            let motion = |a: Real, p: Point3| {
                let (h, gr) = g.at(a);
                let s = if g.left_hand { -1.0 } else { 1.0 };
                let rot = nalgebra::Rotation3::from_axis_angle(
                    &nalgebra::Unit::new_normalize(g.axis),
                    s * a,
                );
                g.origin + rot * (p - g.origin + plan.radial * gr) + g.axis * h
            };
            let steps = ((g.total / TAU * 36.0).ceil() as usize).max(2);
            let mut tags = vec![FaceTag {
                name: Some(format!("{}:Side", plan.prefix)),
                surface: None,
                curved: true,
            }];
            let mut polys = Vec::new();
            let mut poly = plan.polygon.clone();
            let area: Real = (0..poly.len())
                .map(|i| {
                    let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
                    p.x * q.y - q.x * p.y
                })
                .sum();
            if area < 0.0 {
                poly.reverse();
            }
            let m = poly.len();
            let ring = |a: Real| -> Vec<Point3> {
                poly.iter().map(|q| motion(a, pl.point_at(q.x, q.y))).collect()
            };
            let mut prev = ring(0.0);
            let first = prev.clone();
            for i in 1..=steps {
                let next = ring(g.total * i as Real / steps as Real);
                for j in 0..m {
                    let (a, b, c, d) = (prev[j], next[j], next[(j + 1) % m], prev[(j + 1) % m]);
                    polys.extend(CsgPolygon::new(vec![a, b, c], 0));
                    polys.extend(CsgPolygon::new(vec![a, c, d], 0));
                }
                prev = next;
            }
            if !surface {
                let (_, tris) = crate::kernel::triangulate_with_holes(&poly, &[]);
                for (name, pts, rev) in [("Start", &first, false), ("End", &prev, true)] {
                    let tag = tags.len();
                    tags.push(FaceTag {
                        name: Some(format!("{}:{name}", plan.prefix)),
                        surface: None,
                        curved: false,
                    });
                    for t in &tris {
                        let v = if rev {
                            vec![pts[t[0]], pts[t[2]], pts[t[1]]]
                        } else {
                            vec![pts[t[0]], pts[t[1]], pts[t[2]]]
                        };
                        polys.extend(CsgPolygon::new(v, tag));
                    }
                }
                if crate::kernel::csg::volume(&polys) < 0.0 {
                    polys = polys
                        .into_iter()
                        .filter_map(|p| {
                            let tag = p.tag;
                            CsgPolygon::new(p.pts.into_iter().rev().collect(), tag)
                        })
                        .collect();
                }
            }
            let ids = body
                .add_polygons(&polys, &tags, surface)
                .map_err(|e| e.to_string())?;
            for id in ids {
                body.name_solid(id, &self.id);
            }
        }
        Ok(())
    }
}

/// The axis Coil picks by itself: the sketch's only centerline.
pub fn auto_axis(sketch: &Sketch) -> Option<CoilAxis> {
    match super::revolve::auto_axis(sketch)? {
        super::AxisRef::SketchLine(id) => Some(CoilAxis::Sketch(id)),
        super::AxisRef::Origin(k) => Some(CoilAxis::Line(LineRef::Origin(k))),
    }
}

/// The annular zone around a coil (radii and heights about its axis,
/// padded), from its stations.
fn zone_of(plan: &CoilPlan, plane: &crate::kernel::Plane) -> [Real; 4] {
    let g = &plan.geom;
    let steps = ((g.total / TAU * 72.0).ceil() as usize).max(8);
    let s = if g.left_hand { -1.0 } else { 1.0 };
    let (mut r0, mut r1) = (Real::INFINITY, 0.0);
    let (mut z0, mut z1) = (Real::INFINITY, Real::NEG_INFINITY);
    for i in 0..=steps {
        let a = g.total * i as Real / steps as Real;
        let (h, gr) = g.at(a);
        let rot =
            nalgebra::Rotation3::from_axis_angle(&nalgebra::Unit::new_normalize(g.axis), s * a);
        for q in &plan.polygon {
            let p = plane.point_at(q.x, q.y);
            let w = g.origin + rot * (p - g.origin + plan.radial * gr) + g.axis * h;
            let d = w - g.origin;
            let z = d.dot(&g.axis);
            let r = (d - g.axis * z).norm();
            r0 = Real::min(r0, r);
            r1 = Real::max(r1, r);
            z0 = Real::min(z0, z);
            z1 = Real::max(z1, z);
        }
    }
    // Arcs bulge past their chords by less than the sampling tolerance.
    let pad = 0.05 + 1e-3 * (r1 - r0 + z1 - z0);
    [(r0 - pad).max(0.0), r1 + pad, z0 - pad, z1 + pad]
}

/// New solids and the coil tools of an exact build.
pub(super) type CoilBuilt = (
    Vec<crate::kernel::exact::ExactSolid>,
    Vec<crate::kernel::exact::ExactSolid>,
);

impl CoilFeature {
    /// The coil through the exact kernel, combined with the solids so far
    /// (`cur`): `Ok(None)` for Surface output (built faceted). Returns the
    /// new solids and the coil tools (for patterns).
    pub(super) fn build_exact(
        &self,
        sketch: &Sketch,
        body: &BRep,
        works: &WorkGeom,
        cur: &[crate::kernel::exact::ExactSolid],
    ) -> Result<Option<CoilBuilt>, super::exact::Fail> {
        use super::exact::Fail::{Kernel, User};
        use crate::kernel::exact::{self as ex, CoilMotion, CoilZone, ExactOp};
        if self.output == Output::Surface {
            return Ok(None);
        }
        let plans = self.plans(sketch, body, works).map_err(User)?;
        let plane = &sketch.plane;
        let chosen = |s: &ex::ExactSolid| {
            self.solids.is_empty() || s.name.as_ref().is_some_and(|n| self.solids.contains(n))
        };
        let mut out = cur.to_vec();
        let mut tools = Vec::new();
        for plan in &plans {
            if plan.holes > 0 {
                return Err(User(
                    "a coil profile with holes is not supported yet".into(),
                ));
            }
            let prof = ex::profile(&plan.outer, &[]).map_err(Kernel)?;
            let g = &plan.geom;
            let at = |a: Real| g.at(a);
            let m = CoilMotion {
                origin: g.origin,
                axis: g.axis,
                left_hand: g.left_hand,
                radial: plan.radial,
                at: &at,
            };
            let tool = ex::coil(
                plane,
                &prof,
                &plan.polygon,
                &m,
                g.total,
                g.constant_pitch(),
                &plan.prefix,
            )
            .map_err(|e| User(format!("the geometry kernel cannot build the coil ({e})")))?;
            tools.push(tool.clone());
            if self.operation == Operation::NewSolid {
                out.push(tool);
                continue;
            }
            let targets: Vec<usize> = (0..out.len()).filter(|&i| chosen(&out[i])).collect();
            if targets.is_empty() {
                return Err(User(format!(
                    "{}: there is no solid body to operate on",
                    self.operation.label()
                )));
            }
            let mesh = ex::coil_mesh(plane, &plan.polygon, &m, 0.0, g.total, 36).map_err(Kernel)?;
            let zone = CoilZone {
                origin: g.origin,
                axis: g.axis,
                zone: zone_of(plan, plane),
            };
            let name = format!("{}:Side", plan.prefix);
            let op = match self.operation {
                Operation::Join => ExactOp::Union,
                Operation::Cut => ExactOp::Cut,
                _ => ExactOp::Intersect,
            };
            let mut next: Vec<ex::ExactSolid> = Vec::new();
            let mut hit = false;
            for (i, s) in out.iter().enumerate() {
                // A Join goes into the first chosen body only.
                let wanted = targets.contains(&i) && !(op == ExactOp::Union && hit);
                if !wanted {
                    next.push(s.clone());
                    continue;
                }
                match ex::coil_boolean(s, &mesh, &zone, op, &name).map_err(Kernel)? {
                    Some(pieces) => {
                        hit = true;
                        next.extend(pieces);
                    }
                    None if op == ExactOp::Union => {
                        // Apart from the body or sitting on a face: exact.
                        let r = ex::boolean(s, &tool, ExactOp::Union)
                            .map_err(|e| Kernel(format!("Join failed: {e}")))?
                            .ok_or_else(|| Kernel("Join failed: the result is empty".into()))?;
                        hit = true;
                        next.push(r);
                    }
                    None if op == ExactOp::Cut => next.push(s.clone()),
                    None => {}
                }
            }
            if !hit {
                return Err(User(format!(
                    "{}: the coil does not intersect the body",
                    self.operation.label()
                )));
            }
            out = next;
        }
        Ok(Some((out, tools)))
    }
}

impl super::PartDocument {
    /// **Coil** of regions of sketch `index` (id `Coil<N>`; name and sketch
    /// are assigned here). The sketch is consumed (hidden) or shared when
    /// another feature uses it. Returns the id; on failure nothing changes.
    pub fn add_coil(
        &mut self,
        index: usize,
        mut def: CoilFeature,
    ) -> Result<String, super::PartError> {
        let sketch_id = self
            .sketches
            .get(index)
            .ok_or(super::PartError::NoSketch(index))?
            .id
            .clone();
        if def.profiles.is_empty() {
            return Err(super::PartError::Feature("select at least one profile".into()));
        }
        let id = self.next_feature_id("Coil");
        (def.id, def.name, def.sketch) = (id.clone(), id.clone(), sketch_id.clone());
        self.add_consuming(super::Feature::Coil(def), &[sketch_id])?;
        Ok(id)
    }
}
