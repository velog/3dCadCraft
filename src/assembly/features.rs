//! Assembly features (spec 3.10 §3.8, T7): cuts that exist only in the
//! assembly — a drilled hole through two plates, a shelf-pin row through
//! both sides of a cabinet — made after assembly; the parts' own documents
//! are unchanged.
//!
//! The tools are the bodies of a hidden **tools part** of the design file
//! placed in the assembly at its origin (so its coordinates are the
//! assembly's): it is edited in place like any part (sketches on the other
//! components' faces, Extrude, Hole), and each of its bodies is one assembly
//! feature. A feature cuts its **participants** (occurrence paths); a
//! participant's shown body is its part's body minus the tools moved into
//! its frame ([`cut`]), computed per occurrence, never written into the part.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::tree::{self, Linked};
use crate::document::{AssemblyDocument, DocRef, DocumentData, DocumentId, Project};
use crate::kernel::exact::{self, ExactOp, ExactSolid};
use crate::kernel::{Point3, Transform3};
use crate::part::PartDocument;

/// The assembly features of an assembly.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AsmFeatures {
    /// The tools part's occurrence (hidden, grounded at the origin).
    pub occurrence: u32,
    /// Per tool body (its persistent name in the tools part): the
    /// occurrence paths it cuts.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub participants: BTreeMap<String, Vec<Vec<u32>>>,
}

/// The tool bodies of the tools part (solids only), in its (= the
/// assembly's) coordinates.
pub fn tools(tools_part: &PartDocument) -> Vec<ExactSolid> {
    tools_part
        .exact_bodies
        .0
        .iter()
        .filter(|s| !s.is_open())
        .cloned()
        .collect()
}

/// `part`'s bodies minus `tools` (assembly coordinates) for an occurrence
/// placed at `at`: a copy of the part whose body is the cut one (display,
/// picking, mass). `Ok(None)` when no tool reaches it.
pub fn cut(
    part: &PartDocument,
    at: &Transform3,
    tools: &[ExactSolid],
) -> Result<Option<PartDocument>, String> {
    if tools.is_empty() || part.exact_bodies.0.is_empty() {
        return Ok(None);
    }
    let into = at.inverse().to_homogeneous();
    let mut local: Vec<ExactSolid> = tools
        .iter()
        .map(|t| exact::similar(t, &into))
        .collect::<Result<_, _>>()?;
    // Tools apart from each other cut as one (a row of 32 pins: one
    // Boolean, not 32 — CLAUDE.md "many profiles in one cut");
    // `union_apart`'s own test meshes long thin pins coarsely and finds
    // them meeting; our boxes follow the smallest side.
    if local.len() > 1 {
        let boxes: Vec<_> = local.iter().map(exact::bounds).collect();
        let apart = boxes.iter().enumerate().all(|(i, a)| {
            boxes[i + 1..].iter().all(|b| match (a, b) {
                (Some((alo, ahi)), Some((blo, bhi))) => {
                    (0..3).any(|k| ahi[k] < blo[k] || bhi[k] < alo[k])
                }
                _ => false,
            })
        });
        let one = if apart {
            exact::join_apart(&local)?
        } else {
            exact::union_apart(&local)?
        };
        if let Some(one) = one {
            local = vec![one];
        }
    }
    let mut bodies = Vec::new();
    let mut any = false;
    for b in &part.exact_bodies.0 {
        let mut cur = b.clone();
        for t in &local {
            if !exact::meets(&cur, t) {
                continue;
            }
            match exact::boolean(&cur, t, ExactOp::Cut)? {
                Some(r) => {
                    cur = r;
                    any = true;
                }
                // Cut away entirely.
                None => {
                    any = true;
                    cur = b.clone();
                    break;
                }
            }
        }
        bodies.push(cur);
    }
    if !any {
        return Ok(None);
    }
    let mut out = part.clone();
    out.body = exact::to_brep(&bodies)?;
    out.exact_bodies = crate::part::ExactBodies(bodies);
    Ok(Some(out))
}

/// `part` with its bodies built: a document of the file that was never
/// shown has no exact bodies until it is regenerated (a copy is).
pub fn built(part: &PartDocument) -> std::borrow::Cow<'_, PartDocument> {
    if part.exact_bodies.0.is_empty() && !part.features.is_empty() {
        let mut p = part.clone();
        let _ = p.regenerate();
        std::borrow::Cow::Owned(p)
    } else {
        std::borrow::Cow::Borrowed(part)
    }
}

/// The world box of `s` placed by `at`.
fn world_box(s: &ExactSolid, at: &Transform3) -> Option<(Point3, Point3)> {
    let (lo, hi) = exact::bounds(s)?;
    let mut out: Option<(Point3, Point3)> = None;
    for k in 0..8 {
        let p = Point3::new(
            if k & 1 == 0 { lo.x } else { hi.x },
            if k & 2 == 0 { lo.y } else { hi.y },
            if k & 4 == 0 { lo.z } else { hi.z },
        );
        let w = at * p;
        out = Some(match out {
            Some((a, b)) => (a.inf(&w), b.sup(&w)),
            None => (w, w),
        });
    }
    out
}

/// The part's body in the tools part named `tool`, and the occurrence paths
/// of assembly `asm` (shown parts, the tools part left out) whose parts it
/// reaches — Inventor's default participants.
pub fn reached(
    project: &Project,
    linked: &Linked,
    top: DocRef,
    asm: &AssemblyDocument,
    tool: &ExactSolid,
) -> Vec<Vec<u32>> {
    let skip = asm.features.as_ref().map(|f| f.occurrence);
    let Some((tlo, thi)) = world_box(tool, &Transform3::identity()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for l in tree::leaves(project, linked, top, asm) {
        if Some(l.path[0]) == skip {
            continue;
        }
        let Some(part) = tree::part_of(project, linked, l.component) else {
            continue;
        };
        let part = built(part);
        let hit = part.exact_bodies.0.iter().any(|b| {
            world_box(b, &l.world).is_some_and(|(lo, hi)| {
                lo.x <= thi.x
                    && hi.x >= tlo.x
                    && lo.y <= thi.y
                    && hi.y >= tlo.y
                    && lo.z <= thi.z
                    && hi.z >= tlo.z
            })
        });
        if hit {
            out.push(l.path.clone());
        }
    }
    out
}

/// The assembly features of assembly `asm_id` brought up to date with its
/// tools part: a new tool body cuts what it reaches, a removed one goes.
/// Returns whether the participants changed.
pub fn refresh(project: &mut Project, linked: &Linked, asm_id: DocumentId) -> bool {
    let Some(DocumentData::Assembly(asm)) = project.document(asm_id).map(|d| &d.data) else {
        return false;
    };
    let Some(f) = asm.features.clone() else {
        return false;
    };
    let Some(o) = asm.find(f.occurrence) else {
        return false;
    };
    let Some(tp) = tree::part_of(project, linked, o.component) else {
        return false;
    };
    let bodies = tools(&built(tp));
    let mut participants = f.participants.clone();
    participants.retain(|name, _| bodies.iter().any(|b| b.name.as_deref() == Some(name)));
    for b in &bodies {
        let Some(name) = b.name.clone() else { continue };
        participants
            .entry(name)
            .or_insert_with(|| reached(project, linked, DocRef::local(asm_id), asm, b));
    }
    if participants == f.participants {
        return false;
    }
    if let Some(DocumentData::Assembly(a)) = project.document_mut(asm_id).map(|d| &mut d.data)
        && let Some(f) = a.features.as_mut()
    {
        f.participants = participants;
    }
    true
}

/// The tools cutting the occurrence at `path` of assembly `asm` (whose
/// tools part is `tools_part`): those whose participants list it.
pub fn tools_for(
    asm: &AssemblyDocument,
    tools_part: &PartDocument,
    path: &[u32],
) -> Vec<ExactSolid> {
    let Some(f) = &asm.features else {
        return Vec::new();
    };
    tools(tools_part)
        .into_iter()
        .filter(|b| {
            b.name
                .as_ref()
                .and_then(|n| f.participants.get(n))
                .is_some_and(|ps| ps.iter().any(|p| p == path))
        })
        .collect()
}

/// The tools part of assembly `asm_id`, made when it has none: a part
/// document `<assembly> Features` placed hidden and grounded at the origin
/// (BOM Reference). Returns its occurrence and document.
pub fn ensure_tools(
    project: &mut Project,
    asm_id: DocumentId,
) -> Result<(u32, DocumentId), String> {
    let Some(DocumentData::Assembly(asm)) = project.document(asm_id).map(|d| &d.data) else {
        return Err("not an assembly".into());
    };
    if let Some(f) = &asm.features
        && let Some(o) = asm.find(f.occurrence)
    {
        return Ok((f.occurrence, o.component.document));
    }
    let name = project
        .document(asm_id)
        .map(|d| format!("{} Features", d.name))
        .unwrap_or_else(|| "Assembly Features".into());
    let doc = project.add_document(&name, DocumentData::Part(PartDocument::default()));
    let Some(DocumentData::Assembly(a)) = project.document_mut(asm_id).map(|d| &mut d.data) else {
        return Err("not an assembly".into());
    };
    let occ = a.place(
        DocRef::local(doc),
        "Assembly Features",
        Transform3::identity(),
        true,
    );
    if let Some(o) = a.find_mut(occ) {
        o.visible = false;
        o.bom = Some(crate::document::iprops::BomStructure::Reference);
    }
    a.features = Some(AsmFeatures {
        occurrence: occ,
        participants: BTreeMap::new(),
    });
    Ok((occ, doc))
}
