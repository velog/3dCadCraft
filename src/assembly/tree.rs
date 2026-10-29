//! Subassemblies (`docs/spec/3.1a-subassemblies.md`): an occurrence whose
//! component is an assembly — of the same design file or of a linked file
//! (`.ccasm`). This module walks the tree — every part at any depth with its
//! path and world placement — with a guard against an assembly that contains
//! itself.
//!
//! A linked assembly's own component references are written in *its* file's
//! terms (a local document there, or one of its library links). The walk
//! rebases them onto the top file's terms ([`rebase`]): a local document of
//! the linked file becomes `DocRef { library: <the link that reached the
//! file>, .. }`, a link of the linked file keeps its id, and [`Linked`] holds
//! every linked file's project by any library id leading to it (ids are
//! random, so links of different files do not collide).

use std::collections::HashMap;
use std::rc::Rc;

use crate::document::{
    AssemblyDocument, DocRef, DocumentData, DocumentId, LibraryId, Occurrence, Project,
};
use crate::kernel::Transform3;
use crate::part::PartDocument;

/// Deepest nesting walked (deeper levels are left out, as a loop would be).
pub const MAX_DEPTH: usize = 32;

/// The projects of linked files, by the library ids that lead to them (from
/// the top file or from another linked file). Empty: only this design file.
#[derive(Clone, Debug, Default)]
pub struct Linked {
    files: HashMap<LibraryId, Rc<Project>>,
}

impl Linked {
    /// No linked files.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records the project library `id` leads to.
    pub fn insert(&mut self, id: LibraryId, project: Rc<Project>) {
        self.files.insert(id, project);
    }

    /// The project library `id` leads to, when it is loaded.
    pub fn get(&self, id: LibraryId) -> Option<&Project> {
        self.files.get(&id).map(|p| &**p)
    }

    /// Whether library `id` is known.
    pub fn contains(&self, id: LibraryId) -> bool {
        self.files.contains_key(&id)
    }
}

/// Component reference `r`, written in a document reached through library
/// `from` (`None`: the top file), in the top file's terms.
pub fn rebase(r: DocRef, from: Option<LibraryId>) -> DocRef {
    match r.library {
        None => DocRef {
            library: from,
            document: r.document,
        },
        Some(_) => r,
    }
}

/// A copy of assembly `a`, reached through library `from`, with its
/// components in the top file's terms (for walks that read occurrences
/// themselves, like the BOM).
pub fn rebased(a: &AssemblyDocument, from: Option<LibraryId>) -> AssemblyDocument {
    let mut a = a.clone();
    for o in &mut a.occurrences {
        o.component = rebase(o.component, from);
    }
    a
}

/// The document `r` (in the top file's terms) refers to.
fn document_of<'a>(
    project: &'a Project,
    linked: &'a Linked,
    r: DocRef,
) -> Option<&'a DocumentData> {
    let p = match r.library {
        None => project,
        Some(lib) => linked.get(lib)?,
    };
    p.document(r.document).map(|d| &d.data)
}

/// The assembly `r` shows: one of this design file or of a linked file.
pub fn assembly_of<'a>(
    project: &'a Project,
    linked: &'a Linked,
    r: DocRef,
) -> Option<&'a AssemblyDocument> {
    match document_of(project, linked, r) {
        Some(DocumentData::Assembly(a)) => Some(a),
        _ => None,
    }
}

/// The part `r` shows: one of this design file or of a linked file.
pub fn part_of<'a>(
    project: &'a Project,
    linked: &'a Linked,
    r: DocRef,
) -> Option<&'a PartDocument> {
    match document_of(project, linked, r) {
        Some(DocumentData::Part(p)) => Some(p),
        _ => None,
    }
}

/// Whether placing assembly `inner` into assembly `outer` would make a loop:
/// `inner` is `outer` or contains it at some depth (linked files included).
pub fn would_loop(project: &Project, linked: &Linked, outer: DocumentId, inner: DocRef) -> bool {
    fn contains(p: &Project, l: &Linked, a: DocRef, target: DocumentId, depth: usize) -> bool {
        if a.document == target || depth > MAX_DEPTH {
            return true;
        }
        let Some(asm) = assembly_of(p, l, a) else {
            return false;
        };
        asm.occurrences.iter().any(|o| {
            let c = rebase(o.component, a.library);
            assembly_of(p, l, c).is_some() && contains(p, l, c, target, depth + 1)
        })
    }
    contains(project, linked, inner, outer, 0)
}

/// Where a reference into the tree lands ([`nested`]).
#[derive(Clone, Debug)]
pub struct Nested<'a> {
    /// The occurrence at the end of the path (in the innermost assembly).
    pub occurrence: &'a Occurrence,
    /// Its component in the top file's terms (use it, never
    /// `occurrence.component`, which is in its own file's terms).
    pub component: DocRef,
    /// Its placement in the top occurrence's own coordinates.
    pub at: Transform3,
    /// Its name with the path, `Bracket:1/Post:2`.
    pub name: String,
}

/// The component a reference into occurrence `top` of `asm` lies on: the
/// part (or subassembly) at the end of `sub`. A path that no longer exists is
/// an error (the relationship becomes sick, rule 8).
pub fn nested<'a>(
    project: &'a Project,
    linked: &'a Linked,
    asm: &'a AssemblyDocument,
    top: u32,
    sub: &[u32],
) -> Result<Nested<'a>, String> {
    let o = asm.find(top).ok_or("the component no longer exists")?;
    let mut n = Nested {
        occurrence: o,
        component: o.component,
        at: Transform3::identity(),
        name: o.name.clone(),
    };
    for &id in sub {
        let inner = assembly_of(project, linked, n.component)
            .ok_or_else(|| format!("{} is no longer a subassembly", n.name))?;
        let o = inner
            .find(id)
            .ok_or_else(|| format!("{}: the component inside no longer exists", n.name))?;
        n.component = rebase(o.component, n.component.library);
        n.occurrence = o;
        n.at *= o.transform;
        n.name = format!("{}/{}", n.name, o.name);
    }
    Ok(n)
}

/// One part placed somewhere in the tree.
#[derive(Clone, Debug)]
pub struct Leaf<'a> {
    /// Occurrence ids from the top assembly down to this part.
    pub path: Vec<u32>,
    /// The part's occurrence (in the innermost assembly).
    pub occurrence: &'a Occurrence,
    /// Its component in the top file's terms (use it, never
    /// `occurrence.component`).
    pub component: DocRef,
    /// Its placement in the top assembly's coordinates.
    pub world: Transform3,
}

/// Every shown part of `asm` at any depth (hidden or suppressed occurrences
/// and their subtrees left out); subassemblies are walked, not listed.
/// `top` is the document of `asm` in the top file's terms (the loop guard
/// starts there; its library rebases the references inside).
pub fn leaves<'a>(
    project: &'a Project,
    linked: &'a Linked,
    top: DocRef,
    asm: &'a AssemblyDocument,
) -> Vec<Leaf<'a>> {
    let mut out = Vec::new();
    let mut stack = vec![top.document];
    let mut w = Walk {
        project,
        linked,
        path: Vec::new(),
        stack: &mut stack,
        out: &mut out,
    };
    w.walk(asm, top.library, Transform3::identity());
    out
}

struct Walk<'a, 'w> {
    project: &'a Project,
    linked: &'a Linked,
    path: Vec<u32>,
    stack: &'w mut Vec<DocumentId>,
    out: &'w mut Vec<Leaf<'a>>,
}

impl<'a> Walk<'a, '_> {
    fn walk(&mut self, asm: &'a AssemblyDocument, from: Option<LibraryId>, at: Transform3) {
        for o in asm.occurrences.iter().filter(|o| o.shown()) {
            let world = at * o.transform;
            let component = rebase(o.component, from);
            self.path.push(o.id);
            match assembly_of(self.project, self.linked, component) {
                Some(sub) => {
                    let d = component.document;
                    if !self.stack.contains(&d) && self.stack.len() <= MAX_DEPTH {
                        self.stack.push(d);
                        self.walk(sub, component.library, world);
                        self.stack.pop();
                    }
                }
                None => self.out.push(Leaf {
                    path: self.path.clone(),
                    occurrence: o,
                    component,
                    world,
                }),
            }
            self.path.pop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::Vector3;
    use crate::part::PartDocument;

    /// Plate in a subassembly "Sub" (2 bolts), Sub placed twice in "Top".
    fn project() -> (Project, DocumentId, DocumentId, DocumentId) {
        let mut p = Project::new("T");
        let part = PartDocument::with_box("Box1", [10.0, 10.0, 10.0]).unwrap();
        let bolt = p.add_document("Bolt", DocumentData::Part(part));
        let mut sub = AssemblyDocument::default();
        sub.place(DocRef::local(bolt), "Bolt", Transform3::identity(), true);
        let t = Transform3::translation(20.0, 0.0, 0.0);
        sub.place(DocRef::local(bolt), "Bolt", t, false);
        let sid = p.add_document("Sub", DocumentData::Assembly(sub));
        let mut top = AssemblyDocument::default();
        top.place(DocRef::local(sid), "Sub", Transform3::identity(), true);
        let up = Transform3::new(Vector3::new(0.0, 0.0, 100.0), Vector3::new(0.0, 0.0, 0.5));
        top.place(DocRef::local(sid), "Sub", up, false);
        top.place(
            DocRef::local(bolt),
            "Bolt",
            Transform3::translation(0.0, 50.0, 0.0),
            false,
        );
        let tid = p.add_document("Top", DocumentData::Assembly(top));
        (p, bolt, sid, tid)
    }

    /// No linked files (leaked: the walks borrow it for the test's length).
    fn none() -> &'static Linked {
        Box::leak(Box::new(Linked::new()))
    }

    fn top(p: &Project, id: DocumentId) -> &AssemblyDocument {
        assembly_of(p, none(), DocRef::local(id)).unwrap()
    }

    #[test]
    fn leaves_walk_every_level_with_world_placements() {
        let (p, _, _, tid) = project();
        let l = leaves(&p, none(), DocRef::local(tid), top(&p, tid));
        let paths: Vec<Vec<u32>> = l.iter().map(|x| x.path.clone()).collect();
        assert_eq!(
            paths,
            [vec![1, 1], vec![1, 2], vec![2, 1], vec![2, 2], vec![3]]
        );
        // Bolt 2 of the second Sub: Sub's turn and lift, then its own shift.
        let want =
            top(&p, tid).find(2).unwrap().transform * Transform3::translation(20.0, 0.0, 0.0);
        assert!((l[3].world.translation.vector - want.translation.vector).norm() < 1e-12);
    }

    #[test]
    fn nested_references_reach_parts_inside() {
        let (p, bolt, _, tid) = project();
        let a = top(&p, tid);
        let n = nested(&p, none(), a, 2, &[2]).unwrap();
        assert_eq!(
            (n.component, n.name.as_str()),
            (DocRef::local(bolt), "Sub:2/Bolt:2")
        );
        assert!((n.at.translation.vector - Vector3::new(20.0, 0.0, 0.0)).norm() < 1e-12);
        assert_eq!(nested(&p, none(), a, 3, &[]).unwrap().name, "Bolt:1");
        assert!(nested(&p, none(), a, 2, &[7]).is_err());
        // A part has nothing inside.
        assert!(nested(&p, none(), a, 3, &[1]).is_err());
    }

    #[test]
    fn relationships_reach_parts_inside_subassemblies() {
        use crate::assembly::relation::{self, Constraint, ConstraintKind, GeomRef};
        let (mut p, _, _, tid) = project();
        let face = |occurrence, sub: Vec<u32>, name: &str| GeomRef::Face {
            occurrence,
            sub,
            name: format!("Box1:{name}"),
            hint: [5.0, 5.0, 5.0],
        };
        // The free Bolt:1 on top of the second bolt inside the grounded Sub:1.
        let c = Constraint::new(
            ConstraintKind::Mate,
            face(3, Vec::new(), "Bottom"),
            face(1, vec![2], "Top"),
        );
        if let Some(DocumentData::Assembly(a)) = p.document_mut(tid).map(|d| &mut d.data) {
            a.add_constraint(c);
        }
        let a = top(&p, tid);
        // In Sub:1's coordinates: the top of the bolt moved 20 along x.
        match relation::resolve(&p, a, &face(1, vec![2], "Top")) {
            Ok(relation::Geom::Plane { p: at, n }) => {
                assert!((at.x - 25.0).abs() < 1e-9 && (at.z - 10.0).abs() < 1e-9);
                assert!((n - Vector3::z()).norm() < 1e-9);
            }
            other => panic!("{other:?}"),
        }
        let solved = crate::assembly::solve(a, &|r| relation::resolve(&p, a, r), None);
        assert!(solved.converged && solved.sick.is_empty(), "{solved:?}");
        let t = solved.transforms.iter().find(|(id, _)| *id == 3).unwrap().1;
        assert!((t.translation.vector.z - 10.0).abs() < 1e-6, "{t:?}");
        // A path that no longer exists makes it sick (rule 8).
        let gone = face(1, vec![9], "Top");
        assert!(relation::resolve(&p, a, &gone).is_err());
    }

    #[test]
    fn subassemblies_of_linked_files_are_walked() {
        // "Sub" (2 bolts, local to its file) lives in a linked file; the
        // top file places it twice through library link `lib`.
        let (lp, bolt, sid, _) = project();
        let lib = LibraryId::new_random();
        let mut linked = Linked::new();
        linked.insert(lib, Rc::new(lp));
        let mut p = Project::new("Top");
        let mut a = AssemblyDocument::default();
        let s = DocRef {
            library: Some(lib),
            document: sid,
        };
        a.place(s, "Sub", Transform3::identity(), true);
        a.place(s, "Sub", Transform3::translation(0.0, 0.0, 50.0), false);
        let tid = p.add_document("Top", DocumentData::Assembly(a));
        let a = assembly_of(&p, &linked, DocRef::local(tid)).unwrap();
        let l = leaves(&p, &linked, DocRef::local(tid), a);
        assert_eq!(l.len(), 4);
        // The bolts are the linked file's local part, reached through `lib`.
        let want = DocRef {
            library: Some(lib),
            document: bolt,
        };
        assert!(l.iter().all(|x| x.component == want));
        assert!(part_of(&p, &linked, want).is_some());
        assert!((l[3].world.translation.vector - Vector3::new(20.0, 0.0, 50.0)).norm() < 1e-12);
        let n = nested(&p, &linked, a, 2, &[2]).unwrap();
        assert_eq!((n.component, n.name.as_str()), (want, "Sub:2/Bolt:2"));
        // Without the file loaded each Sub is one missing component.
        let missing = leaves(&p, none(), DocRef::local(tid), a);
        assert!(missing.len() == 2 && missing.iter().all(|x| x.component == s));
        // The linked file's Sub cannot take the top file's assembly.
        assert!(!would_loop(&p, &linked, tid, s));
    }

    #[test]
    fn hidden_subassemblies_are_left_out() {
        let (mut p, _, _, tid) = project();
        if let Some(DocumentData::Assembly(a)) = p.document_mut(tid).map(|d| &mut d.data) {
            a.find_mut(1).unwrap().visible = false;
        }
        assert_eq!(
            leaves(&p, none(), DocRef::local(tid), top(&p, tid)).len(),
            3
        );
    }

    #[test]
    fn loops_are_found_and_never_walked() {
        let (mut p, _, sid, tid) = project();
        // Placing Top in Sub would loop; Sub in Top again would not.
        assert!(would_loop(&p, none(), sid, DocRef::local(tid)));
        assert!(would_loop(&p, none(), tid, DocRef::local(tid)));
        assert!(!would_loop(&p, none(), tid, DocRef::local(sid)));
        // A loop already in a file: the walk stops instead of recursing.
        if let Some(DocumentData::Assembly(a)) = p.document_mut(sid).map(|d| &mut d.data) {
            a.place(DocRef::local(tid), "Top", Transform3::identity(), false);
        }
        let n = leaves(&p, none(), DocRef::local(tid), top(&p, tid)).len();
        assert_eq!(n, 5);
    }
}
