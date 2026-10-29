//! Copy Object (`docs/spec/2.13-copy-object.md`): faces or bodies copied into surface bodies,
//! one composite, one solid or several solids; Delete Original; the copies follow their source
//! and keep their own face names.

mod common;
use common::*;

use cadcraft3d::kernel::{Point3, Real};
use cadcraft3d::part::copy_object::{CopyObjectOp, CopyOutput, CopySelect};
use cadcraft3d::part::shell::FaceRef;
use cadcraft3d::part::{
    BodyOp, BodyOpFeature, ExtrudeFeature, Feature, OriginPlane, PartDocument, SketchPlacement,
    ThickenOp, ThickenSelect,
};
use cadcraft3d::sketch::Point2;

/// A plate 80 × 60 × 20 (body Extrusion1).
fn base() -> PartDocument {
    plate(80.0, 60.0, 20.0)
}

/// The plate and a block 10 × 10 × 10 at x 100..110 (body Extrusion2, New Solid).
fn two_bodies() -> PartDocument {
    let mut p = base();
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    let sk = &mut p.sketches[i].sketch;
    sk.add_rectangle_two_point(Point2::new(100.0, 0.0), Point2::new(110.0, 10.0))
        .unwrap();
    let r = sk.regions();
    p.add_extrude(i, ExtrudeFeature::new(region_profiles(&r, |_| true), 10.0))
        .unwrap();
    p
}

/// The face whose display mesh has its centre nearest `at`, as a reference.
fn face_at(p: &PartDocument, at: [Real; 3]) -> FaceRef {
    let q = Point3::from(at);
    let (name, _) = p
        .body
        .faces()
        .filter_map(|(_, f)| {
            let m = f.mesh.as_ref()?;
            let c = m
                .positions
                .iter()
                .fold(nalgebra::Vector3::zeros(), |a, x| a + x.coords)
                / m.positions.len().max(1) as Real;
            Some((f.name.clone()?, (Point3::from(c) - q).norm()))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap();
    FaceRef {
        face: name,
        hint: at,
    }
}

const TOP: [Real; 3] = [40.0, 30.0, 20.0];
const BOTTOM: [Real; 3] = [40.0, 30.0, 0.0];
const FRONT: [Real; 3] = [40.0, 0.0, 10.0];
const BACK: [Real; 3] = [40.0, 60.0, 10.0];
const LEFT: [Real; 3] = [0.0, 30.0, 10.0];
const RIGHT: [Real; 3] = [80.0, 30.0, 10.0];

fn copy(p: &mut PartDocument, op: CopyObjectOp) -> Result<String, String> {
    p.add_body_op(BodyOpFeature::new(BodyOp::CopyObject(op)))
        .map_err(|e| e.to_string())
}

fn faces_op(p: &PartDocument, at: &[[Real; 3]], output: CopyOutput) -> CopyObjectOp {
    CopyObjectOp {
        select: CopySelect::Face,
        faces: at.iter().map(|a| face_at(p, *a)).collect(),
        output,
        ..Default::default()
    }
}

/// (name, surface?, face count, volume when solid) of each body.
fn summary(p: &PartDocument) -> Vec<(String, bool, usize, Real)> {
    p.exact_bodies
        .0
        .iter()
        .map(|s| {
            (
                s.name.clone().unwrap_or_default(),
                s.is_open(),
                s.face_names.len(),
                if s.is_open() {
                    0.0
                } else {
                    s.volume_at(1e-3).unwrap_or(0.0)
                },
            )
        })
        .collect()
}

fn body<'a>(s: &'a [(String, bool, usize, Real)], name: &str) -> &'a (String, bool, usize, Real) {
    s.iter()
        .find(|b| b.0 == name)
        .unwrap_or_else(|| panic!("no body {name} in {s:?}"))
}

fn close(a: Real, b: Real) -> bool {
    (a - b).abs() < 1e-3 * b.abs().max(1.0)
}

/// Contiguous faces (top + front) give one surface body; the source stays whole.
#[test]
fn contiguous_faces_into_one_surface() {
    let mut p = base();
    let op = faces_op(&p, &[TOP, FRONT], CopyOutput::Surface);
    copy(&mut p, op).unwrap();
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    let s = summary(&p);
    assert_eq!(s.len(), 2, "{s:?}");
    assert!(close(body(&s, "Extrusion1").3, 96_000.0));
    let c = body(&s, "CopyObject1");
    assert!(c.1 && c.2 == 2, "{c:?}");
    // The copy's faces have names of their own.
    let copy = p
        .exact_bodies
        .0
        .iter()
        .find(|b| b.name.as_deref() == Some("CopyObject1"))
        .unwrap();
    for n in copy.face_names.iter().flatten() {
        assert!(n.starts_with("CopyObject1:Extrusion1:"), "{n}");
    }
}

/// Faces apart (top + bottom) give one surface body each; Composite puts them in one.
#[test]
fn faces_apart_surface_and_composite() {
    let mut p = base();
    let op = faces_op(&p, &[TOP, BOTTOM], CopyOutput::Surface);
    copy(&mut p, op).unwrap();
    let s = summary(&p);
    assert_eq!(s.len(), 3, "{s:?}");
    assert_eq!(body(&s, "CopyObject1").2, 1);
    assert_eq!(body(&s, "CopyObject1#2").2, 1);

    let mut p = base();
    let op = faces_op(&p, &[TOP, BOTTOM], CopyOutput::Composite);
    copy(&mut p, op).unwrap();
    let s = summary(&p);
    assert_eq!(s.len(), 2, "{s:?}");
    let c = body(&s, "CopyObject1");
    assert!(c.1 && c.2 == 2, "{c:?}");
}

/// All six faces make a solid of the plate's volume; five do not close and are refused,
/// leaving the part as it was.
#[test]
fn solid_from_faces_must_close() {
    let mut p = base();
    let all = [TOP, BOTTOM, FRONT, BACK, LEFT, RIGHT];
    let op = faces_op(&p, &all, CopyOutput::Solid);
    copy(&mut p, op).unwrap();
    let s = summary(&p);
    let c = body(&s, "CopyObject1");
    assert!(!c.1 && close(c.3, 96_000.0), "{c:?}");

    let mut p = base();
    let before = p.features.len();
    let op = faces_op(&p, &all[..5], CopyOutput::Solid);
    let e = copy(&mut p, op).unwrap_err();
    assert!(e.contains("do not close"), "{e}");
    assert_eq!(p.features.len(), before);
    assert_eq!(summary(&p).len(), 1);

    let mut p = base();
    let op = faces_op(&p, &all, CopyOutput::MultipleSolids);
    copy(&mut p, op).unwrap();
    assert!(close(body(&summary(&p), "CopyObject1").3, 96_000.0));
}

/// Bodies: Solid joins the two copies, Multiple Solids keeps them apart, Surface copies them as
/// surface bodies.
#[test]
fn bodies_into_solid_solids_and_surfaces() {
    let names = vec!["Extrusion1".to_owned(), "Extrusion2".to_owned()];
    let op = |output| CopyObjectOp {
        select: CopySelect::Body,
        bodies: names.clone(),
        output,
        ..Default::default()
    };
    let mut p = two_bodies();
    copy(&mut p, op(CopyOutput::Solid)).unwrap();
    let s = summary(&p);
    assert_eq!(s.len(), 3, "{s:?}");
    assert!(close(body(&s, "CopyObject1").3, 97_000.0), "{s:?}");

    let mut p = two_bodies();
    copy(&mut p, op(CopyOutput::MultipleSolids)).unwrap();
    let s = summary(&p);
    assert!(close(body(&s, "CopyObject1").3, 96_000.0), "{s:?}");
    assert!(close(body(&s, "CopyObject1#2").3, 1_000.0), "{s:?}");

    let mut p = two_bodies();
    copy(&mut p, op(CopyOutput::Surface)).unwrap();
    let s = summary(&p);
    let (a, b) = (body(&s, "CopyObject1"), body(&s, "CopyObject1#2"));
    assert!(a.1 && a.2 == 6 && b.1 && b.2 == 6, "{s:?}");

    let mut p = two_bodies();
    copy(&mut p, op(CopyOutput::Composite)).unwrap();
    let s = summary(&p);
    assert_eq!(s.len(), 3, "{s:?}");
    assert_eq!(body(&s, "CopyObject1").2, 12);
}

/// Delete Original: a copied body leaves the part; copied faces leave their body (it opens).
#[test]
fn delete_original() {
    let mut p = two_bodies();
    copy(
        &mut p,
        CopyObjectOp {
            select: CopySelect::Body,
            bodies: vec!["Extrusion2".into()],
            output: CopyOutput::MultipleSolids,
            delete_original: true,
            ..Default::default()
        },
    )
    .unwrap();
    let s = summary(&p);
    assert!(s.iter().all(|b| b.0 != "Extrusion2"), "{s:?}");
    assert!(close(body(&s, "CopyObject1").3, 1_000.0));

    let mut p = base();
    let mut op = faces_op(&p, &[TOP], CopyOutput::Surface);
    op.delete_original = true;
    copy(&mut p, op).unwrap();
    let s = summary(&p);
    let src = body(&s, "Extrusion1");
    assert!(src.1 && src.2 == 5, "the plate opens: {s:?}");
    assert_eq!(body(&s, "CopyObject1").2, 1);
}

/// The copy follows its source: a taller plate gives a taller copied solid.
#[test]
fn copy_follows_the_source() {
    let mut p = base();
    let op = CopyObjectOp {
        select: CopySelect::Body,
        bodies: vec!["Extrusion1".into()],
        output: CopyOutput::Solid,
        ..Default::default()
    };
    copy(&mut p, op).unwrap();
    for f in p.features.iter_mut() {
        if let Feature::Extrude(e) = f {
            e.distance = 30.0;
        }
    }
    p.regenerate().unwrap();
    assert!(close(body(&summary(&p), "CopyObject1").3, 144_000.0));
}

/// A copied face is a surface body later features take (Thicken of it as a quilt), and the
/// feature survives a save and load.
#[test]
fn later_features_and_save_load() {
    let mut p = base();
    let op = faces_op(&p, &[TOP], CopyOutput::Surface);
    copy(&mut p, op).unwrap();
    let mut t = ThickenOp::new();
    t.select = ThickenSelect::Quilt;
    t.quilts = vec!["CopyObject1".into()];
    t.distance = 5.0;
    t.operation = cadcraft3d::part::Operation::NewSolid;
    p.add_body_op(BodyOpFeature::new(BodyOp::Thicken(t)))
        .unwrap();
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    let total: Real = summary(&p).iter().map(|b| b.3).sum();
    assert!(close(total, 96_000.0 + 80.0 * 60.0 * 5.0), "{total}");

    let text = serde_json::to_string(&p.features).unwrap();
    let back: Vec<Feature> = serde_json::from_str(&text).unwrap();
    assert_eq!(back, p.features);
}

/// Nothing picked: refused before any geometry.
#[test]
fn nothing_selected_is_refused() {
    let mut p = base();
    let e = copy(&mut p, CopyObjectOp::default()).unwrap_err();
    assert!(e.contains("select the faces or bodies"), "{e}");
}
