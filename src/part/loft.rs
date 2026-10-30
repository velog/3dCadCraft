//! The **Loft** feature (Inventor: 3D Model → Create → Loft), see
//! `docs/spec/5.2-loft.md`.
//!
//! A body through sections in order: closed sketch profiles, the outer loop
//! of a planar face, or a point at either end. Two sections give ruled
//! sides, three or more one smooth face per segment through all of them
//! (the exact kernel's `loft`); a point end is a pyramid (`loft_to_point`).

use serde::{Deserialize, Serialize};

use crate::kernel::exact::{self as kx, ExactSolid};
use crate::kernel::{
    BRep, Curve, Plane, Point3, ProfileSegment, Real, Segment2, TessellationOptions, Vector3,
};
use crate::sketch::PointId;

use super::PartSketch;
use super::exact::Fail;
use super::extrude::{ProfileRef, kernel_profile};
use super::params::FeatureParams;
use super::shell::FaceRef;
use super::sweep::PathRef;
use super::{Operation, Output};

/// One section of a loft.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum LoftSection {
    /// A closed region of a sketch.
    Profile {
        /// `id` of the sketch.
        sketch: String,
        /// The region.
        profile: ProfileRef,
    },
    /// A sketch point (first or last section only).
    Point {
        /// `id` of the sketch.
        sketch: String,
        /// The point.
        point: PointId,
    },
    /// The outer loop of a planar face.
    FaceLoop(FaceRef),
    /// A chain of model edges, e.g. a surface's free edges (format revision 41): two such
    /// sections give a surface bridging them (Output Surface).
    Edges(Vec<super::surface_op::SurfEdge>),
}

impl LoftSection {
    /// The sketch it uses.
    pub fn sketch_id(&self) -> Option<&str> {
        match self {
            LoftSection::Profile { sketch, .. } | LoftSection::Point { sketch, .. } => Some(sketch),
            LoftSection::FaceLoop(_) | LoftSection::Edges(_) => None,
        }
    }
}

/// A boundary condition of an end section or a rail (Conditions tab).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConditionKind {
    /// No condition.
    #[default]
    Free,
    /// Tangent (G1) to the neighbouring face.
    Tangent,
    /// At an angle to the section plane.
    Direction,
    /// Smooth (G2) with the neighbouring face.
    Smooth,
    /// A point end: a sharp tip.
    SharpPoint,
    /// A point end: a rounded tip.
    TangentPoint,
    /// A point end: rounded, tangent to a plane.
    TangentToPlane,
}

/// A condition with its angle and weight.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LoftCondition {
    /// The kind.
    pub kind: ConditionKind,
    /// Angle to the section plane (radians, 0…π; π/2 perpendicular).
    #[serde(default = "right")]
    pub angle: Real,
    /// How far the condition reaches (unitless).
    #[serde(default = "one")]
    pub weight: Real,
}

fn right() -> Real {
    std::f64::consts::FRAC_PI_2
}

fn one() -> Real {
    1.0
}

impl Default for LoftCondition {
    fn default() -> Self {
        Self {
            kind: ConditionKind::Free,
            angle: right(),
            weight: one(),
        }
    }
}

/// What guides the shape between the sections.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoftGuide {
    /// Rails (none: the sections alone).
    #[default]
    Rails,
    /// A centre line.
    CenterLine,
    /// Area Loft along a centre line.
    AreaLoft,
}

/// A placed section of an Area Loft.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlacedSection {
    /// Position along the centre line (0…1 when proportional, else mm).
    pub position: Real,
    /// Position as a fraction of the centre line's length.
    #[serde(default)]
    pub proportional: bool,
    /// Area (mm²) or scale factor.
    pub size: Real,
    /// `size` is a scale factor (else an area).
    #[serde(default)]
    pub scale: bool,
    /// Driving (else driven: size follows).
    #[serde(default)]
    pub driving: bool,
}

/// A manual point mapping (Transition tab): one model point per section.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MapPoint {
    /// A point on each section, in section order.
    pub points: Vec<[Real; 3]>,
}

/// A loft through sections.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LoftFeature {
    /// Immutable identity, the prefix of its topology names.
    pub id: String,
    /// Display name.
    pub name: String,
    /// The sections, in order.
    pub sections: Vec<LoftSection>,
    /// Rails, Center Line or Area Loft.
    #[serde(default)]
    pub guide: LoftGuide,
    /// Rails.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rails: Vec<PathRef>,
    /// The centre line (Center Line, Area Loft).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub center_line: Option<PathRef>,
    /// Placed sections (Area Loft).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub placed: Vec<PlacedSection>,
    /// The last section joins the first (a ring).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub closed_loop: bool,
    /// No edges between tangent faces.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub merge_tangent_faces: bool,
    /// Conditions by section index (missing: Free).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<LoftCondition>,
    /// Automatic Mapping (Transition tab).
    #[serde(default = "yes")]
    pub automatic_mapping: bool,
    /// Manual point sets (without Automatic Mapping).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub map_points: Vec<MapPoint>,
    /// Boolean operation with the body.
    #[serde(default)]
    pub operation: Operation,
    /// Solid or surface.
    #[serde(default)]
    pub output: Output,
    /// Bodies the operation works on (by body name); empty = the default.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub solids: Vec<String>,
    /// Parameter names and equations.
    #[serde(default, skip_serializing_if = "FeatureParams::is_empty")]
    pub params: FeatureParams,
    /// The preset the feature was made with (name only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
}

fn yes() -> bool {
    true
}

impl LoftFeature {
    /// A loft through `sections` (id and name are set by
    /// [`super::PartDocument::add_loft`]).
    pub fn new(sections: Vec<LoftSection>) -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            sections,
            guide: LoftGuide::Rails,
            rails: Vec::new(),
            center_line: None,
            placed: Vec::new(),
            closed_loop: false,
            merge_tangent_faces: false,
            conditions: Vec::new(),
            automatic_mapping: true,
            map_points: Vec::new(),
            operation: Operation::NewSolid,
            output: Output::Solid,
            solids: Vec::new(),
            params: FeatureParams::default(),
            preset: None,
        }
    }

    /// Every sketch it uses, sections first.
    pub fn sketch_ids(&self) -> Vec<&str> {
        let all = self
            .sections
            .iter()
            .map(LoftSection::sketch_id)
            .chain(self.rails.iter().map(PathRef::sketch_id))
            .chain(self.center_line.iter().map(PathRef::sketch_id));
        let mut v: Vec<&str> = Vec::new();
        for s in all.flatten() {
            if !s.is_empty() && !v.contains(&s) {
                v.push(s);
            }
        }
        v
    }

    /// The condition of section `k`.
    pub fn condition(&self, k: usize) -> LoftCondition {
        self.conditions.get(k).copied().unwrap_or_default()
    }

    /// Whether manual point mapping is in use.
    pub fn manual_mapping(&self) -> bool {
        !self.automatic_mapping && !self.map_points.is_empty()
    }

    /// Whether it needs the guided builder (rails, a centre line, Area
    /// Loft, conditions, point mapping or Closed Loop).
    pub fn guided(&self) -> bool {
        (self.guide == LoftGuide::Rails && !self.rails.is_empty())
            || self.guide != LoftGuide::Rails
            || self.closed_loop
            || self.manual_mapping()
            || self
                .conditions
                .iter()
                .any(|c| matches!(c.kind, ConditionKind::Tangent | ConditionKind::Direction))
    }

    /// The loft's solid, before its operation. `bodies`: the part's bodies so far (solids and
    /// surface bodies), which edge sections are found on.
    pub(super) fn build_in(
        &self,
        sketches: &[PartSketch],
        body: &BRep,
        bodies: &[ExactSolid],
    ) -> Result<Vec<ExactSolid>, Fail> {
        use Fail::User;
        if self
            .sections
            .iter()
            .any(|s| matches!(s, LoftSection::Edges(_)))
        {
            return self.bridge(bodies).map(|s| vec![s]);
        }
        let kernel = |e: String| User(format!("the geometry kernel cannot build it ({e})"));
        let surface = self.output == Output::Surface;
        if surface && (self.guided() || self.closed_loop) {
            return Err(User(
                "Surface output with rails, a centre line, conditions, point mapping or Closed Loop is not supported yet"
                    .into(),
            ));
        }
        let n = self.sections.len();
        if n < 2 {
            return Err(User("select at least two sections".into()));
        }
        self.check_conditions().map_err(User)?;
        let points: Vec<usize> = (0..n)
            .filter(|&k| matches!(self.sections[k], LoftSection::Point { .. }))
            .collect();
        if points.iter().any(|&k| k != 0 && k + 1 != n) {
            return Err(User(
                "a point can only be the first or the last section".into(),
            ));
        }
        if !points.is_empty() && surface {
            return Err(User(
                "a surface loft to a point is not supported yet".into(),
            ));
        }
        if !points.is_empty() {
            if self.closed_loop {
                return Err(User("a loft to a point cannot be a closed loop".into()));
            }
            if self.guided() {
                return Err(User(
                    "a loft to a point with rails, a centre line, conditions or point mapping \
                     is not supported yet"
                        .into(),
                ));
            }
            if n != 2 || points.len() != 1 {
                return Err(User(
                    "a loft to a point with more than one other section is not supported yet"
                        .into(),
                ));
            }
            let (pk, sk) = if points[0] == 0 { (0, 1) } else { (1, 0) };
            let apex = self.point(sketches, pk).map_err(User)?;
            let (plane, prof) = self.profile(sketches, body, sk).map_err(User)?;
            let s = kx::loft_to_point(&plane, &prof, apex, &self.id).map_err(kernel)?;
            return Ok(vec![s]);
        }
        let secs = (0..n)
            .map(|k| self.profile(sketches, body, k))
            .collect::<Result<Vec<_>, _>>()
            .map_err(User)?;
        for (k, w) in secs.windows(2).enumerate() {
            let (a, b) = (&w[0].0, &w[1].0);
            let same = a.normal.cross(&b.normal).norm() < 1e-9
                && (b.origin - a.origin).dot(&a.normal).abs() < 1e-6;
            if same {
                return Err(User(format!(
                    "sections {} and {} lie in the same plane",
                    k + 1,
                    k + 2
                )));
            }
        }
        if self.closed_loop && n < 3 {
            return Err(User("Closed Loop needs at least three sections".into()));
        }
        let refs: Vec<(Plane, &kx::ExactProfile)> =
            secs.iter().map(|(p, s)| (p.clone(), s)).collect();
        if self.guided() {
            let g = self.guided_input(sketches, body, refs).map_err(User)?;
            return kx::loft_guided(&g, &self.id).map(|s| vec![s]).map_err(User);
        }
        let s = if surface {
            kx::loft_surface(&refs, &self.id).map_err(kernel)?
        } else {
            kx::loft(&refs, &self.id).map_err(kernel)?
        };
        Ok(vec![s])
    }

    /// The surface bodies a Loft with Output Solid sews into its solid (edge sections of whole
    /// boundaries): they leave the part, as Stitch's inputs do.
    pub(super) fn sewn_surfaces(&self, sheets: &[ExactSolid]) -> Vec<String> {
        if self.output != Output::Solid {
            return Vec::new();
        }
        let mut out = Vec::new();
        for s in &self.sections {
            let LoftSection::Edges(chain) = s else {
                return Vec::new();
            };
            let Some(e) = chain.first() else {
                continue;
            };
            if let Some(Some((i, _))) =
                kx::faces_in_solids(sheets, &[(e.face.as_str(), Point3::from(e.hint))]).pop()
                && let Some(n) = sheets[i].name.clone()
            {
                out.push(n);
            }
        }
        out
    }

    /// Two edge sections bridged (G0 to both): open chains by one surface (Output Surface);
    /// whole boundaries of two surfaces by one face per pair of sides — with Output Solid the two
    /// surfaces and the bridges become one solid. No rails, centre line or conditions yet.
    fn bridge(&self, bodies: &[ExactSolid]) -> Result<ExactSolid, Fail> {
        use Fail::User;
        let solid = self.output == Output::Solid;
        let chains: Vec<&Vec<super::surface_op::SurfEdge>> = self
            .sections
            .iter()
            .map(|s| match s {
                LoftSection::Edges(e) => Ok(e),
                _ => Err(User(
                    "edge sections go with edge sections only (two of them) for now".into(),
                )),
            })
            .collect::<Result<_, _>>()?;
        let [a, b] = chains.as_slice() else {
            return Err(User(
                "a loft between edges takes two edge sections for now".into(),
            ));
        };
        // Conditions: Free or Tangent at each edge section (the bridge leaves the edges tangent
        // to their surface); rails, a centre line and the other conditions not yet.
        let weights = [0, 1].map(|k| {
            let c = self.condition(k);
            (c.kind == ConditionKind::Tangent).then_some(c.weight)
        });
        let other_condition = self
            .conditions
            .iter()
            .any(|c| !matches!(c.kind, ConditionKind::Free | ConditionKind::Tangent));
        let plain = Self {
            conditions: Vec::new(),
            ..self.clone()
        };
        if plain.guided() || other_condition {
            return Err(User(
                "a loft between edges with rails, a centre line or conditions other than Tangent is not supported yet"
                    .into(),
            ));
        }
        // Each section: the body holding its edges, and a point on each edge.
        let side = |chain: &[super::surface_op::SurfEdge], k: usize| {
            let refs: Vec<(&str, Point3)> = chain
                .iter()
                .map(|e| (e.face.as_str(), Point3::from(e.hint)))
                .collect();
            let found = kx::faces_in_solids(bodies, &refs);
            let mut owner: Option<usize> = None;
            for (e, f) in chain.iter().zip(found) {
                let Some((i, _)) = f else {
                    return Err(User(format!(
                        "section {k}: the edge along {} no longer exists",
                        e.face
                    )));
                };
                if owner.is_some_and(|o| o != i) {
                    return Err(User(format!(
                        "section {k}: its edges belong to different bodies"
                    )));
                }
                owner = Some(i);
            }
            let i = owner.ok_or_else(|| User(format!("section {k} has no edges")))?;
            let pts: Vec<Point3> = chain.iter().map(|e| Point3::from(e.hint)).collect();
            Ok((i, pts))
        };
        let (ia, pa) = side(a, 1)?;
        let (ib, pb) = side(b, 2)?;
        let bridge =
            kx::surface_ops::bridge_edges(&bodies[ia], &pa, &bodies[ib], &pb, weights, &self.id)
                .map_err(|e| User(format!("the geometry kernel cannot bridge the edges ({e})")))?;
        if !solid {
            return Ok(bridge);
        }
        // Output Solid: the two surfaces and the bridges sewn into one body, every face facing
        // out — a surface made the other way round is turned over first (B, A, then both).
        let (sa, sb) = (&bodies[ia], &bodies[ib]);
        let (ta, tb) = (
            kx::surface_ops::reversed(sa).ok(),
            kx::surface_ops::reversed(sb).ok(),
        );
        let mut tries: Vec<(&ExactSolid, &ExactSolid)> = vec![(sa, sb)];
        if let Some(t) = &tb {
            tries.push((sa, t));
        }
        if let Some(t) = &ta {
            tries.push((t, sb));
        }
        if let (Some(x), Some(y)) = (&ta, &tb) {
            tries.push((x, y));
        }
        let mut last = String::from("no try");
        let mut found = None;
        for (a, b) in tries {
            match kx::surface_ops::stitch(&[a, b, &bridge], BRIDGE_STITCH, false, &self.id) {
                Ok(r) if !r.is_open() && faces_out(&r) => {
                    found = Some(r);
                    break;
                }
                Ok(r) if r.is_open() => {
                    last = concat!(
                        "the bridges' edges do not match the surfaces' (sewing them into a solid ",
                        "is not supported yet: use Output Surface, spec 5.4a L3)"
                    )
                    .into()
                }
                Ok(_) => last = "the faces do not all face out".into(),
                Err(e) => last = e,
            }
        }
        let r = found.ok_or_else(|| {
            User(format!(
                "the surfaces and the bridges cannot be sewn ({last})"
            ))
        })?;
        if r.is_open() {
            return Err(User(
                concat!(
                    "the bridges and the two surfaces do not close a volume ",
                    "(pick whole boundaries, or set Output to Surface)"
                )
                .into(),
            ));
        }
        Ok(r)
    }

    /// Refuses conditions that do not fit their section or are not built yet.
    fn check_conditions(&self) -> Result<(), String> {
        let n = self.sections.len();
        for (k, c) in self.conditions.iter().enumerate().take(n) {
            let sec = &self.sections[k];
            let point = matches!(sec, LoftSection::Point { .. });
            let end = k == 0 || k + 1 == n;
            match c.kind {
                ConditionKind::Free => {}
                ConditionKind::SharpPoint
                | ConditionKind::TangentPoint
                | ConditionKind::TangentToPlane
                    if !point =>
                {
                    return Err(format!(
                        "section {} is not a point (point conditions apply to point sections)",
                        k + 1
                    ));
                }
                ConditionKind::SharpPoint => {}
                ConditionKind::TangentPoint | ConditionKind::TangentToPlane => {
                    return Err(
                        "a rounded point (Tangent / Tangent to Plane) is not supported yet: the \
                         kernel lofts to a point with straight sides only"
                            .into(),
                    );
                }
                ConditionKind::Smooth => {
                    return Err(
                        "the Smooth (G2) condition is not supported yet (use Tangent)".into(),
                    );
                }
                ConditionKind::Tangent | ConditionKind::Direction if point => {
                    return Err(format!(
                        "section {} is a point: use a point condition",
                        k + 1
                    ));
                }
                ConditionKind::Tangent | ConditionKind::Direction if !end => {
                    return Err(format!(
                        "section {}: conditions apply to the first and the last section only",
                        k + 1
                    ));
                }
                ConditionKind::Tangent | ConditionKind::Direction if self.closed_loop => {
                    return Err("a closed loop has no end sections to take conditions".into());
                }
                ConditionKind::Tangent => {
                    if !matches!(sec, LoftSection::FaceLoop(_)) {
                        return Err(format!(
                            "Tangent needs a face loop (section {} is a sketch profile: use \
                             Direction)",
                            k + 1
                        ));
                    }
                }
                ConditionKind::Direction => {
                    if matches!(sec, LoftSection::FaceLoop(_)) {
                        return Err(format!(
                            "Direction applies to sketch sections (section {} is a face loop: \
                             use Tangent)",
                            k + 1
                        ));
                    }
                }
            }
            if matches!(c.kind, ConditionKind::Tangent | ConditionKind::Direction)
                && !(0.0..=std::f64::consts::PI).contains(&c.angle)
            {
                return Err("the angle must be between 0 and 180 deg".into());
            }
            if matches!(c.kind, ConditionKind::Tangent | ConditionKind::Direction)
                && !(c.weight >= 0.0 && c.weight <= 50.0)
            {
                return Err("the weight must be between 0 and 50".into());
            }
        }
        Ok(())
    }

    /// The guided builder's input.
    fn guided_input<'a>(
        &self,
        sketches: &[PartSketch],
        body: &BRep,
        sections: Vec<(Plane, &'a kx::ExactProfile)>,
    ) -> Result<kx::GuidedLoft<'a>, String> {
        let n = sections.len();
        let near = sections
            .first()
            .map(|s| s.0.origin)
            .unwrap_or_else(Point3::origin);
        let rails = if self.guide == LoftGuide::Rails {
            self.rails
                .iter()
                .enumerate()
                .map(|(j, r)| {
                    r.resolve(sketches, body, near)
                        .map(|g| g.polyline())
                        .map_err(|e| format!("rail {}: {e}", j + 1))
                })
                .collect::<Result<Vec<_>, _>>()?
        } else {
            Vec::new()
        };
        let center = match self.guide {
            LoftGuide::Rails => None,
            LoftGuide::CenterLine | LoftGuide::AreaLoft => {
                let c = self
                    .center_line
                    .as_ref()
                    .filter(|c| !c.is_empty())
                    .ok_or("select the centre line")?;
                Some(
                    c.resolve(sketches, body, near)
                        .map_err(|e| format!("centre line: {e}"))?
                        .polyline(),
                )
            }
        };
        let area = (self.guide == LoftGuide::AreaLoft).then(|| {
            self.placed
                .iter()
                .filter(|p| p.driving)
                .map(|p| kx::LoftPlaced {
                    position: p.position,
                    proportional: p.proportional,
                    size: p.size,
                    scale: p.scale,
                })
                .collect()
        });
        let end = |k: usize| -> Result<kx::LoftEnd, String> {
            let c = self.condition(k);
            Ok(match c.kind {
                ConditionKind::Direction => kx::LoftEnd::Direction {
                    angle: c.angle,
                    weight: c.weight,
                },
                ConditionKind::Tangent => match self.sections.get(k) {
                    Some(LoftSection::FaceLoop(f)) => kx::LoftEnd::Tangent {
                        dirs: tangent_dirs(body, f)?,
                        weight: c.weight,
                    },
                    _ => return Err("Tangent needs a face loop".into()),
                },
                _ => kx::LoftEnd::Free,
            })
        };
        let map = if self.manual_mapping() {
            self.map_points
                .iter()
                .map(|m| m.points.iter().map(|p| Point3::from(*p)).collect())
                .collect()
        } else {
            Vec::new()
        };
        Ok(kx::GuidedLoft {
            sections,
            rails,
            center,
            area,
            start: end(0)?,
            end: end(n - 1)?,
            map,
            closed: self.closed_loop,
        })
    }

    /// The guided loft's stations (outlines in model space), for previews.
    pub fn stations(
        &self,
        sketches: &[PartSketch],
        body: &BRep,
    ) -> Result<Vec<Vec<Point3>>, String> {
        let secs = (0..self.sections.len())
            .map(|k| self.profile(sketches, body, k))
            .collect::<Result<Vec<_>, _>>()?;
        let refs: Vec<(Plane, &kx::ExactProfile)> =
            secs.iter().map(|(p, s)| (p.clone(), s)).collect();
        let g = self.guided_input(sketches, body, refs)?;
        kx::loft_stations(&g)
    }

    /// Section `k`'s plane and outer outline (model space; for picking map
    /// points). `None` for a point section or a missing one.
    pub fn section_outline(
        &self,
        sketches: &[PartSketch],
        body: &BRep,
        k: usize,
    ) -> Option<(Plane, Vec<Point3>)> {
        let (plane, prof) = self.profile(sketches, body, k).ok()?;
        let pts = kx::profile_outline(&plane, &prof);
        Some((plane, pts))
    }

    /// The area (mm²) of section `k` (Area Loft's driven sections).
    pub fn section_area(&self, sketches: &[PartSketch], body: &BRep, k: usize) -> Option<Real> {
        self.profile(sketches, body, k)
            .ok()
            .map(|(_, p)| kx::profile_area(&p))
    }

    /// A point section's model point.
    fn point(&self, sketches: &[PartSketch], k: usize) -> Result<Point3, String> {
        let Some(LoftSection::Point { sketch, point }) = self.sections.get(k) else {
            return Err("not a point section".into());
        };
        let ps = sketches
            .iter()
            .find(|s| &s.id == sketch)
            .ok_or_else(|| format!("sketch {sketch} is missing"))?;
        let p = ps
            .sketch
            .point(*point)
            .ok_or("the section point no longer exists")?
            .position;
        Ok(ps.sketch.plane.point_at(p.x, p.y))
    }

    /// A region or face loop section: its plane and profile.
    fn profile(
        &self,
        sketches: &[PartSketch],
        body: &BRep,
        k: usize,
    ) -> Result<(Plane, kx::ExactProfile), String> {
        match self.sections.get(k) {
            Some(LoftSection::Profile { sketch, profile }) => {
                let ps = sketches
                    .iter()
                    .find(|s| &s.id == sketch)
                    .ok_or_else(|| format!("sketch {sketch} is missing"))?;
                let regions = ps.sketch.regions();
                let region = profile
                    .resolve(&regions)
                    .ok_or_else(|| format!("section {} no longer exists in its sketch", k + 1))?;
                let (outer, holes) = kernel_profile(region);
                Ok((ps.sketch.plane.clone(), kx::profile(&outer, &holes)?))
            }
            Some(LoftSection::FaceLoop(f)) => face_profile(body, f),
            Some(LoftSection::Edges(_)) => Err(format!(
                "section {} is a chain of edges (only with another edge section)",
                k + 1
            )),
            Some(LoftSection::Point { .. }) => Err(format!(
                "section {} is a point (only the first or the last may be)",
                k + 1
            )),
            None => Err("the section no longer exists".into()),
        }
    }
}

/// How far a bridge's own edges may be from the surfaces' edges when they are sewn (mm): the
/// bridge's edges are cubic splines through points on the surfaces' edges.
const BRIDGE_STITCH: Real = 0.01;

/// Whether a closed body's faces face out: the signed volume of its display triangles is
/// positive (a sewn shell is oriented one way throughout, so the sign tells). Asking the kernel
/// whether points lie inside re-meshed the body for every point: 290 s for tangent bridges.
fn faces_out(s: &ExactSolid) -> bool {
    let Ok(tris) = s.triangles() else {
        return false;
    };
    let v: Real = tris
        .iter()
        .map(|[a, b, c]| a.coords.dot(&b.coords.cross(&c.coords)) / 6.0)
        .sum();
    v > 0.0
}

/// Tangent condition of a face-loop section: at points along the face's
/// outline, the direction its neighbouring face runs on in past the edge.
fn tangent_dirs(body: &BRep, r: &FaceRef) -> Result<Vec<(Point3, Vector3)>, String> {
    let f = super::shell::face_near(body, &r.face, Point3::from(r.hint))
        .ok_or_else(|| format!("the face {} no longer exists", r.face))?;
    let face = body.face(f).ok_or("the section face no longer exists")?;
    let opts = TessellationOptions::default();
    let mut out = Vec::new();
    for ce in &face.outer.coedges {
        let Some(adj) = body.edge_faces(ce.edge).into_iter().find(|&g| g != f) else {
            continue;
        };
        let Some(mesh) = body.face(adj).and_then(|x| x.mesh.clone()) else {
            continue;
        };
        let pts = body
            .sample_edge(ce.edge, &opts)
            .map_err(|e| e.to_string())?;
        for w in pts.windows(2) {
            let q = nalgebra::center(&w[0], &w[1]);
            let Some(t) = (w[1] - w[0]).try_normalize(1e-12) else {
                continue;
            };
            // The neighbour's triangle nearest the point: its normal and a
            // point inside it.
            let best = mesh
                .triangles
                .iter()
                .filter_map(|tri| {
                    let [a, b, c] = tri.map(|i| mesh.positions.get(i as usize).copied());
                    let (a, b, c) = (a?, b?, c?);
                    let g = Point3::from((a.coords + b.coords + c.coords) / 3.0);
                    let nrm = (b - a).cross(&(c - a)).try_normalize(1e-18)?;
                    Some(((g - q).norm(), g, nrm))
                })
                .min_by(|x, y| x.0.total_cmp(&y.0));
            let Some((_, inside, nrm)) = best else {
                continue;
            };
            let Some(mut d) = t.cross(&nrm).try_normalize(1e-12) else {
                continue;
            };
            if (q - inside).dot(&d) < 0.0 {
                d = -d;
            }
            out.push((q, d));
        }
    }
    if out.is_empty() {
        return Err("the section face has no neighbouring faces to be tangent to".into());
    }
    Ok(out)
}

/// The outer loop of a planar face as a profile on its plane: lines and
/// arcs exactly, other edges by their points.
fn face_profile(body: &BRep, r: &FaceRef) -> Result<(Plane, kx::ExactProfile), String> {
    let f = super::shell::face_near(body, &r.face, Point3::from(r.hint))
        .ok_or_else(|| format!("the face {} no longer exists", r.face))?;
    let plane = body.face_plane(f).ok_or("the section face is not planar")?;
    let face = body.face(f).ok_or("the section face no longer exists")?;
    let opts = TessellationOptions::default();
    let mut segs = Vec::new();
    for (k, ce) in face.outer.coedges.iter().enumerate() {
        let mut pts = body
            .sample_edge(ce.edge, &opts)
            .map_err(|e| e.to_string())?;
        if ce.sense != crate::kernel::Sense::Same {
            pts.reverse();
        }
        let uv: Vec<nalgebra::Point2<Real>> = pts.iter().map(|p| plane.project(p)).collect();
        let (Some(&a), Some(&b)) = (uv.first(), uv.last()) else {
            continue;
        };
        let name = format!("{}.e{k}", r.face);
        let curve = body.edge(ce.edge).and_then(|e| body.curve(e.curve));
        match curve {
            Some(Curve::Line(_)) => segs.push(ProfileSegment {
                geometry: Segment2::Line { a, b },
                name,
            }),
            Some(Curve::Circle(c)) => {
                let cc = plane.project(&c.center);
                let sweep: Real = uv
                    .windows(2)
                    .map(|w| {
                        let (p, q) = (w[0] - cc, w[1] - cc);
                        p.perp(&q).atan2(p.dot(&q))
                    })
                    .sum();
                segs.push(ProfileSegment {
                    geometry: Segment2::Arc {
                        c: cc,
                        r: c.radius,
                        a0: (a.y - cc.y).atan2(a.x - cc.x),
                        sweep,
                    },
                    name,
                });
            }
            _ => {
                for (j, w) in uv.windows(2).enumerate() {
                    segs.push(ProfileSegment {
                        geometry: Segment2::Line { a: w[0], b: w[1] },
                        name: format!("{name}.{j}"),
                    });
                }
            }
        }
    }
    if segs.is_empty() {
        return Err("the section face has no outline".into());
    }
    Ok((plane, kx::profile(&segs, &[])?))
}

#[cfg(test)]
#[path = "loft_tests.rs"]
mod tests;
