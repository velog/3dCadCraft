//! Export Face As (`docs/spec/7.7-export.md` §3a): a 100 × 60 × 5 plate with
//! a Ø20 hole and a 20 mm slot 10 wide, its faces written as DXF and SVG.

use std::f64::consts::PI;

use cadcraft3d::kernel::Real;
use cadcraft3d::part::face_export::{self, FaceFormat, Seg};
use cadcraft3d::part::{
    ExtrudeFeature, OriginPlane, PartDocument, ProfileRef, SketchOptions, SketchPlacement,
};
use cadcraft3d::sketch::Point2;

const TOP: &str = "Extrusion1:End";
const BOTTOM: &str = "Extrusion1:Start";

fn plate() -> PartDocument {
    let mut part = PartDocument::default();
    let opts = SketchOptions {
        auto_project_origin: false,
        ..Default::default()
    };
    let i = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts)
        .unwrap();
    let sk = &mut part.sketches[i].sketch;
    sk.add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(100.0, 60.0))
        .unwrap();
    sk.add_circle(Point2::new(25.0, 30.0), 10.0).unwrap();
    sk.add_slot_center_to_center(Point2::new(60.0, 30.0), Point2::new(80.0, 30.0), 10.0)
        .unwrap();
    let r = sk.regions();
    let plate = r.iter().find(|x| x.holes.len() == 2).unwrap();
    part.add_extrude(
        i,
        ExtrudeFeature::new(vec![ProfileRef::of(plate, plate.interior_point())], 5.0),
    )
    .unwrap();
    part
}

fn names(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

const HOLE: Real = PI * 100.0;
const SLOT: Real = 20.0 * 10.0 + PI * 25.0;

#[test]
fn top_face_outline_hole_and_slot_are_exact() {
    let part = plate();
    let o = face_export::outline(&part.body, &names(&[TOP])).unwrap();
    assert_eq!(o.loops.len(), 3, "{:?}", o.loops);
    assert!((o.size[0] - 100.0).abs() < 1e-6 && (o.size[1] - 60.0).abs() < 1e-6);
    let outer: Vec<_> = o.loops.iter().filter(|l| !l.inner).collect();
    assert_eq!(outer.len(), 1);
    assert!(
        (outer[0].area() - 6000.0).abs() < 1e-6,
        "{}",
        outer[0].area()
    );
    assert_eq!(outer[0].segs.len(), 4, "the outline's lines are joined");
    let mut holes: Vec<Real> = o
        .loops
        .iter()
        .filter(|l| l.inner)
        .map(|l| l.area())
        .collect();
    holes.sort_by(|a, b| b.partial_cmp(a).unwrap());
    // Holes run clockwise.
    assert!((holes[0] + SLOT).abs() < 1e-6, "{holes:?}");
    assert!((holes[1] + HOLE).abs() < 1e-6, "{holes:?}");
    // The hole is one circle, the slot two lines and two arcs, at the
    // sketch's place (the plate starts at the origin, so nothing moved).
    let circle = o.loops.iter().find_map(|l| l.circle()).unwrap();
    assert!((circle.0[0] - 25.0).abs() < 1e-6 && (circle.0[1] - 30.0).abs() < 1e-6);
    let slot = o
        .loops
        .iter()
        .find(|l| l.inner && l.circle().is_none())
        .unwrap();
    let arcs = slot
        .segs
        .iter()
        .filter(|s| matches!(s, Seg::Arc { .. }))
        .count();
    assert_eq!((slot.segs.len(), arcs), (4, 2));
}

#[test]
fn dxf_and_svg_texts() {
    let part = plate();
    let o = face_export::outline(&part.body, &names(&[TOP])).unwrap();
    let d = face_export::text(&o, FaceFormat::Dxf);
    assert_eq!(d.matches("\nCIRCLE\n").count(), 1);
    assert_eq!(d.matches("\nPOLYLINE\n").count(), 2);
    // The slot's half circles are bulges of ±1 (tan 45°).
    assert_eq!(d.matches("\n42\n").count(), 2, "{d}");
    let s = face_export::text(&o, FaceFormat::Svg);
    assert_eq!(s.matches("<circle").count(), 1);
    assert_eq!(s.matches("<path").count(), 2);
    assert_eq!(s.matches(" A ").count(), 2);
    assert!(s.contains(r#"viewBox="0 0 100.000000 60.000000""#));
    assert_eq!(
        FaceFormat::of_path(std::path::Path::new("a/b.SVG")),
        Some(FaceFormat::Svg)
    );
}

#[test]
fn bottom_face_is_seen_from_below() {
    let part = plate();
    let o = face_export::outline(&part.body, &names(&[BOTTOM])).unwrap();
    assert_eq!(o.loops.len(), 3);
    // Seen from below (x along X, y along -Y), moved to positive
    // coordinates: the hole at (25, 30) stays at (25, 60 - 30).
    let c = o.loops.iter().find_map(|l| l.circle()).unwrap();
    assert!(
        (c.0[0] - 25.0).abs() < 1e-6 && (c.0[1] - 30.0).abs() < 1e-6,
        "{c:?}"
    );
    let outer = o.loops.iter().find(|l| !l.inner).unwrap();
    assert!((outer.area() - 6000.0).abs() < 1e-6);
}

#[test]
fn several_faces_parallel_or_refused() {
    let part = plate();
    // Top and bottom: parallel; their loops lie on one another and are
    // drawn once (a cutter would go round each twice).
    let o = face_export::outline(&part.body, &names(&[TOP, BOTTOM])).unwrap();
    assert_eq!(o.loops.len(), 3);
    // A side face is not parallel to the top.
    let side = part
        .body
        .faces()
        .filter_map(|(id, f)| Some((id, f.name.clone()?)))
        .find(|(id, n)| {
            n != TOP
                && n != BOTTOM
                && part
                    .body
                    .face_plane(*id)
                    .is_some_and(|p| p.normal.z.abs() < 1e-6)
        })
        .map(|(_, n)| n)
        .unwrap();
    let err = face_export::outline(&part.body, &names(&[TOP, &side])).unwrap_err();
    assert!(err.contains("not parallel"), "{err}");
    // A hole's wall is not planar.
    let wall = part
        .body
        .faces()
        .filter_map(|(id, f)| Some((id, f.name.clone()?)))
        .find(|(id, _)| part.body.face_plane(*id).is_none())
        .map(|(_, n)| n)
        .unwrap();
    let err = face_export::outline(&part.body, &names(&[&wall])).unwrap_err();
    assert!(err.contains("not a planar face"), "{err}");
}
