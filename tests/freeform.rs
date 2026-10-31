//! Freeform (`docs/spec/5.5-freeform.md`): a control cage's cubic B-spline sheet. Areas are
//! measured on the display triangles.

mod common;

use cadcraft3d::kernel::Real;
use cadcraft3d::part::freeform::FreeformOp;
use cadcraft3d::part::{BodyOp, BodyOpFeature, OriginPlane, PartDocument, PlaneRef};
use common::*;

fn sheet(p: &PartDocument, name: &str) -> Vec<[cadcraft3d::kernel::Point3; 3]> {
    p.exact_bodies
        .0
        .iter()
        .find(|s| s.name.as_deref() == Some(name))
        .unwrap()
        .triangles()
        .unwrap()
}

fn area(t: &[[cadcraft3d::kernel::Point3; 3]]) -> Real {
    t.iter()
        .map(|[a, b, c]| (b - a).cross(&(c - a)).norm() / 2.0)
        .sum()
}

#[test]
fn freeform_plane_at_rest_is_flat_and_a_pulled_point_raises_it() {
    let mut p = PartDocument::default();
    let op = FreeformOp::plane(PlaneRef::Origin(OriginPlane::XY), 40.0, 20.0);
    let id = p
        .add_body_op(BodyOpFeature::new(BodyOp::Freeform(op.clone())))
        .unwrap();
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    let t = sheet(&p, &id);
    assert!((area(&t) - 800.0).abs() < 1e-6, "{}", area(&t));
    assert!(t.iter().flatten().all(|q| q.z.abs() < 1e-9));
    assert!(p.body.find_face(&format!("{id}:Face")).is_some());
    // The middle point pulled 10 up: the sheet rises in the middle, less than the point (a
    // B-spline lies inside its cage), its border stays on the plane.
    let mut pulled = op.clone();
    pulled.set_offset(2, 2, [0.0, 0.0, 10.0]);
    let k = p.features.len() - 1;
    let mut f = BodyOpFeature::new(BodyOp::Freeform(pulled.clone()));
    if let cadcraft3d::part::Feature::BodyOp(b) = &p.features[k] {
        f.id = b.id.clone();
        f.name = b.name.clone();
    }
    p.replace_feature(k, cadcraft3d::part::Feature::BodyOp(f))
        .unwrap();
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    let t = sheet(&p, &id);
    let zmax = t.iter().flatten().map(|q| q.z).fold(Real::MIN, Real::max);
    assert!(zmax > 2.0 && zmax < 10.0, "{zmax}");
    let rim = t
        .iter()
        .flatten()
        .filter(|q| (q.x.abs() - 20.0).abs() < 1e-9 || (q.y.abs() - 10.0).abs() < 1e-9);
    assert!(
        rim.into_iter().all(|q| q.z.abs() < 1e-9),
        "the border stays"
    );
    // Saved and loaded, the same.
    let text = serde_json::to_string(&p.features).unwrap();
    let back: Vec<cadcraft3d::part::Feature> = serde_json::from_str(&text).unwrap();
    assert_eq!(back, p.features);
}

#[test]
fn freeform_refuses_bad_sizes_and_changes_nothing() {
    let mut p = PartDocument::default();
    let mut op = FreeformOp::plane(PlaneRef::Origin(OriginPlane::XY), 40.0, 20.0);
    op.length_faces = 0;
    assert!(
        p.add_body_op(BodyOpFeature::new(BodyOp::Freeform(op)))
            .is_err()
    );
    assert!(p.features.is_empty());
    let _ = opts();
}

#[test]
fn freeform_cylinder_closes_around_and_bulges_where_pulled() {
    let mut p = PartDocument::default();
    let mut op = FreeformOp::cylinder(PlaneRef::Origin(OriginPlane::XY), 40.0, 30.0);
    let id = p
        .add_body_op(BodyOpFeature::new(BodyOp::Freeform(op.clone())))
        .unwrap();
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    let t = sheet(&p, &id);
    // A closed tube inside its cage (a cubic B-spline ring of 8 points on r 20 runs at ~r 18.3),
    // from z = 0 to 30; two faces sharing their seams (no free edge along the height).
    let r: Vec<Real> = t.iter().flatten().map(|q| q.x.hypot(q.y)).collect();
    let (rmin, rmax) = r
        .iter()
        .fold((Real::MAX, Real::MIN), |(a, b), x| (a.min(*x), b.max(*x)));
    assert!(rmin > 17.0 && rmax < 20.0, "{rmin} {rmax}");
    let zs: Vec<Real> = t.iter().flatten().map(|q| q.z).collect();
    let (zmin, zmax) = zs
        .iter()
        .fold((Real::MAX, Real::MIN), |(a, b), x| (a.min(*x), b.max(*x)));
    assert!(zmin > -1e-6 && zmax < 30.0 + 1e-6, "{zmin} {zmax}");
    let free = p
        .body
        .edges()
        .filter(|(e, _)| p.body.edge_faces(*e).len() == 1)
        .count();
    assert_eq!(
        free, 4,
        "two rims, each in two halves; the seams are shared"
    );
    // One ring point at mid height pulled outwards 10: the tube bulges there.
    op.set_offset(0, 2, [10.0, 0.0, 0.0]);
    let mut p2 = PartDocument::default();
    let id2 = p2
        .add_body_op(BodyOpFeature::new(BodyOp::Freeform(op)))
        .unwrap();
    let xmax = sheet(&p2, &id2)
        .iter()
        .flatten()
        .map(|q| q.x)
        .fold(Real::MIN, Real::max);
    assert!(xmax > rmax + 2.0, "{xmax}");
}
