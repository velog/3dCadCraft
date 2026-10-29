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
