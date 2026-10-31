//! The part's parameter table (Inventor: Manage → Parameters, fx).
//!
//! Every driving sketch dimension and every numeric value of a feature is a
//! **model parameter** with a name (`d0`, `d1`, …); the user adds **user
//! parameters**. Each has a value or an **equation** over the others. See
//! `docs/spec/0.5-parameters.md`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::expr::{self, Unit};
use super::units::{self, UnitDef};
use super::{Feature, PartDocument, PartError};
use crate::kernel::Real;
use crate::sketch::{ConstraintId, Tolerance};

/// A user parameter.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UserParam {
    /// Name.
    pub name: String,
    /// Equation or value (as typed).
    pub expr: String,
    /// Unit / type.
    #[serde(default)]
    pub unit: Unit,
    /// Comment.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
    /// Last evaluated value (internal units).
    #[serde(default)]
    pub value: Real,
    /// A unit specification beyond [`Unit`] (`N/m`, `kg`, `MPa`, `km/h`):
    /// it wins over `unit`, which then holds the nearest old kind for older
    /// builds (format revision 18).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit_spec: Option<String>,
    /// Multi-Value: the values to choose from (each an equation).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<String>,
    /// The evaluated text of a Text parameter (runtime).
    #[serde(skip)]
    pub text: String,
    /// A linked parameter (spec 3.10 §3.9): its value comes from another
    /// document or a CSV file at Update; `expr` holds the last value (so the
    /// part builds alone and old builds read a plain value).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<ParamLink>,
}

/// Reads a linked parameter's source: its value (internal units) and unit.
pub type LinkReader<'a> = dyn Fn(&ParamLink) -> Result<(Real, Unit), String> + 'a;

/// Where a linked user parameter takes its value (spec 3.10 §3.9: Inventor's
/// Link of the Parameters dialog, a master parameter document, a
/// spreadsheet).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "from")]
pub enum ParamLink {
    /// Parameter `name` of another document (a part of this design file or
    /// of a linked file).
    Document {
        /// The document.
        source: crate::document::DocRef,
        /// Its parameter.
        name: String,
    },
    /// Row `name` of a CSV file (`name,value[,unit[,comment]]` per line).
    Csv {
        /// The file.
        path: String,
        /// Its row.
        name: String,
    },
}

impl ParamLink {
    /// The parameter it reads (shown in the table).
    pub fn describe(&self) -> String {
        match self {
            ParamLink::Document { name, .. } => format!("linked: {name}"),
            ParamLink::Csv { path, name } => {
                let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
                format!("linked: {name} ({file})")
            }
        }
    }
}

/// The rows of a CSV parameter file: `name,value[,unit[,comment]]` per line
/// (a header line whose value is no number is skipped, `#` starts a
/// comment). Values with a unit after them (`800 mm`) or in the third
/// column; a bare number takes millimetres for a length unit column, else is
/// unitless.
pub fn csv_parameters(text: &str) -> Vec<(String, Real, Unit)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.split([',', ';', '\t']).map(str::trim).collect();
        let (Some(name), Some(raw)) = (cols.first(), cols.get(1)) else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        let unit_col = cols.get(2).copied().unwrap_or("");
        let unit = Unit::ALL
            .iter()
            .copied()
            .find(|u| u.label() == unit_col)
            .or_else(|| {
                Unit::ALL
                    .iter()
                    .copied()
                    .find(|u| raw.ends_with(u.label()) && !u.label().is_empty())
            })
            .unwrap_or(Unit::Ul);
        let number = raw.trim_end_matches(unit.label()).trim();
        let Ok(v) = number.parse::<Real>() else {
            continue;
        };
        out.push(((*name).to_owned(), unit.from_display(v), unit));
    }
    out
}

/// Key flag and display precision of a parameter (any kind).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ParamMeta {
    /// A key parameter (the table's Key filter).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub key: bool,
    /// Decimals shown (`None`: up to 6, trailing zeros cut).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub precision: Option<u8>,
}

impl UserParam {
    /// The parameter's unit.
    pub fn def(&self) -> UnitDef {
        self.unit_spec
            .as_deref()
            .and_then(|s| units::parse_spec(s).ok())
            .unwrap_or_else(|| self.unit.def())
    }

    /// The unit as shown and typed.
    pub fn unit_label(&self) -> String {
        self.unit_spec
            .clone()
            .unwrap_or_else(|| self.unit.label().to_owned())
    }
}

/// Name and equation of one numeric field of a feature.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FeatureParam {
    /// Parameter name.
    pub name: String,
    /// Equation (`None` = plain value).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expr: Option<String>,
}

/// Parameters of a feature, by field name (`"distance"`, `"angle"`…).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FeatureParams(pub BTreeMap<String, FeatureParam>);

impl FeatureParams {
    /// No parameters recorded yet.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Where a parameter lives.
#[derive(Clone, Debug, PartialEq)]
pub enum ParamOwner {
    /// A physical property of the part (`PHYSICAL[k]`, read only).
    Physical(usize),
    /// A dimension of a sketch.
    Dimension {
        /// Sketch index.
        sketch: usize,
        /// The dimension.
        constraint: ConstraintId,
    },
    /// A numeric field of a feature.
    Feature {
        /// Feature index.
        feature: usize,
        /// Field name.
        field: &'static str,
    },
    /// User parameter `index`.
    User(usize),
}

/// One row of the Parameters table.
#[derive(Clone, Debug, PartialEq)]
pub struct ParamRow {
    /// Name.
    pub name: String,
    /// Where it lives.
    pub owner: ParamOwner,
    /// What uses it (sketch / feature name), for the table.
    pub consumed_by: String,
    /// Unit / type.
    pub unit: Unit,
    /// Equation, if any (`None` = plain value).
    pub expr: Option<String>,
    /// Nominal value (internal units).
    pub value: Real,
    /// Tolerance (dimensions only).
    pub tol: Option<Tolerance>,
    /// Comment.
    pub comment: String,
    /// A unit specification beyond `unit` (user parameters, physical
    /// properties).
    pub spec: Option<String>,
    /// The text of a Text parameter.
    pub text: Option<String>,
}

impl ParamRow {
    /// The text in the Equation column.
    pub fn equation(&self) -> String {
        self.expr
            .clone()
            .unwrap_or_else(|| format_in(self.value, self.def(), &self.unit_label()))
    }

    /// The row's unit.
    pub fn def(&self) -> UnitDef {
        self.spec
            .as_deref()
            .and_then(|s| units::parse_spec(s).ok())
            .unwrap_or_else(|| self.unit.def())
    }

    /// The unit as shown.
    pub fn unit_label(&self) -> String {
        self.spec
            .clone()
            .unwrap_or_else(|| self.unit.label().to_owned())
    }

    /// The value as shown in the row's unit (True / False, "text").
    pub fn shown(&self) -> String {
        match self.def().logic {
            units::Logic::Bool => if self.value > 0.5 { "True" } else { "False" }.to_owned(),
            units::Logic::Text => format!("\"{}\"", self.text.clone().unwrap_or_default()),
            units::Logic::Num => format_in(self.value, self.def(), &self.unit_label()),
        }
    }

    /// The row's value for other equations.
    pub fn as_value(&self) -> expr::Value {
        let def = self.def();
        match def.logic {
            units::Logic::Bool => expr::Value::Bool(self.value > 0.5),
            units::Logic::Text => expr::Value::Text(self.text.clone().unwrap_or_default()),
            units::Logic::Num => expr::Value::Num(self.value, def.dim),
        }
    }
}

/// A value in a unit with its label (`12.5 mm`, `3.2 N/m`, `5` unitless).
pub fn format_in(v: Real, def: UnitDef, label: &str) -> String {
    let d = def.to_display(v);
    let s = format!("{d:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.').to_owned();
    if label.is_empty() || label == "ul" {
        s
    } else {
        format!("{s} {label}")
    }
}

/// The part's physical properties as read-only parameters: name, unit,
/// how it is read from the result.
pub const PHYSICAL: [(&str, &str); 6] = [
    ("PartMass", "kg"),
    ("PartVolume", "mm^3"),
    ("PartArea", "mm^2"),
    ("PartCogX", "mm"),
    ("PartCogY", "mm"),
    ("PartCogZ", "mm"),
];

/// Whether `name` is one of [`PHYSICAL`].
pub fn is_physical(name: &str) -> bool {
    PHYSICAL.iter().any(|(n, _)| *n == name)
}

/// A value as typed in its unit (`12.5 mm`, `30 deg`).
pub fn format_value(v: Real, unit: Unit) -> String {
    let d = unit.to_display(v);
    let s = format!("{d:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.').to_owned();
    match unit {
        Unit::Ul => s,
        u => format!("{s} {}", u.label()),
    }
}

/// Numeric fields of a feature that are parameters: (field, unit).
fn feature_fields(f: &Feature) -> Vec<(&'static str, Unit)> {
    match f {
        Feature::Box(_) => vec![
            ("length", Unit::Mm),
            ("width", Unit::Mm),
            ("height", Unit::Mm),
        ],
        Feature::Extrude(e) => {
            let mut v = vec![("distance", Unit::Mm)];
            if e.direction == super::ExtentDirection::Asymmetric {
                v.push(("distance_b", Unit::Mm));
            }
            v.push(("taper", Unit::Deg));
            if e.direction == super::ExtentDirection::Asymmetric {
                v.push(("taper_b", Unit::Deg));
            }
            v
        }
        Feature::Revolve(r) => {
            let mut v = vec![("angle", Unit::Deg)];
            if r.direction == super::ExtentDirection::Asymmetric {
                v.push(("angle_b", Unit::Deg));
            }
            v
        }
        Feature::Blend(b) => match &b.kind {
            super::BlendKind::Fillet { .. } => match b.fillet_type {
                super::FilletType::FullRound | super::FilletType::Variable => Vec::new(),
                _ => std::iter::once("radius")
                    .chain(SET_RADII.iter().copied().take(b.sets.len()))
                    .map(|n| (n, Unit::Mm))
                    .collect(),
            },
            super::BlendKind::Chamfer { method, .. } => {
                let mut v = chamfer_fields(*method);
                if b.partial.is_some() {
                    v.extend([("to_start", Unit::Mm), ("partial_length", Unit::Mm)]);
                }
                v
            }
        },
        Feature::Shell(_) => vec![("thickness", Unit::Mm)],
        Feature::Draft(d) => {
            if d.uses_angle2() {
                vec![("angle", Unit::Deg), ("angle2", Unit::Deg)]
            } else {
                vec![("angle", Unit::Deg)]
            }
        }
        Feature::Pattern(p) => match &p.kind {
            super::PatternKind::Rectangular { second, .. } => {
                let mut v = vec![("count1", Unit::Ul), ("spacing1", Unit::Mm)];
                if second.is_some() {
                    v.extend([("count2", Unit::Ul), ("spacing2", Unit::Mm)]);
                }
                v
            }
            super::PatternKind::Circular { .. } => vec![("count", Unit::Ul), ("angle", Unit::Deg)],
            super::PatternKind::Mirror { .. } => Vec::new(),
        },
        Feature::Hole(h) => h.fields(),
        Feature::Sweep(s) => s.fields(),
        Feature::Loft(_) => Vec::new(),
        Feature::Coil(c) => c.fields(),
        Feature::Rib(r) => r.fields(),
        Feature::SheetFlange(f) => f.fields(),
        Feature::SheetConvert(_) => Vec::new(),
        Feature::SheetOp(f) => f.fields(),
        Feature::BodyOp(f) => f.fields(),
        Feature::SheetFace(_) => Vec::new(),
        Feature::Thread(t) => t.fields(),
        Feature::Combine(_) | Feature::Split(_) => Vec::new(),
        Feature::Work(w) => match &w.def {
            super::WorkDef::Plane(super::WorkPlaneDef::Offset { .. }) => {
                vec![("distance", Unit::Mm)]
            }
            super::WorkDef::Plane(super::WorkPlaneDef::Angle { .. }) => vec![("angle", Unit::Deg)],
            super::WorkDef::Point(super::WorkPointDef::Fixed { .. }) => {
                vec![("x", Unit::Mm), ("y", Unit::Mm), ("z", Unit::Mm)]
            }
            super::WorkDef::Ucs(super::work::UcsDef::Offsets { .. }) => vec![
                ("x", Unit::Mm),
                ("y", Unit::Mm),
                ("z", Unit::Mm),
                ("rx", Unit::Deg),
                ("ry", Unit::Deg),
                ("rz", Unit::Deg),
            ],
            _ => Vec::new(),
        },
    }
}

/// A chamfer's size fields for its method.
fn chamfer_fields(method: super::ChamferMethod) -> Vec<(&'static str, Unit)> {
    match method {
        super::ChamferMethod::Distance => vec![("distance", Unit::Mm)],
        super::ChamferMethod::TwoDistances => vec![("distance", Unit::Mm), ("distance2", Unit::Mm)],
        super::ChamferMethod::DistanceAngle => vec![("distance", Unit::Mm), ("angle", Unit::Deg)],
    }
}

/// Parameter fields of a fillet's further edge sets (set k → `radius<k+2>`).
pub const SET_RADII: [&str; 8] = [
    "radius2", "radius3", "radius4", "radius5", "radius6", "radius7", "radius8", "radius9",
];

/// The edge set a `radius<k>` field belongs to.
fn set_index(field: &str) -> Option<usize> {
    SET_RADII.iter().position(|&n| n == field)
}

/// Names of a feature's numeric fields that are parameters.
pub fn feature_field_names(f: &Feature) -> Vec<&'static str> {
    feature_fields(f).into_iter().map(|(n, _)| n).collect()
}

/// The current value of a feature's numeric field (internal units).
pub fn feature_value(f: &Feature, field: &str) -> Option<Real> {
    match f {
        Feature::Box(b) => match field {
            "length" => Some(b.size[0]),
            "width" => Some(b.size[1]),
            "height" => Some(b.size[2]),
            _ => None,
        },
        Feature::Extrude(e) => Some(match field {
            "distance" => e.distance,
            "distance_b" => e.distance_b,
            "taper" => e.taper,
            "taper_b" => e.taper_b,
            _ => return None,
        }),
        Feature::Revolve(r) => Some(match field {
            "angle" => r.angle,
            "angle_b" => r.angle_b,
            _ => return None,
        }),
        Feature::Blend(b) => match (&b.kind, field) {
            (super::BlendKind::Fillet { radius }, "radius") => Some(*radius),
            (super::BlendKind::Fillet { .. }, f) if set_index(f).is_some() => {
                set_index(f).and_then(|k| b.sets.get(k)).map(|s| s.radius)
            }
            (super::BlendKind::Chamfer { distance, .. }, "distance") => Some(*distance),
            (super::BlendKind::Chamfer { distance2, .. }, "distance2") => Some(*distance2),
            (super::BlendKind::Chamfer { angle, .. }, "angle") => Some(*angle),
            (_, "to_start") => b.partial.as_ref().map(|p| p.to_start),
            (_, "partial_length") => b.partial.as_ref().map(|p| p.length),
            _ => None,
        },
        Feature::Shell(s) => (field == "thickness").then_some(s.thickness),
        Feature::Draft(d) => match field {
            "angle" => Some(d.angle),
            "angle2" => Some(d.angle2),
            _ => None,
        },
        Feature::Pattern(p) => match (&p.kind, field) {
            (super::PatternKind::Rectangular { first, .. }, "count1") => Some(first.count as Real),
            (super::PatternKind::Rectangular { first, .. }, "spacing1") => Some(first.spacing),
            (super::PatternKind::Rectangular { second, .. }, "count2") => {
                second.as_ref().map(|d| d.count as Real)
            }
            (super::PatternKind::Rectangular { second, .. }, "spacing2") => {
                second.as_ref().map(|d| d.spacing)
            }
            (super::PatternKind::Circular { count, .. }, "count") => Some(*count as Real),
            (super::PatternKind::Circular { angle, .. }, "angle") => Some(*angle),
            _ => None,
        },
        Feature::Hole(h) => h.value(field),
        Feature::Sweep(s) => s.value(field),
        Feature::Loft(_) => None,
        Feature::Coil(c) => c.value(field),
        Feature::Rib(r) => r.value(field),
        Feature::SheetFlange(f) => f.value(field),
        Feature::SheetConvert(_) => None,
        Feature::SheetOp(f) => f.value(field),
        Feature::BodyOp(f) => f.value(field),
        Feature::SheetFace(_) => None,
        Feature::Thread(t) => t.value(field),
        Feature::Combine(_) | Feature::Split(_) => None,
        Feature::Work(w) => match (&w.def, field) {
            (super::WorkDef::Plane(super::WorkPlaneDef::Offset { distance, .. }), "distance") => {
                Some(*distance)
            }
            (super::WorkDef::Plane(super::WorkPlaneDef::Angle { angle, .. }), "angle") => {
                Some(*angle)
            }
            (super::WorkDef::Point(super::WorkPointDef::Fixed { x, .. }), "x") => Some(*x),
            (super::WorkDef::Point(super::WorkPointDef::Fixed { y, .. }), "y") => Some(*y),
            (super::WorkDef::Point(super::WorkPointDef::Fixed { z, .. }), "z") => Some(*z),
            (
                super::WorkDef::Ucs(super::work::UcsDef::Offsets {
                    x,
                    y,
                    z,
                    rx,
                    ry,
                    rz,
                }),
                f,
            ) => match f {
                "x" => Some(*x),
                "y" => Some(*y),
                "z" => Some(*z),
                "rx" => Some(*rx),
                "ry" => Some(*ry),
                "rz" => Some(*rz),
                _ => None,
            },
            _ => None,
        },
    }
}

fn set_feature_value(f: &mut Feature, field: &str, v: Real) {
    match f {
        Feature::Box(b) => match field {
            "length" => b.size[0] = v,
            "width" => b.size[1] = v,
            "height" => b.size[2] = v,
            _ => {}
        },
        Feature::Extrude(e) => match field {
            "distance" => e.distance = v,
            "distance_b" => e.distance_b = v,
            "taper" => e.taper = v,
            "taper_b" => e.taper_b = v,
            _ => {}
        },
        Feature::Revolve(r) => match field {
            "angle" => r.angle = v,
            "angle_b" => r.angle_b = v,
            _ => {}
        },
        Feature::Blend(b) => match (&mut b.kind, field) {
            (super::BlendKind::Fillet { radius }, "radius") => *radius = v,
            (super::BlendKind::Fillet { .. }, f) if set_index(f).is_some() => {
                if let Some(s) = set_index(f).and_then(|k| b.sets.get_mut(k)) {
                    s.radius = v;
                }
            }
            (super::BlendKind::Chamfer { distance, .. }, "distance") => *distance = v,
            (super::BlendKind::Chamfer { distance2, .. }, "distance2") => *distance2 = v,
            (super::BlendKind::Chamfer { angle, .. }, "angle") => *angle = v,
            (_, "to_start") => {
                if let Some(p) = b.partial.as_mut() {
                    p.to_start = v;
                }
            }
            (_, "partial_length") => {
                if let Some(p) = b.partial.as_mut() {
                    p.length = v;
                }
            }
            _ => {}
        },
        Feature::Shell(s) => {
            if field == "thickness" {
                s.thickness = v;
            }
        }
        Feature::Draft(d) => match field {
            "angle" => d.angle = v,
            "angle2" => d.angle2 = v,
            _ => {}
        },
        Feature::Pattern(p) => {
            // Counts are whole numbers (a typed 2.6 is 3); out-of-range
            // values are kept for the feature to report.
            let count = || v.round().clamp(0.0, 1e6) as u32;
            match (&mut p.kind, field) {
                (super::PatternKind::Rectangular { first, .. }, "count1") => first.count = count(),
                (super::PatternKind::Rectangular { first, .. }, "spacing1") => first.spacing = v,
                (
                    super::PatternKind::Rectangular {
                        second: Some(d), ..
                    },
                    "count2",
                ) => d.count = count(),
                (
                    super::PatternKind::Rectangular {
                        second: Some(d), ..
                    },
                    "spacing2",
                ) => d.spacing = v,
                (super::PatternKind::Circular { count: c, .. }, "count") => *c = count(),
                (super::PatternKind::Circular { angle, .. }, "angle") => *angle = v,
                _ => {}
            }
        }
        Feature::Hole(h) => h.set(field, v),
        Feature::Sweep(s) => s.set(field, v),
        Feature::Loft(_) => {}
        Feature::Coil(c) => c.set(field, v),
        Feature::Rib(r) => r.set(field, v),
        Feature::SheetFlange(f) => f.set(field, v),
        Feature::SheetConvert(_) => {}
        Feature::SheetOp(f) => f.set(field, v),
        Feature::BodyOp(f) => f.set(field, v),
        Feature::SheetFace(_) => {}
        Feature::Thread(t) => t.set(field, v),
        Feature::Combine(_) | Feature::Split(_) => {}
        Feature::Work(w) => match (&mut w.def, field) {
            (super::WorkDef::Plane(super::WorkPlaneDef::Offset { distance, .. }), "distance") => {
                *distance = v
            }
            (super::WorkDef::Plane(super::WorkPlaneDef::Angle { angle, .. }), "angle") => {
                *angle = v
            }
            (super::WorkDef::Point(super::WorkPointDef::Fixed { x, .. }), "x") => *x = v,
            (super::WorkDef::Point(super::WorkPointDef::Fixed { y, .. }), "y") => *y = v,
            (super::WorkDef::Point(super::WorkPointDef::Fixed { z, .. }), "z") => *z = v,
            (
                super::WorkDef::Ucs(super::work::UcsDef::Offsets {
                    x,
                    y,
                    z,
                    rx,
                    ry,
                    rz,
                }),
                f,
            ) => match f {
                "x" => *x = v,
                "y" => *y = v,
                "z" => *z = v,
                "rx" => *rx = v,
                "ry" => *ry = v,
                "rz" => *rz = v,
                _ => {}
            },
            _ => {}
        },
    }
}

fn feature_params_mut(f: &mut Feature) -> Option<&mut FeatureParams> {
    match f {
        Feature::Box(b) => Some(&mut b.params),
        Feature::Extrude(e) => Some(&mut e.params),
        Feature::Revolve(r) => Some(&mut r.params),
        Feature::Work(w) => Some(&mut w.params),
        Feature::Blend(b) => Some(&mut b.params),
        Feature::Shell(s) => Some(&mut s.params),
        Feature::Draft(d) => Some(&mut d.params),
        Feature::Pattern(p) => Some(&mut p.params),
        Feature::Hole(h) => Some(&mut h.params),
        Feature::Sweep(s) => Some(&mut s.params),
        Feature::Loft(l) => Some(&mut l.params),
        Feature::Coil(c) => Some(&mut c.params),
        Feature::Rib(r) => Some(&mut r.params),
        Feature::SheetFlange(f) => Some(&mut f.params),
        Feature::SheetConvert(_) => None,
        Feature::SheetOp(f) => Some(&mut f.params),
        Feature::BodyOp(f) => Some(&mut f.params),
        Feature::SheetFace(_) => None,
        Feature::Thread(t) => Some(&mut t.params),
        Feature::Combine(_) | Feature::Split(_) => None,
    }
}

fn feature_params(f: &Feature) -> Option<&FeatureParams> {
    match f {
        Feature::Box(b) => Some(&b.params),
        Feature::Extrude(e) => Some(&e.params),
        Feature::Revolve(r) => Some(&r.params),
        Feature::Work(w) => Some(&w.params),
        Feature::Blend(b) => Some(&b.params),
        Feature::Shell(s) => Some(&s.params),
        Feature::Draft(d) => Some(&d.params),
        Feature::Pattern(p) => Some(&p.params),
        Feature::Hole(h) => Some(&h.params),
        Feature::Sweep(s) => Some(&s.params),
        Feature::Loft(l) => Some(&l.params),
        Feature::Coil(c) => Some(&c.params),
        Feature::Rib(r) => Some(&r.params),
        Feature::SheetFlange(f) => Some(&f.params),
        Feature::SheetConvert(_) => None,
        Feature::SheetOp(f) => Some(&f.params),
        Feature::BodyOp(f) => Some(&f.params),
        Feature::SheetFace(_) => None,
        Feature::Thread(t) => Some(&t.params),
        Feature::Combine(_) | Feature::Split(_) => None,
    }
}

fn feature_name(f: &Feature) -> &str {
    f.name()
}

/// Replaces whole-word occurrences of `old` by `new` in an equation.
fn rename_in(text: &str, old: &str, new: &str) -> String {
    let mut out = String::new();
    let cs: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < cs.len() {
        if cs[i].is_alphabetic() || cs[i] == '_' {
            let start = i;
            while i < cs.len() && (cs[i].is_alphanumeric() || cs[i] == '_') {
                i += 1;
            }
            let word: String = cs[start..i].iter().collect();
            out.push_str(if word == old { new } else { &word });
        } else {
            out.push(cs[i]);
            i += 1;
        }
    }
    out
}

impl PartDocument {
    /// Gives every driving dimension and feature value a parameter name
    /// (`d<N>`), keeping existing names; renames duplicates.
    pub fn sync_parameters(&mut self) {
        let mut taken: std::collections::HashSet<String> =
            self.user_params.iter().map(|p| p.name.clone()).collect();
        // Never reuse a number: start after the highest d<N> in use.
        let mut highest = self.next_param;
        let mut note = |n: &str| {
            if let Some(k) = n.strip_prefix('d').and_then(|r| r.parse::<usize>().ok()) {
                highest = highest.max(k + 1);
            }
        };
        for ps in &self.sketches {
            for (id, c) in ps.sketch.constraints() {
                if c.is_dimension() {
                    note(&ps.sketch.dim_param(id).name);
                }
            }
        }
        for f in &self.features {
            if let Some(fp) = feature_params(f) {
                for p in fp.0.values() {
                    note(&p.name);
                }
            }
        }
        let mut next = highest;
        let mut fresh = |taken: &mut std::collections::HashSet<String>| loop {
            let n = format!("d{next}");
            next += 1;
            if taken.insert(n.clone()) {
                return n;
            }
        };
        for ps in &mut self.sketches {
            let ids: Vec<ConstraintId> = ps
                .sketch
                .constraints()
                .filter(|(_, c)| c.is_dimension())
                .map(|(id, _)| id)
                .collect();
            for id in ids {
                let name = ps.sketch.dim_param(id).name;
                if name.is_empty() || !taken.insert(name) {
                    let n = fresh(&mut taken);
                    if let Some(p) = ps.sketch.dim_param_mut(id) {
                        p.name = n;
                    }
                }
            }
        }
        for f in &mut self.features {
            let fields = feature_fields(f);
            if let Some(fp) = feature_params_mut(f) {
                for (field, _) in fields {
                    let entry = fp.0.entry(field.to_owned()).or_default();
                    if entry.name.is_empty() || !taken.insert(entry.name.clone()) {
                        entry.name = fresh(&mut taken);
                    }
                }
            }
        }
        self.next_param = next;
    }

    /// All parameters, model first (sketch dimensions, feature values), then
    /// user parameters.
    pub fn parameter_table(&self) -> Vec<ParamRow> {
        let mut rows = Vec::new();
        for (i, ps) in self.sketches.iter().enumerate() {
            // `Sketch1/Extrusion1`: the sketch and the features it feeds.
            let users: Vec<&str> = self
                .consumers(&ps.id)
                .into_iter()
                .filter_map(|k| self.features.get(k).map(feature_name))
                .collect();
            let consumed_by = if users.is_empty() {
                ps.name.clone()
            } else {
                format!("{}/{}", ps.name, users.join(", "))
            };
            for (id, c) in ps.sketch.constraints() {
                if !c.is_dimension() || ps.sketch.is_driven(id) {
                    continue;
                }
                let p = ps.sketch.dim_param(id);
                let unit = if matches!(c, crate::sketch::Constraint::Angle { .. }) {
                    Unit::Deg
                } else {
                    Unit::Mm
                };
                let value = ps.sketch.dimension_nominal(id).unwrap_or_default();
                rows.push(ParamRow {
                    name: p.name.clone(),
                    owner: ParamOwner::Dimension {
                        sketch: i,
                        constraint: id,
                    },
                    consumed_by: consumed_by.clone(),
                    unit,
                    expr: p.expr.clone(),
                    // Angles are shown unsigned.
                    value: if unit.is_angle() { value.abs() } else { value },
                    tol: Some(p.tol),
                    comment: p.comment.clone(),
                    spec: None,
                    text: None,
                });
            }
        }
        for (k, f) in self.features.iter().enumerate() {
            let Some(fp) = feature_params(f) else {
                continue;
            };
            for (field, unit) in feature_fields(f) {
                let Some(p) = fp.0.get(field) else { continue };
                rows.push(ParamRow {
                    name: p.name.clone(),
                    owner: ParamOwner::Feature { feature: k, field },
                    consumed_by: feature_name(f).to_owned(),
                    unit,
                    expr: p.expr.clone(),
                    value: feature_value(f, field).unwrap_or_default(),
                    tol: None,
                    comment: String::new(),
                    spec: None,
                    text: None,
                });
            }
        }
        for (i, u) in self.user_params.iter().enumerate() {
            rows.push(ParamRow {
                name: u.name.clone(),
                owner: ParamOwner::User(i),
                consumed_by: String::new(),
                unit: u.unit,
                expr: (!expr::is_plain_number(&u.expr)).then(|| u.expr.clone()),
                value: u.value,
                tol: None,
                comment: u.comment.clone(),
                spec: u.unit_spec.clone(),
                text: (u.unit == Unit::Text).then(|| u.text.clone()),
            });
        }
        // The physical properties of the last regeneration (read only).
        if let Some(vals) = &self.physical_values {
            for (k, (name, unit)) in PHYSICAL.iter().enumerate() {
                rows.push(ParamRow {
                    name: (*name).to_owned(),
                    owner: ParamOwner::Physical(k),
                    consumed_by: "Physical properties".to_owned(),
                    unit: if *unit == "mm" { Unit::Mm } else { Unit::Ul },
                    expr: None,
                    value: vals[k],
                    tol: None,
                    comment: String::new(),
                    spec: Some((*unit).to_owned()),
                    text: None,
                });
            }
        }
        rows
    }

    /// The physical-property values for the equations (internal units with
    /// their dimensions).
    pub fn physical_fixed(&self) -> std::collections::HashMap<String, expr::Value> {
        let mut out = std::collections::HashMap::new();
        if let Some(vals) = &self.physical_values {
            for (k, (name, unit)) in PHYSICAL.iter().enumerate() {
                if let Ok(u) = units::parse_spec(unit) {
                    out.insert((*name).to_owned(), expr::Value::Num(vals[k], u.dim));
                }
            }
        }
        out
    }

    /// Whether some equation uses a physical property.
    pub fn uses_physical(&self) -> bool {
        self.user_params
            .iter()
            .map(|u| u.expr.as_str())
            .chain(
                self.parameter_table()
                    .iter()
                    .filter_map(|r| r.expr.as_deref())
                    .collect::<Vec<_>>(),
            )
            .any(|e| expr::names_in(e).iter().any(|n| is_physical(n)))
    }

    /// Computes the physical properties of the body as it is now (mass with
    /// the materials' densities, display accuracy).
    pub fn update_physical(&mut self) {
        self.physical_values =
            super::physical::physical_properties(self, super::physical::Accuracy::default())
                .ok()
                .map(|p| {
                    let t = &p.total;
                    [
                        t.mass / 1000.0,
                        t.volume,
                        t.area,
                        t.center.x,
                        t.center.y,
                        t.center.z,
                    ]
                });
    }

    /// Evaluates every equation in dependency order and writes the results
    /// into the dimensions, feature values and user parameters. Errors go
    /// to [`PartDocument::param_errors`]; such parameters keep their value.
    pub fn evaluate_parameters(&mut self) {
        self.apply_pattern_counts();
        let rows = self.parameter_table();
        let list = self.equations(&rows);
        let fixed = self.physical_fixed();
        let (values, mut errors) = expr::eval_all_with(&list, &fixed);
        // A size cannot follow the part's mass / volume / area: it would
        // change what it follows (and the part would build differently
        // each time). Only user parameters may use them.
        let geometry: Vec<&ParamRow> = rows
            .iter()
            .filter(|r| {
                matches!(
                    r.owner,
                    ParamOwner::Dimension { .. } | ParamOwner::Feature { .. }
                )
            })
            .collect();
        for r in geometry {
            if let Some(p) = physical_dependency(&r.name, &list) {
                errors.insert(
                    r.name.clone(),
                    format!("a size cannot depend on {p} (it would change it)"),
                );
            }
        }
        for r in &rows {
            if errors.contains_key(&r.name) {
                continue;
            }
            let Some(val) = values.get(&r.name) else {
                continue;
            };
            let v = val.number();
            if let (ParamOwner::User(i), expr::Value::Text(t)) = (&r.owner, val) {
                self.user_params[*i].text = t.clone();
            }
            match &r.owner {
                ParamOwner::Dimension { sketch, constraint } if r.expr.is_some() => {
                    let _ = self.sketches[*sketch]
                        .sketch
                        .set_dimension_nominal(*constraint, v);
                }
                ParamOwner::Feature { feature, field } if r.expr.is_some() => {
                    set_feature_value(&mut self.features[*feature], field, v);
                }
                ParamOwner::User(i) => self.user_params[*i].value = v,
                _ => {}
            }
        }
        // Suppress if: the features whose condition is true, and what
        // depends on them.
        let mut cond = std::collections::BTreeSet::new();
        for (id, text) in &self.suppress_if {
            let label = format!("Suppress if ({id})");
            if let Some(p) = expr::names_in(text).iter().find_map(|n| {
                if is_physical(n) {
                    Some(n.clone())
                } else {
                    physical_dependency(n, &list)
                }
            }) {
                errors.insert(
                    label,
                    format!("a condition cannot depend on {p} (it would change it)"),
                );
                continue;
            }
            match expr::eval_value(text, Unit::Bool, &|n| values.get(n).cloned()) {
                Ok((expr::Value::Bool(true), _)) => {
                    if let Some(k) = self.feature_index(id) {
                        cond.insert(id.clone());
                        for j in self.dependents(k) {
                            cond.insert(self.features[j].id().to_owned());
                        }
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    errors.insert(label, e);
                }
            }
        }
        self.cond_suppressed = cond;
        self.param_errors = errors.into_iter().collect();
    }

    /// Sets feature `k`'s Suppress if condition (`None` removes it). Refused,
    /// with nothing changed, when it is not a true / false equation of the
    /// parameters.
    pub fn set_suppress_if(&mut self, k: usize, text: Option<&str>) -> Result<(), PartError> {
        let id = self
            .features
            .get(k)
            .map(|f| f.id().to_owned())
            .ok_or_else(|| PartError::Feature("the feature no longer exists".into()))?;
        let before = self.clone();
        match text.map(str::trim).filter(|t| !t.is_empty()) {
            None => {
                self.suppress_if.remove(&id);
            }
            Some(t) => {
                self.check_condition(t)
                    .map_err(|e| PartError::Feature(format!("Suppress if: {e}")))?;
                self.suppress_if.insert(id.clone(), t.to_owned());
            }
        }
        let label = format!("Suppress if ({id})");
        if let Err(e) = self.regenerate() {
            *self = before;
            return Err(e);
        }
        if let Some(e) = self.param_errors.get(&label).cloned() {
            *self = before;
            return Err(PartError::Feature(format!("Suppress if: {e}")));
        }
        Ok(())
    }

    /// Evaluates a Suppress if condition with the current parameters:
    /// whether it is true now, or why it is not a condition (not true /
    /// false, an unknown name, a physical property).
    pub fn check_condition(&self, text: &str) -> Result<bool, String> {
        let table = self.parameter_table();
        let list = self.equations(&table);
        if let Some(p) = expr::names_in(text).iter().find_map(|n| {
            if is_physical(n) {
                Some(n.clone())
            } else {
                physical_dependency(n, &list)
            }
        }) {
            return Err(format!(
                "a condition cannot depend on {p} (it would change it)"
            ));
        }
        match expr::eval_value(text, Unit::Bool, &|n| {
            table.iter().find(|r| r.name == n).map(ParamRow::as_value)
        })? {
            (expr::Value::Bool(b), _) => Ok(b),
            _ => Err("the condition must be true or false".into()),
        }
    }

    /// Feature `k`'s Suppress if condition now: `None` without one, else
    /// whether it is true, or its error.
    pub fn suppress_if_state(&self, k: usize) -> Option<Result<bool, String>> {
        let text = self.suppress_if.get(self.features.get(k)?.id())?;
        Some(self.check_condition(text))
    }

    /// The equations of the table (physical properties are values, not
    /// equations).
    fn equations(&self, rows: &[ParamRow]) -> Vec<(String, String, UnitDef)> {
        let mut list: Vec<(String, String, UnitDef)> = rows
            .iter()
            .filter(|r| !matches!(r.owner, ParamOwner::Physical(_)))
            .map(|r| (r.name.clone(), r.equation(), r.def()))
            .collect();
        for u in &self.user_params {
            if let Some(e) = list.iter_mut().find(|e| e.0 == u.name) {
                e.1 = u.expr.clone();
            }
        }
        list
    }

    /// Sketch patterns whose count has an equation: rebuilt with the
    /// evaluated count (rounded, at least 1; 2 for circular). Runs before
    /// the dimensions are evaluated, since a rebuild renews the pattern's
    /// dimensions (their parameters move along, see `Sketch::edit_pattern`).
    fn apply_pattern_counts(&mut self) {
        use crate::sketch::{PatternId, PatternKind};
        if !self.sketches.iter().any(|ps| {
            ps.sketch
                .patterns()
                .any(|(_, p)| p.count_exprs.iter().any(Option::is_some))
        }) {
            return;
        }
        let rows = self.parameter_table();
        let list = self.equations(&rows);
        let (values, _) = expr::eval_all_with(&list, &self.physical_fixed());
        let lookup = |n: &str| values.get(n).cloned();
        let count = |e: &Option<String>| -> Option<usize> {
            let (v, _) = expr::eval(e.as_deref()?, Unit::Ul, &lookup).ok()?;
            v.is_finite()
                .then(|| v.round().clamp(1.0, 10_000.0) as usize)
        };
        for ps in &mut self.sketches {
            let jobs: Vec<(PatternId, PatternKind)> = ps
                .sketch
                .patterns()
                .filter_map(|(id, p)| {
                    let mut kind = p.kind;
                    match &mut kind {
                        PatternKind::Rectangular { dir1, dir2 } => {
                            if let Some(c) = count(&p.count_exprs[0]) {
                                dir1.count = c;
                            }
                            if let (Some(d2), Some(c)) = (dir2.as_mut(), count(&p.count_exprs[1])) {
                                d2.count = c;
                            }
                        }
                        PatternKind::Circular { count: n, .. } => {
                            if let Some(c) = count(&p.count_exprs[0]) {
                                *n = c.max(2);
                            }
                        }
                        _ => {}
                    }
                    (kind != p.kind).then_some((id, kind))
                })
                .collect();
            for (id, kind) in jobs {
                // Failure (e.g. a count the geometry cannot take) keeps the
                // pattern as it is.
                let _ = ps.sketch.edit_pattern(id, kind);
            }
        }
    }

    /// The row of parameter `name`.
    pub fn parameter(&self, name: &str) -> Option<ParamRow> {
        self.parameter_table().into_iter().find(|r| r.name == name)
    }

    /// Sets the equation of parameter `name` (a plain number sets the
    /// value). Refused, with nothing changed, if it does not evaluate
    /// (syntax, unknown name, circular reference).
    pub fn set_parameter_expr(&mut self, name: &str, text: &str) -> Result<(), PartError> {
        let row = self
            .parameter(name)
            .ok_or_else(|| PartError::Feature(format!("no parameter '{name}'")))?;
        let text = text.trim();
        let before = self.clone();
        let plain = expr::is_plain_number(text);
        // Check it alone first (clear message for a typo).
        if matches!(row.owner, ParamOwner::Physical(_)) {
            return Err(PartError::Feature(format!(
                "{name} is a physical property of the part (read only)"
            )));
        }
        let table = self.parameter_table();
        let (value, _) = expr::eval(text, row.def(), &|n| {
            table.iter().find(|r| r.name == n).map(ParamRow::as_value)
        })
        .map_err(|e| PartError::Feature(format!("{name}: {e}")))?;
        let stored = (!plain).then(|| text.to_owned());
        match row.owner {
            ParamOwner::Dimension { sketch, constraint } => {
                let sk = &mut self.sketches[sketch].sketch;
                if let Some(p) = sk.dim_param_mut(constraint) {
                    p.expr = stored;
                }
                sk.set_dimension_nominal(constraint, value)
                    .map_err(|e| PartError::Feature(e.to_string()))?;
            }
            ParamOwner::Feature { feature, field } => {
                let f = &mut self.features[feature];
                set_feature_value(f, field, value);
                if let Some(fp) = feature_params_mut(f) {
                    fp.0.entry(field.to_owned()).or_default().expr = stored;
                }
            }
            ParamOwner::User(i) => {
                self.user_params[i].expr = text.to_owned();
                self.user_params[i].value = value;
            }
            ParamOwner::Physical(_) => {}
        }
        self.evaluate_parameters();
        if let Some(e) = self.param_errors.get(name).cloned() {
            *self = before;
            return Err(PartError::Feature(format!("{name}: {e}")));
        }
        Ok(())
    }

    /// Renames a parameter; every equation using it follows.
    pub fn rename_parameter(&mut self, old: &str, new: &str) -> Result<(), PartError> {
        let new = new.trim();
        if !expr::valid_name(new) {
            return Err(PartError::Feature(format!(
                "'{new}' is not a valid parameter name"
            )));
        }
        if old == new {
            return Ok(());
        }
        let rows = self.parameter_table();
        if rows.iter().any(|r| r.name == new) {
            return Err(PartError::Feature(format!("'{new}' is already used")));
        }
        let row = rows
            .iter()
            .find(|r| r.name == old)
            .cloned()
            .ok_or_else(|| PartError::Feature(format!("no parameter '{old}'")))?;
        match row.owner {
            ParamOwner::Dimension { sketch, constraint } => {
                if let Some(p) = self.sketches[sketch].sketch.dim_param_mut(constraint) {
                    p.name = new.to_owned();
                }
            }
            ParamOwner::Feature { feature, field } => {
                if let Some(fp) = feature_params_mut(&mut self.features[feature]) {
                    fp.0.entry(field.to_owned()).or_default().name = new.to_owned();
                }
            }
            ParamOwner::User(i) => self.user_params[i].name = new.to_owned(),
            ParamOwner::Physical(_) => {
                return Err(PartError::Feature(format!(
                    "{old} is a physical property; it keeps its name"
                )));
            }
        }
        if let Some(meta) = self.param_meta.remove(old) {
            self.param_meta.insert(new.to_owned(), meta);
        }
        for e in self.suppress_if.values_mut() {
            *e = rename_in(e, old, new);
        }
        // Equations that used the old name.
        for ps in &mut self.sketches {
            let ids: Vec<ConstraintId> = ps.sketch.constraints().map(|(id, _)| id).collect();
            for id in ids {
                if let Some(e) = ps.sketch.dim_param(id).expr {
                    let renamed = rename_in(&e, old, new);
                    if let Some(p) = ps.sketch.dim_param_mut(id) {
                        p.expr = Some(renamed);
                    }
                }
            }
        }
        for f in &mut self.features {
            if let Some(fp) = feature_params_mut(f) {
                for p in fp.0.values_mut() {
                    if let Some(e) = &p.expr {
                        p.expr = Some(rename_in(e, old, new));
                    }
                }
            }
        }
        for u in &mut self.user_params {
            u.expr = rename_in(&u.expr, old, new);
        }
        Ok(())
    }

    /// Adds a user parameter (Add Numeric) and returns its name.
    pub fn add_user_parameter(&mut self, unit: Unit) -> String {
        let rows = self.parameter_table();
        // `rows.len() + 1` candidates: at least one is free.
        let name = (0..=rows.len())
            .map(|k| format!("User{k}"))
            .find(|n| !rows.iter().any(|r| &r.name == n))
            .unwrap_or_default();
        let value = unit.from_display(if unit.is_angle() { 0.0 } else { 1.0 });
        self.user_params.push(UserParam {
            name: name.clone(),
            expr: format_value(value, unit),
            unit,
            comment: String::new(),
            value,
            unit_spec: None,
            values: Vec::new(),
            text: String::new(),
            link: None,
        });
        name
    }

    /// **Link** (Parameters dialog, spec 3.10 §3.9): a user parameter that
    /// takes its value (`value` in internal units of `unit`) from `link`;
    /// named `name`, or `name_2`… when taken. Returns its name.
    pub fn add_linked_parameter(
        &mut self,
        name: &str,
        value: Real,
        unit: Unit,
        link: ParamLink,
    ) -> Result<String, PartError> {
        let rows = self.parameter_table();
        let free = |n: &str| !rows.iter().any(|r| r.name == n);
        let mut new = name.to_owned();
        let mut k = 2;
        while !free(&new) {
            new = format!("{name}_{k}");
            k += 1;
        }
        if !expr::valid_name(&new) {
            return Err(PartError::Feature(format!(
                "'{name}' is not a parameter name"
            )));
        }
        let before = self.clone();
        let comment = link.describe();
        self.user_params.push(UserParam {
            name: new.clone(),
            expr: format_value(value, unit),
            unit,
            comment,
            value,
            unit_spec: None,
            values: Vec::new(),
            text: String::new(),
            link: Some(link),
        });
        if let Err(e) = self.set_parameter_expr(&new, &format_value(value, unit)) {
            *self = before;
            return Err(e);
        }
        Ok(new)
    }

    /// Update of the linked user parameters: each reads its source through
    /// `read` (value in internal units, unit); changed values are written and
    /// the part rebuilt. Returns whether something changed and the links
    /// that could not be read (`<name>: <why>`; their last value stays).
    pub fn update_param_links(&mut self, read: &LinkReader) -> (bool, Vec<String>) {
        let mut changed = false;
        let mut errors = Vec::new();
        let links: Vec<(String, ParamLink, Unit, String)> = self
            .user_params
            .iter()
            .filter_map(|u| {
                u.link
                    .clone()
                    .map(|l| (u.name.clone(), l, u.unit, u.expr.clone()))
            })
            .collect();
        for (name, link, unit, expr) in links {
            match read(&link) {
                Ok((v, u)) => {
                    // The source's unit kind decides how it is written here.
                    let shown = if u.is_length() == unit.is_length() {
                        unit
                    } else {
                        u
                    };
                    let text = format_value(v, shown);
                    if text != expr {
                        match self.set_parameter_expr(&name, &text) {
                            Ok(()) => changed = true,
                            Err(e) => errors.push(format!("{name}: {e}")),
                        }
                    }
                }
                Err(e) => errors.push(format!("{name}: {e}")),
            }
        }
        // The new values reach the features.
        if changed && let Err(e) = self.regenerate() {
            errors.push(e.to_string());
        }
        (changed, errors)
    }

    /// Break Link of linked parameter `name`: it keeps its value.
    pub fn break_param_link(&mut self, name: &str) -> bool {
        self.user_params
            .iter_mut()
            .find(|u| u.name == name)
            .is_some_and(|u| u.link.take().is_some())
    }

    /// Multi-Value: user parameter `i` chooses from `list` (each checked as
    /// its equation; the current value becomes the first if it is not in
    /// the list). An empty list makes it a single value again.
    pub fn set_user_values(&mut self, i: usize, list: Vec<String>) -> Result<(), PartError> {
        let Some(u) = self.user_params.get(i) else {
            return Err(PartError::Feature("no such user parameter".into()));
        };
        let (name, cur) = (u.name.clone(), u.expr.clone());
        let before = self.clone();
        for v in &list {
            if let Err(e) = self.set_parameter_expr(&name, v) {
                *self = before;
                return Err(e);
            }
        }
        // Back to the value it had (or the first when that is not offered).
        let pick = if list.contains(&cur) || list.is_empty() {
            cur
        } else {
            list[0].clone()
        };
        self.set_parameter_expr(&name, &pick)?;
        self.user_params[i].values = list;
        Ok(())
    }

    /// The Parameters table as CSV: Name, Unit, Equation, Value, Comment
    /// (a header row; fields quoted when they hold a comma or a quote).
    pub fn parameters_csv(&self) -> String {
        let q = |s: &str| {
            if s.contains([',', '"', '\n']) {
                format!("\"{}\"", s.replace('"', "\"\""))
            } else {
                s.to_owned()
            }
        };
        let mut out = String::from("Name,Unit,Equation,Value,Comment\n");
        for r in self.parameter_table() {
            let value = format_in(r.value, r.def(), "");
            out.push_str(&format!(
                "{},{},{},{},{}\n",
                q(&r.name),
                q(&r.unit_label()),
                q(&r.equation()),
                q(&value),
                q(&r.comment)
            ));
        }
        out
    }

    /// Reads [`parameters_csv`](Self::parameters_csv) back: a known name
    /// takes the equation and comment, an unknown one becomes a user
    /// parameter with its unit; physical rows are skipped. All or nothing:
    /// a row that fails leaves the part as it was. Returns the rows used.
    pub fn import_parameters_csv(&mut self, text: &str) -> Result<usize, PartError> {
        let before = self.clone();
        let r = self.import_rows(text);
        if r.is_err() {
            *self = before;
        }
        r
    }

    fn import_rows(&mut self, text: &str) -> Result<usize, PartError> {
        let mut n = 0;
        for (k, line) in text.lines().enumerate() {
            let f = csv_fields(line);
            if k == 0 && f.first().is_some_and(|x| x.eq_ignore_ascii_case("name")) {
                continue;
            }
            let [name, unit, eq, ..] = f.as_slice() else {
                if line.trim().is_empty() {
                    continue;
                }
                return Err(PartError::Feature(format!(
                    "line {}: too few fields",
                    k + 1
                )));
            };
            let comment = f.get(4).cloned().unwrap_or_default();
            if is_physical(name) {
                continue;
            }
            let row = self.parameter(name);
            if row.is_none() {
                if !expr::valid_name(name) {
                    return Err(PartError::Feature(format!(
                        "line {}: '{name}' is not a valid name",
                        k + 1
                    )));
                }
                let made = self.add_user_parameter(Unit::Ul);
                self.rename_parameter(&made, name)?;
                let i = self.user_params.len() - 1;
                self.set_user_unit(i, unit)
                    .map_err(|e| PartError::Feature(format!("line {}: {e}", k + 1)))?;
            }
            self.set_parameter_expr(name, eq)
                .map_err(|e| PartError::Feature(format!("line {}: {e}", k + 1)))?;
            if let Some(i) = self.user_params.iter().position(|u| &u.name == name) {
                self.user_params[i].comment = comment;
            }
            n += 1;
        }
        Ok(n)
    }

    /// Sets user parameter `i`'s unit from a specification (`N/m`, `kg`,
    /// `mm`). A plain value of the same kind keeps its size (25.4 mm →
    /// 1 in); of another kind it keeps its number; an equation stays as
    /// typed and is evaluated again. A bad specification changes nothing.
    pub fn set_user_unit(&mut self, i: usize, spec: &str) -> Result<(), PartError> {
        let spec = spec.trim();
        let legacy = Unit::ALL.into_iter().find(|u| u.label() == spec);
        let new = match legacy {
            Some(u) => u.def(),
            None => units::parse_spec(spec).map_err(PartError::Feature)?,
        };
        let Some(p) = self.user_params.get(i) else {
            return Err(PartError::Feature("no such user parameter".into()));
        };
        let old = p.def();
        let (name, expr, value) = (p.name.clone(), p.expr.clone(), p.value);
        let before = self.user_params[i].clone();
        // Conditions that hold now must still be conditions afterwards.
        let snapshot = self.clone();
        let working: Vec<String> = self
            .suppress_if
            .iter()
            .filter(|(_, c)| self.check_condition(c).is_ok())
            .map(|(id, _)| id.clone())
            .collect();
        // An old kind when the specification is one (files stay readable by
        // older builds), else the nearest old kind and the specification.
        {
            let p = &mut self.user_params[i];
            match legacy {
                Some(u) => {
                    p.unit = u;
                    p.unit_spec = None;
                }
                None => {
                    p.unit = if new.dim == Unit::Mm.def().dim {
                        Unit::Mm
                    } else if new.is_angle() {
                        Unit::Deg
                    } else {
                        Unit::Ul
                    };
                    p.unit_spec = Some(spec.to_owned());
                }
            }
        }
        let label = self.user_params[i].unit_label();
        let r = if new.logic != old.logic && expr::is_plain_value(&expr) {
            // To or from true / false or a text: a plain value starts over.
            let start = match new.logic {
                units::Logic::Bool => "false".to_owned(),
                units::Logic::Text => "\"\"".to_owned(),
                units::Logic::Num => format_in(new.from_display(0.0), new, &label),
            };
            self.set_parameter_expr(&name, &start)
        } else if expr::is_plain_number(&expr) {
            let shown = if units::compatible(old.dim, new.dim) {
                new.to_display(value)
            } else {
                old.to_display(value)
            };
            let text = format_in(new.from_display(shown), new, &label);
            self.set_parameter_expr(&name, &text)
        } else {
            self.set_parameter_expr(&name, &expr)
        };
        if r.is_err() {
            self.user_params[i] = before;
            return r;
        }
        for id in working {
            if let Some(Err(e)) = self
                .feature_index(&id)
                .and_then(|k| self.suppress_if_state(k))
            {
                *self = snapshot;
                return Err(PartError::Feature(format!(
                    "{name} is used by Suppress if ({id}): {e}"
                )));
            }
        }
        r
    }

    /// Parameters whose equations use `name`.
    pub fn parameter_users(&self, name: &str) -> Vec<String> {
        let mut out: Vec<String> = self
            .parameter_table()
            .into_iter()
            .filter(|r| {
                r.expr
                    .as_deref()
                    .is_some_and(|e| expr::names_in(e).iter().any(|n| n == name))
            })
            .map(|r| r.name)
            .collect();
        for (id, e) in &self.suppress_if {
            if expr::names_in(e).iter().any(|n| n == name) {
                out.push(format!("Suppress if ({id})"));
            }
        }
        out
    }

    /// Pick whip (After Effects): parameter `target` follows `source` — its
    /// equation becomes `source`. A circular link is refused and leaves
    /// everything as it was.
    pub fn link_parameter(&mut self, target: &str, source: &str) -> Result<(), PartError> {
        if target == source {
            return Err(PartError::Feature(format!(
                "'{target}' cannot follow itself"
            )));
        }
        self.set_parameter_expr(target, source)
    }

    /// User parameters no equation, dimension or feature field uses.
    pub fn unused_user_parameters(&self) -> Vec<String> {
        let table = self.parameter_table();
        self.user_params
            .iter()
            .filter(|u| {
                !table.iter().any(|r| {
                    r.name != u.name
                        && r.expr
                            .as_deref()
                            .is_some_and(|e| expr::names_in(e).contains(&u.name))
                }) && !self
                    .suppress_if
                    .values()
                    .any(|e| expr::names_in(e).contains(&u.name))
            })
            .map(|u| u.name.clone())
            .collect()
    }

    /// Purge Unused: deletes every unused user parameter; their names.
    pub fn purge_unused_parameters(&mut self) -> Vec<String> {
        let gone = self.unused_user_parameters();
        self.user_params.retain(|u| !gone.contains(&u.name));
        gone
    }

    /// Deletes user parameter `name`; refused while an equation uses it.
    pub fn delete_user_parameter(&mut self, name: &str) -> Result<(), PartError> {
        let users = self.parameter_users(name);
        if !users.is_empty() {
            return Err(PartError::Feature(format!(
                "'{name}' is used by {}",
                users.join(", ")
            )));
        }
        let i = self
            .user_params
            .iter()
            .position(|u| u.name == name)
            .ok_or_else(|| PartError::Feature(format!("no user parameter '{name}'")))?;
        self.user_params.remove(i);
        Ok(())
    }
}

/// The physical property `name`'s equation reaches through other
/// equations, if any.
fn physical_dependency(name: &str, list: &[(String, String, UnitDef)]) -> Option<String> {
    let mut seen: Vec<String> = Vec::new();
    let mut todo = vec![name.to_owned()];
    while let Some(n) = todo.pop() {
        let Some((_, text, _)) = list.iter().find(|e| e.0 == n) else {
            continue;
        };
        for dep in expr::names_in(text) {
            if is_physical(&dep) {
                return Some(dep);
            }
            if !seen.contains(&dep) {
                seen.push(dep.clone());
                todo.push(dep);
            }
        }
    }
    None
}

/// The fields of one CSV line (quotes `"…"` with `""` inside).
fn csv_fields(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut cs = line.chars().peekable();
    while let Some(c) = cs.next() {
        match (c, quoted) {
            ('"', true) if cs.peek() == Some(&'"') => {
                cur.push('"');
                cs.next();
            }
            ('"', _) => quoted = !quoted,
            (',', false) => out.push(std::mem::take(&mut cur).trim().to_owned()),
            _ => cur.push(c),
        }
    }
    out.push(cur.trim().to_owned());
    out
}

/// Splits field input `name = expr` into the new name and the expression
/// (`None` when there is no valid assignment).
pub fn split_assignment(text: &str) -> (Option<String>, String) {
    let cs: Vec<char> = text.chars().collect();
    let at = (0..cs.len()).find(|&i| {
        cs[i] == '='
            && cs.get(i + 1) != Some(&'=')
            && !(i > 0 && matches!(cs[i - 1], '<' | '>' | '!' | '='))
    });
    let split = at.map(|i| {
        let lhs: String = cs[..i].iter().collect();
        let rhs: String = cs[i + 1..].iter().collect();
        (lhs, rhs)
    });
    if let Some((lhs, rhs)) = split.as_ref().map(|(a, b)| (a.as_str(), b.as_str())) {
        let lhs = lhs.trim();
        if expr::valid_name(lhs) {
            return (Some(lhs.to_owned()), rhs.trim().to_owned());
        }
    }
    (None, text.trim().to_owned())
}

/// [`PartDocument::eval_field`] against a copy of the parameter table
/// (for the GUI, which cannot borrow the part while a dialog is open).
pub fn eval_field_in(
    table: &[ParamRow],
    text: &str,
    unit: impl Into<UnitDef>,
    own: Option<&str>,
) -> Result<(Option<String>, Real, Option<String>), String> {
    let unit: UnitDef = unit.into();
    let (name, body) = split_assignment(text);
    if let Some(n) = &name
        && Some(n.as_str()) != own
        && table.iter().any(|r| r.name == *n)
    {
        return Err(format!("the name '{n}' is already used"));
    }
    let (value, used) = expr::eval(&body, unit, &|n| {
        (Some(n) != own)
            .then(|| table.iter().find(|r| r.name == n).map(ParamRow::as_value))
            .flatten()
    })?;
    // A feature field (a size) cannot follow a physical property.
    if let Some(p) = used.iter().find(|n| is_physical(n)) {
        return Err(format!("a size cannot depend on {p} (it would change it)"));
    }
    if let Some(o) = own
        && used.iter().any(|u| u == o)
    {
        return Err(format!("'{o}' cannot refer to itself"));
    }
    let keep = (!expr::is_plain_number(&body)).then_some(body);
    Ok((name, value, keep))
}

/// Sets a feature field to a plain value (a manipulator drag): the
/// equation is dropped, the parameter name stays.
pub fn set_feature_field_value(f: &mut Feature, field: &str, v: Real) {
    set_feature_value(f, field, v);
    if let Some(fp) = feature_params_mut(f)
        && let Some(p) = fp.0.get_mut(field)
    {
        p.expr = None;
    }
}

/// The unit of a feature's numeric field.
pub fn feature_field_unit(f: &Feature, field: &str) -> Option<Unit> {
    feature_fields(f)
        .into_iter()
        .find(|(n, _)| *n == field)
        .map(|(_, u)| u)
}

/// The parameter (name, equation) recorded for a feature field.
pub fn feature_field_param(f: &Feature, field: &str) -> Option<FeatureParam> {
    feature_params(f).and_then(|p| p.0.get(field).cloned())
}

impl PartDocument {
    /// Evaluates text typed into a numeric field (`20*2`, `width + 5`,
    /// `dist = 40 mm`) against the part's parameters, in `unit`. Returns
    /// (new name, value, equation to keep — `None` for a plain number).
    /// `own` is the field's current parameter name (it may not refer to
    /// itself, and keeping it is not a clash).
    pub fn eval_field(
        &self,
        text: &str,
        unit: Unit,
        own: Option<&str>,
    ) -> Result<(Option<String>, Real, Option<String>), String> {
        eval_field_in(&self.parameter_table(), text, unit, own)
    }

    /// Applies field text to `field` of a feature that is not (yet) in the
    /// part — the open dialog of a command. Nothing changes on error.
    pub fn apply_draft_field(
        &self,
        f: &mut Feature,
        field: &str,
        text: &str,
    ) -> Result<(), String> {
        let unit = feature_field_unit(f, field).ok_or("not a parameter field")?;
        let own = feature_field_param(f, field).map(|p| p.name);
        let (name, value, keep) = self.eval_field(text, unit, own.as_deref())?;
        set_feature_value(f, field, value);
        if let Some(fp) = feature_params_mut(f) {
            let e = fp.0.entry(field.to_owned()).or_default();
            if let Some(n) = name {
                e.name = n;
            }
            e.expr = keep;
        }
        Ok(())
    }

    /// Applies field text to `field` of feature `k` (Edit Feature): a new
    /// name renames the parameter (equations follow), the rest becomes its
    /// equation. The part is regenerated; on error nothing changes.
    pub fn set_feature_field(
        &mut self,
        k: usize,
        field: &str,
        text: &str,
    ) -> Result<(), PartError> {
        let f = self
            .features
            .get(k)
            .ok_or_else(|| PartError::Feature(format!("no feature #{k}")))?;
        let own = feature_field_param(f, field)
            .map(|p| p.name)
            .filter(|n| !n.is_empty())
            .ok_or_else(|| PartError::Feature(format!("{field} has no parameter")))?;
        let unit = feature_field_unit(f, field).unwrap_or_default();
        let (name, _, _) = self
            .eval_field(text, unit, Some(&own))
            .map_err(PartError::Feature)?;
        let (_, body) = split_assignment(text);
        let before = self.clone();
        let mut current = own;
        if let Some(n) = name
            && n != current
        {
            self.rename_parameter(&current, &n)?;
            current = n;
        }
        if let Err(e) = self.set_parameter_expr(&current, &body) {
            *self = before;
            return Err(e);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sketch::{Constraint, Point2};

    #[test]
    fn feature_fields_take_expressions_and_names() {
        use crate::part::ExtrudeFeature;
        let mut part = PartDocument::default();
        let w = part.add_user_parameter(Unit::Mm);
        part.rename_parameter(&w, "width").unwrap();
        part.set_parameter_expr("width", "30").unwrap();
        // The open dialog: calculations, parameters, a new name.
        let mut f = Feature::Extrude(ExtrudeFeature::new(Vec::new(), 10.0));
        part.apply_draft_field(&mut f, "distance", "20*2").unwrap();
        assert_eq!(feature_value(&f, "distance"), Some(40.0));
        assert_eq!(
            feature_field_param(&f, "distance").unwrap().expr,
            Some("20*2".into())
        );
        part.apply_draft_field(&mut f, "distance", "width + 5")
            .unwrap();
        assert_eq!(feature_value(&f, "distance"), Some(35.0));
        part.apply_draft_field(&mut f, "taper", "3").unwrap();
        assert!((feature_value(&f, "taper").unwrap() - 3f64.to_radians()).abs() < 1e-12);
        part.apply_draft_field(&mut f, "distance", "depth = 12")
            .unwrap();
        let p = feature_field_param(&f, "distance").unwrap();
        assert_eq!((p.name.as_str(), p.expr.clone()), ("depth", None));
        // Errors change nothing.
        assert!(
            part.apply_draft_field(&mut f, "distance", "nope * 2")
                .is_err()
        );
        assert!(part.apply_draft_field(&mut f, "distance", "2 *").is_err());
        assert!(
            part.apply_draft_field(&mut f, "distance", "width = 3")
                .is_err(),
            "name taken"
        );
        assert_eq!(feature_value(&f, "distance"), Some(12.0));
        // An existing feature: rename + equation, followed by the table.
        part.features.push(f);
        part.sync_parameters();
        part.set_feature_field(0, "distance", "deep = width * 2")
            .unwrap();
        let row = part.parameter("deep").unwrap();
        assert_eq!((row.value, row.expr.as_deref()), (60.0, Some("width * 2")));
        assert!(
            part.set_feature_field(0, "distance", "deep + 1").is_err(),
            "self reference"
        );
        assert_eq!(part.parameter("deep").unwrap().value, 60.0);
    }

    #[test]
    fn dimensions_follow_equations_and_renames() {
        let mut part = PartDocument::default();
        let i = part
            .create_sketch(Default::default(), &super::super::SketchOptions::default())
            .unwrap();
        let sk = &mut part.sketches[i].sketch;
        let a = sk.add_point(Point2::new(0.0, 0.0));
        let b = sk.add_point(Point2::new(40.0, 0.0));
        let c = sk.add_point(Point2::new(0.0, 10.0));
        let w = sk
            .add_constraint(Constraint::Distance { a, b, value: 40.0 })
            .unwrap();
        let h = sk
            .add_constraint(Constraint::Distance {
                a,
                b: c,
                value: 10.0,
            })
            .unwrap();
        part.sync_parameters();
        let (nw, nh) = (
            part.sketches[i].sketch.dim_param(w).name,
            part.sketches[i].sketch.dim_param(h).name,
        );
        assert_eq!((nw.as_str(), nh.as_str()), ("d0", "d1"));
        part.set_parameter_expr("d1", "d0 / 4").unwrap();
        assert_eq!(
            part.sketches[i].sketch.constraint(h).unwrap().value(),
            Some(10.0)
        );
        part.set_parameter_expr("d0", "60").unwrap();
        assert_eq!(
            part.sketches[i].sketch.constraint(h).unwrap().value(),
            Some(15.0)
        );
        // A user parameter drives the width; renaming updates the equations.
        let u = part.add_user_parameter(Unit::Mm);
        part.set_parameter_expr(&u, "80 mm").unwrap();
        part.set_parameter_expr("d0", &u).unwrap();
        part.rename_parameter(&u, "width").unwrap();
        assert_eq!(part.parameter("d0").unwrap().expr.as_deref(), Some("width"));
        part.set_parameter_expr("width", "100").unwrap();
        assert_eq!(
            part.sketches[i].sketch.constraint(h).unwrap().value(),
            Some(25.0)
        );
        // Cycles and unknown names are refused; nothing changes.
        assert!(part.set_parameter_expr("width", "d1 * 2").is_err());
        assert!(part.set_parameter_expr("d1", "nope + 1").is_err());
        assert_eq!(
            part.parameter("d1").unwrap().expr.as_deref(),
            Some("d0 / 4")
        );
        assert!(part.delete_user_parameter("width").is_err(), "in use");
        // Saved and loaded.
        let back: PartDocument =
            serde_json::from_str(&serde_json::to_string(&part).unwrap()).unwrap();
        assert_eq!(
            back.parameter("d1").unwrap().expr.as_deref(),
            Some("d0 / 4")
        );
        assert_eq!(back.user_params.len(), 1);
    }
}

#[cfg(test)]
mod box_params {
    use super::super::PartDocument;
    use super::Unit;

    /// The box's sizes are parameters: an equation on a user parameter
    /// follows it (they were plain numbers, not parametric).
    /// The part's volume as a read-only parameter: a user parameter
    /// follows it after each regeneration; a size may not (it would change
    /// what it follows); units of the result are checked (`mm^3` into cm^3).
    #[test]
    fn physical_properties_in_equations() {
        let mut part = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
        let v = part.add_user_parameter(Unit::Ul);
        part.user_params[0].unit_spec = Some("cm^3".into());
        part.set_parameter_expr(&v, "PartVolume").ok();
        part.user_params[0].expr = "PartVolume".into();
        part.regenerate().unwrap();
        let shown = super::super::units::parse_spec("cm^3")
            .unwrap()
            .to_display(part.user_params[0].value);
        assert!((shown - 24.0).abs() < 1e-6, "{shown} cm^3");
        assert!(part.parameter("PartMass").is_some());
        // A size following the volume (through a length) is refused.
        let w = part.add_user_parameter(Unit::Mm);
        part.user_params[1].expr = "PartVolume / 1000 mm^2".into();
        part.regenerate().unwrap();
        assert!((part.user_params[1].value - 24.0).abs() < 1e-6);
        let e = part.set_feature_field(0, "width", &w).unwrap_err();
        assert!(e.to_string().contains("PartVolume"), "{e}");
        assert!(
            part.set_parameter_expr("PartMass", "5").is_err(),
            "read only"
        );
    }

    /// Unit specifications: a plain value keeps its size within a kind, an
    /// equation of the wrong kind is refused and nothing changes.
    #[test]
    fn user_parameter_units() {
        let mut part = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
        let k = part.add_user_parameter(Unit::Mm);
        part.set_parameter_expr(&k, "25.4").unwrap();
        part.set_user_unit(0, "in").unwrap();
        assert_eq!(part.user_params[0].expr, "1 in");
        assert!(part.user_params[0].unit_spec.is_none(), "an old kind");
        part.set_user_unit(0, "N/m").unwrap();
        assert_eq!(part.user_params[0].unit_spec.as_deref(), Some("N/m"));
        assert_eq!(
            part.user_params[0].expr, "1 N/m",
            "another kind keeps the number"
        );
        let f = part.add_user_parameter(Unit::Ul);
        part.set_user_unit(1, "N").unwrap();
        part.set_parameter_expr(&f, "500").unwrap();
        // 500 N over a 40 mm box length = 12.5 N/mm = 12500 N/m.
        part.set_parameter_expr(&k, &format!("{f} / 40 mm"))
            .unwrap();
        let shown = part.user_params[0]
            .def()
            .to_display(part.user_params[0].value);
        assert!((shown - 12_500.0).abs() < 1e-6, "{shown}");
        let before = part.user_params.clone();
        assert!(
            part.set_user_unit(0, "mm").is_err(),
            "N/m equation into a length"
        );
        assert_eq!(part.user_params, before);
        assert!(part.set_user_unit(0, "furlong").is_err());
    }

    /// Suppress if: a true / false parameter decides a feature; its
    /// dependents go with it; a condition that is not true / false is
    /// refused; the parameter cannot be purged while a condition uses it.
    #[test]
    fn suppress_if_conditions() {
        use crate::part::{ExtrudeFeature, Operation, ProfileRef, SketchOptions, SketchPlacement};
        let mut part = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
        let o = SketchOptions {
            auto_project_origin: false,
            auto_reference_edges: false,
            ..Default::default()
        };
        let j = part
            .create_sketch(SketchPlacement::on_face("Box1:Top"), &o)
            .unwrap();
        let c = part.sketches[j]
            .sketch
            .plane
            .project(&crate::kernel::Point3::new(20.0, 15.0, 20.0));
        part.sketches[j].sketch.add_circle(c, 5.0).unwrap();
        let rg = part.sketches[j].sketch.regions();
        let mut ex =
            ExtrudeFeature::new(vec![ProfileRef::of(&rg[0], rg[0].interior_point())], 10.0);
        ex.operation = Operation::Join;
        part.add_extrude(j, ex).unwrap();
        let k = part.last_inserted();
        let heavy = part.add_user_parameter(Unit::Bool);
        part.set_parameter_expr(&heavy, "true").unwrap();
        let faces = |p: &PartDocument| p.body.faces().count();
        let with_boss = faces(&part);
        part.set_suppress_if(k, Some(&format!("not {heavy}")))
            .unwrap();
        assert!(!part.is_suppressed(k));
        part.set_parameter_expr(&heavy, "false").unwrap();
        part.regenerate().unwrap();
        assert!(part.is_suppressed(k), "the condition is true now");
        assert!(faces(&part) < with_boss);
        assert!(
            part.unused_user_parameters().is_empty(),
            "the condition uses it"
        );
        let before = part.suppress_if.clone();
        assert!(
            part.set_suppress_if(k, Some("d0 + 1")).is_err(),
            "not true / false"
        );
        assert_eq!(part.suppress_if, before);
        part.set_suppress_if(k, None).unwrap();
        assert!(!part.is_suppressed(k));
    }

    /// Multi-Value: the list is checked value by value; a bad one changes
    /// nothing; the current value stays when it is in the list.
    #[test]
    fn multi_value_lists() {
        let mut part = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
        let k = part.add_user_parameter(Unit::Mm);
        part.set_parameter_expr(&k, "8 mm").unwrap();
        let list: Vec<String> = ["6 mm", "8 mm", "10 mm"].map(String::from).to_vec();
        part.set_user_values(0, list.clone()).unwrap();
        assert_eq!(part.user_params[0].values, list);
        assert!((part.user_params[0].value - 8.0).abs() < 1e-12, "kept");
        let before = part.user_params.clone();
        assert!(
            part.set_user_values(0, vec!["6 mm".into(), "3 kg".into()])
                .is_err()
        );
        assert_eq!(part.user_params, before);
        part.set_user_values(0, Vec::new()).unwrap();
        assert!(part.user_params[0].values.is_empty());
    }

    /// CSV round trip: export, change, import back (known names take the
    /// equation, new ones become user parameters with their units); a bad
    /// row changes nothing.
    #[test]
    fn parameters_csv_round_trip() {
        let mut part = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
        let k = part.add_user_parameter(Unit::Mm);
        part.set_parameter_expr(&k, "12").unwrap();
        part.user_params[0].comment = "wall, outer".into();
        let csv = part.parameters_csv();
        assert!(csv.contains("\"wall, outer\""), "{csv}");
        assert!(csv.starts_with("Name,Unit,Equation"));
        let text = format!("{csv}Force,N,500 N,500,load\n{k},mm,15 mm,15,\n");
        assert_eq!(
            part.import_parameters_csv(&text).unwrap(),
            csv.lines().count() - 1 + 2
        );
        assert_eq!(part.user_params[0].expr, "15 mm");
        let force = part.user_params.iter().find(|u| u.name == "Force").unwrap();
        assert_eq!(force.unit_spec.as_deref(), Some("N"));
        assert_eq!(force.comment, "load");
        let before = part.user_params.clone();
        assert!(
            part.import_parameters_csv("Name,Unit,Equation\nBad,mm,1 kg\n")
                .is_err()
        );
        assert_eq!(part.user_params, before, "nothing changed");
        // A rename carries the key flag.
        part.param_meta.insert(
            "Force".into(),
            super::ParamMeta {
                key: true,
                precision: Some(2),
            },
        );
        part.rename_parameter("Force", "Load").unwrap();
        assert!(part.param_meta.get("Load").is_some_and(|m| m.key));
    }

    /// Pick whip: a link makes the target follow the source; a circular
    /// link is refused; Purge Unused keeps what is used.
    #[test]
    fn link_and_purge() {
        let mut part = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
        let a = part.add_user_parameter(Unit::Mm);
        part.set_parameter_expr(&a, "25").unwrap();
        let b = part.add_user_parameter(Unit::Mm);
        let c = part.add_user_parameter(Unit::Mm);
        part.link_parameter(&b, &a).unwrap();
        assert!((part.user_params[1].value - 25.0).abs() < 1e-12);
        let before = part.user_params.clone();
        assert!(part.link_parameter(&a, &b).is_err(), "a circle");
        assert_eq!(part.user_params, before, "unchanged after the refusal");
        assert!(part.link_parameter(&a, &a).is_err());
        part.set_feature_field(0, "width", &b).unwrap();
        assert_eq!(part.unused_user_parameters(), vec![c.clone()]);
        assert_eq!(part.purge_unused_parameters(), vec![c]);
        assert_eq!(part.user_params.len(), 2);
    }

    #[test]
    fn box_size_follows_a_user_parameter() {
        let mut part = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
        let w = part.add_user_parameter(Unit::Mm);
        part.set_parameter_expr(&w, "25 mm").unwrap();
        part.set_feature_field(0, "width", &w).unwrap();
        part.regenerate().unwrap();
        let size = |p: &PartDocument| match &p.features[0] {
            super::super::Feature::Box(b) => b.size,
            _ => panic!("no box"),
        };
        assert_eq!(size(&part)[1], 25.0);
        part.set_parameter_expr(&w, "32 mm").unwrap();
        part.regenerate().unwrap();
        assert_eq!(size(&part)[1], 32.0);
        part.set_feature_field(0, "length", &format!("2 * {w}"))
            .unwrap();
        part.regenerate().unwrap();
        assert_eq!(size(&part)[0], 64.0);
    }
}

#[cfg(test)]
mod pattern_params {
    use super::super::{OriginPlane, PartDocument, SketchOptions, SketchPlacement};
    use super::Unit;
    use crate::sketch::{Entity, PatternDirection, Point2, Style};

    /// A sketch pattern's spacing dimension with an equation on a user
    /// parameter: the copies follow the parameter (what the pattern dialog
    /// sets when a parameter name is typed into Spacing).
    #[test]
    fn pattern_spacing_follows_a_parameter() {
        let opts = SketchOptions {
            auto_project_origin: false,
            ..Default::default()
        };
        let mut part = PartDocument::default();
        let i = part
            .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts)
            .unwrap();
        let sk = &mut part.sketches[i].sketch;
        let a = sk.add_fixed_point(Point2::new(0.0, -10.0));
        let b = sk.add_fixed_point(Point2::new(10.0, -10.0));
        let axis = sk.add_line_between(a, b).unwrap();
        sk.set_style(axis, Style::Construction).unwrap();
        let c = sk.add_circle(Point2::new(0.0, 0.0), 2.0).unwrap();
        let Some(Entity::Circle { center, .. }) = sk.entity(c).cloned() else {
            panic!("circle")
        };
        sk.set_fixed(center, true).unwrap();
        let dir = PatternDirection {
            line: axis,
            flip: false,
            count: 3,
            spacing: 10.0,
            fitted: false,
        };
        sk.rectangular_pattern(&[c], dir, None).unwrap();
        let (pid, p) = sk.patterns().next().unwrap();
        let _ = pid;
        let dim = *p
            .constraints
            .iter()
            .find(|&&k| sk.constraint(k).and_then(|x| x.value()).is_some())
            .unwrap();
        let w = part.add_user_parameter(Unit::Mm);
        part.set_parameter_expr(&w, "12 mm").unwrap();
        part.sketches[i].sketch.dim_param_mut(dim).unwrap().expr = Some(w.clone());
        part.regenerate().unwrap();
        let last_x = |part: &PartDocument| {
            let sk = &part.sketches[i].sketch;
            sk.entities()
                .filter_map(|(_, e)| match e {
                    Entity::Circle { center, .. } => sk.point(*center).map(|p| p.position.x),
                    _ => None,
                })
                .fold(f64::MIN, f64::max)
        };
        assert!((last_x(&part) - 24.0).abs() < 1e-6, "{}", last_x(&part));
        part.set_parameter_expr(&w, "15 mm").unwrap();
        part.regenerate().unwrap();
        assert!((last_x(&part) - 30.0).abs() < 1e-6, "{}", last_x(&part));

        // Count on a unitless parameter: 3 → 5 copies along; the spacing
        // keeps its equation through the rebuild.
        let n = part.add_user_parameter(Unit::Ul);
        part.set_parameter_expr(&n, "3").unwrap();
        let (pid, _) = part.sketches[i].sketch.patterns().next().unwrap();
        part.sketches[i]
            .sketch
            .set_pattern_count_exprs(pid, [Some(format!("{n} + 2")), None])
            .unwrap();
        part.regenerate().unwrap();
        assert!((last_x(&part) - 60.0).abs() < 1e-6, "{}", last_x(&part));
        part.set_parameter_expr(&w, "10 mm").unwrap();
        part.regenerate().unwrap();
        assert!((last_x(&part) - 40.0).abs() < 1e-6, "{}", last_x(&part));
    }
}

/// Suppress if, end to end: conditions of every kind, dependents, renames,
/// deletes, save / load, refusals that leave the part as it was.
#[cfg(test)]
mod suppress_if_tests {
    use super::*;
    use crate::part::history::DeleteOptions;
    use crate::part::{ExtrudeFeature, Operation, ProfileRef, SketchOptions, SketchPlacement};

    /// A 40 × 30 × 20 box, a boss (Extrusion1) on its top and a second boss
    /// (Extrusion2) on the first one's end face, so Extrusion2 depends on
    /// Extrusion1. Returns the part and both feature indices.
    fn two_bosses() -> (PartDocument, usize, usize) {
        let mut part = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
        let o = SketchOptions {
            auto_project_origin: false,
            auto_reference_edges: false,
            ..Default::default()
        };
        let boss = |part: &mut PartDocument, face: &str, r: f64, z: f64| {
            let j = part
                .create_sketch(SketchPlacement::on_face(face), &o)
                .unwrap();
            let c = part.sketches[j]
                .sketch
                .plane
                .project(&crate::kernel::Point3::new(20.0, 15.0, z));
            part.sketches[j].sketch.add_circle(c, r).unwrap();
            let rg = part.sketches[j].sketch.regions();
            let mut ex =
                ExtrudeFeature::new(vec![ProfileRef::of(&rg[0], rg[0].interior_point())], 10.0);
            ex.operation = Operation::Join;
            part.add_extrude(j, ex).unwrap();
            part.last_inserted()
        };
        let a = boss(&mut part, "Box1:Top", 8.0, 20.0);
        let b = boss(&mut part, "Extrusion1:End", 4.0, 30.0);
        (part, a, b)
    }

    fn faces(p: &PartDocument) -> usize {
        p.body.faces().count()
    }

    fn user(part: &mut PartDocument, unit: Unit, expr: &str) -> String {
        let n = part.add_user_parameter(unit);
        part.set_parameter_expr(&n, expr).unwrap();
        n
    }

    /// A condition on a length: compared in one unit (cm against mm).
    #[test]
    fn length_comparison_in_any_unit() {
        let (mut part, a, _) = two_bosses();
        let full = faces(&part);
        let w = user(&mut part, Unit::Mm, "120 mm");
        part.set_suppress_if(a, Some(&format!("{w} < 10 cm")))
            .unwrap();
        assert!(!part.is_suppressed(a));
        part.set_parameter_expr(&w, "80 mm").unwrap();
        part.regenerate().unwrap();
        assert!(part.is_suppressed(a));
        assert!(faces(&part) < full);
        // Back again: the same part as before (faces, volume).
        part.set_parameter_expr(&w, "120 mm").unwrap();
        part.regenerate().unwrap();
        assert!(!part.is_suppressed(a));
        assert_eq!(faces(&part), full);
    }

    /// The dependents of a conditionally suppressed feature go with it, and
    /// come back with it.
    #[test]
    fn dependents_follow() {
        let (mut part, a, b) = two_bosses();
        let on = user(&mut part, Unit::Bool, "true");
        part.set_suppress_if(a, Some(&format!("not {on}"))).unwrap();
        part.set_parameter_expr(&on, "false").unwrap();
        part.regenerate().unwrap();
        assert!(part.is_suppressed(a) && part.is_suppressed(b));
        assert!(part.feature_health.is_empty(), "{:?}", part.feature_health);
        part.set_parameter_expr(&on, "true").unwrap();
        part.regenerate().unwrap();
        assert!(!part.is_suppressed(a) && !part.is_suppressed(b));
    }

    /// Text parameters, and / or, if() inside the condition.
    #[test]
    fn text_and_logic() {
        let (mut part, a, b) = two_bosses();
        let mat = user(&mut part, Unit::Text, "\"Steel\"");
        let n = user(&mut part, Unit::Ul, "3");
        part.set_suppress_if(a, Some(&format!("{mat} == \"Al\" and {n} > 2")))
            .unwrap();
        assert!(!part.is_suppressed(a));
        part.set_parameter_expr(&mat, "\"Al\"").unwrap();
        part.regenerate().unwrap();
        assert!(part.is_suppressed(a));
        part.set_suppress_if(b, Some(&format!("if({n} > 5, true, {mat} != \"Al\")")))
            .unwrap();
        // a is suppressed (b with it); the condition of b alone is false.
        assert!(part.is_suppressed(b));
        part.set_suppress_if(a, None).unwrap();
        assert!(!part.is_suppressed(a) && !part.is_suppressed(b));
        part.set_parameter_expr(&n, "6").unwrap();
        part.regenerate().unwrap();
        assert!(part.is_suppressed(b) && !part.is_suppressed(a));
    }

    /// A condition may use the model's own dimensions (d-names) and
    /// user parameters with equations.
    #[test]
    fn model_parameters_and_equations() {
        let (mut part, a, b) = two_bosses();
        let d = part
            .parameter_table()
            .into_iter()
            .find(|r| matches!(r.owner, ParamOwner::Feature { feature, .. } if feature == a))
            .map(|r| r.name)
            .unwrap();
        let big = user(&mut part, Unit::Bool, &format!("{d} > 15 mm"));
        part.set_suppress_if(b, Some(&big)).unwrap();
        assert!(!part.is_suppressed(b));
        part.set_parameter_expr(&d, "20 mm").unwrap();
        part.regenerate().unwrap();
        assert!(part.is_suppressed(b), "{big} is true now");
        assert!(!part.is_suppressed(a));
    }

    /// Refusals leave everything as it was: not true / false, an unknown
    /// name, a physical property, a feature that does not exist.
    #[test]
    fn refusals_change_nothing() {
        let (mut part, a, _) = two_bosses();
        let on = user(&mut part, Unit::Bool, "true");
        part.set_suppress_if(a, Some(&format!("not {on}"))).unwrap();
        let before = (
            part.suppress_if.clone(),
            part.cond_suppressed.clone(),
            faces(&part),
        );
        for bad in ["12 mm", "Nope > 1", "Mass > 1 kg", "not (", "\"text\""] {
            assert!(
                part.set_suppress_if(a, Some(bad)).is_err(),
                "{bad} accepted"
            );
            assert_eq!(
                (
                    part.suppress_if.clone(),
                    part.cond_suppressed.clone(),
                    faces(&part)
                ),
                before,
                "{bad} changed the part"
            );
        }
        assert!(part.set_suppress_if(99, Some("true")).is_err());
        // Blank text removes it.
        part.set_suppress_if(a, Some("   ")).unwrap();
        assert!(part.suppress_if.is_empty());
    }

    /// Renaming a parameter rewrites the condition; a used parameter cannot
    /// be deleted or purged.
    #[test]
    fn renames_and_deletes_of_parameters() {
        let (mut part, a, _) = two_bosses();
        let on = user(&mut part, Unit::Bool, "false");
        part.set_suppress_if(a, Some(&format!("not {on}"))).unwrap();
        assert!(part.is_suppressed(a));
        part.rename_parameter(&on, "HasBoss").unwrap();
        assert_eq!(
            part.suppress_if.values().next().map(String::as_str),
            Some("not HasBoss")
        );
        part.regenerate().unwrap();
        assert!(part.is_suppressed(a), "still evaluated after the rename");
        assert!(part.delete_user_parameter("HasBoss").is_err());
        assert!(part.purge_unused_parameters().is_empty());
        assert!(
            part.parameter_users("HasBoss")
                .iter()
                .any(|u| u.starts_with("Suppress if"))
        );
    }

    /// Deleting the feature drops its condition, also for the dependents
    /// deleted with it.
    #[test]
    fn deleting_features_drops_their_conditions() {
        let (mut part, a, b) = two_bosses();
        part.set_suppress_if(b, Some("false")).unwrap();
        part.set_suppress_if(a, Some("false")).unwrap();
        part.delete_features(a, DeleteOptions::default()).unwrap();
        assert!(part.suppress_if.is_empty(), "{:?}", part.suppress_if);
        assert!(part.cond_suppressed.is_empty());
    }

    /// Save and load: the conditions are kept and evaluated again.
    #[test]
    fn save_and_load() {
        let (mut part, a, b) = two_bosses();
        let on = user(&mut part, Unit::Bool, "false");
        part.set_suppress_if(a, Some(&format!("not {on}"))).unwrap();
        let full_faces = faces(&part);
        let json = serde_json::to_string(&part).unwrap();
        let mut back: PartDocument = serde_json::from_str(&json).unwrap();
        back.regenerate().unwrap();
        assert_eq!(back.suppress_if, part.suppress_if);
        assert!(back.is_suppressed(a) && back.is_suppressed(b));
        assert_eq!(faces(&back), full_faces);
    }

    /// A manual suppression and a condition: either one suppresses.
    #[test]
    fn manual_and_conditional_suppression() {
        let (mut part, a, _) = two_bosses();
        part.set_suppress_if(a, Some("false")).unwrap();
        assert!(!part.is_suppressed(a));
        part.suppress_feature(a).unwrap();
        assert!(part.is_suppressed(a));
        part.unsuppress_feature(a).unwrap();
        part.set_suppress_if(a, Some("true")).unwrap();
        assert!(part.is_suppressed(a));
    }

    /// Changing the unit of a parameter a condition uses into one that
    /// breaks the condition is refused, with nothing changed.
    #[test]
    fn unit_change_that_breaks_a_condition_is_refused() {
        let (mut part, _, b) = two_bosses();
        let on = user(&mut part, Unit::Bool, "true");
        part.set_suppress_if(b, Some(&format!("not {on}"))).unwrap();
        let i = part.user_params.iter().position(|u| u.name == on).unwrap();
        let before = part.user_params[i].clone();
        let r = part.set_user_unit(i, "mm");
        assert!(r.is_err(), "accepted: {r:?}");
        assert_eq!(part.user_params[i].unit, before.unit);
        assert_eq!(part.suppress_if_state(b), Some(Ok(false)));
        assert!(part.param_errors.is_empty(), "{:?}", part.param_errors);
    }

    /// A condition whose dimension went with a deleted feature is kept (the
    /// user's text) and its feature shows why it is not evaluated.
    #[test]
    fn a_condition_that_lost_its_parameter_is_health() {
        let (mut part, a, b) = two_bosses();
        let d = part
            .parameter_table()
            .into_iter()
            .find(|r| matches!(r.owner, ParamOwner::Feature { feature, .. } if feature == b))
            .map(|r| r.name)
            .unwrap();
        part.set_suppress_if(a, Some(&format!("{d} > 50 mm")))
            .unwrap();
        part.delete_features(b, DeleteOptions::default()).unwrap();
        let id = part.features[a].id().to_owned();
        assert!(part.suppress_if.contains_key(&id), "the text stays");
        assert!(matches!(part.suppress_if_state(a), Some(Err(_))));
        let h = part.feature_health.get(&id).cloned().unwrap_or_default();
        assert!(h.starts_with("Suppress if:") && h.contains(&d), "{h}");
        assert!(
            !part.is_suppressed(a),
            "built while the condition cannot be read"
        );
        // Fixing the condition clears it.
        part.set_suppress_if(a, Some("false")).unwrap();
        assert!(!part.feature_health.contains_key(&id));
    }

    /// The condition's current state for the GUI: true, false or the error.
    #[test]
    fn condition_state() {
        let (mut part, a, _) = two_bosses();
        let n = user(&mut part, Unit::Ul, "1");
        part.set_suppress_if(a, Some(&format!("{n} > 2"))).unwrap();
        assert_eq!(part.suppress_if_state(a), Some(Ok(false)));
        assert_eq!(part.check_condition(&format!("{n} < 2")), Ok(true));
        assert!(part.check_condition("12 mm").is_err());
        assert_eq!(part.suppress_if_state(99), None);
    }
}
