//! The part's parameter table (Inventor: Manage → Parameters, fx).
//!
//! Every driving sketch dimension and every numeric value of a feature is a
//! **model parameter** with a name (`d0`, `d1`, …); the user adds **user
//! parameters**. Each has a value or an **equation** over the others. See
//! `docs/spec/0.5-parameters.md`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::expr::{self, Unit};
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
}

impl ParamRow {
    /// The text in the Equation column.
    pub fn equation(&self) -> String {
        self.expr
            .clone()
            .unwrap_or_else(|| format_value(self.value, self.unit))
    }
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
const SET_RADII: [&str; 8] = [
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
                    consumed_by: ps.name.clone(),
                    unit,
                    expr: p.expr.clone(),
                    // Angles are shown unsigned.
                    value: if unit == Unit::Deg {
                        value.abs()
                    } else {
                        value
                    },
                    tol: Some(p.tol),
                    comment: p.comment.clone(),
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
            });
        }
        rows
    }

    /// Evaluates every equation in dependency order and writes the results
    /// into the dimensions, feature values and user parameters. Errors go
    /// to [`PartDocument::param_errors`]; such parameters keep their value.
    pub fn evaluate_parameters(&mut self) {
        self.apply_pattern_counts();
        let rows = self.parameter_table();
        let mut list: Vec<(String, String, Unit)> = rows
            .iter()
            .map(|r| (r.name.clone(), r.equation(), r.unit))
            .collect();
        for u in &self.user_params {
            if let Some(e) = list.iter_mut().find(|e| e.0 == u.name) {
                e.1 = u.expr.clone();
            }
        }
        let (values, errors) = expr::eval_all(&list);
        for r in &rows {
            let Some(&v) = values.get(&r.name) else {
                continue;
            };
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
        self.param_errors = errors.into_iter().collect();
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
        let mut list: Vec<(String, String, Unit)> = rows
            .iter()
            .map(|r| (r.name.clone(), r.equation(), r.unit))
            .collect();
        for u in &self.user_params {
            if let Some(e) = list.iter_mut().find(|e| e.0 == u.name) {
                e.1 = u.expr.clone();
            }
        }
        let (values, _) = expr::eval_all(&list);
        let lookup = |n: &str| values.get(n).copied();
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
        let (value, _) = expr::eval(text, row.unit, &|n| {
            self.parameter_table()
                .into_iter()
                .find(|r| r.name == n)
                .map(|r| r.value)
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
        let value = unit.from_display(if unit == Unit::Deg { 0.0 } else { 1.0 });
        self.user_params.push(UserParam {
            name: name.clone(),
            expr: format_value(value, unit),
            unit,
            comment: String::new(),
            value,
        });
        name
    }

    /// Parameters whose equations use `name`.
    pub fn parameter_users(&self, name: &str) -> Vec<String> {
        self.parameter_table()
            .into_iter()
            .filter(|r| {
                r.expr
                    .as_deref()
                    .is_some_and(|e| expr::names_in(e).iter().any(|n| n == name))
            })
            .map(|r| r.name)
            .collect()
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

/// Splits field input `name = expr` into the new name and the expression
/// (`None` when there is no valid assignment).
pub fn split_assignment(text: &str) -> (Option<String>, String) {
    if let Some((lhs, rhs)) = text.split_once('=') {
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
    unit: Unit,
    own: Option<&str>,
) -> Result<(Option<String>, Real, Option<String>), String> {
    let (name, body) = split_assignment(text);
    if let Some(n) = &name
        && Some(n.as_str()) != own
        && table.iter().any(|r| r.name == *n)
    {
        return Err(format!("the name '{n}' is already used"));
    }
    let (value, used) = expr::eval(&body, unit, &|n| {
        (Some(n) != own)
            .then(|| table.iter().find(|r| r.name == n).map(|r| r.value))
            .flatten()
    })?;
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
