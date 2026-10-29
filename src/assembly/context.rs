//! Update of cross-part references (spec 3.10 §3.3, T3): each part's
//! [`ContextRef`](crate::part::context::ContextRef) copies are compared with their sources in the assembly
//! they were made in, and changed copies are written into the part, which
//! then regenerates (the part never looks its sources up itself, rule 8).

use super::relation::{self, Geom, GeomRef};
use super::tree::{self, Linked};
use crate::document::{AssemblyDocument, DocumentData, DocumentId, Project};
use crate::kernel::{Plane, Point3, Transform3, Vector3};
use crate::part::context::{ContextGeom, ContextSource};

/// What an Update changed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    /// Parts whose copies changed (regenerated).
    pub changed: Vec<DocumentId>,
    /// References whose source could not be read: `<label>: <why>` (their
    /// last copies stay, rule 5).
    pub sick: Vec<String>,
}

/// The placement of the part at occurrence `path` of `asm` in the
/// assembly's coordinates, and that part's component.
fn placed<'a>(
    project: &'a Project,
    linked: &'a Linked,
    asm: &'a AssemblyDocument,
    path: &[u32],
) -> Result<(tree::Nested<'a>, Transform3), String> {
    let (&top, sub) = path.split_first().ok_or("an empty occurrence path")?;
    let n = tree::nested(project, linked, asm, top, sub)?;
    let t = asm
        .find(top)
        .ok_or("the component no longer exists")?
        .transform
        * n.at;
    Ok((n, t))
}

/// The geometry of `source` now, in the coordinates of its target part: a
/// planar face as the plane through the target's origin projected onto it
/// (the sketch origin stays put when the face only grows), its x axis the
/// face default (spec 3.10 §3.3).
pub fn current(
    project: &Project,
    linked: &Linked,
    asm: &AssemblyDocument,
    source: &ContextSource,
) -> Result<ContextGeom, String> {
    let (src, ts) = placed(project, linked, asm, &source.path)?;
    let part = tree::part_of(project, linked, src.component)
        .ok_or_else(|| format!("{} shows no part", src.name))?;
    let face = GeomRef::Face {
        occurrence: 0,
        sub: Vec::new(),
        name: source.face.clone(),
        hint: source.hint,
    };
    let Geom::Plane { p, n } = relation::resolve_in(part, &face)? else {
        return Err(format!("the face {} is not planar", source.face));
    };
    let (_, tt) = placed(project, linked, asm, &source.target)?;
    let m = tt.inverse() * ts;
    let (p, n): (Point3, Vector3) = (m * p, (m * n).normalize());
    let origin = Point3::origin() - n * n.dot(&(Point3::origin() - p));
    let plane = Plane::new(origin, n, crate::part::placement::default_face_x(&n))
        .map_err(|e| e.to_string())?;
    Ok(ContextGeom::Plane { plane })
}

/// New copies per part: (part, [(reference id, geometry)]).
type Writes = Vec<(DocumentId, Vec<(String, ContextGeom)>)>;

/// The current copies of the references made in assembly `asm_id`, part
/// by part (only those that differ from the stored ones when `changed`),
/// and the sources that could not be read.
fn copies(
    project: &Project,
    linked: &Linked,
    asm_id: DocumentId,
    changed: bool,
) -> (Writes, Vec<String>) {
    let mut sick = Vec::new();
    let Some(DocumentData::Assembly(asm)) = project.document(asm_id).map(|d| &d.data) else {
        return (Vec::new(), sick);
    };
    let mut writes = Vec::new();
    for leaf in tree::leaves(project, linked, crate::document::DocRef::local(asm_id), asm) {
        if leaf.component.library.is_some() {
            continue;
        }
        let Some(part) = tree::part_of(project, linked, leaf.component) else {
            continue;
        };
        let mut copies = Vec::new();
        for c in part
            .context
            .iter()
            .filter(|c| c.source.assembly == asm_id && c.source.target == leaf.path)
        {
            match current(project, linked, asm, &c.source) {
                Ok(g) if changed && g.same_as(&c.geom) => {}
                Ok(g) => copies.push((c.id.clone(), g)),
                Err(e) => sick.push(format!("{}: {e}", c.source.label)),
            }
        }
        if !copies.is_empty() {
            writes.push((leaf.component.document, copies));
        }
    }
    (writes, sick)
}

/// The references made in assembly `asm_id` whose source moved or changed
/// since their copies were taken (the Update button's out-of-date state):
/// their labels.
pub fn out_of_date(project: &Project, linked: &Linked, asm_id: DocumentId) -> Vec<String> {
    let (writes, _) = copies(project, linked, asm_id, true);
    let mut labels = Vec::new();
    for (doc, cs) in writes {
        if let Some(DocumentData::Part(p)) = project.document(doc).map(|d| &d.data) {
            labels.extend(
                cs.iter()
                    .filter_map(|(id, _)| p.context_ref(id).map(|c| c.source.label.clone())),
            );
        }
    }
    labels
}

/// Most rounds of [`settle`]: a reference whose part moves its own source
/// (through the constraints) never settles.
pub const SETTLE_ROUNDS: usize = 8;

/// Update, then `solve` (the assembly's constraints, which may move the
/// parts just rebuilt), until nothing changes — at most [`SETTLE_ROUNDS`]
/// rounds. `Err` with the parts still changing when it does not settle.
pub fn settle(
    project: &mut Project,
    linked: &Linked,
    asm_id: DocumentId,
    mut solve: impl FnMut(&mut Project),
) -> Result<Report, Report> {
    let mut total = Report::default();
    for _ in 0..SETTLE_ROUNDS {
        let r = update(project, linked, asm_id);
        total.sick = r.sick;
        if r.changed.is_empty() {
            return Ok(total);
        }
        for d in r.changed {
            if !total.changed.contains(&d) {
                total.changed.push(d);
            }
        }
        solve(project);
    }
    Err(total)
}

/// Update of assembly `asm_id` of `project`: the parts of this design file
/// placed in it get the current copies of the references made in it.
pub fn update(project: &mut Project, linked: &Linked, asm_id: DocumentId) -> Report {
    let (writes, sick) = copies(project, linked, asm_id, false);
    let mut report = Report {
        sick,
        ..Report::default()
    };
    for (doc, copies) in writes {
        if let Some(DocumentData::Part(p)) = project.document_mut(doc).map(|d| &mut d.data)
            && p.set_context(&copies)
        {
            // A failing feature is its own health; the part still builds.
            let _ = p.regenerate();
            report.changed.push(doc);
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::DocRef;
    use crate::part::context::ContextRef;
    use crate::part::{PartDocument, PlaneRef, SketchPlacement};

    /// A 100 × 100 × 10 base and a second part standing on it, whose sketch
    /// lies on the base's top face through a cross-part reference.
    fn design() -> (Project, DocumentId, DocumentId) {
        let mut p = Project::new("T");
        let base = PartDocument::with_box("Box1", [100.0, 100.0, 10.0]).unwrap();
        let b = p.add_document("Base", DocumentData::Part(base));
        let n = p.add_document("New", DocumentData::Part(PartDocument::default()));
        let mut a = AssemblyDocument::default();
        a.place(DocRef::local(b), "Base", Transform3::identity(), true);
        a.place(
            DocRef::local(n),
            "New",
            Transform3::translation(0.0, 0.0, 30.0),
            false,
        );
        let aid = p.add_document("Top", DocumentData::Assembly(a.clone()));
        let source = ContextSource {
            assembly: aid,
            target: vec![2],
            path: vec![1],
            face: "Box1:Top".into(),
            hint: [50.0, 50.0, 10.0],
            label: "Base:1 face".into(),
        };
        let geom = current(&p, &Linked::new(), &a, &source).unwrap();
        if let Some(DocumentData::Part(part)) = p.document_mut(n).map(|d| &mut d.data) {
            let id = part.add_context(ContextRef {
                id: String::new(),
                source,
                geom,
            });
            let placement = SketchPlacement {
                plane: PlaneRef::Context(id),
                ..Default::default()
            };
            part.create_sketch(placement, &Default::default()).unwrap();
        }
        (p, aid, n)
    }

    fn sketch_z(p: &Project, n: DocumentId) -> f64 {
        match p.document(n).map(|d| &d.data) {
            Some(DocumentData::Part(part)) => part.sketches[0].sketch.plane.origin.z,
            _ => f64::NAN,
        }
    }

    #[test]
    fn a_moved_source_moves_the_copy_on_update() {
        let (mut p, aid, n) = design();
        // The base's top (z = 10) seen from the part placed at z = 30.
        assert!((sketch_z(&p, n) + 20.0).abs() < 1e-9);
        // Nothing moved: Update changes nothing.
        assert_eq!(update(&mut p, &Linked::new(), aid), Report::default());
        // The base rises by 5: the sketch follows after Update.
        if let Some(DocumentData::Assembly(a)) = p.document_mut(aid).map(|d| &mut d.data) {
            a.find_mut(1).unwrap().transform = Transform3::translation(0.0, 0.0, 5.0);
        }
        let r = update(&mut p, &Linked::new(), aid);
        assert_eq!(r.changed, vec![n]);
        assert!((sketch_z(&p, n) + 15.0).abs() < 1e-9);
    }

    #[test]
    fn a_moved_source_is_out_of_date_until_update() {
        let (mut p, aid, _) = design();
        assert!(out_of_date(&p, &Linked::new(), aid).is_empty());
        if let Some(DocumentData::Assembly(a)) = p.document_mut(aid).map(|d| &mut d.data) {
            a.find_mut(1).unwrap().transform = Transform3::translation(0.0, 0.0, 5.0);
        }
        assert_eq!(out_of_date(&p, &Linked::new(), aid), vec!["Base:1 face"]);
        update(&mut p, &Linked::new(), aid);
        assert!(out_of_date(&p, &Linked::new(), aid).is_empty());
    }

    #[test]
    fn settle_stops_when_nothing_changes_and_gives_up_on_a_loop() {
        let (mut p, aid, n) = design();
        let lift = |p: &mut Project, dz: f64| {
            if let Some(DocumentData::Assembly(a)) = p.document_mut(aid).map(|d| &mut d.data) {
                let o = a.find_mut(1).unwrap();
                o.transform = Transform3::translation(0.0, 0.0, dz) * o.transform;
            }
        };
        // One move: one round changes the part, the next finds it settled.
        lift(&mut p, 5.0);
        let mut solves = 0;
        let r = settle(&mut p, &Linked::new(), aid, |_| solves += 1).unwrap();
        assert_eq!((r.changed, solves), (vec![n], 1));
        assert!((sketch_z(&p, n) + 15.0).abs() < 1e-9);
        // A "solve" that moves the source every time never settles.
        lift(&mut p, 1.0);
        let r = settle(&mut p, &Linked::new(), aid, |p| lift(p, 1.0));
        assert!(r.is_err());
    }

    #[test]
    fn a_lost_source_keeps_the_last_copy_and_is_reported() {
        let (mut p, aid, n) = design();
        if let Some(DocumentData::Assembly(a)) = p.document_mut(aid).map(|d| &mut d.data) {
            a.remove(1);
        }
        let r = update(&mut p, &Linked::new(), aid);
        assert!(r.changed.is_empty() && r.sick.len() == 1, "{r:?}");
        assert!((sketch_z(&p, n) + 20.0).abs() < 1e-9);
    }
}
