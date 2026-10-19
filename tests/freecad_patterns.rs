//! Test recipes 25–48 of `docs/FREECAD_ISSUES.md` (Pattern, Mirror, Hole;
//! FreeCAD issue numbers in the comments), plus pattern / hole
//! combinations with the other features. Only the issue *text* was used.

mod common;
use common::*;

use cadcraft3d::part::hole::HoleCenter;
use cadcraft3d::part::standards::Standards;
use cadcraft3d::part::{
    HoleFeature, HoleSpec, LineRef, PatternDirection, PatternFeature, PatternKind, PullRef,
    SeatType, Termination,
};

const X: PullRef = PullRef::Line(LineRef::Origin(0));
const Y: PullRef = PullRef::Line(LineRef::Origin(1));

/// A through hole r at (x, y) on the face named `face` at height z (a cut
/// through all); returns the feature id.
fn hole(part: &mut PartDocument, face: &str, z: Real, x: Real, y: Real, r: Real) -> String {
    let n = part.features.len();
    let j = sketch_on(part, face, |sk, at| {
        sk.add_circle(at(x, y, z), r).unwrap();
    });
    cut(part, j, None, |rg| {
        rg.holes.is_empty() && rg.area() < PI * r * r * 1.01
    });
    part.features[n].id().to_owned()
}

fn rect(
    part: &mut PartDocument,
    features: &[&str],
    first: (PullRef, u32, Real),
    second: Option<(PullRef, u32, Real)>,
) -> Result<String, String> {
    let mut p = PatternFeature::rectangular();
    p.features = features.iter().map(|s| s.to_string()).collect();
    let dir = |(d, n, s): (PullRef, u32, Real)| PatternDirection {
        dir: Some(d),
        count: n,
        spacing: s,
        ..PatternDirection::new()
    };
    p.kind = PatternKind::Rectangular {
        first: dir(first),
        second: second.map(dir),
    };
    part.add_pattern(p).map_err(|e| e.to_string())
}

fn circ(part: &mut PartDocument, features: &[&str], n: u32) -> Result<String, String> {
    let mut p = PatternFeature::circular();
    p.features = features.iter().map(|s| s.to_string()).collect();
    if let PatternKind::Circular { axis, count, .. } = &mut p.kind {
        (*axis, *count) = (Some(LineRef::Origin(2)), n);
    }
    part.add_pattern(p).map_err(|e| e.to_string())
}

fn mirror(
    part: &mut PartDocument,
    features: &[&str],
    plane: OriginPlane,
) -> Result<String, String> {
    let mut p = PatternFeature::mirror();
    p.features = features.iter().map(|s| s.to_string()).collect();
    p.kind = PatternKind::Mirror {
        plane: Some(PlaneRef::Origin(plane)),
    };
    part.add_pattern(p).map_err(|e| e.to_string())
}

/// A disc R × h on XY at the origin.
fn disc(r: Real, h: Real) -> PartDocument {
    let mut p = PartDocument::default();
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    p.sketches[i]
        .sketch
        .add_circle(Point2::new(0.0, 0.0), r)
        .unwrap();
    let reg = p.sketches[i].sketch.regions();
    p.add_extrude(i, ExtrudeFeature::new(region_profiles(&reg, |_| true), h))
        .unwrap();
    p
}

/// Hole feature `spec` at `centres` (x, y) on the face named `face` at z.
fn hole_feature(
    part: &mut PartDocument,
    face: &str,
    z: Real,
    spec: HoleSpec,
    centres: &[(Real, Real)],
) -> Result<String, String> {
    let pts: Vec<Point3> = centres.iter().map(|&(x, y)| Point3::new(x, y, z)).collect();
    let (i, ids) = part
        .create_hole_sketch(face, &pts)
        .map_err(|e| e.to_string())?;
    let mut def = HoleFeature::new(spec);
    def.sketch = part.sketches[i].id.clone();
    def.centers = ids.into_iter().map(HoleCenter::Point).collect();
    part.add_hole(def).map_err(|e| e.to_string())
}

// ----------------------------------------------------- Pattern / Mirror ---

/// Recipe 25 (FreeCAD #7791, #18473): a pattern of count 1 leaves the body;
/// count 0 is refused with a message, the part unchanged.
#[test]
fn r25_pattern_count_one_and_zero() {
    let mut p = plate(40.0, 30.0, 10.0);
    let t = top(&p, 10.0);
    let h = hole(&mut p, &t, 10.0, 10.0, 15.0, 3.0);
    let v = volume(&p);
    match rect(&mut p, &[&h], (X, 1, 10.0), None) {
        Ok(_) => check(&p, v, 1e-9),
        Err(e) => assert!(!e.is_empty()),
    }
    let mut q = plate(40.0, 30.0, 10.0);
    let t = top(&q, 10.0);
    let h = hole(&mut q, &t, 10.0, 10.0, 15.0, 3.0);
    let n = q.features.len();
    assert!(rect(&mut q, &[&h], (X, 0, 10.0), None).is_err());
    assert_eq!(q.features.len(), n);
}

/// Recipe 26 (FreeCAD #23975): plate 120×20×5, a Ø5 hole patterned 10
/// times every 10 → 12 000 − 10·π·2.5²·5.
#[test]
fn r26_ten_holes_in_a_row() {
    let mut p = plate(120.0, 20.0, 5.0);
    let t = top(&p, 5.0);
    let h = hole(&mut p, &t, 5.0, 10.0, 10.0, 2.5);
    rect(&mut p, &[&h], (X, 10, 10.0), None).unwrap();
    check(&p, 12_000.0 - 10.0 * PI * 6.25 * 5.0, 2e-4);
}

/// Recipe 27 (FreeCAD #18720): a disc R30 × 5 with a tooth 6×4×5 on its
/// rim, Polar Pattern N = 2, 3, 7, 12: V = disc + N·(tooth outside).
#[test]
fn r27_polar_teeth_counts() {
    // The tooth's part outside the disc: [28, 34] × [−2, 2] less r ≤ 30.
    let n = 4000;
    let h = 4.0 / n as Real;
    let outside: Real = (0..n)
        .map(|i| {
            let y = -2.0 + (i as Real + 0.5) * h;
            (34.0 - (900.0 - y * y).sqrt()) * h
        })
        .sum();
    for count in [2, 3, 7, 12] {
        let mut p = disc(30.0, 5.0);
        let t = top(&p, 5.0);
        let _ = t;
        let i = p
            .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
            .unwrap();
        p.sketches[i]
            .sketch
            .add_rectangle_two_point(Point2::new(28.0, -2.0), Point2::new(34.0, 2.0))
            .unwrap();
        let reg = p.sketches[i].sketch.regions();
        let mut ex = ExtrudeFeature::new(region_profiles(&reg, |_| true), 5.0);
        ex.operation = Operation::Join;
        p.add_extrude(i, ex).unwrap();
        let tooth = p.features[1].id().to_owned();
        circ(&mut p, &[&tooth], count).unwrap();
        check(&p, PI * 900.0 * 5.0 + count as Real * outside * 5.0, 3e-4);
        assert_eq!(p.body.solids().count(), 1, "N {count}");
    }
}

/// Recipe 28 (FreeCAD #27252): a tapered boss (R3, −10°, 5 high: a
/// frustum) on a disc, Polar Pattern 2 … 12: all copies there.
#[test]
fn r28_polar_pattern_of_a_tapered_boss() {
    let fr = |a: Real, b: Real, h: Real| PI * h / 3.0 * (a * a + a * b + b * b);
    for count in [2, 5, 12] {
        let mut p = disc(30.0, 5.0);
        let t = top(&p, 5.0);
        let j = sketch_on(&mut p, &t, |sk, at| {
            sk.add_circle(at(20.0, 0.0, 5.0), 3.0).unwrap();
        });
        let reg = p.sketches[j].sketch.regions();
        let mut ex = ExtrudeFeature::new(
            region_profiles(&reg, |r| r.holes.is_empty() && r.area() < 30.0),
            5.0,
        );
        ex.operation = Operation::Join;
        ex.taper = (-10f64).to_radians();
        p.add_extrude(j, ex).unwrap();
        let boss = p.features[1].id().to_owned();
        circ(&mut p, &[&boss], count).unwrap();
        let cone = fr(3.0, 3.0 - 5.0 * k(10.0), 5.0);
        check(&p, PI * 900.0 * 5.0 + count as Real * cone, 5e-4);
    }
}

/// Recipe 29 (FreeCAD #12099): radial slots overlapping at the centre:
/// a Polar Pattern of one slot cut = one cut of the same six slots drawn
/// in a sketch (the union of the tools).
#[test]
fn r29_overlapping_polar_cuts_equal_one_cut() {
    let slot = |sk: &mut cadcraft3d::sketch::Sketch, a: Real| {
        let (c, s) = (a.cos(), a.sin());
        let p = |u: Real, v: Real| Point2::new(u * c - v * s, u * s + v * c);
        sk.add_rectangle_three_point(p(-2.0, -2.0), p(28.0, -2.0), p(28.0, 2.0))
            .unwrap();
    };
    let mut a = disc(30.0, 10.0);
    let i = a
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    slot(&mut a.sketches[i].sketch, 0.0);
    let reg = a.sketches[i].sketch.regions();
    let mut ex = ExtrudeFeature::new(region_profiles(&reg, |r| r.area() < 130.0), 10.0);
    ex.operation = Operation::Cut;
    a.add_extrude(i, ex).unwrap();
    let cut_id = a.features[1].id().to_owned();
    circ(&mut a, &[&cut_id], 6).unwrap();
    let mut b = disc(30.0, 10.0);
    let i = b
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    for k in 0..6 {
        slot(&mut b.sketches[i].sketch, k as Real * PI / 3.0);
    }
    let reg = b.sketches[i].sketch.regions();
    // Every region inside the slots (not the disc's remaining sectors).
    let mut ex = ExtrudeFeature::new(
        region_profiles(&reg, |r| {
            let q = r.interior_point();
            let (ang, rad) = (q.y.atan2(q.x), q.x.hypot(q.y));
            let near = (0..6).any(|k| {
                let d = ang - k as Real * PI / 3.0;
                (rad * d.sin()).abs() < 2.0 && rad * d.cos() > -2.0
            });
            near && rad < 28.0
        }),
        10.0,
    );
    ex.operation = Operation::Cut;
    b.add_extrude(i, ex).unwrap();
    let (va, vb) = (volume(&a), volume(&b));
    assert!((va - vb).abs() < 1e-3 * va, "{va} vs {vb}");
    check(&a, vb, 1e-3);
}

/// Recipe 30 (FreeCAD #30866, #29375): a Polar Pattern of 5 through holes
/// makes exactly 5 holes (volume and face count).
#[test]
fn r30_polar_five_holes_exactly() {
    let mut p = disc(30.0, 5.0);
    let t = top(&p, 5.0);
    let h = hole(&mut p, &t, 5.0, 20.0, 0.0, 3.0);
    circ(&mut p, &[&h], 5).unwrap();
    check(&p, PI * 900.0 * 5.0 - 5.0 * PI * 9.0 * 5.0, 3e-4);
    // Top, bottom, rim and five bores.
    assert_eq!(p.exact_bodies.0[0].measure_full().unwrap().faces, 8);
}

/// Recipe 32 (FreeCAD #5704): a 10 mm cube touching the YZ plane mirrored
/// across it: one body 20×10×10 with 6 faces after merging.
#[test]
fn r32_mirror_across_a_touching_plane() {
    let mut p = plate(10.0, 10.0, 10.0);
    let mut m = PatternFeature::mirror();
    m.solid_mode = true;
    m.kind = PatternKind::Mirror {
        plane: Some(PlaneRef::Origin(OriginPlane::YZ)),
    };
    p.add_pattern(m).unwrap();
    check(&p, 2000.0, 1e-9);
    assert_eq!(p.body.solids().count(), 1);
    assert_eq!(p.exact_bodies.0[0].measure_full().unwrap().faces, 6);
}

/// Recipe 34 (FreeCAD #28399): a Ø4 × 10 pin patterned 3×3, then the pins'
/// top edges chamfered 1: nine pins, each with its chamfer.
#[test]
fn r34_pins_patterned_then_chamfered() {
    let mut p = plate(40.0, 40.0, 5.0);
    let t = top(&p, 5.0);
    let j = sketch_on(&mut p, &t, |sk, at| {
        sk.add_circle(at(8.0, 8.0, 5.0), 2.0).unwrap();
    });
    boss(&mut p, j, 10.0, |r| r.holes.is_empty() && r.area() < 13.0);
    let pin = p.features[1].id().to_owned();
    rect(&mut p, &[&pin], (X, 3, 12.0), Some((Y, 3, 12.0))).unwrap();
    let pin_v = PI * 4.0 * 10.0;
    check(&p, 8000.0 + 9.0 * pin_v, 2e-4);
    let e = edges_where(&p, |h| (h[2] - 15.0).abs() < 1e-6);
    assert!(e.len() >= 9, "{} top edges", e.len());
    p.add_blend(BlendFeature::chamfer(e, 1.0)).unwrap();
    // A 1×1 triangle revolved at radius 2 − 1/3 on each pin.
    let ring = 2.0 * PI * (2.0 - 1.0 / 3.0) * 0.5;
    check(&p, 8000.0 + 9.0 * (pin_v - ring), 3e-4);
}

/// Recipe 35 (FreeCAD #28396): two mirrors in a row (across YZ, then the
/// boss and its mirror across XZ): four bosses, the original intact.
#[test]
fn r35_two_mirrors_make_four() {
    let mut p = PartDocument::default();
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    p.sketches[i]
        .sketch
        .add_rectangle_two_point(Point2::new(-30.0, -20.0), Point2::new(30.0, 20.0))
        .unwrap();
    let reg = p.sketches[i].sketch.regions();
    p.add_extrude(i, ExtrudeFeature::new(region_profiles(&reg, |_| true), 5.0))
        .unwrap();
    let t = top(&p, 5.0);
    let j = sketch_on(&mut p, &t, |sk, at| {
        sk.add_circle(at(15.0, 10.0, 5.0), 3.0).unwrap();
    });
    boss(&mut p, j, 6.0, |r| r.holes.is_empty() && r.area() < 30.0);
    let b = p.features[1].id().to_owned();
    let m1 = mirror(&mut p, &[&b], OriginPlane::YZ).unwrap();
    mirror(&mut p, &[&b, &m1], OriginPlane::XZ).unwrap();
    check(&p, 60.0 * 40.0 * 5.0 + 4.0 * PI * 9.0 * 6.0, 2e-4);
}

/// Recipe 36 (FreeCAD #30858): a plate with its front top edge chamfered
/// 2, a Ø4 hole crossing the chamfer, patterned 4 along the edge: each
/// copy removes what the first did.
#[test]
fn r36_holes_across_a_chamfer() {
    let mut p = plate(60.0, 20.0, 10.0);
    let e = edges_where(&p, |h| (h[2] - 10.0).abs() < 1e-6 && h[1].abs() < 1e-6);
    p.add_blend(BlendFeature::chamfer(e, 2.0)).unwrap();
    let v0 = volume(&p);
    let bottom = top(&p, 0.0);
    // From below, y 1…5: through the chamfer (y < 2 at the top).
    let h = hole(&mut p, &bottom, 0.0, 8.0, 3.0, 2.0);
    let v1 = volume(&p);
    rect(&mut p, &[&h], (X, 4, 14.0), None).unwrap();
    check(&p, v0 - 4.0 * (v0 - v1), 1e-4);
}

// ----------------------------------------------------------------- Hole ---

/// Recipe 40 (FreeCAD #5894): plate 20×20×10, Ø6.6 through with a
/// counterbore Ø11 × 5.5 → 4000 − π(3.3²·4.5 + 5.5²·5.5).
#[test]
fn r40_counterbore_volume() {
    let mut p = plate(20.0, 20.0, 10.0);
    let t = top(&p, 10.0);
    let mut s = HoleSpec::simple(6.6, 10.0);
    s.termination = Termination::ThroughAll;
    s.seat = SeatType::Counterbore;
    s.seat_diameter = 11.0;
    s.seat_depth = 5.5;
    hole_feature(&mut p, &t, 10.0, s, &[(10.0, 10.0)]).unwrap();
    check(&p, 4000.0 - PI * (3.3 * 3.3 * 4.5 + 5.5 * 5.5 * 5.5), 3e-4);
}

/// Recipe 41 (FreeCAD #8777, #31527): a countersink Ø10 on a Ø5.5 hole at
/// 90° is 2.25 deep, at 60° 3.897: the frustum removed matches.
#[test]
fn r41_countersink_depth_from_the_angle() {
    for angle in [90.0_f64, 60.0] {
        let mut p = plate(20.0, 20.0, 10.0);
        let t = top(&p, 10.0);
        let mut s = HoleSpec::simple(5.5, 10.0);
        s.termination = Termination::ThroughAll;
        s.seat = SeatType::Countersink;
        s.csink_diameter = 10.0;
        s.csink_angle = angle.to_radians();
        hole_feature(&mut p, &t, 10.0, s, &[(10.0, 10.0)]).unwrap();
        let depth = (5.0 - 2.75) / (angle.to_radians() / 2.0).tan();
        let frustum = PI * depth / 3.0 * (25.0 + 5.0 * 2.75 + 2.75 * 2.75);
        let bore = PI * 2.75 * 2.75 * (10.0 - depth);
        check(&p, 4000.0 - frustum - bore, 3e-4);
    }
}

/// Recipe 44 (FreeCAD #15353, #18105): four tapped M3 holes at (±10, ±10)
/// in one sketch, then a boss on the plate still builds.
#[test]
fn r44_four_tapped_holes_then_a_boss() {
    let m3 = cadcraft3d::part::ThreadSpec::from_table(
        &Standards::builtin(),
        cadcraft3d::part::standards::ISO_METRIC,
        "M3",
        true,
    )
    .expect("M3 in the table");
    let mut p = PartDocument::default();
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    p.sketches[i]
        .sketch
        .add_rectangle_two_point(Point2::new(-20.0, -20.0), Point2::new(20.0, 20.0))
        .unwrap();
    let reg = p.sketches[i].sketch.regions();
    p.add_extrude(i, ExtrudeFeature::new(region_profiles(&reg, |_| true), 8.0))
        .unwrap();
    let top_face = top(&p, 8.0);
    hole_feature(
        &mut p,
        &top_face,
        8.0,
        HoleSpec::tapped(m3, 6.0),
        &[(10.0, 10.0), (-10.0, 10.0), (-10.0, -10.0), (10.0, -10.0)],
    )
    .unwrap();
    assert_eq!(p.holes().len(), 4);
    let v = volume(&p);
    let t2 = top(&p, 8.0);
    let j = sketch_on(&mut p, &t2, |sk, at| {
        sk.add_circle(at(0.0, 0.0, 8.0), 4.0).unwrap();
    });
    boss(&mut p, j, 5.0, |r| r.holes.is_empty() && r.area() < 60.0);
    check(&p, v + PI * 16.0 * 5.0, 1e-4);
}

/// Recipe 46 (FreeCAD #17825, #23703): a Ø5 hole cut through a cylinder
/// R20 square to its axis (from the XZ plane, both ways): exactly two
/// openings, V = cylinder − (cylinder Ø5 ∩ cylinder R20).
#[test]
fn r46_hole_across_a_cylinder() {
    let mut p = disc(20.0, 40.0);
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XZ), &opts())
        .unwrap();
    p.sketches[i]
        .sketch
        .add_circle(Point2::new(0.0, 20.0), 2.5)
        .unwrap();
    let reg = p.sketches[i].sketch.regions();
    let mut ex = ExtrudeFeature::new(region_profiles(&reg, |_| true), 1.0);
    ex.operation = Operation::Cut;
    ex.extent = Extent::ThroughAll;
    ex.direction = ExtentDirection::Symmetric;
    p.add_extrude(i, ex).unwrap();
    // ∫ over the small disc (u along X, v along Z) of the chord 2√(R² − u²).
    let n = 2000;
    let h = 5.0 / n as Real;
    let mut inter = 0.0;
    for a in 0..n {
        let u = -2.5 + (a as Real + 0.5) * h;
        let half = (6.25 - u * u).sqrt();
        inter += 2.0 * half * 2.0 * (400.0 - u * u).sqrt() * h;
    }
    check(&p, PI * 400.0 * 40.0 - inter, 5e-4);
    assert_eq!(p.body.solids().count(), 1);
}

/// Recipe 48 (FreeCAD #17335): a countersunk hole as the last feature,
/// exported to STL: the STL's volume is the body's (±1 %).
#[test]
fn r48_stl_volume_of_a_countersunk_plate() {
    let mut p = plate(20.0, 20.0, 10.0);
    let t = top(&p, 10.0);
    let mut s = HoleSpec::simple(5.5, 10.0);
    s.termination = Termination::ThroughAll;
    s.seat = SeatType::Countersink;
    s.csink_diameter = 10.0;
    hole_feature(&mut p, &t, 10.0, s, &[(10.0, 10.0)]).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("plate.stl");
    cadcraft3d::part::export::export(&p, "plate", &path).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let n = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
    let mut v = 0.0;
    for t in 0..n {
        let o = 84 + t * 50 + 12;
        let f = |k: usize| {
            let b = &bytes[o + 4 * k..o + 4 * k + 4];
            f32::from_le_bytes([b[0], b[1], b[2], b[3]]) as Real
        };
        let (a, b, c) = (
            nalgebra::Vector3::new(f(0), f(1), f(2)),
            nalgebra::Vector3::new(f(3), f(4), f(5)),
            nalgebra::Vector3::new(f(6), f(7), f(8)),
        );
        v += a.dot(&b.cross(&c)) / 6.0;
    }
    let body = volume(&p);
    assert!((v - body).abs() < 0.01 * body, "STL {v} vs body {body}");
}

// ------------------------------------------------- pattern combinations ---

/// A hole pattern, then the rims of all copies rounded r0.5.
#[test]
fn hole_pattern_then_round_every_rim() {
    let mut p = plate(60.0, 40.0, 6.0);
    let t = top(&p, 6.0);
    let h = hole(&mut p, &t, 6.0, 10.0, 10.0, 2.0);
    rect(&mut p, &[&h], (X, 3, 20.0), Some((Y, 2, 20.0))).unwrap();
    let e = edges_where(&p, |q| {
        (q[2] - 6.0).abs() < 1e-6 && q[0] > 1.0 && q[0] < 59.0 && q[1] > 1.0 && q[1] < 39.0
    });
    assert!(!e.is_empty());
    p.add_blend(BlendFeature::fillet(e, 0.5)).unwrap();
    let c = 0.5 * (10.0 - 3.0 * PI) / (3.0 * (4.0 - PI));
    let rim = 2.0 * PI * (2.0 + c) * round_loss(0.5);
    check(&p, 60.0 * 40.0 * 6.0 - 6.0 * (PI * 4.0 * 6.0 + rim), 3e-4);
}

/// A boss mirrored, then the whole part shelled from the bottom: the
/// cavity follows both bosses.
#[test]
fn mirror_then_shell() {
    let mut p = PartDocument::default();
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    p.sketches[i]
        .sketch
        .add_rectangle_two_point(Point2::new(-30.0, -15.0), Point2::new(30.0, 15.0))
        .unwrap();
    let reg = p.sketches[i].sketch.regions();
    p.add_extrude(
        i,
        ExtrudeFeature::new(region_profiles(&reg, |_| true), 10.0),
    )
    .unwrap();
    let t = top(&p, 10.0);
    let j = sketch_on(&mut p, &t, |sk, at| {
        sk.add_rectangle_two_point(at(10.0, -5.0, 10.0), at(20.0, 5.0, 10.0))
            .unwrap();
    });
    boss(&mut p, j, 5.0, |r| {
        r.holes.is_empty() && (r.area() - 100.0).abs() < 1e-3
    });
    let b = p.features[1].id().to_owned();
    mirror(&mut p, &[&b], OriginPlane::YZ).unwrap();
    let outer = 60.0 * 30.0 * 10.0 + 2.0 * 500.0;
    check(&p, outer, 1e-9);
    let bottom = top(&p, 0.0);
    shell(
        &mut p,
        &[(&bottom, [0.0, 0.0, 0.0])],
        1.0,
        ShellDirection::Inside,
    );
    // Cavity: 58×28×9 plus each boss 8×8×5 (its walls 1 thick).
    check(
        &p,
        outer - (58.0 * 28.0 * 9.0 + 2.0 * 8.0 * 8.0 * 5.0),
        1e-6,
    );
}

/// A shelled box patterned as a solid (New Solid): three equal shells.
#[test]
fn shell_then_solid_pattern_new_solids() {
    let mut p = plate(20.0, 20.0, 10.0);
    let t = top(&p, 10.0);
    shell(
        &mut p,
        &[(&t, [10.0, 10.0, 10.0])],
        1.0,
        ShellDirection::Inside,
    );
    let one = volume(&p);
    let mut pat = PatternFeature::rectangular();
    pat.solid_mode = true;
    pat.operation = Operation::NewSolid;
    if let PatternKind::Rectangular { first, .. } = &mut pat.kind {
        (first.dir, first.count, first.spacing) = (Some(X), 3, 30.0);
    }
    p.add_pattern(pat).unwrap();
    check(&p, 3.0 * one, 1e-9);
    assert_eq!(p.body.solids().count(), 3);
}

/// Counterbored holes patterned circularly, then the plate's outline
/// rounded: holes and rounds do not meet.
#[test]
fn counterbore_polar_pattern_then_outer_rounds() {
    let mut p = disc(40.0, 8.0);
    let t = top(&p, 8.0);
    let mut s = HoleSpec::simple(6.6, 8.0);
    s.termination = Termination::ThroughAll;
    s.seat = SeatType::Counterbore;
    s.seat_diameter = 11.0;
    s.seat_depth = 4.0;
    let h = hole_feature(&mut p, &t, 8.0, s, &[(28.0, 0.0)]).unwrap();
    circ(&mut p, &[&h], 6).unwrap();
    assert_eq!(p.holes().len(), 6);
    let one = PI * (3.3 * 3.3 * 4.0 + 5.5 * 5.5 * 4.0);
    check(&p, PI * 1600.0 * 8.0 - 6.0 * one, 3e-4);
    // The top rim: between the disc's top and its side (by face names; a
    // circle's hint is a chord midpoint).
    let e: Vec<EdgeRef> = edges_where(&p, |q| (q[2] - 8.0).abs() < 1e-6)
        .into_iter()
        .filter(|r| r.faces.iter().any(|f| f.starts_with("Extrusion1:Side")))
        .collect();
    assert!(!e.is_empty());
    p.add_blend(BlendFeature::fillet(e, 1.0)).unwrap();
    let c = 1.0 * (10.0 - 3.0 * PI) / (3.0 * (4.0 - PI));
    let lost = 2.0 * PI * (40.0 - c) * round_loss(1.0);
    check(&p, PI * 1600.0 * 8.0 - 6.0 * one - lost, 3e-4);
    let g = p.hole_groups();
    assert_eq!(g.len(), 1, "{g:?}");
    assert!(g[0].label().starts_with("6x"), "{}", g[0].label());
}
