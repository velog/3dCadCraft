//! Assembly features (spec 3.10 §3.8) on real cases: a row of shelf-pin
//! holes drilled through both sides of a cabinet after assembly (the side
//! panel's own document keeps no hole), a participant removed, a tool
//! removed, a tool reaching nothing, and a table's dowel hole through a leg
//! turned in the assembly.

use cadcraft3d::assembly::features::{self, AsmFeatures};
use cadcraft3d::assembly::tree::Linked;
use cadcraft3d::document::{AssemblyDocument, DocRef, DocumentData, DocumentId, Project};
use cadcraft3d::kernel::{Point3, Real, Transform3, Vector3};
use cadcraft3d::part::{
    ExtentDirection, ExtrudeFeature, OriginPlane, PartDocument, ProfileRef, SketchOptions,
    SketchPlacement,
};

fn opts() -> SketchOptions {
    SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    }
}

fn volume(p: &PartDocument) -> Real {
    p.exact_bodies
        .0
        .iter()
        .filter_map(|s| s.volume_at(1e-4))
        .sum()
}

/// Two 18 × 300 × 600 sides 400 apart (inner faces at x 18 and 418).
fn cabinet() -> (Project, DocumentId, DocumentId) {
    let mut p = Project::new("Cabinet");
    let side = p.add_document(
        "Side",
        DocumentData::Part(PartDocument::with_box("Box1", [18.0, 300.0, 600.0]).unwrap()),
    );
    let mut a = AssemblyDocument::default();
    a.place(DocRef::local(side), "Side", Transform3::identity(), true);
    a.place(
        DocRef::local(side),
        "Side",
        Transform3::translation(418.0, 0.0, 0.0),
        true,
    );
    let aid = p.add_document("Cabinet", DocumentData::Assembly(a));
    (p, aid, side)
}

fn tools_part_mut(p: &mut Project, asm: DocumentId) -> &mut PartDocument {
    let (_, doc) = features::ensure_tools(p, asm).unwrap();
    match p.document_mut(doc).map(|d| &mut d.data) {
        Some(DocumentData::Part(x)) => x,
        _ => panic!(),
    }
}

/// In the tools part: Ø5 cylinders along X through the whole cabinet at
/// heights `zs` (a row of shelf-pin holes), one body.
fn pin_row(tp: &mut PartDocument, zs: &[Real]) {
    let i = tp
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::YZ), &opts())
        .unwrap();
    let plane = tp.sketches[i].sketch.plane.clone();
    for z in zs {
        let c = plane.project(&Point3::new(-20.0, 37.0, *z));
        tp.sketches[i].sketch.add_circle(c, 2.5).unwrap();
    }
    let rg = tp.sketches[i].sketch.regions();
    let profiles: Vec<ProfileRef> = rg
        .iter()
        .map(|r| ProfileRef::of(r, r.interior_point()))
        .collect();
    let mut e = ExtrudeFeature::new(profiles, 1000.0);
    e.direction = ExtentDirection::Symmetric;
    tp.add_extrude(i, e).unwrap();
}

fn asm(p: &Project, id: DocumentId) -> &AssemblyDocument {
    match p.document(id).map(|d| &d.data) {
        Some(DocumentData::Assembly(a)) => a,
        _ => panic!(),
    }
}

/// The shown part of occurrence `path` (cut when tools reach it).
fn shown(p: &Project, aid: DocumentId, path: &[u32]) -> Option<PartDocument> {
    let a = asm(p, aid);
    let f = a.features.as_ref()?;
    let tp = match p
        .document(a.find(f.occurrence)?.component.document)
        .map(|d| &d.data)
    {
        Some(DocumentData::Part(x)) => x,
        _ => return None,
    };
    let tools = features::tools_for(a, tp, path);
    let o = a.find(path[0])?;
    let part = match p.document(o.component.document).map(|d| &d.data) {
        Some(DocumentData::Part(x)) => x,
        _ => return None,
    };
    features::cut(&features::built(part), &o.transform, &tools).unwrap()
}

#[test]
fn shelf_pin_holes_go_through_both_sides_only_in_the_assembly() {
    let (mut p, aid, side) = cabinet();
    // A document of the file never shown has no bodies built yet (the app
    // found no participants then).
    if let Some(DocumentData::Part(x)) = p.document_mut(side).map(|d| &mut d.data) {
        x.exact_bodies = Default::default();
    }
    pin_row(tools_part_mut(&mut p, aid), &[150.0, 200.0, 250.0, 300.0]);
    assert!(features::refresh(&mut p, &Linked::new(), aid));
    let f: &AsmFeatures = asm(&p, aid).features.as_ref().unwrap();
    assert!(!f.participants.is_empty());
    for ps in f.participants.values() {
        assert_eq!(ps, &vec![vec![1], vec![2]], "both sides");
    }
    let whole = 18.0 * 300.0 * 600.0;
    let hole = std::f64::consts::PI * 2.5 * 2.5 * 18.0;
    for o in [1, 2] {
        let cut = shown(&p, aid, &[o]).expect("cut");
        let v = volume(&cut);
        assert!((v - (whole - 4.0 * hole)).abs() < 1e-2 * hole, "{v}");
    }
    // The side's own document has no hole.
    match p.document(side).map(|d| &d.data) {
        Some(DocumentData::Part(x)) => {
            let mut x = x.clone();
            x.regenerate().unwrap();
            assert!((volume(&x) - whole).abs() < 1e-3);
        }
        _ => panic!(),
    }
    // The tools part is hidden and Reference.
    let o = asm(&p, aid).find(f.occurrence).unwrap();
    assert!(!o.visible && o.grounded);
}

#[test]
fn a_participant_removed_keeps_its_side_whole() {
    let (mut p, aid, _) = cabinet();
    pin_row(tools_part_mut(&mut p, aid), &[200.0]);
    features::refresh(&mut p, &Linked::new(), aid);
    if let Some(DocumentData::Assembly(a)) = p.document_mut(aid).map(|d| &mut d.data) {
        for ps in a.features.as_mut().unwrap().participants.values_mut() {
            ps.retain(|x| x != &vec![2]);
        }
    }
    assert!(shown(&p, aid, &[1]).is_some());
    assert!(shown(&p, aid, &[2]).is_none(), "the right side is not cut");
    // A refresh does not bring it back (the user's choice stays).
    assert!(!features::refresh(&mut p, &Linked::new(), aid));
}

#[test]
fn a_removed_tool_and_a_tool_reaching_nothing() {
    let (mut p, aid, _) = cabinet();
    // Far above the cabinet: no participant.
    pin_row(tools_part_mut(&mut p, aid), &[2000.0]);
    features::refresh(&mut p, &Linked::new(), aid);
    let f = asm(&p, aid).features.clone().unwrap();
    assert!(f.participants.values().all(|v| v.is_empty()), "{f:?}");
    // The tool's feature deleted: its entry goes.
    tools_part_mut(&mut p, aid).features.clear();
    tools_part_mut(&mut p, aid).regenerate().unwrap();
    features::refresh(&mut p, &Linked::new(), aid);
    assert!(
        asm(&p, aid)
            .features
            .as_ref()
            .unwrap()
            .participants
            .is_empty()
    );
}

#[test]
fn a_dowel_hole_through_a_turned_table_leg() {
    // A 50 × 50 × 700 leg laid on its side (turned 90° about Y); a Ø8 hole
    // drilled down Z through its middle.
    let mut p = Project::new("Table");
    let leg = p.add_document(
        "Leg",
        DocumentData::Part(PartDocument::with_box("Box1", [50.0, 50.0, 700.0]).unwrap()),
    );
    let mut a = AssemblyDocument::default();
    let turned = Transform3::new(
        Vector3::new(0.0, 0.0, 50.0),
        Vector3::y() * std::f64::consts::FRAC_PI_2,
    );
    a.place(DocRef::local(leg), "Leg", turned, true);
    let aid = p.add_document("Table", DocumentData::Assembly(a));
    let tp = tools_part_mut(&mut p, aid);
    let i = tp
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    let plane = tp.sketches[i].sketch.plane.clone();
    let c = plane.project(&Point3::new(350.0, 25.0, 0.0));
    tp.sketches[i].sketch.add_circle(c, 4.0).unwrap();
    let rg = tp.sketches[i].sketch.regions();
    let mut e = ExtrudeFeature::new(vec![ProfileRef::of(&rg[0], rg[0].interior_point())], 200.0);
    e.direction = ExtentDirection::Symmetric;
    tp.add_extrude(i, e).unwrap();
    features::refresh(&mut p, &Linked::new(), aid);
    let cut = shown(&p, aid, &[1]).expect("cut");
    let v = volume(&cut);
    let expect = 50.0 * 50.0 * 700.0 - std::f64::consts::PI * 16.0 * 50.0;
    assert!((v - expect).abs() < 1.0, "{v} vs {expect}");
}
