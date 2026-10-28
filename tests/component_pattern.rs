//! Pattern Component, Associative (`docs/spec/3.0-assembly-commands.md`
//! §3.7) on a real part: a plate with a row of holes from a rectangular
//! feature pattern. A face of a copy or of the original hole names the
//! pattern; its instances are the copies' motions; components placed by it
//! follow a changed count.

use cadcraft3d::assembly::pattern::{self, Layout, Lookup};
use cadcraft3d::assembly::{Geom, GeomRef};
use cadcraft3d::document::{AssemblyDocument, DocRef, DocumentId};
use cadcraft3d::kernel::{Point3, Real, Transform3};
use cadcraft3d::part::{
    Extent, ExtentDirection, ExtrudeFeature, Feature, LineRef, Operation, PartDocument,
    PatternFeature, PatternKind, ProfileRef, PullRef, SketchOptions, SketchPlacement,
};

/// 100 × 60 × 8 plate, a Ø8 hole at (20, 20), patterned `n` × 20 mm
/// along X; the hole's and the pattern's ids.
fn plate(n: u32) -> (PartDocument, String, String) {
    let mut part = PartDocument::with_box("Box1", [100.0, 60.0, 8.0]).unwrap();
    let opts = SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    };
    let j = part
        .create_sketch(SketchPlacement::on_face("Box1:Top"), &opts)
        .unwrap();
    let c = part.sketches[j]
        .sketch
        .plane
        .project(&Point3::new(20.0, 20.0, 8.0));
    part.sketches[j].sketch.add_circle(c, 4.0).unwrap();
    let rg = part.sketches[j].sketch.regions();
    let mut ex = ExtrudeFeature::new(vec![ProfileRef::of(&rg[0], rg[0].interior_point())], 1.0);
    ex.operation = Operation::Cut;
    ex.extent = Extent::ThroughAll;
    ex.direction = ExtentDirection::Flipped;
    let hole = part.add_extrude(j, ex).unwrap();
    let mut p = PatternFeature::rectangular();
    p.features = vec![hole.clone()];
    if let PatternKind::Rectangular { first, .. } = &mut p.kind {
        first.dir = Some(PullRef::Line(LineRef::Origin(0)));
        first.count = n;
        first.spacing = 20.0;
    }
    let pat = part.add_pattern(p).unwrap();
    (part, hole, pat)
}

fn set_count(part: &mut PartDocument, n: u32) {
    for f in &mut part.features {
        if let Feature::Pattern(p) = f
            && let PatternKind::Rectangular { first, .. } = &mut p.kind
        {
            first.count = n;
        }
    }
    part.regenerate().unwrap();
}

#[test]
fn associative_pattern_follows_the_part() {
    let (mut part, hole, pat) = plate(4);
    let names: Vec<String> = part
        .body
        .faces()
        .filter_map(|(_, f)| f.name.clone())
        .collect();
    let copy = names
        .iter()
        .find(|n| n.starts_with(&format!("{pat}#")))
        .expect("a face of a copy");
    let original = names
        .iter()
        .find(|n| n.starts_with(&format!("{hole}:")))
        .expect("a face of the hole");
    assert_eq!(
        pattern::pattern_of_face(&part, copy).as_deref(),
        Some(pat.as_str())
    );
    assert_eq!(
        pattern::pattern_of_face(&part, original).as_deref(),
        Some(pat.as_str())
    );
    assert_eq!(pattern::pattern_of_face(&part, "Box1:Front"), None);

    let inst = pattern::feature_instances(&part, &pat).unwrap();
    let steps: Vec<(u32, Real)> = inst
        .iter()
        .map(|(k, t)| (*k, t.translation.vector.x))
        .collect();
    assert_eq!(steps.len(), 3);
    for (k, x) in steps {
        assert!(
            (x - 20.0 * Real::from(k - 1)).abs() < 1e-9,
            "element {k} at {x}"
        );
    }

    // A bolt in the first hole follows the pattern, then its new count.
    let doc = DocRef {
        document: DocumentId::new_random(),
        library: None,
    };
    let mut a = AssemblyDocument::default();
    let p = a.place(doc, "Plate", Transform3::translation(0.0, 0.0, 5.0), true);
    let bolt = a.place(
        doc,
        "Bolt",
        Transform3::translation(20.0, 20.0, 13.0),
        false,
    );
    let local = |_: &GeomRef| -> Result<Geom, String> { Err("none".into()) };
    let part_now = std::cell::RefCell::new(part.clone());
    let feature = |_: u32, id: &str| pattern::feature_instances(&part_now.borrow(), id);
    let look = Lookup {
        local: &local,
        feature: &feature,
    };
    let layout = Layout::Feature {
        occurrence: p,
        feature: pat.clone(),
    };
    a.add_pattern(vec![bolt], layout, &look).unwrap();
    let last = a.patterns[0].elements[2].occurrences[0];
    let at = a.find(last).unwrap().transform * Point3::origin();
    assert!((at - Point3::new(80.0, 20.0, 13.0)).norm() < 1e-9);

    set_count(&mut part, 2);
    *part_now.borrow_mut() = part;
    pattern::place(&mut a, &look).unwrap();
    assert_eq!(a.patterns[0].elements.len(), 1);
    assert_eq!(a.occurrences.len(), 3);
}
