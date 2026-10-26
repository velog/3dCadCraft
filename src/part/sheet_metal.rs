//! Sheet metal parts (`docs/spec/5.6-sheet-metal.md`, stage S1): the
//! document's Sheet Metal Defaults (one rule: thickness, bend radius,
//! K-factor, reliefs, gap), the **Face** (a plate from a sketch profile)
//! and **Flange** (bends on sheet edges) features, and the flat pattern.
//! The sheet model itself is SolidCraft's, behind
//! `kernel::exact::sheet_metal`; regeneration keeps one model per sheet body
//! (`PartDocument::sheet_models`, runtime) next to the exact solids.

use serde::{Deserialize, Serialize};

use super::extrude::ProfileRef;
use crate::kernel::Real;
use crate::kernel::exact::sheet_metal as ksm;

/// Sheet Metal Defaults: the part's rule. `None` values follow the
/// thickness as Inventor's default rule does (`BendRadius = Thickness`…).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SheetMetalDefaults {
    /// Thickness (mm).
    pub thickness: Real,
    /// Inner bend radius (mm); `None`: the thickness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bend_radius: Option<Real>,
    /// K-factor of the linear unfold rule (0..1).
    #[serde(default = "k_default")]
    pub k_factor: Real,
    /// Bend relief width; `None`: the thickness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relief_width: Option<Real>,
    /// Bend relief depth; `None`: half the thickness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relief_depth: Option<Real>,
    /// Corner relief size; `None`: four times the thickness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corner_relief: Option<Real>,
    /// Gap between flanges meeting at a corner; `None`: the thickness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gap: Option<Real>,
    /// The relief where two bends meet at a corner.
    #[serde(default, skip_serializing_if = "CornerRelief::is_default")]
    pub corner_shape: CornerRelief,
}

/// Corner Relief Shape (Inventor's sheet rule; SolidWorks' Corner Relief
/// types): what is cut where two bends meet at a corner of the base.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CornerRelief {
    /// Round, the Corner Relief size across, centred on the corner.
    #[default]
    Round,
    /// Square, the Corner Relief size across.
    Square,
    /// Tear: no material removed; each bend tears straight to its panel.
    Tear,
    /// None: both bends end square at the corner (Trim to Bend).
    Trim,
}

impl CornerRelief {
    /// Every shape, in menu order.
    pub const ALL: [CornerRelief; 4] = [
        CornerRelief::Round,
        CornerRelief::Square,
        CornerRelief::Tear,
        CornerRelief::Trim,
    ];

    /// Label (Defaults window).
    pub fn label(self) -> &'static str {
        match self {
            CornerRelief::Round => "Round",
            CornerRelief::Square => "Square",
            CornerRelief::Tear => "Tear",
            CornerRelief::Trim => "Trim to Bend",
        }
    }

    fn is_default(&self) -> bool {
        *self == CornerRelief::Round
    }

    pub(super) fn kernel_pub(self) -> ksm::CornerShape {
        self.kernel()
    }

    fn kernel(self) -> ksm::CornerShape {
        match self {
            CornerRelief::Round => ksm::CornerShape::Round,
            CornerRelief::Square => ksm::CornerShape::Rectangular,
            CornerRelief::Tear => ksm::CornerShape::Tear,
            CornerRelief::Trim => ksm::CornerShape::Notch,
        }
    }
}

fn k_default() -> Real {
    0.44
}

impl Default for SheetMetalDefaults {
    fn default() -> Self {
        Self {
            thickness: 2.0,
            bend_radius: None,
            k_factor: k_default(),
            relief_width: None,
            relief_depth: None,
            corner_relief: None,
            gap: None,
            corner_shape: CornerRelief::Round,
        }
    }
}

impl SheetMetalDefaults {
    /// The values with their defaults filled in.
    pub fn values(&self) -> ksm::RuleValues {
        let t = self.thickness;
        ksm::RuleValues {
            thickness: t,
            k_factor: self.k_factor,
            bend_radius: self.bend_radius.unwrap_or(t),
            relief_width: self.relief_width.unwrap_or(t),
            relief_depth: self.relief_depth.unwrap_or(t * 0.5),
            corner_relief: self.corner_relief.unwrap_or(t * 4.0),
            gap: self.gap.unwrap_or(t),
            corner_shape: self.corner_shape.kernel(),
        }
    }

    /// Checks the rule (an error names what is wrong).
    pub fn validate(&self) -> Result<(), String> {
        let v = self.values();
        if !(v.thickness.is_finite() && v.thickness > 0.0) {
            return Err("the thickness must be positive".into());
        }
        if !(v.bend_radius.is_finite() && v.bend_radius > 0.0) {
            return Err("the bend radius must be positive".into());
        }
        if !(0.0..=1.0).contains(&v.k_factor) {
            return Err("the K-factor must be between 0 and 1".into());
        }
        Ok(())
    }
}

/// Face: a plate from closed profiles of a sketch, the rule's thickness
/// thick.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SheetFaceFeature {
    /// Immutable id (`Face1`): the sheet body's name and its faces' prefix.
    pub id: String,
    /// Display name.
    pub name: String,
    /// The sketch.
    pub sketch: String,
    /// Its profile(s) (the first makes the sheet).
    pub profiles: Vec<ProfileRef>,
    /// The thickness on the other side of the sketch plane.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub flip: bool,
}

/// Where a flange's bend lies against its edge (Inventor's Bend Position).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FlangePosition {
    /// Inside of the base face extents: the base keeps its size.
    #[default]
    Inside,
    /// Outside of the base face extents: the bend starts at the edge.
    Outside,
    /// The bend centred on the edge.
    Middle,
}

impl FlangePosition {
    /// Label (panel).
    pub fn label(self) -> &'static str {
        match self {
            FlangePosition::Inside => "Inside of base face extents",
            FlangePosition::Outside => "Outside of base face extents",
            FlangePosition::Middle => "Bend centred on the edge",
        }
    }

    /// The kernel's value.
    pub(super) fn kernel_pub(self) -> ksm::BendPosition {
        match self {
            FlangePosition::Inside => ksm::BendPosition::Inside,
            FlangePosition::Outside => ksm::BendPosition::Outside,
            FlangePosition::Middle => ksm::BendPosition::Middle,
        }
    }
}

/// A sheet edge by the sheet's persistent edge name and a point near the
/// pick (part coordinates).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SheetEdge {
    /// The sheet's name of the edge.
    pub name: String,
    /// Where it was picked.
    pub hint: [Real; 3],
}

/// A Corner Override of a Flange (Inventor: Relief Override per corner): the
/// corner between its flanges on edges `a` and `b` (indices into its edges).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CornerOverride {
    /// The first flange's edge index (the one that overlaps with Overlap).
    pub a: usize,
    /// The other's.
    pub b: usize,
    /// The seam.
    pub seam: SeamType,
    /// The gap (`None`: the feature's or the rule's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gap: Option<Real>,
    /// Overlap: share of the thickness past the other's inner face.
    #[serde(default = "ratio_default", skip_serializing_if = "is_one")]
    pub ratio: Real,
}

/// What a sheet metal Cut removes (Inventor's Cut extents).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CutExtent {
    /// Through the face the sketch lies on (Thickness).
    #[default]
    Thickness,
    /// `depth` into the material from that face.
    Distance,
    /// Through every wall along the sketch normal.
    ThroughAll,
}

impl CutExtent {
    /// Every extent, in menu order.
    pub const ALL: [CutExtent; 3] = [
        CutExtent::Thickness,
        CutExtent::Distance,
        CutExtent::ThroughAll,
    ];

    /// Label (panel).
    pub fn label(self) -> &'static str {
        match self {
            CutExtent::Thickness => "Thickness",
            CutExtent::Distance => "Distance",
            CutExtent::ThroughAll => "Through All",
        }
    }

    fn is_default(&self) -> bool {
        *self == CutExtent::Thickness
    }
}

/// Width Extents of a flange (Inventor: Edge / Width / Offset; SolidWorks:
/// Edit Flange Profile; Solid Edge: the flange's sides dragged): how much
/// of each picked edge the flange covers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FlangeWidth {
    /// The whole edge.
    #[default]
    Edge,
    /// `width` long, `offset1` from the edge's start (or centred).
    Width,
    /// Held back `offset1` from the start and `offset2` from the end.
    Offset,
}

impl FlangeWidth {
    /// Every kind, in menu order.
    pub const ALL: [FlangeWidth; 3] = [FlangeWidth::Edge, FlangeWidth::Width, FlangeWidth::Offset];

    /// Label (panel).
    pub fn label(self) -> &'static str {
        match self {
            FlangeWidth::Edge => "Edge",
            FlangeWidth::Width => "Width",
            FlangeWidth::Offset => "Offset",
        }
    }

    fn is_edge(&self) -> bool {
        *self == FlangeWidth::Edge
    }
}

fn is_zero(v: &Real) -> bool {
    *v == 0.0
}

/// Flange: bends on sheet edges, all with one height and angle.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SheetFlangeFeature {
    /// Immutable id (`Flange1`).
    pub id: String,
    /// Display name.
    pub name: String,
    /// The sheet body (its Face's id).
    pub body: String,
    /// The edges.
    pub edges: Vec<SheetEdge>,
    /// Height to the outer face, from the virtual sharp (mm).
    pub height: Real,
    /// Bend angle (radians, 0..π).
    pub angle: Real,
    /// Bend radius; `None`: the rule's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius: Option<Real>,
    /// Bend Position.
    #[serde(default)]
    pub position: FlangePosition,
    /// The flange to the other side.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub flip: bool,
    /// Width Extents.
    #[serde(default, skip_serializing_if = "FlangeWidth::is_edge")]
    pub extents: FlangeWidth,
    /// Offset from the edge's start (Width, Offset).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub offset1: Real,
    /// Offset from the edge's end (Offset).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub offset2: Real,
    /// The flange's width (Width).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub width: Real,
    /// Width centred on the edge (Width).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub centered: bool,
    /// Corner relief shape where two of its bends meet (`None`: the rule's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corner_shape: Option<CornerRelief>,
    /// Corner relief size (`None`: the rule's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corner_size: Option<Real>,
    /// Gap between its flanges at a corner (`None`: the rule's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corner_gap: Option<Real>,
    /// Corner Overrides: single corners between two of its flanges.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub seams: Vec<CornerOverride>,
    /// Equations of the fields (`height`, `angle`, `offset1`, `offset2`, `width`).
    #[serde(
        default,
        skip_serializing_if = "super::params::FeatureParams::is_empty"
    )]
    pub params: super::params::FeatureParams,
}

impl SheetFlangeFeature {
    /// Its numeric fields (parameters).
    pub fn fields(&self) -> Vec<(&'static str, super::expr::Unit)> {
        use super::expr::Unit;
        let mut v = vec![("height", Unit::Mm), ("angle", Unit::Deg)];
        match self.extents {
            FlangeWidth::Edge => {}
            FlangeWidth::Width => {
                v.push(("width", Unit::Mm));
                if !self.centered {
                    v.push(("offset1", Unit::Mm));
                }
            }
            FlangeWidth::Offset => v.extend([("offset1", Unit::Mm), ("offset2", Unit::Mm)]),
        }
        v
    }

    /// How far the flange stays off the start and end of an edge `len`
    /// long, or why it cannot (the error names the edge's length).
    pub fn spans(&self, len: Real) -> Result<(Real, Real), String> {
        let (a, b) = match self.extents {
            FlangeWidth::Edge => return Ok((0.0, 0.0)),
            FlangeWidth::Width if self.centered => {
                let rest = (len - self.width) * 0.5;
                (rest, rest)
            }
            FlangeWidth::Width => (self.offset1, len - self.offset1 - self.width),
            FlangeWidth::Offset => (self.offset1, self.offset2),
        };
        if self.extents == FlangeWidth::Width
            && self.width.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater)
        {
            return Err("the flange's width must be positive".into());
        }
        if !(a >= -1e-9 && b >= -1e-9 && a + b < len - 1e-6) {
            return Err(format!(
                "the width and offsets do not fit the edge ({len:.3} mm long)"
            ));
        }
        Ok((a.max(0.0), b.max(0.0)))
    }

    /// A field's value.
    pub fn value(&self, field: &str) -> Option<Real> {
        match field {
            "height" => Some(self.height),
            "angle" => Some(self.angle),
            "offset1" => Some(self.offset1),
            "offset2" => Some(self.offset2),
            "width" => Some(self.width),
            _ => None,
        }
    }

    /// Sets a field.
    pub fn set(&mut self, field: &str, v: Real) {
        match field {
            "height" => self.height = v,
            "angle" => self.angle = v,
            "offset1" => self.offset1 = v,
            "offset2" => self.offset2 = v,
            "width" => self.width = v,
            _ => {}
        }
    }

    /// A flange of `height` at 90° on `edges` of `body`.
    pub fn new(body: &str, edges: Vec<SheetEdge>, height: Real) -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            body: body.to_owned(),
            edges,
            height,
            angle: std::f64::consts::FRAC_PI_2,
            radius: None,
            position: FlangePosition::Inside,
            flip: false,
            extents: FlangeWidth::Edge,
            offset1: 0.0,
            offset2: 0.0,
            width: 0.0,
            centered: false,
            corner_shape: None,
            corner_size: None,
            corner_gap: None,
            seams: Vec::new(),
            params: Default::default(),
        }
    }
}

/// Convert to Sheet Metal of a solid (SolidWorks' Convert to Sheet Metal,
/// Solid Edge's Transform to Sheet Metal; Inventor converts a thin part by
/// its base face): the face that stays put and the sharp edges that become
/// bends. A shelled body keeps its wall thickness; any other body becomes the
/// outside of a sheet of the rule's thickness. Edges between faces that are
/// not picked are cut open (rips).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SheetConvertFeature {
    /// Immutable id (`Convert1`): the prefix of the sheet's face names.
    pub id: String,
    /// Display name.
    pub name: String,
    /// The solid converted (its persistent name; the sheet keeps it).
    pub body: String,
    /// The fixed face.
    pub face: super::shell::FaceRef,
    /// The bend edges.
    pub bends: Vec<super::blend::EdgeRef>,
}

/// Where a Fold's sketch line lies in the bend (Inventor's Fold Location).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FoldLocation {
    /// The line is the bend's centre line.
    #[default]
    Centerline,
    /// The line is where the bend starts (on the fixed side).
    Start,
    /// The line is where the bend ends.
    End,
    /// The line is the mould line (the outer faces' intersection).
    Mould,
}

impl FoldLocation {
    /// Every location, in menu order.
    pub const ALL: [FoldLocation; 4] = [
        FoldLocation::Centerline,
        FoldLocation::Start,
        FoldLocation::End,
        FoldLocation::Mould,
    ];

    /// Label (panel).
    pub fn label(self) -> &'static str {
        match self {
            FoldLocation::Centerline => "Centerline of Bend",
            FoldLocation::Start => "Start of Bend",
            FoldLocation::End => "End of Bend",
            FoldLocation::Mould => "Mould Line",
        }
    }

    pub(super) fn kernel(self) -> ksm::FoldLine {
        match self {
            FoldLocation::Centerline => ksm::FoldLine::Centerline,
            FoldLocation::Start => ksm::FoldLine::Start,
            FoldLocation::End => ksm::FoldLine::End,
            FoldLocation::Mould => ksm::FoldLine::Mould,
        }
    }
}

/// How Corner Seam closes a corner (Inventor's Seam types).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SeamType {
    /// Symmetric Gap: both flanges stop half the gap from the plane between them.
    #[default]
    SymmetricGap,
    /// Overlap: the first flange runs over the second one's edge.
    Overlap,
    /// Reverse Overlap: the second flange over the first.
    ReverseOverlap,
}

impl SeamType {
    /// Every type, in menu order.
    pub const ALL: [SeamType; 3] = [
        SeamType::SymmetricGap,
        SeamType::Overlap,
        SeamType::ReverseOverlap,
    ];

    /// Label (panel).
    pub fn label(self) -> &'static str {
        match self {
            SeamType::SymmetricGap => "Symmetric Gap",
            SeamType::Overlap => "Overlap",
            SeamType::ReverseOverlap => "Reverse Overlap",
        }
    }

    pub(super) fn kernel(self) -> ksm::Seam {
        match self {
            SeamType::SymmetricGap => ksm::Seam::Gap,
            SeamType::Overlap => ksm::Seam::Overlap,
            SeamType::ReverseOverlap => ksm::Seam::ReverseOverlap,
        }
    }
}

fn ratio_default() -> Real {
    1.0
}

fn is_one(v: &Real) -> bool {
    *v == 1.0
}

/// A corner between two flanges of a sheet (Corner Seam picks).
#[derive(Clone, Debug, PartialEq)]
pub struct SheetCorner {
    /// The sheet body.
    pub body: String,
    /// The first flange's tag (`Flange1.2`).
    pub a: String,
    /// The second's.
    pub b: String,
    /// The corner on the base.
    pub at: crate::kernel::Point3,
    /// The way the flanges rise.
    pub up: crate::kernel::Vector3,
}

/// The further sheet metal operations (stage S2), one feature type:
/// Hem, Fold, Contour Flange, Unfold / Refold and Cut.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum SheetOp {
    /// Hem: a 180° bend folded back at each edge.
    Hem {
        /// The sheet body.
        body: String,
        /// The edges.
        edges: Vec<SheetEdge>,
        /// Length to the hem's outer face (mm).
        length: Real,
        /// Gap between the hem and the sheet; `None`: half the thickness.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        gap: Option<Real>,
        /// Folded to the other face.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        flip: bool,
    },
    /// Fold: bends the sheet along a sketch line on one of its faces.
    Fold {
        /// The sheet body.
        body: String,
        /// The sketch with the line.
        sketch: String,
        /// The line (sketch entity key).
        line: String,
        /// Bend angle (radians).
        angle: Real,
        /// Bend radius; `None`: the rule's.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        radius: Option<Real>,
        /// Where the line lies in the bend.
        #[serde(default)]
        location: FoldLocation,
        /// The other way.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        flip: bool,
        /// The other side moves (by default the larger side stays).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        flip_side: bool,
    },
    /// Contour Flange: a new sheet from an open chain of sketch lines.
    Contour {
        /// The sketch.
        sketch: String,
        /// Width along the sketch normal (mm).
        width: Real,
        /// The material on the other side of the chain.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        flip: bool,
    },
    /// Unfold: the whole sheet flat; features after it work on the flat pattern.
    Unfold {
        /// The sheet body.
        body: String,
    },
    /// Refold: back to the folded sheet (with what was cut while flat).
    Refold {
        /// The sheet body.
        body: String,
    },
    /// Corner Seam: the corner between two flanges (by their tags) closed again.
    CornerSeam {
        /// The sheet body.
        body: String,
        /// The first flange (`Flange1.2`): the one that overlaps with Overlap.
        a: String,
        /// The second flange.
        b: String,
        /// The seam.
        #[serde(default)]
        seam: SeamType,
        /// The gap; `None`: the rule's.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        gap: Option<Real>,
        /// Overlap: how much of the thickness the flange runs past the other's
        /// inner face (1 = flush with its outer face).
        #[serde(default = "ratio_default", skip_serializing_if = "is_one")]
        ratio: Real,
    },
    /// Cut: closed sketch profiles cut through the face their sketch lies on.
    Cut {
        /// The sketch.
        sketch: String,
        /// The profiles.
        profiles: Vec<ProfileRef>,
        /// How deep.
        #[serde(default, skip_serializing_if = "CutExtent::is_default")]
        extent: CutExtent,
        /// Depth for Distance (mm).
        #[serde(default, skip_serializing_if = "is_zero")]
        depth: Real,
        /// Cut Across Bend: the profile wraps across the bends past its face.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        across: bool,
    },
}

/// A further sheet metal feature (`Hem1`, `Fold1`, `ContourFlange1`,
/// `Unfold1`, `Refold1`, `Cut1`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SheetOpFeature {
    /// Immutable id (faces and bends of the new panels are named after it).
    pub id: String,
    /// Display name.
    pub name: String,
    /// What it does.
    pub op: SheetOp,
    /// Equations of the fields (`length`, `gap`, `angle`, `width`).
    #[serde(
        default,
        skip_serializing_if = "super::params::FeatureParams::is_empty"
    )]
    pub params: super::params::FeatureParams,
}

impl SheetOpFeature {
    /// A feature of this kind (its id is given by `add_sheet_op`).
    pub fn new(op: SheetOp) -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            op,
            params: Default::default(),
        }
    }

    /// The command's name (the id's prefix).
    pub fn kind_name(&self) -> &'static str {
        match self.op {
            SheetOp::Hem { .. } => "Hem",
            SheetOp::Fold { .. } => "Fold",
            SheetOp::Contour { .. } => "ContourFlange",
            SheetOp::Unfold { .. } => "Unfold",
            SheetOp::Refold { .. } => "Refold",
            SheetOp::Cut { .. } => "Cut",
            SheetOp::CornerSeam { .. } => "CornerSeam",
        }
    }

    /// The sketch it uses.
    pub fn sketch(&self) -> Option<&str> {
        match &self.op {
            SheetOp::Fold { sketch, .. }
            | SheetOp::Contour { sketch, .. }
            | SheetOp::Cut { sketch, .. } => Some(sketch),
            _ => None,
        }
    }

    /// The body it creates (Contour Flange).
    pub fn creates_body(&self) -> bool {
        matches!(self.op, SheetOp::Contour { .. })
    }

    /// Its numeric fields (parameters).
    pub fn fields(&self) -> Vec<(&'static str, super::expr::Unit)> {
        use super::expr::Unit;
        match &self.op {
            SheetOp::Hem { gap, .. } => {
                let mut v = vec![("length", Unit::Mm)];
                if gap.is_some() {
                    v.push(("gap", Unit::Mm));
                }
                v
            }
            SheetOp::Fold { .. } => vec![("angle", Unit::Deg)],
            SheetOp::Contour { .. } => vec![("width", Unit::Mm)],
            SheetOp::Cut {
                extent: CutExtent::Distance,
                across: false,
                ..
            } => vec![("depth", Unit::Mm)],
            SheetOp::CornerSeam { gap, .. } => {
                if gap.is_some() {
                    vec![("gap", Unit::Mm)]
                } else {
                    Vec::new()
                }
            }
            _ => Vec::new(),
        }
    }

    /// A field's value.
    pub fn value(&self, field: &str) -> Option<Real> {
        match (&self.op, field) {
            (SheetOp::Hem { length, .. }, "length") => Some(*length),
            (SheetOp::Hem { gap, .. }, "gap") => *gap,
            (SheetOp::Fold { angle, .. }, "angle") => Some(*angle),
            (SheetOp::Contour { width, .. }, "width") => Some(*width),
            (SheetOp::Cut { depth, .. }, "depth") => Some(*depth),
            (SheetOp::CornerSeam { gap, .. }, "gap") => *gap,
            _ => None,
        }
    }

    /// Sets a field.
    pub fn set(&mut self, field: &str, v: Real) {
        match (&mut self.op, field) {
            (SheetOp::Hem { length, .. }, "length") => *length = v,
            (SheetOp::Hem { gap, .. }, "gap") => *gap = Some(v),
            (SheetOp::Fold { angle, .. }, "angle") => *angle = v,
            (SheetOp::Contour { width, .. }, "width") => *width = v,
            (SheetOp::Cut { depth, .. }, "depth") => *depth = v,
            (SheetOp::CornerSeam { gap, .. }, "gap") => *gap = Some(v),
            _ => {}
        }
    }

    /// One line for the browser's hover / Properties.
    pub fn describe(&self) -> String {
        match &self.op {
            SheetOp::Hem { edges, length, .. } => {
                format!("Hem on {} edge(s), {length:.2} mm", edges.len())
            }
            SheetOp::Fold { angle, line, .. } => {
                format!("Fold along {line}, {:.1}°", angle.to_degrees())
            }
            SheetOp::Contour { sketch, width, .. } => {
                format!("Contour Flange of {sketch}, {width:.2} mm wide")
            }
            SheetOp::Unfold { body } => format!("Unfold {body}"),
            SheetOp::Refold { body } => format!("Refold {body}"),
            SheetOp::Cut { sketch, .. } => format!("Cut of {sketch} through the sheet"),
            SheetOp::CornerSeam { a, b, seam, .. } => {
                format!("Corner Seam {a} / {b}: {}", seam.label())
            }
        }
    }
}

/// A sheet body's model with the face count the sheet steps left it with:
/// a Hole or Cut on it later is not in the model yet (stage S2), so a
/// further flange would drop it — that is refused.
#[derive(Clone, Debug)]
pub struct SheetState {
    /// The model.
    pub model: ksm::SheetModel,
    /// Faces of the solid when the last sheet step made it.
    pub faces: usize,
}

/// The flat pattern of a sheet body.
#[derive(Clone, Debug)]
pub struct FlatPattern {
    /// The sheet body's name.
    pub body: String,
    /// Length and width of the flat (mm).
    pub size: (Real, Real),
    /// Its bend lines.
    pub bends: Vec<ksm::BendLine>,
    /// The flat solid (for the view).
    pub solid: crate::kernel::exact::ExactSolid,
}

/// A minimal DXF (R12 entities: LINE, POLYLINE) of a flat pattern.
fn dxf(loops: &[Vec<[Real; 2]>], bends: &[ksm::BendLine]) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    let mut put = |code: i32, v: &str| {
        let _ = writeln!(s, "{code}\n{v}");
    };
    put(0, "SECTION");
    put(2, "TABLES");
    put(0, "TABLE");
    put(2, "LAYER");
    for (name, colour) in [("OUTER", "7"), ("BEND_UP", "3"), ("BEND_DOWN", "1")] {
        put(0, "LAYER");
        put(2, name);
        put(70, "0");
        put(62, colour);
        put(6, "CONTINUOUS");
    }
    put(0, "ENDTAB");
    put(0, "ENDSEC");
    put(0, "SECTION");
    put(2, "ENTITIES");
    let num = |v: Real| format!("{v:.6}");
    for lp in loops {
        put(0, "POLYLINE");
        put(8, "OUTER");
        put(66, "1");
        put(70, "1");
        for p in lp {
            put(0, "VERTEX");
            put(8, "OUTER");
            put(10, &num(p[0]));
            put(20, &num(p[1]));
        }
        put(0, "SEQEND");
    }
    for b in bends {
        put(0, "LINE");
        put(8, if b.up { "BEND_UP" } else { "BEND_DOWN" });
        put(10, &num(b.start[0]));
        put(20, &num(b.start[1]));
        put(11, &num(b.end[0]));
        put(21, &num(b.end[1]));
    }
    put(0, "ENDSEC");
    put(0, "EOF");
    s
}

/// The rule of a part: its defaults, or Inventor's default rule.
pub fn rule_of(part: &super::PartDocument) -> SheetMetalDefaults {
    part.sheet_metal.clone().unwrap_or_default()
}

impl super::PartDocument {
    /// Makes the part a sheet metal part (Convert to Sheet Metal / a new
    /// Sheet Metal part): its rule is `rule` (Inventor's default if `None`).
    pub fn make_sheet_metal(&mut self, rule: Option<SheetMetalDefaults>) {
        self.sheet_metal = Some(rule.unwrap_or_default());
    }

    /// **Face**: a plate from the first default profile of sketch
    /// `sketch` (id `Face<N>`). On failure nothing changes.
    pub fn add_sheet_face(
        &mut self,
        sketch: usize,
        profiles: Vec<ProfileRef>,
        flip: bool,
    ) -> Result<String, super::PartError> {
        if self.sheet_metal.is_none() {
            self.make_sheet_metal(None);
        }
        let ps = self
            .sketches
            .get(sketch)
            .ok_or_else(|| super::PartError::Feature("no such sketch".into()))?;
        let id = self.next_feature_id("Face");
        let def = SheetFaceFeature {
            id: id.clone(),
            name: id.clone(),
            sketch: ps.id.clone(),
            profiles,
            flip,
        };
        self.insert_feature_checked(super::Feature::SheetFace(def))?;
        Ok(id)
    }

    /// **Convert to Sheet Metal** of a solid (id `Convert<N>`); makes the
    /// part a sheet metal part first. On failure nothing changes.
    pub fn add_sheet_convert(
        &mut self,
        mut def: SheetConvertFeature,
    ) -> Result<String, super::PartError> {
        let had = self.sheet_metal.clone();
        if self.sheet_metal.is_none() {
            self.make_sheet_metal(None);
        }
        let id = self.next_feature_id("Convert");
        (def.id, def.name) = (id.clone(), id.clone());
        match self.insert_feature_checked(super::Feature::SheetConvert(def)) {
            Ok(_) => Ok(id),
            Err(e) => {
                self.sheet_metal = had;
                Err(e)
            }
        }
    }

    /// **Hem / Fold / Contour Flange / Unfold / Refold / Cut** (id
    /// `<Kind><N>`). On failure nothing changes.
    pub fn add_sheet_op(&mut self, mut def: SheetOpFeature) -> Result<String, super::PartError> {
        if self.sheet_metal.is_none() {
            self.make_sheet_metal(None);
        }
        let id = self.next_feature_id(def.kind_name());
        (def.id, def.name) = (id.clone(), id.clone());
        self.insert_feature_checked(super::Feature::SheetOp(def))?;
        Ok(id)
    }

    /// The sheet body the edge or point `at` is on, if one.
    pub fn sheet_body_at(&self, at: crate::kernel::Point3) -> Option<String> {
        self.sheet_edge_at(at).map(|(b, _)| b)
    }

    /// The corners between flanges of every sheet (Corner Seam).
    pub fn sheet_corners(&self) -> Vec<SheetCorner> {
        let mut v: Vec<SheetCorner> = self
            .sheet_models
            .iter()
            .flat_map(|(body, st)| {
                ksm::corners(&st.model)
                    .into_iter()
                    .map(move |c| SheetCorner {
                        body: body.clone(),
                        a: c.a,
                        b: c.b,
                        at: c.at,
                        up: c.up,
                    })
            })
            .collect();
        v.sort_by(|x, y| (&x.body, &x.a, &x.b).cmp(&(&y.body, &y.a, &y.b)));
        v
    }

    /// The corner nearest `at` (part coordinates): distance to the corner's
    /// edge, from the base up the flanges' height.
    pub fn sheet_corner_near(&self, at: crate::kernel::Point3, reach: Real) -> Option<SheetCorner> {
        self.sheet_corners()
            .into_iter()
            .map(|c| {
                let h = (at - c.at).dot(&c.up).clamp(0.0, reach);
                ((at - (c.at + c.up * h)).norm(), c)
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, c)| c)
    }

    /// Whether the sheet body `body` is unfolded now.
    pub fn sheet_is_flat(&self, body: &str) -> bool {
        self.sheet_models
            .get(body)
            .is_some_and(|st| ksm::is_flat(&st.model))
    }

    /// The flat pattern of `body` as DXF text (outline and cut-outs on layer
    /// `OUTER`, bend lines on `BEND_UP` / `BEND_DOWN`).
    pub fn flat_pattern_dxf(&self, body: &str) -> Result<String, String> {
        let st = self
            .sheet_models
            .get(body)
            .ok_or_else(|| format!("{body} is not a sheet metal body"))?;
        let loops = ksm::flat_loops(&st.model)?;
        let bends = ksm::bends(&st.model);
        Ok(dxf(&loops, &bends))
    }

    /// **Flange** (id `Flange<N>`). On failure nothing changes.
    pub fn add_sheet_flange(
        &mut self,
        mut def: SheetFlangeFeature,
    ) -> Result<String, super::PartError> {
        let id = self.next_feature_id("Flange");
        (def.id, def.name) = (id.clone(), id.clone());
        self.insert_feature_checked(super::Feature::SheetFlange(def))?;
        Ok(id)
    }

    /// The sheet edge nearest `at` (part coordinates): (sheet body, edge
    /// name) — what a Flange stores.
    pub fn sheet_edge_at(&self, at: crate::kernel::Point3) -> Option<(String, SheetEdge)> {
        self.sheet_models
            .iter()
            .filter_map(|(body, st)| {
                let name = ksm::edge_name(&st.model, at)?;
                let p = ksm::edge_point(&st.model, &name, at)?;
                Some(((p - at).norm(), body.clone(), name))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, body, name)| {
                (
                    body,
                    SheetEdge {
                        name,
                        hint: [at.x, at.y, at.z],
                    },
                )
            })
    }

    /// The ends of a sheet edge (start, end: the order Width Extents count
    /// them in), for the drag handles of Flange.
    pub fn sheet_edge_ends(
        &self,
        body: &str,
        e: &SheetEdge,
    ) -> Option<(crate::kernel::Point3, crate::kernel::Point3)> {
        let st = self.sheet_models.get(body)?;
        let at = ksm::edge_point(&st.model, &e.name, crate::kernel::Point3::from(e.hint))?;
        ksm::edge_ends(&st.model, at).map(|(a, b, _)| (a, b))
    }

    /// A sheet edge's frame for the manipulators: (start, end, the normal of
    /// the face it was picked on, the direction out of its panel).
    pub fn sheet_edge_frame(
        &self,
        body: &str,
        e: &SheetEdge,
    ) -> Option<(
        crate::kernel::Point3,
        crate::kernel::Point3,
        crate::kernel::Vector3,
        crate::kernel::Vector3,
    )> {
        let st = self.sheet_models.get(body)?;
        let at = ksm::edge_point(&st.model, &e.name, crate::kernel::Point3::from(e.hint))?;
        ksm::edge_frame(&st.model, at)
    }

    /// The flat patterns of the sheet bodies (Create Flat Pattern).
    pub fn flat_patterns(&self) -> Vec<FlatPattern> {
        let mut v: Vec<FlatPattern> = self
            .sheet_models
            .iter()
            .filter_map(|(body, st)| {
                Some(FlatPattern {
                    body: body.clone(),
                    size: ksm::flat_size(&st.model).ok()?,
                    bends: ksm::bends(&st.model),
                    solid: ksm::flat_solid(&st.model, body).ok()?,
                })
            })
            .collect();
        v.sort_by(|a, b| a.body.cmp(&b.body));
        v
    }
}

#[cfg(test)]
mod tests {
    use crate::kernel::{Point3, Real};
    use crate::part::{OriginPlane, PartDocument, SketchOptions, SketchPlacement};
    use crate::sketch::Point2;

    fn volume(p: &PartDocument) -> Real {
        p.exact_bodies
            .0
            .iter()
            .filter_map(|s| s.volume_at(1e-3))
            .sum()
    }

    /// A 100 × 60 plate 2 thick (Face), a 25 mm flange on its front edge;
    /// the flat pattern is longer by the developed flange; save / load
    /// keeps both; a missing edge makes the flange sick.
    #[test]
    fn face_flange_and_flat_pattern_in_a_part() {
        let mut p = PartDocument::default();
        p.make_sheet_metal(None);
        let o = SketchOptions {
            auto_project_origin: false,
            ..Default::default()
        };
        let i = p
            .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &o)
            .unwrap();
        p.sketches[i]
            .sketch
            .add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(100.0, 60.0))
            .unwrap();
        let r = p.sketches[i].sketch.regions();
        let prof = vec![crate::part::ProfileRef::of(&r[0], r[0].interior_point())];
        let face = p.add_sheet_face(i, prof, false).unwrap();
        assert!(p.exact);
        assert!((volume(&p) - 12_000.0).abs() < 1.0, "{}", volume(&p));
        let (body, edge) = p
            .sheet_edge_at(Point3::new(50.0, 0.0, 2.0))
            .expect("an edge");
        assert_eq!(body, face);
        let fl = super::SheetFlangeFeature::new(&body, vec![edge], 25.0);
        p.add_sheet_flange(fl).unwrap();
        assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
        let flats = p.flat_patterns();
        assert_eq!(flats.len(), 1);
        let (a, b) = flats[0].size;
        let ba = std::f64::consts::FRAC_PI_2 * (2.0 + 0.44 * 2.0);
        let want = 60.0 - 4.0 + ba + (25.0 - 4.0);
        assert!((a.min(b) - want).abs() < 0.05, "{a} × {b}, want {want}");
        assert_eq!(flats[0].bends.len(), 1);
        // The sheet's faces are named.
        assert!(p.body.faces().all(|(_, f)| f.name.is_some()));
        // Round trip through JSON (the file's part document).
        let json = serde_json::to_string(&p).unwrap();
        let mut q: PartDocument = serde_json::from_str(&json).unwrap();
        q.regenerate().unwrap();
        assert!((volume(&q) - volume(&p)).abs() < 1e-6 * volume(&p));
        assert_eq!(q.flat_patterns()[0].size, flats[0].size);
        // A thicker rule: everything follows.
        q.sheet_metal.as_mut().unwrap().thickness = 3.0;
        q.regenerate().unwrap();
        assert!(q.feature_health.is_empty(), "{:?}", q.feature_health);
        assert!(volume(&q) > volume(&p) * 1.4);
    }
}
