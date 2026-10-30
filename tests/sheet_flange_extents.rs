//! Flange Width Extents Between and Height Extents To
//! (`docs/spec/5.6-sheet-metal.md` §6.1i): a 100 × 60 Face (T 2) with a flange
//! on its y = 0 edge. Inventor 2027 for the same plate: a 30 high flange
//! 17342.478 mm³; Between work planes x = 20 and x = 70 14631.239 mm³ (the
//! flange 50 wide, the base's bend reliefs at both ends); Height To a work
//! point at z = 30 the same flange as Distance 30.

use cadcraft3d::kernel::{Point3, Real};
use cadcraft3d::part::sheet_metal::{
    FlangeRef, FlangeWidth, SheetFlangeFeature, SheetMetalDefaults,
};
use cadcraft3d::part::work::WorkPointDef;
use cadcraft3d::part::{
    OriginPlane, PartDocument, PlaneRef, PointRef, ProfileRef, SketchOptions, SketchPlacement,
    WorkDef, WorkFeature, WorkPlaneDef,
};
use cadcraft3d::sketch::Point2;

const PLATE: Real = 12000.0;
const FLANGE_30: Real = 17342.4777960769;
const BETWEEN_20_70: Real = 14631.2388980385;

fn plate() -> PartDocument {
    let mut p = PartDocument::default();
    p.make_sheet_metal(Some(SheetMetalDefaults {
        thickness: 2.0,
        k_factor: 0.44,
        ..Default::default()
    }));
    let o = SketchOptions {
        auto_project_origin: false,
        ..Default::default()
    };
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &o)
        .unwrap();
    p.sketches[i]
        .sketch
        .add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(100.0, 60.0))
        .unwrap();
    let r = p.sketches[i].sketch.regions();
    p.add_sheet_face(i, vec![ProfileRef::of(&r[0], r[0].interior_point())], false)
        .unwrap();
    p
}

fn offset(p: &mut PartDocument, base: OriginPlane, d: Real) -> String {
    p.add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
        base: PlaneRef::Origin(base),
        distance: d,
    })))
    .unwrap()
}

/// The flange on the y = 0 top edge, 30 high unless `f` changes it.
fn flange(mut p: PartDocument, f: impl FnOnce(&mut SheetFlangeFeature)) -> PartDocument {
    let (body, e) = p.sheet_edge_at(Point3::new(50.0, 0.0, 2.0)).unwrap();
    let mut fl = SheetFlangeFeature::new(&body, vec![e], 30.0);
    f(&mut fl);
    p.add_sheet_flange(fl).unwrap();
    p
}

fn volume(p: &PartDocument) -> Real {
    p.exact_bodies.0[0].volume_at(1e-3).unwrap()
}

fn near(a: Real, b: Real) -> bool {
    (a - b).abs() < 0.5
}

#[test]
fn a_plain_flange_is_inventors() {
    let p = flange(plate(), |_| {});
    assert!(near(volume(&p), FLANGE_30), "{}", volume(&p));
    assert!(near(volume(&plate()), PLATE));
}

#[test]
fn between_two_work_planes() {
    let mut p = plate();
    // The YZ plane's normal is +X: an offset d lies at x = d.
    let (a, b) = (
        offset(&mut p, OriginPlane::YZ, 20.0),
        offset(&mut p, OriginPlane::YZ, 70.0),
    );
    let p = flange(p, |f| {
        f.extents = FlangeWidth::Between;
        f.from = Some(FlangeRef::Plane(PlaneRef::Work(a)));
        f.to = Some(FlangeRef::Plane(PlaneRef::Work(b)));
    });
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    assert!(near(volume(&p), BETWEEN_20_70), "{}", volume(&p));
    // The same as Offset 20 / 30.
    let q = flange(plate(), |f| {
        f.extents = FlangeWidth::Offset;
        f.offset1 = 20.0;
        f.offset2 = 30.0;
    });
    assert!((volume(&p) - volume(&q)).abs() < 1e-6);
    // The references the other way round give the same flange.
    let mut r = plate();
    let (a, b) = (
        offset(&mut r, OriginPlane::YZ, 70.0),
        offset(&mut r, OriginPlane::YZ, 20.0),
    );
    let r = flange(r, |f| {
        f.extents = FlangeWidth::Between;
        f.from = Some(FlangeRef::Plane(PlaneRef::Work(a)));
        f.to = Some(FlangeRef::Plane(PlaneRef::Work(b)));
    });
    assert!((volume(&r) - volume(&q)).abs() < 1e-6);
}

#[test]
fn between_a_vertex_and_a_plane() {
    let mut p = plate();
    let b = offset(&mut p, OriginPlane::YZ, 70.0);
    // A work point at x = 20 on the edge's line.
    let z = offset(&mut p, OriginPlane::XY, 2.0);
    let x = offset(&mut p, OriginPlane::YZ, 20.0);
    let pt = p
        .add_work(WorkFeature::new(WorkDef::Point(
            WorkPointDef::ThreePlanes {
                planes: [
                    PlaneRef::Work(x),
                    PlaneRef::Origin(OriginPlane::XZ),
                    PlaneRef::Work(z),
                ],
            },
        )))
        .unwrap();
    let p = flange(p, |f| {
        f.extents = FlangeWidth::Between;
        f.from = Some(FlangeRef::Point(PointRef::Work(pt)));
        f.to = Some(FlangeRef::Plane(PlaneRef::Work(b)));
    });
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    assert!(near(volume(&p), BETWEEN_20_70), "{}", volume(&p));
}

#[test]
fn height_to_a_point_or_a_plane() {
    // A work point at (5, 0, 30): the flange's top at z = 30.
    let mut p = plate();
    let z = offset(&mut p, OriginPlane::XY, 30.0);
    let x = offset(&mut p, OriginPlane::YZ, 5.0);
    let pt = p
        .add_work(WorkFeature::new(WorkDef::Point(
            WorkPointDef::ThreePlanes {
                planes: [
                    PlaneRef::Work(x),
                    PlaneRef::Origin(OriginPlane::XZ),
                    PlaneRef::Work(z.clone()),
                ],
            },
        )))
        .unwrap();
    let mut q = p.clone();
    let p = flange(p, |f| {
        f.height = 10.0;
        f.height_to = Some(FlangeRef::Point(PointRef::Work(pt)));
    });
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    assert!(near(volume(&p), FLANGE_30), "{}", volume(&p));
    // A work plane at z = 30.
    let (body, e) = q.sheet_edge_at(Point3::new(50.0, 0.0, 2.0)).unwrap();
    let mut fl = SheetFlangeFeature::new(&body, vec![e], 10.0);
    fl.height_to = Some(FlangeRef::Plane(PlaneRef::Work(z)));
    q.add_sheet_flange(fl).unwrap();
    assert!(near(volume(&q), FLANGE_30), "{}", volume(&q));
    let top = q.body.bounding_box().unwrap().1.z;
    assert!((top - 30.0).abs() < 1e-6, "{top}");
}

#[test]
fn bad_references_are_refused() {
    let mut p = plate();
    // A plane parallel to the edge for Between, a point below the plate for Height To.
    let along = offset(&mut p, OriginPlane::XZ, -30.0);
    let (body, e) = p.sheet_edge_at(Point3::new(50.0, 0.0, 2.0)).unwrap();
    let mut fl = SheetFlangeFeature::new(&body, vec![e.clone()], 30.0);
    fl.extents = FlangeWidth::Between;
    fl.from = Some(FlangeRef::Plane(PlaneRef::Work(along.clone())));
    fl.to = Some(FlangeRef::Plane(PlaneRef::Work(along)));
    assert!(p.clone().add_sheet_flange(fl).is_err());
    let below = offset(&mut p, OriginPlane::XY, -10.0);
    let mut fl = SheetFlangeFeature::new(&body, vec![e], 30.0);
    fl.height_to = Some(FlangeRef::Plane(PlaneRef::Work(below)));
    let n = p.features.len();
    assert!(p.add_sheet_flange(fl).is_err());
    assert_eq!(p.features.len(), n);
}
