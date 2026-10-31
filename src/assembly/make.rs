//! Make Part and Make Components (spec 3.10 §3.6, T6): bodies of a
//! multi-body master part become parts of their own — each a part whose
//! base feature derives that one body from the master (`part::derive`) — and
//! optionally the components of an assembly, placed where the bodies are.
//! Changing the master updates every part made from it ([`update_derived`]).

use crate::document::{AssemblyDocument, DocRef, DocumentData, DocumentId, Project};
use crate::kernel::Transform3;
use crate::part::PartDocument;
use crate::part::derive::{DeriveLink, DeriveSource};

/// Where Make Components puts the parts.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// No assembly: only the parts.
    None,
    /// A new assembly of the design file, named so.
    New(String),
    /// An assembly of the design file.
    Existing(DocumentId),
}

/// What Make Components made.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Made {
    /// The new parts, in the order of the bodies.
    pub parts: Vec<DocumentId>,
    /// The assembly they were placed in.
    pub assembly: Option<DocumentId>,
}

/// The master part `master` of `project`, built (a stored part has no
/// bodies until it is regenerated).
fn built(project: &Project, master: DocumentId) -> Result<(PartDocument, String), String> {
    let d = project
        .document(master)
        .ok_or("the master part no longer exists")?;
    let DocumentData::Part(p) = &d.data else {
        return Err(format!("{} is not a part", d.name));
    };
    let mut p = p.clone();
    if !p.exact || p.exact_bodies.0.is_empty() {
        p.regenerate().map_err(|e| e.to_string())?;
    }
    Ok((p, d.name.clone()))
}

/// The derive link taking only `bodies` of master `master` named `name`.
fn link_for(
    master: DocumentId,
    name: &str,
    part: &PartDocument,
    bodies: &[String],
) -> Result<DeriveLink, String> {
    let all: Vec<String> = part
        .exact_bodies
        .0
        .iter()
        .filter_map(|s| s.name.clone())
        .collect();
    for b in bodies {
        if !all.contains(b) {
            return Err(format!("{name} has no body {b}"));
        }
    }
    let mut link = DeriveLink::new(DocRef::local(master), name);
    link.surfaces = true;
    link.exclude = all.into_iter().filter(|b| !bodies.contains(b)).collect();
    Ok(link)
}

/// **Make Part**: a new part `name` of `project` whose base feature derives
/// `bodies` of master `master`. Returns the new document. Nothing changes
/// on failure.
pub fn make_part(
    project: &mut Project,
    master: DocumentId,
    bodies: &[String],
    name: &str,
) -> Result<DocumentId, String> {
    if bodies.is_empty() {
        return Err("choose the bodies".into());
    }
    let (part, master_name) = built(project, master)?;
    let link = link_for(master, &master_name, &part, bodies)?;
    let mut new = PartDocument::default();
    new.add_derive(&part, link).map_err(|e| e.to_string())?;
    Ok(project.add_document(name, DocumentData::Part(new)))
}

/// **Make Components**: each of `bodies` of master `master` becomes a part
/// (named `names[i]`, else `<master> <body>`), placed grounded at the
/// master's frame in `target` — so each stands where its body is. Nothing
/// changes on failure.
pub fn make_components(
    project: &mut Project,
    master: DocumentId,
    bodies: &[String],
    names: &[String],
    target: Target,
) -> Result<Made, String> {
    if bodies.is_empty() {
        return Err("choose the bodies".into());
    }
    let (part, master_name) = built(project, master)?;
    if let Target::Existing(a) = target
        && !matches!(
            project.document(a).map(|d| &d.data),
            Some(DocumentData::Assembly(_))
        )
    {
        return Err("the target is not an assembly".into());
    }
    // Every part first, on a copy: all or nothing.
    let mut made: Vec<(String, PartDocument)> = Vec::new();
    for (i, b) in bodies.iter().enumerate() {
        let link = link_for(master, &master_name, &part, std::slice::from_ref(b))?;
        let mut new = PartDocument::default();
        new.add_derive(&part, link)
            .map_err(|e| format!("{b}: {e}"))?;
        // Named after the body as the browser shows it (`Master Solid2`).
        let shown = part
            .bodies_before(part.features.len())
            .into_iter()
            .find(|(n, _)| n == b)
            .map_or_else(|| b.clone(), |(_, d)| d);
        let name = names
            .get(i)
            .filter(|n| !n.trim().is_empty())
            .cloned()
            .unwrap_or_else(|| format!("{master_name} {shown}"));
        made.push((name, new));
    }
    let mut out = Made::default();
    let mut labels = Vec::new();
    for (name, p) in made {
        out.parts
            .push(project.add_document(&name, DocumentData::Part(p)));
        labels.push(name);
    }
    let asm = match target {
        Target::None => return Ok(out),
        Target::New(n) => {
            project.add_document(&n, DocumentData::Assembly(AssemblyDocument::default()))
        }
        Target::Existing(a) => a,
    };
    if let Some(DocumentData::Assembly(a)) = project.document_mut(asm).map(|d| &mut d.data) {
        for (id, name) in out.parts.iter().zip(&labels) {
            a.place(DocRef::local(*id), name, Transform3::identity(), true);
        }
    }
    out.assembly = Some(asm);
    Ok(out)
}

/// The parts of this file whose derived features read other parts of this
/// file brought up to date with them (a master changed: every part made
/// from it follows), chains included. Returns the parts that changed;
/// failures as `<part>: <why>` (their last state stays).
pub fn update_derived(project: &mut Project) -> (Vec<DocumentId>, Vec<String>) {
    let mut changed = Vec::new();
    let mut errors = Vec::new();
    // Chains settle in as many rounds as they are long.
    for _ in 0..8 {
        let mut round = false;
        let ids: Vec<DocumentId> = project.documents().iter().map(|d| d.id).collect();
        for id in ids {
            let derived: Vec<(usize, DocRef)> = match project.document(id).map(|d| &d.data) {
                Some(DocumentData::Part(p)) => p
                    .derived()
                    .into_iter()
                    .map(|(k, l)| (k, l.source))
                    .collect(),
                _ => continue,
            };
            for (k, src) in derived {
                if src.library.is_some() {
                    continue;
                }
                let Some(parts) = source_parts(project, src.document) else {
                    continue;
                };
                let source = DeriveSource::of(parts.iter().map(|(p, t)| (p, *t)).collect());
                let name = project
                    .document(id)
                    .map(|d| d.name.clone())
                    .unwrap_or_default();
                if let Some(DocumentData::Part(p)) = project.document_mut(id).map(|d| &mut d.data) {
                    match p.update_derive_from(k, &source) {
                        Ok(true) => {
                            round = true;
                            if !changed.contains(&id) {
                                changed.push(id);
                            }
                        }
                        Ok(false) => {}
                        Err(e) => errors.push(format!("{name}: {e}")),
                    }
                }
            }
        }
        if !round {
            break;
        }
    }
    (changed, errors)
}

/// The parts document `id` of this file shows, built: a part, or every
/// part of an assembly where it stands.
fn source_parts(project: &Project, id: DocumentId) -> Option<Vec<(PartDocument, Transform3)>> {
    match project.document(id).map(|d| &d.data)? {
        DocumentData::Part(_) => Some(vec![(built(project, id).ok()?.0, Transform3::identity())]),
        DocumentData::Assembly(a) => {
            let linked = super::tree::Linked::new();
            let mut out = Vec::new();
            for l in super::tree::leaves(project, &linked, DocRef::local(id), a) {
                if l.component.library.is_some() {
                    return None;
                }
                out.push((built(project, l.component.document).ok()?.0, l.world));
            }
            Some(out)
        }
        _ => None,
    }
}
