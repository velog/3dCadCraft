//! The **Extrude** feature (Inventor: 3D Model → Create → Extrude).
//!
//! An extrusion references a sketch (by its immutable `id`) and one or more
//! of its closed regions. A region is remembered by the sketch curves of
//! its outer boundary ([`ProfileRef::sources`]) — that survives dimension
//! edits — with a point inside it ([`ProfileRef::seed`]) as fallback when
//! the boundary curves changed (e.g. a line was split).
//!
//! The full behaviour (Inventor's options and our status) is specified in
//! `docs/spec/2.1-extrude.md`. Until Boolean operations exist (FEATURES
//! 2.13) every extrusion is a **new solid** of the part's body.

use serde::{Deserialize, Serialize};
use slotmap::Key;

use crate::kernel::{BRep, ExtrudeLevel, Plane, ProfileSegment, Real, Segment2};
use crate::sketch::{EntityId, Point2, ProfileCurve, ProfileLoop, Region, Sketch};

/// Which way an extrusion goes from its sketch plane.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExtentDirection {
    /// Along the sketch normal.
    #[default]
    Default,
    /// Against the sketch normal.
    Flipped,
    /// Half the distance to each side.
    Symmetric,
    /// Distance A along the normal, distance B against it.
    Asymmetric,
}

/// Where an extrusion ends (Inventor's extents).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Extent {
    /// At a distance (A, and B for Asymmetric).
    #[default]
    Distance,
    /// At a face, work plane or vertex (with `from`: Between).
    To(TargetRef),
    /// At the next face of the body, in the direction, that covers the
    /// whole profile.
    ToNext,
    /// Through all of the body in the direction.
    ThroughAll,
}

/// Model geometry an extent refers to, by persistent name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TargetRef {
    /// A planar face.
    Face(String),
    /// A vertex: the termination plane through it, parallel to the sketch.
    Vertex(String),
}

impl TargetRef {
    /// Persistent name of the referenced entity.
    pub fn name(&self) -> &str {
        match self {
            TargetRef::Face(n) | TargetRef::Vertex(n) => n,
        }
    }

    /// Offset of the target along `plane`'s normal (from its origin).
    /// Faces must be planar and parallel to the sketch for now.
    fn offset(&self, body: &BRep, plane: &Plane) -> Result<Real, String> {
        let n = plane.normal.into_inner();
        match self {
            TargetRef::Vertex(name) => body
                .find_vertex(name)
                .and_then(|v| body.vertex(v))
                .map(|v| (v.point - plane.origin).dot(&n))
                .ok_or_else(|| format!("vertex {name} not found")),
            TargetRef::Face(name) => {
                let f = body
                    .find_face(name)
                    .ok_or_else(|| format!("face {name} not found"))?;
                let fp = body
                    .face_plane(f)
                    .ok_or_else(|| format!("face {name} is not planar"))?;
                if fp.normal.cross(&plane.normal).norm() > 1e-9 {
                    return Err(format!(
                        "face {name} is not parallel to the sketch (only parallel faces are supported yet)"
                    ));
                }
                Ok((fp.origin - plane.origin).dot(&n))
            }
        }
    }
}

/// A closed region of a sketch chosen as (part of) a profile.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProfileRef {
    /// The sketch curves of the region's outer boundary (sorted, unique).
    pub sources: Vec<EntityId>,
    /// A point inside the region (sketch coordinates), used when no region
    /// has exactly these boundary curves any more.
    pub seed: Point2,
}

/// An extrusion of sketch regions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExtrudeFeature {
    /// Immutable identity, the prefix of its topology names.
    pub id: String,
    /// Display name (free to rename).
    pub name: String,
    /// `id` of the profile sketch.
    pub sketch: String,
    /// The chosen regions.
    pub profiles: Vec<ProfileRef>,
    /// Distance A (mm, > 0).
    pub distance: Real,
    /// Distance B for [`ExtentDirection::Asymmetric`] (mm).
    #[serde(default)]
    pub distance_b: Real,
    /// Direction of the extent.
    #[serde(default)]
    pub direction: ExtentDirection,
    /// Where it ends.
    #[serde(default)]
    pub extent: Extent,
    /// Start face instead of the sketch plane (with `To`: Between).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<TargetRef>,
    /// Taper A (radians; positive grows the section along the extrusion).
    #[serde(default)]
    pub taper: Real,
    /// Taper B, the second side of an Asymmetric extrusion (radians).
    #[serde(default)]
    pub taper_b: Real,
    /// Boolean operation with the body.
    #[serde(default)]
    pub operation: super::Operation,
    /// Solid or surface.
    #[serde(default)]
    pub output: super::Output,
    /// Bodies the operation works on (Inventor's **Solids**), by body name;
    /// empty = the default (Join: one body, Cut/Intersect: all).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub solids: Vec<String>,
    /// Parameter names and equations of the numeric fields (see
    /// [`super::params`]).
    #[serde(
        default,
        skip_serializing_if = "super::params::FeatureParams::is_empty"
    )]
    pub params: super::params::FeatureParams,
}

/// Outer-boundary curves of a region: its identity in a [`ProfileRef`].
pub fn region_sources(region: &Region) -> Vec<EntityId> {
    let mut s: Vec<EntityId> = region.outer.edges.iter().map(|e| e.source).collect();
    s.sort();
    s.dedup();
    s
}

impl ProfileRef {
    /// A reference to `region`, seeded at `seed`.
    pub fn of(region: &Region, seed: Point2) -> Self {
        Self {
            sources: region_sources(region),
            seed,
        }
    }

    /// Finds the referenced region among `regions`.
    pub fn resolve<'a>(&self, regions: &'a [Region]) -> Option<&'a Region> {
        regions
            .iter()
            .find(|r| region_sources(r) == self.sources)
            .or_else(|| regions.iter().find(|r| r.contains(self.seed)))
    }
}

/// Name of a sketch entity in topology names, e.g. `"3v1"`.
fn entity_key(e: EntityId) -> String {
    format!("{:?}", e.data())
}

fn segment(curve: &ProfileCurve) -> Segment2 {
    match curve {
        ProfileCurve::Line { a, b } => Segment2::Line { a: *a, b: *b },
        ProfileCurve::Arc { c, r, a0, sweep } => Segment2::Arc {
            c: *c,
            r: *r,
            a0: *a0,
            sweep: *sweep,
        },
        ProfileCurve::Spline {
            points,
            reversed,
            knots,
        } => Segment2::Spline {
            points: points.clone(),
            reversed: *reversed,
            knots: knots.clone(),
        },
    }
}

/// Kernel profile of a region: segments named `"<entity>:<k>"`, k counting
/// the pieces of the same sketch curve in the region.
pub(super) fn kernel_profile(region: &Region) -> (Vec<ProfileSegment>, Vec<Vec<ProfileSegment>>) {
    let mut seen: Vec<EntityId> = Vec::new();
    let mut convert = |lp: &ProfileLoop| -> Vec<ProfileSegment> {
        lp.edges
            .iter()
            .map(|e| {
                let k = seen.iter().filter(|&&s| s == e.source).count();
                seen.push(e.source);
                ProfileSegment {
                    geometry: segment(&e.curve),
                    name: format!("{}:{k}", entity_key(e.source)),
                }
            })
            .collect()
    };
    let outer = convert(&region.outer);
    let holes = region.holes.iter().map(&mut convert).collect();
    (outer, holes)
}

/// The profiles a new Extrude / Revolve starts with: the only region of
/// the sketch (Inventor), or else every region the user's own curves close
/// (no projected reference edge on its boundary) — 40 rectangles drawn on a
/// face are all taken at once — except one that fills a hole of another such
/// region (a plate with a hole keeps the hole).
pub fn default_profiles(ps: &super::PartSketch, regions: &[Region]) -> Vec<ProfileRef> {
    if let [r] = regions {
        return vec![ProfileRef::of(r, r.interior_point())];
    }
    let own = |r: &Region| {
        std::iter::once(&r.outer)
            .chain(&r.holes)
            .flat_map(|l| l.edges.iter())
            .all(|e| !ps.reference_lines.contains(&e.source))
    };
    let islands: Vec<&Region> = regions.iter().filter(|r| own(r)).collect();
    let fills_a_hole = |r: &Region| {
        let p = r.interior_point();
        islands.iter().any(|o| {
            !std::ptr::eq(*o, r)
                && o.holes.iter().any(|h| {
                    let poly = h.polygon(1e-3);
                    let mut inside = false;
                    for k in 0..poly.len() {
                        let (a, b) = (poly[k], poly[(k + 1) % poly.len()]);
                        if (a.y > p.y) != (b.y > p.y)
                            && p.x < a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y)
                        {
                            inside = !inside;
                        }
                    }
                    inside
                })
        })
    };
    islands
        .iter()
        .filter(|r| !fills_a_hole(r))
        .map(|r| ProfileRef::of(r, r.interior_point()))
        .collect()
}

impl ExtrudeFeature {
    /// An extrusion of `profiles` by `distance` with default options (id,
    /// name and sketch are set by [`super::PartDocument::add_extrude`]).
    pub fn new(profiles: Vec<ProfileRef>, distance: Real) -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            sketch: String::new(),
            profiles,
            distance,
            distance_b: distance,
            direction: ExtentDirection::Default,
            extent: Extent::Distance,
            from: None,
            taper: 0.0,
            taper_b: 0.0,
            operation: Default::default(),
            output: Default::default(),
            solids: Vec::new(),
            params: Default::default(),
        }
    }

    /// Whether the body's material lies just below the first profile (on
    /// the side opposite the sketch normal) and not above it: a cut there
    /// should go against the normal. `None`: no profile / no body.
    pub fn material_below(&self, body: &BRep, sketch: &Sketch) -> Option<bool> {
        let regions = sketch.regions();
        let r = self.profiles.first()?.resolve(&regions)?;
        let p = r.interior_point();
        let plane = &sketch.plane;
        let n = plane.normal.into_inner();
        let at = plane.point_at(p.x, p.y);
        let eps = 1e-3;
        let (below, above) = (
            body.contains_point(&(at - n * eps)),
            body.contains_point(&(at + n * eps)),
        );
        Some(below && !above)
    }

    /// Builds the extrusion into `body` from the (already up-to-date)
    /// profile sketch. Each region becomes one solid; with several regions
    /// the topology names get a `#<n>` suffix on the prefix.
    pub fn build(&self, body: &mut BRep, sketch: &Sketch) -> Result<(), String> {
        let regions = sketch.regions();
        let plane: &Plane = &sketch.plane;
        for (n, pr) in self.profiles.iter().enumerate() {
            let region = pr
                .resolve(&regions)
                .ok_or_else(|| format!("profile #{} no longer exists in the sketch", n + 1))?;
            let levels = self.levels(body, plane, region)?;
            let (outer, holes) = kernel_profile(region);
            let prefix = if n == 0 {
                self.id.clone()
            } else {
                format!("{}#{}", self.id, n + 1)
            };
            let id = body
                .extrude_profile(plane, &outer, &holes, &levels, &prefix)
                .map_err(|e| e.to_string())?;
            if self.output == super::Output::Surface {
                body.open_up(id, &[format!("{prefix}:Start"), format!("{prefix}:End")]);
            }
        }
        Ok(())
    }

    /// The cross-section levels (offsets along the sketch normal and
    /// taper insets) for one region, from direction, extents and tapers.
    pub(super) fn levels(
        &self,
        body: &BRep,
        plane: &Plane,
        region: &Region,
    ) -> Result<Vec<ExtrudeLevel>, String> {
        let positive = |v: Real, what: &str| {
            if v.is_finite() && v > 0.0 {
                Ok(v)
            } else {
                Err(format!("{what} must be positive"))
            }
        };
        let s0 = match &self.from {
            Some(t) => t.offset(body, plane)?,
            None => 0.0,
        };
        let sign = if self.direction == ExtentDirection::Flipped {
            -1.0
        } else {
            1.0
        };
        let (ta, tb) = (self.taper.tan(), self.taper_b.tan());
        let one = |end: Real| -> Result<Vec<ExtrudeLevel>, String> {
            if (end - s0).abs() < 1e-9 {
                return Err("the extrusion has zero depth".into());
            }
            Ok(vec![
                ExtrudeLevel {
                    offset: s0,
                    inset: 0.0,
                },
                ExtrudeLevel {
                    offset: end,
                    inset: (end - s0).abs() * ta,
                },
            ])
        };
        // Two-sided around s0: `up` along the normal (taper A), `down`
        // against it (taper B); kinked at s0 only when tapered.
        let two = |up: Real, down: Real, tb: Real| {
            let mut v = vec![ExtrudeLevel {
                offset: s0 - down,
                inset: down * tb,
            }];
            if ta != 0.0 || tb != 0.0 {
                v.push(ExtrudeLevel {
                    offset: s0,
                    inset: 0.0,
                });
            }
            v.push(ExtrudeLevel {
                offset: s0 + up,
                inset: up * ta,
            });
            v
        };
        match &self.extent {
            Extent::Distance => {
                let a = positive(self.distance, "the distance")?;
                match self.direction {
                    ExtentDirection::Default | ExtentDirection::Flipped => one(s0 + sign * a),
                    ExtentDirection::Symmetric => Ok(two(a / 2.0, a / 2.0, ta)),
                    ExtentDirection::Asymmetric => {
                        let b = positive(self.distance_b, "distance B")?;
                        Ok(two(a, b, tb))
                    }
                }
            }
            Extent::To(t) => one(t.offset(body, plane)?),
            Extent::ToNext => {
                let n = plane.normal.into_inner();
                let probe: Vec<_> = region
                    .outer
                    .polygon(0.05)
                    .into_iter()
                    .chain(std::iter::once(region.interior_point()))
                    .collect();
                body.faces()
                    .filter_map(|(f, _)| {
                        let fp = body.face_plane(f)?;
                        if fp.normal.cross(&plane.normal).norm() > 1e-9 {
                            return None;
                        }
                        let t = (fp.origin - plane.origin).dot(&n);
                        let covers = probe
                            .iter()
                            .all(|p| body.face_contains(f, &(plane.point_at(p.x, p.y) + n * t)));
                        ((t - s0) * sign > 1e-9 && covers).then_some(t)
                    })
                    .min_by(|a, b| ((a - s0) * sign).total_cmp(&((b - s0) * sign)))
                    .ok_or_else(|| {
                        "To Next: no face in that direction covers the profile".to_owned()
                    })
                    .and_then(one)
            }
            Extent::ThroughAll => {
                let n = plane.normal.into_inner();
                // The far corner of the body's bounding box in that direction.
                let far = body.bounding_box().map_or(0.0, |(lo, hi)| {
                    (0..8)
                        .map(|i| {
                            let c = crate::kernel::Point3::new(
                                if i & 1 == 0 { lo.x } else { hi.x },
                                if i & 2 == 0 { lo.y } else { hi.y },
                                if i & 4 == 0 { lo.z } else { hi.z },
                            );
                            ((c - plane.origin).dot(&n) - s0) * sign
                        })
                        .fold(0.0, Real::max)
                });
                if far <= 1e-9 {
                    return Err("Through All: there is no body in that direction".into());
                }
                one(s0 + sign * far)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::part::{
        Feature, OriginPlane, PartDocument, PlaneRef, SketchOptions, SketchPlacement,
    };

    /// A box 80×60×20 and a sketch on the XY origin plane (z = 0) with a
    /// circle inside the box's footprint; returns the part, sketch index,
    /// the circle's profile and its topology key.
    fn box_and_circle() -> (PartDocument, usize, ProfileRef, String) {
        let mut part = PartDocument::with_box("Box1", [80.0, 60.0, 20.0]).unwrap();
        let placement = SketchPlacement {
            plane: PlaneRef::Origin(OriginPlane::XY),
            ..Default::default()
        };
        let opts = SketchOptions {
            auto_project_origin: false,
            ..Default::default()
        };
        let i = part.create_sketch(placement, &opts).unwrap();
        let sk = &mut part.sketches[i].sketch;
        let c = sk.add_circle(Point2::new(10.0, 10.0), 5.0).unwrap();
        let r = sk.regions();
        let pr = ProfileRef::of(&r[0], r[0].interior_point());
        (part, i, pr, entity_key(c))
    }

    /// z of the start (`S`) or end (`E`) vertex of the circle in `id`.
    fn z(part: &PartDocument, id: &str, level: &str, key: &str) -> Real {
        let v = part
            .body
            .find_vertex(&format!("{id}:V{level}:{key}:0"))
            .unwrap_or_else(|| panic!("{id}:V{level}"));
        part.body.vertex(v).unwrap().point.z
    }

    /// A region bounded by an interpolation spline (non-uniform knots) and
    /// a line extrudes into a valid solid.
    #[test]
    fn extrudes_a_region_with_an_interpolation_spline() {
        let mut part = PartDocument::default();
        let placement = SketchPlacement {
            plane: PlaneRef::Origin(OriginPlane::XY),
            ..Default::default()
        };
        let opts = SketchOptions {
            auto_project_origin: false,
            auto_reference_edges: false,
            ..Default::default()
        };
        let i = part.create_sketch(placement, &opts).unwrap();
        let sk = &mut part.sketches[i].sketch;
        let pts: Vec<_> = [(0.0, 0.0), (8.0, 12.0), (30.0, 9.0), (40.0, 0.0)]
            .iter()
            .map(|&(x, y)| sk.add_point(Point2::new(x, y)))
            .collect();
        sk.add_interp_spline(pts.clone()).unwrap();
        sk.add_line_between(pts[3], pts[0]).unwrap();
        let r = sk.regions();
        assert_eq!(r.len(), 1);
        let pr = ProfileRef::of(&r[0], r[0].interior_point());
        part.add_extrude(i, ExtrudeFeature::new(vec![pr], 5.0))
            .unwrap();
        let (sid, _) = part.body.solids().next().unwrap();
        part.body.validate_solid(sid).unwrap();

        // A closed interpolation spline alone is a profile too.
        let i = part
            .create_sketch(
                SketchPlacement {
                    plane: PlaneRef::Origin(OriginPlane::XY),
                    ..Default::default()
                },
                &opts,
            )
            .unwrap();
        let sk = &mut part.sketches[i].sketch;
        let pts: Vec<_> = [(0.0, 30.0), (20.0, 32.0), (25.0, 50.0), (5.0, 45.0)]
            .iter()
            .map(|&(x, y)| sk.add_point(Point2::new(x, y)))
            .collect();
        let s = sk.add_interp_spline(pts).unwrap();
        sk.set_spline_closed(s, true).unwrap();
        let r = sk.regions();
        assert_eq!(r.len(), 1);
        let pr = ProfileRef::of(&r[0], r[0].interior_point());
        part.add_extrude(i, ExtrudeFeature::new(vec![pr], 5.0))
            .unwrap();
        for (sid, _) in part.body.solids() {
            part.body.validate_solid(sid).unwrap();
        }
    }

    /// Inventor's **Solids**: a Cut limited to one body leaves the other
    /// alone; the bodies keep their names (the creating feature's id).
    #[test]
    fn cut_only_the_chosen_body() {
        let mut part = PartDocument::with_box("Box1", [80.0, 60.0, 20.0]).unwrap();
        let opts = SketchOptions {
            auto_project_origin: false,
            auto_reference_edges: false,
            ..Default::default()
        };
        let on_xy = || SketchPlacement {
            plane: PlaneRef::Origin(OriginPlane::XY),
            ..Default::default()
        };
        // A second body: a cylinder beside the box.
        let i = part.create_sketch(on_xy(), &opts).unwrap();
        let sk = &mut part.sketches[i].sketch;
        sk.add_circle(Point2::new(120.0, 30.0), 10.0).unwrap();
        let r = sk.regions();
        let pr = ProfileRef::of(&r[0], r[0].interior_point());
        let mut cyl = ExtrudeFeature::new(vec![pr], 20.0);
        cyl.operation = crate::part::Operation::NewSolid;
        let cyl_id = part.add_extrude(i, cyl).unwrap();
        // A slab over both, cut 5 mm deep into the cylinder only.
        let j = part.create_sketch(on_xy(), &opts).unwrap();
        let sk = &mut part.sketches[j].sketch;
        sk.add_rectangle_two_point(Point2::new(-10.0, -10.0), Point2::new(150.0, 70.0))
            .unwrap();
        let r = sk.regions();
        let pr = ProfileRef::of(&r[0], r[0].interior_point());
        let mut cut = ExtrudeFeature::new(vec![pr], 5.0);
        cut.operation = crate::part::Operation::Cut;
        cut.solids = vec![cyl_id.clone()];
        part.add_extrude(j, cut).unwrap();
        let vol = |name: &str| {
            let (sid, _) = part
                .body
                .solids()
                .find(|(_, s)| s.name.as_deref() == Some(name))
                .unwrap();
            let mut t = Vec::new();
            crate::kernel::csg::volume(&part.body.solid_polygons(sid, &mut t).unwrap())
        };
        assert!(
            (vol("Box1") - 80.0 * 60.0 * 20.0).abs() < 1e-6,
            "box untouched"
        );
        let exact = std::f64::consts::PI * 100.0 * 15.0;
        assert!((vol(&cyl_id) - exact).abs() / exact < 0.01, "cylinder cut");
        assert_eq!(
            part.bodies_before(part.features.len()),
            vec![
                ("Box1".to_owned(), "Solid1".to_owned()),
                (cyl_id.clone(), "Solid2".to_owned())
            ]
        );
        // A chosen body that no longer exists makes the feature sick.
        if let crate::part::Feature::Extrude(e) = &mut part.features[2] {
            e.solids = vec!["Gone".into()];
        }
        part.regenerate().unwrap();
        assert!(
            part.feature_health
                .values()
                .any(|m| m.contains("no longer exists"))
        );
    }

    #[test]
    fn directions_extents_from_and_taper() {
        type Case = (Box<dyn Fn(&mut ExtrudeFeature)>, Real, Real);
        let cases: Vec<Case> = vec![
            (Box::new(|_| {}), 0.0, 15.0),
            (
                Box::new(|f| f.direction = ExtentDirection::Flipped),
                0.0,
                -15.0,
            ),
            (
                Box::new(|f| f.direction = ExtentDirection::Symmetric),
                -7.5,
                7.5,
            ),
            (
                Box::new(|f| {
                    f.direction = ExtentDirection::Asymmetric;
                    f.distance_b = 4.0;
                }),
                -4.0,
                15.0,
            ),
            (
                Box::new(|f| f.extent = Extent::To(TargetRef::Face("Box1:Top".into()))),
                0.0,
                20.0,
            ),
            (
                Box::new(|f| f.extent = Extent::To(TargetRef::Vertex("Box1:V111".into()))),
                0.0,
                20.0,
            ),
            (Box::new(|f| f.extent = Extent::ToNext), 0.0, 20.0),
            (Box::new(|f| f.extent = Extent::ThroughAll), 0.0, 20.0),
            (
                // Between: from the top face to 5 mm above it.
                Box::new(|f| f.from = Some(TargetRef::Face("Box1:Top".into()))),
                20.0,
                35.0,
            ),
        ];
        for (k, (set, z0, z1)) in cases.into_iter().enumerate() {
            let (mut part, i, pr, key) = box_and_circle();
            let mut def = ExtrudeFeature::new(vec![pr], 15.0);
            set(&mut def);
            let id = part
                .add_extrude(i, def)
                .unwrap_or_else(|e| panic!("case {k}: {e}"));
            assert!(
                (z(&part, &id, "S", &key) - z0).abs() < 1e-9,
                "case {k} start"
            );
            assert!((z(&part, &id, "E", &key) - z1).abs() < 1e-9, "case {k} end");
        }
        // Taper: the circle grows by 15·tan(10°) at the end.
        let (mut part, i, pr, key) = box_and_circle();
        let mut def = ExtrudeFeature::new(vec![pr], 15.0);
        def.taper = 10f64.to_radians();
        let id = part.add_extrude(i, def).unwrap();
        let v = part.body.find_vertex(&format!("{id}:VE:{key}:0")).unwrap();
        let p = part.body.vertex(v).unwrap().point;
        let r = ((p.x - 10.0).powi(2) + (p.y - 10.0).powi(2)).sqrt();
        assert!((r - (5.0 + 15.0 * 10f64.to_radians().tan())).abs() < 1e-9);
        // Errors: To a face that is not parallel; nothing behind for To Next.
        let (mut part, i, pr, _) = box_and_circle();
        let mut def = ExtrudeFeature::new(vec![pr.clone()], 15.0);
        def.extent = Extent::To(TargetRef::Face("Box1:Front".into()));
        assert!(part.add_extrude(i, def).is_err());
        let mut def = ExtrudeFeature::new(vec![pr], 15.0);
        def.extent = Extent::ToNext;
        def.direction = ExtentDirection::Flipped;
        assert!(part.add_extrude(i, def).is_err());
    }

    #[test]
    fn consumed_and_shared_sketches() {
        let (mut part, i, pr, _) = box_and_circle();
        part.add_extrude(i, ExtrudeFeature::new(vec![pr.clone()], 5.0))
            .unwrap();
        let ps = &part.sketches[i];
        assert!(!ps.visible && !ps.shared, "consumed and hidden");
        assert_eq!(part.owner_of_sketch(i), Some(1));
        // A second feature from the same sketch shares it.
        part.add_extrude(i, ExtrudeFeature::new(vec![pr], 3.0))
            .unwrap();
        assert!(part.sketches[i].shared);
        assert_eq!(part.owner_of_sketch(i), None);
        assert!(part.unshare_sketch(i).is_err(), "two features use it");
        part.features.pop();
        part.unshare_sketch(i).unwrap();
        assert_eq!(part.owner_of_sketch(i), Some(1));
    }

    #[test]
    fn extrusion_follows_the_sketch_and_names_its_faces() {
        let mut part = PartDocument::with_box("Box1", [80.0, 60.0, 20.0]).unwrap();
        let placement = SketchPlacement {
            plane: PlaneRef::Face("Box1:Top".into()),
            ..Default::default()
        };
        let opts = SketchOptions {
            auto_reference_edges: false,
            ..Default::default()
        };
        let i = part.create_sketch(placement, &opts).unwrap();
        let sk = &mut part.sketches[i].sketch;
        let c = sk.add_circle(Point2::new(10.0, 10.0), 5.0).unwrap();
        let regions = sk.regions();
        assert_eq!(regions.len(), 1);
        let pr = ProfileRef::of(&regions[0], Point2::new(10.0, 10.0));
        let id = part
            .add_extrude(i, ExtrudeFeature::new(vec![pr], 15.0))
            .unwrap();
        assert_eq!(id, "Extrusion1");
        let end = part.body.find_face("Extrusion1:End").expect("named cap");
        let z = part
            .body
            .vertex(
                part.body
                    .find_vertex(&format!("Extrusion1:VE:{}:0", entity_key(c)))
                    .unwrap(),
            )
            .unwrap()
            .point
            .z;
        assert!((z - 35.0).abs() < 1e-9, "box top 20 + 15: {z}");
        let _ = end;
        // Taller box: the sketch moves with its face, the cylinder follows.
        if let Some(Feature::Box(b)) = part.features.first_mut() {
            b.size[2] = 30.0;
        }
        part.regenerate().unwrap();
        assert!(part.feature_health.is_empty(), "{:?}", part.feature_health);
        let v = part
            .body
            .find_vertex(&format!("Extrusion1:VE:{}:0", entity_key(c)))
            .unwrap();
        assert!((part.body.vertex(v).unwrap().point.z - 45.0).abs() < 1e-9);
        // A region that is gone makes the feature unhealthy, not the part broken.
        part.sketches[i].sketch.remove_entity(c).unwrap();
        part.regenerate().unwrap();
        assert!(part.feature_health.contains_key("Extrusion1"));
        // Adding an extrusion that cannot be built is refused.
        assert!(
            part.add_extrude(i, ExtrudeFeature::new(vec![], 5.0))
                .is_err()
        );
        // The feature survives saving and loading.
        let json = serde_json::to_string(&part).unwrap();
        let mut back: PartDocument = serde_json::from_str(&json).unwrap();
        back.regenerate().unwrap();
        assert_eq!(back.features, part.features);
    }
}

#[cfg(test)]
mod default_profile_tests {
    use super::default_profiles;
    use crate::part::{
        ExtrudeFeature, OriginPlane, PartDocument, ProfileRef, SketchOptions, SketchPlacement,
    };
    use crate::sketch::Point2;

    /// 40 rectangles drawn on a box's face (its edges projected): all 40
    /// are the starting profiles, not the face's outline region.
    #[test]
    fn many_rectangles_on_a_face_are_all_taken() {
        let mut part = PartDocument::default();
        let opts = SketchOptions {
            auto_project_origin: false,
            ..Default::default()
        };
        let i = part
            .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts)
            .unwrap();
        part.sketches[i]
            .sketch
            .add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(100.0, 50.0))
            .unwrap();
        let r = part.sketches[i].sketch.regions();
        part.add_extrude(
            i,
            ExtrudeFeature::new(vec![ProfileRef::of(&r[0], r[0].interior_point())], 5.0),
        )
        .unwrap();
        let with_edges = SketchOptions {
            auto_project_origin: false,
            auto_reference_edges: true,
            ..Default::default()
        };
        let j = part
            .create_sketch(SketchPlacement::on_face("Extrusion1:End"), &with_edges)
            .unwrap();
        let plane = part.sketches[j].sketch.plane.clone();
        for a in 0..10 {
            for b in 0..4 {
                let (x, y) = (2.0 + 10.0 * a as f64, 2.0 + 12.0 * b as f64);
                let p = plane.project(&crate::kernel::Point3::new(x, y, 5.0));
                let q = plane.project(&crate::kernel::Point3::new(x + 6.0, y + 8.0, 5.0));
                part.sketches[j]
                    .sketch
                    .add_rectangle_two_point(p, q)
                    .unwrap();
            }
        }
        let regions = part.sketches[j].sketch.regions();
        assert_eq!(
            regions.len(),
            41,
            "40 rectangles and the face outline with 40 holes"
        );
        // The shading of the outline region covers exactly its area.
        for r in &regions {
            let outer = r.outer.polygon(0.05);
            let holes: Vec<_> = r.holes.iter().map(|h| h.polygon(0.05)).collect();
            let (pts, tris) = crate::kernel::triangulate_with_holes(&outer, &holes);
            let tri: f64 = tris
                .iter()
                .map(|t| ((pts[t[1]] - pts[t[0]]).perp(&(pts[t[2]] - pts[t[0]])) / 2.0).abs())
                .sum();
            assert!((tri - r.area()).abs() < 1e-6, "{tri} vs {}", r.area());
        }
        let picked = default_profiles(&part.sketches[j], &regions);
        assert_eq!(picked.len(), 40);
    }

    /// A plate with a hole drawn by the user: the ring only (the circle
    /// inside fills its hole).
    #[test]
    fn a_plate_with_a_hole_keeps_the_hole() {
        let mut part = PartDocument::default();
        let opts = SketchOptions {
            auto_project_origin: false,
            ..Default::default()
        };
        let i = part
            .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts)
            .unwrap();
        let sk = &mut part.sketches[i].sketch;
        sk.add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(40.0, 30.0))
            .unwrap();
        sk.add_circle(Point2::new(20.0, 15.0), 5.0).unwrap();
        let regions = sk.regions();
        assert_eq!(regions.len(), 2);
        let picked = default_profiles(&part.sketches[i], &regions);
        assert_eq!(picked.len(), 1);
        let ring = regions.iter().find(|r| !r.holes.is_empty()).unwrap();
        assert_eq!(
            picked[0].sources,
            ProfileRef::of(ring, ring.interior_point()).sources
        );
    }
}
