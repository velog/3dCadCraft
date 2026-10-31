//! Derive Part (spec 3.10 §3.5) on cases from real designs: a machining blank
//! derived from a casting, a mould insert scaled for shrinkage, a left-hand
//! bracket mirrored from the right-hand one, a derived part following its
//! source (a hole cut into it keeps its face), two bodies kept apart or
//! joined, and the refusals.

use cadcraft3d::document::{DocRef, DocumentId};
use cadcraft3d::kernel::{Point3, Real};
use cadcraft3d::part::derive::{DeriveLink, DeriveStyle, MirrorPlane};
use cadcraft3d::part::{
    Extent, ExtentDirection, ExtrudeFeature, Feature, Operation, PartDocument, ProfileRef,
    SketchOptions, SketchPlacement,
};

fn opts() -> SketchOptions {
    SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    }
}

fn link(name: &str) -> DeriveLink {
    DeriveLink::new(DocRef::local(DocumentId::new_random()), name)
}

/// Exact volume of the part's bodies.
fn volume(p: &PartDocument) -> Real {
    p.exact_bodies
        .0
        .iter()
        .filter_map(|s| s.volume_at(1e-4))
        .sum()
}

fn bbox(p: &PartDocument) -> (Point3, Point3) {
    p.body.bounding_box().unwrap()
}

/// A 60 × 40 × 20 casting with a Ø10 bore through its top.
fn casting(w: Real) -> PartDocument {
    let mut p = PartDocument::with_box("Box1", [w, 40.0, 20.0]).unwrap();
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
    assert!(p.exact);
    p
}

#[test]
fn a_machining_blank_is_derived_from_its_casting() {
    let src = casting(60.0);
    let mut blank = PartDocument::default();
    let id = blank.add_derive(&src, link("Casting")).unwrap();
    assert!(
        blank.feature_health.is_empty(),
        "{:?}",
        blank.feature_health
    );
    let (v0, v1) = (volume(&src), volume(&blank));
    assert!((v0 - v1).abs() < 1e-3 * v0, "{v0} vs {v1}");
    assert_eq!(blank.derived().len(), 1);
    assert!(blank.features.iter().any(|f| f.id() == id));
    // It regenerates alone (the text is in the part) and survives a save.
    let text = serde_json::to_string(&blank).unwrap();
    let mut back: PartDocument = serde_json::from_str(&text).unwrap();
    back.regenerate().unwrap();
    assert!((volume(&back) - v0).abs() < 1e-3 * v0);
    assert_eq!(back.derived().len(), 1);
}

#[test]
fn a_mould_insert_is_scaled_for_shrinkage() {
    // 0.6 % shrinkage: the cavity is the part scaled by 1.006.
    let src = casting(60.0);
    let mut l = link("Casting");
    l.scale = 1.006;
    let mut insert = PartDocument::default();
    insert.add_derive(&src, l).unwrap();
    let v = volume(&insert);
    let expect = volume(&src) * 1.006f64.powi(3);
    assert!((v - expect).abs() < 1e-3 * expect, "{v} vs {expect}");
    let (lo, hi) = bbox(&insert);
    assert!(((hi.x - lo.x) - 60.0 * 1.006).abs() < 1e-3, "{lo:?} {hi:?}");
}

#[test]
fn a_left_hand_bracket_is_mirrored_from_the_right_hand_one() {
    let src = casting(60.0);
    let mut l = link("Bracket RH");
    l.mirror = Some(MirrorPlane::Yz);
    let mut lh = PartDocument::default();
    lh.add_derive(&src, l).unwrap();
    assert!(lh.feature_health.is_empty(), "{:?}", lh.feature_health);
    let v = volume(&lh);
    assert!((v - volume(&src)).abs() < 1e-3 * v, "{v}");
    let (lo, hi) = bbox(&lh);
    assert!(
        (lo.x + 60.0).abs() < 1e-6 && hi.x.abs() < 1e-6,
        "{lo:?} {hi:?}"
    );
    // The bore is mirrored too: at x = −20.
    assert!(lh.body.faces().any(|(_, f)| {
        f.mesh.as_ref().is_some_and(|m| {
            m.positions
                .iter()
                .all(|q| ((q.x + 20.0).hypot(q.y - 20.0) - 5.0).abs() < 1e-3)
        })
    }));
}

#[test]
fn a_derived_part_follows_its_source_and_keeps_its_faces() {
    let src = casting(60.0);
    let mut d = PartDocument::default();
    d.add_derive(&src, link("Casting")).unwrap();
    // A pocket cut into the derived part's top face.
    let top = d
        .body
        .faces()
        .find_map(|(_, f)| {
            let m = f.mesh.as_ref()?;
            m.positions
                .iter()
                .all(|q| (q.z - 20.0).abs() < 1e-6)
                .then(|| f.name.clone())
                .flatten()
        })
        .unwrap();
    let j = d
        .create_sketch(SketchPlacement::on_face(&top), &opts())
        .unwrap();
    let plane = d.sketches[j].sketch.plane.clone();
    let a = plane.project(&Point3::new(40.0, 10.0, 20.0));
    let b = plane.project(&Point3::new(50.0, 30.0, 20.0));
    d.sketches[j].sketch.add_rectangle_two_point(a, b).unwrap();
    let rg = d.sketches[j].sketch.regions();
    let r = rg
        .iter()
        .min_by(|x, y| x.area().total_cmp(&y.area()))
        .unwrap();
    let mut cut = ExtrudeFeature::new(vec![ProfileRef::of(r, r.interior_point())], 5.0);
    cut.operation = Operation::Cut;
    cut.direction = ExtentDirection::Flipped;
    d.add_extrude(j, cut).unwrap();
    let before = volume(&d);
    // Same source: nothing to do.
    let k = d.derived()[0].0;
    assert!(!d.update_derive(k, &src).unwrap());
    // The casting grows 60 → 80 long: the blank follows, the pocket stays.
    let src2 = casting(80.0);
    assert!(d.update_derive(k, &src2).unwrap());
    assert!(d.feature_health.is_empty(), "{:?}", d.feature_health);
    let after = volume(&d);
    assert!(
        (after - before - 20.0 * 40.0 * 20.0).abs() < 1e-2 * after,
        "{before} → {after}"
    );
}

#[test]
fn two_bodies_kept_apart_or_joined() {
    // A two-body source: the casting and a lug welded to its end (a New
    // Solid touching it).
    let mut src = casting(60.0);
    let j = src
        .create_sketch(SketchPlacement::on_face("Box1:Top"), &opts())
        .unwrap();
    let plane = src.sketches[j].sketch.plane.clone();
    let a = plane.project(&Point3::new(60.0, 0.0, 20.0));
    let b = plane.project(&Point3::new(80.0, 40.0, 20.0));
    src.sketches[j]
        .sketch
        .add_rectangle_two_point(a, b)
        .unwrap();
    let rg = src.sketches[j].sketch.regions();
    let far = rg
        .iter()
        .find(|r| r.interior_point().x > 0.0 && r.area() > 700.0)
        .unwrap_or(&rg[0]);
    let mut lug = ExtrudeFeature::new(vec![ProfileRef::of(far, far.interior_point())], 10.0);
    lug.operation = Operation::NewSolid;
    lug.direction = ExtentDirection::Flipped;
    src.add_extrude(j, lug).unwrap();
    let bodies = src.exact_bodies.0.len();
    assert_eq!(bodies, 2);
    let mut each = PartDocument::default();
    each.add_derive(&src, link("Two")).unwrap();
    assert_eq!(each.exact_bodies.0.len(), 2);
    let mut l = link("Two");
    l.style = DeriveStyle::Single;
    let mut one = PartDocument::default();
    one.add_derive(&src, l).unwrap();
    assert_eq!(one.exact_bodies.0.len(), 1, "joined into one body");
    // Excluding the lug derives the casting alone.
    let mut l = link("Two");
    let lug_name = src
        .exact_bodies
        .0
        .iter()
        .filter_map(|b| b.name.clone())
        .find(|n| n != "Box1")
        .unwrap();
    l.exclude = vec![lug_name];
    let mut only = PartDocument::default();
    only.add_derive(&src, l).unwrap();
    assert_eq!(only.exact_bodies.0.len(), 1);
    let alone = volume(&casting(60.0));
    assert!(
        (volume(&only) - alone).abs() < 1e-3 * alone,
        "{} vs {alone}",
        volume(&only)
    );
}

#[test]
fn refusals_leave_the_part_unchanged() {
    let src = casting(60.0);
    let mut d = PartDocument::default();
    let mut l = link("Casting");
    l.scale = 0.0;
    assert!(d.add_derive(&src, l).is_err());
    let mut l = link("Casting");
    l.solids = false;
    assert!(d.add_derive(&src, l).is_err(), "no body kept");
    assert!(
        d.add_derive(&PartDocument::default(), link("Empty"))
            .is_err()
    );
    assert!(d.features.is_empty());
    // Break Link: a plain import of the text it holds.
    d.add_derive(&src, link("Casting")).unwrap();
    let k = d.derived()[0].0;
    assert!(d.break_derive(k));
    assert!(d.derived().is_empty());
    d.regenerate().unwrap();
    assert!(matches!(d.features[k], Feature::BodyOp(_)));
    assert!((volume(&d) - volume(&src)).abs() < 1e-3 * volume(&src));
}

// Derive Assembly: the parts of an assembly, each placed, as the source.

use cadcraft3d::kernel::{Transform3, Vector3};
use cadcraft3d::part::derive::DeriveSource;

/// A 100 × 60 × 10 base plate and a 10 × 10 × 50 post standing on it at
/// `post_at` (a weldment's parts).
fn weldment() -> (PartDocument, PartDocument) {
    (
        PartDocument::with_box("Box1", [100.0, 60.0, 10.0]).unwrap(),
        PartDocument::with_box("Box1", [10.0, 10.0, 50.0]).unwrap(),
    )
}

#[test]
fn a_weldment_assembly_is_derived_into_one_part() {
    let (plate, post) = weldment();
    let at = Transform3::translation(45.0, 25.0, 10.0);
    let src = DeriveSource::of(vec![(&plate, Transform3::identity()), (&post, at)]);
    // Each part a body, where it stands.
    let mut each = PartDocument::default();
    each.add_derive_from(&src, link("Weldment")).unwrap();
    assert_eq!(each.exact_bodies.0.len(), 2);
    let (lo, hi) = bbox(&each);
    assert!(
        (hi.z - 60.0).abs() < 1e-6 && lo.z.abs() < 1e-6,
        "{lo:?} {hi:?}"
    );
    let v = 100.0 * 60.0 * 10.0 + 10.0 * 10.0 * 50.0;
    assert!((volume(&each) - v).abs() < 1e-3 * v);
    // One solid: the post welded onto the plate.
    let mut l = link("Weldment");
    l.style = DeriveStyle::Single;
    let mut one = PartDocument::default();
    one.add_derive_from(&src, l).unwrap();
    assert_eq!(one.exact_bodies.0.len(), 1);
    assert!((volume(&one) - v).abs() < 1e-3 * v);
}

#[test]
fn a_turned_component_is_derived_where_it_stands() {
    // The post lies on its side: turned 90° about Y.
    let (_, post) = weldment();
    let at = Transform3::new(
        Vector3::new(0.0, 0.0, 10.0),
        Vector3::y() * std::f64::consts::FRAC_PI_2,
    );
    let src = DeriveSource::of(vec![(&post, at)]);
    let mut d = PartDocument::default();
    d.add_derive_from(&src, link("Lying post")).unwrap();
    let (lo, hi) = bbox(&d);
    assert!(((hi.x - lo.x) - 50.0).abs() < 1e-6, "{lo:?} {hi:?}");
    assert!(((hi.z - lo.z) - 10.0).abs() < 1e-6, "{lo:?} {hi:?}");
}

#[test]
fn a_moved_component_updates_the_derived_part() {
    let (plate, post) = weldment();
    let src = DeriveSource::of(vec![
        (&plate, Transform3::identity()),
        (&post, Transform3::translation(45.0, 25.0, 10.0)),
    ]);
    let mut d = PartDocument::default();
    d.add_derive_from(&src, link("Weldment")).unwrap();
    // Same placements: nothing to do.
    assert!(!d.update_derive_from(0, &src).unwrap());
    // The post moved to the plate's corner.
    let moved = DeriveSource::of(vec![
        (&plate, Transform3::identity()),
        (&post, Transform3::translation(0.0, 0.0, 10.0)),
    ]);
    assert!(d.update_derive_from(0, &moved).unwrap());
    let (lo, _) = bbox(&d);
    assert!(lo.x.abs() < 1e-6);
    assert!(
        d.body
            .faces()
            .filter_map(|(_, f)| f.mesh.as_ref())
            .flat_map(|m| m.positions.iter())
            .any(|q| (q.z - 60.0).abs() < 1e-6 && q.x.abs() < 1e-6 && q.y.abs() < 1e-6)
    );
}

#[test]
fn editing_a_derived_feature_mirrors_it_in_place() {
    // The right-hand blank is edited into the left-hand one: same feature,
    // same id, mirrored about YZ; a second edit back undoes it.
    let src = casting(60.0);
    let mut d = PartDocument::default();
    let id = d.add_derive(&src, link("Casting")).unwrap();
    let k = d.derived()[0].0;
    let mut l = d.derived()[0].1.clone();
    l.mirror = Some(MirrorPlane::Yz);
    d.edit_derive(k, &DeriveSource::part(&src), l.clone())
        .unwrap();
    assert_eq!(d.features[k].id(), id);
    assert_eq!(d.features.len(), 1);
    let (lo, hi) = bbox(&d);
    assert!(
        (lo.x + 60.0).abs() < 1e-6 && hi.x.abs() < 1e-6,
        "{lo:?} {hi:?}"
    );
    l.mirror = None;
    d.edit_derive(k, &DeriveSource::part(&src), l).unwrap();
    let (lo, _) = bbox(&d);
    assert!(lo.x.abs() < 1e-6);
    // A bad edit changes nothing.
    let mut bad = d.derived()[0].1.clone();
    bad.scale = -1.0;
    let before = volume(&d);
    assert!(d.edit_derive(k, &DeriveSource::part(&src), bad).is_err());
    assert!((volume(&d) - before).abs() < 1e-6);
    assert_eq!(d.derived()[0].1.scale, 1.0);
}
