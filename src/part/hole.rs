//! **Hole** and **Thread** (Inventor: 3D Model → Modify → Hole / Thread,
//! `docs/spec/2.3-hole.md`, `docs/spec/2.9-thread.md`).
//!
//! A hole keeps its full meaning ([`HoleSpec`]: type, standard, size,
//! thread, seat, termination, drill point), so drawings can write its note
//! ([`HoleSpec::note`]) and count it ([`group_holes`]). The numbers come
//! from the standards tables ([`super::standards`]) when the dialog fills
//! them in, and are stored in the feature: the part regenerates without the
//! tables.
//!
//! Geometry: every hole is one tool of revolution (bore, seat, drill point)
//! cut with the others in one Boolean. A thread is cosmetic (data and a
//! display hint) or modeled: a helical groove cut after every feature
//! (`kernel::exact::cut_threads`).
//!
//! Faces are named `<id>:<role>:<k>` with `k` the hole's number (from 1):
//! `Hole1:Bore:2`, `Hole1:CBore:1`, `Hole1:CSink:1`, `Hole1:Tip:1`.

use serde::{Deserialize, Serialize};

use crate::kernel::{BRep, Point3, Real, Surface, Vector3};
use crate::sketch::PointId;

use super::blend::EdgeRef;
use super::expr::Unit;
use super::params::FeatureParams;
use super::shell::FaceRef;
use super::standards::{self, Standards};

/// Hole type (Inventor's four).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum HoleType {
    /// A plain drilled hole.
    #[default]
    Simple,
    /// Sized for a fastener from the clearance table.
    Clearance,
    /// Threaded (straight thread).
    Tapped,
    /// Taper thread (NPT, ISO 7): not built yet.
    TaperTapped,
}

impl HoleType {
    /// Every type, in the panel's order.
    pub const ALL: [HoleType; 4] = [
        HoleType::Simple,
        HoleType::Clearance,
        HoleType::Tapped,
        HoleType::TaperTapped,
    ];

    /// The name as Inventor shows it.
    pub fn label(self) -> &'static str {
        match self {
            HoleType::Simple => "Simple",
            HoleType::Clearance => "Clearance",
            HoleType::Tapped => "Tapped",
            HoleType::TaperTapped => "Taper Tapped",
        }
    }
}

/// Seat at the top of the hole.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SeatType {
    /// No seat.
    #[default]
    None,
    /// Cylindrical counterbore (socket head screws).
    Counterbore,
    /// Shallow counterbore for a flat seat; the hole depth is measured
    /// from its floor.
    Spotface,
    /// Conical countersink (countersunk screws).
    Countersink,
}

impl SeatType {
    /// Every seat, in the panel's order.
    pub const ALL: [SeatType; 4] = [
        SeatType::None,
        SeatType::Counterbore,
        SeatType::Spotface,
        SeatType::Countersink,
    ];

    /// The name as Inventor shows it.
    pub fn label(self) -> &'static str {
        match self {
            SeatType::None => "None",
            SeatType::Counterbore => "Counterbore",
            SeatType::Spotface => "Spotface",
            SeatType::Countersink => "Countersink",
        }
    }
}

/// Where the hole ends.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Termination {
    /// At `depth` from the start plane.
    #[default]
    Distance,
    /// Through the whole part.
    ThroughAll,
    /// At a planar face (its plane: Inventor's Extend To).
    To(FaceRef),
}

impl Termination {
    /// The name as Inventor shows it.
    pub fn label(&self) -> &'static str {
        match self {
            Termination::Distance => "Distance",
            Termination::ThroughAll => "Through All",
            Termination::To(_) => "To",
        }
    }
}

/// The bottom of a blind hole.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DrillPoint {
    /// Flat bottom.
    Flat,
    /// A cone of `point_angle` (118° by default); the depth ends where the
    /// cone starts.
    #[default]
    Angle,
    /// Flat, annotated the ISO way (format revision 10).
    FlatW,
    /// Angled, annotated the ISO way (format revision 10).
    AngleV,
    /// Angled, the point included in the depth (format revision 10).
    AngleY,
}

impl DrillPoint {
    /// Every drill point, in the panel's order.
    pub const ALL: [DrillPoint; 5] = [
        DrillPoint::Flat,
        DrillPoint::FlatW,
        DrillPoint::Angle,
        DrillPoint::AngleV,
        DrillPoint::AngleY,
    ];

    /// Whether the bottom is a cone.
    pub fn angled(self) -> bool {
        matches!(
            self,
            DrillPoint::Angle | DrillPoint::AngleV | DrillPoint::AngleY
        )
    }

    /// The name as Inventor shows it.
    pub fn label(self) -> &'static str {
        match self {
            DrillPoint::Flat => "Flat",
            DrillPoint::FlatW => "Flat W",
            DrillPoint::Angle => "Angle",
            DrillPoint::AngleV => "Angle V",
            DrillPoint::AngleY => "Angle Y",
        }
    }
}

/// Fit of a clearance hole.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FastenerFit {
    /// ISO 273 fine, ASME close.
    Close,
    /// ISO 273 medium.
    #[default]
    Normal,
    /// ISO 273 coarse.
    Loose,
}

impl FastenerFit {
    /// Every fit.
    pub const ALL: [FastenerFit; 3] = [FastenerFit::Close, FastenerFit::Normal, FastenerFit::Loose];

    /// The name as Inventor shows it.
    pub fn label(self) -> &'static str {
        match self {
            FastenerFit::Close => "Close",
            FastenerFit::Normal => "Normal",
            FastenerFit::Loose => "Loose",
        }
    }
}

/// The fastener of a clearance hole.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClearanceSpec {
    /// Standard of the clearance table (`ISO 273`, `ASME B18.2.8`).
    pub standard: String,
    /// Fastener type (for the note and the seat tables).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub fastener: String,
    /// Fastener size (`M6`, `1/4`).
    pub size: String,
    /// Fit.
    #[serde(default)]
    pub fit: FastenerFit,
}

/// Thread data of a tapped hole or a Thread feature (Inventor's
/// HoleTapInfo / ThreadInfo), with its diameters copied from the table.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ThreadSpec {
    /// Thread type (the table: `ISO Metric profile`).
    pub thread_type: String,
    /// Nominal size (`M8`, `1/4`).
    pub size: String,
    /// Designation (`M8`, `M8x1`, `1/4-20 UNC`).
    pub designation: String,
    /// Tolerance class (`6H`, `2B`, `6g`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub class: String,
    /// Major diameter (mm).
    pub major: Real,
    /// Pitch (mm).
    pub pitch: Real,
    /// Basic minor diameter of the internal thread.
    pub minor: Real,
    /// Tap drill diameter.
    pub tap_drill: Real,
    /// Thread along the whole hole (Full Depth) or face.
    #[serde(default = "yes")]
    pub full_depth: bool,
    /// Thread depth when not full (mm); parameter `thread_depth`.
    #[serde(default)]
    pub depth: Real,
    /// Right-hand thread.
    #[serde(default = "yes")]
    pub right_hand: bool,
    /// Real helical geometry instead of a cosmetic thread.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub modeled: bool,
    /// Clearance for 3D printing (mm): a modeled internal thread grows by
    /// it, an external one shrinks; parameter `thread_clearance`.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub clearance: Real,
}

fn yes() -> bool {
    true
}

fn is_zero(v: &Real) -> bool {
    *v == 0.0
}

impl ThreadSpec {
    /// The thread `designation` of table `thread_type`, its first class
    /// (internal or external) as the default.
    pub fn from_table(
        tables: &Standards,
        thread_type: &str,
        designation: &str,
        internal: bool,
    ) -> Option<Self> {
        let t = tables.thread_table(thread_type)?;
        let r = t.row(designation)?;
        let classes = if internal {
            &r.classes_internal
        } else {
            &r.classes_external
        };
        Some(Self {
            thread_type: thread_type.to_owned(),
            size: r.size.clone(),
            designation: r.designation.clone(),
            class: classes.first().cloned().unwrap_or_default(),
            major: r.major,
            pitch: r.pitch,
            minor: r.minor,
            tap_drill: r.tap_drill,
            full_depth: true,
            depth: 0.0,
            right_hand: true,
            modeled: false,
            clearance: 0.0,
        })
    }

    /// ISO M8 coarse (the dialog's default).
    pub fn default_metric() -> Self {
        Self::from_table(Standards::global(), standards::ISO_METRIC, "M8", true).unwrap_or(Self {
            thread_type: standards::ISO_METRIC.into(),
            size: "M8".into(),
            designation: "M8".into(),
            class: "6H".into(),
            major: 8.0,
            pitch: 1.25,
            minor: 8.0 - 1.082532 * 1.25,
            tap_drill: 6.75,
            full_depth: true,
            depth: 0.0,
            right_hand: true,
            modeled: false,
            clearance: 0.0,
        })
    }

    /// The designation as a note writes it: ISO threads always with their
    /// pitch (`M8x1.25`), then the class (`M8x1.25 - 6H`), `LH` when
    /// left-handed.
    pub fn note(&self) -> String {
        let mut s = if self.thread_type == standards::ISO_METRIC && !self.designation.contains('x')
        {
            format!("{}x{}", self.designation, num(self.pitch, 3))
        } else {
            self.designation.clone()
        };
        if !self.class.is_empty() {
            s += &format!(" - {}", self.class);
        }
        if !self.right_hand {
            s += " LH";
        }
        s
    }

    /// Checks the values.
    pub fn validate(&self) -> Result<(), String> {
        if !(self.major > 0.0 && self.pitch > 0.0 && self.minor > 0.0) {
            return Err("the thread diameters and pitch must be positive".into());
        }
        if self.minor >= self.major || self.pitch >= self.major {
            return Err("the thread's minor diameter must be below its major diameter".into());
        }
        if !self.full_depth && !(self.depth.is_finite() && self.depth > 0.0) {
            return Err("the thread depth must be greater than 0".into());
        }
        if !(self.clearance >= 0.0 && self.clearance < self.pitch / 2.0) {
            return Err("the thread clearance must be at least 0 and below half the pitch".into());
        }
        Ok(())
    }

    /// The groove section of a modeled internal thread (ISO 68-1 basic
    /// profile, 60° flanks), `[r, z]` about the axis: from inside the bore
    /// (`bore_r`) out to the major radius plus the clearance, where the
    /// nut's root flat is P/8 wide (after SolidCraft's `thread_groove`).
    pub fn internal_groove(&self, bore_r: Real) -> Result<Vec<[Real; 2]>, String> {
        let p = self.pitch;
        let slope = (std::f64::consts::PI / 6.0).tan();
        let r1 = self.major / 2.0 + self.clearance;
        let r0 = bore_r - 0.1 * p;
        let w1 = p / 16.0;
        let w0 = (p / 8.0 + 2.0 * (r1 - r0) * slope) / 2.0;
        if !(r0 > 0.0 && r1 > bore_r) || 2.0 * w0 >= 0.98 * p {
            return Err("the hole is too far from the thread's diameter".into());
        }
        Ok(vec![[r0, -w0], [r1, -w1], [r1, w1], [r0, w0]])
    }

    /// The groove section of a modeled external thread on a shaft of radius
    /// `shaft_r` (already shrunk by the clearance): from the root (5H/8
    /// under the major radius, flat P/4 wide, moved in by the clearance)
    /// out past the surface.
    pub fn external_groove(&self, shaft_r: Real) -> Result<Vec<[Real; 2]>, String> {
        let p = self.pitch;
        let h = p * 3f64.sqrt() / 2.0;
        let slope = (std::f64::consts::PI / 6.0).tan();
        let root = self.major / 2.0 - 5.0 * h / 8.0 - self.clearance;
        let out = shaft_r + 0.05 * p;
        let w0 = p / 8.0;
        let w1 = (p / 4.0 + 2.0 * (out - root) * slope) / 2.0;
        if !(root.is_finite() && root > 0.0) || 2.0 * w1 >= 0.98 * p {
            return Err("the face is too far from the thread's diameter".into());
        }
        Ok(vec![[root, -w0], [out, -w1], [out, w1], [root, w0]])
    }
}

/// The full definition of a hole (Inventor's HoleFeature data), shared by
/// every centre of one Hole feature.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HoleSpec {
    /// Simple / Clearance / Tapped / Taper Tapped.
    #[serde(default)]
    pub hole_type: HoleType,
    /// None / Counterbore / Spotface / Countersink.
    #[serde(default)]
    pub seat: SeatType,
    /// Drill diameter (mm): typed (Simple), from the fit (Clearance), the
    /// thread's minor diameter (Tapped); parameter `diameter`.
    pub diameter: Real,
    /// Where it ends.
    #[serde(default)]
    pub termination: Termination,
    /// Depth (Distance), from the start plane — from the spotface floor
    /// with a spotface; parameter `depth`.
    #[serde(default)]
    pub depth: Real,
    /// Flat or angled bottom.
    #[serde(default)]
    pub drill_point: DrillPoint,
    /// Included angle of the drill point (radians, 118°); `point_angle`.
    #[serde(default = "drill_angle")]
    pub point_angle: Real,
    /// Drill the other way (Direction 2).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub flip: bool,
    /// Counterbore or spotface diameter; `seat_diameter`.
    #[serde(default)]
    pub seat_diameter: Real,
    /// Counterbore or spotface depth; `seat_depth`.
    #[serde(default)]
    pub seat_depth: Real,
    /// Countersink diameter; `csink_diameter`.
    #[serde(default)]
    pub csink_diameter: Real,
    /// Countersink included angle (radians; 90° ISO, 82° ANSI); `csink_angle`.
    #[serde(default = "csink_angle")]
    pub csink_angle: Real,
    /// The fastener of a clearance hole.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clearance: Option<ClearanceSpec>,
    /// The thread of a tapped hole.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<ThreadSpec>,
    /// Extend Start: the tool starts above the start plane even when it
    /// lies inside the material.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub extend_start: bool,
    /// Symmetric: through all in both directions (Simple, no seat, Through
    /// All only).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub symmetric: bool,
    /// Extend To (on by default): a To face is taken as its whole plane;
    /// off, the hole must meet the face itself.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub extend_to: bool,
    /// Far side countersink (our addition, as SolidWorks / Fusion): a cone
    /// where the bore leaves the body; `far_csink_diameter`,
    /// `far_csink_angle`. An older build ignores it (the hole stays plain).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub far_csink: bool,
    /// Its diameter at the far face.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub far_csink_diameter: Real,
    /// Its included angle (radians, 90° when not set).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub far_csink_angle: Real,
}

fn is_true(v: &bool) -> bool {
    *v
}

fn drill_angle() -> Real {
    118f64.to_radians()
}

fn csink_angle() -> Real {
    90f64.to_radians()
}

impl Default for HoleSpec {
    fn default() -> Self {
        Self::simple(6.0, 10.0)
    }
}

impl HoleSpec {
    /// A simple blind hole with Inventor's defaults (118° drill point).
    pub fn simple(diameter: Real, depth: Real) -> Self {
        Self {
            hole_type: HoleType::Simple,
            seat: SeatType::None,
            diameter,
            termination: Termination::Distance,
            depth,
            drill_point: DrillPoint::Angle,
            point_angle: drill_angle(),
            flip: false,
            seat_diameter: 2.0 * diameter,
            seat_depth: diameter / 2.0,
            csink_diameter: 2.0 * diameter,
            csink_angle: csink_angle(),
            clearance: None,
            thread: None,
            extend_start: false,
            symmetric: false,
            extend_to: true,
            far_csink: false,
            far_csink_diameter: 0.0,
            far_csink_angle: 0.0,
        }
    }

    /// A clearance hole for `size` of `standard` with `fit` (diameter from
    /// the table), through all.
    pub fn clearance(
        tables: &Standards,
        standard: &str,
        size: &str,
        fit: FastenerFit,
    ) -> Option<Self> {
        let row = tables.clearance(standard, size)?;
        let d = row.diameter(fit);
        Some(Self {
            hole_type: HoleType::Clearance,
            termination: Termination::ThroughAll,
            clearance: Some(ClearanceSpec {
                standard: standard.to_owned(),
                fastener: String::new(),
                size: size.to_owned(),
                fit,
            }),
            ..Self::simple(d, 10.0)
        })
    }

    /// A tapped hole of `thread` (drilled at its minor diameter), blind to
    /// `depth`.
    pub fn tapped(thread: ThreadSpec, depth: Real) -> Self {
        Self {
            hole_type: HoleType::Tapped,
            diameter: thread.minor,
            thread: Some(thread),
            ..Self::simple(6.0, depth)
        }
    }

    /// Fills the seat from the counterbore table (`fastener`, `size`):
    /// hole, counterbore diameter and depth. `false`: no such row.
    pub fn counterbore_from(&mut self, tables: &Standards, fastener: &str, size: &str) -> bool {
        let Some(r) = tables
            .counterbores
            .iter()
            .find(|r| r.fastener == fastener && r.size == size)
        else {
            return false;
        };
        self.seat = SeatType::Counterbore;
        if self.hole_type != HoleType::Tapped {
            self.diameter = r.hole;
        }
        self.seat_diameter = r.diameter;
        self.seat_depth = r.value;
        true
    }

    /// Fills the countersink from its table (`fastener`, `size`).
    pub fn countersink_from(&mut self, tables: &Standards, fastener: &str, size: &str) -> bool {
        let Some(r) = tables
            .countersinks
            .iter()
            .find(|r| r.fastener == fastener && r.size == size)
        else {
            return false;
        };
        self.seat = SeatType::Countersink;
        if self.hole_type != HoleType::Tapped {
            self.diameter = r.hole;
        }
        self.csink_diameter = r.diameter;
        self.csink_angle = r.value.to_radians();
        true
    }

    /// Whether the hole has a thread.
    pub fn tapped_thread(&self) -> Option<&ThreadSpec> {
        match self.hole_type {
            HoleType::Tapped | HoleType::TaperTapped => self.thread.as_ref(),
            _ => None,
        }
    }

    /// The radius of the bore as cut: the drill diameter, or for a modeled
    /// thread its minor diameter plus the clearance.
    pub fn bore_radius(&self) -> Real {
        match self.tapped_thread() {
            Some(t) if t.modeled => t.minor / 2.0 + t.clearance,
            _ => self.diameter / 2.0,
        }
    }

    /// Checks the values against each other (the messages are the
    /// feature's health).
    pub fn validate(&self) -> Result<(), String> {
        if self.hole_type == HoleType::TaperTapped {
            return Err("Taper Tapped holes are not supported yet".into());
        }
        let r = self.bore_radius();
        if !(r.is_finite() && r > 1e-6) {
            return Err("the hole diameter must be greater than 0".into());
        }
        if self.termination == Termination::Distance
            && !(self.depth.is_finite() && self.depth > 0.0)
        {
            return Err("the hole depth must be greater than 0".into());
        }
        if self.drill_point.angled()
            && !(self.point_angle > 1e-3 && self.point_angle < std::f64::consts::PI - 1e-3)
        {
            return Err("the drill point angle must be between 0 and 180 degrees".into());
        }
        match self.seat {
            SeatType::None => {}
            SeatType::Counterbore | SeatType::Spotface => {
                if !(self.seat_diameter.is_finite() && self.seat_diameter / 2.0 > r) {
                    return Err(format!(
                        "the {} must be wider than the hole",
                        self.seat.label().to_lowercase()
                    ));
                }
                let min_depth = if self.seat == SeatType::Spotface {
                    0.0
                } else {
                    1e-6
                };
                if !(self.seat_depth.is_finite() && self.seat_depth >= min_depth) {
                    return Err(format!(
                        "the {} depth must be greater than 0",
                        self.seat.label().to_lowercase()
                    ));
                }
                if self.seat == SeatType::Counterbore
                    && self.termination == Termination::Distance
                    && self.seat_depth >= self.depth
                {
                    return Err("the counterbore must be shallower than the hole".into());
                }
            }
            SeatType::Countersink => {
                if !(self.csink_diameter.is_finite() && self.csink_diameter / 2.0 > r) {
                    return Err("the countersink must be wider than the hole".into());
                }
                if !(self.csink_angle > 1e-3 && self.csink_angle < std::f64::consts::PI - 1e-3) {
                    return Err("the countersink angle must be between 0 and 180 degrees".into());
                }
                if self.termination == Termination::Distance && self.seat_bottom() >= self.depth {
                    return Err("the countersink must be shallower than the hole".into());
                }
            }
        }
        if self.symmetric
            && !(self.termination == Termination::ThroughAll
                && self.seat == SeatType::None
                && matches!(self.hole_type, HoleType::Simple | HoleType::Clearance))
        {
            return Err("Symmetric is only for a simple Through All hole without a seat".into());
        }
        if let Some(t) = self.tapped_thread() {
            t.validate()?;
            if self.termination == Termination::Distance
                && !t.full_depth
                && t.depth > self.depth + 1e-9
            {
                return Err("the thread is deeper than the hole".into());
            }
        } else if matches!(self.hole_type, HoleType::Tapped) {
            return Err("select the thread".into());
        }
        if self.far_csink {
            if !(self.far_diameter().is_finite() && self.far_diameter() / 2.0 > r) {
                return Err("the far side countersink must be wider than the hole".into());
            }
            let a = self.far_angle();
            if !(a > 1e-3 && a < std::f64::consts::PI - 1e-3) {
                return Err(
                    "the far side countersink angle must be between 0 and 180 degrees".into(),
                );
            }
        }
        Ok(())
    }

    /// How far below the start plane the seat goes.
    fn seat_bottom(&self) -> Real {
        match self.seat {
            SeatType::Counterbore | SeatType::Spotface => self.seat_depth,
            SeatType::Countersink => {
                let half = self.csink_angle / 2.0;
                (self.csink_diameter / 2.0 - self.bore_radius()) / half.tan()
            }
            SeatType::None => 0.0,
        }
    }

    /// The depth of the cylinder below the start plane for a resolved
    /// `depth` (the spotface's floor adds its own).
    fn bore_bottom(&self, depth: Real) -> Real {
        if self.seat == SeatType::Spotface {
            depth + self.seat_depth
        } else {
            depth
        }
    }

    /// How deep the bore's cylindrical wall goes for the resolved `depth`
    /// (below it only the drill point, if any).
    pub fn wall_bottom(&self, depth: Real) -> Real {
        let zb = self.bore_bottom(depth);
        if self.drill_point == DrillPoint::AngleY {
            zb - self.bore_radius() / (self.point_angle / 2.0).tan()
        } else {
            zb
        }
    }

    /// The far side countersink's diameter (twice the bore + 1 mm when not
    /// set: a 0.5 mm chamfer).
    pub fn far_diameter(&self) -> Real {
        if self.far_csink_diameter > 0.0 {
            self.far_csink_diameter
        } else {
            2.0 * self.bore_radius() + 1.0
        }
    }

    /// The far side countersink's included angle (90° when not set).
    pub fn far_angle(&self) -> Real {
        if self.far_csink_angle > 0.0 {
            self.far_csink_angle
        } else {
            csink_angle()
        }
    }

    /// The far side countersink as a tool profile `(r, z)` (z along the
    /// hole's up, the far face at `-exit`): from `margin` past the face into
    /// the air to just inside the bore.
    pub fn far_tool(&self, exit: Real, margin: Real) -> Vec<([Real; 2], &'static str)> {
        let half = self.far_angle() / 2.0;
        let rf = self.far_diameter() / 2.0;
        let r_end = 0.9 * self.bore_radius();
        let z_end = -exit + (rf - r_end) / half.tan();
        let zb = -exit - margin;
        vec![
            ([0.0, z_end], "FarCSinkEnd"),
            ([r_end, z_end], "FarCSink"),
            ([rf + margin * half.tan(), zb], "FarBottom"),
            ([0.0, zb], "Axis"),
        ]
    }

    /// The hole's tools of revolution, each a profile `[r, z]` with the role
    /// of each segment (from that point to the next): z up out of the
    /// material, from `top` (above the start plane, in the air) down to
    /// the resolved `depth`; `point` puts the drill point at the bottom.
    /// The seat (if any) comes first, the bore last: like SolidCraft's
    /// `hole_tools_and_prisms`, wide and shallow first, then the narrow hole
    /// through the seat's floor (one stepped tool per hole made 36
    /// counterbores take 43 s instead of 2 s).
    pub fn tools(
        &self,
        depth: Real,
        top: Real,
        point: bool,
    ) -> Vec<Vec<([Real; 2], &'static str)>> {
        let r = self.bore_radius();
        let mut out = Vec::new();
        match self.seat {
            SeatType::None => {}
            SeatType::Counterbore | SeatType::Spotface => {
                let (rc, dc) = (self.seat_diameter / 2.0, self.seat_depth);
                let (wall, floor) = if self.seat == SeatType::Counterbore {
                    ("CBore", "CBoreFloor")
                } else {
                    ("Spotface", "SpotfaceFloor")
                };
                // A spotface 0 deep seats on the face: nothing to cut.
                if dc > 1e-9 {
                    out.push(vec![
                        ([0.0, top], "Top"),
                        ([rc, top], wall),
                        ([rc, -dc], floor),
                        ([0.0, -dc], "Axis"),
                    ]);
                }
            }
            SeatType::Countersink => {
                // The cone from above the face to just inside the bore.
                let half = self.csink_angle / 2.0;
                let rt = self.csink_diameter / 2.0 + top * half.tan();
                let r_end = 0.9 * r;
                let z_end = -(self.csink_diameter / 2.0 - r_end) / half.tan();
                out.push(vec![
                    ([0.0, top], "Top"),
                    ([rt, top], "CSink"),
                    ([r_end, z_end], "CSinkEnd"),
                    ([0.0, z_end], "Axis"),
                ]);
            }
        }
        let zb = -self.bore_bottom(depth);
        let mut bore = vec![([0.0, top], "Top"), ([r, top], "Bore")];
        if point && self.drill_point.angled() {
            let tip = r / (self.point_angle / 2.0).tan();
            // Angle Y: the point is part of the depth.
            let zc = if self.drill_point == DrillPoint::AngleY {
                zb + tip
            } else {
                zb
            };
            bore.push(([r, zc], "Tip"));
            bore.push(([0.0, zc - tip], "Axis"));
        } else {
            bore.push(([r, zb], "Bottom"));
            bore.push(([0.0, zb], "Axis"));
        }
        out.push(bore);
        out
    }

    /// The thread length below the start plane for a resolved hole depth.
    pub fn thread_length(&self, depth: Real) -> Real {
        match self.tapped_thread() {
            Some(t) if !t.full_depth => t.depth.min(self.bore_bottom(depth)),
            _ => self.bore_bottom(depth),
        }
    }

    /// The hole note as Inventor writes it on a drawing, one line per
    /// part: thread, drill, seat — e.g. `Ø6.6 THRU`,
    /// `M8x1.25 - 6H ↧ 12` / `Ø6.75 ↧ 15`, `⌴ Ø11 ↧ 6.4`, `⌵ Ø12 X 90°`.
    /// Numbers have at most two decimals.
    pub fn note(&self) -> String {
        self.note_lines(2).join("\n")
    }

    /// [`HoleSpec::note`] on one line, parts joined by ` / `.
    pub fn note_line(&self) -> String {
        self.note_lines(2).join(" / ")
    }

    /// The note with text instead of the drawing symbols (`DEEP`, `CBORE`,
    /// `CSK`), for fonts without them.
    pub fn note_ascii(&self) -> String {
        ascii(&self.note_line())
    }

    /// The note's lines with `digits` decimals at most.
    pub fn note_lines(&self, digits: usize) -> Vec<String> {
        let n = |x: Real| num(x, digits);
        let through = self.termination == Termination::ThroughAll;
        let depth_text = |d: Real| {
            if through {
                " THRU".to_owned()
            } else {
                format!(" ↧ {}", n(d))
            }
        };
        let mut lines = Vec::new();
        let depth = if self.termination == Termination::Distance {
            self.depth
        } else {
            0.0
        };
        match self.tapped_thread() {
            Some(t) => {
                let len = if t.full_depth { depth } else { t.depth };
                let thread_through = through && t.full_depth;
                lines.push(format!(
                    "{}{}",
                    t.note(),
                    if thread_through {
                        " THRU".to_owned()
                    } else {
                        format!(" ↧ {}", n(len))
                    }
                ));
                if !thread_through {
                    lines.push(format!("Ø{}{}", n(t.tap_drill), depth_text(depth)));
                }
            }
            None => lines.push(format!("Ø{}{}", n(self.diameter), depth_text(depth))),
        }
        match self.seat {
            SeatType::None => {}
            SeatType::Counterbore => lines.push(format!(
                "⌴ Ø{} ↧ {}",
                n(self.seat_diameter),
                n(self.seat_depth)
            )),
            SeatType::Spotface => lines.push(format!(
                "⌴ SF Ø{} ↧ {}",
                n(self.seat_diameter),
                n(self.seat_depth)
            )),
            SeatType::Countersink => lines.push(format!(
                "⌵ Ø{} X {}°",
                n(self.csink_diameter),
                n(self.csink_angle.to_degrees())
            )),
        }
        if self.far_csink {
            lines.push(format!(
                "FAR SIDE ⌵ Ø{} X {}°",
                n(self.far_diameter()),
                n(self.far_angle().to_degrees())
            ));
        }
        lines
    }
}

/// The drawing symbols as text (`↧` DEEP, `⌴` CBORE, `⌵` CSK).
pub fn ascii(s: &str) -> String {
    s.replace("⌴ SF", "SFACE")
        .replace('↧', "DEEP")
        .replace('⌴', "CBORE")
        .replace('⌵', "CSK")
}

/// `x` with at most `digits` decimals, no trailing zeros.
pub fn num(x: Real, digits: usize) -> String {
    let s = format!("{x:.digits$}");
    let s = if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        s
    };
    if s == "-0" { "0".into() } else { s }
}

/// A hole centre.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum HoleCenter {
    /// A point of the feature's sketch; the hole goes square to the sketch
    /// plane.
    Point(PointId),
    /// The centre of a circular edge, square to the planar face it bounds.
    Concentric(EdgeRef),
    /// Concentric to a cylindrical face: on its axis at the end nearest the
    /// face's hint, drilled along the axis into the face (format revision 10).
    Axis(FaceRef),
}

/// The Hole feature: one [`HoleSpec`] at every centre.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HoleFeature {
    /// Immutable identity.
    pub id: String,
    /// Display name.
    pub name: String,
    /// The sketch holding the centre points (`id`); empty when every centre
    /// is concentric.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sketch: String,
    /// The centres, in hole order (`k` = index + 1 in face names).
    pub centers: Vec<HoleCenter>,
    /// What every hole is.
    pub spec: HoleSpec,
    /// The solids to drill (persistent body names); empty: all.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub solids: Vec<String>,
    /// Equations of its values.
    #[serde(default, skip_serializing_if = "FeatureParams::is_empty")]
    pub params: FeatureParams,
}

impl HoleFeature {
    /// Its numeric fields that are parameters (they follow the options).
    pub fn fields(&self) -> Vec<(&'static str, Unit)> {
        let s = &self.spec;
        let mut v = Vec::new();
        if s.hole_type != HoleType::Tapped {
            v.push(("diameter", Unit::Mm));
        }
        if s.termination == Termination::Distance {
            v.push(("depth", Unit::Mm));
            if s.drill_point.angled() {
                v.push(("point_angle", Unit::Deg));
            }
        }
        match s.seat {
            SeatType::Counterbore | SeatType::Spotface => {
                v.extend([("seat_diameter", Unit::Mm), ("seat_depth", Unit::Mm)]);
            }
            SeatType::Countersink => {
                v.extend([("csink_diameter", Unit::Mm), ("csink_angle", Unit::Deg)]);
            }
            SeatType::None => {}
        }
        if s.far_csink {
            v.extend([
                ("far_csink_diameter", Unit::Mm),
                ("far_csink_angle", Unit::Deg),
            ]);
        }
        if let Some(t) = s.tapped_thread() {
            if !t.full_depth {
                v.push(("thread_depth", Unit::Mm));
            }
            if t.modeled {
                v.push(("thread_clearance", Unit::Mm));
            }
        }
        v
    }

    /// The value of field `f`.
    pub fn value(&self, f: &str) -> Option<Real> {
        let s = &self.spec;
        Some(match f {
            "diameter" => s.diameter,
            "depth" => s.depth,
            "point_angle" => s.point_angle,
            "seat_diameter" => s.seat_diameter,
            "seat_depth" => s.seat_depth,
            "csink_diameter" => s.csink_diameter,
            "csink_angle" => s.csink_angle,
            "far_csink_diameter" => s.far_diameter(),
            "far_csink_angle" => s.far_angle(),
            "thread_depth" => s.thread.as_ref()?.depth,
            "thread_clearance" => s.thread.as_ref()?.clearance,
            _ => return None,
        })
    }

    /// Sets field `f`.
    pub fn set(&mut self, f: &str, v: Real) {
        let s = &mut self.spec;
        match f {
            "diameter" => s.diameter = v,
            "depth" => s.depth = v,
            "point_angle" => s.point_angle = v,
            "seat_diameter" => s.seat_diameter = v,
            "seat_depth" => s.seat_depth = v,
            "csink_diameter" => s.csink_diameter = v,
            "csink_angle" => s.csink_angle = v,
            "far_csink_diameter" => s.far_csink_diameter = v,
            "far_csink_angle" => s.far_csink_angle = v,
            "thread_depth" => {
                if let Some(t) = s.thread.as_mut() {
                    t.depth = v;
                }
            }
            "thread_clearance" => {
                if let Some(t) = s.thread.as_mut() {
                    t.clearance = v;
                }
            }
            _ => {}
        }
    }

    /// A hole feature with `spec` and no centres yet.
    pub fn new(spec: HoleSpec) -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            sketch: String::new(),
            centers: Vec::new(),
            spec,
            solids: Vec::new(),
            params: FeatureParams::default(),
        }
    }
}

/// The Thread feature: a thread on a cylindrical face.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ThreadFeature {
    /// Immutable identity.
    pub id: String,
    /// Display name.
    pub name: String,
    /// The cylindrical face; the hint picks the end the thread starts at.
    pub face: FaceRef,
    /// The thread (full depth = along the whole face).
    pub spec: ThreadSpec,
    /// Offset from the start end (when not full depth); `offset`.
    #[serde(default)]
    pub offset: Real,
    /// Show the cosmetic thread in the model (Inventor: Display thread in model);
    /// the thread length when not full depth is `spec.depth` (field `length`).
    #[serde(default = "yes")]
    pub display: bool,
    /// Equations of its values.
    #[serde(default, skip_serializing_if = "FeatureParams::is_empty")]
    pub params: FeatureParams,
}

/// A cylindrical face resolved for a thread: axis from the start end into
/// the face, radius, length, internal (a hole wall) or external (a shaft).
#[derive(Clone, Debug, PartialEq)]
pub struct CylinderFace {
    /// The axis point at the start end.
    pub start: Point3,
    /// Unit axis from the start end along the face.
    pub axis: Vector3,
    /// Radius.
    pub radius: Real,
    /// Length along the axis.
    pub length: Real,
    /// A hole wall (the material is outside the cylinder).
    pub internal: bool,
}

/// The cylindrical face named `r.face` nearest its hint, with the end
/// nearest the hint as the start.
pub fn cylinder_face(body: &BRep, r: &FaceRef) -> Result<CylinderFace, String> {
    let hint = Point3::from(r.hint);
    // Every piece of the face with that name (a Boolean may split it).
    let pieces: Vec<_> = body
        .faces()
        .filter(|(_, f)| f.name.as_deref() == Some(r.face.as_str()))
        .collect();
    let f = super::shell::face_near(body, &r.face, hint)
        .ok_or_else(|| format!("the face {} no longer exists", r.face))?;
    let face = body
        .face(f)
        .ok_or_else(|| format!("the face {} no longer exists", r.face))?;
    let surf = face
        .facet_of
        .and_then(|s| body.surface(s))
        .or_else(|| body.surface(face.surface));
    let Some(Surface::Cylinder(c)) = surf else {
        return Err("select a cylindrical face".into());
    };
    let k = c.axis.into_inner();
    let mut lo = Real::INFINITY;
    let mut hi = Real::NEG_INFINITY;
    for (_, pf) in &pieces {
        let same = match pf
            .facet_of
            .and_then(|s| body.surface(s))
            .or_else(|| body.surface(pf.surface))
        {
            Some(Surface::Cylinder(o)) => {
                (o.radius - c.radius).abs() < 1e-6
                    && o.axis.cross(&c.axis).norm() < 1e-6
                    && (o.origin - c.origin).cross(&k).norm() < 1e-6
            }
            _ => false,
        };
        if !same {
            continue;
        }
        if let Some(m) = &pf.mesh {
            for p in &m.positions {
                let h = (p - c.origin).dot(&k);
                lo = lo.min(h);
                hi = hi.max(h);
            }
        }
    }
    if !(hi.is_finite() && hi > lo) {
        return Err("the cylindrical face has no length".into());
    }
    // A hole wall faces the axis: its outward normals point inwards.
    let radial_sense = face.mesh.as_ref().and_then(|m| {
        let s: Real = m
            .positions
            .iter()
            .zip(&m.normals)
            .map(|(p, n)| {
                let v = p - c.origin;
                (v - k * v.dot(&k)).dot(n)
            })
            .sum();
        (s.abs() > 1e-12).then_some(s < 0.0)
    });
    let internal = radial_sense.unwrap_or(face.sense == crate::kernel::Sense::Reversed);
    let h = (hint - c.origin).dot(&k);
    let (start, axis) = if (h - lo).abs() <= (hi - h).abs() {
        (c.origin + k * lo, k)
    } else {
        (c.origin + k * hi, -k)
    };
    Ok(CylinderFace {
        start,
        axis,
        radius: c.radius,
        length: hi - lo,
        internal,
    })
}

impl ThreadFeature {
    /// Its numeric fields that are parameters.
    pub fn fields(&self) -> Vec<(&'static str, Unit)> {
        let mut v = Vec::new();
        if !self.spec.full_depth {
            v.extend([("length", Unit::Mm), ("offset", Unit::Mm)]);
        }
        if self.spec.modeled {
            v.push(("clearance", Unit::Mm));
        }
        v
    }

    /// The value of field `f`.
    pub fn value(&self, f: &str) -> Option<Real> {
        Some(match f {
            "length" => self.spec.depth,
            "offset" => self.offset,
            "clearance" => self.spec.clearance,
            _ => return None,
        })
    }

    /// Sets field `f`.
    pub fn set(&mut self, f: &str, v: Real) {
        match f {
            "length" => self.spec.depth = v,
            "offset" => self.offset = v,
            "clearance" => self.spec.clearance = v,
            _ => {}
        }
    }

    /// A thread on `face` with `spec`.
    pub fn new(face: FaceRef, spec: ThreadSpec) -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            face,
            spec,
            offset: 0.0,
            display: true,
            params: FeatureParams::default(),
        }
    }

    /// The threaded stretch: (face, from, to) along its axis.
    pub fn stretch(&self, body: &BRep) -> Result<(CylinderFace, Real, Real), String> {
        let cf = cylinder_face(body, &self.face)?;
        self.spec.validate()?;
        // SolidCraft #2705 / #2969: a size must match the face (the shaft
        // its major diameter, a hole its minor diameter or tap drill).
        let d = 2.0 * cf.radius;
        let fits = if cf.internal {
            (d - self.spec.minor).abs() <= 0.15 * self.spec.major
                || (d - self.spec.tap_drill).abs() <= 0.15 * self.spec.major
        } else {
            (d - self.spec.major).abs() <= 0.15 * self.spec.major
        };
        if !fits {
            return Err(format!(
                "an {} thread does not fit a Ø{} {}",
                self.spec.designation,
                num(d, 3),
                if cf.internal { "hole" } else { "shaft" }
            ));
        }
        let (a, b) = if self.spec.full_depth {
            (0.0, cf.length)
        } else {
            if !(self.offset.is_finite() && self.offset >= 0.0) {
                return Err("the offset must not be negative".into());
            }
            (self.offset, self.offset + self.spec.depth)
        };
        if b > cf.length + 1e-6 {
            return Err("the thread is longer than the face".into());
        }
        Ok((cf, a, b))
    }
}

/// A cosmetic thread as the view shows it (a display hint).
#[derive(Clone, Debug, PartialEq)]
pub struct CosmeticThread {
    /// The feature.
    pub feature: String,
    /// Start of the thread on its axis.
    pub start: Point3,
    /// Unit axis along the thread.
    pub axis: Vector3,
    /// Radius of the thread's other diameter (a hole: the major radius; a
    /// shaft: the minor radius).
    pub radius: Real,
    /// Thread length.
    pub length: Real,
    /// Pitch.
    pub pitch: Real,
    /// Drawn in the model (Thread: Display thread in model).
    pub shown: bool,
    /// Radius of the threaded face itself (a hole: the drill; a shaft: its
    /// radius): the thread texture lies on it.
    pub face_radius: Real,
    /// An internal thread (a hole wall, the material outside the cylinder).
    pub internal: bool,
    /// Right-hand thread (the bands lean the other way for a left-hand one).
    pub right_hand: bool,
}

impl CosmeticThread {
    /// The thread moved by the rigid motion `m` (Pattern / Mirror copies). A
    /// mirror (negative determinant) turns a right-hand thread left.
    pub fn transformed(&self, m: &nalgebra::Matrix4<Real>) -> Self {
        let a = m.transform_vector(&self.axis);
        let det = m.fixed_view::<3, 3>(0, 0).determinant();
        Self {
            start: m.transform_point(&self.start),
            axis: a.try_normalize(1e-12).unwrap_or(self.axis),
            right_hand: if det < 0.0 {
                !self.right_hand
            } else {
                self.right_hand
            },
            ..self.clone()
        }
    }

    /// Segments around the face for the texture mesh: chords within
    /// [`BAND_SAG`] of the circle.
    fn band_segments(&self) -> usize {
        let r = self.face_radius.max(1e-6);
        let c = (1.0 - BAND_SAG / r).clamp(-1.0, 1.0);
        let n = std::f64::consts::PI / c.acos().max(1e-9);
        (n.ceil() as usize).clamp(16, 192)
    }
}

/// Largest chord sag (mm) of the thread texture's own mesh.
pub const BAND_SAG: Real = 0.004;

/// How far the thread texture floats off its face (mm), towards the air:
/// more than the display tolerance of the body's triangles (0.01 mm) plus
/// [`BAND_SAG`], so the face never pokes through it, and small enough not
/// to be seen as a step at the rim.
pub const BAND_LIFT: Real = 0.016;

/// The display mesh of cosmetic threads (Inventor shows a thread as a
/// texture on its face): one sleeve per thread on the threaded face, only
/// over the thread length, lifted by [`BAND_LIFT`] towards the air. Each
/// vertex carries the thread's *phase* `h / pitch ∓ θ / 2π`: whole numbers
/// lie on one helix of the thread, so a shader painting `fract(phase)` as
/// light/dark draws one band per pitch, leaning with the hand.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ThreadBands {
    /// Vertices: two per angle step (the thread's start and end).
    pub positions: Vec<Point3>,
    /// Unit normals towards the air (front faces, CCW seen from there).
    pub normals: Vec<Vector3>,
    /// The thread phase of each vertex (one per pitch).
    pub phase: Vec<f32>,
    /// Triangles into `positions`.
    pub triangles: Vec<[u32; 3]>,
}

impl ThreadBands {
    /// The sleeves of every shown thread of `threads`, in one mesh.
    pub fn of(threads: &[CosmeticThread]) -> Self {
        let mut out = Self::default();
        for c in threads.iter().filter(|c| c.shown) {
            out.add(c);
        }
        out
    }

    fn add(&mut self, c: &CosmeticThread) {
        use std::f64::consts::TAU;
        let k = c.axis;
        if !(c.length.is_finite() && c.length > 1e-9 && c.face_radius > 1e-9) || k.norm() < 0.5 {
            return;
        }
        let any = if k.x.abs() < 0.9 {
            Vector3::x()
        } else {
            Vector3::y()
        };
        let u = (any - k * any.dot(&k)).normalize();
        let w = k.cross(&u);
        let r = if c.internal {
            (c.face_radius - BAND_LIFT).max(c.face_radius * 0.5)
        } else {
            c.face_radius + BAND_LIFT
        };
        let turns = c.length / c.pitch.max(1e-3);
        let hand = if c.right_hand { 1.0 } else { -1.0 };
        let n = c.band_segments();
        let base = self.positions.len() as u32;
        for i in 0..=n {
            let t = i as Real / n as Real * TAU;
            let radial = u * t.cos() + w * t.sin();
            let normal = if c.internal { -radial } else { radial };
            for (h, ph) in [(0.0, 0.0), (c.length, turns)] {
                self.positions.push(c.start + k * h + radial * r);
                self.normals.push(normal);
                self.phase.push((ph - hand * i as Real / n as Real) as f32);
            }
        }
        // Quad i: vertices 2i (start), 2i+1 (end), 2i+2, 2i+3. (next − this
        // around) × (along the axis) points outwards, so a shaft's sleeve is
        // (a, c, b) and a hole's, seen from the axis, the reverse.
        for i in 0..n as u32 {
            let (a, b, c2, d) = (
                base + 2 * i,
                base + 2 * i + 1,
                base + 2 * i + 2,
                base + 2 * i + 3,
            );
            if c.internal {
                self.triangles.push([a, b, c2]);
                self.triangles.push([c2, b, d]);
            } else {
                self.triangles.push([a, c2, b]);
                self.triangles.push([c2, d, b]);
            }
        }
    }
}

/// One hole of the part (drawing data: Hole Table, hole notes).
#[derive(Clone, Debug, PartialEq)]
pub struct HoleInstance {
    /// The Hole feature's id.
    pub feature: String,
    /// The hole's number in its feature (from 1, as in face names).
    pub occurrence: usize,
    /// What it is.
    pub spec: HoleSpec,
    /// The centre on the start plane.
    pub origin: Point3,
    /// Unit drilling direction (into the material).
    pub direction: Vector3,
    /// The face it starts on (persistent name), if it starts on one.
    pub start_face: Option<String>,
    /// Pattern copies: the indices of the pattern elements it was copied
    /// through (empty for the original).
    pub copy: Vec<usize>,
}

impl HoleInstance {
    /// Identity of the hole in its part: feature, number and pattern copy
    /// (`Hole1:2`, `Hole1:2#3`), stable while the feature keeps its holes.
    pub fn key(&self) -> String {
        let mut k = format!("{}:{}", self.feature, self.occurrence);
        for c in &self.copy {
            k.push_str(&format!("#{c}"));
        }
        k
    }

    /// The hole moved by the rigid motion `m`: the hook for Pattern / Mirror
    /// (each copy of a Hole feature becomes `h.transformed(&m)` with its
    /// element index pushed onto `copy`). A mirror keeps the drilling
    /// direction a direction (the matrix's linear part applies to it).
    pub fn transformed(&self, m: &nalgebra::Matrix4<Real>) -> Self {
        let p = m.transform_point(&self.origin);
        let d = m.transform_vector(&self.direction);
        Self {
            origin: p,
            direction: d.try_normalize(1e-12).unwrap_or(self.direction),
            ..self.clone()
        }
    }
}

/// Identical holes on the same face (Hole Table rows, the `4x` of a note).
#[derive(Clone, Debug, PartialEq)]
pub struct HoleGroup {
    /// The face the first of them starts on.
    pub start_face: Option<String>,
    /// The note of each of them.
    pub note: String,
    /// The holes: (feature id, occurrence).
    pub members: Vec<(String, usize)>,
    /// Their keys (`HoleInstance::key`; pattern copies count one each).
    pub keys: Vec<String>,
    /// Drilling direction (unit) and the start plane's offset along it.
    plane: (Vector3, Real),
}

impl HoleGroup {
    /// How many holes.
    pub fn count(&self) -> usize {
        self.keys.len()
    }

    /// The note with the quantity: `4x Ø6.6 THRU` (no prefix for one hole).
    pub fn label(&self) -> String {
        if self.count() > 1 {
            format!("{}x {}", self.count(), self.note)
        } else {
            self.note.clone()
        }
    }
}

/// Groups identical holes (same note) that start on one plane and drill
/// the same way, from any Hole feature, pattern or mirror copy, in order of
/// first appearance: a drawing's note counts them all (`4x Ø6.6 THRU`),
/// also across a face split in pieces or on a work plane.
pub fn group_holes(holes: &[HoleInstance]) -> Vec<HoleGroup> {
    let mut out: Vec<HoleGroup> = Vec::new();
    let size = holes
        .iter()
        .map(|h| h.origin.coords.norm())
        .fold(1.0, Real::max);
    for h in holes {
        let note = h.spec.note();
        let d = h.direction.normalize();
        let off = h.origin.coords.dot(&d);
        let same = |g: &HoleGroup| {
            g.note == note
                && g.plane.0.dot(&d) > 1.0 - 1e-9
                && (g.plane.1 - off).abs() <= 1e-6 * size
        };
        match out.iter_mut().find(|g| same(g)) {
            Some(g) => {
                g.members.push((h.feature.clone(), h.occurrence));
                g.keys.push(h.key());
            }
            None => out.push(HoleGroup {
                start_face: h.start_face.clone(),
                note,
                members: vec![(h.feature.clone(), h.occurrence)],
                keys: vec![h.key()],
                plane: (d, off),
            }),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(feature: &str, k: usize, o: [Real; 3], dir: [Real; 3], face: &str) -> HoleInstance {
        HoleInstance {
            feature: feature.into(),
            occurrence: k,
            spec: HoleSpec::simple(6.0, 10.0),
            origin: Point3::new(o[0], o[1], o[2]),
            direction: Vector3::new(dir[0], dir[1], dir[2]),
            start_face: Some(face.into()),
            copy: Vec::new(),
        }
    }

    #[test]
    fn identical_holes_group_by_plane() {
        let down = [0.0, 0.0, -1.0];
        let holes = vec![
            // One plane (z = 10) in two face pieces, two features.
            at("Hole1", 1, [0.0, 0.0, 10.0], down, "Box1:Top~Split1.1"),
            at("Hole2", 1, [30.0, 0.0, 10.0], down, "Box1:Top~Split1.2"),
            // A pattern copy of Hole1's hole.
            HoleInstance {
                copy: vec![1],
                ..at("Hole1", 1, [0.0, 20.0, 10.0], down, "Box1:Top~Split1.1")
            },
            // Another plane, and the same plane drilled the other way.
            at("Hole3", 1, [0.0, 0.0, 5.0], down, "Step:Top"),
            at("Hole4", 1, [0.0, 0.0, 10.0], [0.0, 0.0, 1.0], "Box1:Top"),
        ];
        let g = group_holes(&holes);
        assert_eq!(g.len(), 3, "{g:?}");
        assert_eq!(g[0].count(), 3);
        assert_eq!(g[0].label(), "3x Ø6 ↧ 10");
        assert_eq!(g[0].keys, ["Hole1:1", "Hole2:1", "Hole1:1#1"]);
        assert_eq!((g[1].count(), g[2].count()), (1, 1));
    }

    fn m8() -> ThreadSpec {
        ThreadSpec::from_table(&Standards::builtin(), standards::ISO_METRIC, "M8", true).unwrap()
    }

    fn cosmetic(internal: bool, right_hand: bool) -> CosmeticThread {
        CosmeticThread {
            feature: "Hole1".into(),
            start: Point3::new(5.0, 0.0, 10.0),
            axis: -Vector3::z(),
            radius: 4.0,
            length: 10.0,
            pitch: 1.25,
            shown: true,
            face_radius: if internal { 3.375 } else { 4.0 },
            internal,
            right_hand,
        }
    }

    /// Phase difference along the axis at the same angle (vertex pairs).
    fn axial_bands(b: &ThreadBands) -> Vec<f32> {
        b.phase.chunks(2).map(|p| p[1] - p[0]).collect()
    }

    #[test]
    fn thread_bands_follow_pitch_radius_and_length() {
        for internal in [true, false] {
            let c = cosmetic(internal, true);
            let b = ThreadBands::of(std::slice::from_ref(&c));
            // One band per pitch: 10 / 1.25 = 8 along the axis everywhere.
            assert!(axial_bands(&b).iter().all(|d| (d - 8.0).abs() < 1e-5));
            let want = if internal {
                3.375 - BAND_LIFT
            } else {
                4.0 + BAND_LIFT
            };
            for (p, n) in b.positions.iter().zip(&b.normals) {
                let radial = Vector3::new(p.x - 5.0, p.y, 0.0);
                assert!((radial.norm() - want).abs() < 1e-9);
                // Only over the thread: z from 10 down to 0.
                assert!(p.z > -1e-9 && p.z < 10.0 + 1e-9);
                // Normals towards the air: into the hole, out of a shaft.
                assert_eq!(n.dot(&radial) > 0.0, !internal);
            }
            // Front faces: each triangle wound CCW seen from its normal.
            for t in &b.triangles {
                let [a, q, r] = t.map(|i| b.positions[i as usize]);
                let n = b.normals[t[0] as usize];
                assert!((q - a).cross(&(r - a)).dot(&n) > 0.0, "internal {internal}");
            }
            // The mesh's own chords stay within BAND_SAG of the sleeve.
            let seg = c.band_segments() as Real;
            assert!(want * (1.0 - (std::f64::consts::PI / seg).cos()) <= BAND_SAG + 1e-12);
            assert_eq!(b.positions.len(), 2 * (c.band_segments() + 1));
        }
    }

    #[test]
    fn thread_bands_lean_with_the_hand_and_hide() {
        let around = |c: &CosmeticThread| {
            let b = ThreadBands::of(std::slice::from_ref(c));
            b.phase[2] - b.phase[0]
        };
        let r = around(&cosmetic(true, true));
        let l = around(&cosmetic(true, false));
        assert!(r < 0.0 && (r + l).abs() < 1e-6, "{r} {l}");
        let mut hidden = cosmetic(true, true);
        hidden.shown = false;
        assert_eq!(ThreadBands::of(&[hidden]), ThreadBands::default());
        // Two threads: one mesh with both sleeves.
        let two = ThreadBands::of(&[cosmetic(true, true), cosmetic(false, true)]);
        let one = ThreadBands::of(&[cosmetic(true, true)]);
        assert!(two.positions.len() > one.positions.len());
        assert!(
            two.triangles
                .iter()
                .flatten()
                .all(|&i| (i as usize) < two.positions.len())
        );
        // A mirror turns a right-hand thread left; a move keeps it.
        let mut m = nalgebra::Matrix4::<Real>::identity();
        m[(0, 0)] = -1.0;
        let c = cosmetic(true, true).transformed(&m);
        assert!(!c.right_hand && (c.start.x + 5.0).abs() < 1e-12);
        let t = nalgebra::Matrix4::new_translation(&Vector3::new(1.0, 2.0, 0.0));
        let c = cosmetic(true, true).transformed(&t);
        assert!(c.right_hand && (c.start - Point3::new(6.0, 2.0, 10.0)).norm() < 1e-12);
    }

    #[test]
    fn thread_bands_of_a_thousand_holes_are_quick() {
        let threads: Vec<CosmeticThread> = (0..1000)
            .map(|k| {
                let mut c = cosmetic(true, true);
                c.start = Point3::new((k % 32) as Real * 12.0, (k / 32) as Real * 12.0, 10.0);
                c
            })
            .collect();
        let t = std::time::Instant::now();
        let b = ThreadBands::of(&threads);
        let ms = t.elapsed().as_secs_f64() * 1e3;
        assert_eq!(
            b.positions.len(),
            1000 * 2 * (threads[0].band_segments() + 1)
        );
        // About 1 ms in a debug build; generous for a busy machine.
        assert!(ms < 500.0, "{ms:.1} ms");
    }

    #[test]
    fn notes_of_every_kind() {
        let t = Standards::builtin();
        let mut h = HoleSpec::clearance(&t, "ISO 273", "M6", FastenerFit::Normal).unwrap();
        assert_eq!(h.diameter, 6.6);
        assert_eq!(h.note(), "Ø6.6 THRU");
        h.termination = Termination::Distance;
        h.depth = 12.0;
        assert_eq!(h.note(), "Ø6.6 ↧ 12");
        let mut tap = HoleSpec::tapped(m8(), 15.0);
        tap.thread.as_mut().unwrap().full_depth = false;
        tap.thread.as_mut().unwrap().depth = 12.0;
        assert_eq!(tap.note(), "M8x1.25 - 6H ↧ 12\nØ6.75 ↧ 15");
        assert_eq!(tap.note_line(), "M8x1.25 - 6H ↧ 12 / Ø6.75 ↧ 15");
        assert_eq!(tap.note_lines(1)[1], "Ø6.8 ↧ 15");
        tap.termination = Termination::ThroughAll;
        tap.thread.as_mut().unwrap().full_depth = true;
        tap.thread.as_mut().unwrap().right_hand = false;
        assert_eq!(tap.note(), "M8x1.25 - 6H LH THRU");
        let mut cb = HoleSpec::clearance(&t, "ISO 273", "M6", FastenerFit::Normal).unwrap();
        assert!(cb.counterbore_from(&t, "Socket Head Cap Screw ISO 4762", "M6"));
        assert_eq!(cb.note(), "Ø6.6 THRU\n⌴ Ø11 ↧ 6.4");
        assert_eq!(cb.note_ascii(), "Ø6.6 THRU / CBORE Ø11 DEEP 6.4");
        let mut cs = HoleSpec::clearance(&t, "ISO 273", "M6", FastenerFit::Normal).unwrap();
        cs.seat = SeatType::Countersink;
        cs.csink_diameter = 12.0;
        cs.csink_angle = 90f64.to_radians();
        assert_eq!(cs.note(), "Ø6.6 THRU\n⌵ Ø12 X 90°");
        let mut sf = HoleSpec::simple(9.0, 20.0);
        sf.seat = SeatType::Spotface;
        sf.seat_diameter = 20.0;
        sf.seat_depth = 1.0;
        assert_eq!(sf.note(), "Ø9 ↧ 20\n⌴ SF Ø20 ↧ 1");
        let un = ThreadSpec::from_table(&t, standards::ANSI_UNIFIED, "1/4-20 UNC", true).unwrap();
        assert_eq!(un.note(), "1/4-20 UNC - 2B");
        assert_eq!(num(6.647, 2), "6.65");
        assert_eq!(num(-0.0001, 2), "0");
    }

    #[test]
    fn tools_follow_the_seat_and_point() {
        fn roles<'a>(t: &[([Real; 2], &'a str)]) -> Vec<&'a str> {
            t.iter().map(|x| x.1).collect()
        }
        let mut h = HoleSpec::simple(6.0, 10.0);
        let t = h.tools(10.0, 1.0, true);
        assert_eq!(t.len(), 1);
        // Top cap, bore, tip, then back up the axis.
        assert_eq!(roles(&t[0]), ["Top", "Bore", "Tip", "Axis"]);
        let tip = 3.0 / (59f64.to_radians()).tan();
        assert!((t[0][3].0[1] - (-10.0 - tip)).abs() < 1e-12);
        h.drill_point = DrillPoint::Flat;
        h.seat = SeatType::Counterbore;
        h.seat_diameter = 11.0;
        h.seat_depth = 6.0;
        let t = h.tools(10.0, 1.0, true);
        assert_eq!(roles(&t[0]), ["Top", "CBore", "CBoreFloor", "Axis"]);
        assert_eq!(roles(&t[1]), ["Top", "Bore", "Bottom", "Axis"]);
        assert!(h.validate().is_ok());
        h.seat_depth = 12.0;
        assert!(h.validate().is_err(), "deeper than the hole");
        h.seat = SeatType::Countersink;
        h.csink_diameter = 12.0;
        let t = h.tools(10.0, 1.0, true);
        // 90°: the cone runs 1 mm above the face 1 mm wider, down to just
        // inside the bore (0.9 r at (6 − 2.7) / tan 45° = 3.3 deep).
        assert!((t[0][1].0[0] - 7.0).abs() < 1e-12, "{t:?}");
        assert!((t[0][2].0[1] + 3.3).abs() < 1e-12, "{t:?}");
        assert!((h.seat_bottom() - 3.0).abs() < 1e-12);
        h.depth = 2.0;
        assert!(h.validate().is_err(), "a countersink deeper than the hole");
        let mut s = HoleSpec::simple(6.0, 10.0);
        s.seat = SeatType::Spotface;
        s.seat_diameter = 14.0;
        s.seat_depth = 0.0;
        assert!(s.validate().is_ok(), "a spotface may be 0 deep");
        assert_eq!(s.tools(10.0, 1.0, true).len(), 1, "nothing to cut for it");
    }
    #[test]
    fn thread_sections_clear_one_pitch() {
        let mut t = m8();
        let g = t.internal_groove(t.minor / 2.0).unwrap();
        let w = g[3][1] - g[0][1];
        assert!(w < t.pitch, "{w}");
        assert!((g[1][0] - 4.0).abs() < 1e-12);
        t.clearance = 0.2;
        let g = t.internal_groove(t.minor / 2.0 + 0.2).unwrap();
        assert!(
            (g[1][0] - 4.2).abs() < 1e-12,
            "the clearance widens the groove"
        );
        let e = m8().external_groove(4.0).unwrap();
        let h = 1.25 * 3f64.sqrt() / 2.0;
        assert!((e[0][0] - (4.0 - 5.0 * h / 8.0)).abs() < 1e-12);
        assert!(m8().internal_groove(1.0).is_err(), "far too small a hole");
        t.clearance = 0.7;
        assert!(t.validate().is_err());
    }

    #[test]
    fn groups_count_identical_holes_across_features() {
        let t = Standards::builtin();
        let a = HoleSpec::clearance(&t, "ISO 273", "M6", FastenerFit::Normal).unwrap();
        let b = HoleSpec::tapped(m8(), 15.0);
        let mk = |f: &str, k: usize, spec: &HoleSpec, face: &str| HoleInstance {
            feature: f.into(),
            occurrence: k,
            spec: spec.clone(),
            origin: Point3::new(k as Real, 0.0, 0.0),
            direction: -Vector3::z(),
            start_face: Some(face.into()),
            copy: Vec::new(),
        };
        let holes = vec![
            mk("Hole1", 1, &a, "Box1:Top"),
            mk("Hole1", 2, &a, "Box1:Top"),
            mk("Hole2", 1, &a, "Box1:Top"),
            mk("Hole2", 2, &a, "Box1:Top"),
            mk("Hole2", 3, &b, "Box1:Top"),
            // The front face: another plane, drilled along Y.
            HoleInstance {
                direction: Vector3::y(),
                ..mk("Hole3", 1, &a, "Box1:Front")
            },
        ];
        let g = group_holes(&holes);
        assert_eq!(g.len(), 3);
        assert_eq!(g[0].label(), "4x Ø6.6 THRU");
        assert_eq!(g[0].members[2], ("Hole2".to_owned(), 1));
        assert_eq!(g[1].count(), 1);
        assert_eq!(g[2].start_face.as_deref(), Some("Box1:Front"));
        // The pattern hook: a copy moved 20 mm along X.
        let m = nalgebra::Matrix4::new_translation(&Vector3::new(20.0, 0.0, 0.0));
        let c = holes[0].transformed(&m);
        assert_eq!(c.origin, Point3::new(21.0, 0.0, 0.0));
        assert_eq!(c.direction, -Vector3::z());
    }
}
