//! Simplify (spec 2.18 part, 3.11 assembly) on cases from real use: a supplier's housing
//! handed on without its bores and rounds, as a model state of the same part and as a part of
//! its own that follows the source; envelopes for a keep-out space; small bodies and parts
//! left out; an assembly as one simplified part following its components; the refusals.

use cadcraft3d::document::{DocRef, DocumentId};
use cadcraft3d::kernel::{Point3, Real, Transform3};
use cadcraft3d::part::blend::{BlendFeature, EdgeRef};
use cadcraft3d::part::derive::{DeriveLink, DeriveSource, DeriveStyle};
use cadcraft3d::part::simplify::{Envelope, EnvelopeType, Removal, SimplifySpec};
use cadcraft3d::part::{
    BodyOp, Extent, ExtentDirection, ExtrudeFeature, Feature, Operation, PartDocument, ProfileRef,
    SketchOptions, SketchPlacement,
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

fn close(a: Real, b: Real, rel: Real) -> bool {
    (a - b).abs() <= rel * b.abs().max(1.0)
}

/// A 60 × 40 × 20 housing with a Ø10 bore through its top at (20, 20), and a 3 mm round on
/// its top front edge.
fn housing() -> PartDocument {
    let mut p = PartDocument::with_box("Box1", [60.0, 40.0, 20.0]).unwrap();
    let j = p
        .create_sketch(SketchPlacement::on_face("Box1:Top"), &opts())
        .unwrap();
    let plane = p.sketches[j].sketch.plane.clone();
    let c = plane.project(&Point3::new(20.0, 20.0, 20.0));
    p.sketches[j].sketch.add_circle(c, 5.0).unwrap();
    let rg = p.sketches[j].sketch.regions();
    let hole: Vec<ProfileRef> = rg
        .iter()
        .filter(|r| r.area() < 100.0)
        .map(|r| ProfileRef::of(r, r.interior_point()))
        .collect();
    let mut cut = ExtrudeFeature::new(hole, 1.0);
    cut.operation = Operation::Cut;
    cut.extent = Extent::ThroughAll;
    cut.direction = ExtentDirection::Flipped;
    p.add_extrude(j, cut).unwrap();
    let edge = EdgeRef {
        faces: ["Box1:Top".into(), "Box1:Front".into()],
        hint: [40.0, 0.0, 20.0],
    };
    p.add_blend(BlendFeature::fillet(vec![edge], 3.0)).unwrap();
    assert!(p.exact, "{:?}", p.feature_health);
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    p
}

const FULL: Real = 60.0 * 40.0 * 20.0;
/// What a 3 mm round takes off a 60 mm edge.
const ROUND: Real = (9.0 - std::f64::consts::PI * 9.0 / 4.0) * 60.0;
const BORE: Real = std::f64::consts::PI * 25.0 * 20.0;

#[test]
fn the_housing_measures_as_built() {
    let p = housing();
    assert!(
        close(volume(&p), FULL - ROUND - BORE, 1e-3),
        "{}",
        volume(&p)
    );
}

#[test]
fn holes_and_rounds_go_by_size() {
    let p = housing();
    // Holes up to 8 mm: the Ø10 bore stays; rounds up to 5 mm go.
    let spec = SimplifySpec {
        holes: Removal::range(8.0),
        fillets: Removal::range(5.0),
        ..Default::default()
    };
    let s = p.simplified(&spec).unwrap();
    assert!(close(volume(&s), FULL - BORE, 1e-3), "{}", volume(&s));
    // Holes up to 12 mm: the bore goes too.
    let spec = SimplifySpec {
        holes: Removal::range(12.0),
        fillets: Removal::range(5.0),
        ..Default::default()
    };
    let s = p.simplified(&spec).unwrap();
    assert!(close(volume(&s), FULL, 1e-4), "{}", volume(&s));
    // The removed features show as suppressed while it is active.
    let cut = p.features[0..]
        .iter()
        .find(|f| f.id().starts_with("Extrusion"))
        .unwrap()
        .id();
    assert!(s.simplified_away().contains(cut));
    // Preserve keeps the bore whatever the rule says.
    let spec = SimplifySpec {
        holes: Removal::all(),
        preserve: vec![cut.to_owned()],
        ..Default::default()
    };
    let s = p.simplified(&spec).unwrap();
    assert!(
        close(volume(&s), FULL - ROUND - BORE, 1e-3),
        "{}",
        volume(&s)
    );
    // The source itself is untouched.
    assert!(close(volume(&p), FULL - ROUND - BORE, 1e-3));
}

#[test]
fn a_simplified_model_state_follows_the_master_and_can_be_edited() {
    let mut p = housing();
    let spec = SimplifySpec {
        holes: Removal::all(),
        fillets: Removal::all(),
        ..Default::default()
    };
    let (v, id) = p.add_simplified_variant(spec, None).unwrap();
    assert_eq!(p.variants.active, v);
    assert_eq!(p.variants.get(v).unwrap().name, "Simplified1");
    assert!(close(volume(&p), FULL, 1e-4), "{}", volume(&p));
    // [Primary] keeps every detail: its Simplify is suppressed.
    p.activate_variant(0).unwrap();
    assert!(
        close(volume(&p), FULL - ROUND - BORE, 1e-3),
        "{}",
        volume(&p)
    );
    assert!(p.suppressed.contains(&id));
    // The master changes (Factory Scope: every state): the simplified state follows.
    p.variants.factory_scope = true;
    p.box_feature_mut().unwrap().size[0] = 80.0;
    p.regenerate().unwrap();
    p.variants_sync();
    p.variants.factory_scope = false;
    p.activate_variant(v).unwrap();
    assert!(
        close(volume(&p), 80.0 * 40.0 * 20.0, 1e-4),
        "{}",
        volume(&p)
    );
    // Edit Feature: keep the bore (holes up to 5 mm only).
    let k = p.feature_index(&id).unwrap();
    let mut f = p.features[k].clone();
    if let Feature::BodyOp(b) = &mut f
        && let BodyOp::Simplify(o) = &mut b.op
    {
        o.spec.holes = Removal::range(5.0);
    }
    p.replace_feature(k, f).unwrap();
    assert!(
        close(volume(&p), 80.0 * 40.0 * 20.0 - BORE, 1e-3),
        "{}",
        volume(&p)
    );
    // Save and load: the state and its feature come back.
    let text = serde_json::to_string(&p).unwrap();
    let mut back: PartDocument = serde_json::from_str(&text).unwrap();
    back.regenerate().unwrap();
    assert!(close(volume(&back), volume(&p), 1e-6));
    back.activate_variant(0).unwrap();
    let round80 = ROUND / 60.0 * 80.0;
    assert!(
        close(volume(&back), 80.0 * 40.0 * 20.0 - round80 - BORE, 1e-3),
        "{}",
        volume(&back)
    );
}

#[test]
fn envelopes_replace_the_bodies() {
    let p = housing();
    for (t, expect) in [
        (EnvelopeType::Box, FULL),
        (EnvelopeType::OrientedBox, FULL),
        // The circle round a 60 × 40 rectangle, 20 high (the axis along Z is the smallest).
        (
            EnvelopeType::Cylinder,
            std::f64::consts::PI * (30.0f64.powi(2) + 20.0f64.powi(2)) * 20.0,
        ),
    ] {
        let spec = SimplifySpec {
            envelope: Envelope::All,
            envelope_type: t,
            ..Default::default()
        };
        let s = p.simplified(&spec).unwrap();
        assert_eq!(s.exact_bodies.0.len(), 1);
        assert!(close(volume(&s), expect, 2e-3), "{t:?}: {}", volume(&s));
        // Envelope faces are named after the feature.
        let id = s
            .active_simplify()
            .map(|(k, _)| s.features[k].id().to_owned())
            .unwrap();
        assert!(
            s.exact_bodies.0[0]
                .face_names
                .iter()
                .flatten()
                .all(|n| n.starts_with(&id))
        );
    }
}

/// The housing with a small separate body (a 4 mm cube) beside it and a core body inside a
/// cavity... built as a second New Solid.
fn housing_and_cube() -> PartDocument {
    let mut p = housing();
    let j = p
        .create_sketch(SketchPlacement::on_face("Box1:Right"), &opts())
        .unwrap();
    let plane = p.sketches[j].sketch.plane.clone();
    let a = plane.project(&Point3::new(60.0, 10.0, 5.0));
    let b = plane.project(&Point3::new(60.0, 14.0, 9.0));
    p.sketches[j].sketch.add_rectangle_two_point(a, b).unwrap();
    let rg = p.sketches[j].sketch.regions();
    let small = rg.iter().find(|r| r.area() < 20.0).unwrap();
    let mut lug = ExtrudeFeature::new(vec![ProfileRef::of(small, small.interior_point())], 4.0);
    lug.operation = Operation::NewSolid;
    p.add_extrude(j, lug).unwrap();
    assert_eq!(p.exact_bodies.0.len(), 2, "{:?}", p.feature_health);
    p
}

#[test]
fn small_bodies_are_left_out_and_each_body_gets_its_envelope() {
    let p = housing_and_cube();
    let all = volume(&p);
    let spec = SimplifySpec {
        exclude_size: Some(10.0),
        ..Default::default()
    };
    let s = p.simplified(&spec).unwrap();
    assert_eq!(s.exact_bodies.0.len(), 1);
    assert!(close(volume(&s), all - 64.0, 1e-3), "{}", volume(&s));
    // By name.
    let cube = p
        .exact_bodies
        .0
        .iter()
        .filter_map(|b| b.name.clone())
        .find(|n| n != "Box1")
        .unwrap();
    let spec = SimplifySpec {
        exclude: vec![cube.clone()],
        ..Default::default()
    };
    assert_eq!(p.simplified(&spec).unwrap().exact_bodies.0.len(), 1);
    // Each body: two boxes, named as the bodies were.
    let spec = SimplifySpec {
        envelope: Envelope::EachBody,
        ..Default::default()
    };
    let s = p.simplified(&spec).unwrap();
    assert_eq!(s.exact_bodies.0.len(), 2);
    assert!(close(volume(&s), FULL + 64.0, 1e-3), "{}", volume(&s));
    assert!(
        s.exact_bodies
            .0
            .iter()
            .any(|b| b.name.as_deref() == Some(cube.as_str()))
    );
    // Selected bodies: only the cube's envelope (a box of itself), the housing as it is.
    let spec = SimplifySpec {
        envelope: Envelope::Selected,
        envelope_bodies: vec!["Box1".into()],
        ..Default::default()
    };
    let s = p.simplified(&spec).unwrap();
    assert!(close(volume(&s), FULL + 64.0, 1e-3), "{}", volume(&s));
}

#[test]
fn a_simplified_part_of_its_own_follows_the_source() {
    let src = housing();
    let mut link = DeriveLink::new(DocRef::local(DocumentId::new_random()), "Housing");
    link.simplify = Some(SimplifySpec {
        holes: Removal::all(),
        fillets: Removal::all(),
        ..Default::default()
    });
    let mut d = PartDocument::default();
    d.add_derive(&src, link.clone()).unwrap();
    assert!(d.feature_health.is_empty(), "{:?}", d.feature_health);
    assert!(close(volume(&d), FULL, 1e-4), "{}", volume(&d));
    // It regenerates alone and survives a save.
    let text = serde_json::to_string(&d).unwrap();
    let mut back: PartDocument = serde_json::from_str(&text).unwrap();
    back.regenerate().unwrap();
    assert!(close(volume(&back), FULL, 1e-4));
    assert!(back.derived()[0].1.simplify.is_some());
    // The source grows: Update follows it, still simplified.
    let mut src2 = src.clone();
    src2.box_feature_mut().unwrap().size[2] = 30.0;
    src2.regenerate().unwrap();
    let k = d.derived()[0].0;
    assert!(d.update_derive(k, &src2).unwrap());
    assert!(
        close(volume(&d), 60.0 * 40.0 * 30.0, 1e-4),
        "{}",
        volume(&d)
    );
    // Edit: how much it simplifies changes (the bore comes back).
    link.simplify = Some(SimplifySpec {
        fillets: Removal::all(),
        ..Default::default()
    });
    d.edit_derive(k, &DeriveSource::part(&src2), link).unwrap();
    let bore30 = std::f64::consts::PI * 25.0 * 30.0;
    assert!(
        close(volume(&d), 60.0 * 40.0 * 30.0 - bore30, 1e-3),
        "{}",
        volume(&d)
    );
}

/// A 100 × 60 × 10 plate and a 10 × 10 × 50 post standing on it at (20, 20); a tiny 2 mm
/// cube in a corner (a fastener-sized part).
fn assembly_parts() -> (PartDocument, PartDocument, PartDocument) {
    let plate = PartDocument::with_box("Box1", [100.0, 60.0, 10.0]).unwrap();
    let post = PartDocument::with_box("Box1", [10.0, 10.0, 50.0]).unwrap();
    let tiny = PartDocument::with_box("Box1", [2.0, 2.0, 2.0]).unwrap();
    (plate, post, tiny)
}

#[test]
fn an_assembly_becomes_one_simplified_part() {
    let (plate, post, tiny) = assembly_parts();
    let at_post = Transform3::translation(20.0, 20.0, 10.0);
    let at_tiny = Transform3::translation(90.0, 50.0, 10.0);
    let parts = vec![
        (&plate, Transform3::identity()),
        (&post, at_post),
        (&tiny, at_tiny),
    ];
    let paths = vec![vec![1], vec![2], vec![3]];
    let src = DeriveSource::with_paths(parts.clone(), paths.clone());
    let base = DeriveLink::new(DocRef::local(DocumentId::new_random()), "Assembly1");
    let make = |spec: SimplifySpec, style: DeriveStyle| {
        let mut l = base.clone();
        l.style = style;
        l.simplify = Some(spec);
        let mut d = PartDocument::default();
        d.add_derive_from(&src, l).map(|_| d)
    };
    let plate_v = 100.0 * 60.0 * 10.0;
    let post_v = 10.0 * 10.0 * 50.0;
    // Small parts left out by size.
    let d = make(
        SimplifySpec {
            exclude_size: Some(5.0),
            ..Default::default()
        },
        DeriveStyle::Each,
    )
    .unwrap();
    assert_eq!(d.exact_bodies.0.len(), 2);
    assert!(close(volume(&d), plate_v + post_v, 1e-4), "{}", volume(&d));
    // One solid without seams.
    let d = make(
        SimplifySpec {
            exclude: vec!["3".into()],
            ..Default::default()
        },
        DeriveStyle::SingleMerged,
    )
    .unwrap();
    assert_eq!(d.exact_bodies.0.len(), 1);
    assert!(close(volume(&d), plate_v + post_v, 1e-4), "{}", volume(&d));
    // All in one envelope: the box round everything (100 × 60 × 60).
    let d = make(
        SimplifySpec {
            envelope: Envelope::All,
            ..Default::default()
        },
        DeriveStyle::Each,
    )
    .unwrap();
    assert_eq!(d.exact_bodies.0.len(), 1);
    assert!(
        close(volume(&d), 100.0 * 60.0 * 60.0, 1e-4),
        "{}",
        volume(&d)
    );
    // An envelope per first-level component.
    let d = make(
        SimplifySpec {
            envelope: Envelope::EachTop,
            ..Default::default()
        },
        DeriveStyle::Each,
    )
    .unwrap();
    assert_eq!(d.exact_bodies.0.len(), 3);
    // Update: the post moves; the derived part follows.
    let k = d.derived()[0].0;
    let moved = Transform3::translation(60.0, 20.0, 10.0);
    let src2 = DeriveSource::with_paths(
        vec![
            (&plate, Transform3::identity()),
            (&post, moved),
            (&tiny, at_tiny),
        ],
        paths,
    );
    let mut d = d;
    assert!(d.update_derive_from(k, &src2).unwrap());
    let (lo, hi) = d.body.bounding_box().unwrap();
    assert!(
        (hi.z - 60.0).abs() < 1e-3 && lo.x.abs() < 1e-3,
        "{lo:?} {hi:?}"
    );
}

#[test]
fn internal_parts_are_removed() {
    // A hollow box (a cover) with a part inside it, and a part outside.
    let mut cover = PartDocument::with_box("Box1", [40.0, 40.0, 40.0]).unwrap();
    let shell = cadcraft3d::part::shell::ShellFeature::new(2.0);
    cover.add_shell(shell).unwrap();
    assert!(
        cover.feature_health.is_empty(),
        "{:?}",
        cover.feature_health
    );
    let inner = PartDocument::with_box("Box1", [10.0, 10.0, 10.0]).unwrap();
    let outside = PartDocument::with_box("Box1", [10.0, 10.0, 10.0]).unwrap();
    let src = DeriveSource::with_paths(
        vec![
            (&cover, Transform3::identity()),
            (&inner, Transform3::translation(15.0, 15.0, 15.0)),
            (&outside, Transform3::translation(60.0, 0.0, 0.0)),
        ],
        vec![vec![1], vec![2], vec![3]],
    );
    let mut l = DeriveLink::new(DocRef::local(DocumentId::new_random()), "Asm");
    l.simplify = Some(SimplifySpec {
        remove_internal: true,
        ..Default::default()
    });
    let mut d = PartDocument::default();
    d.add_derive_from(&src, l.clone()).unwrap();
    assert_eq!(d.exact_bodies.0.len(), 2, "the inner part is gone");
    // Fill internal voids: the cover becomes solid.
    l.simplify = Some(SimplifySpec {
        remove_internal: true,
        fill_voids: true,
        ..Default::default()
    });
    let mut d = PartDocument::default();
    d.add_derive_from(&src, l).unwrap();
    assert!(
        close(volume(&d), 40.0f64.powi(3) + 1000.0, 1e-4),
        "{}",
        volume(&d)
    );
}

#[test]
fn refusals_leave_the_part_unchanged() {
    let mut p = housing();
    let before = volume(&p);
    let n = p.features.len();
    let bad = SimplifySpec {
        holes: Removal::range(0.0),
        ..Default::default()
    };
    assert!(p.add_simplify(bad.clone()).is_err());
    assert!(p.add_simplified_variant(bad, None).is_err());
    let gone = SimplifySpec {
        exclude: vec!["Box1".into()],
        ..Default::default()
    };
    assert!(p.simplified(&gone).is_err(), "every body left out");
    assert_eq!(p.features.len(), n);
    assert!(p.variants.is_empty());
    assert!(close(volume(&p), before, 1e-9));
}

/// With `CADCRAFT_WRITE_DEMO=<file>`: a design file for the visual check (the housing, and an
/// assembly of a plate, a post, a small cube and the housing).
#[test]
fn write_demo_file() {
    use cadcraft3d::document::{AssemblyDocument, DocumentData, Project};
    let Ok(path) = std::env::var("CADCRAFT_WRITE_DEMO") else {
        return;
    };
    let mut p = Project::new("SimplifyDemo");
    let h = p.add_document("Housing", DocumentData::Part(housing()));
    let (plate, post, tiny) = assembly_parts();
    let a = p.add_document("Plate", DocumentData::Part(plate));
    let b = p.add_document("Post", DocumentData::Part(post));
    let c = p.add_document("Pin", DocumentData::Part(tiny));
    let mut asm = AssemblyDocument::default();
    asm.place(DocRef::local(a), "Plate", Transform3::identity(), true);
    asm.place(
        DocRef::local(b),
        "Post",
        Transform3::translation(20.0, 20.0, 10.0),
        false,
    );
    asm.place(
        DocRef::local(c),
        "Pin",
        Transform3::translation(90.0, 50.0, 10.0),
        false,
    );
    asm.place(
        DocRef::local(h),
        "Housing",
        Transform3::translation(40.0, 10.0, 10.0),
        false,
    );
    p.add_document("Assembly1", DocumentData::Assembly(asm));
    p.save(std::path::Path::new(&path)).unwrap();
}
