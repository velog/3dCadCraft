//! Part tools: sketches, features, parameters and queries (spec 0.7 §3.2–3.4).
//!
//! Every tool goes through the part's own operations (`create_sketch`, `add_extrude`,
//! `add_hole`…), so it builds, names and fails exactly as the GUI's command does.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};
use slotmap::Key;

use super::args::{Args, p3, point2, point3, round};
use super::names;
use super::session::{ApiError, Session};
use super::tools::{obj, part_prop};
use crate::kernel::{BRep, EdgeId, Point3, Real, TessellationOptions};
use crate::part::blend::{BlendFeature, EdgeRef, edge_ref};
use crate::part::draft::PullRef;
use crate::part::extrude::{Extent, ExtentDirection, ExtrudeFeature, ProfileRef, default_profiles};
use crate::part::hole::{HoleCenter, HoleFeature, HoleSpec, ThreadSpec};
use crate::part::pattern::{PatternDirection, PatternFeature, PatternKind};
use crate::part::revolve::{AxisRef, RevolveExtent, RevolveFeature};
use crate::part::shell::{FaceRef, ShellDirection, ShellFeature};
use crate::part::standards::{self, Standards};
use crate::part::work::LineRef;
use crate::part::{
    FastenerFit, Feature, Operation, OriginPlane, PartDocument, SketchOptions, SketchPlacement,
    Termination,
};
use crate::sketch::{Constraint, ConstraintId, Entity, EntityId, Point2, PointId, Sketch};

// ---- handles ---------------------------------------------------------------

fn idx<K: Key>(k: K) -> u64 {
    k.data().as_ffi() & 0xffff_ffff
}

/// The handle of a sketch point (`p3`).
pub fn ph(p: PointId) -> String {
    format!("p{}", idx(p))
}

/// The handle of a sketch curve (`e3`).
pub fn eh(e: EntityId) -> String {
    format!("e{}", idx(e))
}

/// The handle of a constraint (`c3`).
pub fn ch(c: ConstraintId) -> String {
    format!("c{}", idx(c))
}

pub(super) fn find_point(sk: &Sketch, h: &str) -> Option<PointId> {
    sk.points().map(|(id, _)| id).find(|id| ph(*id) == h)
}

pub(super) fn find_curve(sk: &Sketch, h: &str) -> Option<EntityId> {
    sk.entities().map(|(id, _)| id).find(|id| eh(*id) == h)
}

pub(super) fn part_mut<'s>(s: &'s mut Session, a: &Args) -> Result<&'s mut PartDocument, ApiError> {
    let name = a.opt_str("part")?;
    s.part_mut(a.cmd, name)
}

pub(super) fn part_ref<'s>(s: &'s Session, a: &Args) -> Result<&'s PartDocument, ApiError> {
    let name = a.opt_str("part")?;
    s.part(a.cmd, name)
}

pub(super) fn sketch_index(p: &PartDocument, a: &Args) -> Result<usize, ApiError> {
    let s = a.str("sketch")?;
    p.sketches
        .iter()
        .position(|ps| ps.id == s || ps.name == s)
        .ok_or_else(|| a.err(format!("no sketch '{s}'")))
}

pub(super) fn perr(a: &Args, e: impl std::fmt::Display) -> ApiError {
    a.err(e.to_string())
}

/// A point argument: a handle of an existing point, or `[x, y]` for a new one.
fn point_arg(sk: &mut Sketch, a: &Args, key: &str) -> Result<PointId, ApiError> {
    let v = a
        .get(key)
        .ok_or_else(|| a.err(format!("'{key}' is required")))?;
    match v {
        Value::String(h) => find_point(sk, h).ok_or_else(|| a.err(format!("no point '{h}'"))),
        _ => Ok(sk.add_point(point2(a, v, key)?)),
    }
}

pub(super) fn sk_prop() -> Value {
    json!({"type": "string", "description": "Sketch id (Sketch1)"})
}

fn pt_prop(what: &str) -> Value {
    json!({"description": format!("{what}: [x, y] in sketch mm, or a point handle such as p3"),
           "anyOf": [{"type": "array", "items": {"type": "number"}}, {"type": "string"}]})
}

/// After a geometry change: solve, rebuild the features using the sketch, and answer with
/// the new handles and the DOF left.
fn geometry_answer(
    part: &mut PartDocument,
    i: usize,
    a: &Args,
    points: &[PointId],
    curves: &[EntityId],
) -> Result<Value, ApiError> {
    let r = part.sketches[i].sketch.solve().map_err(|e| perr(a, e))?;
    rebuild_users(part, i, a)?;
    Ok(json!({
        "points": points.iter().map(|p| ph(*p)).collect::<Vec<_>>(),
        "curves": curves.iter().map(|e| eh(*e)).collect::<Vec<_>>(),
        "dof": r.dof,
    }))
}

/// Rebuilds the part when a feature uses sketch `i`; a feature the change breaks fails the call.
fn rebuild_users(part: &mut PartDocument, i: usize, a: &Args) -> Result<(), ApiError> {
    if part.consumers(&part.sketches[i].id).is_empty() {
        return Ok(());
    }
    let before: Vec<String> = part.feature_health.keys().cloned().collect();
    part.regenerate().map_err(|e| perr(a, e))?;
    let new: Vec<String> = part
        .feature_health
        .iter()
        .filter(|(k, _)| !before.contains(k))
        .map(|(k, v)| format!("{k}: {v}"))
        .collect();
    if new.is_empty() {
        Ok(())
    } else {
        Err(a.err(format!("the change breaks {}", new.join("; "))))
    }
}

// ---- sketch tools ----------------------------------------------------------

/// Schema of `sketch_create`.
pub fn sketch_create_schema() -> Value {
    obj(
        json!({
            "part": part_prop(),
            "plane": {"type": "string", "description": "Origin plane XY (default), XZ, YZ, or a work plane (WorkPlane1)"},
            "face": {"type": "string", "description": "A planar face's persistent name or alias (Extrusion1:Top, +Z) instead of a plane"},
        }),
        &[],
    )
}

/// `sketch_create`.
pub fn sketch_create(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    let face = match a.opt_str("face")? {
        Some(f) => Some(face_name(&part_ref(s, a)?.body, a, f)?),
        None => None,
    };
    let plane = a.opt_str("plane")?.unwrap_or("XY");
    let placement = match face {
        Some(f) => SketchPlacement::on_face(f),
        None if part_ref(s, a)?.work.planes.contains_key(plane) => SketchPlacement {
            plane: crate::part::PlaneRef::Work(plane.to_owned()),
            ..Default::default()
        },
        None => SketchPlacement::on_origin_plane(match plane.to_ascii_uppercase().as_str() {
            "XY" => OriginPlane::XY,
            "XZ" => OriginPlane::XZ,
            "YZ" => OriginPlane::YZ,
            _ => {
                return Err(a.err(format!(
                    "'plane' must be XY, XZ, YZ or a work plane, not '{plane}'"
                )));
            }
        }),
    };
    let part = part_mut(s, a)?;
    let i = part
        .create_sketch(placement, &SketchOptions::default())
        .map_err(|e| perr(a, e))?;
    let ps = &part.sketches[i];
    let pl = &ps.sketch.plane;
    let o = pl.point_at(0.0, 0.0);
    let (x, y) = (pl.point_at(1.0, 0.0) - o, pl.point_at(0.0, 1.0) - o);
    let fixed: Vec<String> = ps
        .sketch
        .points()
        .filter(|(_, p)| p.fixed)
        .map(|(id, _)| ph(id))
        .collect();
    Ok(json!({
        "sketch": ps.id,
        "origin": p3(&o),
        "x_axis": [round(x.x), round(x.y), round(x.z)],
        "y_axis": [round(y.x), round(y.y), round(y.z)],
        "projected_points": fixed,
    }))
}

/// Schema of `sketch_line`.
pub fn sketch_line_schema() -> Value {
    obj(
        json!({"part": part_prop(), "sketch": sk_prop(), "from": pt_prop("Start"), "to": pt_prop("End")}),
        &["sketch", "from", "to"],
    )
}

/// `sketch_line`.
pub fn sketch_line(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    let part = part_mut(s, a)?;
    let i = sketch_index(part, a)?;
    let sk = &mut part.sketches[i].sketch;
    let p = point_arg(sk, a, "from")?;
    let q = point_arg(sk, a, "to")?;
    let e = sk.add_line_between(p, q).map_err(|e| perr(a, e))?;
    geometry_answer(part, i, a, &[p, q], &[e])
}

/// Schema of `sketch_rectangle`.
pub fn sketch_rectangle_schema() -> Value {
    obj(
        json!({"part": part_prop(), "sketch": sk_prop(),
               "corner1": {"type": "array", "items": {"type": "number"}, "description": "[x, y] (the centre when center=true)"},
               "corner2": {"type": "array", "items": {"type": "number"}, "description": "[x, y] opposite corner"},
               "center": {"type": "boolean"}}),
        &["sketch", "corner1", "corner2"],
    )
}

/// `sketch_rectangle`.
pub fn sketch_rectangle(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    let c1 = point2(a, a.get("corner1").unwrap_or(&Value::Null), "corner1")?;
    let c2 = point2(a, a.get("corner2").unwrap_or(&Value::Null), "corner2")?;
    let center = a.flag("center")?;
    let part = part_mut(s, a)?;
    let i = sketch_index(part, a)?;
    let sk = &mut part.sketches[i].sketch;
    let shape = if center {
        sk.add_rectangle_center(c1, c2)
    } else {
        sk.add_rectangle_two_point(c1, c2)
    }
    .map_err(|e| perr(a, e))?;
    geometry_answer(part, i, a, &shape.points, &shape.entities)
}

/// Schema of `sketch_circle`.
pub fn sketch_circle_schema() -> Value {
    obj(
        json!({"part": part_prop(), "sketch": sk_prop(), "center": pt_prop("Centre"), "radius": {"type": "number"}}),
        &["sketch", "center", "radius"],
    )
}

/// `sketch_circle`.
pub fn sketch_circle(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    let r = a.num("radius")?;
    let part = part_mut(s, a)?;
    let i = sketch_index(part, a)?;
    let sk = &mut part.sketches[i].sketch;
    let c = point_arg(sk, a, "center")?;
    let e = sk.add_circle_at(c, r).map_err(|e| perr(a, e))?;
    geometry_answer(part, i, a, &[c], &[e])
}

/// Schema of `sketch_arc`.
pub fn sketch_arc_schema() -> Value {
    obj(
        json!({"part": part_prop(), "sketch": sk_prop(), "center": pt_prop("Centre"),
               "start": pt_prop("Start"), "end": pt_prop("End (counter-clockwise from the start)")}),
        &["sketch", "center", "start", "end"],
    )
}

/// `sketch_arc`.
pub fn sketch_arc(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    let part = part_mut(s, a)?;
    let i = sketch_index(part, a)?;
    let sk = &mut part.sketches[i].sketch;
    let c = point_arg(sk, a, "center")?;
    let p = point_arg(sk, a, "start")?;
    // A new end goes onto the circle through the start.
    let q = match a.get("end") {
        Some(Value::Array(_)) => {
            let want = point2(a, a.get("end").unwrap_or(&Value::Null), "end")?;
            let (cc, pp) = match (sk.point(c), sk.point(p)) {
                (Some(cc), Some(pp)) => (cc.position, pp.position),
                _ => return Err(a.err("the centre or start is missing")),
            };
            let r = (pp - cc).norm();
            let d = want - cc;
            let n = d.norm();
            if n < 1e-9 {
                return Err(a.err("the end lies on the centre"));
            }
            sk.add_point(cc + d * (r / n))
        }
        _ => point_arg(sk, a, "end")?,
    };
    let e = sk.add_arc(c, p, q).map_err(|e| perr(a, e))?;
    geometry_answer(part, i, a, &[c, p, q], &[e])
}

/// Schema of `sketch_point`.
pub fn sketch_point_schema() -> Value {
    obj(
        json!({"part": part_prop(), "sketch": sk_prop(), "at": {"type": "array", "items": {"type": "number"}},
               "center": {"type": "boolean", "description": "A Center Point (hole centre)"}}),
        &["sketch", "at"],
    )
}

/// `sketch_point`.
pub fn sketch_point(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    let at = point2(a, a.get("at").unwrap_or(&Value::Null), "at")?;
    let center = a.flag("center")?;
    let part = part_mut(s, a)?;
    let i = sketch_index(part, a)?;
    let sk = &mut part.sketches[i].sketch;
    let p = if center {
        sk.add_center_point(at)
    } else {
        sk.add_point(at)
    };
    geometry_answer(part, i, a, &[p], &[])
}

/// Schema of `sketch_constrain`.
pub fn sketch_constrain_schema() -> Value {
    obj(
        json!({"part": part_prop(), "sketch": sk_prop(),
               "constraint": {"type": "object", "description": "{type, ...handles, value}", "properties": {"type": {"type": "string"}}, "required": ["type"]}}),
        &["sketch", "constraint"],
    )
}

const CONSTRAINT_TYPES: &[&str] = &[
    "Coincident",
    "PointOnLine",
    "PointOnCircle",
    "PointOnSpline",
    "Midpoint",
    "Collinear",
    "Concentric",
    "Fix",
    "Horizontal",
    "Vertical",
    "HorizontalPoints",
    "VerticalPoints",
    "Parallel",
    "Perpendicular",
    "Tangent",
    "Equal",
    "Symmetric",
    "Distance",
    "HorizontalDistance",
    "VerticalDistance",
    "PointLineDistance",
    "DiameterToAxis",
    "Angle",
    "Radius",
    "Diameter",
];

/// The agent's constraint object with handles replaced by the sketch's ids.
fn constraint_of(sk: &Sketch, a: &Args, v: &Value) -> Result<Constraint, ApiError> {
    let mut m: Map<String, Value> = v
        .as_object()
        .cloned()
        .ok_or_else(|| a.err("'constraint' must be an object with 'type'"))?;
    let ty = m.get("type").and_then(Value::as_str).unwrap_or("");
    let ty = CONSTRAINT_TYPES
        .iter()
        .find(|t| t.eq_ignore_ascii_case(ty) || t.to_ascii_lowercase() == ty.replace('_', ""))
        .copied()
        .ok_or_else(|| a.err(format!("unknown constraint type '{ty}'")))?;
    m.insert("type".into(), json!(ty));
    for (k, val) in m.iter_mut() {
        if k == "type" {
            continue;
        }
        if let Value::String(h) = val {
            let key = if h.starts_with('p') {
                find_point(sk, h).map(serde_json::to_value)
            } else if h.starts_with('e') {
                find_curve(sk, h).map(serde_json::to_value)
            } else {
                None
            };
            *val = key
                .ok_or_else(|| a.err(format!("no point or curve '{h}'")))?
                .map_err(|e| perr(a, e))?;
        }
    }
    if ty == "Angle"
        && let Some(d) = m.get("value").and_then(Value::as_f64)
    {
        m.insert("value".into(), json!(d.to_radians()));
    }
    if ty == "Tangent" && !m.contains_key("internal") {
        m.insert("internal".into(), json!(false));
    }
    if ty == "Fix" && !m.contains_key("at") {
        let at = m
            .get("point")
            .and_then(|p| serde_json::from_value::<PointId>(p.clone()).ok())
            .and_then(|p| sk.point(p))
            .map(|p| p.position)
            .ok_or_else(|| a.err("Fix needs 'point'"))?;
        m.insert("at".into(), json!([at.x, at.y]));
    }
    serde_json::from_value(Value::Object(m)).map_err(|e| a.err(format!("{ty}: {e}")))
}

/// `sketch_constrain`.
pub fn sketch_constrain(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    let v = a
        .get("constraint")
        .ok_or_else(|| a.err("'constraint' is required"))?;
    let part = part_mut(s, a)?;
    let i = sketch_index(part, a)?;
    let sk = &mut part.sketches[i].sketch;
    let before = sk.solve().map_err(|e| perr(a, e))?;
    let c = constraint_of(sk, a, v)?;
    let dimension = c.is_dimension();
    let id = sk.add_constraint(c).map_err(|e| perr(a, e))?;
    let r = sk.solve().map_err(|e| perr(a, e))?;
    if r.redundant > before.redundant {
        return Err(a.err(
            "it would over-constrain the sketch (it repeats what the other constraints hold)",
        ));
    }
    if r.status != crate::sketch::SolveStatus::Solved {
        return Err(a.err(format!(
            "the sketch does not solve with it (residual {:.3e})",
            r.max_residual
        )));
    }
    let mut out = json!({"constraint": ch(id), "dof": r.dof});
    if dimension {
        part.sync_parameters();
        out["parameter"] = json!(part.sketches[i].sketch.dim_param(id).name);
    }
    rebuild_users(part, i, a)?;
    Ok(out)
}

/// Schema of `sketch_info`.
pub fn sketch_info_schema() -> Value {
    obj(
        json!({"part": part_prop(), "sketch": sk_prop(), "detail": {"type": "boolean"}}),
        &["sketch"],
    )
}

/// `sketch_info`.
pub fn sketch_info(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    let detail = a.flag("detail")?;
    let part = part_ref(s, a)?;
    let i = sketch_index(part, a)?;
    let ps = &part.sketches[i];
    let sk = &ps.sketch;
    let r = sk.clone().solve().map_err(|e| perr(a, e))?;
    let points: Vec<Value> = sk
        .points()
        .map(|(id, p)| {
            let mut v = json!({"h": ph(id), "at": [round(p.position.x), round(p.position.y)]});
            if p.fixed {
                v["fixed"] = json!(true);
            }
            v
        })
        .collect();
    let curves: Vec<Value> = sk
        .entities()
        .map(|(id, e)| {
            let mut v = match e {
                Entity::Line { start, end } => {
                    json!({"kind": "line", "points": [ph(*start), ph(*end)]})
                }
                Entity::Circle { center, radius } => {
                    json!({"kind": "circle", "points": [ph(*center)], "radius": round(*radius)})
                }
                Entity::Arc { center, start, end } => {
                    json!({"kind": "arc", "points": [ph(*center), ph(*start), ph(*end)]})
                }
                _ => json!({"kind": "spline"}),
            };
            v["h"] = json!(eh(id));
            if ps.is_reference(id) {
                v["projected"] = json!(true);
            }
            v
        })
        .collect();
    let regions: Vec<Value> = sk
        .regions()
        .iter()
        .enumerate()
        .map(|(k, g)| {
            let p = g.interior_point();
            json!({"index": k, "area": round(g.area()), "inside": [round(p.x), round(p.y)]})
        })
        .collect();
    let mut out = json!({
        "sketch": ps.id,
        "dof": r.dof,
        "redundant": r.redundant,
        "points": points,
        "curves": curves,
        "regions": regions,
        "constraints": sk.constraints().count(),
    });
    if detail {
        out["constraints"] = json!(
            sk.constraints()
                .map(|(id, c)| {
                    let mut v = json!({"h": ch(id), "type": c.name()});
                    if c.is_dimension() {
                        v["parameter"] = json!(sk.dim_param(id).name);
                    }
                    v
                })
                .collect::<Vec<_>>()
        );
    }
    Ok(out)
}

// ---- features ----------------------------------------------------------------

pub(super) fn operation(a: &Args, part: &PartDocument) -> Result<Operation, ApiError> {
    let first = part.body.solids().next().is_none();
    Ok(
        match a.choice(
            "operation",
            &["join", "cut", "intersect", "new_solid"],
            if first { "new_solid" } else { "join" },
        )? {
            "cut" => Operation::Cut,
            "intersect" => Operation::Intersect,
            "new_solid" => Operation::NewSolid,
            _ => Operation::Join,
        },
    )
}

pub(super) fn profiles(
    part: &PartDocument,
    i: usize,
    a: &Args,
) -> Result<Vec<ProfileRef>, ApiError> {
    let ps = &part.sketches[i];
    let regions = ps.sketch.regions();
    let list = match a.get("profiles") {
        None => default_profiles(ps, &regions),
        Some(Value::Array(ks)) => ks
            .iter()
            .map(|k| {
                let r = k
                    .as_u64()
                    .and_then(|k| regions.get(k as usize))
                    .ok_or_else(|| a.err(format!("no region {k} (see sketch_info)")))?;
                Ok(ProfileRef::of(r, r.interior_point()))
            })
            .collect::<Result<_, ApiError>>()?,
        Some(_) => return Err(a.err("'profiles' must list region indices")),
    };
    if list.is_empty() {
        return Err(a.err("the sketch has no closed region"));
    }
    Ok(list)
}

/// What a new feature answers: its id and the part's state.
pub(super) fn feature_answer(part: &PartDocument, id: &str) -> Value {
    let mut v = json!({
        "feature": id,
        "solids": part.body.solids().filter(|(_, s)| !s.open).count(),
        "exact": part.exact,
    });
    let sick: Vec<String> = part
        .feature_health
        .iter()
        .map(|(k, m)| format!("{k}: {m}"))
        .collect();
    if !sick.is_empty() {
        v["health"] = json!(sick);
    }
    v
}

/// Schema of `extrude`.
pub fn extrude_schema() -> Value {
    obj(
        json!({"part": part_prop(), "sketch": sk_prop(),
               "profiles": {"type": "array", "items": {"type": "integer"}, "description": "Region indices (default: the sketch's own closed regions)"},
               "distance": {"type": "number", "description": "mm"},
               "extent": {"type": "string", "enum": ["distance", "through_all"]},
               "direction": {"type": "string", "enum": ["default", "flipped", "symmetric"]},
               "operation": {"type": "string", "enum": ["join", "cut", "intersect", "new_solid"]},
               "taper": {"type": "number", "description": "degrees"}}),
        &["sketch"],
    )
}

/// `extrude`.
pub fn extrude(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    let part = part_mut(s, a)?;
    let i = sketch_index(part, a)?;
    let through = a.choice("extent", &["distance", "through_all"], "distance")? == "through_all";
    let distance = match a.opt_num("distance")? {
        Some(d) => d,
        None if through => 10.0,
        None => return Err(a.err("'distance' is required")),
    };
    let mut def = ExtrudeFeature::new(profiles(part, i, a)?, distance);
    if through {
        def.extent = Extent::ThroughAll;
    }
    def.direction = match a.choice("direction", &["default", "flipped", "symmetric"], "default")? {
        "flipped" => ExtentDirection::Flipped,
        "symmetric" => ExtentDirection::Symmetric,
        _ => ExtentDirection::Default,
    };
    def.operation = operation(a, part)?;
    def.taper = a.opt_num("taper")?.unwrap_or(0.0).to_radians();
    let id = part.add_extrude(i, def).map_err(|e| perr(a, e))?;
    part.sync_parameters();
    Ok(feature_answer(part, &id))
}

/// Schema of `revolve`.
pub fn revolve_schema() -> Value {
    obj(
        json!({"part": part_prop(), "sketch": sk_prop(),
               "profiles": {"type": "array", "items": {"type": "integer"}},
               "axis": {"type": "string", "description": "X, Y, Z or a sketch line handle (e3)"},
               "angle": {"type": "number", "description": "degrees, default 360"},
               "operation": {"type": "string", "enum": ["join", "cut", "intersect", "new_solid"]}}),
        &["sketch", "axis"],
    )
}

pub(super) fn origin_axis(s: &str) -> Option<usize> {
    match s.to_ascii_uppercase().as_str() {
        "X" => Some(0),
        "Y" => Some(1),
        "Z" => Some(2),
        _ => None,
    }
}

/// `revolve`.
pub fn revolve(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    let part = part_mut(s, a)?;
    let i = sketch_index(part, a)?;
    let axis_s = a.str("axis")?;
    let axis = match origin_axis(axis_s) {
        Some(k) => AxisRef::Origin(k),
        None => AxisRef::SketchLine(
            find_curve(&part.sketches[i].sketch, axis_s)
                .ok_or_else(|| a.err(format!("no sketch line '{axis_s}'")))?,
        ),
    };
    let mut def = RevolveFeature::new(profiles(part, i, a)?, axis);
    let angle = a.opt_num("angle")?.unwrap_or(360.0);
    if angle < 360.0 - 1e-9 {
        def.extent = RevolveExtent::Angle;
        def.angle = angle.to_radians();
    }
    def.operation = operation(a, part)?;
    let id = part.add_revolve(i, def).map_err(|e| perr(a, e))?;
    part.sync_parameters();
    Ok(feature_answer(part, &id))
}

/// Schema of `hole`.
pub fn hole_schema() -> Value {
    obj(
        json!({"part": part_prop(),
               "face": {"type": "string", "description": "Planar face (persistent name)"},
               "centers": {"type": "array", "items": {"type": "array", "items": {"type": "number"}}, "description": "[[x, y, z], ...] model mm"},
               "type": {"type": "string", "enum": ["simple", "clearance", "tapped"]},
               "diameter": {"type": "number"}, "depth": {"type": "number"},
               "through_all": {"type": "boolean"},
               "size": {"type": "string", "description": "Thread size, e.g. M8"},
               "fit": {"type": "string", "enum": ["close", "normal", "loose"]}}),
        &["face", "centers"],
    )
}

/// `hole`.
pub fn hole(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    let face = a.str("face")?.to_owned();
    let centers: Vec<Point3> = a
        .get("centers")
        .and_then(Value::as_array)
        .ok_or_else(|| a.err("'centers' must be a list of [x, y, z]"))?
        .iter()
        .map(|c| point3(a, c, "centers"))
        .collect::<Result<_, _>>()?;
    if centers.is_empty() {
        return Err(a.err("give at least one centre"));
    }
    let tables = Standards::builtin();
    let through = a.flag("through_all")?;
    let mut spec = match a.choice("type", &["simple", "clearance", "tapped"], "simple")? {
        "clearance" => {
            let size = a.str("size")?;
            let fit = match a.choice("fit", &["close", "normal", "loose"], "normal")? {
                "close" => FastenerFit::Close,
                "loose" => FastenerFit::Loose,
                _ => FastenerFit::Normal,
            };
            HoleSpec::clearance(&tables, "ISO 273", size, fit)
                .ok_or_else(|| a.err(format!("no ISO 273 clearance size '{size}'")))?
        }
        "tapped" => {
            let size = a.str("size")?;
            let t = ThreadSpec::from_table(&tables, standards::ISO_METRIC, size, true)
                .ok_or_else(|| a.err(format!("no ISO metric thread '{size}'")))?;
            let depth = a.opt_num("depth")?.unwrap_or(2.0 * t.major);
            HoleSpec::tapped(t, depth)
        }
        _ => {
            let d = a.num("diameter")?;
            let depth = match a.opt_num("depth")? {
                Some(x) => x,
                None if through => 10.0,
                None => return Err(a.err("give 'depth' or through_all")),
            };
            HoleSpec::simple(d, depth)
        }
    };
    if through {
        spec.termination = Termination::ThroughAll;
    }
    let part = part_mut(s, a)?;
    let face = face_name(&part.body, a, &face)?;
    let (i, ids) = part
        .create_hole_sketch(&face, &centers)
        .map_err(|e| perr(a, e))?;
    let mut def = HoleFeature::new(spec);
    def.sketch = part.sketches[i].id.clone();
    def.centers = ids.into_iter().map(HoleCenter::Point).collect();
    let id = part.add_hole(def).map_err(|e| perr(a, e))?;
    part.sync_parameters();
    let mut v = feature_answer(part, &id);
    v["holes"] = json!(part.holes().len());
    Ok(v)
}

/// Edge groups of a body: `"FaceA|FaceB"` (names sorted) → the pieces and their references.
fn edge_groups(body: &BRep) -> BTreeMap<String, Vec<(EdgeId, EdgeRef)>> {
    let mut out: BTreeMap<String, Vec<(EdgeId, EdgeRef)>> = BTreeMap::new();
    for (e, _) in body.edges() {
        if let Some(r) = edge_ref(body, e) {
            let mut n = r.faces.clone();
            n.sort();
            out.entry(format!("{}|{}", n[0], n[1]))
                .or_default()
                .push((e, r));
        }
    }
    out
}

/// The edges an agent named (`FaceA|FaceB` in either order, or an edge's own name).
pub(super) fn edges_named(
    body: &BRep,
    a: &Args,
    names: &[String],
) -> Result<Vec<(EdgeId, EdgeRef)>, ApiError> {
    let groups = edge_groups(body);
    let mut out = Vec::new();
    for n in names {
        let n = &names::resolve_edge(body, n);
        let key = match n.split_once('|') {
            Some((x, y)) => {
                let mut v = [x.trim().to_owned(), y.trim().to_owned()];
                v.sort();
                format!("{}|{}", v[0], v[1])
            }
            None => String::new(),
        };
        if let Some(g) = groups.get(&key) {
            out.extend(g.iter().cloned());
        } else if let Some(e) = body.find_edge(n)
            && let Some(r) = edge_ref(body, e)
        {
            out.push((e, r));
        } else {
            return Err(a.err(format!("no edge '{n}' (see edges)")));
        }
    }
    if out.is_empty() {
        return Err(a.err("give at least one edge"));
    }
    Ok(out)
}

fn edges_prop() -> Value {
    json!({"type": "array", "items": {"type": "string"}, "description": "Edges as 'FaceA|FaceB'"})
}

/// Schema of `fillet`.
pub fn fillet_schema() -> Value {
    obj(
        json!({"part": part_prop(), "edges": edges_prop(), "radius": {"type": "number"}}),
        &["edges", "radius"],
    )
}

/// Schema of `chamfer`.
pub fn chamfer_schema() -> Value {
    obj(
        json!({"part": part_prop(), "edges": edges_prop(), "distance": {"type": "number"}}),
        &["edges", "distance"],
    )
}

fn blend(s: &mut Session, a: &Args, fillet: bool) -> Result<Value, ApiError> {
    let names = a.strs("edges")?;
    let size = a.num(if fillet { "radius" } else { "distance" })?;
    let part = part_mut(s, a)?;
    let edges: Vec<EdgeRef> = edges_named(&part.body, a, &names)?
        .into_iter()
        .map(|(_, r)| r)
        .collect();
    let def = if fillet {
        BlendFeature::fillet(edges, size)
    } else {
        BlendFeature::chamfer(edges, size)
    };
    let id = part.add_blend(def).map_err(|e| perr(a, e))?;
    part.sync_parameters();
    Ok(feature_answer(part, &id))
}

/// `fillet`.
pub fn fillet(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    blend(s, a, true)
}

/// `chamfer`.
pub fn chamfer(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    blend(s, a, false)
}

/// A point inside face `name`: the centre of its largest display triangle.
pub fn face_point(body: &BRep, name: &str) -> Option<Point3> {
    let mut best: Option<(Real, Point3)> = None;
    for (_, f) in body.faces() {
        if f.name.as_deref() != Some(name) {
            continue;
        }
        let Some(m) = &f.mesh else { continue };
        for t in &m.triangles {
            let (Some(p), Some(q), Some(r)) = (
                m.positions.get(t[0] as usize),
                m.positions.get(t[1] as usize),
                m.positions.get(t[2] as usize),
            ) else {
                continue;
            };
            let area = (q - p).cross(&(r - p)).norm();
            if best.is_none_or(|(b, _)| area > b) {
                best = Some((area, Point3::from((p.coords + q.coords + r.coords) / 3.0)));
            }
        }
    }
    best.map(|(_, p)| p)
}

/// The persistent name of face `name` (a name or an alias, spec 0.7 §3.4a).
pub fn face_name(body: &BRep, a: &Args, name: &str) -> Result<String, ApiError> {
    names::resolve_face(body, name).ok_or_else(|| a.err(format!("no face '{name}' (see faces)")))
}

fn face_ref(body: &BRep, a: &Args, name: &str) -> Result<FaceRef, ApiError> {
    let name = face_name(body, a, name)?;
    let p =
        face_point(body, &name).ok_or_else(|| a.err(format!("no face '{name}' (see faces)")))?;
    Ok(FaceRef {
        face: name,
        hint: [p.x, p.y, p.z],
    })
}

/// Schema of `shell`.
pub fn shell_schema() -> Value {
    obj(
        json!({"part": part_prop(),
               "remove_faces": {"type": "array", "items": {"type": "string"}},
               "thickness": {"type": "number"},
               "direction": {"type": "string", "enum": ["inside", "outside", "both"]}}),
        &["thickness"],
    )
}

/// `shell`.
pub fn shell(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    let faces = a.strs("remove_faces")?;
    let mut def = ShellFeature::new(a.num("thickness")?);
    def.direction = match a.choice("direction", &["inside", "outside", "both"], "inside")? {
        "outside" => ShellDirection::Outside,
        "both" => ShellDirection::Center,
        _ => ShellDirection::Inside,
    };
    let part = part_mut(s, a)?;
    def.remove = faces
        .iter()
        .map(|f| face_ref(&part.body, a, f))
        .collect::<Result<_, _>>()?;
    let id = part.add_shell(def).map_err(|e| perr(a, e))?;
    part.sync_parameters();
    Ok(feature_answer(part, &id))
}

/// Schema of `pattern`.
pub fn pattern_schema() -> Value {
    obj(
        json!({"part": part_prop(),
               "features": {"type": "array", "items": {"type": "string"}, "description": "Feature ids to copy"},
               "kind": {"type": "string", "enum": ["rectangular", "circular"]},
               "direction": {"type": "string", "description": "X, Y, Z or a straight edge's name"},
               "count": {"type": "integer"}, "spacing": {"type": "number"},
               "direction2": {"type": "string"}, "count2": {"type": "integer"}, "spacing2": {"type": "number"},
               "axis": {"type": "string", "description": "X, Y or Z (circular)"},
               "angle": {"type": "number", "description": "degrees, default 360 (circular)"}}),
        &["features", "kind", "count"],
    )
}

fn pull(a: &Args, s: &str) -> Result<PullRef, ApiError> {
    Ok(PullRef::Line(match origin_axis(s) {
        Some(k) => LineRef::Origin(k),
        None if !s.is_empty() => LineRef::Edge(s.to_owned()),
        None => return Err(a.err("a direction is empty")),
    }))
}

/// `pattern`.
pub fn pattern(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    let features = a.strs("features")?;
    let count = a.opt_count("count")?.unwrap_or(2);
    if count < 2 {
        return Err(a.err("'count' must be at least 2"));
    }
    let mut def = if a.choice("kind", &["rectangular", "circular"], "rectangular")? == "circular" {
        let axis = origin_axis(a.opt_str("axis")?.unwrap_or("Z"))
            .ok_or_else(|| a.err("'axis' must be X, Y or Z"))?;
        let mut d = PatternFeature::circular();
        if let PatternKind::Circular {
            axis: ax,
            count: n,
            angle,
            ..
        } = &mut d.kind
        {
            *ax = Some(LineRef::Origin(axis));
            *n = count;
            *angle = a.opt_num("angle")?.unwrap_or(360.0).to_radians();
        }
        d
    } else {
        let mut d = PatternFeature::rectangular();
        if let PatternKind::Rectangular { first, second } = &mut d.kind {
            first.dir = Some(pull(a, a.opt_str("direction")?.unwrap_or("X"))?);
            first.count = count;
            first.spacing = a.num("spacing")?;
            if let Some(n2) = a.opt_count("count2")? {
                *second = Some(PatternDirection {
                    dir: Some(pull(a, a.opt_str("direction2")?.unwrap_or("Y"))?),
                    count: n2,
                    spacing: a.num("spacing2")?,
                    ..PatternDirection::new()
                });
            }
        }
        d
    };
    let part = part_mut(s, a)?;
    for f in &features {
        if !part.features.iter().any(|x| x.id() == f) {
            return Err(a.err(format!("no feature '{f}' (see features)")));
        }
    }
    def.features = features;
    let id = part.add_pattern(def).map_err(|e| perr(a, e))?;
    part.sync_parameters();
    Ok(feature_answer(part, &id))
}

// ---- parameters ----------------------------------------------------------------

/// `parameters`.
pub fn parameters(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    let part = part_ref(s, a)?;
    let rows: Vec<Value> = part
        .parameter_table()
        .iter()
        .map(|r| {
            let mut v = json!({"name": r.name, "value": round(r.unit.to_display(r.value)), "unit": r.unit.label(), "of": r.consumed_by});
            if let Some(e) = &r.expr {
                v["expression"] = json!(e);
            }
            v
        })
        .collect();
    Ok(json!({"parameters": rows}))
}

/// `parameter_set`.
pub fn parameter_set(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    let name = a.str("name")?;
    let text = a.str("expression")?;
    let part = part_mut(s, a)?;
    let before: Vec<String> = part.feature_health.keys().cloned().collect();
    part.set_parameter_expr(name, text)
        .map_err(|e| perr(a, e))?;
    part.regenerate().map_err(|e| perr(a, e))?;
    let new: Vec<String> = part
        .feature_health
        .iter()
        .filter(|(k, _)| !before.contains(k))
        .map(|(k, v)| format!("{k}: {v}"))
        .collect();
    if !new.is_empty() {
        return Err(a.err(format!("{name} = {text} breaks {}", new.join("; "))));
    }
    let row = part.parameter(name);
    Ok(json!({"name": name, "value": row.map(|r| round(r.unit.to_display(r.value)))}))
}

// ---- queries -------------------------------------------------------------------

fn feature_type(f: &Feature) -> String {
    if let Feature::Blend(b) = f {
        return b.command().to_owned(); // Fillet / Chamfer, as the browser says
    }
    serde_json::to_value(f)
        .ok()
        .and_then(|v| v.get("type").and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_default()
}

/// `features`: the feature tree, compact.
pub fn features(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    let part = part_ref(s, a)?;
    Ok(tree(part))
}

/// The feature tree of `part` (also the `features` resource).
pub fn tree(part: &PartDocument) -> Value {
    let features: Vec<Value> = part
        .features
        .iter()
        .enumerate()
        .map(|(k, f)| {
            let mut v = json!({"id": f.id(), "type": feature_type(f)});
            if f.name() != f.id() {
                v["name"] = json!(f.name());
            }
            if let Some(m) = part.feature_health.get(f.id()) {
                v["health"] = json!(m);
            }
            if part.is_suppressed(k) {
                v["suppressed"] = json!(true);
            }
            v
        })
        .collect();
    let sketches: Vec<Value> = part
        .sketches
        .iter()
        .map(|ps| {
            let dof = ps.sketch.clone().solve().map(|r| r.dof).ok();
            json!({"id": ps.id, "dof": dof, "used_by": part.consumers(&ps.id).iter().filter_map(|k| part.features.get(*k).map(|f| f.id().to_owned())).collect::<Vec<_>>()})
        })
        .collect();
    json!({
        "features": features,
        "sketches": sketches,
        "solids": part.body.solids().filter(|(_, s)| !s.open).count(),
        "exact": part.exact,
    })
}

/// `faces`.
pub fn faces(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    let filter = a.opt_str("feature")?;
    let detail = a.flag("detail")?;
    let part = part_ref(s, a)?;
    let list: Vec<Value> = names::faces(&part.body)
        .iter()
        .filter(|f| {
            filter.is_none_or(|p| {
                f.name == p
                    || f.name.starts_with(&format!("{p}:"))
                    || f.name.starts_with(&format!("{p}#"))
            })
        })
        .map(|f| if detail { f.detail() } else { f.row() })
        .collect();
    let mut v = json!({"faces": list});
    if !detail {
        v["columns"] = json!(["face", "surface", "alias", "dir"]);
    }
    Ok(v)
}

/// Schema of `face_at`.
pub fn face_at_schema() -> Value {
    obj(
        json!({"part": part_prop(),
               "point": {"type": "array", "items": {"type": "number"}, "description": "[x, y, z] mm"},
               "direction": {"type": "array", "items": {"type": "number"}, "description": "[dx, dy, dz]: the first face the ray from the point meets; without it the face nearest the point"}}),
        &["point"],
    )
}

/// `face_at`.
pub fn face_at(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    let o = point3(a, a.get("point").unwrap_or(&Value::Null), "point")?;
    let d = match a.get("direction") {
        Some(v) => Some(point3(a, v, "direction")?.coords),
        None => None,
    };
    let part = part_ref(s, a)?;
    let hit = names::face_at(&part.body, &o, d.as_ref()).ok_or_else(|| {
        a.err(if d.is_some() {
            "the ray meets no face"
        } else {
            "the part has no faces"
        })
    })?;
    let info = names::faces(&part.body)
        .into_iter()
        .find(|f| f.name == hit.name);
    let mut v = info
        .map(|f| f.detail())
        .unwrap_or_else(|| json!({"name": hit.name}));
    v["point"] = json!(p3(&hit.point));
    v["distance"] = json!(round(hit.distance));
    Ok(v)
}

fn polyline_length(pts: &[Point3]) -> Real {
    pts.windows(2).map(|w| (w[1] - w[0]).norm()).sum()
}

/// `edges`.
pub fn edges(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    let face = a.opt_str("face")?;
    let feature = a.opt_str("feature")?;
    let detail = a.flag("detail")?;
    let part = part_ref(s, a)?;
    let body = &part.body;
    let face = match face {
        Some(f) => Some(face_name(body, a, f)?),
        None => None,
    };
    let alias: BTreeMap<String, String> = names::faces(body)
        .into_iter()
        .filter_map(|f| f.alias().map(str::to_owned).map(|x| (f.name, x)))
        .collect();
    let opts = TessellationOptions::default();
    let list: Vec<Value> = edge_groups(body)
        .into_iter()
        .filter(|(_, g)| {
            g.first().is_some_and(|(_, r)| {
                face.as_ref().is_none_or(|f| r.faces.iter().any(|x| x == f))
                    && feature
                        .is_none_or(|p| r.faces.iter().any(|x| x.starts_with(&format!("{p}:"))))
            })
        })
        .map(|(key, g)| {
            let straight = g.iter().all(|(e, _)| body.edge_direction(*e).is_some());
            let samples: Vec<Vec<Point3>> = g
                .iter()
                .filter_map(|(e, _)| body.sample_edge(*e, &opts).ok())
                .collect();
            let length: Real = samples.iter().map(|p| polyline_length(p)).sum();
            let kind = if straight {
                "line"
            } else if g.iter().any(|(e, _)| {
                body.edge(*e)
                    .and_then(|x| body.curve(x.curve))
                    .is_some_and(|c| matches!(c, crate::kernel::Curve::Circle(_)))
            }) {
                "circle"
            } else {
                "curve"
            };
            let ali = key
                .split_once('|')
                .and_then(|(x, y)| Some(format!("{}|{}", alias.get(x)?, alias.get(y)?)));
            if !detail {
                return json!([key, kind, round(length), ali]);
            }
            let mut v = json!({"edge": key, "kind": kind, "length": round(length), "alias": ali});
            if let Some(m) = samples
                .first()
                .and_then(|p| crate::part::select::polyline_middle(p))
            {
                v["middle"] = json!(p3(&m));
            }
            if let Some(d) = g
                .first()
                .and_then(|(e, _)| body.edge_direction(*e))
                .and_then(|d| d.try_normalize(1e-12))
            {
                v["direction"] = json!([round(d.x), round(d.y), round(d.z)]);
            }
            v
        })
        .collect();
    let mut v = json!({"edges": list});
    if !detail {
        v["columns"] = json!(["edge", "kind", "length", "alias"]);
    }
    Ok(v)
}

fn target(body: &BRep, a: &Args, v: &Value) -> Result<crate::part::measure::Target, ApiError> {
    use crate::part::measure::Target;
    if v.is_array() {
        return Ok(Target::Point(point3(a, v, "point")?));
    }
    let s = v.as_str().ok_or_else(|| {
        a.err("an item is 'face:<name>', 'edge:<A|B>', 'vertex:<name>', 'body:<name>' or [x, y, z]")
    })?;
    let (kind, name) = s.split_once(':').unwrap_or(("", s));
    // Face names hold ':' too (Extrusion1:End): only a known prefix is taken off.
    Ok(match kind {
        "face" => Target::Face(face_name(body, a, name)?),
        "vertex" => Target::Vertex(name.to_owned()),
        "body" => Target::Body(name.to_owned()),
        "edge" => Target::Edge(edges_named(body, a, &[name.to_owned()])?[0].0),
        _ => Target::Face(face_name(body, a, s)?),
    })
}

/// `measure`.
pub fn measure(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    let part = part_ref(s, a)?;
    let mut targets = vec![target(&part.body, a, a.get("a").unwrap_or(&Value::Null))?];
    if let Some(b) = a.get("b") {
        targets.push(target(&part.body, a, b)?);
    }
    let m = crate::part::measure::measure(&part.body, &targets).map_err(|e| a.err(e))?;
    let items: Vec<Value> = m
        .items
        .iter()
        .map(|i| {
            let mut v = json!({"kind": i.kind});
            let mut put = |k: &str, x: Option<Real>| {
                if let Some(x) = x {
                    v[k] = json!(round(x));
                }
            };
            put("length", i.length);
            put("area", i.area);
            put("volume", i.volume);
            put("radius", i.radius);
            put("arc_degrees", i.arc.map(Real::to_degrees));
            if let Some(p) = &i.position {
                v["position"] = json!(p3(p));
            }
            v
        })
        .collect();
    let mut out = json!({"items": items});
    if let Some((d, p, q)) = &m.distance {
        out["distance"] = json!(round(*d));
        out["closest"] = json!([p3(p), p3(q)]);
    }
    if let Some(c) = m.center_distance {
        out["center_distance"] = json!(round(c));
    }
    if let Some(g) = m.angle_deg {
        out["angle_degrees"] = json!(round(g));
    }
    Ok(out)
}

/// `physical`.
pub fn physical(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    let part = part_ref(s, a)?;
    let p = crate::part::physical::physical_properties(part, Default::default())
        .map_err(|e| a.err(e))?;
    let t = &p.total;
    let bx = part.body.bounding_box();
    Ok(json!({
        "volume": round(t.volume),
        "area": round(t.area),
        "mass_g": round(t.mass),
        "center": p3(&t.center),
        "box": bx.map(|(lo, hi)| json!([p3(&lo), p3(&hi)])),
        "material": part.material,
        "bodies": p.bodies.len(),
        "warnings": p.warnings,
    }))
}

/// `export`.
pub fn export(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    use crate::part::export::{ExportFormat, ExportMesh, glb_bytes, step_text, stl_bytes};
    let path = super::tools::checked_path(a, a.str("path")?)?;
    let doc = s.doc_id(a.cmd, a.opt_str("part")?)?;
    let name = s
        .project
        .document(doc)
        .map(|d| d.name.clone())
        .unwrap_or_default();
    let part = part_ref(s, a)?;
    let format = match a.opt_str("format")? {
        Some(f) => match f.to_ascii_lowercase().as_str() {
            "step" | "stp" => ExportFormat::Step,
            "stl" => ExportFormat::Stl,
            "glb" => ExportFormat::Glb,
            _ => return Err(a.err("'format' must be step, stl or glb")),
        },
        None => ExportFormat::from_path(&path)
            .ok_or_else(|| a.err("give 'format' or a .step / .stl / .glb path"))?,
    };
    let bytes = match format {
        ExportFormat::Step => step_text(part, &name).map_err(|e| a.err(e))?.into_bytes(),
        ExportFormat::Stl => stl_bytes(&ExportMesh::of(part).map_err(|e| a.err(e))?, &name),
        ExportFormat::Glb => glb_bytes(&ExportMesh::of(part).map_err(|e| a.err(e))?, &name),
    };
    std::fs::write(&path, &bytes).map_err(|e| a.err(format!("{}: {e}", path.display())))?;
    Ok(json!({"path": path.display().to_string(), "bytes": bytes.len()}))
}

/// The point of `p` in the sketch (for tests and the GUI).
pub fn sketch_point_at(sk: &Sketch, h: &str) -> Option<Point2> {
    find_point(sk, h)
        .and_then(|p| sk.point(p))
        .map(|p| p.position)
}
