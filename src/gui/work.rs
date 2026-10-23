//! The Work Plane / Work Axis / Work Point / UCS commands
//! (`docs/spec/2.10-work-features.md`): one command per kind whose method
//! follows from what is picked (Inventor's general Plane / Axis / Point).
//! Also draws the work features and picks them in the view.
//!
//! Selection logic and status prompts live here (not in the shared command
//! machinery) so a later common command module can take them over.

use std::borrow::Cow;

use eframe::egui::{self, Color32, Pos2, Rect, Shape, Stroke};

use super::CadCraftApp;
use crate::kernel::{Plane, Point3, Real, Surface, Vector3};
use crate::part::work::{WorkResult, ucs_parts};
use crate::part::{
    CurveRef, Feature, LineRef, PlaneRef, PointRef, UcsDef, WorkAxisDef, WorkDef, WorkFeature,
    WorkPlaneDef, WorkPointDef,
};
use crate::sketch::{EntityId, PointId};

/// Which work feature a command makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WorkKind {
    Plane,
    Axis,
    Point,
    Ucs,
}

impl WorkKind {
    pub(super) fn title(self) -> &'static str {
        match self {
            WorkKind::Plane => "Work Plane",
            WorkKind::Axis => "Work Axis",
            WorkKind::Point => "Work Point",
            WorkKind::Ucs => "UCS",
        }
    }

    /// The kind of an existing feature's definition.
    pub(super) fn of(def: &WorkDef) -> Self {
        match def {
            WorkDef::Plane(_) => WorkKind::Plane,
            WorkDef::Axis(_) => WorkKind::Axis,
            WorkDef::Point(_) => WorkKind::Point,
            WorkDef::Ucs(_) => WorkKind::Ucs,
        }
    }
}

/// The kind of a curved analytic face (what a method can do with it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SurfKind {
    Cylinder,
    Cone,
    Sphere,
    Torus,
}

impl SurfKind {
    fn label(self) -> &'static str {
        match self {
            SurfKind::Cylinder => "Cylindrical face",
            SurfKind::Cone => "Conical face",
            SurfKind::Sphere => "Spherical face",
            SurfKind::Torus => "Toroidal face",
        }
    }
}

/// One input of a work feature command.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum WpInput {
    Plane(PlaneRef),
    Line(LineRef),
    Point(PointRef),
    /// A curved model edge (circle, ellipse, spline), by name.
    Curve(String),
    /// A face of a Revolve feature (by feature id): faceted bodies.
    Revolved(String),
    /// A curved analytic face (by name).
    Surface(String, SurfKind),
    /// A point of a 2D sketch.
    SketchPoint(String, PointId),
    /// A line of a 2D sketch.
    SketchLine(String, EntityId),
    /// A circle, arc or spline of a 2D sketch.
    SketchCurve(String, EntityId),
}

fn plane_text(r: &PlaneRef) -> String {
    match r {
        PlaneRef::Origin(p) => p.label().to_owned(),
        PlaneRef::Face(f) => format!("Face {f}"),
        PlaneRef::Work(id) => id.replace("WorkPlane", "Work Plane"),
        PlaneRef::Fixed => "Plane".to_owned(),
    }
}

impl WpInput {
    pub(super) fn describe(&self) -> String {
        match self {
            WpInput::Plane(p) => plane_text(p),
            WpInput::Line(l) => l.describe(),
            WpInput::Point(p) => p.describe(),
            WpInput::Curve(e) => format!("Curved edge {e}"),
            WpInput::Revolved(f) => format!("Revolved feature {f}"),
            WpInput::Surface(f, k) => format!("{} {f}", k.label()),
            WpInput::SketchPoint(s, _) => format!("Point of {s}"),
            WpInput::SketchLine(s, _) => format!("Line of {s}"),
            WpInput::SketchCurve(s, _) => format!("Curve of {s}"),
        }
    }
}

/// The command dialog's state.
#[derive(Clone, Debug)]
pub(super) struct WorkDraft {
    pub kind: WorkKind,
    /// Picked inputs, in order.
    pub picks: Vec<WpInput>,
    /// Offset distance (mm).
    pub distance: Real,
    /// Angle (radians).
    pub angle: Real,
    /// Work point: ground it at `xyz`.
    pub grounded: bool,
    /// Grounded point position (mm).
    pub xyz: [Real; 3],
    /// The other of two solutions (tangent planes).
    pub other: bool,
    /// Which hit along a line (surface and line).
    pub index: usize,
    /// Flip Normal (work planes).
    pub flip: bool,
    /// UCS relative to the origin: X, Y, Z (mm) and angles about X, Y, Z
    /// (radians).
    pub ucs: [Real; 6],
    /// Redefine of feature k (same id and name).
    pub edit: Option<usize>,
}

/// English → Bulgarian for this module's prompts (kept here, beside the
/// prompts, so the shared table in `i18n.rs` does not need to change).
const PROMPTS_BG: &[(&str, &str)] = &[
    (
        "select a plane, an edge, a point or a curved face",
        "изберете равнина, ръб, точка или крива стена",
    ),
    (
        "select a plane (angle), a second edge, a point or a curved face",
        "изберете равнина (ъгъл), втори ръб, точка или крива стена",
    ),
    (
        "select a plane, an axis, a curve, a curved face or two more points",
        "изберете равнина, ос, крива, крива стена или още две точки",
    ),
    ("select the third point", "изберете третата точка"),
    (
        "select an edge, a point or a plane to go with the face",
        "изберете ръб, точка или равнина към стената",
    ),
    (
        "select a point on the curve",
        "изберете точка върху кривата",
    ),
    (
        "select an edge, two points, two planes, a circular edge or a revolved face",
        "изберете ръб, две точки, две равнини, кръгъл ръб или завъртяна стена",
    ),
    (
        "select a second point, a line or a plane",
        "изберете втора точка, линия или равнина",
    ),
    (
        "select a second plane or a point",
        "изберете втора равнина или точка",
    ),
    (
        "select a vertex, a sketch point, planes, lines or a round face",
        "изберете връх, точка от скица, равнини, линии или кръгла стена",
    ),
    (
        "select more planes (three) or a line",
        "изберете още равнини (три) или линия",
    ),
    (
        "select a second line or a plane",
        "изберете втора линия или равнина",
    ),
    (
        "select a line that pierces the face",
        "изберете линия, която пробива стената",
    ),
    (
        "set X, Y, Z and the angles, or select three points (centre, X, Y)",
        "задайте X, Y, Z и ъглите или изберете три точки (център, X, Y)",
    ),
    (
        "select the X direction point",
        "изберете точката за посока X",
    ),
    (
        "select the Y direction point",
        "изберете точката за посока Y",
    ),
    (
        "this combination is no method of the command",
        "тази комбинация не е метод на командата",
    ),
    (
        "OK to create, or pick more",
        "OK за създаване, или изберете още",
    ),
    ("OK to redefine", "OK за промяна"),
];

/// A prompt in the hint language.
pub(super) fn tr(s: &str) -> Cow<'_, str> {
    if super::i18n::bulgarian()
        && let Some((_, bg)) = PROMPTS_BG.iter().find(|(en, _)| *en == s)
    {
        return Cow::Borrowed(bg);
    }
    super::i18n::tr(s)
}

const NO_METHOD: &str = "this combination is no method of the command";

/// Id of the in-line work feature that stands for pick `k` in a preview.
fn inline_id(k: usize) -> String {
    format!("__inline{k}")
}

impl WorkDraft {
    pub(super) fn new(kind: WorkKind) -> Self {
        Self {
            kind,
            picks: Vec::new(),
            distance: 10.0,
            angle: std::f64::consts::FRAC_PI_2,
            grounded: false,
            xyz: [0.0; 3],
            other: false,
            index: 0,
            flip: false,
            ucs: [0.0; 6],
            edit: None,
        }
    }

    /// Redefine of an existing work feature: its values come along, its
    /// inputs are picked again.
    pub(super) fn redefine(k: usize, w: &WorkFeature) -> Self {
        let mut d = Self::new(WorkKind::of(&w.def));
        d.edit = Some(k);
        d.flip = w.flip;
        match &w.def {
            WorkDef::Plane(WorkPlaneDef::Offset { distance, .. }) => d.distance = *distance,
            WorkDef::Plane(WorkPlaneDef::Angle { angle, .. }) => d.angle = *angle,
            WorkDef::Point(WorkPointDef::Fixed { x, y, z }) => {
                d.grounded = true;
                d.xyz = [*x, *y, *z];
            }
            WorkDef::Ucs(UcsDef::Offsets {
                x,
                y,
                z,
                rx,
                ry,
                rz,
            }) => d.ucs = [*x, *y, *z, *rx, *ry, *rz],
            _ => {}
        }
        d
    }

    /// Whether pick `k` (a sketch point or line) stands for an in-line work
    /// point / axis: everywhere except as the only input of On Sketch
    /// Point / On Line.
    fn is_inline(&self, k: usize) -> bool {
        let sole = self.picks.len() == 1;
        match self.picks.get(k) {
            Some(WpInput::SketchPoint(..)) => !(sole && self.kind == WorkKind::Point),
            Some(WpInput::SketchLine(..)) => !(sole && self.kind == WorkKind::Axis),
            _ => false,
        }
    }

    /// The in-line work features the picks need, by pick index.
    pub(super) fn inline_defs(&self) -> Vec<(usize, WorkDef)> {
        (0..self.picks.len())
            .filter(|&k| self.is_inline(k))
            .filter_map(|k| match &self.picks[k] {
                WpInput::SketchPoint(s, p) => Some((
                    k,
                    WorkDef::Point(WorkPointDef::SketchPoint {
                        sketch: s.clone(),
                        point: *p,
                    }),
                )),
                WpInput::SketchLine(s, e) => Some((
                    k,
                    WorkDef::Axis(WorkAxisDef::SketchLine {
                        sketch: s.clone(),
                        entity: *e,
                    }),
                )),
                _ => None,
            })
            .collect()
    }

    /// The definition the picks make (in-line inputs under their preview
    /// ids), or what is still needed.
    pub(super) fn def(&self) -> Result<WorkDef, &'static str> {
        self.def_with(&inline_id)
    }

    /// The definition, with `inline(k)` the id of pick k's in-line feature.
    pub(super) fn def_with(
        &self,
        inline: &dyn Fn(usize) -> String,
    ) -> Result<WorkDef, &'static str> {
        use WpInput as I;
        let picks: Vec<WpInput> = self
            .picks
            .iter()
            .enumerate()
            .map(|(k, p)| match p {
                I::SketchPoint(..) if self.is_inline(k) => I::Point(PointRef::Work(inline(k))),
                I::SketchLine(..) if self.is_inline(k) => I::Line(LineRef::Work(inline(k))),
                other => other.clone(),
            })
            .collect();
        let curve = |p: &WpInput| match p {
            I::Curve(e) => Some(CurveRef::Edge(e.clone())),
            I::SketchCurve(s, e) => Some(CurveRef::Sketch {
                sketch: s.clone(),
                entity: *e,
            }),
            _ => None,
        };
        let other = self.other;
        match self.kind {
            WorkKind::Plane => {
                let d = match &picks[..] {
                    [] => return Err("select a plane, an edge, a point or a curved face"),
                    [I::Plane(p)] => WorkPlaneDef::Offset {
                        base: p.clone(),
                        distance: self.distance,
                    },
                    [I::Surface(f, SurfKind::Torus)] => {
                        WorkPlaneDef::TorusMidplane { face: f.clone() }
                    }
                    [I::Surface(..)] => {
                        return Err("select an edge, a point or a plane to go with the face");
                    }
                    [I::Plane(a), I::Plane(b)] => WorkPlaneDef::Midplane {
                        a: a.clone(),
                        b: b.clone(),
                    },
                    [I::Plane(p), I::Line(l)] | [I::Line(l), I::Plane(p)] => WorkPlaneDef::Angle {
                        base: p.clone(),
                        axis: l.clone(),
                        angle: self.angle,
                    },
                    [I::Plane(p), I::Point(q)] | [I::Point(q), I::Plane(p)] => {
                        WorkPlaneDef::ThroughPoint {
                            base: p.clone(),
                            point: q.clone(),
                        }
                    }
                    [I::Point(a), I::Point(b), I::Point(c)] => WorkPlaneDef::ThreePoints {
                        points: [a.clone(), b.clone(), c.clone()],
                    },
                    [I::Line(a), I::Line(b)] => WorkPlaneDef::TwoLines {
                        a: a.clone(),
                        b: b.clone(),
                    },
                    [I::Line(l), I::Point(p)] | [I::Point(p), I::Line(l)] => {
                        WorkPlaneDef::NormalToAxis {
                            axis: l.clone(),
                            point: p.clone(),
                        }
                    }
                    [I::Surface(f, _), I::Line(l)] | [I::Line(l), I::Surface(f, _)] => {
                        WorkPlaneDef::TangentEdge {
                            face: f.clone(),
                            line: l.clone(),
                            other,
                        }
                    }
                    [I::Surface(f, _), I::Point(p)] | [I::Point(p), I::Surface(f, _)] => {
                        WorkPlaneDef::TangentPoint {
                            face: f.clone(),
                            point: p.clone(),
                            other,
                        }
                    }
                    [I::Surface(f, _), I::Plane(p)] | [I::Plane(p), I::Surface(f, _)] => {
                        WorkPlaneDef::TangentParallel {
                            face: f.clone(),
                            base: p.clone(),
                            other,
                        }
                    }
                    [c, I::Point(p)] | [I::Point(p), c] if curve(c).is_some() => {
                        WorkPlaneDef::NormalToCurve {
                            curve: curve(c).ok_or(NO_METHOD)?,
                            point: p.clone(),
                        }
                    }
                    [c] if curve(c).is_some() => return Err("select a point on the curve"),
                    [I::Line(_)] => {
                        return Err(
                            "select a plane (angle), a second edge, a point or a curved face",
                        );
                    }
                    [I::Point(_)] => {
                        return Err(
                            "select a plane, an axis, a curve, a curved face or two more points",
                        );
                    }
                    [I::Point(_), I::Point(_)] => return Err("select the third point"),
                    _ => return Err(NO_METHOD),
                };
                Ok(WorkDef::Plane(d))
            }
            WorkKind::Axis => {
                let d = match &picks[..] {
                    [] => {
                        return Err(
                            "select an edge, two points, two planes, a circular edge or a revolved face",
                        );
                    }
                    [I::Line(l)] => WorkAxisDef::OnLine { line: l.clone() },
                    [I::SketchLine(s, e)] => WorkAxisDef::SketchLine {
                        sketch: s.clone(),
                        entity: *e,
                    },
                    [I::Curve(e)] => WorkAxisDef::CircularEdge { edge: e.clone() },
                    [I::SketchCurve(s, e)] => WorkAxisDef::SketchCircle {
                        sketch: s.clone(),
                        entity: *e,
                    },
                    [I::Revolved(f)] => WorkAxisDef::Revolved { feature: f.clone() },
                    [I::Surface(f, _)] => WorkAxisDef::RevolvedFace { face: f.clone() },
                    [I::Point(a), I::Point(b)] => WorkAxisDef::TwoPoints {
                        a: a.clone(),
                        b: b.clone(),
                    },
                    [I::Plane(a), I::Plane(b)] => WorkAxisDef::TwoPlanes {
                        a: a.clone(),
                        b: b.clone(),
                    },
                    [I::Plane(p), I::Point(q)] | [I::Point(q), I::Plane(p)] => {
                        WorkAxisDef::NormalToPlane {
                            plane: p.clone(),
                            point: q.clone(),
                        }
                    }
                    [I::Point(q), I::Line(l)] | [I::Line(l), I::Point(q)] => {
                        WorkAxisDef::ParallelThroughPoint {
                            line: l.clone(),
                            point: q.clone(),
                        }
                    }
                    [I::Line(l), I::Plane(p)] | [I::Plane(p), I::Line(l)] => {
                        WorkAxisDef::LineOnPlane {
                            line: l.clone(),
                            plane: p.clone(),
                        }
                    }
                    [I::Point(_)] => return Err("select a second point, a line or a plane"),
                    [I::Plane(_)] => return Err("select a second plane or a point"),
                    _ => return Err(NO_METHOD),
                };
                Ok(WorkDef::Axis(d))
            }
            WorkKind::Point => {
                if self.grounded {
                    let [x, y, z] = self.xyz;
                    return Ok(WorkDef::Point(WorkPointDef::Fixed { x, y, z }));
                }
                let d = match &picks[..] {
                    [] => {
                        return Err(
                            "select a vertex, a sketch point, planes, lines or a round face",
                        );
                    }
                    [I::Point(p)] => WorkPointDef::OnPoint { point: p.clone() },
                    [I::SketchPoint(s, p)] => WorkPointDef::SketchPoint {
                        sketch: s.clone(),
                        point: *p,
                    },
                    [I::Curve(e)] => WorkPointDef::LoopCenter {
                        edge: e.clone(),
                        face: None,
                    },
                    [I::Surface(f, SurfKind::Torus)] => {
                        WorkPointDef::TorusCenter { face: f.clone() }
                    }
                    [I::Surface(f, SurfKind::Sphere)] => {
                        WorkPointDef::SphereCenter { face: f.clone() }
                    }
                    [I::Surface(..)] => return Err("select a line that pierces the face"),
                    [I::Surface(f, _), I::Line(l)] | [I::Line(l), I::Surface(f, _)] => {
                        WorkPointDef::SurfaceLine {
                            face: f.clone(),
                            line: l.clone(),
                            index: self.index,
                        }
                    }
                    [I::Plane(a), I::Plane(b), I::Plane(c)] => WorkPointDef::ThreePlanes {
                        planes: [a.clone(), b.clone(), c.clone()],
                    },
                    [I::Line(a), I::Line(b)] => WorkPointDef::TwoLines {
                        a: a.clone(),
                        b: b.clone(),
                    },
                    [I::Plane(p), I::Line(l)] | [I::Line(l), I::Plane(p)] => {
                        WorkPointDef::PlaneLine {
                            plane: p.clone(),
                            line: l.clone(),
                        }
                    }
                    [I::Plane(_)] | [I::Plane(_), I::Plane(_)] => {
                        return Err("select more planes (three) or a line");
                    }
                    [I::Line(_)] => return Err("select a second line or a plane"),
                    _ => return Err(NO_METHOD),
                };
                Ok(WorkDef::Point(d))
            }
            WorkKind::Ucs => match &picks[..] {
                [] => {
                    let [x, y, z, rx, ry, rz] = self.ucs;
                    Ok(WorkDef::Ucs(UcsDef::Offsets {
                        x,
                        y,
                        z,
                        rx,
                        ry,
                        rz,
                    }))
                }
                [I::Point(_)] => Err("select the X direction point"),
                [I::Point(_), I::Point(_)] => Err("select the Y direction point"),
                [I::Point(a), I::Point(b), I::Point(c)] => Ok(WorkDef::Ucs(UcsDef::ThreePoints {
                    points: [a.clone(), b.clone(), c.clone()],
                })),
                _ => Err(NO_METHOD),
            },
        }
    }

    /// Adds a pick (a second click on the same input removes it); a pick
    /// that fits no method starts over with it.
    pub(super) fn pick(&mut self, input: WpInput) {
        if let Some(k) = self.picks.iter().position(|p| *p == input) {
            self.picks.remove(k);
            return;
        }
        self.picks.push(input.clone());
        if self.def().is_err_and(|e| e == NO_METHOD) || self.picks.len() > 3 {
            self.picks = vec![input];
        }
    }

    /// Whether the method has two solutions (Flip side).
    pub(super) fn has_other(def: &WorkDef) -> bool {
        matches!(
            def,
            WorkDef::Plane(
                WorkPlaneDef::TangentEdge { .. }
                    | WorkPlaneDef::TangentPoint { .. }
                    | WorkPlaneDef::TangentParallel { .. }
            )
        )
    }

    /// The status-bar prompt: "<Command>: <what next>".
    pub(super) fn prompt(&self) -> String {
        let need = match self.def() {
            Ok(def) => {
                let next = if self.edit.is_some() {
                    "OK to redefine"
                } else {
                    "OK to create, or pick more"
                };
                format!("{}: {}", def.method(), tr(next))
            }
            Err(e) => tr(e).into_owned(),
        };
        format!("{}: {need}", self.kind.title())
    }
}

/// Corners of the drawn rectangle of `plane` around `center`.
fn quad(plane: &Plane, center: &Point3, half: Real) -> [Point3; 4] {
    let x = plane.x_axis.into_inner() * half;
    let y = plane.y_axis().into_inner() * half;
    [
        center - x - y,
        center + x - y,
        center + x + y,
        center - x + y,
    ]
}

/// Ray hit of the rectangle (ray parameter).
fn hit(plane: &Plane, center: &Point3, half: Real, origin: &Point3, dir: &Vector3) -> Option<Real> {
    let n = plane.normal.into_inner();
    let denom = dir.dot(&n);
    if denom.abs() < 1e-12 {
        return None;
    }
    let t = (plane.origin - origin).dot(&n) / denom;
    let p = origin + dir * t;
    let d = p - center;
    (t > 0.0
        && d.dot(&plane.x_axis.into_inner()).abs() <= half
        && d.dot(&plane.y_axis().into_inner()).abs() <= half)
        .then_some(t)
}

/// Screen distance from `p` to the segment `a`–`b`.
fn seg_dist(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    (a + ab * t).distance(p)
}

/// The kind of curved analytic face `name` is, if any.
pub(super) fn surf_kind(body: &crate::kernel::BRep, name: &str) -> Option<SurfKind> {
    let f = body.face(body.find_face(name)?)?;
    match body.surface(f.facet_of.unwrap_or(f.surface))? {
        Surface::Cylinder(_) => Some(SurfKind::Cylinder),
        Surface::Cone(_) => Some(SurfKind::Cone),
        Surface::Sphere(_) => Some(SurfKind::Sphere),
        Surface::Torus(_) => Some(SurfKind::Torus),
        _ => None,
    }
}

/// One drawable, pickable element of a work feature (a UCS has seven).
struct Item {
    /// The id other features use (`WorkPlane1`, `UCS1:XY`).
    id: String,
    /// Shown beside it.
    label: String,
    /// Part of a UCS (drawn small, around the UCS centre).
    ucs: bool,
}

impl CadCraftApp {
    /// Centre of the model and the half size of drawn work features.
    fn work_extent(&self) -> (Point3, Real) {
        let (c, half) = match self.part().body.bounding_box() {
            Some((lo, hi)) => (nalgebra::center(&lo, &hi), (hi - lo).norm() * 0.42),
            None => (Point3::origin(), 30.0),
        };
        (c, half.max(self.origin_plane_half()))
    }

    /// Where a work plane is drawn: around the model's centre projected
    /// onto it, a bit larger than the model.
    pub(super) fn work_plane_frame(&self, plane: &Plane) -> (Point3, Real) {
        let (c, half) = self.work_extent();
        let uv = plane.project(&c);
        (plane.point_at(uv.x, uv.y), half)
    }

    /// Where an element is drawn: work planes around the model, UCS planes
    /// as small squares in the first quadrant of the UCS.
    fn item_plane_frame(&self, plane: &Plane, ucs: bool) -> (Point3, Real) {
        if ucs {
            let (_, half) = self.work_extent();
            let s = half * 0.12;
            let c = plane.origin + (plane.x_axis.into_inner() + plane.y_axis().into_inner()) * s;
            (c, s)
        } else {
            self.work_plane_frame(plane)
        }
    }

    /// The drawn segment of a work axis.
    fn axis_segment(&self, p: &Point3, d: &Vector3) -> (Point3, Point3) {
        let (c, half) = self.work_extent();
        let foot = p + d * (c - p).dot(d);
        (foot - d * half, foot + d * half)
    }

    /// The drawn segment of an element's axis (UCS axes start at the
    /// centre).
    fn item_axis_segment(&self, p: &Point3, d: &Vector3, ucs: bool) -> (Point3, Point3) {
        if ucs {
            let (_, half) = self.work_extent();
            (*p, p + d * (half * 0.35))
        } else {
            self.axis_segment(p, d)
        }
    }

    fn visible_work(&self) -> impl Iterator<Item = &crate::part::WorkFeature> {
        self.part().features.iter().filter_map(|f| match f {
            Feature::Work(w) if w.visible => Some(w),
            _ => None,
        })
    }

    /// The elements of a work feature.
    fn items(w: &WorkFeature) -> Vec<Item> {
        match w.def {
            WorkDef::Ucs(_) => ucs_parts(&w.id)
                .into_iter()
                .map(|(id, label)| Item {
                    id,
                    label: label.split(' ').next().unwrap_or(label).to_owned(),
                    ucs: true,
                })
                .collect(),
            _ => vec![Item {
                id: w.id.clone(),
                label: w.name.clone(),
                ucs: false,
            }],
        }
    }

    /// The visible work plane hit by the eye ray at `pos`: (id, ray t).
    pub(super) fn work_plane_under(&self, pos: Pos2, rect: Rect) -> Option<(String, Real)> {
        if !self.cat_on(crate::command::selection::Category::WorkPlane) {
            return None;
        }
        let (origin, dir) = self.camera.ray(pos, rect)?;
        let part = self.part();
        self.visible_work()
            .flat_map(Self::items)
            .filter_map(|it| {
                let plane = part.work.planes.get(&it.id)?;
                let (c, half) = self.item_plane_frame(plane, it.ucs);
                Some((it.id, hit(plane, &c, half, &origin, &dir)?))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// The visible work point (8 px) or work axis (6 px) under `pos`.
    pub(super) fn work_axis_or_point_under(&self, pos: Pos2, rect: Rect) -> Option<WpInput> {
        let part = self.part();
        let mut best: Option<(f32, WpInput)> = None;
        use crate::command::selection::Category;
        let (points, axes) = (
            self.cat_on(Category::WorkPoint),
            self.cat_on(Category::WorkAxis),
        );
        for it in self.visible_work().flat_map(Self::items) {
            let cand = if let Some(p) = part.work.points.get(&it.id) {
                if !points {
                    continue;
                }
                let s = self.camera.project(p, rect)?;
                let d = s.distance(pos);
                (d <= 8.0).then(|| (d - 2.0, WpInput::Point(PointRef::Work(it.id.clone()))))
            } else if let Some((p, d)) = part.work.axes.get(&it.id) {
                if !axes {
                    continue;
                }
                let (a, b) = self.item_axis_segment(p, d, it.ucs);
                let (a, b) = (
                    self.camera.project(&a, rect)?,
                    self.camera.project(&b, rect)?,
                );
                let d = seg_dist(pos, a, b);
                (d <= 6.0).then(|| (d, WpInput::Line(LineRef::Work(it.id.clone()))))
            } else {
                None
            };
            if let Some((d, i)) = cand
                && best.as_ref().is_none_or(|(bd, _)| d < *bd)
            {
                best = Some((d, i));
            }
        }
        best.map(|(_, i)| i)
    }

    /// The id of the visible work feature under `pos` (points, axes, then
    /// planes): for selection in the view. A UCS element selects its UCS.
    pub(super) fn work_feature_under(&self, pos: Pos2, rect: Rect) -> Option<String> {
        let id = match self.work_axis_or_point_under(pos, rect) {
            Some(WpInput::Point(PointRef::Work(id)) | WpInput::Line(LineRef::Work(id))) => Some(id),
            _ => self.work_plane_under(pos, rect).map(|(id, _)| id),
        }?;
        Some(id.split(':').next().unwrap_or(&id).to_owned())
    }

    /// The named model vertex or straight-edge midpoint under `pos` (8 px),
    /// not hidden by a face.
    pub(super) fn vertex_under(&self, pos: Pos2, rect: Rect) -> Option<PointRef> {
        if !self.cat_on(crate::command::selection::Category::Vertex) {
            return None;
        }
        let (origin, dir) = self.camera.ray(pos, rect)?;
        let dir = dir.try_normalize(1e-12)?;
        let body = &self.part().body;
        let face_t = self.picker().pick(&origin, &dir).map(|(_, t)| t);
        let visible = |p: &Point3| {
            let t = (p - origin).dot(&dir) / dir.norm_squared();
            !face_t.is_some_and(|ft| ft < t - 0.01 * t.abs().max(1.0))
        };
        let mut best: Option<(f32, PointRef)> = None;
        let mut consider = |p: Point3, r: PointRef, bias: f32| {
            if let Some(s) = self.camera.project(&p, rect) {
                let d = s.distance(pos) + bias;
                if d <= 8.0 && visible(&p) && best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                    best = Some((d, r));
                }
            }
        };
        for (_, v) in body.vertices() {
            if let Some(name) = &v.name {
                consider(v.point, PointRef::Vertex(name.clone()), 0.0);
            }
        }
        for (e, x) in body.edges() {
            if let (Some(name), Some(_)) = (&x.name, body.edge_direction(e))
                && let (Some(a), Some(b)) = (body.vertex(x.start), body.vertex(x.end))
            {
                // Vertices win over a midpoint at the same spot.
                consider(
                    nalgebra::center(&a.point, &b.point),
                    PointRef::EdgeMid(name.clone()),
                    1.0,
                );
            }
        }
        best.map(|(_, r)| r)
    }

    /// A point, line or curve of a visible sketch under `pos`.
    fn sketch_input_under(&self, pos: Pos2, rect: Rect) -> Option<WpInput> {
        use super::overlay;
        if !self.cat_on(crate::command::selection::Category::SketchEntity) {
            return None;
        }
        for ps in self.part().sketches.iter().filter(|s| s.visible) {
            let sk = &ps.sketch;
            if let Some(p) =
                overlay::pick_any_point(&self.camera, rect, sk, pos, overlay::POINT_PICK_PX)
            {
                return Some(WpInput::SketchPoint(ps.id.clone(), p));
            }
            if let Some((e, _)) =
                overlay::pick_entity(&self.camera, rect, sk, pos, overlay::CURVE_SNAP_PX)
            {
                return Some(match sk.entity(e) {
                    Some(crate::sketch::Entity::Line { .. }) => {
                        WpInput::SketchLine(ps.id.clone(), e)
                    }
                    _ => WpInput::SketchCurve(ps.id.clone(), e),
                });
            }
        }
        None
    }

    /// What a work feature command takes under `pos`: work point/axis >
    /// vertex/midpoint > sketch geometry > edge > face / work plane /
    /// origin plane.
    pub(super) fn wp_input_under(&self, pos: Pos2, rect: Rect, kind: WorkKind) -> Option<WpInput> {
        if let Some(i) = self.work_axis_or_point_under(pos, rect) {
            return Some(i);
        }
        if let Some(v) = self.vertex_under(pos, rect) {
            return Some(WpInput::Point(v));
        }
        if let Some(i) = self.sketch_input_under(pos, rect) {
            return Some(i);
        }
        let body = &self.part().body;
        if let Some(e) = self.edge_under(pos, rect)
            && let Some(name) = body.edge(e).and_then(|x| x.name.clone())
        {
            if body.edge_direction(e).is_some() {
                return Some(WpInput::Line(LineRef::Edge(name)));
            }
            return Some(WpInput::Curve(name));
        }
        let plane = self.plane_under(pos, rect, true)?;
        if let PlaneRef::Face(name) = &plane {
            // A curved analytic face.
            if let Some(k) = surf_kind(body, name) {
                return Some(WpInput::Surface(name.clone(), k));
            }
            // An axis through a faceted revolved face: its Revolve feature.
            if kind == WorkKind::Axis
                && let Some(f) = body.find_face(name).and_then(|f| body.face(f))
                && f.facet_of.is_some()
                && let Some(k) = self.face_feature(name)
                && let Feature::Revolve(r) = &self.part().features[k]
            {
                return Some(WpInput::Revolved(r.id.clone()));
            }
            // Other faces only as planes.
            body.find_face(name).and_then(|f| body.face_plane(f))?;
        }
        Some(WpInput::Plane(plane))
    }

    /// Draws the visible work features (and the open command's preview).
    pub(super) fn draw_work_features(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        hovered: Option<&str>,
    ) {
        let part = self.part();
        let selected = match self.selection {
            super::Selection::Feature(k) => part.features.get(k).map(|f| f.id().to_owned()),
            _ => None,
        };
        // A work feature whose handle is dragged is drawn where it would go
        // (the feature itself changes on release).
        let dragged = match &self.manip_drag {
            Some((super::ManipOwner::Feature(k), _, v)) if v.is_finite() => {
                match self.pending_work(*k) {
                    Some(Feature::Work(pw)) => Some(pw),
                    _ => None,
                }
            }
            _ => None,
        };
        if let Some(pw) = &dragged
            && let Ok(WorkResult::Plane(_, plane)) = self.compute_work(&pw.def)
        {
            let fill = Color32::from_rgba_unmultiplied(60, 150, 255, 70);
            self.draw_plane_rect(painter, rect, &plane, fill, true, &pw.name, false);
        }
        for w in self.visible_work() {
            if dragged.as_ref().is_some_and(|pw| pw.id == w.id)
                && matches!(w.def, WorkDef::Plane(_))
            {
                continue;
            }
            let hot = hovered == Some(w.id.as_str()) || selected.as_deref() == Some(w.id.as_str());
            for it in Self::items(w) {
                let color = if it.ucs {
                    ucs_color(&it.id)
                } else {
                    AXIS_COLOR
                };
                if let Some(plane) = part.work.planes.get(&it.id) {
                    // Only the outline while a sketch is edited (no tint).
                    let fill = if self.editing.is_some() {
                        Color32::TRANSPARENT
                    } else if hot {
                        Color32::from_rgba_unmultiplied(255, 160, 40, 80)
                    } else {
                        Color32::from_rgba_unmultiplied(240, 200, 90, 40)
                    };
                    let label = if it.ucs { "" } else { it.label.as_str() };
                    self.draw_plane_rect(painter, rect, plane, fill, hot, label, it.ucs);
                } else if let Some((p, d)) = part.work.axes.get(&it.id) {
                    self.draw_axis(painter, rect, p, d, hot, &it.label, color, it.ucs);
                } else if let Some(p) = part.work.points.get(&it.id) {
                    let grounded = matches!(w.def, WorkDef::Point(WorkPointDef::Fixed { .. }));
                    let label = if it.ucs { w.name.as_str() } else { &it.label };
                    self.draw_point(painter, rect, p, hot, label, grounded);
                }
            }
        }
        if let Some(d) = &self.work_draft
            && let Some(Ok(r)) = self.preview_draft(d)
        {
            let preview = Color32::from_rgb(60, 150, 255);
            let fill = Color32::from_rgba_unmultiplied(60, 150, 255, 70);
            match r {
                WorkResult::Plane(_, plane) => {
                    self.draw_plane_rect(painter, rect, &plane, fill, true, d.kind.title(), false)
                }
                WorkResult::Axis(_, p, v) => {
                    self.draw_axis(painter, rect, &p, &v, true, d.kind.title(), preview, false);
                }
                WorkResult::Point(_, p) => {
                    self.draw_point(painter, rect, &p, true, d.kind.title(), d.grounded);
                }
                WorkResult::Ucs(id, o, axes) => {
                    let mut tmp = crate::part::placement::WorkGeom::new();
                    WorkResult::Ucs(id.clone(), o, axes).store(&mut tmp);
                    for (k, (sub, label)) in ucs_parts(&id).into_iter().enumerate() {
                        if let Some(pl) = tmp.planes.get(&sub) {
                            self.draw_plane_rect(painter, rect, pl, fill, true, "", true);
                        } else if let Some((p, v)) = tmp.axes.get(&sub) {
                            let l = label.split(' ').next().unwrap_or(label);
                            let c = [UCS_X, UCS_Y, UCS_Z][k.saturating_sub(3).min(2)];
                            self.draw_axis(painter, rect, p, v, true, l, c, true);
                        }
                    }
                    self.draw_point(painter, rect, &o, true, "UCS", false);
                }
            }
        }
    }

    /// Computes a definition against the current part (preview, checks).
    pub(super) fn compute_work(&self, def: &WorkDef) -> Result<WorkResult, String> {
        self.compute_work_in(def, &self.part().work)
    }

    fn compute_work_in(
        &self,
        def: &WorkDef,
        works: &crate::part::placement::WorkGeom,
    ) -> Result<WorkResult, String> {
        let part = self.part();
        let revolve_axis = |id: &str| {
            part.features.iter().find_map(|f| match f {
                Feature::Revolve(r) if r.id == id => {
                    let ps = part.sketches.iter().find(|s| s.id == r.sketch)?;
                    r.world_axis(&ps.sketch).ok()
                }
                _ => None,
            })
        };
        let ctx = crate::part::work::Ctx {
            body: &part.body,
            works,
            revolve_axis: &revolve_axis,
            sketches: &part.sketches,
        };
        def.build("preview", &ctx)
    }

    /// The draft's result (with its in-line inputs and Flip Normal), or
    /// `None` while inputs are missing.
    pub(super) fn preview_draft(&self, d: &WorkDraft) -> Option<Result<WorkResult, String>> {
        let def = d.def().ok()?;
        let inline = d.inline_defs();
        let r = if inline.is_empty() {
            self.compute_work(&def)
        } else {
            let mut works = self.part().work.clone();
            for (k, idef) in &inline {
                match self.compute_work_in(idef, &works) {
                    Ok(r) => rename(r, inline_id(*k)).store(&mut works),
                    Err(e) => return Some(Err(e)),
                }
            }
            self.compute_work_in(&def, &works)
        };
        Some(r.map(|r| match r {
            WorkResult::Plane(id, p) if d.flip => {
                let n = -p.normal.into_inner();
                let flipped = Plane::new(p.origin, n, p.x_axis.into_inner()).unwrap_or(p);
                WorkResult::Plane(id, flipped)
            }
            r => r,
        }))
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_plane_rect(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        plane: &Plane,
        fill: Color32,
        hot: bool,
        label: &str,
        ucs: bool,
    ) {
        let (c, half) = self.item_plane_frame(plane, ucs);
        let pts: Option<Vec<Pos2>> = quad(plane, &c, half)
            .iter()
            .map(|p| self.camera.project(p, rect))
            .collect();
        let Some(pts) = pts else { return };
        let stroke = Stroke::new(if hot { 2.0 } else { 1.0 }, Color32::from_rgb(200, 140, 30));
        painter.add(Shape::convex_polygon(pts.clone(), fill, stroke));
        if !label.is_empty() {
            painter.text(
                pts[3],
                egui::Align2::LEFT_BOTTOM,
                label,
                egui::FontId::proportional(12.0),
                Color32::from_rgb(130, 85, 0),
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_axis(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        p: &Point3,
        d: &Vector3,
        hot: bool,
        label: &str,
        color: Color32,
        ucs: bool,
    ) {
        let (a, b) = self.item_axis_segment(p, d, ucs);
        let (Some(a), Some(b)) = (self.camera.project(&a, rect), self.camera.project(&b, rect))
        else {
            return;
        };
        let w = if hot { 3.0 } else { 1.5 };
        if ucs {
            // A triad arm: solid with an arrowhead.
            painter.arrow(a, b - a, Stroke::new(w, color));
        } else {
            // Dash-dot like a centre line.
            let len = a.distance(b).max(1.0);
            let dir = (b - a) / len;
            let mut t = 0.0;
            let pattern = [14.0, 4.0, 3.0, 4.0];
            let mut k = 0;
            while t < len {
                let seg = pattern[k % 4];
                if k % 2 == 0 {
                    painter.line_segment(
                        [a + dir * t, a + dir * (t + seg).min(len)],
                        Stroke::new(w, color),
                    );
                }
                t += seg;
                k += 1;
            }
        }
        painter.text(
            b,
            egui::Align2::LEFT_BOTTOM,
            label,
            egui::FontId::proportional(12.0),
            color,
        );
    }

    fn draw_point(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        p: &Point3,
        hot: bool,
        label: &str,
        grounded: bool,
    ) {
        let Some(s) = self.camera.project(p, rect) else {
            return;
        };
        let r = if hot { 6.0 } else { 4.5 };
        let color = Color32::from_rgb(200, 120, 20);
        // A diamond; grounded points are filled (Inventor's pushpin idea).
        let pts = vec![
            s + egui::vec2(0.0, -r),
            s + egui::vec2(r, 0.0),
            s + egui::vec2(0.0, r),
            s + egui::vec2(-r, 0.0),
        ];
        let fill = if grounded {
            color
        } else {
            Color32::TRANSPARENT
        };
        painter.add(Shape::convex_polygon(pts, fill, Stroke::new(1.5, color)));
        painter.text(
            s + egui::vec2(6.0, -6.0),
            egui::Align2::LEFT_BOTTOM,
            label,
            egui::FontId::proportional(12.0),
            color,
        );
    }

    /// Highlights an input under the cursor (or picked) in the view.
    pub(super) fn draw_wp_input(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        input: &WpInput,
        color: Color32,
    ) {
        let part = self.part();
        let body = &part.body;
        let opts = crate::kernel::TessellationOptions::default();
        let poly = |pts: &[Point3]| {
            let s: Vec<_> = pts
                .iter()
                .filter_map(|q| self.camera.project(q, rect))
                .collect();
            painter.add(Shape::line(s, Stroke::new(4.0, color)));
        };
        let sketch_poly = |sid: &str, e: EntityId| {
            if let Some(ps) = part.sketches.iter().find(|s| s.id == sid) {
                let pts: Vec<Point3> = super::overlay::polyline(&ps.sketch, e)
                    .iter()
                    .map(|q| ps.sketch.to_world(q))
                    .collect();
                poly(&pts);
            }
        };
        match input {
            WpInput::Point(r) => {
                if let Ok(p) = crate::part::placement::point_of(r, body, &part.work)
                    && let Some(s) = self.camera.project(&p, rect)
                {
                    painter.circle_filled(s, 5.0, color);
                }
            }
            WpInput::SketchPoint(sid, p) => {
                if let Some(ps) = part.sketches.iter().find(|s| s.id == *sid)
                    && let Some(q) = ps.sketch.point(*p)
                    && let Some(s) = self.camera.project(&ps.sketch.to_world(&q.position), rect)
                {
                    painter.circle_filled(s, 5.0, color);
                }
            }
            WpInput::SketchLine(sid, e) | WpInput::SketchCurve(sid, e) => sketch_poly(sid, *e),
            WpInput::Line(LineRef::Edge(name)) | WpInput::Curve(name) => {
                if let Some(e) = body.find_edge(name)
                    && let Ok(pts) = body.sample_edge(e, &opts)
                {
                    poly(&pts);
                }
            }
            WpInput::Line(l) => {
                if let Ok((p, d)) = l.resolve(body, &part.work) {
                    let (a, b) = self.axis_segment(&p, &d);
                    poly(&[a, b]);
                }
            }
            WpInput::Revolved(f) => {
                if let Ok(WorkResult::Axis(_, p, d)) =
                    self.compute_work(&WorkDef::Axis(WorkAxisDef::Revolved { feature: f.clone() }))
                {
                    let (a, b) = self.axis_segment(&p, &d);
                    poly(&[a, b]);
                }
            }
            WpInput::Plane(PlaneRef::Face(name)) | WpInput::Surface(name, _) => {
                if let Some(mesh) = body
                    .find_face(name)
                    .and_then(|f| body.face_mesh(f, &opts).ok())
                {
                    super::overlay::draw_face_fill(painter, &self.camera, rect, &mesh, color);
                }
            }
            WpInput::Plane(r) => {
                if let Ok(plane) = crate::part::placement::plane_of(r, body, &part.work) {
                    let fill = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 60);
                    let ucs = matches!(r, PlaneRef::Work(id) if id.contains(':'));
                    self.draw_plane_rect(painter, rect, &plane, fill, true, "", ucs);
                }
            }
        }
    }

    /// The angle arc of an Angle to Plane definition (the dialog's draft
    /// or a selected feature): around the hinge, from the base plane.
    pub(super) fn work_angle_handle(&self, def: &WorkDef) -> Option<Vec<super::manip::Handle>> {
        let WorkDef::Plane(WorkPlaneDef::Angle { base, axis, angle }) = def else {
            return None;
        };
        let part = self.part();
        let p = crate::part::placement::plane_of(base, &part.body, &part.work).ok()?;
        let (q, d) = axis.resolve(&part.body, &part.work).ok()?;
        let (c, half) = self.work_extent();
        let center = q + d * (c - q).dot(&d);
        let reference = p.normal.cross(&d) * (half * 0.6);
        Some(vec![super::manip::plane_angle_handle(
            center, d, reference, *angle,
        )])
    }

    /// Starts Redefine of work feature `k`: the command opens with its
    /// values; OK replaces the definition, keeping id and name.
    pub(super) fn redefine_work(&mut self, k: usize) {
        if let Some(Feature::Work(w)) = self.part().features.get(k) {
            self.work_draft = Some(WorkDraft::redefine(k, w));
        }
    }

    /// The command dialog's OK.
    pub(super) fn commit_work(&mut self) {
        let Some(d) = self.work_draft.clone() else {
            return;
        };
        let title = d.kind.title();
        if let Err(e) = d.def() {
            self.file_status = Some(Err(format!("{title}: {e}")));
            return;
        }
        let inline = d.inline_defs();
        if d.edit.is_some() && !inline.is_empty() {
            self.file_status = Some(Err(format!(
                "{title}: pick a work point or axis instead of sketch geometry when redefining"
            )));
            return;
        }
        let label = if d.edit.is_some() {
            "Redefine Feature"
        } else {
            title
        };
        self.checkpoint(label);
        let saved = self.part().clone();
        let result = self.commit_work_steps(&d, inline);
        match result {
            Ok(()) => {
                self.work_draft = None;
                let k = d.edit.unwrap_or(self.part().last_inserted());
                self.selection = super::Selection::Feature(k);
                self.file_status = None;
                self.scene_dirty = true;
            }
            Err(e) => {
                *self.part_mut() = saved;
                self.history.cancel();
                self.file_status = Some(Err(format!("{title}: {e}")));
            }
        }
    }

    /// Adds the in-line inputs (hidden work points / axes, right before the
    /// feature that uses them; Inventor nests them under it), then the
    /// feature, or replaces the redefined one.
    fn commit_work_steps(
        &mut self,
        d: &WorkDraft,
        inline: Vec<(usize, WorkDef)>,
    ) -> Result<(), String> {
        let mut ids: Vec<(usize, String)> = Vec::new();
        for (k, idef) in inline {
            let mut w = WorkFeature::new(idef);
            w.visible = false;
            let id = self.part_mut().add_work(w).map_err(|e| e.to_string())?;
            ids.push((k, id));
        }
        let def = d
            .def_with(&|k| {
                ids.iter()
                    .find(|(i, _)| *i == k)
                    .map(|(_, id)| id.clone())
                    .unwrap_or_else(|| inline_id(k))
            })
            .map_err(str::to_owned)?;
        let mut w = WorkFeature::new(def);
        w.flip = d.flip;
        match d.edit {
            Some(k) => {
                if let Some(Feature::Work(old)) = self.part().features.get(k) {
                    w.visible = old.visible;
                    w.params = old.params.clone();
                }
                self.part_mut()
                    .replace_feature(k, Feature::Work(w))
                    .map_err(|e| e.to_string())?;
            }
            None => {
                self.part_mut().add_work(w).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
}

/// A computed result under another id.
fn rename(r: WorkResult, id: String) -> WorkResult {
    match r {
        WorkResult::Plane(_, p) => WorkResult::Plane(id, p),
        WorkResult::Axis(_, p, d) => WorkResult::Axis(id, p, d),
        WorkResult::Point(_, p) => WorkResult::Point(id, p),
        WorkResult::Ucs(_, o, a) => WorkResult::Ucs(id, o, a),
    }
}

/// Colour of work axes.
pub(super) const AXIS_COLOR: Color32 = Color32::from_rgb(200, 120, 20);
/// UCS triad colours (X red, Y green, Z blue).
const UCS_X: Color32 = Color32::from_rgb(210, 50, 50);
const UCS_Y: Color32 = Color32::from_rgb(40, 160, 60);
const UCS_Z: Color32 = Color32::from_rgb(50, 90, 220);

fn ucs_color(id: &str) -> Color32 {
    if id.ends_with(":X") {
        UCS_X
    } else if id.ends_with(":Y") {
        UCS_Y
    } else if id.ends_with(":Z") {
        UCS_Z
    } else {
        AXIS_COLOR
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::part::OriginPlane;

    fn face(n: &str, k: SurfKind) -> WpInput {
        WpInput::Surface(n.into(), k)
    }

    /// The method follows from the picks (Inventor's general commands).
    #[test]
    fn methods_follow_the_picks() {
        let mut d = WorkDraft::new(WorkKind::Plane);
        let yz = WpInput::Plane(PlaneRef::Origin(OriginPlane::YZ));
        d.pick(face("F", SurfKind::Cylinder));
        assert!(d.def().is_err());
        d.pick(yz);
        assert_eq!(
            d.def().unwrap().method(),
            "Tangent to Surface and Parallel to Plane"
        );
        let mut d = WorkDraft::new(WorkKind::Plane);
        d.pick(face("T", SurfKind::Torus));
        assert_eq!(d.def().unwrap().method(), "Midplane of Torus");
        let mut d = WorkDraft::new(WorkKind::Plane);
        d.pick(WpInput::Curve("E".into()));
        d.pick(WpInput::Point(PointRef::Vertex("V".into())));
        assert_eq!(d.def().unwrap().method(), "Normal to Curve at Point");
        let mut d = WorkDraft::new(WorkKind::Axis);
        d.pick(face("F", SurfKind::Cone));
        assert_eq!(d.def().unwrap().method(), "Through Revolved Face");
        let mut d = WorkDraft::new(WorkKind::Point);
        d.pick(face("S", SurfKind::Sphere));
        assert_eq!(d.def().unwrap().method(), "Center Point of Sphere");
        let mut d = WorkDraft::new(WorkKind::Point);
        d.pick(face("C", SurfKind::Cylinder));
        d.pick(WpInput::Line(LineRef::Origin(0)));
        assert_eq!(
            d.def().unwrap().method(),
            "Intersection of Surface and Line"
        );
        let mut d = WorkDraft::new(WorkKind::Ucs);
        assert_eq!(d.def().unwrap().method(), "Relative to the Origin");
        for v in ["A", "B", "C"] {
            d.pick(WpInput::Point(PointRef::Vertex(v.into())));
        }
        assert_eq!(d.def().unwrap().method(), "Three Points");
    }

    /// A sketch point is the work point itself when it is the only pick,
    /// and an in-line work point otherwise.
    #[test]
    fn sketch_inputs_become_inline_work_features() {
        let p = PointId::default();
        let mut d = WorkDraft::new(WorkKind::Point);
        d.pick(WpInput::SketchPoint("Sketch1".into(), p));
        assert!(matches!(
            d.def(),
            Ok(WorkDef::Point(WorkPointDef::SketchPoint { .. }))
        ));
        assert!(d.inline_defs().is_empty());
        let mut d = WorkDraft::new(WorkKind::Plane);
        d.pick(WpInput::Plane(PlaneRef::Origin(OriginPlane::XY)));
        d.pick(WpInput::SketchPoint("Sketch1".into(), p));
        let Ok(WorkDef::Plane(WorkPlaneDef::ThroughPoint { point, .. })) = d.def() else {
            panic!("{:?}", d.def());
        };
        assert_eq!(point, PointRef::Work("__inline1".into()));
        assert_eq!(d.inline_defs().len(), 1);
        let def = d.def_with(&|k| format!("WorkPoint{k}")).unwrap();
        assert!(format!("{def:?}").contains("WorkPoint1"));
    }

    /// Every prompt has a Bulgarian line.
    #[test]
    fn prompts_are_translated() {
        let kinds = [
            WorkKind::Plane,
            WorkKind::Axis,
            WorkKind::Point,
            WorkKind::Ucs,
        ];
        for k in kinds {
            let d = WorkDraft::new(k);
            if let Err(e) = d.def() {
                assert!(PROMPTS_BG.iter().any(|(en, _)| *en == e), "{e}");
            }
        }
        assert!(PROMPTS_BG.iter().any(|(en, _)| *en == NO_METHOD));
    }
}
