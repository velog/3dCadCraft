//! The frame as a subassembly (`docs/spec/6.1-frame-generator.md` §3.6a):
//! the first Insert Frame makes an assembly document holding the members and,
//! for each component the members lie on, a hidden Reference occurrence of its
//! part. [`assembly::frame`](super::frame) works inside that document unchanged;
//! this module makes the document, translates references between the two
//! levels and keeps the reference occurrences where their sources are.

use super::frame::{self, Skeleton, Solved, Treatment};
use super::relation::{ASSEMBLY, GeomRef};
use super::tree::{self, Linked};
use crate::document::iprops::BomStructure;
use crate::document::{AssemblyDocument, DocRef, DocumentData, DocumentId, Project};
use crate::kernel::Transform3;
use serde::{Deserialize, Serialize};

/// Ties a reference occurrence of a frame assembly to the component of the
/// parent assembly it stands for. Held by the parent: its ids are the
/// parent's, and the frame assembly must open by itself.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SkeletonLink {
    /// The frame assembly's occurrence in this assembly.
    pub frame: u32,
    /// The reference occurrence inside the frame assembly.
    pub reference: u32,
    /// The source occurrence in this assembly.
    pub source: u32,
    /// The path below it to the part (empty: the occurrence's own part).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sub: Vec<u32>,
}

fn asm_of(project: &Project, id: DocumentId) -> Result<&AssemblyDocument, String> {
    match project.document(id).map(|d| &d.data) {
        Some(DocumentData::Assembly(a)) => Ok(a),
        _ => Err("no assembly is open".into()),
    }
}

fn asm_mut(project: &mut Project, id: DocumentId) -> Result<&mut AssemblyDocument, String> {
    match project.document_mut(id).map(|d| &mut d.data) {
        Some(DocumentData::Assembly(a)) => Ok(a),
        _ => Err("no assembly is open".into()),
    }
}

/// The frame assemblies placed in `top`: occurrence and document, in
/// placement order, each document once.
pub fn frame_occurrences(project: &Project, top: &AssemblyDocument) -> Vec<(u32, DocumentId)> {
    let mut out: Vec<(u32, DocumentId)> = Vec::new();
    for o in &top.occurrences {
        let doc = o.component.document;
        if o.component.library.is_none()
            && !out.iter().any(|(_, d)| *d == doc)
            && matches!(
                project.document(doc).map(|d| &d.data),
                Some(DocumentData::Assembly(a)) if !a.frames.is_empty()
            )
        {
            out.push((o.id, doc));
        }
    }
    out
}

/// The default names of a new frame: the next free `Frame0001`, and its
/// skeleton's.
pub fn default_names(project: &Project) -> (String, String) {
    let free = |base: &str| {
        (1..=9999)
            .map(|n| format!("{base}{n:04}"))
            .find(|n| !project.name_taken(n, None))
            .unwrap_or_else(|| base.to_owned())
    };
    (free("Frame"), free("Skeleton"))
}

/// Makes an empty frame assembly named `name` and places it, grounded at the
/// origin, in assembly `top`: its occurrence and document.
pub fn new_frame_assembly(
    project: &mut Project,
    top: DocumentId,
    name: &str,
) -> Result<(u32, DocumentId), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("the frame needs a name".into());
    }
    if project.name_taken(name, None) {
        return Err(format!("a document named {name} is already in this file"));
    }
    asm_of(project, top)?;
    let doc = project.add_document(
        name.to_owned(),
        DocumentData::Assembly(AssemblyDocument::default()),
    );
    let occ = asm_mut(project, top)?.place(DocRef::local(doc), name, Transform3::identity(), true);
    Ok((occ, doc))
}

/// Where the frame assembly lies and how a reference is named.
#[derive(Clone, Copy, Debug)]
pub struct Target<'a> {
    /// The parent assembly.
    pub top: DocumentId,
    /// The frame assembly's occurrence in it.
    pub frame: u32,
    /// The frame assembly.
    pub doc: DocumentId,
    /// Base of the names of new reference occurrences.
    pub skeleton: &'a str,
}

/// The reference occurrence standing for (`source`, `sub`): the existing one,
/// else a new hidden Reference occurrence at the source's place.
fn reference_of(
    project: &mut Project,
    linked: &Linked,
    t: Target,
    source: u32,
    sub: &[u32],
) -> Result<u32, String> {
    let top = asm_of(project, t.top)?;
    let known = top
        .frame_links
        .iter()
        .find(|l| l.frame == t.frame && l.source == source && l.sub == sub)
        .map(|l| l.reference);
    if let Some(r) = known
        && asm_of(project, t.doc)?.find(r).is_some()
    {
        return Ok(r);
    }
    let n = tree::nested(project, linked, top, source, sub)?;
    let component = n.component;
    let holder = top.find(source).ok_or("the component no longer exists")?;
    let world = holder.transform * n.at;
    let frame = top.find(t.frame).ok_or("the frame no longer exists")?;
    let place = frame.transform.inverse() * world;
    let f = asm_mut(project, t.doc)?;
    let reference = f.place(component, t.skeleton, place, true);
    if let Some(o) = f.occurrences.iter_mut().find(|o| o.id == reference) {
        o.visible = false;
        o.bom = Some(BomStructure::Reference);
    }
    let link = SkeletonLink {
        frame: t.frame,
        reference,
        source,
        sub: sub.to_vec(),
    };
    let top = asm_mut(project, t.top)?;
    top.frame_links
        .retain(|l| !(l.frame == t.frame && l.source == source && l.sub == sub));
    top.frame_links.push(link);
    Ok(reference)
}

/// A reference picked in the parent, in the frame assembly's terms: inside the
/// frame, the same geometry one level down; elsewhere, on the source's
/// reference occurrence (made if needed).
pub fn geom_down(
    project: &mut Project,
    linked: &Linked,
    t: Target,
    g: &GeomRef,
) -> Result<GeomRef, String> {
    let mut out = g.clone();
    if g.occurrence() == ASSEMBLY {
        return Ok(out);
    }
    if g.occurrence() == t.frame {
        let (first, rest) = g
            .sub()
            .split_first()
            .ok_or("the frame itself cannot be used here")?;
        *out.occurrence_mut() = *first;
        *out.sub_mut() = rest.to_vec();
    } else {
        *out.occurrence_mut() = reference_of(project, linked, t, g.occurrence(), g.sub())?;
        out.sub_mut().clear();
    }
    Ok(out)
}

/// A skeleton line picked in the parent, in the frame assembly's terms.
pub fn skeleton_down(
    project: &mut Project,
    linked: &Linked,
    t: Target,
    s: &Skeleton,
) -> Result<Skeleton, String> {
    Ok(match s {
        Skeleton::Edge(g) => Skeleton::Edge(geom_down(project, linked, t, g)?),
        Skeleton::SketchLine {
            occurrence,
            sub,
            sketch,
            line,
        } => {
            let (occurrence, sub) = if *occurrence == t.frame {
                let (first, rest) = sub
                    .split_first()
                    .ok_or("the frame itself cannot be used here")?;
                (*first, rest.to_vec())
            } else {
                (
                    reference_of(project, linked, t, *occurrence, sub)?,
                    Vec::new(),
                )
            };
            Skeleton::SketchLine {
                occurrence,
                sub,
                sketch: sketch.clone(),
                line: *line,
            }
        }
        Skeleton::Between {
            from,
            from_end,
            to,
            to_end,
        } => Skeleton::Between {
            from: Box::new(skeleton_down(project, linked, t, from)?),
            from_end: *from_end,
            to: Box::new(skeleton_down(project, linked, t, to)?),
            to_end: *to_end,
        },
        Skeleton::Chain(list) => Skeleton::Chain(
            list.iter()
                .map(|x| skeleton_down(project, linked, t, x))
                .collect::<Result<_, _>>()?,
        ),
    })
}

/// End treatments defined in the parent's terms, in the frame assembly's.
pub fn treatments_down(
    project: &mut Project,
    linked: &Linked,
    t: Target,
    list: Vec<Treatment>,
) -> Result<Vec<Treatment>, String> {
    list.into_iter()
        .map(|x| {
            Ok(match x {
                Treatment::TrimToFace {
                    member,
                    face,
                    offset,
                } => Treatment::TrimToFace {
                    member,
                    face: geom_down(project, linked, t, &face)?,
                    offset,
                },
                other => other,
            })
        })
        .collect()
}

/// Where (occurrence, path) of the frame assembly lies in the parent's terms.
fn up(top: &AssemblyDocument, frame: u32, occurrence: u32, sub: &[u32]) -> (u32, Vec<u32>) {
    match top
        .frame_links
        .iter()
        .find(|l| l.frame == frame && l.reference == occurrence)
    {
        Some(l) => (l.source, l.sub.iter().chain(sub).copied().collect()),
        None => (
            frame,
            std::iter::once(occurrence)
                .chain(sub.iter().copied())
                .collect(),
        ),
    }
}

/// [`geom_down`]'s inverse, to show a stored reference in the parent's view.
pub fn geom_up(top: &AssemblyDocument, frame: u32, g: &GeomRef) -> GeomRef {
    let mut out = g.clone();
    if g.occurrence() != ASSEMBLY {
        let (occurrence, sub) = up(top, frame, g.occurrence(), g.sub());
        *out.occurrence_mut() = occurrence;
        *out.sub_mut() = sub;
    }
    out
}

/// [`skeleton_down`]'s inverse.
pub fn skeleton_up(top: &AssemblyDocument, frame: u32, s: &Skeleton) -> Skeleton {
    match s {
        Skeleton::Edge(g) => Skeleton::Edge(geom_up(top, frame, g)),
        Skeleton::SketchLine {
            occurrence,
            sub,
            sketch,
            line,
        } => {
            let (occurrence, sub) = up(top, frame, *occurrence, sub);
            Skeleton::SketchLine {
                occurrence,
                sub,
                sketch: sketch.clone(),
                line: *line,
            }
        }
        Skeleton::Between {
            from,
            from_end,
            to,
            to_end,
        } => Skeleton::Between {
            from: Box::new(skeleton_up(top, frame, from)),
            from_end: *from_end,
            to: Box::new(skeleton_up(top, frame, to)),
            to_end: *to_end,
        },
        Skeleton::Chain(list) => {
            Skeleton::Chain(list.iter().map(|x| skeleton_up(top, frame, x)).collect())
        }
    }
}

/// Places every reference occurrence where its source is now (frame
/// coordinates), standing in the source's variant. A source that is gone leaves its copy: the members stay
/// valid. Returns how many moved.
fn sync_links(project: &mut Project, linked: &Linked, top: DocumentId) -> Result<usize, String> {
    let a = asm_of(project, top)?;
    let mut moves: Vec<(DocumentId, u32, Transform3, Option<u32>)> = Vec::new();
    for l in &a.frame_links {
        let Some(frame) = a.find(l.frame) else {
            continue;
        };
        let Some(source) = a.find(l.source) else {
            log::warn!("frame skeleton: its source component {} is gone", l.source);
            continue;
        };
        match tree::nested(project, linked, a, l.source, &l.sub) {
            Ok(n) => moves.push((
                frame.component.document,
                l.reference,
                frame.transform.inverse() * source.transform * n.at,
                n.component.variant,
            )),
            Err(e) => log::warn!("frame skeleton: {e}"),
        }
    }
    let mut moved = 0;
    for (doc, reference, place, variant) in moves {
        if let Some(o) = asm_mut(project, doc)?
            .occurrences
            .iter_mut()
            .find(|o| o.id == reference)
        {
            let d = (o.transform.translation.vector - place.translation.vector).norm()
                + o.transform.rotation.angle_to(&place.rotation);
            if d > 1e-9 {
                o.transform = place;
                moved += 1;
            }
            // The skeleton stands in the variant its source places (spec 6.1
            // §3.6a): the members follow that variant's lines.
            if o.component.variant != variant {
                o.component.variant = variant;
                moved += 1;
            }
        }
    }
    Ok(moved)
}

/// Brings the frames of assembly `top` up to date: the skeleton references
/// follow their sources, then every frame assembly and any frame stored
/// flat in `top` itself are solved.
pub fn follow(project: &mut Project, linked: &Linked, top: DocumentId) -> Result<Solved, String> {
    let moved = sync_links(project, linked, top)?;
    let mut out = Solved {
        moved,
        ..Default::default()
    };
    let (docs, flat) = {
        let a = asm_of(project, top)?;
        (frame_occurrences(project, a), !a.frames.is_empty())
    };
    let mut merge = |s: Solved| {
        out.moved += s.moved;
        out.sick.extend(s.sick);
    };
    for (_, doc) in docs {
        merge(frame::solve(project, linked, doc)?);
    }
    if flat {
        merge(frame::solve(project, linked, top)?);
    }
    Ok(out)
}

/// A frame assembly left without members goes: its occurrence, its links and
/// its document.
pub fn prune(project: &mut Project, top: DocumentId) -> Result<(), String> {
    let empty: Vec<(u32, DocumentId)> = {
        let a = asm_of(project, top)?;
        let linked_to: Vec<u32> = a.frame_links.iter().map(|l| l.frame).collect();
        a.occurrences
            .iter()
            .filter(|o| linked_to.contains(&o.id) && o.component.library.is_none())
            .filter(|o| {
                matches!(
                    project.document(o.component.document).map(|d| &d.data),
                    Some(DocumentData::Assembly(f))
                        if f.frames.iter().all(|x| x.members.is_empty())
                )
            })
            .map(|o| (o.id, o.component.document))
            .collect()
    };
    for (occ, doc) in empty {
        let a = asm_mut(project, top)?;
        a.remove(occ);
        a.frame_links.retain(|l| l.frame != occ);
        project.delete_document(doc).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assembly::frame::{CornerType, Corners, InsertOptions, Layout, Orientation};
    use crate::document::Occurrence;
    use crate::kernel::{Point3, Real, Vector3};
    use crate::part::frame::Library;
    use crate::part::{EdgeRef, PartDocument};

    /// A top assembly with a box (occurrence 1) moved by `at`.
    fn project(at: Vector3) -> (Project, DocumentId) {
        let mut p = Project::new("T");
        let sk = PartDocument::with_box("Box1", [400.0, 300.0, 200.0]).unwrap();
        let sk = p.add_document("Skeleton part", DocumentData::Part(sk));
        let mut a = AssemblyDocument::default();
        a.place(
            DocRef::local(sk),
            "Box",
            Transform3::from_parts(at.into(), Default::default()),
            true,
        );
        let top = p.add_document("Top", DocumentData::Assembly(a));
        (p, top)
    }

    fn edge(hint: [Real; 3], f0: &str, f1: &str) -> Skeleton {
        Skeleton::Edge(GeomRef::Edge {
            occurrence: 1,
            sub: Vec::new(),
            edge: EdgeRef {
                faces: [format!("Box1:{f0}"), format!("Box1:{f1}")],
                hint,
            },
        })
    }

    fn opts(corners: Corners) -> InsertOptions {
        InsertOptions {
            family: "Square hollow section (EN 10219)".into(),
            size: "40x40x3".into(),
            material: Some("Steel S235".into()),
            orientation: Orientation::default(),
            corners,
            layout: Layout::default(),
        }
    }

    /// The first Insert: a frame assembly with the members and one hidden
    /// reference, whatever the number of edges on one component.
    fn insert(p: &mut Project, top: DocumentId, c: Corners) -> (Target<'static>, Vec<u32>) {
        let linked = Linked::new();
        let (frame, doc) = new_frame_assembly(p, top, "Frame0001").unwrap();
        let t = Target {
            top,
            frame,
            doc,
            skeleton: "Skeleton0001",
        };
        let lines = [
            edge([200.0, 0.0, 200.0], "Front", "Top"),
            edge([400.0, 150.0, 200.0], "Right", "Top"),
        ];
        let down: Vec<Skeleton> = lines
            .iter()
            .map(|s| skeleton_down(p, &linked, t, s).unwrap())
            .collect();
        let (_, occs) =
            frame::insert(p, &linked, doc, &down, &opts(c), Library::builtin(), None).unwrap();
        (t, occs)
    }

    fn assembly(p: &Project, id: DocumentId) -> &AssemblyDocument {
        asm_of(p, id).unwrap()
    }

    fn world_of(p: &Project, t: Target, occ: u32) -> Point3 {
        let top = assembly(p, t.top);
        let f: &Occurrence = top.find(t.frame).unwrap();
        (f.transform * assembly(p, t.doc).find(occ).unwrap().transform) * Point3::origin()
    }

    #[test]
    fn insert_makes_a_grounded_frame_assembly_with_one_hidden_reference() {
        let (mut p, top) = project(Vector3::new(10.0, 20.0, 30.0));
        let (t, occs) = insert(&mut p, top, Corners::default());
        let a = assembly(&p, top);
        assert_eq!(a.occurrences.len(), 2);
        assert!(a.find(t.frame).unwrap().grounded);
        assert_eq!(a.frame_links.len(), 1);
        assert_eq!(frame_occurrences(&p, a), vec![(t.frame, t.doc)]);
        let f = assembly(&p, t.doc);
        assert_eq!(f.occurrences.len(), 3, "two members and one reference");
        assert_eq!(f.frames.len(), 1);
        assert_eq!(f.frames[0].members.len(), 2);
        let r = f.find(a.frame_links[0].reference).unwrap();
        assert!(!r.visible);
        assert_eq!(r.bom, Some(BomStructure::Reference));
        assert_eq!(
            r.transform.translation.vector,
            Vector3::new(10.0, 20.0, 30.0)
        );
        assert_eq!(occs.len(), 2);
    }

    #[test]
    fn members_follow_a_source_that_moves_in_the_parent() {
        let (mut p, top) = project(Vector3::zeros());
        let (t, occs) = insert(&mut p, top, Corners::default());
        let before = world_of(&p, t, occs[0]);
        let d = Vector3::new(50.0, -30.0, 70.0);
        asm_mut(&mut p, top).unwrap().occurrences[0].transform =
            Transform3::from_parts(d.into(), Default::default());
        let s = follow(&mut p, &Linked::new(), top).unwrap();
        assert!(s.sick.is_empty(), "{:?}", s.sick);
        assert!(
            s.moved >= 3,
            "reference and both members moved: {}",
            s.moved
        );
        assert!((world_of(&p, t, occs[0]) - (before + d)).norm() < 1e-6);
        // Nothing left to do.
        assert_eq!(follow(&mut p, &Linked::new(), top).unwrap().moved, 0);
    }

    #[test]
    fn a_moved_frame_assembly_keeps_its_members_on_the_source() {
        let (mut p, top) = project(Vector3::zeros());
        let (t, occs) = insert(&mut p, top, Corners::default());
        let before = world_of(&p, t, occs[0]);
        let a = asm_mut(&mut p, top).unwrap();
        let f = a.occurrences.iter_mut().find(|o| o.id == t.frame).unwrap();
        f.transform =
            Transform3::from_parts(Vector3::new(5.0, 6.0, 7.0).into(), Default::default());
        follow(&mut p, &Linked::new(), top).unwrap();
        assert!((world_of(&p, t, occs[0]) - before).norm() < 1e-6);
    }

    #[test]
    fn corners_are_solved_inside_the_frame_assembly() {
        let (mut p, top) = project(Vector3::zeros());
        let c = Corners {
            kind: CornerType::Miter,
            ..Default::default()
        };
        let (t, _) = insert(&mut p, top, c);
        assert_eq!(assembly(&p, t.doc).frames[0].treatments.len(), 1);
    }

    #[test]
    fn references_translate_down_and_up() {
        let (mut p, top) = project(Vector3::zeros());
        let (t, occs) = insert(&mut p, top, Corners::default());
        let linked = Linked::new();
        // The same source again: the same reference, no new occurrence.
        let s = edge([0.0, 150.0, 200.0], "Left", "Top");
        let down = skeleton_down(&mut p, &linked, t, &s).unwrap();
        assert_eq!(assembly(&p, t.doc).occurrences.len(), 3);
        assert_eq!(skeleton_up(assembly(&p, top), t.frame, &down), s);
        // A face of a member, seen from the parent, is one level deeper.
        let g = GeomRef::Face {
            occurrence: t.frame,
            sub: vec![occs[0]],
            name: "E:Side".into(),
            hint: [0.0; 3],
        };
        let d = geom_down(&mut p, &linked, t, &g).unwrap();
        assert_eq!((d.occurrence(), d.sub().len()), (occs[0], 0));
        assert_eq!(geom_up(assembly(&p, top), t.frame, &d), g);
        // The frame itself is no geometry.
        let bare = GeomRef::Face {
            occurrence: t.frame,
            sub: Vec::new(),
            name: "x".into(),
            hint: [0.0; 3],
        };
        assert!(geom_down(&mut p, &linked, t, &bare).is_err());
    }

    #[test]
    fn deleting_every_member_removes_the_frame_assembly() {
        let (mut p, top) = project(Vector3::zeros());
        let (t, occs) = insert(&mut p, top, Corners::default());
        let linked = Linked::new();
        frame::delete_members(&mut p, &linked, t.doc, &occs[..1]).unwrap();
        prune(&mut p, top).unwrap();
        assert!(p.document(t.doc).is_some(), "one member is left");
        frame::delete_members(&mut p, &linked, t.doc, &occs[1..]).unwrap();
        prune(&mut p, top).unwrap();
        assert!(p.document(t.doc).is_none());
        let a = assembly(&p, top);
        assert_eq!(a.occurrences.len(), 1);
        assert!(a.frame_links.is_empty());
    }

    #[test]
    fn a_name_in_use_is_refused_and_nothing_changes() {
        let (mut p, top) = project(Vector3::zeros());
        let docs = p.documents().len();
        assert!(new_frame_assembly(&mut p, top, "Top").is_err());
        assert!(new_frame_assembly(&mut p, top, "  ").is_err());
        assert_eq!(p.documents().len(), docs);
        assert_eq!(assembly(&p, top).occurrences.len(), 1);
        assert_eq!(default_names(&p).0, "Frame0001");
    }

    #[test]
    fn the_frame_assembly_survives_save_and_load() {
        let (mut p, top) = project(Vector3::new(1.0, 2.0, 3.0));
        let (t, occs) = insert(&mut p, top, Corners::default());
        let dir = std::env::temp_dir().join(format!("cc3d_frame_sub_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("f.3dcadcraft");
        p.save(&file).unwrap();
        let mut q = Project::load(&file).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(assembly(&q, top).frame_links.len(), 1);
        let s = follow(&mut q, &Linked::new(), top).unwrap();
        assert!(s.sick.is_empty(), "{:?}", s.sick);
        assert_eq!(s.moved, 0);
        assert_eq!(assembly(&q, t.doc).frames[0].members[0].occurrence, occs[0]);
    }

    // ---------------------------------------------------- owner's batch 2026-11-12 ---

    /// The skeleton part's parameter holding its x size (400).
    fn x_param(p: &PartDocument) -> String {
        p.parameter_table()
            .into_iter()
            .find(|r| (r.value - 400.0).abs() < 1e-9)
            .map(|r| r.name)
            .unwrap()
    }

    fn part_mut(p: &mut Project, id: DocumentId) -> &mut PartDocument {
        match p.document_mut(id).map(|d| &mut d.data) {
            Some(DocumentData::Part(x)) => x,
            _ => panic!("no part"),
        }
    }

    /// The x extent of member `occ` in the frame assembly's coordinates.
    fn member_length(p: &Project, t: Target, occ: u32) -> Real {
        let f = assembly(p, t.doc);
        let o = f.find(occ).unwrap();
        match p.document(o.component.document).map(|d| &d.data) {
            Some(DocumentData::Part(m)) => frame::length_of(m).unwrap(),
            _ => panic!("no member part"),
        }
    }

    /// The skeleton part (document) of the top assembly's first occurrence,
    /// with a variant `Long` whose x size is 600 (`[Primary]` stays active).
    fn with_long_variant(p: &mut Project, top: DocumentId) -> (DocumentId, u32) {
        let sk = assembly(p, top).occurrences[0].component.document;
        let part = part_mut(p, sk);
        let name = x_param(part);
        let v = part.new_variant();
        part.set_parameter_expr(&name, "600 mm").unwrap();
        part.variants_sync();
        part.activate_variant(crate::part::variants::PRIMARY)
            .unwrap();
        (sk, v)
    }

    #[test]
    fn members_follow_the_variant_the_skeleton_occurrence_places() {
        let (mut p, top) = project(Vector3::zeros());
        let (t, occs) = insert(&mut p, top, Corners::default());
        assert!((member_length(&p, t, occs[0]) - 400.0).abs() < 1e-6);
        let (_, v) = with_long_variant(&mut p, top);
        // The source occurrence places the long variant.
        asm_mut(&mut p, top).unwrap().occurrences[0]
            .component
            .variant = Some(v);
        let linked = tree::with_variants(&p, &Linked::new());
        let s = follow(&mut p, &linked, top).unwrap();
        assert!(s.sick.is_empty(), "{:?}", s.sick);
        // The reference copies the variant: the next build finds it as well.
        let link = assembly(&p, top).frame_links[0].clone();
        let r = assembly(&p, t.doc).find(link.reference).unwrap();
        assert_eq!(r.component.variant, Some(v));
        let linked = tree::with_variants(&p, &Linked::new());
        follow(&mut p, &linked, top).unwrap();
        assert!(
            (member_length(&p, t, occs[0]) - 600.0).abs() < 1e-6,
            "{}",
            member_length(&p, t, occs[0])
        );
        // The y edge did not change.
        assert!((member_length(&p, t, occs[1]) - 300.0).abs() < 1e-6);
        // Back to the part's active variant: back to 400.
        asm_mut(&mut p, top).unwrap().occurrences[0]
            .component
            .variant = None;
        let linked = tree::with_variants(&p, &Linked::new());
        follow(&mut p, &linked, top).unwrap();
        assert!((member_length(&p, t, occs[0]) - 400.0).abs() < 1e-6);
        // Nothing left to do.
        assert_eq!(follow(&mut p, &linked, top).unwrap().moved, 0);
    }

    #[test]
    fn members_follow_the_skeleton_parts_active_variant() {
        let (mut p, top) = project(Vector3::zeros());
        let (t, occs) = insert(&mut p, top, Corners::default());
        let (sk, v) = with_long_variant(&mut p, top);
        part_mut(&mut p, sk).activate_variant(v).unwrap();
        follow(&mut p, &Linked::new(), top).unwrap();
        assert!((member_length(&p, t, occs[0]) - 600.0).abs() < 1e-6);
        part_mut(&mut p, sk)
            .activate_variant(crate::part::variants::PRIMARY)
            .unwrap();
        follow(&mut p, &Linked::new(), top).unwrap();
        assert!((member_length(&p, t, occs[0]) - 400.0).abs() < 1e-6);
    }

    #[test]
    fn a_frame_on_a_variant_survives_save_and_load() {
        let (mut p, top) = project(Vector3::zeros());
        let (t, occs) = insert(&mut p, top, Corners::default());
        let (_, v) = with_long_variant(&mut p, top);
        asm_mut(&mut p, top).unwrap().occurrences[0]
            .component
            .variant = Some(v);
        for _ in 0..2 {
            let linked = tree::with_variants(&p, &Linked::new());
            follow(&mut p, &linked, top).unwrap();
        }
        let dir = std::env::temp_dir().join(format!("cc3d_frame_var_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("f.3dcadcraft");
        p.save(&file).unwrap();
        let mut q = Project::load(&file).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        let linked = tree::with_variants(&q, &Linked::new());
        let s = follow(&mut q, &linked, top).unwrap();
        assert!(s.sick.is_empty(), "{:?}", s.sick);
        assert!((member_length(&q, t, occs[0]) - 600.0).abs() < 1e-6);
    }

    #[test]
    fn the_same_edge_twice_is_an_overlap_and_a_longer_line_removes_a_shorter() {
        let (mut p, top) = project(Vector3::zeros());
        let (t, occs) = insert(&mut p, top, Corners::default());
        let linked = Linked::new();
        assert!(frame::overlapping_members(&p, &linked, assembly(&p, t.doc)).is_empty());
        // The front top edge once more (picked from the front face's edges).
        let again = skeleton_down(
            &mut p,
            &linked,
            t,
            &edge([200.0, 0.0, 200.0], "Front", "Top"),
        )
        .unwrap();
        let (_, more) = frame::insert(
            &mut p,
            &linked,
            t.doc,
            &[again],
            &opts(Corners::default()),
            Library::builtin(),
            Some(1),
        )
        .unwrap();
        assert_eq!(
            frame::overlapping_members(&p, &linked, assembly(&p, t.doc)),
            vec![(occs[0], more[0])]
        );
    }

    #[test]
    fn set_orientations_moves_a_member_and_refuses_bad_values() {
        let (mut p, top) = project(Vector3::zeros());
        let (t, occs) = insert(&mut p, top, Corners::default());
        let linked = Linked::new();
        let before = world_of(&p, t, occs[0]);
        let o = frame::Orientation {
            anchor: frame::Anchor::BottomLeft,
            angle: std::f64::consts::FRAC_PI_2,
            ..Default::default()
        };
        frame::set_orientations(&mut p, &linked, t.doc, &[(occs[0], o)]).unwrap();
        assert_eq!(frame::orientation_of(assembly(&p, t.doc), occs[0]), Some(o));
        assert!((world_of(&p, t, occs[0]) - before).norm() > 1.0);
        // A value that is no number, or no member: nothing changes.
        let copy = p.clone();
        let bad = frame::Orientation {
            h_offset: Real::NAN,
            ..o
        };
        assert!(frame::set_orientations(&mut p, &linked, t.doc, &[(occs[0], bad)]).is_err());
        assert!(frame::set_orientations(&mut p, &linked, t.doc, &[(9999, o)]).is_err());
        assert!(frame::set_orientations(&mut p, &linked, t.doc, &[]).is_err());
        assert_eq!(
            frame::orientation_of(assembly(&p, t.doc), occs[0]),
            frame::orientation_of(assembly(&copy, t.doc), occs[0])
        );
    }

    #[test]
    fn insert_with_takes_a_members_own_orientation() {
        let (mut p, top) = project(Vector3::zeros());
        let linked = Linked::new();
        let (frame, doc) = new_frame_assembly(&mut p, top, "Frame0001").unwrap();
        let t = Target {
            top,
            frame,
            doc,
            skeleton: "Skeleton0001",
        };
        let lines: Vec<Skeleton> = [
            edge([200.0, 0.0, 200.0], "Front", "Top"),
            edge([400.0, 150.0, 200.0], "Right", "Top"),
        ]
        .iter()
        .map(|s| skeleton_down(&mut p, &linked, t, s).unwrap())
        .collect();
        let mine = frame::Orientation {
            anchor: frame::Anchor::TopRight,
            v_offset: 5.0,
            ..Default::default()
        };
        let (_, occs) = frame::insert_with(
            &mut p,
            &linked,
            doc,
            &lines,
            &[None, Some(mine)],
            &opts(Corners::default()),
            Library::builtin(),
            None,
        )
        .unwrap();
        let f = assembly(&p, doc);
        assert_eq!(
            frame::orientation_of(f, occs[0]),
            Some(Orientation::default())
        );
        assert_eq!(frame::orientation_of(f, occs[1]), Some(mine));
    }

    /// The skeleton box split at x = 200 into two bodies.
    fn split_box() -> PartDocument {
        use crate::part::{
            OriginPlane, PlaneRef, SplitFeature, SplitMethod, SplitToolRef, WorkDef, WorkFeature,
            WorkPlaneDef,
        };
        let mut p = PartDocument::with_box("Box1", [400.0, 300.0, 200.0]).unwrap();
        let wp = p
            .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
                base: PlaneRef::Origin(OriginPlane::YZ),
                distance: 200.0,
            })))
            .unwrap();
        let mut s = SplitFeature::new(SplitMethod::SplitSolid);
        s.tool = Some(SplitToolRef::Plane(PlaneRef::Work(wp)));
        p.add_split(s).unwrap();
        p
    }

    #[test]
    fn body_edges_take_the_clicked_body_only() {
        let p = split_box();
        assert_eq!(p.body.solids().count(), 2);
        let all = frame::straight_edges(&p, None);
        let left = frame::body_at(&p, "Box1:Top", Point3::new(100.0, 150.0, 200.0)).unwrap();
        let right = frame::body_at(&p, "Box1:Top", Point3::new(300.0, 150.0, 200.0)).unwrap();
        assert_ne!(left, right);
        let a = frame::straight_edges_in(&p, None, Some(left));
        let b = frame::straight_edges_in(&p, None, Some(right));
        assert_eq!((a.len(), b.len()), (12, 12), "a box each");
        assert!(a.iter().all(|e| e.hint[0] <= 200.0 + 1e-6));
        assert!(b.iter().all(|e| e.hint[0] >= 200.0 - 1e-6));
        // Every body: the edges of both (those on the cut once each).
        assert!(all.len() >= 20 && all.len() <= 24, "{}", all.len());
    }

    fn l(a: [Real; 3], b: [Real; 3]) -> (Point3, Point3) {
        (Point3::from(a), Point3::from(b))
    }

    #[test]
    fn overlap_of_lines_in_many_layouts() {
        let a = l([0.0, 0.0, 0.0], [100.0, 0.0, 0.0]);
        // On one another: the same, reversed, partly, inside, a hair off.
        for b in [
            a,
            l([100.0, 0.0, 0.0], [0.0, 0.0, 0.0]),
            l([50.0, 0.0, 0.0], [150.0, 0.0, 0.0]),
            l([20.0, 0.0, 0.0], [30.0, 0.0, 0.0]),
            l([-10.0, 0.0, 0.0], [110.0, 0.0, 0.0]),
            l([10.0, 0.0004, 0.0], [90.0, 0.0, 0.0004]),
        ] {
            assert!(frame::lines_overlap(a, b), "{b:?}");
            assert!(frame::lines_overlap(b, a), "{b:?} the other way");
        }
        // Not: end to end, a gap, beside, crossing, skew, a little turned.
        for b in [
            l([100.0, 0.0, 0.0], [200.0, 0.0, 0.0]),
            l([110.0, 0.0, 0.0], [200.0, 0.0, 0.0]),
            l([0.0, 40.0, 0.0], [100.0, 40.0, 0.0]),
            l([50.0, -50.0, 0.0], [50.0, 50.0, 0.0]),
            l([0.0, 0.0, 1.0], [100.0, 1.0, 1.0]),
            l([0.0, 0.0, 0.0], [100.0, 0.5, 0.0]),
            l([5.0, 0.0, 0.0], [5.0, 0.0, 0.0]),
        ] {
            assert!(!frame::lines_overlap(a, b), "{b:?}");
        }
    }

    #[test]
    fn overlap_drops_keep_the_longer_and_the_first() {
        let a = l([0.0, 0.0, 0.0], [100.0, 0.0, 0.0]);
        let short = l([20.0, 0.0, 0.0], [60.0, 0.0, 0.0]);
        let long = l([-50.0, 0.0, 0.0], [150.0, 0.0, 0.0]);
        let other = l([0.0, 10.0, 0.0], [100.0, 10.0, 0.0]);
        // The shorter goes, whichever comes first.
        assert_eq!(frame::overlap_drops(&[Some(a), Some(short)], 0), vec![1]);
        assert_eq!(frame::overlap_drops(&[Some(short), Some(a)], 0), vec![0]);
        // As long: the later one goes.
        assert_eq!(frame::overlap_drops(&[Some(a), Some(a)], 0), vec![1]);
        let rev = l([100.0, 0.0, 0.0], [0.0, 0.0, 0.0]);
        assert_eq!(frame::overlap_drops(&[Some(rev), Some(a)], 0), vec![1]);
        // A chain: the longest covers both.
        assert_eq!(
            frame::overlap_drops(&[Some(a), Some(short), Some(long), Some(other)], 0),
            vec![0, 1]
        );
        // Placed members overlapping each other stay; a longer new line
        // removes a placed one it covers; curves (None) never overlap.
        assert!(frame::overlap_drops(&[Some(a), Some(short)], 2).is_empty());
        assert_eq!(frame::overlap_drops(&[Some(short), Some(long)], 1), vec![0]);
        assert_eq!(frame::overlap_drops(&[Some(a), None, Some(a)], 1), vec![2]);
        assert!(frame::overlap_drops(&[], 0).is_empty());
    }

    #[test]
    fn overlapping_pairs_match_every_pair_compared() {
        // A grid of 20 lines with every third one doubled (shifted along).
        let mut lines = Vec::new();
        for k in 0..20u32 {
            let y = Real::from(k) * 50.0;
            lines.push((k, l([0.0, y, 0.0], [300.0, y, 0.0])));
            if k % 3 == 0 {
                lines.push((100 + k, l([250.0, y, 0.0], [600.0, y, 0.0])));
            }
        }
        let mut brute = Vec::new();
        for i in 0..lines.len() {
            for j in i + 1..lines.len() {
                if frame::lines_overlap(lines[i].1, lines[j].1) {
                    brute.push((lines[i].0, lines[j].0));
                }
            }
        }
        brute.sort_unstable();
        assert_eq!(frame::overlapping_pairs(&lines), brute);
        assert_eq!(brute.len(), 7);
    }

    #[test]
    fn two_touching_bodies_overlap_on_their_shared_edges_only() {
        let p = split_box();
        let left = frame::body_at(&p, "Box1:Top", Point3::new(100.0, 150.0, 200.0)).unwrap();
        let right = frame::body_at(&p, "Box1:Top", Point3::new(300.0, 150.0, 200.0)).unwrap();
        let lines: Vec<(u32, (Point3, Point3))> = frame::straight_edge_lines(&p, None, Some(left))
            .into_iter()
            .chain(frame::straight_edge_lines(&p, None, Some(right)))
            .enumerate()
            .map(|(k, (_, a, b))| (k as u32, (a, b)))
            .collect();
        let pairs = frame::overlapping_pairs(&lines);
        assert_eq!(pairs.len(), 4, "{pairs:?} {lines:?}");
        let all: Vec<_> = lines.iter().map(|(_, l)| Some(*l)).collect();
        assert_eq!(frame::overlap_drops(&all, 0).len(), 4);
        // Each body's edge resolves to itself, not to the other body's piece
        // of the same line with the same face names (the two tie at their
        // shared end when only sample points are compared).
        for (r, a, b) in frame::straight_edge_lines(&p, None, Some(right)) {
            let e = crate::part::draft::edge_of(&p.body, &r).unwrap();
            let pts = p.body.sample_edge(e, &Default::default()).unwrap();
            let m = Point3::from((pts[0].coords + pts[pts.len() - 1].coords) / 2.0);
            assert!((m - (a + (b - a) / 2.0)).norm() < 1e-6, "{r:?}");
        }
    }
}
