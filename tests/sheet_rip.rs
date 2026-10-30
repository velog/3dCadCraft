//! Rip (`docs/spec/5.6-sheet-metal.md` §6.1g): a closed rounded tube (outer
//! 100 × 60, corner radius 5, wall 2, height 80) converted to sheet metal
//! does not unfold until a rip opens it. The volumes, the gap's place and the
//! flat sizes are Inventor 2027's for the same tube and rips (K-factor 0.5,
//! GapSize 2).

use cadcraft3d::kernel::{Point3, Real, Surface};
use cadcraft3d::part::sheet_metal::{
    RipSide, RipType, SheetConvertFeature, SheetMetalDefaults, SheetOp, SheetOpFeature,
};
use cadcraft3d::part::shell::FaceRef;
use cadcraft3d::part::{
    ExtrudeFeature, OriginPlane, PartDocument, ProfileRef, SketchOptions, SketchPlacement,
};
use cadcraft3d::sketch::{Point2, Sketch};

const TUBE: Real = 48821.2385965949;

fn rule() -> SheetMetalDefaults {
    SheetMetalDefaults {
        thickness: 2.0,
        k_factor: 0.5,
        gap: Some(2.0),
        ..Default::default()
    }
}

/// A rounded rectangle from (x0, y0) to (x1, y1) with corner radius `r`.
fn rounded(sk: &mut Sketch, x0: Real, y0: Real, x1: Real, y1: Real, r: Real) {
    let mut pt = |x: Real, y: Real| sk.add_point(Point2::new(x, y));
    let p = [
        pt(x0 + r, y0),
        pt(x1 - r, y0),
        pt(x1, y0 + r),
        pt(x1, y1 - r),
        pt(x1 - r, y1),
        pt(x0 + r, y1),
        pt(x0, y1 - r),
        pt(x0, y0 + r),
    ];
    let c = [
        pt(x1 - r, y0 + r),
        pt(x1 - r, y1 - r),
        pt(x0 + r, y1 - r),
        pt(x0 + r, y0 + r),
    ];
    for k in 0..4 {
        sk.add_line_between(p[2 * k], p[2 * k + 1]).unwrap();
        sk.add_arc(c[k], p[2 * k + 1], p[(2 * k + 2) % 8]).unwrap();
    }
}

/// The tube converted from its face at y = 60 (never ripped here).
fn tube() -> PartDocument {
    let mut p = PartDocument::default();
    p.make_sheet_metal(Some(rule()));
    let o = SketchOptions {
        auto_project_origin: false,
        ..Default::default()
    };
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &o)
        .unwrap();
    let sk = &mut p.sketches[i].sketch;
    rounded(sk, 0.0, 0.0, 100.0, 60.0, 5.0);
    rounded(sk, 2.0, 2.0, 98.0, 58.0, 3.0);
    let regions = sk.regions();
    let ring = regions
        .iter()
        .min_by(|a, b| a.area().total_cmp(&b.area()))
        .unwrap();
    p.add_extrude(
        i,
        ExtrudeFeature::new(vec![ProfileRef::of(ring, ring.interior_point())], 80.0),
    )
    .unwrap();
    let body = p.exact_bodies.0[0].name.clone().unwrap();
    let hint = [50.0, 60.0, 40.0];
    p.add_sheet_convert(SheetConvertFeature {
        id: String::new(),
        name: String::new(),
        body,
        face: FaceRef {
            face: face_at(&p, hint),
            hint,
        },
        bends: Vec::new(),
        recognize: true,
    })
    .unwrap();
    p
}

/// The name of the face through `at` (a plane or a cylinder).
fn face_at(p: &PartDocument, at: [Real; 3]) -> String {
    let q = Point3::from(at);
    p.body
        .faces()
        .filter_map(|(_, f)| {
            let d = match p.body.surface(f.surface)? {
                Surface::Plane(pl) => (q - pl.origin).dot(&pl.normal).abs(),
                Surface::Cylinder(c) => {
                    let w = q - c.origin;
                    let a = c.axis.into_inner();
                    ((w - a * w.dot(&a)).norm() - c.radius).abs()
                }
                _ => return None,
            };
            // Within the face's box.
            let pts: Vec<Point3> = f
                .outer
                .coedges
                .iter()
                .filter_map(|c| p.body.edge(c.edge))
                .filter_map(|e| p.body.vertex(e.start).map(|v| v.point))
                .collect();
            let lo = pts.iter().fold(Point3::new(1e9, 1e9, 1e9), |a, q| a.inf(q));
            let hi = pts
                .iter()
                .fold(Point3::new(-1e9, -1e9, -1e9), |a, q| a.sup(q));
            let inside = (0..3).all(|k| q[k] >= lo[k] - 1e-6 && q[k] <= hi[k] + 1e-6);
            (d < 1e-6 && inside).then(|| f.name.clone()).flatten()
        })
        .next()
        .unwrap_or_else(|| panic!("no face through {at:?}"))
}

fn rip(
    mut p: PartDocument,
    face: [Real; 3],
    rip_type: RipType,
    points: Vec<[Real; 3]>,
    gap: Option<Real>,
    side: RipSide,
) -> PartDocument {
    let body = p.sheet_models.keys().next().unwrap().clone();
    let face = FaceRef {
        face: face_at(&p, face),
        hint: face,
    };
    p.add_sheet_op(SheetOpFeature::new(SheetOp::Rip {
        body,
        face,
        rip_type,
        points,
        gap,
        side,
    }))
    .unwrap();
    p
}

fn volume(p: &PartDocument) -> Real {
    p.exact_bodies.0[0].volume_at(1e-3).unwrap()
}

/// The flat's length and width.
fn flat(p: &PartDocument) -> (Real, Real) {
    let s = p.flat_patterns()[0].size;
    (s.0.max(s.1), s.0.min(s.1))
}

/// Whether the solid has a planar wall square to `axis` at `value`.
fn wall(p: &PartDocument, axis: usize, value: Real) -> bool {
    p.body.faces().any(|(_, f)| {
        matches!(p.body.surface(f.surface), Some(Surface::Plane(pl))
            if pl.normal[axis].abs() > 1.0 - 1e-9 && (pl.origin[axis] - value).abs() < 1e-6)
    })
}

fn near(a: Real, b: Real, tol: Real) -> bool {
    (a - b).abs() <= tol
}

#[test]
fn a_closed_tube_needs_a_rip() {
    let p = tube();
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    assert!(near(volume(&p), TUBE, 0.5), "{}", volume(&p));
    assert!(p.flat_patterns().is_empty());
    let why = p.flat_pattern_dxf(p.sheet_models.keys().next().unwrap());
    assert!(
        why.as_ref()
            .is_err_and(|e| e.contains("Rip") && e.contains("1 bend closes")),
        "{why:?}"
    );
}

#[test]
fn single_point_rips_square_to_the_edge() {
    // (face point, rip point, walls of the gap (axis, values)).
    type Case = ([Real; 3], [Real; 3], usize, [Real; 2]);
    let cases: [Case; 4] = [
        ([50.0, 0.0, 40.0], [50.0, 0.0, 80.0], 0, [48.0, 50.0]),
        ([50.0, 0.0, 40.0], [50.0, 0.0, 0.0], 0, [50.0, 52.0]),
        ([50.0, 2.0, 40.0], [50.0, 2.0, 80.0], 0, [50.0, 52.0]),
        ([0.0, 30.0, 40.0], [0.0, 30.0, 80.0], 1, [30.0, 32.0]),
    ];
    for (face, at, axis, walls) in cases {
        let p = rip(
            tube(),
            face,
            RipType::SinglePoint,
            vec![at],
            None,
            RipSide::Default,
        );
        assert!(
            p.feature_health.is_empty(),
            "{at:?}: {:?}",
            p.feature_health
        );
        assert!(
            near(volume(&p), 48501.2385965949, 0.5),
            "{at:?}: {}",
            volume(&p)
        );
        let (l, w) = flat(&p);
        assert!(
            near(l, 303.132741228718, 1e-3) && near(w, 80.0, 1e-3),
            "{at:?}: {l} x {w}"
        );
        for v in walls {
            assert!(wall(&p, axis, v), "{at:?}: no wall at {v}");
        }
        assert_eq!(p.flat_patterns()[0].bends.len(), 4, "{at:?}");
    }
}

#[test]
fn the_gap_and_its_side() {
    let cases = [
        (RipSide::Default, [49.0, 50.0]),
        (RipSide::Flipped, [50.0, 51.0]),
        (RipSide::Both, [49.5, 50.5]),
    ];
    for (side, walls) in cases {
        let p = rip(
            tube(),
            [50.0, 0.0, 40.0],
            RipType::SinglePoint,
            vec![[50.0, 0.0, 80.0]],
            Some(1.0),
            side,
        );
        assert!(
            p.feature_health.is_empty(),
            "{side:?}: {:?}",
            p.feature_health
        );
        assert!(
            near(volume(&p), 48661.2385965949, 0.5),
            "{side:?}: {}",
            volume(&p)
        );
        let (l, _) = flat(&p);
        assert!(near(l, 304.132741228718, 1e-3), "{side:?}: {l}");
        for v in walls {
            assert!(wall(&p, 0, v), "{side:?}: no wall at {v}");
        }
    }
}

#[test]
fn point_to_point_rips() {
    // Straight up the face: as the single point rip from the bottom edge.
    let p = rip(
        tube(),
        [50.0, 0.0, 40.0],
        RipType::PointToPoint,
        vec![[50.0, 0.0, 0.0], [50.0, 0.0, 80.0]],
        None,
        RipSide::Default,
    );
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    assert!(wall(&p, 0, 50.0) && wall(&p, 0, 52.0));
    // Slanted: the gap is 2 wide square to the line, the flat a parallelogram.
    for (gap, side, v, len) in [
        (None, RipSide::Default, 48463.467720195, 342.896673251219),
        (Some(1.0), RipSide::Both, 48642.353158395, 344.014707239968),
    ] {
        let p = rip(
            tube(),
            [50.0, 0.0, 40.0],
            RipType::PointToPoint,
            vec![[30.0, 0.0, 80.0], [70.0, 0.0, 0.0]],
            gap,
            side,
        );
        assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
        assert!(near(volume(&p), v, 0.5), "{}", volume(&p));
        let (l, w) = flat(&p);
        assert!(near(l, len, 1e-3) && near(w, 80.0, 1e-3), "{l} x {w}");
    }
}

#[test]
fn face_extents_removes_a_bend() {
    let s = 5.0 - 5.0 * 0.5f64.sqrt();
    let p = rip(
        tube(),
        [s, s, 40.0],
        RipType::FaceExtents,
        Vec::new(),
        None,
        RipSide::Default,
    );
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    assert!(near(volume(&p), 47815.9289474462, 0.5), "{}", volume(&p));
    let (l, w) = flat(&p);
    assert!(
        near(l, 298.849555921539, 1e-3) && near(w, 80.0, 1e-3),
        "{l} x {w}"
    );
    assert_eq!(p.flat_patterns()[0].bends.len(), 3);
}

#[test]
fn rip_errors_leave_the_part_unchanged() {
    let p = tube();
    let body = p.sheet_models.keys().next().unwrap().clone();
    let face = FaceRef {
        face: face_at(&p, [50.0, 0.0, 40.0]),
        hint: [50.0, 0.0, 40.0],
    };
    // A point inside the face, on none of its edges.
    let mut q = p.clone();
    let r = q.add_sheet_op(SheetOpFeature::new(SheetOp::Rip {
        body,
        face,
        rip_type: RipType::SinglePoint,
        points: vec![[50.0, 0.0, 40.0]],
        gap: None,
        side: RipSide::Default,
    }));
    assert!(r.is_err(), "{r:?}");
    assert_eq!(q.features.len(), p.features.len());
}

/// A rip of a sheet that unfolds already: its gap is cut through the face
/// and stays in the flat pattern (a slot 2 wide, 30 long from the edge).
#[test]
fn a_rip_of_an_open_sheet_is_a_slot() {
    let mut p = PartDocument::default();
    p.make_sheet_metal(Some(rule()));
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
    let before = volume(&p);
    let area = p.flat_extents().map(|e| e.area).unwrap();
    let p = rip(
        p,
        [20.0, 20.0, 2.0],
        RipType::PointToPoint,
        vec![[50.0, 0.0, 2.0], [50.0, 30.0, 2.0]],
        None,
        RipSide::Both,
    );
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    assert!(
        near(before - volume(&p), 2.0 * 30.0 * 2.0, 0.5),
        "{}",
        before - volume(&p)
    );
    assert!(wall(&p, 0, 49.0) && wall(&p, 0, 51.0));
    let after = p.flat_extents().map(|e| e.area).unwrap();
    assert!(near(area - after, 60.0, 1e-3), "{area} -> {after}");
}
