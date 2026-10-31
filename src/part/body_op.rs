//! Body operations of the 3D Model tab that change faces or bodies directly
//! (`docs/spec/2.14-thicken-offset.md`; later Delete Face, Move Bodies, Copy
//! Object, Replace Face, the surface commands). One feature type,
//! [`Feature::BodyOp`](super::Feature::BodyOp), like the sheet metal
//! operations: a new operation is a new [`BodyOp`] variant (and a format
//! revision), not a new `Feature` variant.
//!
//! **Thicken/Offset**: picked faces of a solid (or whole surface bodies,
//! *Quilt*) get a thickness — a slab joined to, cut from, intersected with
//! the solid, or a new solid — or are offset into a surface body. Faces are
//! [`FaceRef`]s (persistent name + a point), bodies persistent names.
//! New faces: `<id>:Offset:<face>` (the moved copy of a face), walls
//! `<id>:Wall:<k>`; the face's own copy keeps the face's name.

use serde::{Deserialize, Serialize};

use super::boolean::Operation;
use super::expr::Unit;
use super::shell::FaceRef;
use super::work::LineRef;
use crate::kernel::Real;

/// What Thicken/Offset takes (Inventor's Select: Face · Quilt).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThickenSelect {
    /// Faces of solids (or of surface bodies).
    #[default]
    Face,
    /// Whole surface bodies.
    Quilt,
}

/// Solid (a thickness) or Surface (an offset copy).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThickenOutput {
    /// A thickness: a slab joined, cut, intersected or a new solid.
    #[default]
    Solid,
    /// An offset copy of the faces (a surface body).
    Surface,
}

/// Which way the thickness or offset goes from the faces.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThickenDirection {
    /// Along the faces' outward normals.
    #[default]
    Direction1,
    /// Against them (into the solid).
    Direction2,
    /// Half each way.
    Symmetric,
}

/// Thicken/Offset.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ThickenOp {
    /// Face or Quilt.
    #[serde(default)]
    pub select: ThickenSelect,
    /// The faces (Face).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub faces: Vec<FaceRef>,
    /// The surface bodies (Quilt), by persistent name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub quilts: Vec<String>,
    /// The solid the result is combined with (multi-body parts); empty: the
    /// solid holding the faces.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub solids: Vec<String>,
    /// Solid or Surface.
    #[serde(default)]
    pub output: ThickenOutput,
    /// Join / Cut / Intersect / New Solid (Output Solid).
    pub operation: Operation,
    /// Thickness or offset (mm, > 0).
    pub distance: Real,
    /// Direction 1, Direction 2 or Symmetric.
    #[serde(default)]
    pub direction: ThickenDirection,
    /// Faces tangent to a picked one come with it.
    #[serde(default = "yes")]
    pub automatic_face_chain: bool,
    /// Create Vertical Surfaces (Output Surface): not built yet.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub vertical_surfaces: bool,
    /// More › Automatic Blending (kept; the kernel blends smooth faces).
    #[serde(default = "yes")]
    pub automatic_blending: bool,
    /// More › Allow Approximation, Specify Tolerance (mm): how far a fitted
    /// free-form face may stray from the true offset. `None`: Optimized (the
    /// finest of 0.01 … 1 mm that fits, at most 2 % of the distance).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<Real>,
}

fn yes() -> bool {
    true
}

impl ThickenOp {
    /// Inventor's defaults: Face, Solid, Join, 1 mm, Direction 1, chains on.
    pub fn new() -> Self {
        Self {
            select: ThickenSelect::Face,
            faces: Vec::new(),
            quilts: Vec::new(),
            solids: Vec::new(),
            output: ThickenOutput::Solid,
            operation: Operation::Join,
            distance: 1.0,
            direction: ThickenDirection::Direction1,
            automatic_face_chain: true,
            vertical_surfaces: false,
            automatic_blending: true,
            tolerance: None,
        }
    }

    /// The offsets of the result's two sides from the faces (`lo`, `hi`),
    /// and whether it is symmetric.
    pub fn signed(&self) -> (Real, bool) {
        match self.direction {
            ThickenDirection::Direction1 => (self.distance, false),
            ThickenDirection::Direction2 => (-self.distance, false),
            ThickenDirection::Symmetric => (self.distance, true),
        }
    }

    /// Checks the inputs before any geometry work.
    pub fn validate(&self) -> Result<(), String> {
        if !(self.distance.is_finite() && self.distance > 0.0) {
            return Err("the distance must be greater than 0".into());
        }
        let picked: Result<(), String> = match self.select {
            ThickenSelect::Face if self.faces.is_empty() => Err("select the faces".into()),
            ThickenSelect::Quilt if self.quilts.is_empty() => {
                Err("select the surface bodies (quilts)".into())
            }
            _ => Ok(()),
        };
        picked?;
        if self
            .tolerance
            .is_some_and(|t| !(t.is_finite() && (1e-6..=10.0).contains(&t)))
        {
            return Err("the tolerance must be 0.000001 … 10 mm".into());
        }
        if self.vertical_surfaces {
            return Err("Create Vertical Surfaces is not supported yet".into());
        }
        if self.output == ThickenOutput::Surface && self.direction == ThickenDirection::Symmetric {
            return Err("an offset surface goes one way (Direction 1 or 2)".into());
        }
        if self.output == ThickenOutput::Surface && self.select == ThickenSelect::Quilt {
            return Err("an offset surface is made from faces (Select: Face)".into());
        }
        Ok(())
    }
}

impl Default for ThickenOp {
    fn default() -> Self {
        Self::new()
    }
}

/// The body operations (one feature type).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum BodyOp {
    /// Thicken/Offset.
    Thicken(ThickenOp),
    /// Delete Face (format revision 34).
    DeleteFace(DeleteFaceOp),
    /// Move Bodies (format revision 35).
    MoveBodies(MoveBodiesOp),
    /// Stitch (format revision 37, like every surface command below).
    Stitch(super::surface_op::StitchOp),
    /// Boundary Patch.
    Patch(super::surface_op::PatchOp),
    /// Trim (a surface).
    Trim(super::surface_op::TrimOp),
    /// Extend (a surface).
    Extend(super::surface_op::ExtendOp),
    /// Ruled Surface.
    Ruled(super::surface_op::RuledOp),
    /// Replace Face.
    ReplaceFace(super::surface_op::ReplaceFaceOp),
    /// Sculpt.
    Sculpt(super::surface_op::SculptOp),
    /// Copy Object (format revision 40).
    CopyObject(super::copy_object::CopyObjectOp),
    /// Import of a STEP file (format revision 42).
    Import(super::import::ImportOp),
    /// A frame member's body: its section between two planes (format
    /// revision 57).
    Prism(super::frame::PrismOp),
}

/// One step of Move Bodies (Inventor's move types); the steps apply in
/// order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MoveStep {
    /// Free Drag: offsets along the part's X, Y, Z (mm).
    FreeDrag {
        /// Along X (mm).
        #[serde(default)]
        x: Real,
        /// Along Y (mm).
        #[serde(default)]
        y: Real,
        /// Along Z (mm).
        #[serde(default)]
        z: Real,
    },
    /// Move Along Ray: `offset` along an edge or axis (`flip`: the other
    /// way).
    AlongRay {
        /// The edge or axis giving the direction.
        direction: LineRef,
        /// The other way along it.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        flip: bool,
        /// How far (mm).
        offset: Real,
    },
    /// Rotate About Line: `angle` (radians, right-handed about the line's
    /// direction; `flip`: the other way) about an edge or axis.
    RotateAboutLine {
        /// The edge or axis turned about.
        axis: LineRef,
        /// The other sense of rotation.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        flip: bool,
        /// The angle (radians).
        angle: Real,
    },
}

impl MoveStep {
    /// The step's name in the panel's Move Type list.
    pub fn label(&self) -> &'static str {
        match self {
            MoveStep::FreeDrag { .. } => "Free Drag",
            MoveStep::AlongRay { .. } => "Move Along Ray",
            MoveStep::RotateAboutLine { .. } => "Rotate About Line",
        }
    }
}

/// **Move Bodies** (`docs/spec/2.13-move-bodies.md`): solid or surface
/// bodies of a multi-body part moved and turned by one or more steps.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MoveBodiesOp {
    /// The bodies, by persistent name.
    pub bodies: Vec<String>,
    /// The steps, applied in order.
    pub steps: Vec<MoveStep>,
}

impl MoveBodiesOp {
    /// No bodies, one Free Drag step of zero (Inventor's start).
    pub fn new() -> Self {
        Self {
            bodies: Vec::new(),
            steps: vec![MoveStep::FreeDrag {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }],
        }
    }

    /// Refuses a definition that cannot be built (before any geometry).
    pub fn validate(&self) -> Result<(), String> {
        if self.bodies.is_empty() {
            return Err("select the bodies to move".into());
        }
        if self.steps.is_empty() {
            return Err("add a move".into());
        }
        if self.steps.len() > MAX_STEPS {
            return Err(format!("at most {MAX_STEPS} moves in one feature"));
        }
        let finite = self.steps.iter().all(|s| match s {
            MoveStep::FreeDrag { x, y, z } => x.is_finite() && y.is_finite() && z.is_finite(),
            MoveStep::AlongRay { offset, .. } => offset.is_finite(),
            MoveStep::RotateAboutLine { angle, .. } => angle.is_finite(),
        });
        if !finite {
            return Err("a value is not a number".into());
        }
        Ok(())
    }
}

impl Default for MoveBodiesOp {
    fn default() -> Self {
        Self::new()
    }
}

/// The most steps of one Move Bodies feature (each value gets a parameter
/// name of its own).
pub const MAX_STEPS: usize = 9;

/// Field names of step k's values (`x1`, `offset2`, `angle3` …).
const XS: [&str; MAX_STEPS] = ["x1", "x2", "x3", "x4", "x5", "x6", "x7", "x8", "x9"];
const YS: [&str; MAX_STEPS] = ["y1", "y2", "y3", "y4", "y5", "y6", "y7", "y8", "y9"];
const ZS: [&str; MAX_STEPS] = ["z1", "z2", "z3", "z4", "z5", "z6", "z7", "z8", "z9"];
const OFFSETS: [&str; MAX_STEPS] = [
    "offset1", "offset2", "offset3", "offset4", "offset5", "offset6", "offset7", "offset8",
    "offset9",
];
const ANGLES: [&str; MAX_STEPS] = [
    "angle1", "angle2", "angle3", "angle4", "angle5", "angle6", "angle7", "angle8", "angle9",
];

/// Step k's fields and the value each one reaches.
fn step_fields(k: usize, s: &MoveStep) -> Vec<(&'static str, Unit)> {
    if k >= MAX_STEPS {
        return Vec::new();
    }
    match s {
        MoveStep::FreeDrag { .. } => vec![(XS[k], Unit::Mm), (YS[k], Unit::Mm), (ZS[k], Unit::Mm)],
        MoveStep::AlongRay { .. } => vec![(OFFSETS[k], Unit::Mm)],
        MoveStep::RotateAboutLine { .. } => vec![(ANGLES[k], Unit::Deg)],
    }
}

/// The step index and the value a field name stands for.
fn step_value<'a>(op: &'a mut MoveBodiesOp, field: &str) -> Option<&'a mut Real> {
    let k = field
        .trim_start_matches(|c: char| c.is_ascii_alphabetic())
        .parse::<usize>()
        .ok()?
        .checked_sub(1)?;
    let kind = field.trim_end_matches(|c: char| c.is_ascii_digit());
    match (op.steps.get_mut(k)?, kind) {
        (MoveStep::FreeDrag { x, .. }, "x") => Some(x),
        (MoveStep::FreeDrag { y, .. }, "y") => Some(y),
        (MoveStep::FreeDrag { z, .. }, "z") => Some(z),
        (MoveStep::AlongRay { offset, .. }, "offset") => Some(offset),
        (MoveStep::RotateAboutLine { angle, .. }, "angle") => Some(angle),
        _ => None,
    }
}

/// Delete Face's **Select**: single faces, or a whole lump or void.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeleteSelect {
    /// Individual Faces (Heal may close the gap).
    #[default]
    Faces,
    /// Lump or Void: the shell of the picked face goes whole (a void fills
    /// with material); Heal does not apply.
    LumpOrVoid,
}

/// **Delete Face** (`docs/spec/2.14-delete-face.md`): faces of one body
/// deleted; with Heal the planar neighbours extend over the gap, without it
/// the body opens into a surface body.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeleteFaceOp {
    /// Individual Faces or Lump or Void.
    #[serde(default)]
    pub select: DeleteSelect,
    /// The faces (Lump or Void: a face of each shell to delete).
    pub faces: Vec<FaceRef>,
    /// Heal (off): close the gap by extending the neighbouring faces.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub heal: bool,
}

impl DeleteFaceOp {
    /// No faces yet, Individual Faces, no Heal (Inventor's defaults).
    pub fn new() -> Self {
        Self {
            select: DeleteSelect::Faces,
            faces: Vec::new(),
            heal: false,
        }
    }

    /// Refuses a definition that cannot be built (before any geometry).
    pub fn validate(&self) -> Result<(), String> {
        if self.faces.is_empty() {
            return Err("select the faces to delete".into());
        }
        Ok(())
    }
}

impl Default for DeleteFaceOp {
    fn default() -> Self {
        Self::new()
    }
}

/// A body operation feature (`Thicken1`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BodyOpFeature {
    /// Immutable id (the new faces are named after it).
    pub id: String,
    /// Display name.
    pub name: String,
    /// What it does.
    pub op: BodyOp,
    /// Equations of the fields (`distance`).
    #[serde(
        default,
        skip_serializing_if = "super::params::FeatureParams::is_empty"
    )]
    pub params: super::params::FeatureParams,
}

impl BodyOpFeature {
    /// A feature of this kind (its id is given by `add_body_op`).
    pub fn new(op: BodyOp) -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            op,
            params: Default::default(),
        }
    }

    /// The command's name: the id's prefix and the browser's name.
    pub fn kind_name(&self) -> &'static str {
        match self.op {
            BodyOp::Thicken(_) => "Thicken",
            BodyOp::DeleteFace(_) => "DeleteFace",
            BodyOp::MoveBodies(_) => "MoveBodies",
            BodyOp::Stitch(_) => "Stitch",
            BodyOp::Patch(_) => "BoundaryPatch",
            BodyOp::Trim(_) => "TrimSurface",
            BodyOp::Extend(_) => "ExtendSurface",
            BodyOp::Ruled(_) => "RuledSurface",
            BodyOp::ReplaceFace(_) => "ReplaceFace",
            BodyOp::Sculpt(_) => "Sculpt",
            BodyOp::CopyObject(_) => "CopyObject",
            BodyOp::Import(_) => "Import",
            BodyOp::Prism(_) => "Prism",
        }
    }

    /// The sketch it uses (a Boundary Patch of a sketch region).
    pub fn sketch(&self) -> Option<&str> {
        match &self.op {
            BodyOp::Patch(p) => p.sketch.as_deref(),
            BodyOp::Trim(t) => t.sketch(),
            _ => None,
        }
    }

    /// Refuses a definition that cannot be built (before any geometry).
    pub fn validate(&self) -> Result<(), String> {
        match &self.op {
            BodyOp::Thicken(t) => t.validate(),
            BodyOp::DeleteFace(d) => d.validate(),
            BodyOp::MoveBodies(m) => m.validate(),
            BodyOp::Stitch(o) => o.validate(),
            BodyOp::Patch(o) => o.validate(),
            BodyOp::Trim(o) => o.validate(),
            BodyOp::Extend(o) => o.validate(),
            BodyOp::Ruled(o) => o.validate(),
            BodyOp::ReplaceFace(o) => o.validate(),
            BodyOp::Sculpt(o) => o.validate(),
            BodyOp::CopyObject(o) => o.validate(),
            BodyOp::Import(o) => o.validate(),
            BodyOp::Prism(o) => o.validate(),
        }
    }

    /// Whether it makes a new body (New Solid, or an offset surface).
    pub fn creates_body(&self) -> bool {
        match &self.op {
            BodyOp::Thicken(t) => {
                t.output == ThickenOutput::Surface || t.operation == Operation::NewSolid
            }
            BodyOp::Patch(_)
            | BodyOp::Ruled(_)
            | BodyOp::Stitch(_)
            | BodyOp::CopyObject(_)
            | BodyOp::Import(_)
            | BodyOp::Prism(_) => true,
            BodyOp::Sculpt(s) => s.operation == super::surface_op::SculptOperation::NewSolid,
            BodyOp::DeleteFace(_)
            | BodyOp::MoveBodies(_)
            | BodyOp::Trim(_)
            | BodyOp::Extend(_)
            | BodyOp::ReplaceFace(_) => false,
        }
    }

    /// The numeric fields (parameters `d<N>` of the feature).
    pub fn fields(&self) -> Vec<(&'static str, Unit)> {
        match &self.op {
            BodyOp::Thicken(_) => vec![("distance", Unit::Mm)],
            BodyOp::Extend(_) | BodyOp::Ruled(_) => vec![("distance", Unit::Mm)],
            BodyOp::DeleteFace(_)
            | BodyOp::Stitch(_)
            | BodyOp::Patch(_)
            | BodyOp::Trim(_)
            | BodyOp::ReplaceFace(_)
            | BodyOp::Sculpt(_)
            | BodyOp::CopyObject(_)
            | BodyOp::Import(_)
            | BodyOp::Prism(_) => Vec::new(),
            BodyOp::MoveBodies(m) => m
                .steps
                .iter()
                .enumerate()
                .flat_map(|(k, s)| step_fields(k, s))
                .collect(),
        }
    }

    /// A field's value.
    pub fn value(&self, field: &str) -> Option<Real> {
        match (&self.op, field) {
            (BodyOp::Thicken(t), "distance") => Some(t.distance),
            (BodyOp::Extend(o), "distance") => Some(o.distance),
            (BodyOp::Ruled(o), "distance") => Some(o.distance),
            (BodyOp::MoveBodies(m), f) => step_value(&mut m.clone(), f).map(|v| *v),
            _ => None,
        }
    }

    /// Sets a field.
    pub fn set(&mut self, field: &str, v: Real) {
        match (&mut self.op, field) {
            (BodyOp::Thicken(t), "distance") => t.distance = v,
            (BodyOp::Extend(o), "distance") => o.distance = v,
            (BodyOp::Ruled(o), "distance") => o.distance = v,
            (BodyOp::MoveBodies(m), f) => {
                if let Some(x) = step_value(m, f) {
                    *x = v;
                }
            }
            _ => {}
        }
    }

    /// The browser's tooltip.
    pub fn describe(&self) -> String {
        match &self.op {
            BodyOp::Thicken(t) => {
                let what = match t.select {
                    ThickenSelect::Face => format!("{} face(s)", t.faces.len()),
                    ThickenSelect::Quilt => format!("{} quilt(s)", t.quilts.len()),
                };
                match t.output {
                    ThickenOutput::Surface => format!("Offset of {what} by {} mm", t.distance),
                    ThickenOutput::Solid => {
                        format!("Thicken {what} by {} mm ({:?})", t.distance, t.operation)
                    }
                }
            }
            BodyOp::DeleteFace(d) => match d.select {
                DeleteSelect::LumpOrVoid => format!("Delete {} lump(s) or void(s)", d.faces.len()),
                DeleteSelect::Faces if d.heal => {
                    format!("Delete {} face(s), healed", d.faces.len())
                }
                DeleteSelect::Faces => format!("Delete {} face(s) (the body opens)", d.faces.len()),
            },
            BodyOp::MoveBodies(m) => {
                let steps: Vec<&str> = m.steps.iter().map(MoveStep::label).collect();
                format!("Move {} body(ies): {}", m.bodies.len(), steps.join(", "))
            }
            BodyOp::Stitch(o) => format!("Stitch of {} surface(s)", o.surfaces.len()),
            BodyOp::Patch(o) => match &o.sketch {
                Some(s) => format!("Boundary Patch of a region of {s}"),
                None => format!("Boundary Patch of {} edge(s)", o.edges.len()),
            },
            BodyOp::Trim(o) => format!("Trim of {}", o.surface),
            BodyOp::Extend(o) => format!("Extend {} edge(s) by {} mm", o.edges.len(), o.distance),
            BodyOp::Ruled(o) => format!(
                "Ruled Surface ({:?}) of {} edge(s), {} mm",
                o.kind,
                o.edges.len(),
                o.distance
            ),
            BodyOp::ReplaceFace(o) => format!("Replace {} face(s)", o.faces.len()),
            BodyOp::Sculpt(o) => format!(
                "Sculpt ({:?}) with {} surface(s)",
                o.operation,
                o.surfaces.len()
            ),
            BodyOp::CopyObject(o) => o.describe(),
            BodyOp::Import(o) => o.describe(),
            BodyOp::Prism(_) => "Frame member".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::part::{Feature, PartDocument};

    fn volume(p: &PartDocument) -> Real {
        p.exact_bodies
            .0
            .iter()
            .filter(|s| !s.is_open())
            .filter_map(|s| s.volume_at(1e-3))
            .sum()
    }

    fn top(part: &PartDocument) -> FaceRef {
        let _ = part;
        FaceRef {
            face: "Box1:Top".into(),
            hint: [20.0, 15.0, 20.0],
        }
    }

    /// A 40 × 30 × 20 box and Thicken/Offset of its top face.
    fn thicken(f: impl FnOnce(&mut ThickenOp)) -> Result<PartDocument, String> {
        let mut part =
            PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).map_err(|e| e.to_string())?;
        let mut t = ThickenOp::new();
        t.faces = vec![top(&part)];
        f(&mut t);
        part.add_body_op(BodyOpFeature::new(BodyOp::Thicken(t)))
            .map_err(|e| e.to_string())?;
        Ok(part)
    }

    fn close(a: Real, b: Real) -> bool {
        (a - b).abs() < 1e-3 * b.abs().max(1.0)
    }

    /// Join, Cut, Intersect, Symmetric and New Solid give the volumes of
    /// the slab on the top face.
    #[test]
    fn operations_and_directions() {
        let cases: [(Operation, ThickenDirection, Real, Real, usize); 5] = [
            (
                Operation::Join,
                ThickenDirection::Direction1,
                5.0,
                40.0 * 30.0 * 25.0,
                1,
            ),
            (
                Operation::Cut,
                ThickenDirection::Direction2,
                5.0,
                40.0 * 30.0 * 15.0,
                1,
            ),
            (
                Operation::Intersect,
                ThickenDirection::Direction2,
                5.0,
                40.0 * 30.0 * 5.0,
                1,
            ),
            (
                Operation::Join,
                ThickenDirection::Symmetric,
                4.0,
                40.0 * 30.0 * 22.0,
                1,
            ),
            (
                Operation::NewSolid,
                ThickenDirection::Direction1,
                5.0,
                40.0 * 30.0 * 25.0,
                2,
            ),
        ];
        for (op, dir, d, want, solids) in cases {
            let part = thicken(|t| {
                t.operation = op;
                t.direction = dir;
                t.distance = d;
            })
            .unwrap_or_else(|e| panic!("{op:?} {dir:?}: {e}"));
            assert!(part.feature_health.is_empty(), "{:?}", part.feature_health);
            let v = volume(&part);
            assert!(close(v, want), "{op:?} {dir:?}: volume {v}, want {want}");
            let n = part.exact_bodies.0.iter().filter(|s| !s.is_open()).count();
            assert_eq!(n, solids, "{op:?}");
        }
    }

    /// The moved copy of the face is named after the feature; walls flush
    /// with the sides merge into them (the sides keep their names); the
    /// solid keeps its name. A New Solid shows its own walls.
    #[test]
    fn names() {
        let part = thicken(|t| t.distance = 5.0).unwrap_or_else(|e| panic!("{e}"));
        let names: Vec<String> = part
            .body
            .faces()
            .filter_map(|(_, f)| f.name.clone())
            .collect();
        assert!(
            names.iter().any(|n| n == "Thicken1:Offset:Box1:Top"),
            "{names:?}"
        );
        assert!(names.iter().any(|n| n == "Box1:Front"), "{names:?}");
        assert!(
            part.body
                .solids()
                .any(|(_, s)| s.name.as_deref() == Some("Box1"))
        );
        let apart =
            thicken(|t| t.operation = Operation::NewSolid).unwrap_or_else(|e| panic!("{e}"));
        let names: Vec<String> = apart
            .body
            .faces()
            .filter_map(|(_, f)| f.name.clone())
            .collect();
        assert!(
            names.iter().any(|n| n.starts_with("Thicken1:Wall:")),
            "{names:?}"
        );
        assert!(
            apart
                .body
                .solids()
                .any(|(_, s)| s.name.as_deref() == Some("Thicken1"))
        );
    }

    /// Output Surface: a surface body 3 mm above the top; the solid is
    /// unchanged. A second Thicken (Quilt) makes it a 2 mm plate.
    #[test]
    fn offset_surface_and_quilt() {
        let mut part = thicken(|t| {
            t.output = ThickenOutput::Surface;
            t.distance = 3.0;
        })
        .unwrap_or_else(|e| panic!("{e}"));
        assert!(close(volume(&part), 40.0 * 30.0 * 20.0));
        assert_eq!(
            part.exact_bodies.0.iter().filter(|s| s.is_open()).count(),
            1
        );
        let mut q = ThickenOp::new();
        q.select = ThickenSelect::Quilt;
        q.quilts = vec!["Thicken1".into()];
        q.operation = Operation::NewSolid;
        q.distance = 2.0;
        part.add_body_op(BodyOpFeature::new(BodyOp::Thicken(q)))
            .unwrap_or_else(|e| panic!("{e}"));
        let v = volume(&part);
        assert!(close(v, 40.0 * 30.0 * 22.0), "volume {v}");
    }

    /// Two faces at once (top and front), and the distance as a parameter.
    #[test]
    fn several_faces_and_the_parameter() {
        let mut part = thicken(|t| {
            t.faces.push(FaceRef {
                face: "Box1:Front".into(),
                hint: [20.0, 0.0, 10.0],
            });
            t.distance = 2.0;
            t.automatic_face_chain = false;
        })
        .unwrap_or_else(|e| panic!("{e}"));
        assert!(volume(&part) > 40.0 * 30.0 * 20.0 + 40.0 * 30.0 * 2.0);
        let d = part
            .parameter_table()
            .into_iter()
            .find(|r| r.consumed_by.contains("Thicken1"))
            .map(|r| r.name)
            .unwrap_or_default();
        assert!(!d.is_empty(), "no distance parameter");
        part.set_parameter_expr(&d, "4 mm")
            .unwrap_or_else(|e| panic!("{e}"));
        part.regenerate().unwrap_or_else(|e| panic!("{e}"));
        assert!(volume(&part) > 40.0 * 30.0 * 20.0 + 40.0 * 30.0 * 4.0);
    }

    /// Refusals change nothing: no face, a missing face, Surface with
    /// Symmetric, distance 0.
    #[test]
    fn refusals_leave_the_part() {
        let mut part =
            PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap_or_else(|e| panic!("{e}"));
        let before = part.features.len();
        let bad = [
            ThickenOp::new(),
            ThickenOp {
                faces: vec![FaceRef {
                    face: "Nope:Top".into(),
                    hint: [0.0; 3],
                }],
                ..ThickenOp::new()
            },
            ThickenOp {
                faces: vec![top(&part)],
                output: ThickenOutput::Surface,
                direction: ThickenDirection::Symmetric,
                ..ThickenOp::new()
            },
            ThickenOp {
                faces: vec![top(&part)],
                distance: 0.0,
                ..ThickenOp::new()
            },
        ];
        for t in bad {
            assert!(
                part.add_body_op(BodyOpFeature::new(BodyOp::Thicken(t.clone())))
                    .is_err(),
                "{t:?}"
            );
            assert_eq!(part.features.len(), before);
        }
        assert!(close(volume(&part), 40.0 * 30.0 * 20.0));
    }

    /// Save and load: the same feature and volume.
    #[test]
    fn save_and_load() {
        let part = thicken(|t| t.distance = 5.0).unwrap_or_else(|e| panic!("{e}"));
        let json = serde_json::to_string(&part).unwrap_or_default();
        let mut back: PartDocument = serde_json::from_str(&json).unwrap_or_else(|e| panic!("{e}"));
        back.regenerate().unwrap_or_else(|e| panic!("{e}"));
        assert!(matches!(back.features.last(), Some(Feature::BodyOp(_))));
        assert!(close(volume(&back), volume(&part)));
    }
}
