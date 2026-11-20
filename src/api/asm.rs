//! Assembly tools: place components, add relationships, solve (spec 0.7 §3.5).

use serde_json::{Value, json};

use super::args::{Args, p3, point3, round};
use super::part::face_point;
use super::session::{ApiError, Session};
use super::tools::obj;
use crate::assembly::mechanisms::solve_in;
use crate::assembly::relation::{Constraint, ConstraintKind, GeomRef};
use crate::assembly::solve::Solved;
use crate::document::{AssemblyDocument, DocRef, DocumentData};
use crate::kernel::{Point3, Transform3};

fn asm_prop() -> Value {
    json!({"type": "string", "description": "Assembly document (default: the active document)"})
}

/// Schema of `place`.
pub fn place_schema() -> Value {
    obj(
        json!({"assembly": asm_prop(),
               "component": {"type": "string", "description": "A part document of this project"},
               "at": {"type": "array", "items": {"type": "number"}, "description": "[x, y, z] mm (default origin)"},
               "grounded": {"type": "boolean"}}),
        &["component"],
    )
}

/// `place`.
pub fn place(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    let comp = a.str("component")?;
    let cid = s.doc_id(a.cmd, Some(comp))?;
    let base = match s.project.document(cid) {
        Some(d) if matches!(d.data, DocumentData::Part(_)) => d.name.clone(),
        _ => return Err(a.err(format!("'{comp}' is not a part"))),
    };
    let at = match a.get("at") {
        Some(v) => point3(a, v, "at")?,
        None => Point3::origin(),
    };
    let asm = s.assembly_mut(a.cmd, a.opt_str("assembly")?)?;
    let grounded = match a.get("grounded") {
        Some(_) => a.flag("grounded")?,
        None => asm.occurrences.is_empty(),
    };
    let id = asm.place(
        DocRef::local(cid),
        &base,
        Transform3::translation(at.x, at.y, at.z),
        grounded,
    );
    let name = asm.find(id).map(|o| o.name.clone()).unwrap_or_default();
    Ok(json!({"occurrence": name, "id": id, "grounded": grounded}))
}

fn side_prop() -> Value {
    json!({"type": "object", "properties": {
        "occurrence": {"type": "string", "description": "Occurrence name (Plate:1)"},
        "face": {"type": "string"}, "edge": {"type": "string", "description": "'FaceA|FaceB' of a circular edge"}},
        "required": ["occurrence"]})
}

/// Schema of `constrain`.
pub fn constrain_schema() -> Value {
    obj(
        json!({"assembly": asm_prop(),
               "kind": {"type": "string", "enum": ["mate", "flush", "insert", "angle"]},
               "a": side_prop(), "b": side_prop(),
               "offset": {"type": "number", "description": "mm (mate, flush, insert); degrees for angle"}}),
        &["kind", "a", "b"],
    )
}

/// One side of a relationship: an occurrence's face or edge, in its part's coordinates.
fn side(s: &Session, asm: &AssemblyDocument, a: &Args, key: &str) -> Result<GeomRef, ApiError> {
    let v = a
        .get(key)
        .ok_or_else(|| a.err(format!("'{key}' is required")))?;
    let side = Args::new(a.cmd, v);
    let occ = side.str("occurrence")?;
    let o = asm
        .occurrences
        .iter()
        .find(|o| o.name.eq_ignore_ascii_case(occ))
        .ok_or_else(|| a.err(format!("no occurrence '{occ}' (see components)")))?;
    let part = match s.project.document(o.component.document).map(|d| &d.data) {
        Some(DocumentData::Part(p)) => p,
        _ => return Err(a.err(format!("{occ} is not a part of this project"))),
    };
    if let Some(face) = side.opt_str("face")? {
        let face = &super::names::resolve_face(&part.body, face)
            .ok_or_else(|| a.err(format!("{occ} has no face '{face}'")))?;
        let p = face_point(&part.body, face)
            .ok_or_else(|| a.err(format!("{occ} has no face '{face}'")))?;
        return Ok(GeomRef::Face {
            occurrence: o.id,
            sub: Vec::new(),
            name: face.to_owned(),
            hint: [p.x, p.y, p.z],
        });
    }
    let edge = side
        .opt_str("edge")?
        .ok_or_else(|| a.err(format!("'{key}' needs 'face' or 'edge'")))?;
    let edge = &super::names::resolve_edge(&part.body, edge);
    let (x, y) = edge
        .split_once('|')
        .ok_or_else(|| a.err("an edge is 'FaceA|FaceB'"))?;
    let want = |r: &crate::part::blend::EdgeRef| {
        (r.faces[0] == x && r.faces[1] == y) || (r.faces[0] == y && r.faces[1] == x)
    };
    let r = part
        .body
        .edges()
        .filter_map(|(e, _)| crate::part::blend::edge_ref(&part.body, e))
        .find(want)
        .ok_or_else(|| a.err(format!("{occ} has no edge '{edge}'")))?;
    Ok(GeomRef::Edge {
        occurrence: o.id,
        sub: Vec::new(),
        edge: r,
    })
}

fn solved_answer(asm: &AssemblyDocument, r: &Solved) -> Value {
    let name = |id: u32| asm.find(id).map(|o| o.name.clone()).unwrap_or_default();
    let mut dof: Vec<(String, usize)> = r.dof.iter().map(|(k, v)| (name(*k), *v)).collect();
    dof.sort();
    json!({
        "converged": r.converged,
        "residual": r.residual,
        "dof": dof.into_iter().map(|(n, d)| json!([n, d])).collect::<Vec<_>>(),
        "sick": r.sick.iter().map(|(k, m)| format!("{}: {m}", asm.constraints.iter().find(|c| c.id == *k).map(|c| c.name.clone()).unwrap_or_default())).collect::<Vec<_>>(),
    })
}

/// Solves `asm` (a copy) against the project and writes the places into it.
fn solve_copy(s: &Session, asm: &mut AssemblyDocument) -> Solved {
    let r = solve_in(&s.project, asm, None);
    crate::assembly::flexible::apply(&r, asm);
    r
}

/// `constrain`.
pub fn constrain(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    let kind = match a.choice("kind", &["mate", "flush", "insert", "angle"], "mate")? {
        "flush" => ConstraintKind::Flush,
        "insert" => ConstraintKind::Insert,
        "angle" => ConstraintKind::Angle,
        _ => ConstraintKind::Mate,
    };
    let id = s.doc_id(a.cmd, a.opt_str("assembly")?)?;
    let mut asm = s.assembly(a.cmd, a.opt_str("assembly")?)?.clone();
    let ga = side(s, &asm, a, "a")?;
    let gb = side(s, &asm, a, "b")?;
    let mut c = Constraint::new(kind, ga, gb);
    let off = a.opt_num("offset")?.unwrap_or(0.0);
    c.value = if kind == ConstraintKind::Angle {
        off.to_radians()
    } else {
        off
    };
    let cid = asm.add_constraint(c);
    let r = solve_copy(s, &mut asm);
    if !r.converged {
        return Err(a.err(format!(
            "the assembly does not solve with it (residual {:.3e}{})",
            r.residual,
            if r.sick.is_empty() {
                String::new()
            } else {
                format!(
                    "; {}",
                    r.sick
                        .iter()
                        .map(|(_, m)| m.clone())
                        .collect::<Vec<_>>()
                        .join("; ")
                )
            }
        )));
    }
    let name = asm
        .constraints
        .iter()
        .find(|c| c.id == cid)
        .map(|c| c.name.clone())
        .unwrap_or_default();
    let mut out = solved_answer(&asm, &r);
    out["relationship"] = json!(name);
    if let Some(DocumentData::Assembly(dst)) = s.project.document_mut(id).map(|d| &mut d.data) {
        *dst = asm;
    }
    Ok(out)
}

/// `assembly_solve`.
pub fn solve(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    let id = s.doc_id(a.cmd, a.opt_str("assembly")?)?;
    let mut asm = s.assembly(a.cmd, a.opt_str("assembly")?)?.clone();
    let r = solve_copy(s, &mut asm);
    let out = solved_answer(&asm, &r);
    if r.converged
        && let Some(DocumentData::Assembly(dst)) = s.project.document_mut(id).map(|d| &mut d.data)
    {
        *dst = asm;
    }
    Ok(out)
}

/// `components`.
pub fn components(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    let asm = s.assembly(a.cmd, a.opt_str("assembly")?)?;
    let doc_name = |o: &crate::document::Occurrence| {
        s.project
            .document(o.component.document)
            .map(|d| d.name.clone())
            .unwrap_or_default()
    };
    let occ: Vec<Value> = asm
        .occurrences
        .iter()
        .map(|o| {
            let t = o.transform.translation.vector;
            let (r, p, y) = o.transform.rotation.euler_angles();
            let mut v = json!({"name": o.name, "part": doc_name(o), "at": p3(&Point3::from(t)),
                "rotation_deg": [round(r.to_degrees()), round(p.to_degrees()), round(y.to_degrees())]});
            if o.grounded {
                v["grounded"] = json!(true);
            }
            v
        })
        .collect();
    let rel: Vec<Value> = asm
        .constraints
        .iter()
        .map(|c| json!({"name": c.name, "kind": c.kind.label(), "value": round(c.value)}))
        .collect();
    Ok(json!({"components": occ, "relationships": rel}))
}
