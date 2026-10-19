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
    BRep, Curve, Plane, Point3, ProfileSegment, Real, Segment2, TessellationOptions,
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
}

impl LoftSection {
    /// The sketch it uses.
    pub fn sketch_id(&self) -> Option<&str> {
        match self {
            LoftSection::Profile { sketch, .. } | LoftSection::Point { sketch, .. } => Some(sketch),
            LoftSection::FaceLoop(_) => None,
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

    /// The loft's solid, before its operation.
    pub(super) fn build(
        &self,
        sketches: &[PartSketch],
        body: &BRep,
    ) -> Result<Vec<ExactSolid>, Fail> {
        use Fail::User;
        let kernel = |e: String| User(format!("the geometry kernel cannot build it ({e})"));
        if self.output == Output::Surface {
            return Err(User("Surface output is not supported yet".into()));
        }
        match self.guide {
            LoftGuide::Rails if !self.rails.is_empty() => {
                return Err(User("Rails are not supported yet".into()));
            }
            LoftGuide::CenterLine => {
                return Err(User("Center Line is not supported yet".into()));
            }
            LoftGuide::AreaLoft => return Err(User("Area Loft is not supported yet".into())),
            LoftGuide::Rails => {}
        }
        if !self.automatic_mapping && !self.map_points.is_empty() {
            return Err(User("manual point mapping is not supported yet".into()));
        }
        let n = self.sections.len();
        if n < 2 {
            return Err(User("select at least two sections".into()));
        }
        for (k, c) in self.conditions.iter().enumerate().take(n) {
            let point = matches!(self.sections.get(k), Some(LoftSection::Point { .. }));
            match c.kind {
                ConditionKind::Free => {}
                ConditionKind::SharpPoint if point => {}
                ConditionKind::SharpPoint
                | ConditionKind::TangentPoint
                | ConditionKind::TangentToPlane
                    if !point =>
                {
                    return Err(User(format!(
                        "section {} is not a point (point conditions apply to point sections)",
                        k + 1
                    )));
                }
                kind => {
                    return Err(User(format!("the condition {kind:?} is not supported yet")));
                }
            }
        }
        let points: Vec<usize> = (0..n)
            .filter(|&k| matches!(self.sections[k], LoftSection::Point { .. }))
            .collect();
        if points.iter().any(|&k| k != 0 && k + 1 != n) {
            return Err(User(
                "a point can only be the first or the last section".into(),
            ));
        }
        if !points.is_empty() {
            if self.closed_loop {
                return Err(User("a loft to a point cannot be a closed loop".into()));
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
        let mut s = kx::loft(&refs, &self.id).map_err(kernel)?;
        if self.closed_loop
            && let (Some(last), Some(first)) = (refs.last(), refs.first())
        {
            // The closing span (last → first), joined at both ends.
            let close = kx::loft(&[last.clone(), first.clone()], &self.id).map_err(kernel)?;
            s = kx::boolean(&s, &close, kx::ExactOp::Union)
                .map_err(|e| User(format!("the geometry kernel cannot close the loop ({e})")))?
                .ok_or_else(|| User("the loft is empty".into()))?;
        }
        Ok(vec![s])
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
            Some(LoftSection::Point { .. }) => Err(format!(
                "section {} is a point (only the first or the last may be)",
                k + 1
            )),
            None => Err("the section no longer exists".into()),
        }
    }
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
