//! The browser's restructuring (Inventor's Demote and Promote, spec 3.1a S7)
//! and browser folders: components moved into a new subassembly and back out
//! of one. Components keep their place in the assembly.

use serde::{Deserialize, Serialize};

use super::relation::{ASSEMBLY, GeomRef};
use crate::document::{AssemblyDocument, DocRef, DocumentData, DocumentId, Project};
use crate::kernel::Transform3;

/// A folder of the browser: only a way to list components (Inventor's Create
/// Folder), with no effect on the assembly's structure or the BOM.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BrowserFolder {
    /// Stable id (never reused).
    pub id: u32,
    /// Its name in the browser.
    pub name: String,
    /// The occurrences listed in it.
    pub items: Vec<u32>,
}

impl AssemblyDocument {
    /// A new folder holding `items` (taken out of other folders); its id.
    pub fn add_folder(&mut self, name: Option<&str>, items: &[u32]) -> u32 {
        let id = self.folders.iter().map(|f| f.id).max().unwrap_or(0) + 1;
        for f in &mut self.folders {
            f.items.retain(|i| !items.contains(i));
        }
        let name = name.map_or_else(|| format!("Folder{id}"), str::to_owned);
        self.folders.push(BrowserFolder {
            id,
            name,
            items: items.to_vec(),
        });
        id
    }

    /// Moves `items` into folder `id` (`None`: out of every folder).
    pub fn move_to_folder(&mut self, id: Option<u32>, items: &[u32]) {
        for f in &mut self.folders {
            f.items.retain(|i| !items.contains(i));
        }
        if let Some(f) = id.and_then(|id| self.folders.iter_mut().find(|f| f.id == id)) {
            f.items.extend_from_slice(items);
        }
    }

    /// Deletes folder `id`; its components stay in the assembly.
    pub fn delete_folder(&mut self, id: u32) {
        self.folders.retain(|f| f.id != id);
    }
}

/// What a Demote did.
#[derive(Clone, Debug)]
pub struct Demoted {
    /// The new subassembly's document.
    pub document: DocumentId,
    /// Its occurrence in the assembly.
    pub occurrence: u32,
}

/// Why a component cannot leave its assembly: what depends on its place in it.
fn pinned(a: &AssemblyDocument, id: u32) -> Option<&'static str> {
    if a.pattern_of(id).is_some() || a.patterns.iter().any(|p| p.sources.contains(&id)) {
        return Some("it belongs to a component pattern");
    }
    if a.frames
        .iter()
        .any(|f| f.members.iter().any(|m| m.occurrence == id))
    {
        return Some("it is a frame member");
    }
    None
}

/// Moves the occurrences `ids` of assembly `asm` into a new subassembly named
/// `name`, placed at the assembly's origin so nothing moves. Relationships among
/// them go with them; those to a component that stays remain, by a path into it.
pub fn demote(
    project: &mut Project,
    asm: DocumentId,
    ids: &[u32],
    name: &str,
) -> Result<Demoted, String> {
    if ids.is_empty() {
        return Err("select the components to demote".into());
    }
    let a = match project.document(asm).map(|d| &d.data) {
        Some(DocumentData::Assembly(a)) => a.clone(),
        _ => return Err("not an assembly".into()),
    };
    for &id in ids {
        if a.find(id).is_none() {
            return Err("a component no longer exists".into());
        }
        if let Some(why) = pinned(&a, id) {
            return Err(format!("{}: {why}", a.find(id).map_or("?", |o| &o.name)));
        }
    }
    let mut inner = AssemblyDocument::default();
    let mut outer = a.clone();
    let moves = |g: &GeomRef| ids.contains(&g.occurrence());
    // Relationships among the moved ones go with them; those to a component that stays
    // remain here, reaching into the subassembly by a path.
    outer.constraints.retain(|c| {
        let all = std::iter::once(&c.a)
            .chain(std::iter::once(&c.b))
            .chain(c.plane.iter())
            .all(moves);
        if all {
            inner.constraints.push(c.clone());
        }
        !all
    });
    // The same occurrence ids inside: a subassembly has its own id space, and the
    // relationships keep pointing at them.
    for o in a.occurrences.iter().filter(|o| ids.contains(&o.id)) {
        inner.occurrences.push(o.clone());
    }
    inner.fix_ids();
    let grounded = inner.occurrences.iter().any(|o| o.grounded);
    // Removed first: `remove` also takes the relationships that use an occurrence.
    let kept = std::mem::take(&mut outer.constraints);
    for &id in ids {
        outer.remove(id);
    }
    outer.move_to_folder(None, ids);
    let document = project.add_document(name, DocumentData::Assembly(inner));
    let occurrence = outer.place(
        DocRef::local(document),
        name,
        Transform3::identity(),
        grounded,
    );
    outer.constraints = kept;
    for c in &mut outer.constraints {
        for g in [&mut c.a, &mut c.b].into_iter().chain(c.plane.as_mut()) {
            let old = g.occurrence();
            if ids.contains(&old) {
                *g.occurrence_mut() = occurrence;
                g.sub_mut().insert(0, old);
            }
        }
    }
    if let Some(d) = project.document_mut(asm) {
        d.data = DocumentData::Assembly(outer);
    }
    Ok(Demoted {
        document,
        occurrence,
    })
}

/// Moves occurrence `inner` of the subassembly placed as `top` up into assembly
/// `asm`, keeping its place. Relationships inside the subassembly that use it move up
/// with it, reaching the others by a path; one to the subassembly's own origin cannot
/// (a placed subassembly has no origin to name) and is removed and listed. The
/// assembly's references into it follow.
pub fn promote(
    project: &mut Project,
    asm: DocumentId,
    top: u32,
    inner: u32,
) -> Result<(u32, Vec<String>), String> {
    let mut a = match project.document(asm).map(|d| &d.data) {
        Some(DocumentData::Assembly(a)) => a.clone(),
        _ => return Err("not an assembly".into()),
    };
    let t = a.find(top).ok_or("the subassembly no longer exists")?;
    let (place, component, holder) = (t.transform, t.component, t.clone());
    if component.library.is_some() {
        return Err("the subassembly is in another file: open it to change it".into());
    }
    if a.occurrences
        .iter()
        .filter(|o| o.component == component)
        .count()
        > 1
    {
        return Err("the subassembly is placed more than once".into());
    }
    let mut sub = match project.document(component.document).map(|d| &d.data) {
        Some(DocumentData::Assembly(s)) => s.clone(),
        _ => return Err("not a subassembly".into()),
    };
    if let Some(why) = pinned(&sub, inner) {
        return Err(why.into());
    }
    let o = sub
        .find(inner)
        .ok_or("the component no longer exists")?
        .clone();
    let (moving, stay): (Vec<_>, Vec<_>) = std::mem::take(&mut sub.constraints)
        .into_iter()
        .partition(|c| c.uses(inner));
    sub.constraints = stay;
    sub.remove(inner);
    // A flexible subassembly's component comes up from where it stands.
    let id = a.place(
        o.component,
        &o.name,
        place * holder.child_transform(&o),
        o.grounded,
    );
    if let Some(n) = a.find_mut(id) {
        n.visible = o.visible;
        n.enabled = o.enabled;
        n.transparent = o.transparent;
        n.suppressed = o.suppressed;
        n.looks = o.looks.clone();
    }
    let mut removed = Vec::new();
    for mut c in moving {
        let refs = std::iter::once(&c.a)
            .chain(std::iter::once(&c.b))
            .chain(c.plane.iter());
        if refs.clone().any(|g| g.occurrence() == ASSEMBLY) {
            removed.push(c.name.clone());
            continue;
        }
        for g in [&mut c.a, &mut c.b].into_iter().chain(c.plane.as_mut()) {
            let old = g.occurrence();
            if old == inner {
                *g.occurrence_mut() = id;
            } else {
                *g.occurrence_mut() = top;
                g.sub_mut().insert(0, old);
            }
        }
        a.add_constraint(c);
    }
    // The assembly's relationships into it now name it directly.
    for c in &mut a.constraints {
        for g in [&mut c.a, &mut c.b].into_iter().chain(c.plane.as_mut()) {
            if g.occurrence() != ASSEMBLY
                && g.occurrence() == top
                && g.sub().first() == Some(&inner)
            {
                *g.occurrence_mut() = id;
                let rest = g.sub()[1..].to_vec();
                *g.sub_mut() = rest;
            }
        }
    }
    if let Some(d) = project.document_mut(component.document) {
        d.data = DocumentData::Assembly(sub);
    }
    if let Some(d) = project.document_mut(asm) {
        d.data = DocumentData::Assembly(a);
    }
    Ok((id, removed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assembly::relation::{Constraint, ConstraintKind};
    use crate::kernel::Vector3;
    use crate::part::PartDocument;

    fn face(o: u32) -> GeomRef {
        GeomRef::Face {
            sub: Vec::new(),
            occurrence: o,
            name: "F".into(),
            hint: [0.0, 0.0, 0.0],
        }
    }

    fn setup() -> (Project, DocumentId, [u32; 3]) {
        let mut p = Project::new("t");
        let part = p.add_document("Part", DocumentData::Part(PartDocument::default()));
        let mut a = AssemblyDocument::default();
        let r = DocRef {
            library: None,
            document: part,
            variant: None,
        };
        let at = |x: f64| Transform3::new(Vector3::new(x, 0.0, 0.0), Vector3::zeros());
        let ids = [
            a.place(r, "A:1", at(0.0), true),
            a.place(r, "B:1", at(10.0), false),
            a.place(r, "C:1", at(20.0), false),
        ];
        a.add_constraint(Constraint::new(
            ConstraintKind::Mate,
            face(ids[1]),
            face(ids[2]),
        ));
        a.add_constraint(Constraint::new(
            ConstraintKind::Mate,
            face(ids[0]),
            face(ids[1]),
        ));
        let asm = p.add_document("Asm", DocumentData::Assembly(a));
        let _ = PartDocument::default();
        (p, asm, ids)
    }

    fn assembly(p: &Project, d: DocumentId) -> &AssemblyDocument {
        match p.document(d).map(|x| &x.data) {
            Some(DocumentData::Assembly(a)) => a,
            _ => panic!("no assembly"),
        }
    }

    #[test]
    fn demote_keeps_places_and_inner_relationships() {
        let (mut p, asm, ids) = setup();
        let d = demote(&mut p, asm, &ids[1..], "Sub").unwrap();
        let top = assembly(&p, asm);
        assert_eq!(top.occurrences.len(), 2);
        // The relationship to the one that stayed remains, by a path.
        assert_eq!(top.constraints.len(), 1);
        let c = &top.constraints[0];
        assert_eq!(c.a.occurrence(), ids[0]);
        assert_eq!(c.b.occurrence(), d.occurrence);
        assert_eq!(c.b.sub(), &[ids[1]]);
        let sub = assembly(&p, d.document);
        assert_eq!(sub.occurrences.len(), 2);
        assert_eq!(sub.constraints.len(), 1);
        let world = top.find(d.occurrence).unwrap().transform * sub.occurrences[1].transform;
        assert!((world.translation.vector.x - 20.0).abs() < 1e-9);
    }

    #[test]
    fn a_failed_demote_changes_nothing() {
        let (mut p, asm, _) = setup();
        let before = assembly(&p, asm).occurrences.len();
        assert!(demote(&mut p, asm, &[99], "Sub").is_err());
        assert_eq!(assembly(&p, asm).occurrences.len(), before);
        assert_eq!(p.documents().len(), 2);
    }

    #[test]
    fn promote_undoes_demote_of_a_place() {
        let (mut p, asm, ids) = setup();
        let d = demote(&mut p, asm, &ids[1..], "Sub").unwrap();
        // The subassembly moved: the promoted part stays where it shows.
        if let Some(crate::document::Document {
            data: DocumentData::Assembly(a),
            ..
        }) = p.document_mut(asm)
        {
            a.find_mut(d.occurrence).unwrap().transform =
                Transform3::new(Vector3::new(0.0, 5.0, 0.0), Vector3::zeros());
        }
        let inner = assembly(&p, d.document).occurrences[1].id;
        let (id, removed) = promote(&mut p, asm, d.occurrence, inner).unwrap();
        assert!(removed.is_empty(), "{removed:?}");
        // Its relationship came up, reaching the other one by a path; the one from the
        // stayer into the subassembly still does.
        let top = assembly(&p, asm);
        assert_eq!(top.constraints.len(), 2);
        assert!(
            top.constraints
                .iter()
                .any(|c| c.uses(id) && c.a.occurrence() == d.occurrence && c.a.sub() == [ids[1]])
        );
        assert!(
            top.constraints
                .iter()
                .any(|c| c.a.occurrence() == ids[0] && c.b.sub() == [ids[1]])
        );
        assert!(assembly(&p, d.document).constraints.is_empty());
        let t = assembly(&p, asm)
            .find(id)
            .unwrap()
            .transform
            .translation
            .vector;
        assert!((t.x - 20.0).abs() < 1e-9 && (t.y - 5.0).abs() < 1e-9);
        assert_eq!(assembly(&p, d.document).occurrences.len(), 1);
    }

    #[test]
    fn folders_list_components_once() {
        let (mut p, asm, ids) = setup();
        let mut a = assembly(&p, asm).clone();
        let f = a.add_folder(None, &ids[..2]);
        assert_eq!(a.folders[0].name, "Folder1");
        let g = a.add_folder(Some("Mine"), &ids[1..]);
        assert_eq!(a.folders[0].items, vec![ids[0]]);
        assert_eq!(a.folders[1].items, ids[1..].to_vec());
        a.move_to_folder(Some(f), &[ids[2]]);
        assert_eq!(a.folders[0].items, vec![ids[0], ids[2]]);
        a.delete_folder(g);
        assert_eq!(a.folders.len(), 1);
        a.remove(ids[0]);
        p.document_mut(asm).unwrap().data = DocumentData::Assembly(a.clone());
        assert_eq!(a.occurrences.len(), 2);
    }
}
