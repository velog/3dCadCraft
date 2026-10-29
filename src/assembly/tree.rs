//! Subassemblies (`docs/spec/3.1a-subassemblies.md`): an occurrence whose
//! component is an assembly of the same design file. This module walks the
//! tree — every part at any depth with its path and world placement — with a
//! guard against an assembly that contains itself.

use crate::document::{AssemblyDocument, DocRef, DocumentData, DocumentId, Occurrence, Project};
use crate::kernel::Transform3;

/// Deepest nesting walked (deeper levels are left out, as a loop would be).
pub const MAX_DEPTH: usize = 32;

/// The assembly `r` shows, when it is one of this design file (subassemblies
/// of linked files come later).
pub fn assembly_of(project: &Project, r: DocRef) -> Option<&AssemblyDocument> {
    if r.library.is_some() {
        return None;
    }
    match project.document(r.document).map(|d| &d.data) {
        Some(DocumentData::Assembly(a)) => Some(a),
        _ => None,
    }
}

/// Whether placing assembly `inner` into assembly `outer` would make a loop:
/// `inner` is `outer` or contains it at some depth.
pub fn would_loop(project: &Project, outer: DocumentId, inner: DocumentId) -> bool {
    fn contains(p: &Project, a: DocumentId, target: DocumentId, depth: usize) -> bool {
        if a == target {
            return true;
        }
        if depth > MAX_DEPTH {
            return true;
        }
        let Some(asm) = assembly_of(p, DocRef::local(a)) else {
            return false;
        };
        asm.occurrences.iter().any(|o| {
            o.component.library.is_none()
                && assembly_of(p, o.component).is_some()
                && contains(p, o.component.document, target, depth + 1)
        })
    }
    contains(project, inner, outer, 0)
}

/// The component a reference into occurrence `top` of `asm` lies on: the
/// part (or subassembly) at the end of `sub`, its placement in `top`'s own
/// coordinates and its name (`Bracket:1/Post:2`). A path that no longer
/// exists is an error (the relationship becomes sick, rule 8).
pub fn nested<'a>(
    project: &'a Project,
    asm: &'a AssemblyDocument,
    top: u32,
    sub: &[u32],
) -> Result<(&'a Occurrence, Transform3, String), String> {
    let mut o = asm.find(top).ok_or("the component no longer exists")?;
    let mut name = o.name.clone();
    let mut at = Transform3::identity();
    for &id in sub {
        let inner = assembly_of(project, o.component)
            .ok_or_else(|| format!("{name} is no longer a subassembly"))?;
        o = inner
            .find(id)
            .ok_or_else(|| format!("{name}: the component inside no longer exists"))?;
        at *= o.transform;
        name = format!("{name}/{}", o.name);
    }
    Ok((o, at, name))
}

/// One part placed somewhere in the tree.
#[derive(Clone, Debug)]
pub struct Leaf<'a> {
    /// Occurrence ids from the top assembly down to this part.
    pub path: Vec<u32>,
    /// The part's occurrence (in the innermost assembly).
    pub occurrence: &'a Occurrence,
    /// Its placement in the top assembly's coordinates.
    pub world: Transform3,
}

/// Every shown part of `asm` at any depth (hidden or suppressed occurrences
/// and their subtrees left out); subassemblies are walked, not listed.
/// `top` is the document of `asm` (the loop guard starts there).
pub fn leaves<'a>(
    project: &'a Project,
    top: DocumentId,
    asm: &'a AssemblyDocument,
) -> Vec<Leaf<'a>> {
    let mut out = Vec::new();
    let mut stack = vec![top];
    walk(
        project,
        asm,
        Transform3::identity(),
        &mut Vec::new(),
        &mut stack,
        &mut out,
    );
    out
}

fn walk<'a>(
    project: &'a Project,
    asm: &'a AssemblyDocument,
    at: Transform3,
    path: &mut Vec<u32>,
    stack: &mut Vec<DocumentId>,
    out: &mut Vec<Leaf<'a>>,
) {
    for o in asm.occurrences.iter().filter(|o| o.shown()) {
        let world = at * o.transform;
        path.push(o.id);
        match assembly_of(project, o.component) {
            Some(sub) => {
                let d = o.component.document;
                if !stack.contains(&d) && stack.len() <= MAX_DEPTH {
                    stack.push(d);
                    walk(project, sub, world, path, stack, out);
                    stack.pop();
                }
            }
            None => out.push(Leaf {
                path: path.clone(),
                occurrence: o,
                world,
            }),
        }
        path.pop();
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

    fn top(p: &Project, id: DocumentId) -> &AssemblyDocument {
        assembly_of(p, DocRef::local(id)).unwrap()
    }

    #[test]
    fn leaves_walk_every_level_with_world_placements() {
        let (p, _, _, tid) = project();
        let l = leaves(&p, tid, top(&p, tid));
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
        let (o, at, name) = nested(&p, a, 2, &[2]).unwrap();
        assert_eq!(
            (o.component, name.as_str()),
            (DocRef::local(bolt), "Sub:2/Bolt:2")
        );
        assert!((at.translation.vector - Vector3::new(20.0, 0.0, 0.0)).norm() < 1e-12);
        assert_eq!(nested(&p, a, 3, &[]).unwrap().2, "Bolt:1");
        assert!(nested(&p, a, 2, &[7]).is_err());
        // A part has nothing inside.
        assert!(nested(&p, a, 3, &[1]).is_err());
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
    fn hidden_subassemblies_are_left_out() {
        let (mut p, _, _, tid) = project();
        if let Some(DocumentData::Assembly(a)) = p.document_mut(tid).map(|d| &mut d.data) {
            a.find_mut(1).unwrap().visible = false;
        }
        assert_eq!(leaves(&p, tid, top(&p, tid)).len(), 3);
    }

    #[test]
    fn loops_are_found_and_never_walked() {
        let (mut p, _, sid, tid) = project();
        // Placing Top in Sub would loop; Sub in Top again would not.
        assert!(would_loop(&p, sid, tid));
        assert!(would_loop(&p, tid, tid));
        assert!(!would_loop(&p, tid, sid));
        // A loop already in a file: the walk stops instead of recursing.
        if let Some(DocumentData::Assembly(a)) = p.document_mut(sid).map(|d| &mut d.data) {
            a.place(DocRef::local(tid), "Top", Transform3::identity(), false);
        }
        let n = leaves(&p, tid, top(&p, tid)).len();
        assert_eq!(n, 5);
    }
}
