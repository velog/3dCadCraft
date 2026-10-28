//! Analyze Interference on coincident and nearly coincident faces (spec 3.0 §3.4): a shaft in a
//! hole of its own diameter touches it everywhere and must report nothing (Inventor: no
//! interference for a line-on-line fit); a shaft 0.2 mm larger reports the thin ring; a block
//! sitting on another (face on face) reports nothing; one sunk 0.5 mm reports the slab.

use cadcraft3d::assembly::interference::{self as itf, Body};
use cadcraft3d::document::{DocRef, DocumentId, Occurrence};
use cadcraft3d::kernel::{Point3, Real, Transform3, Vector3};
use cadcraft3d::part::primitive::{PrimitiveFeature, PrimitiveKind, add_primitive};
use cadcraft3d::part::{
    HoleCenter, HoleFeature, HoleSpec, OriginPlane, PartDocument, SketchPlacement, Termination,
};
use cadcraft3d::sketch::Point2;

use std::f64::consts::PI;

/// A 40 × 40 × 10 plate with one Ø10 through hole in the middle.
fn plate() -> PartDocument {
    let mut p = PartDocument::with_box("Box1", [40.0, 40.0, 10.0]).unwrap();
    let (k, ids) = p
        .create_hole_sketch("Box1:Top", &[Point3::new(20.0, 20.0, 10.0)])
        .unwrap();
    let mut s = HoleSpec::simple(10.0, 10.0);
    s.termination = Termination::ThroughAll;
    let mut def = HoleFeature::new(s);
    def.sketch = p.sketches[k].id.clone();
    def.centers = ids.into_iter().map(HoleCenter::Point).collect();
    p.add_hole(def).unwrap();
    p
}

/// A shaft of diameter `d`, 30 long, standing on XY at the origin.
fn shaft(d: Real) -> PartDocument {
    let mut p = PartDocument::default();
    let f = PrimitiveFeature::extrude(&p, 30.0);
    add_primitive(
        &mut p,
        PrimitiveKind::Cylinder,
        SketchPlacement::on_origin_plane(OriginPlane::XY),
        &[Point2::new(0.0, 0.0), Point2::new(d / 2.0, 0.0)],
        f,
    )
    .unwrap();
    p
}

fn body(id: u32, t: Transform3, local: &[[Point3; 3]]) -> Body {
    let mut o = Occurrence::new(
        format!("P:{id}"),
        DocRef::local(DocumentId::new_random()),
        t,
    );
    o.id = id;
    Body::of(&o, local).unwrap()
}

fn volume(a: &Body, b: &Body) -> Real {
    itf::overlap(a, b).map_or(0.0, |o| o.volume)
}

#[test]
fn shaft_in_a_hole_of_its_own_size_does_not_interfere() {
    let plate = body(1, Transform3::identity(), &itf::part_triangles(&plate()));
    // The shaft through the hole from below, axis on the hole's.
    let at = Transform3::translation(20.0, 20.0, -10.0);
    let fit = body(2, at, &itf::part_triangles(&shaft(10.0)));
    let v = volume(&plate, &fit);
    // The display meshes of the two cylinders differ by their chords
    // (≤ ~0.01 mm): allow that much, nothing like a real overlap.
    let ring = PI * (5.1f64.powi(2) - 25.0) * 10.0;
    assert!(v < 0.05 * ring, "line-on-line fit: {v} mm³");
}

#[test]
fn an_oversize_shaft_interferes_by_its_ring() {
    let plate = body(1, Transform3::identity(), &itf::part_triangles(&plate()));
    let at = Transform3::translation(20.0, 20.0, -10.0);
    for d in [10.2, 10.5, 12.0] {
        let big = body(2, at, &itf::part_triangles(&shaft(d)));
        let v = volume(&plate, &big);
        let want = PI * ((d / 2.0).powi(2) - 25.0) * 10.0;
        assert!((v - want).abs() < 0.05 * want, "Ø{d}: {v} vs {want} mm³");
    }
}

#[test]
fn blocks_face_on_face_and_sunk() {
    let local = itf::part_triangles(&PartDocument::with_box("Box1", [40.0, 40.0, 10.0]).unwrap());
    let a = body(1, Transform3::identity(), &local);
    // On top, exactly touching; shifted sideways and turned about z too.
    for t in [
        Transform3::translation(0.0, 0.0, 10.0),
        Transform3::new(Vector3::new(13.0, -7.0, 10.0), Vector3::new(0.0, 0.0, 0.7)),
    ] {
        let b = body(2, t, &local);
        assert!(volume(&a, &b) < 1e-3, "touching: {}", volume(&a, &b));
    }
    // Sunk 0.5 mm: a 40 × 40 × 0.5 slab (straight), less where turned.
    let b = body(2, Transform3::translation(0.0, 0.0, 9.5), &local);
    let v = volume(&a, &b);
    assert!((v - 800.0).abs() < 0.01 * 800.0, "sunk: {v}");
    // Sunk 1e-3 mm: still found (a press fit), not lost in a tolerance.
    let b = body(2, Transform3::translation(0.0, 0.0, 10.0 - 1e-3), &local);
    let v = volume(&a, &b);
    assert!((v - 1.6).abs() < 0.2, "sunk a micron: {v}");
}
