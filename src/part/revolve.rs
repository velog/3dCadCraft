//! The **Revolve** feature (Inventor: 3D Model → Create → Revolve), see
//! `docs/spec/2.2-revolve.md`.
//!
//! Sketch regions are swept around an axis: a line of the same sketch
//! (usually a centerline) or an origin axis lying in the sketch plane.

use std::f64::consts::TAU;

use serde::{Deserialize, Serialize};

use crate::kernel::{BRep, Real, RevolveAxis, Vector3};
use crate::sketch::{Entity, EntityId, Sketch, Style};

use super::extrude::{ExtentDirection, ProfileRef, kernel_profile};
use super::params::FeatureParams;
use super::{Operation, Output};

/// The axis of a revolve.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AxisRef {
    /// A line of the profile sketch.
    SketchLine(EntityId),
    /// An origin axis (0 = X, 1 = Y, 2 = Z); must lie in the sketch plane.
    Origin(usize),
}

/// How far a revolve goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RevolveExtent {
    /// 360°.
    #[default]
    Full,
    /// By an angle (with direction).
    Angle,
}

/// A revolve of sketch regions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RevolveFeature {
    /// Immutable identity, the prefix of its topology names.
    pub id: String,
    /// Display name.
    pub name: String,
    /// `id` of the profile sketch.
    pub sketch: String,
    /// The chosen regions.
    pub profiles: Vec<ProfileRef>,
    /// The axis.
    pub axis: AxisRef,
    /// Full or by angle.
    #[serde(default)]
    pub extent: RevolveExtent,
    /// Angle A (radians, > 0).
    #[serde(default = "quarter")]
    pub angle: Real,
    /// Angle B for Asymmetric (radians).
    #[serde(default = "quarter")]
    pub angle_b: Real,
    /// Direction of the angle.
    #[serde(default)]
    pub direction: ExtentDirection,
    /// Boolean operation with the body.
    #[serde(default)]
    pub operation: Operation,
    /// Solid or surface.
    #[serde(default)]
    pub output: Output,
    /// Bodies the operation works on (Inventor's **Solids**), by body name;
    /// empty = the default (Join: one body, Cut/Intersect: all).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub solids: Vec<String>,
    /// Parameter names and equations (`angle`, `angle_b`).
    #[serde(default, skip_serializing_if = "FeatureParams::is_empty")]
    pub params: FeatureParams,
}

fn quarter() -> Real {
    TAU / 4.0
}

impl RevolveFeature {
    /// A full revolve of `profiles` around `axis` (id, name and sketch are
    /// set by [`super::PartDocument::add_revolve`]).
    pub fn new(profiles: Vec<ProfileRef>, axis: AxisRef) -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            sketch: String::new(),
            profiles,
            axis,
            extent: RevolveExtent::Full,
            angle: quarter(),
            angle_b: quarter(),
            direction: ExtentDirection::Default,
            operation: Operation::NewSolid,
            output: Output::Solid,
            solids: Vec::new(),
            params: FeatureParams::default(),
        }
    }

    /// Start angle and signed sweep.
    pub(super) fn sweep(&self) -> Result<(Real, Real), String> {
        if self.extent == RevolveExtent::Full {
            return Ok((0.0, TAU));
        }
        let pos = |v: Real, what: &str| {
            if v.is_finite() && v > 1e-9 && v <= TAU + 1e-9 {
                Ok(v)
            } else {
                Err(format!("{what} must be between 0 and 360 deg"))
            }
        };
        let a = pos(self.angle, "the angle")?;
        Ok(match self.direction {
            ExtentDirection::Default => (0.0, a),
            ExtentDirection::Flipped => (0.0, -a),
            ExtentDirection::Symmetric => (-a / 2.0, a),
            ExtentDirection::Asymmetric => {
                let b = pos(self.angle_b, "angle B")?;
                (-b, a + b)
            }
        })
    }

    /// Builds the revolve into `body`.
    pub fn build(&self, body: &mut BRep, sketch: &Sketch) -> Result<(), String> {
        let axis = resolve_axis(&self.axis, sketch)?;
        let (start, sweep) = self.sweep()?;
        let regions = sketch.regions();
        for (n, pr) in self.profiles.iter().enumerate() {
            let region = pr
                .resolve(&regions)
                .ok_or_else(|| format!("profile #{} no longer exists in the sketch", n + 1))?;
            let (outer, holes) = kernel_profile(region);
            let prefix = if n == 0 {
                self.id.clone()
            } else {
                format!("{}#{}", self.id, n + 1)
            };
            body.revolve_profile(
                &sketch.plane,
                &outer,
                &holes,
                axis,
                start,
                sweep,
                self.output == Output::Surface,
                &prefix,
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

impl RevolveFeature {
    /// The axis of revolution in model space: a point and a unit direction.
    pub fn world_axis(&self, sketch: &Sketch) -> Result<(crate::kernel::Point3, Vector3), String> {
        let a = resolve_axis(&self.axis, sketch)?;
        let pl = &sketch.plane;
        let o = pl.point_at(a.origin.x, a.origin.y);
        let d = pl.x_axis.into_inner() * a.dir.x + pl.y_axis().into_inner() * a.dir.y;
        if d.norm() < 1e-12 {
            return Err("degenerate revolve axis".into());
        }
        Ok((o, d.normalize()))
    }
}

/// The axis in sketch coordinates.
pub fn resolve_axis(axis: &AxisRef, sketch: &Sketch) -> Result<RevolveAxis, String> {
    match axis {
        AxisRef::SketchLine(id) => {
            let Some(Entity::Line { start, end }) = sketch.entity(*id) else {
                return Err("the revolve axis line no longer exists".into());
            };
            let (a, b) = (
                sketch.point(*start).map(|p| p.position),
                sketch.point(*end).map(|p| p.position),
            );
            let (Some(a), Some(b)) = (a, b) else {
                return Err("the revolve axis line no longer exists".into());
            };
            Ok(RevolveAxis {
                origin: a,
                dir: b - a,
            })
        }
        AxisRef::Origin(k) => {
            let plane = &sketch.plane;
            let d = [Vector3::x(), Vector3::y(), Vector3::z()]
                .get(*k)
                .copied()
                .ok_or("unknown origin axis")?;
            let off = plane.origin.coords.dot(&plane.normal);
            if d.dot(&plane.normal).abs() > 1e-9 || off.abs() > 1e-6 {
                return Err("the origin axis does not lie in the sketch plane".into());
            }
            let o = plane.project(&crate::kernel::Point3::origin());
            let e = plane.project(&crate::kernel::Point3::from(d));
            Ok(RevolveAxis {
                origin: o,
                dir: e - o,
            })
        }
    }
}

/// The axis Revolve picks by itself: the only centerline of the sketch.
pub fn auto_axis(sketch: &Sketch) -> Option<AxisRef> {
    let mut lines = sketch
        .entities()
        .filter(|(id, e)| {
            matches!(e, Entity::Line { .. }) && sketch.style(*id) == Style::Centerline
        })
        .map(|(id, _)| id);
    let first = lines.next()?;
    lines.next().is_none().then_some(AxisRef::SketchLine(first))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::part::PartDocument;
    use crate::part::placement::{OriginPlane, SketchPlacement};
    use crate::sketch::Point2;

    /// A part with one sketch on XY: a rectangle x 10..20, y 0..30 and a
    /// centerline on the Y axis.
    fn part() -> (PartDocument, usize) {
        let mut p = PartDocument::default();
        let opts = crate::part::SketchOptions {
            auto_project_origin: false,
            auto_reference_edges: false,
            ..Default::default()
        };
        let k = p
            .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts)
            .unwrap();
        let sk = &mut p.sketches[k].sketch;
        sk.add_rectangle_two_point(Point2::new(10.0, 0.0), Point2::new(20.0, 30.0))
            .unwrap();
        let l = sk.add_line(Point2::new(0.0, -5.0), Point2::new(0.0, 40.0));
        sk.set_style(l, Style::Centerline).unwrap();
        (p, k)
    }

    /// A half section closed by its centerline is a closed profile, and
    /// the centerline is also the axis (Inventor).
    #[test]
    fn centerline_closes_a_half_section() {
        let mut p = PartDocument::default();
        let opts = crate::part::SketchOptions {
            auto_project_origin: false,
            auto_reference_edges: false,
            ..Default::default()
        };
        let k = p
            .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts)
            .unwrap();
        let sk = &mut p.sketches[k].sketch;
        let a = sk.add_point(Point2::new(0.0, 0.0));
        let b = sk.add_point(Point2::new(20.0, 0.0));
        let c = sk.add_point(Point2::new(20.0, 10.0));
        let d = sk.add_point(Point2::new(0.0, 10.0));
        sk.add_line_between(a, b).unwrap();
        sk.add_line_between(b, c).unwrap();
        sk.add_line_between(c, d).unwrap();
        let axis = sk.add_line_between(d, a).unwrap();
        sk.set_style(axis, Style::Centerline).unwrap();
        let regions = sk.regions();
        assert_eq!(regions.len(), 1, "closed by the centerline");
        let pr = ProfileRef::of(&regions[0], Point2::new(10.0, 5.0));
        let auto = auto_axis(sk).unwrap();
        p.add_revolve(k, RevolveFeature::new(vec![pr], auto))
            .unwrap();
        let (sid, _) = p.body.solids().next().unwrap();
        let st = p.body.validate_solid(sid).unwrap();
        assert_eq!(st.genus(), Some(0), "a solid cylinder");
        let mut t = Vec::new();
        let v = crate::kernel::csg::volume(&p.body.solid_polygons(sid, &mut t).unwrap());
        let exact = std::f64::consts::PI * 400.0 * 10.0;
        assert!((v - exact).abs() / exact < 0.01, "volume {v} vs {exact}");
    }

    #[test]
    fn revolve_feature_full_and_cut() {
        let (mut p, k) = part();
        let sk = &p.sketches[k].sketch;
        let axis = auto_axis(sk).expect("one centerline");
        let regions = sk.regions();
        assert_eq!(regions.len(), 1, "the centerline is not a profile edge");
        let pr = ProfileRef::of(&regions[0], Point2::new(15.0, 15.0));
        let id = p
            .add_revolve(k, RevolveFeature::new(vec![pr.clone()], axis.clone()))
            .unwrap();
        assert_eq!(id, "Revolution1");
        assert_eq!(p.body.solids().count(), 1);
        // A partial revolve cut from it (shared sketch).
        let mut cut = RevolveFeature::new(vec![pr], axis);
        cut.extent = RevolveExtent::Angle;
        cut.angle = TAU / 4.0;
        cut.operation = Operation::Cut;
        p.add_revolve(k, cut).unwrap();
        assert_eq!(p.body.solids().count(), 1, "three quarters remain");
        let (sid, _) = p.body.solids().next().unwrap();
        p.body.validate_solid(sid).unwrap();
        let mut t = Vec::new();
        let v = crate::kernel::csg::volume(&p.body.solid_polygons(sid, &mut t).unwrap());
        let exact = 0.75 * std::f64::consts::PI * 300.0 * 30.0;
        assert!((v - exact).abs() / exact < 0.01, "volume {v} vs {exact}");
        // Deleting every feature leaves no body behind.
        {
            let mut e = p.clone();
            e.remove_feature(1);
            e.remove_feature(0);
            e.regenerate().unwrap();
            assert_eq!(e.body.solids().count(), 0);
        }
        // Saved and loaded, the part rebuilds the same body.
        let json = serde_json::to_string(&p).unwrap();
        let mut q: PartDocument = serde_json::from_str(&json).unwrap();
        q.regenerate().unwrap();
        assert!(q.feature_health.is_empty(), "{:?}", q.feature_health);
        assert_eq!(q.body.solids().count(), 1);
        assert_eq!(q.body.faces().count(), p.body.faces().count());
    }
}
