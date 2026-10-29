//! Hem types (`docs/spec/5.6-sheet-metal.md`, Hem; format revision 43):
//! Single, Double, Teardrop and Rolled on the 100 mm edge of a 100 × 50
//! plate, against the results Inventor gives: the flat length (the volume
//! is flat length × width × thickness), the height of the folded part and
//! that the hem's outer face stays where the edge was (x = 100).

use cadcraft3d::kernel::{Point3, Real};
use cadcraft3d::part::sheet_metal::{HemType, SheetMetalDefaults, SheetOp, SheetOpFeature};
use cadcraft3d::part::{OriginPlane, PartDocument, ProfileRef, SketchOptions, SketchPlacement};
use cadcraft3d::sketch::Point2;

/// A 100 × 50 sheet of thickness `t` (K 0.5) on XY.
fn plate(t: Real) -> PartDocument {
    let mut p = PartDocument::default();
    p.make_sheet_metal(Some(SheetMetalDefaults {
        thickness: t,
        k_factor: 0.5,
        ..Default::default()
    }));
    let o = SketchOptions {
        auto_project_origin: false,
        ..Default::default()
    };
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &o)
        .unwrap();
    let sk = &mut p.sketches[i].sketch;
    sk.add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(100.0, 50.0))
        .unwrap();
    let regions = sk.regions();
    let r = &regions[0];
    let prof = vec![ProfileRef::of(r, r.interior_point())];
    p.add_sheet_face(i, prof, false).unwrap();
    p
}

/// The hem on the plate's right edge, on its top face; (gap or radius,
/// length or angle in degrees).
fn hem(t: Real, ty: HemType, a: Real, b: Real) -> Result<PartDocument, String> {
    let mut p = plate(t);
    let (body, e) = p
        .sheet_edge_at(Point3::new(100.0, 25.0, t))
        .ok_or("no edge")?;
    let (gap, length, radius, angle) = if ty.by_radius() {
        (None, 0.0, Some(a), Some(b.to_radians()))
    } else {
        (Some(a), b, None, None)
    };
    p.add_sheet_op(SheetOpFeature::new(SheetOp::Hem {
        body,
        edges: vec![e],
        length,
        gap,
        flip: false,
        hem_type: Some(ty),
        radius,
        angle,
    }))
    .map_err(|e| e.to_string())?;
    Ok(p)
}

fn bbox(p: &PartDocument) -> (Point3, Point3) {
    let b = cadcraft3d::kernel::exact::to_brep(&p.exact_bodies.0).unwrap();
    b.bounding_box().unwrap()
}

fn volume(p: &PartDocument) -> Real {
    p.exact_bodies
        .0
        .iter()
        .filter_map(|s| s.volume_at(1e-3))
        .sum()
}

/// (thickness, type, gap or radius, length or angle °) → Inventor's flat
/// length and height of the part.
const CASES: [(Real, HemType, Real, Real, Real, Real); 14] = [
    (2.0, HemType::Single, 1.0, 8.0, 107.712_388_980, 5.0),
    (2.0, HemType::Single, 1.0, 10.0, 109.712_388_980, 5.0),
    (3.0, HemType::Single, 1.0, 10.0, 109.283_185_307, 7.0),
    (2.0, HemType::Double, 1.0, 10.0, 119.924_777_961, 8.0),
    (2.0, HemType::Double, 0.5, 12.0, 123.103_981_634, 7.0),
    (2.0, HemType::Double, 1.0, 20.0, 139.924_777_961, 8.0),
    (3.0, HemType::Double, 1.0, 15.0, 129.066_370_614, 11.0),
    (2.0, HemType::Teardrop, 1.0, 270.0, 106.924_777_961, 6.0),
    (2.0, HemType::Teardrop, 3.0, 200.0, 124.514_577_275, 10.0),
    (3.0, HemType::Teardrop, 1.0, 270.0, 108.030_972_451, 8.0),
    (1.0, HemType::Teardrop, 2.0, 240.0, 110.647_401_993, 6.0),
    (2.0, HemType::Rolled, 2.0, 180.0, 105.424_777_961, 8.0),
    (2.0, HemType::Rolled, 3.0, 120.0, 103.377_580_410, 7.5),
    (2.0, HemType::Rolled, 2.0, 270.0, 110.137_166_941, 8.0),
];

#[test]
fn hem_types_match_inventor() {
    let mut bad = Vec::new();
    for (t, ty, a, b, flat, height) in CASES {
        let what = format!("{} T{t} {a} {b}", ty.label());
        let p = match hem(t, ty, a, b) {
            Ok(p) => p,
            Err(e) => {
                bad.push(format!("{what}: {e}"));
                continue;
            }
        };
        if !p.feature_health.is_empty() || !p.exact {
            bad.push(format!("{what}: {:?}", p.feature_health));
            continue;
        }
        for (sid, _) in p.body.solids() {
            if let Err(e) = p.body.validate_solid(sid) {
                bad.push(format!("{what}: invalid solid {e:?}"));
            }
        }
        let fp = &p.flat_patterns()[0];
        let len = fp.size.0.max(fp.size.1);
        let (lo, hi) = bbox(&p);
        let v = volume(&p);
        let want_v = flat * 50.0 * t;
        if (len - flat).abs() > 1e-3
            || (hi.z - lo.z - height).abs() > 0.02
            || (hi.x - 100.0).abs() > 0.02
            || lo.x.abs() > 1e-6
            || (v - want_v).abs() > want_v * 1e-3
        {
            bad.push(format!(
                "{what}: flat {len:.4} (Inventor {flat:.4}), height {:.3} (Inventor {height}), \
                 x {:.3}..{:.3} (Inventor 0..100), volume {v:.1} (Inventor {want_v:.1})",
                hi.z - lo.z,
                lo.x,
                hi.x
            ));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// Bends and their tags: a Double hem is three bends (90°, 90°, 180°) on
/// each other's far edges, named like a Flange's further bends.
#[test]
fn double_hem_is_three_bends() {
    let p = hem(2.0, HemType::Double, 1.0, 10.0).unwrap();
    let fp = &p.flat_patterns()[0];
    let mut angles: Vec<Real> = fp.bends.iter().map(|b| b.angle_deg).collect();
    angles.sort_by(Real::total_cmp);
    assert_eq!(angles.len(), 3, "{angles:?}");
    assert!((angles[0] - 90.0).abs() < 1e-9 && (angles[1] - 90.0).abs() < 1e-9);
    assert!((angles[2] - 180.0).abs() < 1e-9);
}

/// What Inventor refuses: no gap, a hem shorter than its bends, a teardrop
/// of 180° or less, one whose bend runs into the sheet; the part keeps its
/// last good state (the Hem is not added).
#[test]
fn hem_types_refuse() {
    for (ty, a, b) in [
        (HemType::Single, 0.0, 8.0),
        (HemType::Single, 1.0, 2.0),
        (HemType::Double, 0.0, 10.0),
        (HemType::Double, 1.0, 5.0),
        (HemType::Teardrop, 1.0, 180.0),
        (HemType::Teardrop, 1.0, 120.0),
        (HemType::Teardrop, 0.5, 355.0),
        (HemType::Rolled, 2.0, 360.0),
    ] {
        assert!(hem(2.0, ty, a, b).is_err(), "{} {a} {b}", ty.label());
    }
}

/// Saved and loaded: the type, radius and angle come back.
#[test]
fn hem_type_round_trips() {
    let p = hem(2.0, HemType::Teardrop, 1.5, 250.0).unwrap();
    let json = serde_json::to_string(&p.features).unwrap();
    assert!(json.contains("\"hem_type\":\"Teardrop\""), "{json}");
    let back: Vec<cadcraft3d::part::Feature> = serde_json::from_str(&json).unwrap();
    assert_eq!(back, p.features);
}
