//! Cross-part references to edges (spec 3.10 §3.3, format revision 47) on
//! cases from real assemblies: a gasket sketched on a flange — its outline
//! and bolt holes projected from the flange's top edges — that follows the
//! flange when it grows, a cap following the rim of a tube whose diameter
//! changes, a gasket placed turned and lifted, a source that is removed (the
//! last copy stays) and the part opened alone (it builds from its copies).

use cadcraft3d::assembly::context::{self, current};
use cadcraft3d::assembly::tree::Linked;
use cadcraft3d::document::{AssemblyDocument, DocRef, DocumentData, DocumentId, Project};
use cadcraft3d::kernel::{EdgeId, Point3, Real, TessellationOptions, Transform3, Vector3};
use cadcraft3d::part::blend::edge_ref;
use cadcraft3d::part::context::{ContextRef, ContextSource};
use cadcraft3d::part::{
    EdgeRef, Extent, ExtrudeFeature, Feature, Operation, OriginPlane, PartDocument, ProfileRef,
    SketchOptions, SketchPlacement,
};
use cadcraft3d::sketch::{Constraint as SkCon, Point2};

fn opts() -> SketchOptions {
    SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    }
}

/// A `w` × `w` × 10 flange with four Ø10 bolt holes 15 mm in from its
/// corners (through).
fn flange(w: Real) -> PartDocument {
    let mut p = PartDocument::with_box("Box1", [w, w, 10.0]).unwrap();
    let j = p
        .create_sketch(SketchPlacement::on_face("Box1:Top"), &opts())
        .unwrap();
    let plane = p.sketches[j].sketch.plane.clone();
    for (x, y) in [
        (15.0, 15.0),
        (w - 15.0, 15.0),
        (w - 15.0, w - 15.0),
        (15.0, w - 15.0),
    ] {
        let c = plane.project(&Point3::new(x, y, 10.0));
        p.sketches[j].sketch.add_circle(c, 5.0).unwrap();
    }
    let rg = p.sketches[j].sketch.regions();
    let holes: Vec<ProfileRef> = rg
        .iter()
        .filter(|r| r.area() < 100.0)
        .map(|r| ProfileRef::of(r, r.interior_point()))
        .collect();
    assert_eq!(holes.len(), 4);
    let mut cut = ExtrudeFeature::new(holes, 1.0);
    cut.operation = Operation::Cut;
    cut.extent = Extent::ThroughAll;
    cut.direction = cadcraft3d::part::ExtentDirection::Flipped;
    p.add_extrude(j, cut).unwrap();
    p
}

/// Sets the flange's box size to `w` × `w` (its holes keep their sketch).
fn grow(p: &mut PartDocument, w: Real) {
    if let Some(Feature::Box(b)) = p.features.first_mut() {
        b.size[0] = w;
        b.size[1] = w;
    }
    p.regenerate().unwrap();
}

/// The edges of `p` whose sampled points all lie at height `z`.
fn edges_at(p: &PartDocument, z: Real) -> Vec<EdgeRef> {
    let o = TessellationOptions::default();
    let ids: Vec<EdgeId> = p.body.edges().map(|(e, _)| e).collect();
    ids.into_iter()
        .filter(|e| {
            p.body
                .sample_edge(*e, &o)
                .is_ok_and(|s| s.iter().all(|q| (q.z - z).abs() < 1e-6))
        })
        .filter_map(|e| edge_ref(&p.body, e))
        .collect()
}

/// A project with `source` placed at `at_source` (grounded) and an empty
/// part placed at `at_target`; their occurrence ids 1 and 2.
fn design(
    source: PartDocument,
    at_source: Transform3,
    at_target: Transform3,
) -> (Project, DocumentId, DocumentId, DocumentId) {
    let mut p = Project::new("T");
    let s = p.add_document("Source", DocumentData::Part(source));
    let t = p.add_document("Target", DocumentData::Part(PartDocument::default()));
    let mut a = AssemblyDocument::default();
    a.place(DocRef::local(s), "Source", at_source, true);
    a.place(DocRef::local(t), "Target", at_target, false);
    let aid = p.add_document("Top", DocumentData::Assembly(a));
    (p, aid, s, t)
}

fn part(p: &Project, d: DocumentId) -> &PartDocument {
    match p.document(d).map(|x| &x.data) {
        Some(DocumentData::Part(q)) => q,
        _ => panic!("no part"),
    }
}

fn part_mut(p: &mut Project, d: DocumentId) -> &mut PartDocument {
    match p.document_mut(d).map(|x| &mut x.data) {
        Some(DocumentData::Part(q)) => q,
        _ => panic!("no part"),
    }
}

fn asm(p: &Project, d: DocumentId) -> AssemblyDocument {
    match p.document(d).map(|x| &x.data) {
        Some(DocumentData::Assembly(a)) => a.clone(),
        _ => panic!("no assembly"),
    }
}

/// In the target part, a sketch on XY projecting every edge of `edges`
/// (from the source occurrence 1, the target is occurrence 2), then every
/// largest region bounded by them extruded `h`.
fn project_and_extrude(
    p: &mut Project,
    aid: DocumentId,
    t: DocumentId,
    edges: &[EdgeRef],
    h: Real,
) {
    let a = asm(p, aid);
    let refs: Vec<ContextRef> = edges
        .iter()
        .map(|e| {
            let source = ContextSource {
                assembly: aid,
                target: vec![2],
                path: vec![1],
                face: e.faces[0].clone(),
                edge: Some(e.clone()),
                hint: e.hint,
                label: "Source:1 edge".into(),
            };
            let geom = current(p, &Linked::new(), &a, &source).unwrap();
            ContextRef {
                id: String::new(),
                source,
                geom,
            }
        })
        .collect();
    let tp = part_mut(p, t);
    let i = tp
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    for c in refs {
        tp.project_context_edge(i, c, false).unwrap();
    }
    let rg = tp.sketches[i].sketch.regions();
    assert!(!rg.is_empty(), "the projected edges close a region");
    // The outline with its holes: the largest region.
    let r = rg
        .iter()
        .max_by(|a, b| a.area().total_cmp(&b.area()))
        .unwrap();
    let profiles = vec![ProfileRef::of(r, r.interior_point())];
    tp.add_extrude(i, ExtrudeFeature::new(profiles, h)).unwrap();
}

/// Volume of the display triangles.
fn volume(p: &PartDocument) -> Real {
    let o = TessellationOptions::default();
    p.body
        .solids()
        .map(|(sid, _)| {
            let m = p.body.tessellate_solid(sid, &o).unwrap();
            m.faces
                .triangles
                .iter()
                .map(|t| {
                    let [a, b, c] = t.map(|i| m.faces.positions[i as usize].coords);
                    a.dot(&b.cross(&c)) / 6.0
                })
                .sum::<Real>()
        })
        .sum()
}

fn bbox(p: &PartDocument) -> (Point3, Point3) {
    p.body.bounding_box().unwrap()
}

#[test]
fn a_gasket_follows_the_flange_it_was_drawn_on() {
    // The gasket's part sits on the flange's top (z 10).
    let (mut p, aid, s, t) = design(
        flange(100.0),
        Transform3::identity(),
        Transform3::translation(0.0, 0.0, 10.0),
    );
    let top = edges_at(part(&p, s), 10.0);
    // 4 outline edges + 4 hole rims (each rim may come in pieces).
    assert!(top.len() >= 8, "{} edges", top.len());
    project_and_extrude(&mut p, aid, t, &top, 2.0);
    let hole = std::f64::consts::PI * 25.0;
    let v = volume(part(&p, t));
    let expect = (100.0 * 100.0 - 4.0 * hole) * 2.0;
    assert!((v - expect).abs() < 0.01 * expect, "volume {v} vs {expect}");
    // The flange grows to 120: Update rebuilds the gasket with it.
    grow(part_mut(&mut p, s), 120.0);
    let r = context::update(&mut p, &Linked::new(), aid);
    assert_eq!(r.changed, vec![t]);
    assert!(r.sick.is_empty(), "{:?}", r.sick);
    let (lo, hi) = bbox(part(&p, t));
    assert!(((hi.x - lo.x) - 120.0).abs() < 1e-6, "{lo:?} {hi:?}");
    let v = volume(part(&p, t));
    let expect = (120.0 * 120.0 - 4.0 * hole) * 2.0;
    assert!((v - expect).abs() < 0.01 * expect, "volume {v} vs {expect}");
    assert!(part(&p, t).feature_health.is_empty());
}

/// A Ø`d` tube 40 high (a disc extruded, its diameter dimensioned).
fn tube(d: Real) -> PartDocument {
    let mut p = PartDocument::default();
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    let sk = &mut p.sketches[i].sketch;
    let c0 = sk.add_point(Point2::new(0.0, 0.0));
    sk.set_fixed(c0, true).unwrap();
    let c = sk.add_circle_at(c0, d / 2.0).unwrap();
    sk.add_constraint(SkCon::Diameter {
        circle: c,
        value: d,
    })
    .unwrap();
    let rg = sk.regions();
    p.add_extrude(
        i,
        ExtrudeFeature::new(vec![ProfileRef::of(&rg[0], rg[0].interior_point())], 40.0),
    )
    .unwrap();
    p
}

#[test]
fn a_cap_follows_the_rim_of_a_tube_that_grows() {
    let (mut p, aid, s, t) = design(
        tube(40.0),
        Transform3::translation(50.0, 50.0, 0.0),
        Transform3::translation(50.0, 50.0, 40.0),
    );
    let rim = edges_at(part(&p, s), 40.0);
    assert!(!rim.is_empty());
    project_and_extrude(&mut p, aid, t, &rim, 3.0);
    let pi = std::f64::consts::PI;
    let v = volume(part(&p, t));
    assert!((v - pi * 400.0 * 3.0).abs() < 0.01 * v, "volume {v}");
    // Ø40 → Ø60.
    let sp = part_mut(&mut p, s);
    let (k, _) = sp.sketches[0]
        .sketch
        .constraints()
        .find(|(_, c)| matches!(c, SkCon::Diameter { .. }))
        .unwrap();
    sp.sketches[0]
        .sketch
        .set_dimension_nominal(k, 60.0)
        .unwrap();
    sp.regenerate().unwrap();
    let r = context::update(&mut p, &Linked::new(), aid);
    assert_eq!(r.changed, vec![t], "{r:?}");
    let v = volume(part(&p, t));
    assert!((v - pi * 900.0 * 3.0).abs() < 0.01 * v, "volume {v}");
}

#[test]
fn a_gasket_placed_turned_and_lifted_lies_on_the_flange() {
    // The target occurrence is turned 90° about Z and lifted 10: the
    // projected outline must still cover the flange in the assembly.
    let turn = Transform3::new(
        Vector3::new(30.0, -20.0, 10.0),
        Vector3::z() * 90f64.to_radians(),
    );
    let (mut p, aid, s, t) = design(flange(100.0), Transform3::identity(), turn);
    let top = edges_at(part(&p, s), 10.0);
    project_and_extrude(&mut p, aid, t, &top, 2.0);
    let (lo, hi) = bbox(part(&p, t));
    for corner in [lo, hi] {
        let w = turn * corner;
        for c in [w.x, w.y] {
            assert!(c.abs() < 1e-6 || (c - 100.0).abs() < 1e-6, "{w:?}");
        }
    }
}

#[test]
fn a_removed_source_keeps_the_last_copy() {
    let (mut p, aid, s, t) = design(
        tube(40.0),
        Transform3::identity(),
        Transform3::translation(0.0, 0.0, 40.0),
    );
    let rim = edges_at(part(&p, s), 40.0);
    project_and_extrude(&mut p, aid, t, &rim, 3.0);
    let before = volume(part(&p, t));
    if let Some(DocumentData::Assembly(a)) = p.document_mut(aid).map(|x| &mut x.data) {
        a.remove(1);
    }
    let r = context::update(&mut p, &Linked::new(), aid);
    assert!(r.changed.is_empty() && !r.sick.is_empty(), "{r:?}");
    let after = volume(part(&p, t));
    assert!((after - before).abs() < 1e-9);
}

#[test]
fn the_part_opened_alone_builds_from_its_copies() {
    let (mut p, aid, s, t) = design(
        flange(100.0),
        Transform3::identity(),
        Transform3::translation(0.0, 0.0, 10.0),
    );
    let top = edges_at(part(&p, s), 10.0);
    project_and_extrude(&mut p, aid, t, &top, 2.0);
    let v = volume(part(&p, t));
    let text = serde_json::to_string(part(&p, t)).unwrap();
    let mut alone: PartDocument = serde_json::from_str(&text).unwrap();
    alone.regenerate().unwrap();
    assert!(
        alone.feature_health.is_empty(),
        "{:?}",
        alone.feature_health
    );
    assert!((volume(&alone) - v).abs() < 1e-6);
    assert_eq!(alone.context.len(), top.len());
}

#[test]
fn a_post_extruded_to_a_cover_follows_it_up() {
    use cadcraft3d::part::{PlaneRef, TargetRef};
    // A 100 × 100 × 10 cover at z 50..60 and a Ø10 post standing at the
    // origin, extruded To the cover's underside.
    let cover = PartDocument::with_box("Box1", [100.0, 100.0, 10.0]).unwrap();
    let (mut p, aid, s, t) = design(
        cover,
        Transform3::translation(-50.0, -50.0, 50.0),
        Transform3::identity(),
    );
    let a = asm(&p, aid);
    let source = ContextSource {
        assembly: aid,
        target: vec![2],
        path: vec![1],
        face: "Box1:Bottom".into(),
        edge: None,
        hint: [50.0, 50.0, 0.0],
        label: "Source:1 face".into(),
    };
    let geom = current(&p, &Linked::new(), &a, &source).unwrap();
    let tp = part_mut(&mut p, t);
    let id = tp.add_context(ContextRef {
        id: String::new(),
        source,
        geom,
    });
    let i = tp
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    tp.sketches[i]
        .sketch
        .add_circle(Point2::new(0.0, 0.0), 5.0)
        .unwrap();
    let rg = tp.sketches[i].sketch.regions();
    let mut ex = ExtrudeFeature::new(vec![ProfileRef::of(&rg[0], rg[0].interior_point())], 1.0);
    ex.extent = Extent::To(TargetRef::Plane(PlaneRef::Context(id)));
    tp.add_extrude(i, ex).unwrap();
    let (lo, hi) = bbox(part(&p, t));
    assert!(
        (lo.z).abs() < 1e-6 && (hi.z - 50.0).abs() < 1e-6,
        "{lo:?} {hi:?}"
    );
    // The cover goes up 20: the post grows to 70 after Update.
    if let Some(DocumentData::Assembly(a)) = p.document_mut(aid).map(|x| &mut x.data) {
        a.find_mut(1).unwrap().transform = Transform3::translation(-50.0, -50.0, 70.0);
    }
    let r = context::update(&mut p, &Linked::new(), aid);
    assert_eq!(r.changed, vec![t], "{r:?}");
    let (_, hi) = bbox(part(&p, t));
    assert!((hi.z - 70.0).abs() < 1e-6, "{hi:?}");
    let _ = s;
}

#[test]
fn break_link_makes_the_gasket_its_own() {
    let (mut p, aid, s, t) = design(
        flange(100.0),
        Transform3::identity(),
        Transform3::translation(0.0, 0.0, 10.0),
    );
    let top = edges_at(part(&p, s), 10.0);
    project_and_extrude(&mut p, aid, t, &top, 2.0);
    let v = volume(part(&p, t));
    let tp = part_mut(&mut p, t);
    assert!(tp.sketch_uses_context(0));
    let n = tp.break_sketch_context(0).unwrap();
    assert_eq!(n, top.len());
    assert!(tp.context.is_empty() && !tp.sketch_uses_context(0));
    tp.regenerate().unwrap();
    assert!(tp.feature_health.is_empty(), "{:?}", tp.feature_health);
    assert!((volume(part(&p, t)) - v).abs() < 1e-6);
    // The flange grows: the gasket no longer follows.
    grow(part_mut(&mut p, s), 120.0);
    let r = context::update(&mut p, &Linked::new(), aid);
    assert!(r.changed.is_empty() && r.sick.is_empty(), "{r:?}");
    let (lo, hi) = bbox(part(&p, t));
    assert!(((hi.x - lo.x) - 100.0).abs() < 1e-6);
}

#[test]
fn break_link_of_a_sketch_on_another_face_keeps_its_place() {
    use cadcraft3d::part::PlaneRef;
    let (mut p, aid, _, t) = design(
        PartDocument::with_box("Box1", [100.0, 100.0, 10.0]).unwrap(),
        Transform3::identity(),
        Transform3::translation(0.0, 0.0, 30.0),
    );
    let a = asm(&p, aid);
    let source = ContextSource {
        assembly: aid,
        target: vec![2],
        path: vec![1],
        face: "Box1:Top".into(),
        edge: None,
        hint: [50.0, 50.0, 10.0],
        label: "Source:1 face".into(),
    };
    let geom = current(&p, &Linked::new(), &a, &source).unwrap();
    let tp = part_mut(&mut p, t);
    let id = tp.add_context(ContextRef {
        id: String::new(),
        source,
        geom,
    });
    let placement = SketchPlacement {
        plane: PlaneRef::Context(id),
        ..Default::default()
    };
    let i = tp.create_sketch(placement, &opts()).unwrap();
    let z = tp.sketches[i].sketch.plane.origin.z;
    assert!((z + 20.0).abs() < 1e-9);
    assert_eq!(tp.break_sketch_context(i).unwrap(), 1);
    assert_eq!(tp.sketches[i].placement.plane, PlaneRef::Fixed);
    assert!(tp.context.is_empty());
    tp.regenerate().unwrap();
    assert!((tp.sketches[i].sketch.plane.origin.z - z).abs() < 1e-9);
}

// Project Geometry of a whole face of another component: every loop of the
// face at once (a cover taking a housing's whole rim, holes included).

/// The name of the planar face of `p` lying at height `z` and a point on it.
fn face_at(p: &PartDocument, z: Real) -> (String, [Real; 3]) {
    let o = TessellationOptions::default();
    for (f, face) in p.body.faces() {
        let Ok(m) = p.body.face_mesh(f, &o) else {
            continue;
        };
        if !m.positions.is_empty() && m.positions.iter().all(|q| (q.z - z).abs() < 1e-6) {
            let q = m.positions[0];
            return (face.name.clone().unwrap(), [q.x, q.y, q.z]);
        }
    }
    panic!("no face at z {z}");
}

/// In the target part, a sketch on XY projecting the whole face `face` of
/// the source (occurrence 1), then the region whose area is nearest `area`
/// extruded `h`. Returns the number of edges projected.
fn project_face_and_extrude(
    p: &mut Project,
    aid: DocumentId,
    t: DocumentId,
    face: (String, [Real; 3]),
    area: Real,
    h: Real,
) -> usize {
    let a = asm(p, aid);
    let source = ContextSource {
        assembly: aid,
        target: vec![2],
        path: vec![1],
        face: face.0,
        edge: None,
        hint: face.1,
        label: "Source:1 face".into(),
    };
    let edges = context::face_outline(p, &Linked::new(), &a, &source).unwrap();
    assert!(edges.iter().all(|c| c.source.label == "Source:1 edge"));
    let n = edges.len();
    let tp = part_mut(p, t);
    let i = tp
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    assert_eq!(tp.project_context_edges(i, edges, false).unwrap().len(), n);
    assert_eq!(tp.context.len(), n);
    let rg = tp.sketches[i].sketch.regions();
    let r = rg
        .iter()
        .min_by(|x, y| (x.area() - area).abs().total_cmp(&(y.area() - area).abs()))
        .unwrap();
    assert!((r.area() - area).abs() < 0.01 * area, "region {}", r.area());
    let profiles = vec![ProfileRef::of(r, r.interior_point())];
    tp.add_extrude(i, ExtrudeFeature::new(profiles, h)).unwrap();
    n
}

#[test]
fn a_gasket_takes_the_whole_top_face_of_its_flange() {
    let (mut p, aid, s, t) = design(
        flange(100.0),
        Transform3::identity(),
        Transform3::translation(0.0, 0.0, 10.0),
    );
    let hole = std::f64::consts::PI * 25.0;
    let face = face_at(part(&p, s), 10.0);
    let n = project_face_and_extrude(&mut p, aid, t, face, 10000.0 - 4.0 * hole, 2.0);
    // 4 sides + 4 rims (in pieces).
    assert!(n >= 8, "{n} edges");
    let v = volume(part(&p, t));
    let expect = (10000.0 - 4.0 * hole) * 2.0;
    assert!((v - expect).abs() < 0.01 * expect, "volume {v} vs {expect}");
    grow(part_mut(&mut p, s), 120.0);
    let r = context::update(&mut p, &Linked::new(), aid);
    assert_eq!(r.changed, vec![t]);
    assert!(r.sick.is_empty(), "{:?}", r.sick);
    let v = volume(part(&p, t));
    let expect = (14400.0 - 4.0 * hole) * 2.0;
    assert!((v - expect).abs() < 0.01 * expect, "volume {v} vs {expect}");
    assert!(part(&p, t).feature_health.is_empty());
}

/// A pipe Ø`d` with a Ø`d_in` bore, 40 long.
fn pipe(d: Real, d_in: Real) -> PartDocument {
    let mut p = PartDocument::default();
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    let sk = &mut p.sketches[i].sketch;
    sk.add_circle(Point2::new(0.0, 0.0), d / 2.0).unwrap();
    sk.add_circle(Point2::new(0.0, 0.0), d_in / 2.0).unwrap();
    let rg = sk.regions();
    let ring = rg
        .iter()
        .min_by(|a, b| a.area().total_cmp(&b.area()))
        .unwrap();
    let ring = ProfileRef::of(ring, ring.interior_point());
    p.add_extrude(i, ExtrudeFeature::new(vec![ring], 40.0))
        .unwrap();
    p
}

#[test]
fn a_washer_takes_the_end_face_of_a_pipe() {
    // A seal ring on a Ø40 / Ø30 pipe end: both rims come with the face.
    let (mut p, aid, s, t) = design(
        pipe(40.0, 30.0),
        Transform3::translation(60.0, 40.0, 0.0),
        Transform3::translation(60.0, 40.0, 40.0),
    );
    let pi = std::f64::consts::PI;
    let ring = pi * (400.0 - 225.0);
    let face = face_at(part(&p, s), 40.0);
    project_face_and_extrude(&mut p, aid, t, face, ring, 1.5);
    let v = volume(part(&p, t));
    assert!((v - ring * 1.5).abs() < 0.01 * v, "volume {v}");
    let (lo, hi) = bbox(part(&p, t));
    assert!(((hi.x - lo.x) - 40.0).abs() < 1e-3, "{lo:?} {hi:?}");
}

#[test]
fn a_cover_takes_the_rounded_rim_of_its_housing() {
    // A slot-shaped housing (two straight sides, two half rounds): the
    // cover's outline is lines and arcs, tangent where they meet.
    let mut h = PartDocument::default();
    let i = h
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    let sk = &mut h.sketches[i].sketch;
    sk.add_slot_center_to_center(Point2::new(0.0, 0.0), Point2::new(50.0, 0.0), 30.0)
        .unwrap();
    let rg = sk.regions();
    h.add_extrude(
        i,
        ExtrudeFeature::new(vec![ProfileRef::of(&rg[0], rg[0].interior_point())], 25.0),
    )
    .unwrap();
    let (mut p, aid, s, t) = design(
        h,
        Transform3::identity(),
        Transform3::translation(0.0, 0.0, 25.0),
    );
    let area = 50.0 * 30.0 + std::f64::consts::PI * 225.0;
    let face = face_at(part(&p, s), 25.0);
    let n = project_face_and_extrude(&mut p, aid, t, face, area, 3.0);
    assert!(n >= 4, "{n} edges");
    let v = volume(part(&p, t));
    assert!((v - area * 3.0).abs() < 0.01 * v, "volume {v}");
    let (lo, hi) = bbox(part(&p, t));
    assert!(
        (lo.x + 15.0).abs() < 1e-3 && (hi.x - 65.0).abs() < 1e-3,
        "{lo:?} {hi:?}"
    );
}

#[test]
fn a_turned_flange_face_is_projected_where_it_lies() {
    // The flange stands turned 30° about Z and shifted; the gasket part is
    // not turned: its outline must cover the turned flange.
    let turn = Transform3::new(
        Vector3::new(200.0, 50.0, 0.0),
        Vector3::z() * 30f64.to_radians(),
    );
    let (mut p, aid, s, t) = design(flange(100.0), turn, Transform3::translation(0.0, 0.0, 10.0));
    let hole = std::f64::consts::PI * 25.0;
    let face = face_at(part(&p, s), 10.0);
    project_face_and_extrude(&mut p, aid, t, face, 10000.0 - 4.0 * hole, 2.0);
    let back = turn.inverse() * Transform3::translation(0.0, 0.0, 10.0);
    for (_, v) in part(&p, t).body.vertices() {
        let q = back * v.point;
        assert!(
            (-1e-6..=100.0 + 1e-6).contains(&q.x) && (-1e-6..=100.0 + 1e-6).contains(&q.y),
            "{q:?}"
        );
    }
    let v = volume(part(&p, t));
    let expect = (10000.0 - 4.0 * hole) * 2.0;
    assert!((v - expect).abs() < 0.01 * expect, "volume {v} vs {expect}");
}

/// A cam plate 6 thick: a straight side from (0, 0) to (60, 0) and an
/// interpolation spline back through (35, `top`) and (10, 25).
fn cam(top: Real) -> PartDocument {
    let mut cam = PartDocument::default();
    let i = cam
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    let sk = &mut cam.sketches[i].sketch;
    let a = sk.add_point(Point2::new(0.0, 0.0));
    let b = sk.add_point(Point2::new(60.0, 0.0));
    let m = sk.add_point(Point2::new(35.0, top));
    let n = sk.add_point(Point2::new(10.0, 25.0));
    for q in [a, b, m, n] {
        sk.set_fixed(q, true).unwrap();
    }
    sk.add_line_between(a, b).unwrap();
    sk.add_interp_spline(vec![b, m, n, a]).unwrap();
    let rg = sk.regions();
    assert_eq!(rg.len(), 1);
    cam.add_extrude(
        i,
        ExtrudeFeature::new(vec![ProfileRef::of(&rg[0], rg[0].interior_point())], 6.0),
    )
    .unwrap();
    cam
}

/// The cam's outline area (its sketch region).
fn cam_area(p: &PartDocument) -> Real {
    p.sketches[0].sketch.regions()[0].area()
}

#[test]
fn a_follower_plate_takes_a_cam_face_with_a_spline_edge() {
    // A cover plate copying a cam's whole face: its straight side and its
    // free-form side (revision 51). The cam is reshaped: Update follows.
    let (mut p, aid, s, t) = design(
        cam(30.0),
        Transform3::identity(),
        Transform3::translation(0.0, 0.0, 6.0),
    );
    let area = cam_area(part(&p, s));
    let face = face_at(part(&p, s), 6.0);
    project_face_and_extrude(&mut p, aid, t, face, area, 2.0);
    let v = volume(part(&p, t));
    assert!(
        (v - area * 2.0).abs() < 0.01 * v,
        "volume {v} vs {}",
        area * 2.0
    );
    // The cam's high point raised 30 → 40.
    *part_mut(&mut p, s) = cam(40.0);
    let area = cam_area(part(&p, s));
    let r = context::update(&mut p, &Linked::new(), aid);
    assert_eq!(r.changed, vec![t], "{r:?}");
    assert!(r.sick.is_empty(), "{:?}", r.sick);
    let v = volume(part(&p, t));
    assert!(
        (v - area * 2.0).abs() < 0.01 * v,
        "volume {v} vs {}",
        area * 2.0
    );
    assert!(part(&p, t).feature_health.is_empty());
}

#[test]
fn a_spline_edge_projected_alone_survives_save_and_reload() {
    let (mut p, aid, s, t) = design(cam(30.0), Transform3::identity(), Transform3::identity());
    let spline = edges_at(part(&p, s), 6.0)
        .into_iter()
        .find(|e| {
            let a = asm(&p, aid);
            let src = ContextSource {
                assembly: aid,
                target: vec![2],
                path: vec![1],
                face: e.faces[0].clone(),
                edge: Some(e.clone()),
                hint: e.hint,
                label: "Source:1 edge".into(),
            };
            matches!(
                current(&p, &Linked::new(), &a, &src),
                Ok(cadcraft3d::part::context::ContextGeom::Curve {
                    curve: cadcraft3d::part::context::ContextCurve::Spline { .. }
                })
            )
        })
        .expect("a spline edge");
    project_and_extrude_open(&mut p, aid, t, &spline);
    let tp = part(&p, t);
    let text = serde_json::to_string(tp).unwrap();
    let mut back: PartDocument = serde_json::from_str(&text).unwrap();
    back.regenerate().unwrap();
    assert_eq!(back.context.len(), 1);
    assert!(back.sketches[0].links[0].status.is_none());
    assert_eq!(
        back.sketches[0].sketch.entities().count(),
        tp.sketches[0].sketch.entities().count()
    );
}

/// A sketch on the target's XY projecting `edge` of the source alone.
fn project_and_extrude_open(p: &mut Project, aid: DocumentId, t: DocumentId, edge: &EdgeRef) {
    let a = asm(p, aid);
    let source = ContextSource {
        assembly: aid,
        target: vec![2],
        path: vec![1],
        face: edge.faces[0].clone(),
        edge: Some(edge.clone()),
        hint: edge.hint,
        label: "Source:1 edge".into(),
    };
    let geom = current(p, &Linked::new(), &a, &source).unwrap();
    let tp = part_mut(p, t);
    let i = tp
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    tp.project_context_edge(
        i,
        ContextRef {
            id: String::new(),
            source,
            geom,
        },
        false,
    )
    .unwrap();
}

#[test]
fn projecting_a_face_is_all_or_nothing() {
    // A good edge followed by a reference that is no edge: nothing stays.
    let (mut p, aid, s, t) = design(
        flange(100.0),
        Transform3::identity(),
        Transform3::identity(),
    );
    let a = asm(&p, aid);
    let source = ContextSource {
        assembly: aid,
        target: vec![2],
        path: vec![1],
        face: "Box1:Top".into(),
        edge: None,
        hint: [50.0, 50.0, 10.0],
        label: "Source:1 face".into(),
    };
    let mut edges = context::face_outline(&p, &Linked::new(), &a, &source).unwrap();
    let plane = ContextRef {
        id: String::new(),
        geom: current(&p, &Linked::new(), &a, &source).unwrap(),
        source,
    };
    edges.push(plane);
    let _ = s;
    let tp = part_mut(&mut p, t);
    let j = tp
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    let before = serde_json::to_string(&tp.sketches[j]).unwrap();
    assert!(tp.project_context_edges(j, edges, false).is_err());
    assert!(tp.context.is_empty());
    assert!(tp.work.context_curves.is_empty());
    assert_eq!(serde_json::to_string(&tp.sketches[j]).unwrap(), before);
}

#[test]
fn a_pipe_rim_projected_onto_an_inclined_sketch_is_an_ellipse() {
    // A mitred cut planned on a plane tilted 30° across a Ø40 pipe: the rim
    // seen on it is an ellipse 40 × 40·cos 30° (a spline through it).
    let (mut p, aid, s, t) = design(tube(40.0), Transform3::identity(), Transform3::identity());
    let rim = edges_at(part(&p, s), 40.0);
    let a = asm(&p, aid);
    let refs: Vec<ContextRef> = rim
        .iter()
        .map(|e| {
            let source = ContextSource {
                assembly: aid,
                target: vec![2],
                path: vec![1],
                face: e.faces[0].clone(),
                edge: Some(e.clone()),
                hint: e.hint,
                label: "Source:1 edge".into(),
            };
            let geom = current(&p, &Linked::new(), &a, &source).unwrap();
            ContextRef {
                id: String::new(),
                source,
                geom,
            }
        })
        .collect();
    let tp = part_mut(&mut p, t);
    let i = tp
        .create_sketch(SketchPlacement::default(), &opts())
        .unwrap();
    let n = Vector3::new(0.0, -30f64.to_radians().sin(), 30f64.to_radians().cos());
    tp.sketches[i].sketch.plane =
        cadcraft3d::kernel::Plane::new(Point3::new(0.0, 0.0, 10.0), n, Vector3::x()).unwrap();
    tp.project_context_edges(i, refs, false).unwrap();
    let rg = tp.sketches[i].sketch.regions();
    let area = rg.iter().map(|r| r.area()).fold(0.0, Real::max);
    let expect = std::f64::consts::PI * 400.0 * 30f64.to_radians().cos();
    assert!(
        (area - expect).abs() < 0.01 * expect,
        "area {area} vs {expect}"
    );
}

// Project Geometry of one loop of another component's face (Ctrl+click
// on one of its edges): an outline without its holes, or one hole.

/// The loop through the edge of `p`'s source at height `z` that `pick`
/// chooses (by its hint), projected into a new sketch of the target on XY
/// (`normal` the sketch's, for the face choice); the regions' areas.
fn project_loop(
    p: &mut Project,
    aid: DocumentId,
    s: DocumentId,
    t: DocumentId,
    z: Real,
    pick: impl Fn(&[Real; 3]) -> bool,
    normal: Vector3,
) -> (usize, Vec<Real>) {
    let edge = edges_at(part(p, s), z)
        .into_iter()
        .find(|e| pick(&e.hint))
        .expect("an edge");
    let a = asm(p, aid);
    let source = ContextSource {
        assembly: aid,
        target: vec![2],
        path: vec![1],
        face: edge.faces[0].clone(),
        hint: edge.hint,
        edge: Some(edge),
        label: "Source:1 edge".into(),
    };
    let refs = context::edge_loop(p, &Linked::new(), &a, &source, Some(normal)).unwrap();
    let n = refs.len();
    let tp = part_mut(p, t);
    let i = tp
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    tp.project_context_edges(i, refs, false).unwrap();
    let areas = tp.sketches[i]
        .sketch
        .regions()
        .iter()
        .map(|r| r.area())
        .collect();
    (n, areas)
}

#[test]
fn a_cover_takes_only_the_outline_of_a_flange_not_its_holes() {
    let (mut p, aid, s, t) = design(
        flange(100.0),
        Transform3::identity(),
        Transform3::identity(),
    );
    let front = |h: &[Real; 3]| h[1].abs() < 1e-6 && (h[0] - 50.0).abs() < 1.0;
    let (n, areas) = project_loop(&mut p, aid, s, t, 10.0, front, Vector3::z());
    assert_eq!(n, 4);
    assert_eq!(areas.len(), 1);
    assert!((areas[0] - 10000.0).abs() < 1e-6, "{areas:?}");
}

#[test]
fn a_dowel_takes_one_bolt_hole_of_the_flange() {
    let (mut p, aid, s, t) = design(
        flange(100.0),
        Transform3::identity(),
        Transform3::identity(),
    );
    let near = |h: &[Real; 3]| ((h[0] - 85.0).hypot(h[1] - 85.0) - 5.0).abs() < 1e-3;
    let (_, areas) = project_loop(&mut p, aid, s, t, 10.0, near, Vector3::z());
    assert_eq!(areas.len(), 1, "{areas:?}");
    let disc = std::f64::consts::PI * 25.0;
    assert!((areas[0] - disc).abs() < 0.01 * disc, "{areas:?}");
}

#[test]
fn a_cap_takes_the_outer_rim_of_a_pipe_and_a_plug_the_inner() {
    let pi = std::f64::consts::PI;
    for (r, area) in [(20.0, pi * 400.0), (15.0, pi * 225.0)] {
        let (mut p, aid, s, t) = design(
            pipe(40.0, 30.0),
            Transform3::identity(),
            Transform3::identity(),
        );
        let on = |h: &[Real; 3]| (h[0].hypot(h[1]) - r).abs() < 1e-3;
        let (_, areas) = project_loop(&mut p, aid, s, t, 40.0, on, Vector3::z());
        assert_eq!(areas.len(), 1, "r {r}: {areas:?}");
        assert!((areas[0] - area).abs() < 0.01 * area, "r {r}: {areas:?}");
    }
}

#[test]
fn the_loop_is_taken_on_the_face_parallel_to_the_sketch() {
    // The flange's front top edge bounds the top (4 edges, 4 holes inside)
    // and the front side (100 × 10). A sketch facing −Y takes the side's loop.
    let (mut p, aid, s, _) = design(
        flange(100.0),
        Transform3::identity(),
        Transform3::identity(),
    );
    let edge = edges_at(part(&p, s), 10.0)
        .into_iter()
        .find(|h| h.hint[1].abs() < 1e-6 && (h.hint[0] - 50.0).abs() < 1.0)
        .unwrap();
    let a = asm(&p, aid);
    let source = ContextSource {
        assembly: aid,
        target: vec![2],
        path: vec![1],
        face: edge.faces[0].clone(),
        hint: edge.hint,
        edge: Some(edge),
        label: "Source:1 edge".into(),
    };
    let side = context::edge_loop(&p, &Linked::new(), &a, &source, Some(-Vector3::y())).unwrap();
    assert_eq!(side.len(), 4);
    let lowest = side
        .iter()
        .flat_map(|c| c.geom.curve().unwrap().polyline())
        .map(|q| q.z)
        .fold(Real::MAX, Real::min);
    assert!(lowest.abs() < 1e-6, "the side reaches z 0: {lowest}");
    let _ = &mut p;
}

// Cross-part references of a part in a linked part file (Inventor's file
// model: the gasket is its own file placed in the assembly's file).

/// A top file (flange + assembly) saved in a fresh folder, and a gasket
/// part file linked from it holding a sketch that projects the flange's
/// whole top face (made while it was edited in place) extruded 2: (top
/// project, its linked files, assembly id, flange id, library, gasket id).
fn linked_gasket(
    tag: &str,
) -> (
    Project,
    Linked,
    DocumentId,
    DocumentId,
    cadcraft3d::document::LibraryId,
    DocumentId,
) {
    use cadcraft3d::document::{Resolver, storage};
    let dir = std::env::temp_dir().join(format!(
        "cadcraft-linked-{tag}-{}",
        DocumentId::new_random()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let mut gasket_file = Project::new("GasketFile");
    let g = gasket_file.add_document("Gasket", DocumentData::Part(PartDocument::default()));
    let gasket_path = dir.join("Gasket.3dcadcraft");
    gasket_file.save(&gasket_path).unwrap();
    let mut top = Project::new("Top");
    let f = top.add_document("Flange", DocumentData::Part(flange(100.0)));
    let top_path = dir.join("Top.3dcadcraft");
    let lib = storage::link_file(&mut top, &top_path, &mut Resolver::new(), &gasket_path).unwrap();
    let mut a = AssemblyDocument::default();
    a.place(DocRef::local(f), "Flange", Transform3::identity(), true);
    a.place(
        DocRef {
            library: Some(lib),
            document: g,
        },
        "Gasket",
        Transform3::translation(0.0, 0.0, 10.0),
        false,
    );
    let aid = top.add_document("Top", DocumentData::Assembly(a.clone()));
    // In place: the flange's top face projected into the gasket's sketch.
    let source = ContextSource {
        assembly: aid,
        target: vec![2],
        path: vec![1],
        face: face_at(part(&top, f), 10.0).0,
        edge: None,
        hint: face_at(part(&top, f), 10.0).1,
        label: "Flange:1 face".into(),
    };
    let mut linked = Linked::new();
    linked.insert(lib, std::rc::Rc::new(gasket_file.clone()));
    let edges = context::face_outline(&top, &linked, &a, &source).unwrap();
    let gp = match gasket_file.document_mut(g).map(|d| &mut d.data) {
        Some(DocumentData::Part(p)) => p,
        _ => panic!("no gasket"),
    };
    let i = gp
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    gp.project_context_edges(i, edges, false).unwrap();
    let rg = gp.sketches[i].sketch.regions();
    let r = rg
        .iter()
        .max_by(|x, y| x.area().total_cmp(&y.area()))
        .unwrap();
    let profiles = vec![ProfileRef::of(r, r.interior_point())];
    gp.add_extrude(i, ExtrudeFeature::new(profiles, 2.0))
        .unwrap();
    gasket_file.save(&gasket_path).unwrap();
    let mut linked = Linked::new();
    linked.insert(lib, std::rc::Rc::new(gasket_file));
    (top, linked, aid, f, lib, g)
}

/// The gasket as the top file sees it: its edit when it has one.
fn gasket_of<'a>(
    top: &'a Project,
    linked: &'a Linked,
    lib: cadcraft3d::document::LibraryId,
    g: DocumentId,
) -> &'a PartDocument {
    let file = top
        .linked_edits
        .get(&lib)
        .map(|p| &**p)
        .or_else(|| linked.get(lib))
        .unwrap();
    match file.document(g).map(|d| &d.data) {
        Some(DocumentData::Part(p)) => p,
        _ => panic!("no gasket"),
    }
}

#[test]
fn a_linked_gasket_follows_its_flange_as_an_edit_of_its_file() {
    let (mut top, linked, aid, f, lib, g) = linked_gasket("follow");
    let hole = std::f64::consts::PI * 25.0;
    let v = volume(gasket_of(&top, &linked, lib, g));
    assert!((v - (10000.0 - 4.0 * hole) * 2.0).abs() < 0.01 * v, "{v}");
    // Nothing changed: no Update, no edit.
    assert!(context::out_of_date(&top, &linked, aid).is_empty());
    grow(part_mut(&mut top, f), 120.0);
    assert!(!context::out_of_date(&top, &linked, aid).is_empty());
    let r = context::update(&mut top, &linked, aid);
    assert_eq!(r.changed, vec![g], "{r:?}");
    // The linked file itself is untouched; the change is an edit of it.
    assert!(top.linked_edits.contains_key(&lib));
    let v = volume(gasket_of(&top, &linked, lib, g));
    let expect = (14400.0 - 4.0 * hole) * 2.0;
    assert!((v - expect).abs() < 0.01 * expect, "{v} vs {expect}");
    let on_disk = match linked.get(lib).unwrap().document(g).map(|d| &d.data) {
        Some(DocumentData::Part(p)) => volume(p),
        _ => panic!(),
    };
    assert!((on_disk - (10000.0 - 4.0 * hole) * 2.0).abs() < 0.01 * on_disk);
    // It settles: a second Update reads the edit, not the file.
    assert!(context::update(&mut top, &linked, aid).changed.is_empty());
    assert!(context::out_of_date(&top, &linked, aid).is_empty());
}

#[test]
fn an_edit_of_a_linked_file_is_not_saved_into_the_top_file() {
    let (mut top, linked, aid, f, lib, _) = linked_gasket("unsaved");
    grow(part_mut(&mut top, f), 120.0);
    context::update(&mut top, &linked, aid);
    assert!(top.linked_edits.contains_key(&lib));
    let text = serde_json::to_string(&top).unwrap();
    let back: Project = serde_json::from_str(&text).unwrap();
    assert!(back.linked_edits.is_empty());
    // Undo snapshots are clones: they carry the edit.
    assert!(top.clone().linked_edits.contains_key(&lib));
}

#[test]
fn a_part_cannot_build_on_a_part_that_builds_on_it() {
    // The gasket (2) projects the flange (1): the flange may not take
    // geometry from the gasket back, the gasket may take more from it.
    let (mut p, aid, s, t) = design(
        flange(100.0),
        Transform3::identity(),
        Transform3::translation(0.0, 0.0, 10.0),
    );
    let top = edges_at(part(&p, s), 10.0);
    project_and_extrude(&mut p, aid, t, &top, 2.0);
    let l = Linked::new();
    assert!(context::would_loop(&p, &l, aid, &[1], &[2]));
    assert!(!context::would_loop(&p, &l, aid, &[2], &[1]));
    // A part on itself is a loop too.
    assert!(context::would_loop(&p, &l, aid, &[1], &[1]));
}

#[test]
fn a_loop_through_a_third_part_is_found() {
    // Flange (1) ← gasket (2) ← cover (3, a sketch on the gasket's top):
    // the flange building on the cover closes the ring; a fourth, unrelated
    // part may build on any of them.
    let (mut p, aid, s, t) = design(
        flange(100.0),
        Transform3::identity(),
        Transform3::translation(0.0, 0.0, 10.0),
    );
    let top = edges_at(part(&p, s), 10.0);
    project_and_extrude(&mut p, aid, t, &top, 2.0);
    let c = p.add_document("Cover", DocumentData::Part(PartDocument::default()));
    let x = p.add_document("Bolt", DocumentData::Part(PartDocument::default()));
    if let Some(DocumentData::Assembly(a)) = p.document_mut(aid).map(|d| &mut d.data) {
        a.place(
            DocRef::local(c),
            "Cover",
            Transform3::translation(0.0, 0.0, 12.0),
            false,
        );
        a.place(
            DocRef::local(x),
            "Bolt",
            Transform3::translation(200.0, 0.0, 0.0),
            false,
        );
    }
    let a = asm(&p, aid);
    let face = face_at(part(&p, t), 2.0);
    let source = ContextSource {
        assembly: aid,
        target: vec![3],
        path: vec![2],
        face: face.0,
        edge: None,
        hint: face.1,
        label: "Target:1 face".into(),
    };
    let geom = current(&p, &Linked::new(), &a, &source).unwrap();
    part_mut(&mut p, c).add_context(ContextRef {
        id: String::new(),
        source,
        geom,
    });
    let l = Linked::new();
    assert!(context::would_loop(&p, &l, aid, &[1], &[3]));
    assert!(context::would_loop(&p, &l, aid, &[2], &[3]));
    assert!(!context::would_loop(&p, &l, aid, &[3], &[1]));
    for k in 1..=3 {
        assert!(!context::would_loop(&p, &l, aid, &[4], &[k]));
        assert!(!context::would_loop(&p, &l, aid, &[k], &[4]));
    }
}

#[test]
fn a_copy_of_a_flange_face_stays_where_it_was_taken() {
    // Cross-part geometry projection off: the gasket's outline is a copy,
    // held fixed, that no longer follows the flange.
    let (mut p, aid, s, t) = design(
        flange(100.0),
        Transform3::identity(),
        Transform3::translation(0.0, 0.0, 10.0),
    );
    let hole = std::f64::consts::PI * 25.0;
    let face = face_at(part(&p, s), 10.0);
    project_face_and_extrude(&mut p, aid, t, face, 10000.0 - 4.0 * hole, 2.0);
    let tp = part_mut(&mut p, t);
    let i = tp.sketches.len() - 1;
    assert!(tp.copy_sketch_context(i).unwrap() > 0);
    assert!(tp.context.is_empty());
    assert!(tp.sketches[i].links.is_empty());
    assert_eq!(tp.sketches[i].sketch.solve().unwrap().dof, 0);
    grow(part_mut(&mut p, s), 120.0);
    assert!(
        context::update(&mut p, &Linked::new(), aid)
            .changed
            .is_empty()
    );
    let v = volume(part(&p, t));
    let expect = (10000.0 - 4.0 * hole) * 2.0;
    assert!((v - expect).abs() < 0.01 * expect, "volume {v}");
}
