//! More part tools (spec 0.7 §3.3): mirror, work planes / axes / points, sweep, loft, and the
//! Component Library (§3.5). Like the others they only fill a feature definition and call the
//! part's own `add_*`.

use serde_json::{Value, json};

use super::args::{Args, p3, point3, round};
use super::part::{
    edges_named, face_name, feature_answer, find_curve, find_point, operation, origin_axis,
    part_mut, perr, profiles,
};
use super::session::{ApiError, Session};
use super::tools::{obj, part_prop};
use crate::part::loft::{LoftFeature, LoftSection};
use crate::part::pattern::{PatternFeature, PatternKind};
use crate::part::sweep::{PathRef, SweepFeature};
use crate::part::work::{LineRef, WorkAxisDef, WorkDef, WorkFeature, WorkPlaneDef, WorkPointDef};
use crate::part::{OriginPlane, PartDocument, PlaneRef, PointRef};

fn plane_prop() -> Value {
    json!({"type": "string", "description": "XY, XZ, YZ, a work plane (WorkPlane1) or a planar face (name or alias, e.g. Top)"})
}

fn line_prop() -> Value {
    json!({"type": "string", "description": "X, Y, Z, a work axis (WorkAxis1) or a straight edge 'FaceA|FaceB'"})
}

/// A plane argument: an origin plane, a work plane id or a face (name or alias).
pub fn plane_ref(part: &PartDocument, a: &Args, s: &str) -> Result<PlaneRef, ApiError> {
    Ok(match s.to_ascii_uppercase().as_str() {
        "XY" => PlaneRef::Origin(OriginPlane::XY),
        "XZ" => PlaneRef::Origin(OriginPlane::XZ),
        "YZ" => PlaneRef::Origin(OriginPlane::YZ),
        _ if part.work.planes.contains_key(s) => PlaneRef::Work(s.to_owned()),
        _ => PlaneRef::Face(face_name(&part.body, a, s)?),
    })
}

/// An edge's own persistent name from `FaceA|FaceB` (or the name itself).
fn edge_name(part: &PartDocument, a: &Args, s: &str) -> Result<String, ApiError> {
    let (e, _) = edges_named(&part.body, a, &[s.to_owned()])?[0].clone();
    part.body
        .edge(e)
        .and_then(|x| x.name.clone())
        .ok_or_else(|| a.err(format!("the edge '{s}' has no persistent name")))
}

/// A line argument: an origin axis, a work axis id or a straight edge.
fn line_ref(part: &PartDocument, a: &Args, s: &str) -> Result<LineRef, ApiError> {
    Ok(match origin_axis(s) {
        Some(k) => LineRef::Origin(k),
        None if part.work.axes.contains_key(s) => LineRef::Work(s.to_owned()),
        None => LineRef::Edge(edge_name(part, a, s)?),
    })
}

/// A point argument: `origin` or a work point id.
fn point_ref(part: &PartDocument, a: &Args, s: &str) -> Result<PointRef, ApiError> {
    if s.eq_ignore_ascii_case("origin") {
        return Ok(PointRef::PartOrigin);
    }
    if part.work.points.contains_key(s) {
        return Ok(PointRef::Work(s.to_owned()));
    }
    Err(a.err(format!(
        "no point '{s}': give 'origin' or a work point (work_point)"
    )))
}

fn need<'a>(a: &'a Args, key: &str) -> Result<&'a str, ApiError> {
    a.str(key)
}

// ---- mirror ----------------------------------------------------------------

/// Schema of `mirror`.
pub fn mirror_schema() -> Value {
    obj(
        json!({"part": part_prop(),
               "features": {"type": "array", "items": {"type": "string"}, "description": "Feature ids to mirror"},
               "plane": plane_prop()}),
        &["features", "plane"],
    )
}

/// `mirror`.
pub fn mirror(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    let features = a.strs("features")?;
    let part = part_mut(s, a)?;
    let plane = plane_ref(part, a, need(a, "plane")?)?;
    for f in &features {
        if !part.features.iter().any(|x| x.id() == f) {
            return Err(a.err(format!("no feature '{f}' (see features)")));
        }
    }
    let mut def = PatternFeature::mirror();
    def.kind = PatternKind::Mirror { plane: Some(plane) };
    def.features = features;
    let id = part.add_pattern(def).map_err(|e| perr(a, e))?;
    part.sync_parameters();
    Ok(feature_answer(part, &id))
}

// ---- work features ---------------------------------------------------------

fn work_answer(part: &PartDocument, id: &str) -> Value {
    let mut v = feature_answer(part, id);
    if let Some(p) = part.work.planes.get(id) {
        let o = p.point_at(0.0, 0.0);
        v["origin"] = json!(p3(&o));
        v["normal"] = json!([round(p.normal.x), round(p.normal.y), round(p.normal.z)]);
    }
    if let Some((o, d)) = part.work.axes.get(id) {
        v["point"] = json!(p3(o));
        v["direction"] = json!([round(d.x), round(d.y), round(d.z)]);
    }
    if let Some(p) = part.work.points.get(id) {
        v["point"] = json!(p3(p));
    }
    v
}

fn add_work(
    s: &mut Session,
    a: &Args,
    def: impl FnOnce(&PartDocument) -> Result<WorkDef, ApiError>,
) -> Result<Value, ApiError> {
    let part = part_mut(s, a)?;
    let def = def(part)?;
    let id = part
        .add_work(WorkFeature::new(def))
        .map_err(|e| perr(a, e))?;
    if let Some(m) = part.feature_health.get(&id) {
        return Err(a.err(m.clone()));
    }
    part.sync_parameters();
    Ok(work_answer(part, &id))
}

/// Schema of `work_plane`.
pub fn work_plane_schema() -> Value {
    obj(
        json!({"part": part_prop(),
               "method": {"type": "string", "enum": ["offset", "midplane", "angle"], "description": "offset (default): parallel to 'plane' at 'distance'; midplane: between 'plane' and 'plane2'; angle: through 'axis' turned 'angle' degrees from 'plane'"},
               "plane": plane_prop(), "plane2": plane_prop(), "axis": line_prop(),
               "distance": {"type": "number", "description": "mm along the plane's normal (offset)"},
               "angle": {"type": "number", "description": "degrees (angle)"}}),
        &["plane"],
    )
}

/// `work_plane`.
pub fn work_plane(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    let method = a.choice("method", &["offset", "midplane", "angle"], "offset")?;
    add_work(s, a, |part| {
        let base = plane_ref(part, a, need(a, "plane")?)?;
        Ok(WorkDef::Plane(match method {
            "midplane" => WorkPlaneDef::Midplane {
                a: base,
                b: plane_ref(part, a, need(a, "plane2")?)?,
            },
            "angle" => WorkPlaneDef::Angle {
                base,
                axis: line_ref(part, a, need(a, "axis")?)?,
                angle: a.num("angle")?.to_radians(),
            },
            _ => WorkPlaneDef::Offset {
                base,
                distance: a.num("distance")?,
            },
        }))
    })
}

/// Schema of `work_axis`.
pub fn work_axis_schema() -> Value {
    obj(
        json!({"part": part_prop(),
               "method": {"type": "string", "enum": ["planes", "circle", "normal"], "description": "planes: where 'plane' and 'plane2' meet; circle: through the centre of the circular 'edge'; normal: square to 'plane' through 'point'"},
               "plane": plane_prop(), "plane2": plane_prop(),
               "edge": {"type": "string", "description": "A circular edge 'FaceA|FaceB'"},
               "point": {"type": "string", "description": "origin or a work point id"}}),
        &["method"],
    )
}

/// `work_axis`.
pub fn work_axis(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    let method = a.choice("method", &["planes", "circle", "normal"], "planes")?;
    add_work(s, a, |part| {
        Ok(WorkDef::Axis(match method {
            "circle" => WorkAxisDef::CircularEdge {
                edge: edge_name(part, a, need(a, "edge")?)?,
            },
            "normal" => WorkAxisDef::NormalToPlane {
                plane: plane_ref(part, a, need(a, "plane")?)?,
                point: point_ref(part, a, need(a, "point")?)?,
            },
            _ => WorkAxisDef::TwoPlanes {
                a: plane_ref(part, a, need(a, "plane")?)?,
                b: plane_ref(part, a, need(a, "plane2")?)?,
            },
        }))
    })
}

/// Schema of `work_point`.
pub fn work_point_schema() -> Value {
    obj(
        json!({"part": part_prop(),
               "method": {"type": "string", "enum": ["fixed", "planes", "plane_line"], "description": "fixed (default): at 'at'; planes: where three 'planes' meet; plane_line: where 'line' pierces 'plane'"},
               "at": {"type": "array", "items": {"type": "number"}, "description": "[x, y, z] mm (fixed)"},
               "planes": {"type": "array", "items": {"type": "string"}, "description": "three planes"},
               "plane": plane_prop(), "line": line_prop()}),
        &[],
    )
}

/// `work_point`.
pub fn work_point(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    let method = a.choice("method", &["fixed", "planes", "plane_line"], "fixed")?;
    add_work(s, a, |part| {
        Ok(WorkDef::Point(match method {
            "planes" => {
                let names = a.strs("planes")?;
                let [p, q, r] = names.as_slice() else {
                    return Err(a.err("'planes' must list three planes"));
                };
                WorkPointDef::ThreePlanes {
                    planes: [
                        plane_ref(part, a, p)?,
                        plane_ref(part, a, q)?,
                        plane_ref(part, a, r)?,
                    ],
                }
            }
            "plane_line" => WorkPointDef::PlaneLine {
                plane: plane_ref(part, a, need(a, "plane")?)?,
                line: line_ref(part, a, need(a, "line")?)?,
            },
            _ => {
                let p = point3(a, a.get("at").unwrap_or(&Value::Null), "at")?;
                WorkPointDef::Fixed {
                    x: p.x,
                    y: p.y,
                    z: p.z,
                }
            }
        }))
    })
}

// ---- sweep / loft ----------------------------------------------------------

fn sketch_named(part: &PartDocument, a: &Args, id: &str) -> Result<usize, ApiError> {
    part.sketches
        .iter()
        .position(|ps| ps.id == id || ps.name == id)
        .ok_or_else(|| a.err(format!("no sketch '{id}'")))
}

/// Schema of `sweep`.
pub fn sweep_schema() -> Value {
    obj(
        json!({"part": part_prop(),
               "sketch": {"type": "string", "description": "The profile sketch"},
               "profiles": {"type": "array", "items": {"type": "integer"}, "description": "Region indices (default: the sketch's own closed regions)"},
               "path_sketch": {"type": "string", "description": "The sketch holding the path curves"},
               "path_curves": {"type": "array", "items": {"type": "string"}, "description": "Curve handles of the path (e3, e4...; chained in any order)"},
               "path_edges": {"type": "array", "items": {"type": "string"}, "description": "Model edges 'FaceA|FaceB' as the path instead"},
               "operation": {"type": "string", "enum": ["join", "cut", "intersect", "new_solid"]}}),
        &["sketch"],
    )
}

/// `sweep`.
pub fn sweep(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    let part = part_mut(s, a)?;
    let i = sketch_named(part, a, a.str("sketch")?)?;
    let regions = profiles(part, i, a)?;
    let path = if let Some(ps) = a.opt_str("path_sketch")? {
        let k = sketch_named(part, a, ps)?;
        let sk = &part.sketches[k].sketch;
        let curves = a
            .strs("path_curves")?
            .iter()
            .map(|h| find_curve(sk, h).ok_or_else(|| a.err(format!("no curve '{h}' in {ps}"))))
            .collect::<Result<Vec<_>, _>>()?;
        if curves.is_empty() {
            return Err(a.err("give 'path_curves' of the path sketch"));
        }
        PathRef::Sketch {
            sketch: part.sketches[k].id.clone(),
            curves,
        }
    } else {
        let names = a.strs("path_edges")?;
        if names.is_empty() {
            return Err(a.err("give a path: path_sketch + path_curves, or path_edges"));
        }
        PathRef::Edges(
            edges_named(&part.body, a, &names)?
                .into_iter()
                .map(|(_, r)| r)
                .collect(),
        )
    };
    let mut def = SweepFeature::new(regions, path);
    def.operation = operation(a, part)?;
    let id = part.add_sweep(i, def).map_err(|e| perr(a, e))?;
    part.sync_parameters();
    Ok(feature_answer(part, &id))
}

/// Schema of `loft`.
pub fn loft_schema() -> Value {
    obj(
        json!({"part": part_prop(),
               "sections": {"type": "array", "description": "In order: {sketch, region} (a closed region, index from sketch_info, default 0) or {sketch, point} (a point handle, first or last only)",
                   "items": {"type": "object", "properties": {"sketch": {"type": "string"}, "region": {"type": "integer"}, "point": {"type": "string"}}, "required": ["sketch"]}},
               "closed": {"type": "boolean", "description": "Closed Loop: the last section joins the first"},
               "operation": {"type": "string", "enum": ["join", "cut", "intersect", "new_solid"]}}),
        &["sections"],
    )
}

/// `loft`.
pub fn loft(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    let list = a
        .get("sections")
        .and_then(Value::as_array)
        .ok_or_else(|| a.err("'sections' must be a list"))?
        .clone();
    let closed = a.flag("closed")?;
    let part = part_mut(s, a)?;
    let mut sections = Vec::new();
    for (n, v) in list.iter().enumerate() {
        let sec = Args::new(a.cmd, v);
        let k = sketch_named(part, a, sec.str("sketch")?)?;
        let ps = &part.sketches[k];
        if let Some(h) = sec.opt_str("point")? {
            let point = find_point(&ps.sketch, h)
                .ok_or_else(|| a.err(format!("section {n}: no point '{h}'")))?;
            sections.push(LoftSection::Point {
                sketch: ps.id.clone(),
                point,
            });
            continue;
        }
        let regions = ps.sketch.regions();
        let r = sec.opt_count("region")?.unwrap_or(0) as usize;
        let region = regions.get(r).ok_or_else(|| {
            a.err(format!(
                "section {n}: no region {r} in {} (see sketch_info)",
                ps.id
            ))
        })?;
        sections.push(LoftSection::Profile {
            sketch: ps.id.clone(),
            profile: crate::part::extrude::ProfileRef::of(region, region.interior_point()),
        });
    }
    let mut def = LoftFeature::new(sections);
    def.closed_loop = closed;
    def.operation = operation(a, part)?;
    let id = part.add_loft(def).map_err(|e| perr(a, e))?;
    part.sync_parameters();
    Ok(feature_answer(part, &id))
}

// ---- Component Library ---------------------------------------------------

fn catalog(a: &Args) -> Result<crate::components::catalog::Catalog, ApiError> {
    let root = crate::components::root_dir().ok_or_else(|| a.err("no library folder"))?;
    Ok(crate::components::catalog::Catalog::load(&root))
}

/// Schema of `library`.
pub fn library_schema() -> Value {
    obj(
        json!({"search": {"type": "string", "description": "Words of the family's name or category (e.g. hex bolt, bearing)"},
               "family": {"type": "string", "description": "A family name: list its sizes"}}),
        &[],
    )
}

/// `library`: families (category, name, standard, size count), or one family's sizes.
pub fn library(_: &mut Session, a: &Args) -> Result<Value, ApiError> {
    let c = catalog(a)?;
    if let Some(f) = a.opt_str("family")? {
        let fam = c
            .families
            .iter()
            .find(|x| x.name.eq_ignore_ascii_case(f))
            .ok_or_else(|| a.err(format!("no family '{f}' (see library)")))?;
        return Ok(
            json!({"family": fam.name, "category": fam.category, "standard": fam.standard,
            "sizes": fam.members.iter().map(|m| m.size.clone()).collect::<Vec<_>>()}),
        );
    }
    let words: Vec<String> = a
        .opt_str("search")?
        .unwrap_or("")
        .split_whitespace()
        .map(str::to_lowercase)
        .collect();
    let list: Vec<Value> = c
        .families
        .iter()
        .filter(|f| {
            let text = format!("{} {}", f.category, f.name).to_lowercase();
            words.iter().all(|w| text.contains(w))
        })
        .map(|f| json!([f.category, f.name, f.standard, f.members.len()]))
        .collect();
    Ok(json!({"families": list, "columns": ["category", "family", "standard", "sizes"]}))
}

/// Schema of `library_place`.
pub fn library_place_schema() -> Value {
    obj(
        json!({"assembly": {"type": "string", "description": "Assembly document (default: the active one)"},
               "family": {"type": "string"}, "size": {"type": "string", "description": "A size of the family (see library {family})"},
               "at": {"type": "array", "items": {"type": "number"}, "description": "[x, y, z] mm (default origin)"},
               "grounded": {"type": "boolean"}}),
        &["family", "size"],
    )
}

/// `library_place`: the member as a part document of this project (reused when it is there
/// already), placed in the assembly.
pub fn library_place(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    let c = catalog(a)?;
    let fname = a.str("family")?;
    let size = a.str("size")?;
    let (fi, fam) = c
        .families
        .iter()
        .enumerate()
        .find(|(_, x)| x.name.eq_ignore_ascii_case(fname))
        .ok_or_else(|| a.err(format!("no family '{fname}' (see library)")))?;
    let mi = fam
        .members
        .iter()
        .position(|m| m.size.eq_ignore_ascii_case(size))
        .ok_or_else(|| {
            a.err(format!(
                "{} has no size '{size}' (see library {{family}})",
                fam.name
            ))
        })?;
    let doc_name = format!("{} {}", fam.name, fam.members[mi].size);
    s.assembly(a.cmd, a.opt_str("assembly")?)?;
    if !s.project.name_taken(&doc_name, None) {
        let file = c.member_file(fi, mi).map_err(|e| a.err(e))?;
        let lib = crate::document::Project::load(&file).map_err(|e| a.err(e.to_string()))?;
        let mut part = lib
            .documents()
            .iter()
            .find_map(|d| match &d.data {
                crate::document::DocumentData::Part(p) => Some(p.clone()),
                _ => None,
            })
            .ok_or_else(|| a.err("the library member holds no part"))?;
        part.regenerate().map_err(|e| perr(a, e))?;
        let active = s.active;
        s.project
            .add_document(&doc_name, crate::document::DocumentData::Part(part));
        s.active = active;
    }
    let mut args = json!({"component": doc_name});
    for k in ["assembly", "at", "grounded"] {
        if let Some(v) = a.get(k) {
            args[k] = v.clone();
        }
    }
    let mut v = super::asm::place(s, &Args::new(a.cmd, &args))?;
    v["part"] = json!(doc_name);
    Ok(v)
}
