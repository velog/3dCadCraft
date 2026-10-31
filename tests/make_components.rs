//! Make Part and Make Components (spec 3.10 §3.6) on a real multi-body
//! master: an enclosure designed as one part — housing, lid, mounting lug —
//! split into parts placed where the bodies are, following the master when
//! it changes.

use cadcraft3d::assembly::make::{self, Target};
use cadcraft3d::document::{DocumentData, DocumentId, Project};
use cadcraft3d::kernel::{Point3, Real};
use cadcraft3d::part::{
    ExtentDirection, ExtrudeFeature, Feature, Operation, PartDocument, ProfileRef, SketchOptions,
    SketchPlacement,
};

fn opts() -> SketchOptions {
    SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    }
}

/// A `w` × 60 × 30 housing (Box1) with a lid 4 thick on top and a lug at
/// its end, each a body of its own (New Solid).
fn master(w: Real) -> PartDocument {
    let mut p = PartDocument::with_box("Box1", [w, 60.0, 30.0]).unwrap();
    for (a, b, h) in [
        ((0.0, 0.0), (w, 60.0), 4.0_f64),
        ((w, 20.0), (w + 15.0, 40.0), -10.0),
    ] {
        let j = p
            .create_sketch(SketchPlacement::on_face("Box1:Top"), &opts())
            .unwrap();
        let plane = p.sketches[j].sketch.plane.clone();
        let pa = plane.project(&Point3::new(a.0, a.1, 30.0));
        let pb = plane.project(&Point3::new(b.0, b.1, 30.0));
        p.sketches[j]
            .sketch
            .add_rectangle_two_point(pa, pb)
            .unwrap();
        let rg = p.sketches[j].sketch.regions();
        let r = rg
            .iter()
            .max_by(|x, y| x.area().total_cmp(&y.area()))
            .unwrap();
        let mut e = ExtrudeFeature::new(vec![ProfileRef::of(r, r.interior_point())], h.abs());
        e.operation = Operation::NewSolid;
        if h < 0.0 {
            e.direction = ExtentDirection::Flipped;
        }
        p.add_extrude(j, e).unwrap();
    }
    assert_eq!(p.exact_bodies.0.len(), 3);
    p
}

fn part(p: &Project, d: DocumentId) -> &PartDocument {
    match p.document(d).map(|x| &x.data) {
        Some(DocumentData::Part(q)) => q,
        _ => panic!("no part"),
    }
}

fn volume(p: &PartDocument) -> Real {
    p.exact_bodies
        .0
        .iter()
        .filter_map(|s| s.volume_at(1e-4))
        .sum()
}

fn bodies(p: &PartDocument) -> Vec<String> {
    p.exact_bodies
        .0
        .iter()
        .filter_map(|s| s.name.clone())
        .collect()
}

fn design(w: Real) -> (Project, DocumentId, Vec<String>) {
    let mut p = Project::new("Enclosure");
    let m = master(w);
    let b = bodies(&m);
    let id = p.add_document("Master", DocumentData::Part(m));
    (p, id, b)
}

#[test]
fn an_enclosure_master_becomes_three_placed_components() {
    let (mut p, m, b) = design(100.0);
    let names: Vec<String> = ["Housing", "Lid", "Lug"].map(String::from).to_vec();
    let made =
        make::make_components(&mut p, m, &b, &names, Target::New("Enclosure".into())).unwrap();
    assert_eq!(made.parts.len(), 3);
    let total: Real = made.parts.iter().map(|d| volume(part(&p, *d))).sum();
    let expect = 100.0 * 60.0 * 30.0 + 100.0 * 60.0 * 4.0 + 15.0 * 20.0 * 10.0;
    assert!(
        (total - expect).abs() < 1e-3 * expect,
        "{total} vs {expect}"
    );
    // Each part holds one body; the assembly places all three, grounded,
    // at the master's frame.
    for d in &made.parts {
        assert_eq!(part(&p, *d).exact_bodies.0.len(), 1);
    }
    let a = match p.document(made.assembly.unwrap()).map(|x| &x.data) {
        Some(DocumentData::Assembly(a)) => a,
        _ => panic!("no assembly"),
    };
    assert_eq!(a.occurrences.len(), 3);
    assert!(a.occurrences.iter().all(|o| o.grounded));
    assert!(p.documents().iter().any(|d| d.name == "Lid"));
    // The lid sits on the housing (z 30..34), where its body was.
    let lid = made.parts[names.iter().position(|n| n == "Lid").unwrap()];
    let (lo, hi) = part(&p, lid).body.bounding_box().unwrap();
    assert!(
        (lo.z - 30.0).abs() < 1e-6 && (hi.z - 34.0).abs() < 1e-6,
        "{lo:?} {hi:?}"
    );
}

#[test]
fn the_components_follow_the_master() {
    let (mut p, m, b) = design(100.0);
    let made = make::make_components(&mut p, m, &b, &[], Target::None).unwrap();
    assert!(made.assembly.is_none());
    // Named after the bodies as the browser shows them.
    for n in ["Master Solid1", "Master Solid2", "Master Solid3"] {
        assert!(p.documents().iter().any(|d| d.name == n), "{n}");
    }
    // Nothing changed yet.
    assert!(make::update_derived(&mut p).0.is_empty());
    // The master grows 100 → 140 long: every part made from it follows.
    if let Some(DocumentData::Part(x)) = p.document_mut(m).map(|d| &mut d.data) {
        if let Some(Feature::Box(bx)) = x.features.first_mut() {
            bx.size[0] = 140.0;
        }
        x.regenerate().unwrap();
    }
    let (changed, errors) = make::update_derived(&mut p);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(changed.len(), 3, "the housing, the lid and the lug");
    let housing = made.parts[b.iter().position(|n| n == "Box1").unwrap()];
    let (lo, hi) = part(&p, housing).body.bounding_box().unwrap();
    assert!(((hi.x - lo.x) - 140.0).abs() < 1e-6);
}

#[test]
fn make_part_takes_chosen_bodies_and_refusals_change_nothing() {
    let (mut p, m, b) = design(100.0);
    let n = p.documents().len();
    // Housing and lid in one part.
    let two: Vec<String> = b.iter().take(2).cloned().collect();
    let d = make::make_part(&mut p, m, &two, "Body and lid").unwrap();
    assert_eq!(part(&p, d).exact_bodies.0.len(), 2);
    assert_eq!(p.documents().len(), n + 1);
    // Refused: no body, an unknown body, a target that is a part.
    assert!(make::make_part(&mut p, m, &[], "x").is_err());
    assert!(make::make_part(&mut p, m, &["Nope".into()], "x").is_err());
    assert!(make::make_components(&mut p, m, &b, &[], Target::Existing(m)).is_err());
    assert_eq!(p.documents().len(), n + 1);
}

#[test]
fn components_go_into_an_existing_assembly() {
    let (mut p, m, b) = design(100.0);
    let a = p.add_document(
        "Machine",
        DocumentData::Assembly(cadcraft3d::document::AssemblyDocument::default()),
    );
    let made = make::make_components(&mut p, m, &b[..1], &[], Target::Existing(a)).unwrap();
    assert_eq!(made.assembly, Some(a));
    match p.document(a).map(|x| &x.data) {
        Some(DocumentData::Assembly(x)) => assert_eq!(x.occurrences.len(), 1),
        _ => panic!(),
    }
}
