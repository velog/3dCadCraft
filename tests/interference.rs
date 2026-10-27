//! Analyze Interference on parts with holes (docs/spec/3.0 §3.4): the
//! overlap of two plates with holes along their ends. A first version cut
//! each mesh down to the triangles near the common box; the BSP then
//! misjudged the inside and lost 20 % of the volume, or the whole overlap.

use cadcraft3d::assembly::interference::{self as itf, Body};
use cadcraft3d::document::{DocRef, DocumentId, Occurrence};
use cadcraft3d::kernel::{Point3, Real, Transform3};
use cadcraft3d::part::{HoleCenter, HoleFeature, HoleSpec, PartDocument, Termination};

/// A 100 × 100 × 10 plate with Ø5.5 through holes at x = 10, 50 and 90.
fn plate() -> PartDocument {
    let mut p = PartDocument::with_box("Box1", [100.0, 100.0, 10.0]).unwrap();
    let pts: Vec<Point3> = (0..18)
        .map(|i| {
            Point3::new(
                10.0 + 40.0 * (i % 3) as Real,
                10.0 + 16.0 * (i / 3) as Real,
                10.0,
            )
        })
        .collect();
    let (k, ids) = p.create_hole_sketch("Box1:Top", &pts).unwrap();
    let mut s = HoleSpec::simple(5.5, 10.0);
    s.termination = Termination::ThroughAll;
    let mut def = HoleFeature::new(s);
    def.sketch = p.sketches[k].id.clone();
    def.centers = ids.into_iter().map(HoleCenter::Point).collect();
    p.add_hole(def).unwrap();
    p
}

fn at(id: u32, x: Real, y: Real, local: &[[Point3; 3]]) -> Body {
    let mut o = Occurrence::new(
        format!("Plate:{id}"),
        DocRef::local(DocumentId::new_random()),
        Transform3::translation(x, y, 0.0),
    );
    o.id = id;
    Body::of(&o, local).unwrap()
}

#[test]
fn plates_with_holes_overlapping_at_their_ends() {
    let local = itf::part_triangles(&plate());
    let hole = std::f64::consts::PI * 2.75 * 2.75 * 10.0;
    let a = at(1, 0.0, 0.0, &local);
    // 10 mm of overlap: the strip minus half of each end hole of both
    // plates (6 + 6 halves).
    let b = at(2, 90.0, 0.0, &local);
    let v = itf::overlap(&a, &b).expect("overlap").volume;
    let want = 10.0 * 100.0 * 10.0 - 6.0 * hole;
    assert!((v - want).abs() < 0.005 * want, "{v} vs {want}");
    // 50 mm: the middle and end holes of A, the start and middle ones of B
    // (all whole, none shared).
    let b = at(2, 50.0, 0.0, &local);
    let v = itf::overlap(&a, &b).expect("overlap").volume;
    let want = 50.0 * 100.0 * 10.0 - (3.0 + 6.0 + 6.0 + 3.0) * hole;
    assert!((v - want).abs() < 0.005 * want, "{v} vs {want}");
    // Side by side, touching: nothing.
    let b = at(2, 100.0, 0.0, &local);
    assert!(itf::overlap(&a, &b).is_none());
}
