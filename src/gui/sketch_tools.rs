//! Interactive sketch tools (Inventor's Sketch tab: Create, Constrain,
//! Format), as a small state machine per tool.
//!
//! Every tool follows Inventor's input sequence; what a click *means*
//! depends on what is under the cursor ([`Snap`]):
//!
//! - **Line** — click, click, … draws a chain sharing endpoints. Pressing on
//!   the end of a line/arc and *dragging* creates an arc instead: tangent if
//!   the drag starts along the curve, perpendicular if it starts across it.
//!   Near-horizontal/vertical segments get H/V constraints (Ctrl suppresses
//!   inference). Esc or right-click ends the chain.
//! - **Circle** — Center Point (centre, radius point; a radius point on a
//!   curve makes the circle tangent to it) and Tangent (three lines).
//! - **Arc** — Three Point (ends, then a point setting side and radius),
//!   Tangent (from a curve end), Center Point (centre, start, end).
//! - **Rectangle / Slot / Polygon** — via the builders in `sketch::shapes`,
//!   which also create Inventor's constraints and construction geometry.
//! - **Spline** — control vertices; Enter or double-click finishes.
//! - **Constraints** — pick the geometry in Inventor's order; a constraint
//!   that would over-constrain the sketch is refused (nothing changes).
//! - **Dimension** — what is dimensioned follows the selection (circle →
//!   diameter, arc → radius, line or two points → distance, point + line →
//!   distance, two lines → distance if parallel else angle) and, for
//!   distances, the placement click: above/below → horizontal, left/right →
//!   vertical, elsewhere → aligned. Over-constraining offers a driven
//!   (reference) dimension, shown in parentheses.
//!
//! Snapping to the sketch's own points *shares* them (so chains and tangent
//! arcs are truly connected); snapping to projected reference points or onto
//! curves adds Coincident constraints instead, so the user geometry survives
//! when the projections are rebuilt (Redefine).

use std::f64::consts::PI;

use crate::kernel::Real;
use crate::part::PartSketch;
use crate::sketch::{
    ChamferKind, Constraint, Entity, EntityId, PatternDirection, PatternId, PatternKind, Point2,
    PointId, Sketch, SketchError, SolveStatus, Style, Vector2,
};

/// Geometric constraint tools of the Constrain panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstraintTool {
    Coincident,
    Collinear,
    Concentric,
    Fix,
    Parallel,
    Perpendicular,
    Horizontal,
    Vertical,
    Tangent,
    Symmetric,
    Equal,
}

impl ConstraintTool {
    /// All tools in ribbon order.
    pub const ALL: [ConstraintTool; 11] = [
        ConstraintTool::Coincident,
        ConstraintTool::Collinear,
        ConstraintTool::Concentric,
        ConstraintTool::Fix,
        ConstraintTool::Parallel,
        ConstraintTool::Perpendicular,
        ConstraintTool::Horizontal,
        ConstraintTool::Vertical,
        ConstraintTool::Tangent,
        ConstraintTool::Symmetric,
        ConstraintTool::Equal,
    ];

    /// Ribbon label (Inventor's name).
    pub fn label(self) -> &'static str {
        match self {
            ConstraintTool::Coincident => "Coincident",
            ConstraintTool::Collinear => "Collinear",
            ConstraintTool::Concentric => "Concentric",
            ConstraintTool::Fix => "Fix",
            ConstraintTool::Parallel => "Parallel",
            ConstraintTool::Perpendicular => "Perpendicular",
            ConstraintTool::Horizontal => "Horizontal",
            ConstraintTool::Vertical => "Vertical",
            ConstraintTool::Tangent => "Tangent",
            ConstraintTool::Symmetric => "Symmetric",
            ConstraintTool::Equal => "Equal",
        }
    }

    pub fn prompt(self, step: usize) -> &'static str {
        use ConstraintTool::*;
        match (self, step) {
            (Coincident, 0) => "Coincident: select a point",
            (Coincident, _) => "Coincident: select a point or curve to constrain it to",
            (Collinear, 0) | (Parallel, 0) | (Perpendicular, 0) => "Select the first line",
            (Collinear, _) | (Parallel, _) | (Perpendicular, _) => "Select the second line",
            (Concentric, 0) => "Concentric: select the first circle or arc",
            (Concentric, _) => "Concentric: select the second circle or arc",
            (Fix, _) => "Fix: select a point or curve",
            (Horizontal, 0) | (Vertical, 0) => "Select a line, or a first point",
            (Horizontal, _) | (Vertical, _) => "Select the second point",
            (Tangent, 0) => "Tangent: select the first curve",
            (Tangent, _) => "Tangent: select the second curve",
            (Symmetric, 0) => "Symmetric: select the first point or curve",
            (Symmetric, 1) => "Symmetric: select the second point or curve",
            (Symmetric, _) => "Symmetric: select the symmetry line",
            (Equal, 0) => "Equal: select a line, circle or arc",
            (Equal, _) => "Equal: select another of the same kind",
        }
    }
}

/// The active sketch tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    /// Select / drag (no tool).
    Select,
    Line,
    CircleCenter,
    CircleTangent,
    ArcThreePoint,
    ArcTangent,
    ArcCenter,
    RectTwoPoint,
    RectThreePoint,
    RectCenter,
    RectCenterThreePoint,
    SlotCenterToCenter,
    SlotOverall,
    SlotCenterPoint,
    SlotArcThreePoint,
    SlotArcCenter,
    Polygon,
    SplineCv,
    /// Spline through its points (Inventor: Spline Interpolation).
    SplineInterp,
    Point,
    Fillet,
    Chamfer,
    Constrain(ConstraintTool),
    Dimension,
    Trim,
    Extend,
    Split,
    Offset,
    Move,
    Copy,
    Rotate,
    Mirror,
    PatternRect,
    PatternCircular,
    /// Copies of the selection at points (Inventor's Sketch Driven Pattern).
    PatternPoints,
    /// Copies of the selection along a curve.
    PatternPath,
    Scale,
    Stretch,
}

impl Tool {
    /// Display name (Inventor's).
    pub fn label(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Line => "Line",
            Tool::CircleCenter => "Center Point Circle",
            Tool::CircleTangent => "Tangent Circle",
            Tool::ArcThreePoint => "Three Point Arc",
            Tool::ArcTangent => "Tangent Arc",
            Tool::ArcCenter => "Center Point Arc",
            Tool::RectTwoPoint => "Two Point Rectangle",
            Tool::RectThreePoint => "Three Point Rectangle",
            Tool::RectCenter => "Two Point Center Rectangle",
            Tool::RectCenterThreePoint => "Three Point Center Rectangle",
            Tool::SlotCenterToCenter => "Center to Center Slot",
            Tool::SlotOverall => "Overall Slot",
            Tool::SlotCenterPoint => "Center Point Slot",
            Tool::SlotArcThreePoint => "Three Point Arc Slot",
            Tool::SlotArcCenter => "Center Point Arc Slot",
            Tool::Polygon => "Polygon",
            Tool::SplineCv => "Control Vertex Spline",
            Tool::SplineInterp => "Interpolation Spline",
            Tool::Point => "Point",
            Tool::Fillet => "Fillet",
            Tool::Chamfer => "Chamfer",
            Tool::Constrain(c) => c.label(),
            Tool::Dimension => "Dimension",
            Tool::Trim => "Trim",
            Tool::Extend => "Extend",
            Tool::Split => "Split",
            Tool::Offset => "Offset",
            Tool::Move => "Move",
            Tool::Copy => "Copy",
            Tool::Rotate => "Rotate",
            Tool::Mirror => "Mirror",
            Tool::PatternRect => "Rectangular Pattern",
            Tool::PatternCircular => "Circular Pattern",
            Tool::PatternPoints => "Pattern at Points",
            Tool::PatternPath => "Pattern along Path",
            Tool::Scale => "Scale",
            Tool::Stretch => "Stretch",
        }
    }

    /// Clicks needed before the geometry is created (`None` = open-ended).
    fn clicks(self) -> Option<usize> {
        Some(match self {
            Tool::Point => 1,
            Tool::Line
            | Tool::CircleCenter
            | Tool::ArcTangent
            | Tool::RectTwoPoint
            | Tool::RectCenter
            | Tool::Polygon => 2,
            Tool::CircleTangent
            | Tool::ArcThreePoint
            | Tool::ArcCenter
            | Tool::RectThreePoint
            | Tool::RectCenterThreePoint
            | Tool::SlotCenterToCenter
            | Tool::SlotOverall
            | Tool::SlotCenterPoint => 3,
            Tool::SlotArcThreePoint | Tool::SlotArcCenter => 4,
            Tool::SplineCv
            | Tool::SplineInterp
            | Tool::Fillet
            | Tool::Chamfer
            | Tool::Select
            | Tool::Constrain(_)
            | Tool::Dimension
            | Tool::Trim
            | Tool::Extend
            | Tool::Split
            | Tool::Offset
            | Tool::Move
            | Tool::Copy
            | Tool::Rotate
            | Tool::Mirror
            | Tool::PatternRect
            | Tool::PatternCircular
            | Tool::PatternPoints
            | Tool::PatternPath
            | Tool::Scale
            | Tool::Stretch => return None,
        })
    }
}

/// What the cursor snapped to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Snap {
    None,
    Point(PointId),
    OnEntity(EntityId),
    /// The middle of a line (Inventor's midpoint snap → Midpoint constraint).
    Midpoint(EntityId),
    /// The point of a circle/arc where a line from the previous point
    /// touches it (Line tool: PointOnCircle + Tangent; SolidWorks' tangent
    /// snap, AutoCAD's TAN).
    Tangent(EntityId),
}

/// A line started on a circle/arc becomes tangent to it when it leaves
/// within this angle of the tangent there (sin 8°).
const START_TANGENT_SIN: Real = 0.139;

/// Tangent snap radius (px on screen) around the touching point.
pub const TANGENT_PX: Real = 12.0;

/// Where lines from `from` touch the circle (`center`, `r`): two points, or
/// none when `from` is not outside the circle.
pub fn tangent_points(from: Point2, center: Point2, r: Real) -> Vec<Point2> {
    let v = from - center;
    let d = v.norm();
    if d <= r * (1.0 + 1e-9) || r <= 0.0 {
        return Vec::new();
    }
    let beta = (r / d).acos();
    let u = v / d;
    [beta, -beta]
        .iter()
        .map(|&b| {
            let (sn, cs) = b.sin_cos();
            center + Vector2::new(u.x * cs - u.y * sn, u.x * sn + u.y * cs) * r
        })
        .collect()
}

/// Whether `q` (on the circle of arc `e`) lies within the arc's span; any
/// point for a full circle.
fn on_arc_span(sk: &Sketch, e: EntityId, q: Point2) -> bool {
    match sk.entity(e) {
        Some(&Entity::Arc { center, start, end }) => {
            let pos = |p: PointId| sk.point(p).map(|x| x.position);
            let (Some(c), Some(a), Some(b)) = (pos(center), pos(start), pos(end)) else {
                return false;
            };
            let ang = |p: Point2| (p.y - c.y).atan2(p.x - c.x);
            let span = (ang(b) - ang(a)).rem_euclid(std::f64::consts::TAU);
            (ang(q) - ang(a)).rem_euclid(std::f64::consts::TAU) <= span + 1e-9
        }
        Some(Entity::Circle { .. }) => true,
        _ => false,
    }
}

/// Point alignment radius (px on screen).
pub const ALIGN_PX: Real = 6.0;

/// Inferred alignment of a segment (Inventor's constraint inference).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Infer {
    Horizontal,
    Vertical,
    /// Perpendicular / parallel to the previous line of the chain.
    Perpendicular(EntityId),
    Parallel(EntityId),
}

/// One input click (or the live cursor).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pick {
    /// Position in sketch coordinates (snapped).
    pub pos: Point2,
    /// What it snapped to.
    pub snap: Snap,
    /// Alignment inferred relative to the previous pick.
    pub infer: Option<Infer>,
    /// Point alignment: existing points this pick shares x / y with.
    pub align: Align,
}

/// Point alignment (Inventor's *Point alignment* inference): the pick lies
/// on the vertical (`x`) and/or horizontal (`y`) through existing points.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Align {
    /// Same x as this point (vertical dotted line).
    pub x: Option<PointId>,
    /// Same y as this point (horizontal dotted line).
    pub y: Option<PointId>,
}

/// A dimension waiting for its value in the Edit Dimension box.
#[derive(Clone, Debug)]
pub struct DimEdit {
    pub constraint: crate::sketch::ConstraintId,
    /// Text being edited (degrees for angles).
    pub text: String,
    pub is_angle: bool,
    /// Keyboard focus was given to the text field.
    pub focused: bool,
}

/// A dimension that would over-constrain; the user may accept it as driven.
#[derive(Clone, Debug)]
pub struct OverPrompt {
    pub constraint: Constraint,
    /// Where the text was placed.
    pub place: Point2,
}

/// Something drawn as a preview (rubber band) in sketch coordinates.
pub struct PreviewLine {
    pub pts: Vec<Point2>,
    pub dashed: bool,
    /// Drawn in the warning colour (geometry about to be modified).
    pub warn: bool,
}

/// Result of a click: the model changed (with an Undo label), or an error.
pub type ClickResult = Result<Option<String>, String>;

/// Which constraints are inferred while sketching and whether they are
/// created (Inventor: Constraint Settings → Inference).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InferenceSettings {
    /// Infer constraints at all (glyphs at the cursor).
    pub infer: bool,
    /// Create the inferred constraints (only with `infer`).
    pub persist: bool,
    /// Align new points with existing ones (dotted lines).
    pub point_alignment: bool,
    pub horizontal: bool,
    pub vertical: bool,
    pub parallel: bool,
    pub perpendicular: bool,
    pub midpoint: bool,
    pub on_curve: bool,
    /// Snap to and share existing points.
    pub coincident: bool,
}

impl Default for InferenceSettings {
    fn default() -> Self {
        Self {
            infer: true,
            persist: true,
            point_alignment: true,
            horizontal: true,
            vertical: true,
            parallel: true,
            perpendicular: true,
            midpoint: true,
            on_curve: true,
            coincident: true,
        }
    }
}

/// Sizing mode of the 2D Chamfer (Inventor's dialog buttons).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChamferMode {
    Equal,
    TwoDistances,
    DistanceAngle,
}

/// Tool state of the sketch in edit mode.
pub struct SketchTools {
    pub tool: Tool,
    picks: Vec<Pick>,
    /// Polygon options (Inventor's Polygon dialog).
    pub polygon_sides: usize,
    pub polygon_inscribed: bool,
    /// New geometry is created as construction (Format → Construction).
    pub construction: bool,
    /// Selected entities (Select tool, and the input of Move/Copy/Rotate/Mirror).
    pub selected: Vec<EntityId>,
    /// Selected constraints (glyphs or dimensions picked in the window).
    pub selected_constraints: Vec<crate::sketch::ConstraintId>,
    pub dim_edit: Option<DimEdit>,
    pub over_prompt: Option<OverPrompt>,
    /// Offset options (Inventor: right-click → Loop Select / Constrain Offset).
    pub offset_loop: bool,
    pub offset_constrain: bool,
    /// Shift held: Trim ↔ Extend.
    pub shift: bool,
    /// 2D Fillet radius (mm) and the first arc of this command with that
    /// radius: later fillets get Equal to it instead of their own dimension.
    pub fillet_radius: Real,
    fillet_first: Option<(EntityId, Real)>,
    /// First equal-distance chamfer leg of this command (later ones get Equal).
    chamfer_first: Option<(EntityId, Real)>,
    /// Move: keep the original (Inventor's Copy option of Move).
    pub move_copy: bool,
    /// Constraint inference (Inventor's Constraint Settings → Inference).
    pub inference: InferenceSettings,
    /// Rectangular pattern: spacing is the overall length (Fitted).
    pub pattern_fitted: [bool; 2],
    /// Circular pattern: the angle is the overall angle (Fitted), else the
    /// angle between neighbours.
    pub circular_fitted: bool,
    /// 2D Chamfer options: mode, distances (mm), angle (degrees).
    pub chamfer_mode: ChamferMode,
    pub chamfer_d1: Real,
    pub chamfer_d2: Real,
    pub chamfer_angle_deg: Real,
    /// Rectangular pattern (Inventor's dialog): per direction count
    /// (with the original), spacing (mm) and flip.
    pub pattern_count: [usize; 2],
    pub pattern_spacing: [Real; 2],
    /// Equations typed into the spacing fields (parameter names): they stay
    /// on the pattern's dimensions after OK.
    pub pattern_spacing_eq: [Option<String>; 2],
    /// The same for the circular pattern's angle.
    pub circular_angle_eq: Option<String>,
    /// Equations of the counts (direction 1 / 2; circular uses the first).
    pub pattern_count_eq: [Option<String>; 2],
    pub pattern_flip: [bool; 2],
    /// Circular pattern: count (with the original), total angle (degrees),
    /// flip (clockwise).
    pub circular_count: usize,
    pub circular_angle_deg: Real,
    pub circular_flip: bool,
    /// Circular pattern centred by radius and angle from the selection
    /// (SolidWorks' Radius / Arc angle): mm and degrees.
    pub circular_radius: Real,
    pub circular_ref_deg: Real,
    /// Pattern along a path: count (with the original) and chord (mm).
    pub path_count: usize,
    pub path_spacing: Real,
    /// Edit Pattern: clicks on copies toggle "skip" (suppress).
    pub skip_pick: bool,
    /// The open dialog of a pattern command.
    pub pattern_dialog: Option<super::pattern_dialog::PatternDialog>,
    /// Scale factor of the Scale tool.
    pub scale_factor: Real,
    /// Edit Pattern in progress: the pattern and its definition before
    /// (the dialog fields above hold the new values).
    pub edit_pattern: Option<(PatternId, PatternKind)>,
    /// Move/Copy/Rotate/Mirror: selection finished, now picking points.
    transform_ready: bool,
}

impl Default for SketchTools {
    fn default() -> Self {
        Self {
            tool: Tool::Select,
            picks: Vec::new(),
            polygon_sides: 6,
            polygon_inscribed: true,
            construction: false,
            selected: Vec::new(),
            selected_constraints: Vec::new(),
            dim_edit: None,
            over_prompt: None,
            offset_loop: true,
            offset_constrain: true,
            shift: false,
            transform_ready: false,
            fillet_radius: 2.0,
            fillet_first: None,
            chamfer_first: None,
            move_copy: false,
            inference: InferenceSettings::default(),
            pattern_fitted: [false, false],
            circular_fitted: true,
            chamfer_mode: ChamferMode::Equal,
            chamfer_d1: 2.0,
            chamfer_d2: 3.0,
            chamfer_angle_deg: 45.0,
            pattern_count: [3, 1],
            pattern_spacing: [10.0, 10.0],
            pattern_spacing_eq: [None, None],
            circular_angle_eq: None,
            pattern_count_eq: [None, None],
            pattern_flip: [false, false],
            circular_count: 6,
            circular_angle_deg: 360.0,
            circular_flip: false,
            circular_radius: 20.0,
            circular_ref_deg: 0.0,
            path_count: 4,
            path_spacing: 10.0,
            skip_pick: false,
            pattern_dialog: None,
            scale_factor: 2.0,
            edit_pattern: None,
        }
    }
}

impl SketchTools {
    /// Activates a tool, dropping any half-finished input.
    pub fn set_tool(&mut self, tool: Tool) {
        self.tool = tool;
        self.picks.clear();
        self.selected_constraints.clear();
        self.fillet_first = None;
        self.chamfer_first = None;
        // Pattern commands open their dialog (with the pre-selection as the
        // geometry).
        self.pattern_dialog = matches!(
            tool,
            Tool::PatternRect | Tool::PatternCircular | Tool::PatternPath | Tool::PatternPoints
        )
        .then(|| super::pattern_dialog::PatternDialog::new(tool, !self.selected.is_empty()));
        if self.pattern_dialog.is_some() {
            return;
        }
        // Move/Copy/Rotate/Mirror act on the current selection (Inventor's
        // pre-selection); with nothing selected they start by selecting.
        if Self::is_transform(tool) {
            self.transform_ready = !self.selected.is_empty();
        } else if tool != Tool::Select {
            self.selected.clear();
        }
    }

    /// Move/Copy/Rotate/Mirror/Scale/Stretch: the geometry is chosen and
    /// the tool now picks its points.
    pub fn transform_ready(&self) -> bool {
        self.transform_ready
    }

    /// Back to choosing the geometry of a transform tool (its dialog's
    /// Geometry selector).
    pub fn reselect_geometry(&mut self) {
        self.transform_ready = false;
        self.picks.clear();
    }

    /// Position of the first click of the current step.
    pub fn first_pick(&self) -> Option<Point2> {
        self.picks.first().map(|p| p.pos)
    }

    /// Position of click `k` of the current step.
    pub fn pick_pos(&self, k: usize) -> Option<Point2> {
        self.picks.get(k).map(|p| p.pos)
    }

    /// What the first click snapped to.
    pub fn first_snap(&self) -> Option<Snap> {
        self.picks.first().map(|p| p.snap)
    }

    /// Clicks taken so far by the current tool.
    pub fn pick_count(&self) -> usize {
        self.picks.len()
    }

    /// Tools that show a command dialog in Properties.
    pub fn has_dialog(&self) -> bool {
        matches!(
            self.tool,
            Tool::Polygon
                | Tool::Fillet
                | Tool::Chamfer
                | Tool::Move
                | Tool::Copy
                | Tool::Rotate
                | Tool::Mirror
                | Tool::Scale
                | Tool::Stretch
                | Tool::Offset
        ) || self.edit_pattern.is_some()
    }

    fn is_transform(tool: Tool) -> bool {
        matches!(
            tool,
            Tool::Move | Tool::Copy | Tool::Rotate | Tool::Mirror | Tool::Scale | Tool::Stretch
        )
    }

    /// Esc: cancels the current input; a second Esc returns to Select.
    pub fn escape(&mut self) {
        if self.pattern_dialog.is_some() {
            self.set_tool(Tool::Select); // Cancel
            return;
        }
        if self.picks.is_empty() {
            self.set_tool(Tool::Select);
        } else {
            self.picks.clear();
        }
    }

    /// The last committed point of a running chain (for inference).
    pub fn anchor(&self) -> Option<Point2> {
        match self.tool {
            Tool::Constrain(_) | Tool::Dimension | Tool::Select => None,
            _ => self.picks.last().map(|p| p.pos),
        }
    }

    /// A drawing tool (picks positions, may snap and infer)?
    pub fn drawing(&self) -> bool {
        self.tool != Tool::Select && !self.picks_geometry()
    }

    /// Whether picks are points (drawing) or geometry (constraints/dims).
    pub fn picks_geometry(&self) -> bool {
        if let Some(d) = &self.pattern_dialog {
            return d.active.wants_curves();
        }
        matches!(
            self.tool,
            Tool::Constrain(_)
                | Tool::Dimension
                | Tool::CircleTangent
                | Tool::Trim
                | Tool::Extend
                | Tool::Split
                | Tool::Mirror
                | Tool::PatternRect
                | Tool::PatternPath
                | Tool::Fillet
                | Tool::Chamfer
        ) || (self.tool == Tool::Offset && self.picks.is_empty())
            || (Self::is_transform(self.tool) && !self.transform_ready)
    }

    /// Status-bar prompt for the current step.
    pub fn prompt(&self) -> String {
        if let Some(d) = &self.pattern_dialog {
            return format!("{}: {}", self.tool.label(), super::i18n::tr(d.prompt()));
        }
        let n = self.picks.len();
        let s = match self.tool {
            Tool::Select => "Select geometry, drag points; Delete removes the selection",
            Tool::Line if n == 0 => {
                "Line: click the start point (drag from a curve end for a tangent/perpendicular arc)"
            }
            Tool::Line => {
                "Line: click the next point (Esc or right-click ends; Ctrl = no inference)"
            }
            Tool::CircleCenter if n == 0 => "Center Point Circle: click the centre",
            Tool::CircleCenter => {
                "Center Point Circle: click a point on the circle (on a curve = tangent)"
            }
            Tool::CircleTangent => "Tangent Circle: select three lines",
            Tool::ArcThreePoint if n < 2 => "Three Point Arc: click the two end points",
            Tool::ArcThreePoint => {
                "Three Point Arc: click a point on the arc (sets side and radius)"
            }
            Tool::ArcTangent if n == 0 => "Tangent Arc: click the end of a line or arc",
            Tool::ArcTangent => "Tangent Arc: click the arc end point",
            Tool::ArcCenter if n == 0 => "Center Point Arc: click the centre",
            Tool::ArcCenter if n == 1 => "Center Point Arc: click the start point (radius)",
            Tool::ArcCenter => "Center Point Arc: click the end point",
            Tool::RectTwoPoint => "Two Point Rectangle: click two opposite corners",
            Tool::RectThreePoint if n < 2 => {
                "Three Point Rectangle: click a corner, then the end of the first side"
            }
            Tool::RectThreePoint => "Three Point Rectangle: click to set the adjacent side",
            Tool::RectCenter => "Center Rectangle: click the centre, then a corner",
            Tool::RectCenterThreePoint if n < 2 => {
                "Center Rectangle: click the centre, then half a side"
            }
            Tool::RectCenterThreePoint => "Center Rectangle: click to set the adjacent side",
            Tool::SlotCenterToCenter if n < 2 => "Center to Center Slot: click both arc centres",
            Tool::SlotOverall if n < 2 => "Overall Slot: click both slot ends",
            Tool::SlotCenterPoint if n < 2 => {
                "Center Point Slot: click the slot centre, then an arc centre"
            }
            Tool::SlotArcThreePoint if n < 3 => {
                "Three Point Arc Slot: click start, end, then a point on the arc"
            }
            Tool::SlotArcCenter if n < 3 => {
                "Center Point Arc Slot: click the centre, start, then end"
            }
            Tool::SlotCenterToCenter
            | Tool::SlotOverall
            | Tool::SlotCenterPoint
            | Tool::SlotArcThreePoint
            | Tool::SlotArcCenter => "Slot: click to set the width",
            Tool::Polygon => {
                "Polygon: click the centre, then a vertex (inscribed) or side midpoint (circumscribed)"
            }
            Tool::SplineCv => "Spline: click control points; Enter or double-click finishes",
            Tool::SplineInterp => {
                "Interpolation Spline: click the points it passes through; Enter or double-click finishes"
            }
            Tool::Point => "Point: click to place",
            Tool::Constrain(c) => c.prompt(n),
            Tool::Dimension if n == 0 => "Dimension: select a line, circle, arc or point",
            Tool::Dimension => "Dimension: select more geometry, or click empty space to place",
            Tool::Trim => "Trim: click the part of a curve to remove (Shift = Extend)",
            Tool::Extend => "Extend: click near the end to extend (Shift = Trim)",
            Tool::Split => "Split: click a curve near where it should be split",
            Tool::Offset if n == 0 => {
                "Offset: select a curve (Loop Select / Constrain Offset in Properties)"
            }
            Tool::Offset => "Offset: click to set the offset distance",
            Tool::Fillet | Tool::Chamfer if n == 0 => {
                "Select the first line, or click a corner (size in Properties)"
            }
            Tool::Fillet | Tool::Chamfer => "Select the second line",
            Tool::Move
            | Tool::Copy
            | Tool::Rotate
            | Tool::Mirror
            | Tool::PatternRect
            | Tool::PatternCircular
            | Tool::Scale
            | Tool::Stretch
                if !self.transform_ready =>
            {
                "Select geometry, then press Enter (or right-click: Done)"
            }
            Tool::Scale => "Scale: click the base point (scale factor in Properties)",
            Tool::Stretch if n == 0 => "Stretch: click the base point",
            Tool::Stretch => "Stretch: click the destination point",
            Tool::Move | Tool::Copy if n == 0 => "Click the base point",
            Tool::Move | Tool::Copy => "Click the destination point",
            Tool::Rotate if n == 0 => "Rotate: click the centre point",
            Tool::Rotate if n == 1 => "Rotate: click a reference point",
            Tool::Rotate => "Rotate: click to set the angle",
            Tool::Mirror => "Mirror: click the mirror line",
            Tool::PatternRect if n == 0 => {
                "Rectangular Pattern: click the Direction 1 line (count, spacing in Properties)"
            }
            Tool::PatternRect => "Rectangular Pattern: click the Direction 2 line",
            Tool::PatternCircular => {
                "Circular Pattern: click the centre point (count, angle in Properties)"
            }
            Tool::PatternPath => {
                "Pattern along Path: click the path (line, arc or spline; count, spacing in Properties)"
            }
            Tool::PatternPoints if n == 0 => {
                "Pattern at Points: click the base point of the selected geometry"
            }
            Tool::PatternPoints => {
                "Pattern at Points: click target points (empty space adds a point); Enter: finish"
            }
        };
        s.to_owned()
    }

    /// Applies inference to a raw cursor pick unless suppressed with Ctrl:
    /// H/V alignment with the previous point, else perpendicular/parallel to
    /// the line that ends there (Inventor's constraint inference).
    pub fn infer(&self, sketch: &Sketch, mut pick: Pick, ctrl: bool, mm_per_px: Real) -> Pick {
        let k = &self.inference;
        // Tangent snap: a line from the previous point touching the circle
        // or arc under the cursor, near the touching point.
        if self.tool == Tool::Line
            && !ctrl
            && k.infer
            && let Snap::OnEntity(e) = pick.snap
            && let Some(from) = self.anchor()
            && let Some((c, r)) = sketch.circle_geometry(e)
            && let Some(t) = tangent_points(from, c, r)
                .into_iter()
                .filter(|&t| on_arc_span(sketch, e, t))
                .min_by(|a, b| (a - pick.pos).norm().total_cmp(&(b - pick.pos).norm()))
            && (t - pick.pos).norm() <= TANGENT_PX * mm_per_px
        {
            pick.pos = t;
            pick.snap = Snap::Tangent(e);
            return pick;
        }
        if !self.drawing() || ctrl || !k.infer || pick.snap != Snap::None {
            return pick;
        }
        let applies = matches!(self.tool, Tool::Line | Tool::SplineCv | Tool::SplineInterp);
        let prev = self.anchor().filter(|_| applies);
        if let Some(prev) = prev {
            pick = self.infer_segment(sketch, pick, prev);
        }
        if k.point_alignment {
            pick = self.align_points(sketch, pick, prev, mm_per_px);
        }
        pick
    }

    /// Point alignment: snaps x and/or y to existing points within a few
    /// pixels (not the chain's own anchor: that is H/V inference).
    fn align_points(
        &self,
        sketch: &Sketch,
        mut pick: Pick,
        prev: Option<Point2>,
        mm_per_px: Real,
    ) -> Pick {
        let tol = ALIGN_PX * mm_per_px;
        let (free_x, free_y) = match pick.infer {
            Some(Infer::Horizontal) => (true, false),
            Some(Infer::Vertical) => (false, true),
            Some(_) => return pick,
            None => (true, true),
        };
        let mut best_x: Option<(Real, PointId, Real)> = None;
        let mut best_y: Option<(Real, PointId, Real)> = None;
        for (id, p) in sketch.points() {
            let q = p.position;
            if prev.is_some_and(|a| (a - q).norm() < 1e-9) {
                continue;
            }
            // Far along the other axis only: a point right at the cursor
            // is a coincident snap, not an alignment.
            let (dx, dy) = ((q.x - pick.pos.x).abs(), (q.y - pick.pos.y).abs());
            if free_x && dx <= tol && dy > tol && best_x.is_none_or(|b| dx < b.0) {
                best_x = Some((dx, id, q.x));
            }
            if free_y && dy <= tol && dx > tol && best_y.is_none_or(|b| dy < b.0) {
                best_y = Some((dy, id, q.y));
            }
        }
        if let Some((_, id, x)) = best_x {
            pick.pos.x = x;
            pick.align.x = Some(id);
        }
        if let Some((_, id, y)) = best_y {
            pick.pos.y = y;
            pick.align.y = Some(id);
        }
        pick
    }

    /// H/V and ∥/⊥ inference of the segment from `prev` to the pick.
    fn infer_segment(&self, sketch: &Sketch, mut pick: Pick, prev: Point2) -> Pick {
        let k = &self.inference;
        let d = pick.pos - prev;
        let tol = 3f64.to_radians().tan();
        if d.norm() > 1e-6 {
            if k.horizontal && d.y.abs() <= tol * d.x.abs() {
                pick.pos.y = prev.y;
                pick.infer = Some(Infer::Horizontal);
            } else if k.vertical && d.x.abs() <= tol * d.y.abs() {
                pick.pos.x = prev.x;
                pick.infer = Some(Infer::Vertical);
            } else if let Some((line, u)) = self.previous_line(sketch) {
                let n = Vector2::new(-u.y, u.x);
                if k.parallel && d.dot(&n).abs() <= tol * d.dot(&u).abs() {
                    pick.pos = prev + u * d.dot(&u);
                    pick.infer = Some(Infer::Parallel(line));
                } else if k.perpendicular && d.dot(&u).abs() <= tol * d.dot(&n).abs() {
                    pick.pos = prev + n * d.dot(&n);
                    pick.infer = Some(Infer::Perpendicular(line));
                }
            }
        }
        pick
    }

    /// The line ending at the current chain point, and its unit direction.
    fn previous_line(&self, sketch: &Sketch) -> Option<(EntityId, Vector2)> {
        if self.tool != Tool::Line {
            return None;
        }
        let Snap::Point(p) = self.picks.last()?.snap else {
            return None;
        };
        sketch
            .entities()
            .filter(|(_, e)| e.is_line() && e.endpoints().contains(&p))
            .find_map(|(id, _)| {
                let (a, b) = sketch.line_geometry(id)?;
                Some((id, (b - a).try_normalize(1e-12)?))
            })
    }

    // ---- clicks ------------------------------------------------------------

    /// Handles a left click with the (snapped, inferred) pick.
    pub fn click(&mut self, ps: &mut PartSketch, mut pick: Pick) -> ClickResult {
        if self.pattern_dialog.is_some() {
            return self.dialog_pick(&ps.sketch, pick).map(|()| None);
        }
        // Inferred constraints are shown but not created without Persist.
        if self.drawing() && !(self.inference.infer && self.inference.persist) {
            if matches!(pick.snap, Snap::OnEntity(_) | Snap::Midpoint(_)) {
                pick.snap = Snap::None;
            }
            pick.infer = None;
            pick.align = Align::default(); // the position stays aligned
        }
        match self.tool {
            Tool::Select => Ok(None),
            Tool::Fillet | Tool::Chamfer => self.corner_click(ps, pick),
            Tool::Trim | Tool::Extend | Tool::Split => {
                let Snap::OnEntity(e) = pick.snap else {
                    return Err("Click a curve".into());
                };
                if ps.reference_lines.contains(&e) {
                    return Err("Projected geometry cannot be modified".into());
                }
                let tool = match (self.tool, self.shift) {
                    (Tool::Trim, true) => Tool::Extend,
                    (Tool::Extend, true) => Tool::Trim,
                    (t, _) => t,
                };
                let at = pick.pos;
                commit(ps, tool.label(), false, |ps| match tool {
                    Tool::Trim => ps.sketch.trim(e, at),
                    Tool::Extend => ps.sketch.extend(e, at),
                    _ => ps.sketch.split(e, at).map(|_| ()),
                })
            }
            Tool::Offset => {
                if self.picks.is_empty() {
                    let Snap::OnEntity(_) = pick.snap else {
                        return Err("Select a curve".into());
                    };
                    self.picks.push(pick);
                    return Ok(None);
                }
                let e = en_of(&self.picks[0]);
                let d = ps
                    .sketch
                    .offset_side(e, pick.pos)
                    .map_err(|x| x.to_string())?;
                self.picks.clear();
                let (lp, cons) = (self.offset_loop, self.offset_constrain);
                commit(ps, "Offset", self.construction, |ps| {
                    ps.sketch.offset(e, d, lp, cons).map(|_| ())
                })
            }
            t if Self::is_transform(t) && !self.transform_ready => {
                let Snap::OnEntity(e) = pick.snap else {
                    return Err("Select geometry; Enter when done".into());
                };
                if let Some(k) = self.selected.iter().position(|&x| x == e) {
                    self.selected.remove(k);
                } else {
                    self.selected.push(e);
                }
                Ok(None)
            }
            Tool::Mirror => {
                let Snap::OnEntity(line) = pick.snap else {
                    return Err("Click the mirror line".into());
                };
                if !ps.sketch.entity(line).is_some_and(Entity::is_line) {
                    return Err("The mirror line must be a line".into());
                }
                let sel = std::mem::take(&mut self.selected);
                let r = commit(ps, "Mirror", false, |ps| {
                    ps.sketch.mirror_geometry(&sel, line).map(|_| ())
                });
                self.set_tool(Tool::Select);
                r
            }
            Tool::PatternRect | Tool::PatternCircular | Tool::PatternPath | Tool::PatternPoints => {
                Ok(None)
            } // handled by the dialog
            Tool::Scale => {
                let (sel, k) = (self.selected.clone(), self.scale_factor);
                let r = commit(ps, "Scale", false, |ps| {
                    ps.sketch.scale_geometry(&sel, pick.pos, k)
                });
                self.set_tool(Tool::Select);
                r
            }
            Tool::Stretch => {
                if self.picks.is_empty() {
                    self.picks.push(pick);
                    return Ok(None);
                }
                let delta = pick.pos - self.picks[0].pos;
                let sel = self.selected.clone();
                self.picks.clear();
                let r = commit(ps, "Stretch", false, |ps| {
                    ps.sketch.stretch_geometry(&sel, delta)
                });
                self.set_tool(Tool::Select);
                r
            }
            Tool::Move | Tool::Copy | Tool::Rotate => {
                self.picks.push(pick);
                let need = if self.tool == Tool::Rotate { 3 } else { 2 };
                if self.picks.len() < need {
                    return Ok(None);
                }
                let picks = std::mem::take(&mut self.picks);
                let pos: Vec<Point2> = picks.iter().map(|p| p.pos).collect();
                let f = transform_of(self.tool, &pos);
                let sel = self.selected.clone();
                let tool = self.tool;
                let copy = tool == Tool::Copy || (tool == Tool::Move && self.move_copy);
                commit(ps, tool.label(), false, |ps| {
                    if copy {
                        ps.sketch.copy_geometry(&sel, f, false, true).map(|_| ())
                    } else {
                        move_held(&mut ps.sketch, &sel, f)
                    }
                })
            }
            Tool::Constrain(c) => self.constraint_click(ps, c, pick),
            Tool::Dimension => self.dimension_click(ps, pick),
            Tool::CircleTangent => {
                let Snap::OnEntity(e) = pick.snap else {
                    return Err("Select a line".into());
                };
                if !ps.sketch.entity(e).is_some_and(Entity::is_line) {
                    return Err("Tangent Circle needs three lines".into());
                }
                self.picks.push(pick);
                if self.picks.len() < 3 {
                    return Ok(None);
                }
                let picks = std::mem::take(&mut self.picks);
                let lines: Vec<EntityId> = picks
                    .iter()
                    .filter_map(|p| match p.snap {
                        Snap::OnEntity(e) => Some(e),
                        _ => None,
                    })
                    .collect();
                if lines.len() < 3 {
                    return Err("Tangent Circle needs three lines".into());
                }
                let guess = Point2::from(
                    (picks[0].pos.coords + picks[1].pos.coords + picks[2].pos.coords) / 3.0,
                );
                let r = lines
                    .iter()
                    .filter_map(|&l| ps.sketch.line_geometry(l))
                    .map(|(a, b)| ((b - a).perp(&(guess - a)) / (b - a).norm()).abs())
                    .fold(Real::INFINITY, Real::min);
                let construction = self.construction;
                commit(ps, "Tangent Circle", construction, |ps| {
                    ps.sketch
                        .add_tangent_circle([lines[0], lines[1], lines[2]], guess, r)
                        .map(|_| ())
                })
            }
            Tool::SplineCv | Tool::SplineInterp => {
                if self
                    .picks
                    .last()
                    .is_some_and(|p| (p.pos - pick.pos).norm() < 1e-6)
                {
                    return Ok(None); // second click of a double-click
                }
                self.picks.push(pick);
                Ok(None)
            }
            Tool::Line => {
                if self.picks.is_empty() {
                    self.picks.push(pick);
                    return Ok(None);
                }
                let start = self.picks[0];
                if (start.pos - pick.pos).norm() < 1e-6 {
                    return Ok(None);
                }
                let construction = self.construction;
                let mut end_point = None;
                let result = commit(ps, "Line", construction, |ps| {
                    let mut extra = Vec::new();
                    let a = endpoint(ps, &start, &mut extra);
                    let b = endpoint(ps, &pick, &mut extra);
                    let line = ps.sketch.add_line_between(a, b)?;
                    // Point alignment → Horizontal / Vertical between points.
                    for (p, al) in [(a, start.align), (b, pick.align)] {
                        if let Some(q) = al.x.filter(|&q| q != a && q != b) {
                            extra.push(Constraint::VerticalPoints { a: q, b: p });
                        }
                        if let Some(q) = al.y.filter(|&q| q != a && q != b) {
                            extra.push(Constraint::HorizontalPoints { a: q, b: p });
                        }
                    }
                    // Tangent snap at the end; a start clicked on a circle or
                    // arc with the line leaving it (nearly) tangentially is
                    // made tangent too (Inventor: drag off a circle).
                    if let Snap::Tangent(e) = pick.snap {
                        extra.push(Constraint::Tangent {
                            a: line,
                            b: e,
                            internal: false,
                        });
                    }
                    if let Snap::OnEntity(e) = start.snap
                        && let Some((c, _)) = ps.sketch.circle_geometry(e)
                    {
                        let radial = start.pos - c;
                        let dir = pick.pos - start.pos;
                        let sin_off =
                            radial.dot(&dir).abs() / (radial.norm() * dir.norm()).max(1e-12);
                        if sin_off < START_TANGENT_SIN {
                            extra.push(Constraint::Tangent {
                                a: line,
                                b: e,
                                internal: false,
                            });
                        }
                    }
                    match pick.infer {
                        Some(Infer::Horizontal) => extra.push(Constraint::Horizontal { line }),
                        Some(Infer::Vertical) => extra.push(Constraint::Vertical { line }),
                        Some(Infer::Perpendicular(l)) => {
                            extra.push(Constraint::Perpendicular { a: l, b: line })
                        }
                        Some(Infer::Parallel(l)) => {
                            extra.push(Constraint::Parallel { a: l, b: line })
                        }
                        None => {}
                    }
                    for c in extra {
                        ps.sketch.add_constraint(c)?;
                    }
                    end_point = Some(b);
                    Ok(())
                });
                // Inferred alignment may conflict with other constraints:
                // retry without it rather than refusing the line.
                let result = match result {
                    Err(_) if pick.infer.is_some() || pick.align != Align::default() => {
                        let plain = Pick {
                            infer: None,
                            align: Align::default(),
                            ..pick
                        };
                        self.picks[0].align = Align::default();
                        return self.click(ps, plain);
                    }
                    r => r,
                };
                if result.is_ok()
                    && let Some(b) = end_point
                {
                    // Continue the chain from the shared end point.
                    self.picks = vec![Pick {
                        pos: ps.sketch.point(b).map_or(pick.pos, |p| p.position),
                        snap: Snap::Point(b),
                        infer: None,
                        align: Align::default(),
                    }];
                }
                result
            }
            Tool::ArcTangent if self.picks.is_empty() => match pick.snap {
                Snap::Point(p) if curve_ending_at(&ps.sketch, p).is_some() => {
                    self.picks.push(pick);
                    Ok(None)
                }
                _ => Err("Click the end of a line or arc".into()),
            },
            _ => {
                self.picks.push(pick);
                if Some(self.picks.len()) < self.tool.clicks() {
                    return Ok(None);
                }
                let picks = std::mem::take(&mut self.picks);
                self.create(ps, &picks)
            }
        }
    }

    /// Edit Pattern: loads the pattern's definition into the dialog fields.
    pub fn start_edit_pattern(&mut self, id: PatternId, kind: PatternKind) {
        self.set_tool(Tool::Select);
        self.pattern_count_eq = [None, None];
        self.skip_pick = false;
        match kind {
            PatternKind::Rectangular { dir1, dir2 } => {
                // Fitted rows show their overall length, as they were entered.
                let shown = |d: &PatternDirection| {
                    if d.fitted && d.count > 1 {
                        d.spacing * (d.count - 1) as Real
                    } else {
                        d.spacing
                    }
                };
                self.pattern_count = [dir1.count, dir2.map_or(1, |d| d.count)];
                self.pattern_spacing = [shown(&dir1), dir2.map_or(10.0, |d| shown(&d))];
                self.pattern_flip = [dir1.flip, dir2.is_some_and(|d| d.flip)];
                self.pattern_fitted = [dir1.fitted, dir2.is_some_and(|d| d.fitted)];
            }
            PatternKind::Circular {
                count,
                angle,
                fitted,
                ..
            } => {
                self.circular_count = count;
                let full = (angle.abs() - std::f64::consts::TAU).abs() < 1e-9;
                // Fitted (or a full turn) shows the overall angle, else the step.
                self.circular_fitted = fitted || full;
                self.circular_angle_deg = if self.circular_fitted {
                    angle.abs().to_degrees()
                } else {
                    angle.abs().to_degrees() / (count.max(2) - 1) as Real
                };
                self.circular_flip = angle < 0.0;
            }
            PatternKind::Points { .. } => {}
            PatternKind::Path { count, spacing, .. } => {
                self.path_count = count;
                self.path_spacing = spacing;
            }
        }
        self.edit_pattern = Some((id, kind));
    }

    /// The pattern the drag handles belong to: the one in Edit Pattern, or
    /// the one the open Rectangular / Circular dialog describes (built on a
    /// copy of the sketch, so nothing is created before OK). The copy, the
    /// pattern's id in it and its definition.
    pub fn active_pattern(&self, sk: &Sketch) -> Option<(Sketch, PatternId, PatternKind)> {
        if let Some((id, base)) = self.edit_pattern {
            return Some((sk.clone(), id, base));
        }
        let d = self.pattern_dialog.as_ref()?;
        if !matches!(d.tool, Tool::PatternRect | Tool::PatternCircular) {
            return None;
        }
        let mut trial = sk.clone();
        let before: Vec<PatternId> = trial.patterns().map(|(id, _)| id).collect();
        self.dialog_build(&mut trial).ok()?;
        let (id, p) = trial.patterns().find(|(id, _)| !before.contains(id))?;
        let kind = p.kind;
        Some((trial, id, kind))
    }

    /// Whether drag handles of a pattern are shown (Edit Pattern or an
    /// open pattern dialog with its inputs picked).
    pub fn pattern_active(&self) -> bool {
        self.edit_pattern.is_some()
            || self
                .pattern_dialog
                .as_ref()
                .is_some_and(|d| matches!(d.tool, Tool::PatternRect | Tool::PatternCircular))
    }

    /// Drag handles of the pattern being edited (sketch coordinates):
    /// spacing / angle at the first copy, count beyond the last one.
    pub fn pattern_handles(&self, sk: &Sketch) -> Vec<PatHandle> {
        let Some((owned, id, base)) = self.active_pattern(sk) else {
            return Vec::new();
        };
        let sk = &owned;
        let Some(seed) = pattern_seed(sk, id) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        match self.pattern_kind_from_fields(&base) {
            PatternKind::Rectangular { dir1, dir2 } => {
                for (k, d) in std::iter::once(dir1).chain(dir2).enumerate() {
                    let Some(u) = direction_unit(sk, &d) else {
                        continue;
                    };
                    let s = d.spacing;
                    let n = d.count.max(1) as Real;
                    out.push(PatHandle {
                        grip: PatGrip::Spacing(k),
                        from: seed,
                        pos: seed + u * s,
                        dir: u,
                    });
                    out.push(PatHandle {
                        grip: PatGrip::Count(k),
                        from: seed + u * s * (n - 1.0),
                        pos: seed + u * s * (n - 0.5),
                        dir: u,
                    });
                }
            }
            PatternKind::Circular {
                center,
                count,
                angle,
                ..
            } => {
                let Some(c) = sk.point(center).map(|p| p.position) else {
                    return out;
                };
                let n = count.max(2) as Real;
                let full = (angle.abs() - std::f64::consts::TAU).abs() < 1e-9;
                let step = if full { angle / n } else { angle / (n - 1.0) };
                let rot = |t: Real| {
                    let (sn, cs) = t.sin_cos();
                    let r = seed - c;
                    c + Vector2::new(cs * r.x - sn * r.y, sn * r.x + cs * r.y)
                };
                out.push(PatHandle {
                    grip: PatGrip::Angle,
                    from: seed,
                    pos: rot(step),
                    dir: Vector2::zeros(),
                });
                out.push(PatHandle {
                    grip: PatGrip::Count(0),
                    from: rot(step * (n - 1.0)),
                    pos: rot(step * (n - 0.5)),
                    dir: Vector2::zeros(),
                });
            }
            PatternKind::Points { .. } | PatternKind::Path { .. } => {}
        }
        out
    }

    /// Drags a pattern handle to `at` (sketch coordinates): updates the
    /// dialog fields; the preview follows. Returns the new value's text.
    pub fn drag_pattern_handle(
        &mut self,
        sk: &Sketch,
        grip: PatGrip,
        at: Point2,
    ) -> Option<String> {
        let (owned, id, base) = self.active_pattern(sk)?;
        let sk = &owned;
        let seed = pattern_seed(sk, id)?;
        match (self.pattern_kind_from_fields(&base), grip) {
            (PatternKind::Rectangular { dir1, dir2 }, PatGrip::Spacing(k) | PatGrip::Count(k)) => {
                let d = if k == 0 { dir1 } else { dir2? };
                let u = direction_unit(sk, &d)?;
                let t = (at - seed).dot(&u);
                if let PatGrip::Spacing(_) = grip {
                    self.pattern_spacing_eq[k] = None;
                    let step = (t * 10.0).round() / 10.0; // 0.1 mm
                    let step = step.max(0.1);
                    self.pattern_spacing[k] = if self.pattern_fitted[k] && d.count > 1 {
                        step * (d.count - 1) as Real
                    } else {
                        step
                    };
                    Some(format!("Spacing {step:.1} mm"))
                } else {
                    // The grip sits half a step beyond the last copy.
                    let n = ((t / d.spacing.max(1e-9) + 0.5).round() as i64).clamp(1, 500) as usize;
                    self.pattern_count[k] = n;
                    if self.pattern_fitted[k] && n > 1 {
                        // Keep the step while dragging the count.
                        self.pattern_spacing[k] = d.spacing * (n - 1) as Real;
                    }
                    Some(format!("Count {n}"))
                }
            }
            (
                PatternKind::Circular {
                    center,
                    count,
                    angle,
                    ..
                },
                g,
            ) => {
                let c = sk.point(center)?.position;
                let (r, v) = (seed - c, at - c);
                let sign = if angle < 0.0 { -1.0 } else { 1.0 };
                // Angle from the seed to the cursor, in the pattern's sense.
                let theta = (sign * r.perp(&v))
                    .atan2(r.dot(&v))
                    .rem_euclid(std::f64::consts::TAU);
                let n = count.max(2) as Real;
                let full = (angle.abs() - std::f64::consts::TAU).abs() < 1e-9;
                let step = (if full { angle / n } else { angle / (n - 1.0) }).abs();
                match g {
                    PatGrip::Angle => {
                        self.circular_angle_eq = None;
                        let deg = theta.to_degrees().round().clamp(1.0, 359.0);
                        self.circular_angle_deg = if self.circular_fitted {
                            deg * (n - 1.0)
                        } else {
                            deg
                        };
                        Some(format!("Angle {deg:.0} deg"))
                    }
                    _ => {
                        let k =
                            ((theta / step.max(1e-9) + 0.5).round() as i64).clamp(2, 360) as usize;
                        if self.circular_fitted && !full {
                            self.circular_angle_deg = step.to_degrees() * (k - 1) as Real;
                        }
                        self.circular_count = k;
                        Some(format!("Count {k}"))
                    }
                }
            }
            _ => None,
        }
    }

    /// The definition the dialog fields describe, on the lines / centre of
    /// `base`.
    pub fn pattern_kind_from_fields(&self, base: &PatternKind) -> PatternKind {
        let spacing = |k: usize| {
            if self.pattern_fitted[k] && self.pattern_count[k] > 1 {
                self.pattern_spacing[k] / (self.pattern_count[k] - 1) as Real
            } else {
                self.pattern_spacing[k]
            }
        };
        match *base {
            PatternKind::Rectangular { dir1, dir2 } => PatternKind::Rectangular {
                dir1: PatternDirection {
                    count: self.pattern_count[0],
                    spacing: spacing(0),
                    flip: self.pattern_flip[0],
                    fitted: self.pattern_fitted[0],
                    ..dir1
                },
                dir2: dir2.map(|d| PatternDirection {
                    count: self.pattern_count[1],
                    spacing: spacing(1),
                    flip: self.pattern_flip[1],
                    fitted: self.pattern_fitted[1],
                    ..d
                }),
            },
            PatternKind::Circular { center, .. } => {
                let n = self.circular_count.max(2) as Real;
                let total = if self.circular_fitted {
                    self.circular_angle_deg
                } else if (self.circular_angle_deg * n - 360.0).abs() < 1e-6 {
                    360.0
                } else {
                    self.circular_angle_deg * (n - 1.0)
                };
                let sign = if self.circular_flip { -1.0 } else { 1.0 };
                PatternKind::Circular {
                    center,
                    count: self.circular_count,
                    angle: sign * total.to_radians(),
                    fitted: self.circular_fitted,
                }
            }
            k @ PatternKind::Points { .. } => k,
            PatternKind::Path { path, .. } => PatternKind::Path {
                path,
                count: self.path_count,
                spacing: self.path_spacing,
            },
        }
    }

    /// Dialog OK: creates the pattern (one Undo step) and closes.
    pub fn dialog_ok(&mut self, ps: &mut PartSketch) -> ClickResult {
        let label = self.tool.label();
        let before: Vec<PatternId> = ps.sketch.patterns().map(|(id, _)| id).collect();
        let r = commit(ps, label, false, |ps| {
            self.dialog_build(&mut ps.sketch).map(|_| ())
        });
        if r.is_ok() {
            self.link_pattern_equations(ps, &before);
        }
        if r.is_ok() {
            self.selected.clear();
            self.set_tool(Tool::Select);
        }
        r
    }

    /// Equations typed into the dialog's spacing / angle fields go onto the
    /// new pattern's dimensions (its first dimensions are direction 1's and
    /// 2's spacing, or the circular angle), so the copies follow the
    /// parameters.
    fn link_pattern_equations(&mut self, ps: &mut PartSketch, before: &[PatternId]) {
        let Some((_, p)) = ps.sketch.patterns().find(|(id, _)| !before.contains(id)) else {
            return;
        };
        let dims: Vec<crate::sketch::ConstraintId> = p
            .constraints
            .iter()
            .copied()
            .filter(|&k| ps.sketch.constraint(k).and_then(|c| c.value()).is_some())
            .collect();
        let eqs: Vec<Option<String>> = match p.kind {
            PatternKind::Rectangular { dir2, .. } => {
                let mut v = vec![self.pattern_spacing_eq[0].clone()];
                if dir2.is_some() {
                    v.push(self.pattern_spacing_eq[1].clone());
                }
                v
            }
            PatternKind::Circular { .. } => vec![self.circular_angle_eq.clone()],
            _ => Vec::new(),
        };
        for (k, eq) in dims.into_iter().zip(eqs) {
            if let Some(eq) = eq
                && let Some(dp) = ps.sketch.dim_param_mut(k)
            {
                dp.expr = Some(eq);
            }
        }
        let counts = std::mem::take(&mut self.pattern_count_eq);
        let new = ps
            .sketch
            .patterns()
            .map(|(id, _)| id)
            .find(|id| !before.contains(id));
        if let Some(id) = new {
            let _ = ps.sketch.set_pattern_count_exprs(id, counts);
        }
        self.pattern_spacing_eq = [None, None];
        self.circular_angle_eq = None;
    }

    /// Fillet/Chamfer: two lines (the clicked sides are kept), or one click
    /// on a corner point where exactly two lines meet.
    fn corner_click(&mut self, ps: &mut PartSketch, pick: Pick) -> ClickResult {
        let is_line = |e: EntityId| ps.sketch.entity(e).is_some_and(Entity::is_line);
        let (a, b, hints) = match pick.snap {
            Snap::Point(p) if self.picks.is_empty() => {
                let lines: Vec<EntityId> = ps
                    .sketch
                    .entities()
                    .filter(|(_, e)| e.is_line() && e.endpoints().contains(&p))
                    .map(|(id, _)| id)
                    .collect();
                let [a, b] = lines[..] else {
                    return Err("Click a corner where two lines meet, or select a line".into());
                };
                (a, b, None)
            }
            Snap::OnEntity(e) if is_line(e) => {
                let Some(first) = self.picks.first().copied() else {
                    self.picks.push(pick);
                    return Ok(None);
                };
                self.picks.clear();
                (en_of(&first), e, Some((first.pos, pick.pos)))
            }
            _ => return Err("Select a line".into()),
        };
        for e in [a, b] {
            if ps.reference_lines.contains(&e) {
                return Err("Projected geometry cannot be modified".into());
            }
        }
        if self.tool == Tool::Chamfer {
            let kind = match self.chamfer_mode {
                ChamferMode::Equal => ChamferKind::Equal {
                    distance: self.chamfer_d1,
                },
                ChamferMode::TwoDistances => ChamferKind::TwoDistances {
                    first: self.chamfer_d1,
                    second: self.chamfer_d2,
                },
                ChamferMode::DistanceAngle => ChamferKind::DistanceAngle {
                    distance: self.chamfer_d1,
                    angle: self.chamfer_angle_deg.to_radians(),
                },
            };
            let equal_to = self
                .chamfer_first
                .filter(|&(e, d)| {
                    self.chamfer_mode == ChamferMode::Equal
                        && d == self.chamfer_d1
                        && ps.sketch.entity(e).is_some()
                })
                .map(|(e, _)| e);
            let mut leg = None;
            let result = commit(ps, "Chamfer", self.construction, |ps| {
                leg = ps
                    .sketch
                    .chamfer(a, b, kind, hints, equal_to)?
                    .entities
                    .get(1)
                    .copied();
                Ok(())
            });
            if result.is_ok()
                && equal_to.is_none()
                && self.chamfer_mode == ChamferMode::Equal
                && let Some(leg) = leg
            {
                self.chamfer_first = Some((leg, self.chamfer_d1));
            }
            return result;
        }
        let r = self.fillet_radius;
        let equal_to = self
            .fillet_first
            .filter(|&(e, r0)| r0 == r && ps.sketch.entity(e).is_some())
            .map(|(e, _)| e);
        let mut arc = None;
        let result = commit(ps, "Fillet", self.construction, |ps| {
            arc = ps
                .sketch
                .fillet(a, b, r, hints, equal_to)?
                .entities
                .first()
                .copied();
            Ok(())
        });
        if result.is_ok()
            && equal_to.is_none()
            && let Some(arc) = arc
        {
            self.fillet_first = Some((arc, r));
        }
        result
    }

    /// Enter / double-click: finishes open-ended input (spline).
    pub fn finish(&mut self, ps: &mut PartSketch) -> ClickResult {
        if Self::is_transform(self.tool) && !self.transform_ready {
            if self.selected.is_empty() {
                return Err("Select geometry first".into());
            }
            self.transform_ready = true;
            return Ok(None);
        }
        if self.pattern_dialog.is_some() {
            return if self.dialog_enter() {
                self.dialog_ok(ps)
            } else {
                Ok(None)
            };
        }
        if !matches!(self.tool, Tool::SplineCv | Tool::SplineInterp) {
            self.picks.clear();
            return Ok(None);
        }
        let interp = self.tool == Tool::SplineInterp;
        let picks = std::mem::take(&mut self.picks);
        if picks.len() < 2 {
            return Err("A spline needs at least two points".into());
        }
        commit(ps, "Spline", self.construction, |ps| {
            let mut extra = Vec::new();
            let n = picks.len();
            let pts: Vec<PointId> = picks
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    if i == 0 || i == n - 1 {
                        endpoint(ps, p, &mut extra)
                    } else {
                        ps.sketch.add_point(p.pos)
                    }
                })
                .collect();
            if interp {
                ps.sketch.add_interp_spline(pts)?;
            } else {
                ps.sketch.add_spline(pts)?;
            }
            for c in extra {
                ps.sketch.add_constraint(c)?;
            }
            Ok(())
        })
    }

    /// Creates the geometry of a fixed-click tool.
    fn create(&mut self, ps: &mut PartSketch, picks: &[Pick]) -> ClickResult {
        let construction = self.construction;
        let (sides, inscribed) = (self.polygon_sides, self.polygon_inscribed);
        let tool = self.tool;
        let pos: Vec<Point2> = picks.iter().map(|p| p.pos).collect();
        commit(ps, tool.label(), construction, |ps| {
            let mut extra = Vec::new();
            match tool {
                Tool::Point => {
                    endpoint(ps, &picks[0], &mut extra);
                }
                Tool::CircleCenter => {
                    let r = (pos[1] - pos[0]).norm();
                    if r < 1e-9 {
                        return Err(SketchError::InvalidValue("circle has zero radius"));
                    }
                    let c = endpoint(ps, &picks[0], &mut extra);
                    let circle = ps.sketch.add_circle_at(c, r)?;
                    // Inventor: a radius point on a curve makes the circle tangent to it.
                    if let Snap::OnEntity(e) = picks[1].snap
                        && ps
                            .sketch
                            .entity(e)
                            .is_some_and(|x| x.is_line() || x.is_circular())
                    {
                        ps.sketch.add_tangent(circle, e)?;
                    }
                }
                Tool::ArcThreePoint => {
                    let c = circumcenter(pos[0], pos[1], pos[2])
                        .ok_or(SketchError::InvalidValue("the three points are collinear"))?;
                    let ccw = (pos[1] - pos[0]).perp(&(pos[2] - pos[0])) < 0.0;
                    let a = endpoint(ps, &picks[0], &mut extra);
                    let b = endpoint(ps, &picks[1], &mut extra);
                    let pc = ps.sketch.add_point(c);
                    if ccw {
                        ps.sketch.add_arc(pc, a, b)?
                    } else {
                        ps.sketch.add_arc(pc, b, a)?
                    };
                }
                Tool::ArcCenter => {
                    let r = (pos[1] - pos[0]).norm();
                    let ue = (pos[2] - pos[0])
                        .try_normalize(1e-12)
                        .ok_or(SketchError::InvalidValue("end point at the centre"))?;
                    let end = Pick {
                        pos: pos[0] + ue * r,
                        ..picks[2]
                    };
                    let c = endpoint(ps, &picks[0], &mut extra);
                    let s = endpoint(ps, &picks[1], &mut extra);
                    let e = endpoint(ps, &end, &mut extra);
                    // Minor arc in the direction of the clicks.
                    if (pos[1] - pos[0]).perp(&(pos[2] - pos[0])) >= 0.0 {
                        ps.sketch.add_arc(c, s, e)?;
                    } else {
                        ps.sketch.add_arc(c, e, s)?;
                    }
                }
                Tool::ArcTangent => {
                    // Checked on the first click.
                    let Snap::Point(p) = picks[0].snap else {
                        return Err(SketchError::UnknownPoint);
                    };
                    tangent_arc(ps, p, pos[1], ArcKind::Tangent)?;
                }
                _ => {
                    let sh = build_shape(&mut ps.sketch, tool, &pos, sides, inscribed)?;
                    // Honour a snap of the first click (corner/centre) with Coincident.
                    if let Some(&first) = sh.points.first() {
                        let target = if matches!(
                            tool,
                            Tool::RectCenter | Tool::RectCenterThreePoint | Tool::SlotCenterPoint
                        ) {
                            *sh.points.last().unwrap_or(&first)
                        } else {
                            first
                        };
                        snap_constraint(ps, target, &picks[0], &mut extra);
                    }
                }
            }
            for c in extra {
                ps.sketch.add_constraint(c)?;
            }
            Ok(())
        })
    }

    /// Line tool: drag from a curve end released at `to`. The arc is
    /// tangent if the drag started along the curve (`initial_dir` within
    /// 45° of it), perpendicular otherwise.
    pub fn arc_drag(
        &mut self,
        ps: &mut PartSketch,
        from: PointId,
        initial_dir: Vector2,
        to: Point2,
    ) -> ClickResult {
        let Some((_, t)) = curve_ending_at(&ps.sketch, from)
            .and_then(|e| continuation(&ps.sketch, e, from).map(|t| (e, t)))
        else {
            return Err("Drag from the end of a line or arc".into());
        };
        let kind = if initial_dir.normalize().dot(&t).abs() >= std::f64::consts::FRAC_1_SQRT_2 {
            ArcKind::Tangent
        } else {
            ArcKind::Perpendicular
        };
        let mut end = None;
        let label = if kind == ArcKind::Tangent {
            "Tangent Arc"
        } else {
            "Perpendicular Arc"
        };
        let r = commit(ps, label, self.construction, |ps| {
            end = Some(tangent_arc(ps, from, to, kind)?);
            Ok(())
        });
        if r.is_ok()
            && let Some(e) = end
        {
            // Inventor continues the line chain from the arc end.
            self.picks = vec![Pick {
                pos: to,
                snap: Snap::Point(e),
                infer: None,
                align: Align::default(),
            }];
        }
        r
    }

    // ---- constraints -------------------------------------------------------

    fn constraint_click(
        &mut self,
        ps: &mut PartSketch,
        tool: ConstraintTool,
        pick: Pick,
    ) -> ClickResult {
        use ConstraintTool::*;
        if pick.snap == Snap::None {
            return Err("Select a point or curve".into());
        }
        self.picks.push(pick);
        let sk = &ps.sketch;
        let kind = |s: Snap| match s {
            Snap::Point(_) => 'p',
            Snap::OnEntity(e) => match sk.entity(e) {
                Some(x) if x.is_line() => 'l',
                Some(x) if x.is_circular() => 'c',
                _ => 's',
            },
            // Constraint tools never get midpoint or tangent snaps.
            Snap::None | Snap::Midpoint(_) | Snap::Tangent(_) => '-',
        };
        let ks: String = self.picks.iter().map(|p| kind(p.snap)).collect();
        // `ks` fixes the kind of every pick used below; a mismatch would
        // yield the null key, which `add_constraint` rejects as unknown.
        let pt = |i: usize| match self.picks[i].snap {
            Snap::Point(p) => p,
            _ => PointId::default(),
        };
        let en = |i: usize| match self.picks[i].snap {
            Snap::OnEntity(e) => e,
            _ => EntityId::default(),
        };

        // Decide: need more picks / invalid / the constraints to add.
        let wanted: Result<Option<Vec<Constraint>>, &str> = match (tool, ks.as_str()) {
            (Coincident, "p") => Ok(None),
            (Coincident, "pp") => Ok(Some(vec![Constraint::Coincident { a: pt(0), b: pt(1) }])),
            (Coincident, "pl") => Ok(Some(vec![Constraint::PointOnLine {
                point: pt(0),
                line: en(1),
            }])),
            (Coincident, "lp") => Ok(Some(vec![Constraint::PointOnLine {
                point: pt(1),
                line: en(0),
            }])),
            (Coincident, "pc" | "ps") => sk
                .on_curve(pt(0), en(1))
                .map(|c| Some(vec![c]))
                .ok_or("The point defines that curve"),
            (Coincident, "cp" | "sp") => sk
                .on_curve(pt(1), en(0))
                .map(|c| Some(vec![c]))
                .ok_or("The point defines that curve"),
            (Coincident, "l" | "c" | "s") => Ok(None),
            (Collinear | Parallel | Perpendicular, "l") => Ok(None),
            (Collinear, "ll") => Ok(Some(vec![Constraint::Collinear { a: en(0), b: en(1) }])),
            (Parallel, "ll") => Ok(Some(vec![Constraint::Parallel { a: en(0), b: en(1) }])),
            (Perpendicular, "ll") => {
                Ok(Some(vec![Constraint::Perpendicular { a: en(0), b: en(1) }]))
            }
            (Concentric, "c") => Ok(None),
            (Concentric, "cc") => Ok(Some(vec![Constraint::Concentric { a: en(0), b: en(1) }])),
            (Fix, "p") => Ok(Some(vec![fix(sk, pt(0))])),
            (Fix, "l" | "c" | "s") => Ok(Some(
                sk.entity(en(0))
                    .map(Entity::points)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|&p| !sk.point(p).is_some_and(|q| q.fixed))
                    .map(|p| fix(sk, p))
                    .collect(),
            )),
            (Horizontal, "l") => Ok(Some(vec![Constraint::Horizontal { line: en(0) }])),
            (Vertical, "l") => Ok(Some(vec![Constraint::Vertical { line: en(0) }])),
            (Horizontal | Vertical, "p") => Ok(None),
            (Horizontal, "pp") => Ok(Some(vec![Constraint::HorizontalPoints {
                a: pt(0),
                b: pt(1),
            }])),
            (Vertical, "pp") => Ok(Some(vec![Constraint::VerticalPoints {
                a: pt(0),
                b: pt(1),
            }])),
            (Tangent, "l" | "c") => Ok(None),
            (Tangent, "lc" | "cl" | "cc") => Ok(Some(Vec::new())), // via add_tangent below
            (Equal, "l" | "c") => Ok(None),
            (Equal, "ll" | "cc") => Ok(Some(vec![Constraint::Equal { a: en(0), b: en(1) }])),
            (Symmetric, "p" | "l" | "c" | "pp" | "ll" | "cc") => Ok(None),
            (Symmetric, "ppl") => Ok(Some(vec![Constraint::Symmetric {
                a: pt(0),
                b: pt(1),
                line: en(2),
            }])),
            (Symmetric, "lll") => Ok(Some(symmetric_lines(sk, en(0), en(1), en(2)))),
            (Symmetric, "ccl") => {
                let center = |e: EntityId| match sk.entity(e) {
                    Some(Entity::Circle { center, .. } | Entity::Arc { center, .. }) => *center,
                    // `ks` says circular; the null key would be rejected.
                    _ => PointId::default(),
                };
                Ok(Some(vec![
                    Constraint::Symmetric {
                        a: center(en(0)),
                        b: center(en(1)),
                        line: en(2),
                    },
                    Constraint::Equal { a: en(0), b: en(1) },
                ]))
            }
            _ => Err("That selection does not fit this constraint"),
        };
        let constraints = match wanted {
            Err(msg) => {
                self.picks.pop();
                return Err(msg.into());
            }
            Ok(None) => return Ok(None),
            Ok(Some(c)) => c,
        };
        let picks = std::mem::take(&mut self.picks);
        let tangent = (tool == Tangent).then(|| (en_of(&picks[0]), en_of(&picks[1])));
        commit_constraints(ps, tool.label(), |sketch| {
            if let Some((a, b)) = tangent {
                sketch.add_tangent(a, b)?;
            }
            for c in constraints {
                sketch.add_constraint(c)?;
            }
            Ok(())
        })
    }

    // ---- dimensions --------------------------------------------------------

    fn dimension_click(&mut self, ps: &mut PartSketch, pick: Pick) -> ClickResult {
        let sk = &ps.sketch;
        if pick.snap != Snap::None {
            // A selection. Two selections at most; then wait for placement.
            if self.picks.len() >= 2 {
                return Err("Click empty space to place the dimension".into());
            }
            if matches!(pick.snap, Snap::OnEntity(e) if sk.entity(e).is_some_and(Entity::is_spline))
            {
                return Err("Splines cannot be dimensioned yet".into());
            }
            self.picks.push(pick);
            return Ok(None);
        }
        // Placement click.
        let picks = std::mem::take(&mut self.picks);
        let place = pick.pos;
        let c = dimension_from(&ps.sketch, &picks, place)?;
        self.add_dimension(ps, c, place)
    }

    /// Adds a dimension with its measured value; opens the Edit Dimension
    /// box, or the driven-dimension prompt if it would over-constrain.
    fn add_dimension(
        &mut self,
        ps: &mut PartSketch,
        mut c: Constraint,
        place: Point2,
    ) -> ClickResult {
        // Measure the current value through a temporary insertion.
        let mut probe = ps.sketch.clone();
        let id = probe.add_constraint(c.clone()).map_err(|e| e.to_string())?;
        let measured = probe.measure(id).ok_or("cannot measure")?;
        if let Some(v) = c.value_mut() {
            *v = measured;
        }
        if measured.abs() < 1e-9 && !matches!(c, Constraint::Angle { .. }) {
            return Err("The geometry has zero size here".into());
        }
        let before = redundancy(&ps.sketch);
        let added = ps
            .sketch
            .add_constraint(c.clone())
            .map_err(|e| e.to_string())?;
        // The text goes where the user clicked to place the dimension.
        ps.sketch.set_dimension_label(added, Some(place));
        let ok = ps
            .sketch
            .solve()
            .is_ok_and(|r| r.status == SolveStatus::Solved && r.redundant <= before);
        if !ok {
            let _ = ps.sketch.remove_constraint(added);
            let _ = ps.sketch.solve();
            self.over_prompt = Some(OverPrompt {
                constraint: c,
                place,
            });
            return Ok(None);
        }
        let is_angle = matches!(c, Constraint::Angle { .. });
        self.dim_edit = Some(DimEdit {
            constraint: added,
            text: format_value(measured, is_angle),
            is_angle,
            focused: false,
        });
        Ok(Some("Dimension".into()))
    }

    /// Accepts the over-constraining dimension as driven.
    pub fn accept_driven(&mut self, ps: &mut PartSketch) -> ClickResult {
        let Some(p) = self.over_prompt.take() else {
            return Ok(None);
        };
        let id = ps
            .sketch
            .add_constraint(p.constraint)
            .map_err(|e| e.to_string())?;
        ps.sketch.set_driven(id, true).map_err(|e| e.to_string())?;
        ps.sketch.set_dimension_label(id, Some(p.place));
        Ok(Some("Driven Dimension".into()))
    }

    // ---- preview -----------------------------------------------------------

    /// The dimension being placed, as it would be created at the cursor:
    /// a copy of the sketch with it added (measured value, text at the
    /// cursor), and its id. Inventor shows it live before the placing click.
    pub fn dimension_preview(
        &self,
        ps: &PartSketch,
        cursor: &Pick,
    ) -> Option<(Sketch, crate::sketch::ConstraintId)> {
        if self.tool != Tool::Dimension || self.picks.is_empty() || cursor.snap != Snap::None {
            return None;
        }
        let mut c = dimension_from(&ps.sketch, &self.picks, cursor.pos).ok()?;
        let mut sk = ps.sketch.clone();
        if let Some(v) = c.value_mut()
            && *v == 0.0
        {
            *v = 1.0; // placeholder until measured (validation wants > 0)
        }
        let id = sk.add_constraint(c.clone()).ok()?;
        let measured = sk.measure(id)?;
        if let Some(v) = c.value_mut() {
            *v = measured;
        }
        let _ = sk.set_dimension(id, measured);
        sk.set_dimension_label(id, Some(cursor.pos));
        Some((sk, id))
    }

    /// Rubber-band geometry for the current step with the live cursor.
    pub fn preview(&self, ps: &PartSketch, cursor: &Pick) -> Vec<PreviewLine> {
        let mut out = Vec::new();
        // A pattern dialog: the result, as soon as its inputs are chosen.
        if self.pattern_dialog.is_some() {
            let mut tmp = ps.sketch.clone();
            if let Ok(sh) = self.dialog_build(&mut tmp) {
                for id in sh.entities {
                    out.push(PreviewLine {
                        pts: super::overlay::polyline(&tmp, id),
                        dashed: tmp.style(id) == Style::Construction,
                        warn: false,
                    });
                }
            }
            return out;
        }
        // Edit Pattern: the copies as they will be.
        if let Some((id, base)) = &self.edit_pattern {
            let mut tmp = ps.sketch.clone();
            if tmp
                .edit_pattern(*id, self.pattern_kind_from_fields(base))
                .is_ok()
                && let Some(p) = tmp.pattern(*id)
            {
                for e in p.instances.iter().flat_map(|i| i.entities.iter()) {
                    out.push(PreviewLine {
                        pts: super::overlay::polyline(&tmp, *e),
                        // Suppressed (skipped) elements stay dashed.
                        dashed: tmp.style(*e) == Style::Construction,
                        warn: false,
                    });
                }
            }
            return out;
        }
        let mut pos: Vec<Point2> = self.picks.iter().map(|p| p.pos).collect();
        match self.tool {
            Tool::Select | Tool::Constrain(_) | Tool::Dimension | Tool::CircleTangent => {
                return out;
            }
            Tool::Trim | Tool::Extend | Tool::Split => {
                if let Snap::OnEntity(e) = cursor.snap {
                    out.push(PreviewLine {
                        pts: super::overlay::polyline(&ps.sketch, e),
                        dashed: true,
                        warn: true,
                    });
                }
                return out;
            }
            Tool::Fillet | Tool::Chamfer => {
                let first = self.picks.first().map(en_of);
                let hover = match cursor.snap {
                    Snap::OnEntity(e) => Some(e),
                    _ => None,
                };
                for e in first.into_iter().chain(hover) {
                    out.push(PreviewLine {
                        pts: super::overlay::polyline(&ps.sketch, e),
                        dashed: true,
                        warn: false,
                    });
                }
                return out;
            }
            Tool::Offset => {
                if let Some(first) = self.picks.first() {
                    let e = en_of(first);
                    let mut tmp = ps.sketch.clone();
                    if let Ok(d) = tmp.offset_side(e, cursor.pos)
                        && let Ok(sh) = tmp.offset(e, d, self.offset_loop, false)
                    {
                        for id in sh.entities {
                            out.push(PreviewLine {
                                pts: super::overlay::polyline(&tmp, id),
                                dashed: false,
                                warn: false,
                            });
                        }
                    }
                } else if let Snap::OnEntity(e) = cursor.snap {
                    let chain: Vec<EntityId> = if self.offset_loop {
                        ps.sketch.chain(e).0.into_iter().map(|(id, _)| id).collect()
                    } else {
                        vec![e]
                    };
                    for id in chain {
                        out.push(PreviewLine {
                            pts: super::overlay::polyline(&ps.sketch, id),
                            dashed: true,
                            warn: false,
                        });
                    }
                }
                return out;
            }
            t if Self::is_transform(t) => {
                if !self.transform_ready {
                    return out;
                }
                let mut tmp = ps.sketch.clone();
                let mut pts: Vec<Point2> = self.picks.iter().map(|p| p.pos).collect();
                if t == Tool::Scale {
                    if tmp
                        .scale_geometry(&self.selected, cursor.pos, self.scale_factor)
                        .is_ok()
                    {
                        for &id in &self.selected {
                            out.push(PreviewLine {
                                pts: super::overlay::polyline(&tmp, id),
                                dashed: false,
                                warn: false,
                            });
                        }
                    }
                    return out;
                }
                if t == Tool::Stretch {
                    if let Some(base) = self.picks.first()
                        && tmp
                            .stretch_geometry(&self.selected, cursor.pos - base.pos)
                            .is_ok()
                    {
                        for (id, _) in tmp.entities() {
                            out.push(PreviewLine {
                                pts: super::overlay::polyline(&tmp, id),
                                dashed: false,
                                warn: false,
                            });
                        }
                    }
                    return out;
                }
                if t == Tool::Mirror {
                    if let Snap::OnEntity(line) = cursor.snap
                        && tmp.entity(line).is_some_and(Entity::is_line)
                        && let Ok(sh) = tmp.mirror_geometry(&self.selected, line)
                    {
                        for id in sh.entities {
                            out.push(PreviewLine {
                                pts: super::overlay::polyline(&tmp, id),
                                dashed: false,
                                warn: false,
                            });
                        }
                    }
                    return out;
                }
                pts.push(cursor.pos);
                let need = if t == Tool::Rotate { 3 } else { 2 };
                if pts.len() == need {
                    tmp.transform_geometry(&self.selected, transform_of(t, &pts));
                    for &id in &self.selected {
                        out.push(PreviewLine {
                            pts: super::overlay::polyline(&tmp, id),
                            dashed: false,
                            warn: false,
                        });
                    }
                }
                for w in pts.windows(2) {
                    out.push(PreviewLine {
                        pts: vec![w[0], w[1]],
                        dashed: true,
                        warn: false,
                    });
                }
                return out;
            }
            Tool::SplineCv | Tool::SplineInterp => {
                pos.push(cursor.pos);
                let interp = self.tool == Tool::SplineInterp;
                if pos.len() >= 2 {
                    if !interp {
                        out.push(PreviewLine {
                            pts: pos.clone(),
                            dashed: true,
                            warn: false,
                        });
                    }
                    let mut s = Sketch::default();
                    let ids: Vec<PointId> = pos.iter().map(|&p| s.add_point(p)).collect();
                    let sp = if interp {
                        s.add_interp_spline(ids)
                    } else {
                        s.add_spline(ids)
                    };
                    if let Ok(sp) = sp {
                        out.push(PreviewLine {
                            pts: super::overlay::polyline(&s, sp),
                            dashed: false,
                            warn: false,
                        });
                    }
                }
                return out;
            }
            Tool::Line if pos.len() == 1 => {
                out.push(PreviewLine {
                    pts: vec![pos[0], cursor.pos],
                    dashed: false,
                    warn: false,
                });
                return out;
            }
            Tool::ArcTangent if pos.len() == 1 => {
                if let Snap::Point(p) = self.picks[0].snap {
                    let mut tmp = ps.clone();
                    if tangent_arc(&mut tmp, p, cursor.pos, ArcKind::Tangent).is_ok()
                        && let Some((id, _)) = tmp.sketch.entities().last()
                    {
                        out.push(PreviewLine {
                            pts: super::overlay::polyline(&tmp.sketch, id),
                            dashed: false,
                            warn: false,
                        });
                    }
                }
                return out;
            }
            _ => {}
        }
        if pos.is_empty() {
            return out;
        }
        pos.push(cursor.pos);
        let needed = self.tool.clicks().unwrap_or(0);
        // Build the would-be result in a scratch sketch with the same code
        // that commits it, so the preview is exact.
        let mut s = Sketch::default();
        let built = if pos.len() == needed {
            build_any(
                &mut s,
                self.tool,
                &pos,
                self.polygon_sides,
                self.polygon_inscribed,
            )
        } else {
            // Not enough clicks yet: show the construction so far.
            for w in pos.windows(2) {
                out.push(PreviewLine {
                    pts: vec![w[0], w[1]],
                    dashed: true,
                    warn: false,
                });
            }
            match self.tool {
                Tool::CircleCenter | Tool::ArcCenter => {
                    let r = (pos[1] - pos[0]).norm();
                    out.push(PreviewLine {
                        pts: circle_points(pos[0], r),
                        dashed: true,
                        warn: false,
                    });
                }
                Tool::SlotCenterToCenter
                | Tool::SlotOverall
                | Tool::SlotCenterPoint
                | Tool::SlotArcCenter
                    if pos.len() == needed - 1 =>
                {
                    let w = match self.tool {
                        Tool::SlotArcCenter => ((pos[2] - pos[0]).norm()
                            - (pos[1] - pos[0]).norm())
                        .abs()
                        .max(1.0),
                        _ => ((pos[1] - pos[0]).norm() / 4.0).max(1.0),
                    };
                    let mut probe = pos.clone();
                    probe.push(width_point(self.tool, &pos, w));
                    let _ = build_any(
                        &mut s,
                        self.tool,
                        &probe,
                        self.polygon_sides,
                        self.polygon_inscribed,
                    );
                }
                _ => {}
            }
            Ok(())
        };
        if built.is_ok() {
            for (id, _) in s.entities() {
                out.push(PreviewLine {
                    pts: super::overlay::polyline(&s, id),
                    dashed: s.style(id) != Style::Normal,
                    warn: false,
                });
            }
        }
        out
    }
}

// ---- helpers ---------------------------------------------------------------

/// Runs `f`, keeps the result only if the sketch still solves; new entities
/// get the construction style when `construction` is on.
fn commit(
    ps: &mut PartSketch,
    label: &str,
    construction: bool,
    f: impl FnOnce(&mut PartSketch) -> Result<(), SketchError>,
) -> ClickResult {
    let before = ps.sketch.clone();
    let old: Vec<EntityId> = ps.sketch.entities().map(|(id, _)| id).collect();
    if let Err(e) = f(ps) {
        ps.sketch = before;
        return Err(e.to_string());
    }
    if construction {
        let new: Vec<EntityId> = ps
            .sketch
            .entities()
            .map(|(id, _)| id)
            .filter(|id| !old.contains(id))
            .collect();
        for id in new {
            let _ = ps.sketch.set_style(id, Style::Construction);
        }
    }
    match ps.sketch.solve() {
        Ok(r) if r.status == SolveStatus::Solved => Ok(Some(label.to_owned())),
        _ => {
            ps.sketch = before;
            Err(format!(
                "{label}: the result conflicts with existing constraints"
            ))
        }
    }
}

/// The point mapping of Move/Copy (base → destination) or Rotate
/// (centre, reference, target).
fn transform_of(tool: Tool, p: &[Point2]) -> impl Fn(Point2) -> Point2 + use<> {
    let c = p[0];
    let angle = if tool == Tool::Rotate {
        let (u, v) = (p[1] - p[0], p[2] - p[0]);
        v.y.atan2(v.x) - u.y.atan2(u.x)
    } else {
        0.0
    };
    let delta = if tool == Tool::Rotate {
        Vector2::zeros()
    } else {
        p[1] - p[0]
    };
    let rotate = tool == Tool::Rotate;
    move |q: Point2| {
        if rotate {
            let (s, co) = angle.sin_cos();
            let v = q - c;
            c + Vector2::new(co * v.x - s * v.y, s * v.x + co * v.y)
        } else {
            q + delta
        }
    }
}

/// Moves the selection and lets the rest of the sketch follow while the
/// moved points are held (like dragging); the caller's solve then frees them.
fn move_held(
    sk: &mut Sketch,
    sel: &[EntityId],
    f: impl Fn(Point2) -> Point2,
) -> Result<(), SketchError> {
    let pts: Vec<PointId> = sk
        .points_of(sel)
        .into_iter()
        .filter(|&p| !sk.point(p).is_some_and(|q| q.fixed))
        .collect();
    sk.transform_geometry(sel, f);
    for &p in &pts {
        sk.set_fixed(p, true)?;
    }
    let _ = sk.solve();
    for &p in &pts {
        sk.set_fixed(p, false)?;
    }
    Ok(())
}

/// Number of redundant equations of a sketch as it stands.
fn redundancy(sketch: &Sketch) -> usize {
    sketch.clone().solve().map_or(0, |r| r.redundant)
}

/// Adds constraints; refuses them (restoring the sketch) if the sketch
/// would fail or gain redundant equations — Inventor's "this constraint
/// would over-constrain the sketch".
fn commit_constraints(
    ps: &mut PartSketch,
    label: &str,
    f: impl FnOnce(&mut Sketch) -> Result<(), SketchError>,
) -> ClickResult {
    let before = ps.sketch.clone();
    let redundant_before = redundancy(&before);
    if let Err(e) = f(&mut ps.sketch) {
        ps.sketch = before;
        return Err(e.to_string());
    }
    match ps.sketch.solve() {
        Ok(r) if r.status == SolveStatus::Solved && r.redundant <= redundant_before => {
            Ok(Some(label.to_owned()))
        }
        Ok(r) if r.status == SolveStatus::Solved => {
            ps.sketch = before;
            Err(format!(
                "{label} would over-constrain the sketch (not applied)"
            ))
        }
        _ => {
            ps.sketch = before;
            Err(format!(
                "{label} conflicts with existing constraints (not applied)"
            ))
        }
    }
}

/// The entity of an entity pick. Callers check the pick kind first; any
/// other pick yields the null key, which every sketch lookup rejects.
fn en_of(p: &Pick) -> EntityId {
    match p.snap {
        Snap::OnEntity(e) => e,
        _ => EntityId::default(),
    }
}

/// Fix a point where it is now.
fn fix(sk: &Sketch, p: PointId) -> Constraint {
    Constraint::Fix {
        point: p,
        at: sk.point(p).map_or(Point2::origin(), |q| q.position),
    }
}

/// A point for a pick: shares the sketch's own snapped point, otherwise
/// creates one (constrained Coincident to projected points, or onto a curve).
fn endpoint(ps: &mut PartSketch, pick: &Pick, extra: &mut Vec<Constraint>) -> PointId {
    if let Snap::Point(id) = pick.snap
        && ps.sketch.point(id).is_some()
        && !ps.is_projected(id)
    {
        return id;
    }
    let p = ps.sketch.add_point(pick.pos);
    snap_constraint(ps, p, pick, extra);
    p
}

/// A drag handle of a sketch pattern (Edit Pattern).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PatHandle {
    /// What it changes.
    pub grip: PatGrip,
    /// Where its stem starts (sketch coordinates).
    pub from: Point2,
    /// The grip.
    pub pos: Point2,
    /// Direction of a linear handle (zero for angular ones).
    pub dir: Vector2,
}

/// What a pattern handle changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatGrip {
    /// Spacing of direction k.
    Spacing(usize),
    /// Count of direction k (circular: 0).
    Count(usize),
    /// Circular step angle.
    Angle,
}

/// The centre of a pattern's original geometry (its points' average).
fn pattern_seed(sk: &Sketch, id: crate::sketch::PatternId) -> Option<Point2> {
    let p = sk.pattern(id)?;
    let pts = sk.points_of(&p.source);
    if pts.is_empty() {
        return None;
    }
    let sum = pts
        .iter()
        .filter_map(|&q| sk.point(q))
        .fold(Vector2::zeros(), |a, q| a + q.position.coords);
    Some(Point2::from(sum / pts.len() as Real))
}

/// Unit direction of a pattern row.
fn direction_unit(sk: &Sketch, d: &PatternDirection) -> Option<Vector2> {
    let (a, b) = sk.line_geometry(d.line)?;
    let u = (b - a).try_normalize(1e-12)?;
    Some(if d.flip { -u } else { u })
}

/// The constraint that ties a new point to what its pick snapped to.
fn snap_constraint(ps: &PartSketch, p: PointId, pick: &Pick, extra: &mut Vec<Constraint>) {
    match pick.snap {
        Snap::Point(q) if q != p && ps.sketch.point(q).is_some() => {
            extra.push(Constraint::Coincident { a: p, b: q })
        }
        Snap::OnEntity(e) | Snap::Tangent(e) => extra.extend(ps.sketch.on_curve(p, e)),
        Snap::Midpoint(line) => extra.push(Constraint::Midpoint { point: p, line }),
        _ => {}
    }
}

/// A line or arc that ends at point `p`.
fn curve_ending_at(sk: &Sketch, p: PointId) -> Option<EntityId> {
    sk.entities()
        .find(|(_, e)| {
            (e.is_line() || matches!(e, Entity::Arc { .. })) && e.endpoints().contains(&p)
        })
        .map(|(id, _)| id)
}

/// Unit direction in which a curve *continues* beyond its end `p`.
fn continuation(sk: &Sketch, e: EntityId, p: PointId) -> Option<Vector2> {
    let pos = |q: PointId| sk.point(q).map(|x| x.position);
    match *sk.entity(e)? {
        Entity::Line { start, end } => {
            let other = if end == p { start } else { end };
            (pos(p)? - pos(other)?).try_normalize(1e-12)
        }
        Entity::Arc { center, start, end } => {
            let radial = pos(p)? - pos(center)?;
            let ccw = Vector2::new(-radial.y, radial.x); // CCW tangent
            let t = if p == end {
                ccw
            } else if p == start {
                -ccw
            } else {
                return None;
            };
            t.try_normalize(1e-12)
        }
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ArcKind {
    Tangent,
    Perpendicular,
}

/// Creates an arc from the end `from` of a line/arc to `to`, tangent or
/// perpendicular to that curve; returns the arc's far end point.
fn tangent_arc(
    ps: &mut PartSketch,
    from: PointId,
    to: Point2,
    kind: ArcKind,
) -> Result<PointId, SketchError> {
    let e = curve_ending_at(&ps.sketch, from)
        .ok_or(SketchError::InvalidValue("not the end of a line or arc"))?;
    let t = continuation(&ps.sketch, e, from)
        .ok_or(SketchError::InvalidValue("not the end of a line or arc"))?;
    let p = ps
        .sketch
        .point(from)
        .ok_or(SketchError::UnknownPoint)?
        .position;
    let chord = to - p;
    // Direction the arc leaves `from` in.
    let d = match kind {
        ArcKind::Tangent => t,
        ArcKind::Perpendicular => {
            let n = Vector2::new(-t.y, t.x);
            if n.dot(&chord) >= 0.0 { n } else { -n }
        }
    };
    let n = Vector2::new(-d.y, d.x);
    let denom = 2.0 * n.dot(&chord);
    if denom.abs() < 1e-9 * chord.norm().max(1.0) {
        return Err(SketchError::InvalidValue(
            "arc end lies on the tangent line",
        ));
    }
    let s = chord.norm_squared() / denom; // signed radius, centre at p + n·s
    let c = ps.sketch.add_point(p + n * s);
    let q = ps.sketch.add_point(to);
    let arc = if s > 0.0 {
        ps.sketch.add_arc(c, from, q)?
    } else {
        ps.sketch.add_arc(c, q, from)?
    };
    match kind {
        ArcKind::Tangent => {
            ps.sketch.add_tangent(e, arc)?;
        }
        // Perpendicular at `from` ⇔ the centre lies on the line through it.
        ArcKind::Perpendicular => {
            if ps.sketch.entity(e).is_some_and(Entity::is_line) {
                ps.sketch
                    .add_constraint(Constraint::PointOnLine { point: c, line: e })?;
            }
        }
    }
    Ok(q)
}

/// Centre of the circle through three points.
fn circumcenter(a: Point2, b: Point2, c: Point2) -> Option<Point2> {
    let (b, c) = (b - a, c - a);
    let d = 2.0 * b.perp(&c);
    if d.abs() < 1e-12 {
        return None;
    }
    let ux = (c.y * b.norm_squared() - b.y * c.norm_squared()) / d;
    let uy = (b.x * c.norm_squared() - c.x * b.norm_squared()) / d;
    Some(a + Vector2::new(ux, uy))
}

fn circle_points(c: Point2, r: Real) -> Vec<Point2> {
    (0..=64)
        .map(|i| {
            let a = 2.0 * PI * i as Real / 64.0;
            c + Vector2::new(a.cos(), a.sin()) * r
        })
        .collect()
}

/// A plausible width click for previewing a slot before its last click.
fn width_point(tool: Tool, pos: &[Point2], w: Real) -> Point2 {
    match tool {
        Tool::SlotArcCenter => {
            pos[0] + (pos[1] - pos[0]).normalize() * ((pos[1] - pos[0]).norm() + w / 2.0)
        }
        _ => {
            let d = (pos[1] - pos[0])
                .try_normalize(1e-12)
                .unwrap_or(Vector2::x());
            pos[0] + Vector2::new(-d.y, d.x) * (w / 2.0)
        }
    }
}

/// Composite shapes for both commit and preview.
fn build_shape(
    s: &mut Sketch,
    tool: Tool,
    p: &[Point2],
    sides: usize,
    inscribed: bool,
) -> Result<crate::sketch::Shape, SketchError> {
    // Slot width: twice the distance of the last click from the slot axis.
    let width_from_axis = |a: Point2, b: Point2, w: Point2| {
        let d = (b - a).try_normalize(1e-12).unwrap_or(Vector2::x());
        2.0 * d.perp(&(w - a)).abs()
    };
    match tool {
        Tool::RectTwoPoint => s.add_rectangle_two_point(p[0], p[1]),
        Tool::RectThreePoint => s.add_rectangle_three_point(p[0], p[1], p[2]),
        Tool::RectCenter => s.add_rectangle_center(p[0], p[1]),
        Tool::RectCenterThreePoint => s.add_rectangle_center_three_point(p[0], p[1], p[2]),
        Tool::SlotCenterToCenter => {
            s.add_slot_center_to_center(p[0], p[1], width_from_axis(p[0], p[1], p[2]))
        }
        Tool::SlotOverall => s.add_slot_overall(p[0], p[1], width_from_axis(p[0], p[1], p[2])),
        Tool::SlotCenterPoint => {
            s.add_slot_center_point(p[0], p[1], width_from_axis(p[0], p[1], p[2]))
        }
        Tool::SlotArcThreePoint => {
            let c = circumcenter(p[0], p[1], p[2])
                .ok_or(SketchError::InvalidValue("the three points are collinear"))?;
            // Sweep from start to end through the middle click.
            let ccw = (p[1] - p[0]).perp(&(p[2] - p[0])) < 0.0;
            let (start, end) = if ccw { (p[0], p[1]) } else { (p[1], p[0]) };
            let w = 2.0 * ((p[3] - c).norm() - (start - c).norm()).abs();
            s.add_arc_slot(c, start, end, w)
        }
        Tool::SlotArcCenter => {
            let ccw = (p[1] - p[0]).perp(&(p[2] - p[0])) >= 0.0;
            let r = (p[1] - p[0]).norm();
            let w = 2.0 * ((p[3] - p[0]).norm() - r).abs();
            if ccw {
                s.add_arc_slot(p[0], p[1], p[2], w)
            } else {
                let end_dir = (p[2] - p[0]).normalize() * r;
                s.add_arc_slot(p[0], p[0] + end_dir, p[1], w)
            }
        }
        Tool::Polygon => s.add_polygon(p[0], p[1], sides, inscribed),
        _ => Err(SketchError::InvalidValue("not a shape tool")),
    }
}

/// Preview builder for any fixed-click tool.
fn build_any(
    s: &mut Sketch,
    tool: Tool,
    p: &[Point2],
    sides: usize,
    inscribed: bool,
) -> Result<(), SketchError> {
    match tool {
        Tool::Point => {
            s.add_point(p[0]);
        }
        Tool::CircleCenter => {
            s.add_circle(p[0], (p[1] - p[0]).norm().max(1e-6))?;
        }
        Tool::ArcThreePoint => {
            let c = circumcenter(p[0], p[1], p[2]).ok_or(SketchError::InvalidValue("collinear"))?;
            let (a, b, pc) = (s.add_point(p[0]), s.add_point(p[1]), s.add_point(c));
            if (p[1] - p[0]).perp(&(p[2] - p[0])) < 0.0 {
                s.add_arc(pc, a, b)?
            } else {
                s.add_arc(pc, b, a)?
            };
        }
        Tool::ArcCenter => {
            let r = (p[1] - p[0]).norm();
            let e = p[0] + (p[2] - p[0]).try_normalize(1e-12).unwrap_or(Vector2::x()) * r;
            let (c, a, b) = (s.add_point(p[0]), s.add_point(p[1]), s.add_point(e));
            if (p[1] - p[0]).perp(&(p[2] - p[0])) >= 0.0 {
                s.add_arc(c, a, b)?
            } else {
                s.add_arc(c, b, a)?
            };
        }
        _ => {
            build_shape(s, tool, p, sides, inscribed)?;
        }
    }
    Ok(())
}

/// Distance dimension between two points, oriented by the placement click
/// (Inventor): above/below the pair → horizontal, beside it → vertical,
/// otherwise aligned.
fn distance_by_placement(sk: &Sketch, p: PointId, q: PointId, place: Point2) -> Constraint {
    let (a, b) = (
        sk.point(p).map_or(Point2::origin(), |x| x.position),
        sk.point(q).map_or(Point2::origin(), |x| x.position),
    );
    let within = |v: Real, lo: Real, hi: Real| v >= lo.min(hi) && v <= lo.max(hi);
    let in_x = within(place.x, a.x, b.x);
    let in_y = within(place.y, a.y, b.y);
    if in_x && !in_y && (b.x - a.x).abs() > 1e-6 {
        let (l, r) = if a.x <= b.x { (p, q) } else { (q, p) };
        Constraint::HorizontalDistance {
            a: l,
            b: r,
            value: 1.0,
        }
    } else if in_y && !in_x && (b.y - a.y).abs() > 1e-6 {
        let (lo, hi) = if a.y <= b.y { (p, q) } else { (q, p) };
        Constraint::VerticalDistance {
            a: lo,
            b: hi,
            value: 1.0,
        }
    } else {
        Constraint::Distance {
            a: p,
            b: q,
            value: 1.0,
        }
    }
}

/// Symmetric constraints for two lines about a third: endpoints paired so
/// each mirrors its nearest counterpart.
fn symmetric_lines(sk: &Sketch, a: EntityId, b: EntityId, axis: EntityId) -> Vec<Constraint> {
    let (Ok((a0, a1)), Ok((b0, b1))) = (sk.line_points(a), sk.line_points(b)) else {
        return Vec::new();
    };
    let pos = |p: PointId| sk.point(p).map_or(Point2::origin(), |x| x.position);
    let Some((l0, l1)) = sk.line_geometry(axis) else {
        return Vec::new();
    };
    let mirror = |q: Point2| {
        let d = (l1 - l0).normalize();
        let v = q - l0;
        l0 + d * (2.0 * v.dot(&d)) - v
    };
    let straight = (mirror(pos(a0)) - pos(b0)).norm() + (mirror(pos(a1)) - pos(b1)).norm();
    let crossed = (mirror(pos(a0)) - pos(b1)).norm() + (mirror(pos(a1)) - pos(b0)).norm();
    let (q0, q1) = if straight <= crossed {
        (b0, b1)
    } else {
        (b1, b0)
    };
    vec![
        Constraint::Symmetric {
            a: a0,
            b: q0,
            line: axis,
        },
        Constraint::Symmetric {
            a: a1,
            b: q1,
            line: axis,
        },
    ]
}

/// Text for a dimension value (degrees for angles).
pub fn format_value(v: Real, is_angle: bool) -> String {
    if is_angle {
        format!("{:.2}", v.to_degrees().abs())
    } else {
        format!("{v:.3}")
    }
}

/// The dimension the selection `picks` gives when placed at `place`
/// (Inventor's General Dimension rules, see `docs/spec/1.4-dimensions.md`).
fn dimension_from(sk: &Sketch, picks: &[Pick], place: Point2) -> Result<Constraint, String> {
    let is = |s: Snap, f: fn(&Entity) -> bool| matches!(s, Snap::OnEntity(e) if sk.entity(e).is_some_and(f));
    let centerline = |s: Snap| matches!(s, Snap::OnEntity(e) if sk.entity(e).is_some_and(Entity::is_line) && sk.style(e) == Style::Centerline);
    // To a centerline, placed on its far side: a diametric dimension (the
    // diameter a revolve gives), else a radial one.
    let to_axis = |p: PointId, axis: EntityId| -> Option<Constraint> {
        let (l0, l1) = sk.line_geometry(axis)?;
        let d = l1 - l0;
        let q = sk.point(p)?.position;
        let side = |x: Point2| d.perp(&(x - l0));
        let value = 2.0 * (side(q) / d.norm().max(1e-12)).abs();
        (side(q) * side(place) < 0.0 && value > 1e-9).then_some(Constraint::DiameterToAxis {
            point: p,
            line: axis,
            value,
        })
    };
    if let [a, b] = picks {
        let (other, axis) = if centerline(b.snap) && !centerline(a.snap) {
            (a, b)
        } else {
            (b, a)
        };
        if centerline(axis.snap) {
            let axis_e = en_of(axis);
            let p = match other.snap {
                Snap::Point(p) => Some(p),
                Snap::OnEntity(e) if sk.entity(e).is_some_and(Entity::is_line) => {
                    sk.line_points(e).ok().map(|(p, _)| p)
                }
                Snap::OnEntity(e) if sk.entity(e).is_some_and(Entity::is_circular) => {
                    match sk.entity(e) {
                        Some(Entity::Circle { center, .. } | Entity::Arc { center, .. }) => {
                            Some(*center)
                        }
                        _ => None,
                    }
                }
                _ => None,
            };
            if let Some(c) = p.and_then(|p| to_axis(p, axis_e)) {
                return Ok(c);
            }
        }
    }
    Ok(match picks {
        [] => return Err("Select geometry first".into()),
        [a] if is(a.snap, |e| matches!(e, Entity::Circle { .. })) => {
            let e = en_of(a);
            Constraint::Diameter {
                circle: e,
                value: 2.0 * sk.circle_geometry(e).map_or(1.0, |c| c.1),
            }
        }
        [a] if is(a.snap, |e| matches!(e, Entity::Arc { .. })) => {
            let e = en_of(a);
            Constraint::Radius {
                circle: e,
                value: sk.circle_geometry(e).map_or(1.0, |c| c.1),
            }
        }
        [a] if is(a.snap, Entity::is_line) => {
            let (p, q) = sk.line_points(en_of(a)).map_err(|e| e.to_string())?;
            distance_by_placement(sk, p, q, place)
        }
        [a, b] => match (a.snap, b.snap) {
            (Snap::Point(p), Snap::Point(q)) => distance_by_placement(sk, p, q, place),
            (Snap::Point(p), Snap::OnEntity(l)) | (Snap::OnEntity(l), Snap::Point(p))
                if sk.entity(l).is_some_and(Entity::is_line) =>
            {
                Constraint::PointLineDistance {
                    point: p,
                    line: l,
                    value: 1.0,
                }
            }
            (Snap::OnEntity(l1), Snap::OnEntity(l2))
                if is(a.snap, Entity::is_line) && is(b.snap, Entity::is_line) =>
            {
                let (a0, a1) = sk.line_geometry(l1).ok_or("missing line")?;
                let (b0, b1) = sk.line_geometry(l2).ok_or("missing line")?;
                let (da, db) = (a1 - a0, b1 - b0);
                let sin = da.perp(&db) / (da.norm() * db.norm()).max(1e-12);
                if sin.abs() < 1e-3 {
                    let (p, _) = sk.line_points(l2).map_err(|e| e.to_string())?;
                    Constraint::PointLineDistance {
                        point: p,
                        line: l1,
                        value: 1.0,
                    }
                } else {
                    // The quadrant of the placement picks θ or 180° − θ.
                    let (value, supplement) = sk
                        .angle_for_placement(l1, l2, place)
                        .unwrap_or((0.0, false));
                    Constraint::Angle {
                        a: l1,
                        b: l2,
                        value,
                        supplement,
                    }
                }
            }
            _ => return Err("This selection cannot be dimensioned".into()),
        },
        _ => return Err("This selection cannot be dimensioned".into()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::part::PartSketch;
    use approx::assert_relative_eq;

    fn pick(x: Real, y: Real) -> Pick {
        Pick {
            pos: Point2::new(x, y),
            snap: Snap::None,
            infer: None,
            align: Align::default(),
        }
    }

    fn sketch() -> PartSketch {
        PartSketch::new("S", Sketch::default())
    }

    fn at_point(ps: &PartSketch, id: PointId) -> Pick {
        Pick {
            pos: ps.sketch.point(id).unwrap().position,
            snap: Snap::Point(id),
            infer: None,
            align: Align::default(),
        }
    }

    /// Regression: the Rectangular Pattern preview with the cursor off any
    /// line (no direction yet) indexed an empty list and crashed the app.
    #[test]
    fn rect_pattern_preview_without_direction_does_not_panic() {
        let mut ps = sketch();
        let l = ps
            .sketch
            .add_line(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
        let mut t = SketchTools {
            selected: vec![l],
            ..Default::default()
        };
        t.set_tool(Tool::PatternRect);
        t.selected = vec![l];
        assert!(t.preview(&ps, &pick(50.0, 50.0)).is_empty());
    }

    #[test]
    fn move_with_copy_keeps_the_original() {
        let mut ps = sketch();
        let l = ps
            .sketch
            .add_line(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
        let mut t = SketchTools {
            selected: vec![l],
            ..Default::default()
        };
        t.set_tool(Tool::Move);
        t.move_copy = true;
        t.click(&mut ps, pick(0.0, 0.0)).unwrap();
        t.click(&mut ps, pick(0.0, 5.0)).unwrap();
        assert_eq!(ps.sketch.entities().count(), 2, "original + copy");
        let (a, _) = ps.sketch.line_geometry(l).unwrap();
        assert_relative_eq!(a.y, 0.0, epsilon = 1e-9);
    }

    #[test]
    fn circular_pattern_fitted_and_step_angles() {
        // Count 3, angle 45°: fitted → last copy at 45°; not fitted → at 90°.
        for (fitted, last_deg) in [(true, 45.0), (false, 90.0)] {
            let mut ps = sketch();
            let c = ps.sketch.add_fixed_point(Point2::new(0.0, 0.0));
            let hole = ps.sketch.add_circle(Point2::new(10.0, 0.0), 1.0).unwrap();
            let mut t = SketchTools {
                selected: vec![hole],
                ..Default::default()
            };
            t.set_tool(Tool::PatternCircular);
            (t.circular_count, t.circular_angle_deg, t.circular_fitted) = (3, 45.0, fitted);
            let p = at_point(&ps, c);
            t.click(&mut ps, p).unwrap(); // fills the Centre selector
            t.finish(&mut ps).unwrap(); // Enter = OK
            let far = ps
                .sketch
                .entities()
                .filter(|(_, e)| matches!(e, Entity::Circle { .. }))
                .filter_map(|(id, _)| ps.sketch.circle_geometry(id))
                .map(|(p, _)| p.y.atan2(p.x).to_degrees())
                .fold(0.0, Real::max);
            assert_relative_eq!(far, last_deg, epsilon = 1e-6);
        }
    }

    #[test]
    fn chamfer_tool_makes_later_equal_chamfers_follow_the_first() {
        let mut ps = sketch();
        let sh = ps
            .sketch
            .add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(40.0, 20.0))
            .unwrap();
        let mut t = SketchTools::default();
        t.set_tool(Tool::Chamfer);
        t.chamfer_mode = ChamferMode::Equal;
        for corner in [sh.points[0], sh.points[2]] {
            let p = at_point(&ps, corner);
            t.click(&mut ps, p).unwrap();
        }
        let dims = ps
            .sketch
            .constraints()
            .filter(|(_, c)| matches!(c, Constraint::Distance { .. }))
            .count();
        assert_eq!(dims, 1, "the second chamfer is Equal to the first");
        assert_eq!(ps.sketch.solve().unwrap().redundant, 0);
    }

    #[test]
    fn inference_settings_gate_constraints() {
        let mut ps = sketch();
        let mut t = SketchTools::default();
        t.inference.horizontal = false;
        t.set_tool(Tool::Line);
        t.click(&mut ps, pick(0.0, 0.0)).unwrap();
        let p = t.infer(&ps.sketch, pick(20.0, 0.5), false, 0.0);
        assert_eq!(p.infer, None, "Horizontal not inferred");
        // Without Persist an on-curve snap creates no constraint.
        t.inference = InferenceSettings {
            persist: false,
            ..Default::default()
        };
        let target = ps
            .sketch
            .add_line(Point2::new(0.0, 10.0), Point2::new(30.0, 10.0));
        t.set_tool(Tool::Line);
        let on = Pick {
            pos: Point2::new(15.0, 10.0),
            snap: Snap::OnEntity(target),
            infer: None,
            align: Align::default(),
        };
        t.click(&mut ps, on).unwrap();
        t.click(&mut ps, pick(15.0, 25.0)).unwrap();
        assert!(
            !ps.sketch
                .constraints()
                .any(|(_, c)| matches!(c, Constraint::PointOnLine { .. }))
        );
    }

    #[test]
    fn line_chain_shares_points_and_infers_horizontal() {
        let mut ps = sketch();
        let mut t = SketchTools::default();
        t.set_tool(Tool::Line);
        t.click(&mut ps, pick(0.0, 0.0)).unwrap();
        let p = t.infer(&ps.sketch, pick(20.0, 0.5), false, 0.0);
        assert_eq!(p.infer, Some(Infer::Horizontal));
        assert_eq!(t.click(&mut ps, p).unwrap().as_deref(), Some("Line"));
        t.click(&mut ps, pick(25.0, 15.0)).unwrap();
        assert_eq!(ps.sketch.entities().count(), 2);
        assert_eq!(
            ps.sketch.points().count(),
            3,
            "chain shares its middle point"
        );
        assert!(
            ps.sketch
                .constraints()
                .any(|(_, c)| matches!(c, Constraint::Horizontal { .. }))
        );
    }

    /// The pattern dialog: geometry by clicks, Enter to the next selector,
    /// the spline as path, Enter = OK (the user's case).
    #[test]
    fn path_pattern_dialog_on_a_spline() {
        let mut ps = sketch();
        let pts: Vec<PointId> = [(0.0, 0.0), (20.0, 15.0), (45.0, -5.0), (70.0, 10.0)]
            .iter()
            .map(|&(x, y)| ps.sketch.add_point(Point2::new(x, y)))
            .collect();
        let sp = ps.sketch.add_spline(pts).unwrap();
        let c = ps.sketch.add_circle(Point2::new(0.0, -10.0), 2.0).unwrap();
        let mut t = SketchTools::default();
        t.set_tool(Tool::PatternPath);
        let on = |e| Pick {
            pos: Point2::origin(),
            snap: Snap::OnEntity(e),
            infer: None,
            align: Align::default(),
        };
        // Geometry first (nothing pre-selected), then Enter → Path.
        t.click(&mut ps, on(c)).unwrap();
        assert_eq!(t.selected, vec![c]);
        assert!(t.picks_geometry(), "selectors pick curves");
        t.finish(&mut ps).unwrap();
        t.click(&mut ps, on(sp)).unwrap();
        assert!(t.pattern_dialog.as_ref().unwrap().path == Some(sp));
        assert!(!t.preview(&ps, &on(sp)).is_empty(), "preview before OK");
        assert_eq!(ps.sketch.patterns().count(), 0, "nothing created before OK");
        t.finish(&mut ps).unwrap();
        assert_eq!(ps.sketch.patterns().count(), 1);
        assert!(t.pattern_dialog.is_none());
    }

    /// Line tool: near the point where a line from the first click touches
    /// a circle, the pick snaps there (tangent snap) and the line ends on
    /// the circle, tangent to it.
    #[test]
    fn line_tangent_snap_to_a_circle() {
        let mut ps = sketch();
        let c = ps.sketch.add_circle(Point2::new(0.0, 0.0), 10.0).unwrap();
        let mut t = SketchTools::default();
        t.set_tool(Tool::Line);
        let free = |pos| Pick {
            pos,
            snap: Snap::None,
            infer: None,
            align: Align::default(),
        };
        t.click(&mut ps, free(Point2::new(30.0, 0.0))).unwrap();
        let touch = tangent_points(Point2::new(30.0, 0.0), Point2::new(0.0, 0.0), 10.0);
        let near = touch[0] + Vector2::new(0.3, -0.2);
        let raw = Pick {
            pos: near,
            snap: Snap::OnEntity(c),
            infer: None,
            align: Align::default(),
        };
        let pick = t.infer(&ps.sketch, raw, false, 0.1);
        assert_eq!(pick.snap, Snap::Tangent(c));
        assert!((pick.pos - touch[0]).norm() < 1e-9);
        // Far from the touching point it stays a plain on-curve pick.
        let far = Pick {
            pos: Point2::new(-10.0, 0.0),
            snap: Snap::OnEntity(c),
            infer: None,
            align: Align::default(),
        };
        assert_eq!(t.infer(&ps.sketch, far, false, 0.1).snap, Snap::OnEntity(c));
        t.click(&mut ps, pick).unwrap();
        assert!(
            ps.sketch
                .constraints()
                .any(|(_, k)| matches!(k, Constraint::Tangent { b, .. } if *b == c)),
            "tangent constraint"
        );
        let r = ps.sketch.solve().unwrap();
        assert_eq!(r.redundant, 0, "{r:?}");
    }

    /// A parameter name typed into Spacing stays on the pattern's spacing
    /// dimension after OK (direction 2 without one keeps its number).
    #[test]
    fn typed_equation_goes_onto_the_pattern_dimension() {
        let mut ps = sketch();
        let c = ps.sketch.add_circle(Point2::new(5.0, 5.0), 1.0).unwrap();
        let mut t = SketchTools {
            selected: vec![c],
            ..Default::default()
        };
        t.pattern_count = [3, 2];
        t.pattern_spacing = [10.0, 4.0];
        t.pattern_spacing_eq = [Some("w / 2".into()), None];
        t.set_tool(Tool::PatternRect);
        {
            use super::super::pattern_dialog::DirRef;
            let d = t.pattern_dialog.as_mut().expect("dialog opens");
            d.dir = [Some(DirRef::AxisX), Some(DirRef::AxisY)];
        }
        t.dialog_ok(&mut ps).unwrap();
        let (_, p) = ps.sketch.patterns().next().unwrap();
        let exprs: Vec<Option<String>> = p
            .constraints
            .iter()
            .filter(|&&k| ps.sketch.constraint(k).and_then(|c| c.value()).is_some())
            .map(|&k| ps.sketch.dim_param(k).expr)
            .collect();
        assert_eq!(exprs.first().cloned().flatten().as_deref(), Some("w / 2"));
        assert_eq!(exprs.get(1).cloned().flatten(), None);
        assert_eq!(t.pattern_spacing_eq, [None, None], "used up");
    }

    /// Patterns along the sketch axes and around a centre given by radius.
    #[test]
    fn axis_and_radius_patterns() {
        let mut ps = sketch();
        let c = ps.sketch.add_circle(Point2::new(5.0, 5.0), 1.0).unwrap();
        let mut t = SketchTools {
            selected: vec![c],
            ..Default::default()
        };
        t.pattern_count = [3, 2];
        t.pattern_spacing = [10.0, 4.0];
        t.set_tool(Tool::PatternRect);
        {
            use super::super::pattern_dialog::DirRef;
            let d = t.pattern_dialog.as_mut().expect("dialog opens");
            d.dir = [Some(DirRef::AxisX), Some(DirRef::AxisY)];
        }
        t.dialog_ok(&mut ps).unwrap();
        assert!(t.pattern_dialog.is_none(), "OK closes the dialog");
        ps.sketch.solve().unwrap();
        let (_, p) = ps.sketch.patterns().next().unwrap();
        assert_eq!(p.instances.len(), 5, "3 × 2 − original");
        let centres: Vec<Point2> = p
            .instances
            .iter()
            .filter_map(|i| match ps.sketch.entity(i.entities[0]) {
                Some(&Entity::Circle { center, .. }) => Some(ps.sketch.point(center)?.position),
                _ => None,
            })
            .collect();
        assert!(
            centres
                .iter()
                .any(|q| (q - Point2::new(25.0, 9.0)).norm() < 1e-6)
        );
        // Circular around a centre 20 mm to the right of the circle's centre.
        let mut ps = sketch();
        let c = ps.sketch.add_circle(Point2::new(0.0, 0.0), 1.0).unwrap();
        let mut t = SketchTools {
            selected: vec![c],
            ..Default::default()
        };
        t.circular_count = 4;
        t.circular_angle_deg = 360.0;
        t.circular_radius = 20.0;
        t.circular_ref_deg = 0.0;
        t.set_tool(Tool::PatternCircular);
        t.pattern_dialog.as_mut().unwrap().center =
            Some(super::super::pattern_dialog::CenterRef::Radius);
        t.dialog_ok(&mut ps).unwrap();
        let rep = ps.sketch.solve().unwrap();
        assert_eq!(rep.redundant, 0);
        let (_, p) = ps.sketch.patterns().next().unwrap();
        assert_eq!(p.instances.len(), 3);
        let opposite = p
            .instances
            .iter()
            .any(|i| match ps.sketch.entity(i.entities[0]) {
                Some(&Entity::Circle { center, .. }) => {
                    (ps.sketch.point(center).unwrap().position - Point2::new(40.0, 0.0)).norm()
                        < 1e-6
                }
                _ => false,
            });
        assert!(opposite, "half a turn around (20, 0) is (40, 0)");
    }

    /// Edit Pattern handles: dragging the spacing arrow and the count grip
    /// changes the dialog fields (the preview follows).
    #[test]
    fn pattern_handles_drag_spacing_and_count() {
        let mut ps = sketch();
        let dir = ps
            .sketch
            .add_line(Point2::new(0.0, -10.0), Point2::new(50.0, -10.0));
        let c = ps.sketch.add_circle(Point2::new(0.0, 0.0), 2.0).unwrap();
        let d1 = PatternDirection {
            line: dir,
            flip: false,
            count: 3,
            spacing: 10.0,
            fitted: false,
        };
        ps.sketch.rectangular_pattern(&[c], d1, None).unwrap();
        let (id, p) = ps.sketch.patterns().next().unwrap();
        let kind = p.kind;
        let mut t = SketchTools::default();
        t.start_edit_pattern(id, kind);
        let hs = t.pattern_handles(&ps.sketch);
        let sp = hs.iter().find(|h| h.grip == PatGrip::Spacing(0)).unwrap();
        assert!(
            (sp.pos - Point2::new(10.0, 0.0)).norm() < 1e-6,
            "at the first copy"
        );
        let cnt = hs.iter().find(|h| h.grip == PatGrip::Count(0)).unwrap();
        assert!(
            (cnt.pos - Point2::new(25.0, 0.0)).norm() < 1e-6,
            "half a step beyond"
        );
        t.drag_pattern_handle(&ps.sketch, PatGrip::Spacing(0), Point2::new(12.34, 3.0));
        assert!((t.pattern_spacing[0] - 12.3).abs() < 1e-9);
        t.drag_pattern_handle(&ps.sketch, PatGrip::Count(0), Point2::new(55.0, 0.0));
        assert_eq!(t.pattern_count[0], 5, "55 / 12.3 + 0.5 → 5");
    }

    /// Point alignment: a line ending level with another line's end gets
    /// Horizontal between the two points (the user's example).
    #[test]
    fn line_end_aligns_with_an_existing_point() {
        let mut ps = sketch();
        let first = ps
            .sketch
            .add_line(Point2::new(0.0, 0.0), Point2::new(0.0, 10.0));
        let (_, top) = ps.sketch.line_points(first).unwrap();
        let mut t = SketchTools::default();
        t.set_tool(Tool::Line);
        t.click(&mut ps, pick(20.0, 0.0)).unwrap();
        // 0.2 mm off the level of (0, 10), 0.1 mm per pixel: within 6 px.
        let p = t.infer(&ps.sketch, pick(23.0, 10.2), false, 0.1);
        assert_eq!(p.align.y, Some(top));
        assert_eq!(p.pos.y, 10.0);
        assert_eq!(t.click(&mut ps, p).unwrap().as_deref(), Some("Line"));
        assert!(ps.sketch.constraints().any(|(_, c)| matches!(
            c,
            Constraint::HorizontalPoints { a, .. } if *a == top
        )));
        // Ctrl suppresses it; far away nothing aligns.
        let q = t.infer(&ps.sketch, pick(40.0, 10.2), true, 0.1);
        assert_eq!(q.align, Align::default());
        let q = t.infer(&ps.sketch, pick(40.0, 13.0), false, 0.1);
        assert_eq!(q.align.y, None);
    }

    #[test]
    fn line_infers_perpendicular_and_snaps_to_midpoints() {
        let mut ps = sketch();
        let mut t = SketchTools::default();
        t.set_tool(Tool::Line);
        t.click(&mut ps, pick(0.0, 0.0)).unwrap();
        t.click(&mut ps, pick(20.0, 10.0)).unwrap(); // slanted: no H/V
        // Roughly perpendicular to it (direction (−1, 2) ± 2°).
        let p = t.infer(&ps.sketch, pick(20.0 - 5.2, 10.0 + 9.9), false, 0.0);
        assert!(matches!(p.infer, Some(Infer::Perpendicular(_))), "{p:?}");
        t.click(&mut ps, p).unwrap();
        let r = ps.sketch.solve().unwrap();
        assert_eq!(r.redundant, 0);
        assert!(
            ps.sketch
                .constraints()
                .any(|(_, c)| matches!(c, Constraint::Perpendicular { .. }))
        );
        // A new line from the middle of the first one gets a Midpoint.
        t.set_tool(Tool::Line);
        let first = ps.sketch.entities().next().unwrap().0;
        let mid = Pick {
            pos: Point2::new(10.0, 5.0),
            snap: Snap::Midpoint(first),
            infer: None,
            align: Align::default(),
        };
        t.click(&mut ps, mid).unwrap();
        t.click(&mut ps, pick(10.0, -20.0)).unwrap();
        assert!(
            ps.sketch
                .constraints()
                .any(|(_, c)| matches!(c, Constraint::Midpoint { line, .. } if *line == first))
        );
    }

    #[test]
    fn drag_from_line_end_makes_tangent_or_perpendicular_arc() {
        for (dir, expect_tangent) in [
            (Vector2::new(1.0, 0.1), true),
            (Vector2::new(0.1, 1.0), false),
        ] {
            let mut ps = sketch();
            let mut t = SketchTools::default();
            t.set_tool(Tool::Line);
            t.click(&mut ps, pick(0.0, 0.0)).unwrap();
            t.click(&mut ps, pick(20.0, 0.0)).unwrap();
            let end = ps
                .sketch
                .points()
                .find(|(_, p)| p.position.x == 20.0)
                .unwrap()
                .0;
            let label = t
                .arc_drag(&mut ps, end, dir, Point2::new(30.0, 10.0))
                .unwrap()
                .unwrap();
            assert_eq!(label == "Tangent Arc", expect_tangent);
            let arc = ps
                .sketch
                .entities()
                .find(|(_, e)| matches!(e, Entity::Arc { .. }))
                .unwrap()
                .0;
            let (c, _) = ps.sketch.circle_geometry(arc).unwrap();
            if expect_tangent {
                // Centre straight above the shared point: tangent to the line.
                assert_relative_eq!(c.x, 20.0, epsilon = 1e-6);
            } else {
                // Centre on the line itself: perpendicular.
                assert_relative_eq!(c.y, 0.0, epsilon = 1e-6);
            }
        }
    }

    #[test]
    fn constraint_tools_refuse_over_constraining() {
        let mut ps = sketch();
        let mut t = SketchTools::default();
        t.set_tool(Tool::RectTwoPoint);
        t.click(&mut ps, pick(0.0, 0.0)).unwrap();
        t.click(&mut ps, pick(30.0, 20.0)).unwrap();
        let lines: Vec<EntityId> = ps.sketch.entities().map(|(id, _)| id).collect();
        // Bottom ∥ top is implied by H/H → refused, sketch unchanged.
        t.set_tool(Tool::Constrain(ConstraintTool::Parallel));
        let on = |e| Pick {
            pos: Point2::origin(),
            snap: Snap::OnEntity(e),
            infer: None,
            align: Align::default(),
        };
        t.click(&mut ps, on(lines[0])).unwrap();
        let n = ps.sketch.constraints().count();
        assert!(t.click(&mut ps, on(lines[2])).is_err());
        assert_eq!(ps.sketch.constraints().count(), n);
        // Equal bottom/left is fine (makes a square).
        t.set_tool(Tool::Constrain(ConstraintTool::Equal));
        t.click(&mut ps, on(lines[0])).unwrap();
        assert!(t.click(&mut ps, on(lines[3])).unwrap().is_some());
    }

    #[test]
    fn dimension_kind_follows_selection_and_placement() {
        let mut ps = sketch();
        let mut t = SketchTools::default();
        t.set_tool(Tool::Line);
        t.click(&mut ps, pick(0.0, 0.0)).unwrap();
        t.click(&mut ps, pick(30.0, 10.0)).unwrap();
        let line = ps.sketch.entities().next().unwrap().0;
        t.set_tool(Tool::Dimension);
        t.click(
            &mut ps,
            Pick {
                pos: Point2::new(15.0, 5.0),
                snap: Snap::OnEntity(line),
                infer: None,
                align: Align::default(),
            },
        )
        .unwrap();
        // Placed above the line, within its x-range → horizontal distance 30.
        t.click(&mut ps, pick(15.0, 25.0)).unwrap();
        let (_, c) = ps.sketch.constraints().last().unwrap();
        assert!(
            matches!(c, Constraint::HorizontalDistance { value, .. } if (value - 30.0).abs() < 1e-9)
        );
        // Its text sits where it was placed; the editor opens on it.
        let (id, _) = ps.sketch.constraints().last().unwrap();
        assert_eq!(ps.sketch.dimension_label(id), Some(Point2::new(15.0, 25.0)));
        assert!(t.dim_edit.is_some());
        ps.sketch.set_dimension_nominal(id, 40.0).unwrap();
        ps.sketch.solve().unwrap();
        let (a, b) = ps.sketch.line_geometry(line).unwrap();
        assert_relative_eq!((b.x - a.x).abs(), 40.0, epsilon = 1e-6);
    }

    #[test]
    fn dimension_to_a_centerline_is_diametric_on_the_far_side() {
        let mut ps = sketch();
        let axis = ps
            .sketch
            .add_line(Point2::new(0.0, -20.0), Point2::new(0.0, 20.0));
        ps.sketch.set_style(axis, Style::Centerline).unwrap();
        let p = ps.sketch.add_point(Point2::new(12.0, 5.0));
        let picks = [
            Pick {
                pos: Point2::new(12.0, 5.0),
                snap: Snap::Point(p),
                infer: None,
                align: Align::default(),
            },
            Pick {
                pos: Point2::new(0.0, 0.0),
                snap: Snap::OnEntity(axis),
                infer: None,
                align: Align::default(),
            },
        ];
        // Far side: Ø24.
        match dimension_from(&ps.sketch, &picks, Point2::new(-8.0, 10.0)).unwrap() {
            Constraint::DiameterToAxis { value, .. } => assert_relative_eq!(value, 24.0),
            c => panic!("{c:?}"),
        }
        // Same side: the plain distance.
        assert!(matches!(
            dimension_from(&ps.sketch, &picks, Point2::new(6.0, 10.0)).unwrap(),
            Constraint::PointLineDistance { .. }
        ));
        // It drives the geometry as a diameter.
        let id = ps
            .sketch
            .add_constraint(Constraint::DiameterToAxis {
                point: p,
                line: axis,
                value: 30.0,
            })
            .unwrap();
        ps.sketch.solve().unwrap();
        // The axis is free too: measure to it, not to x = 0.
        let (l0, l1) = ps.sketch.line_geometry(axis).unwrap();
        let q = ps.sketch.point(p).unwrap().position;
        let dist = (l1 - l0).normalize().perp(&(q - l0)).abs();
        assert_relative_eq!(dist, 15.0, epsilon = 1e-8);
        assert_eq!(
            ps.sketch
                .dimension_text(id, crate::sketch::DimDisplay::Value),
            "Ø30.00"
        );
    }
}
