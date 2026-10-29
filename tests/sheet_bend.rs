//! Bend (`docs/spec/5.6-sheet-metal.md` §6.1d): two sheets of one face each
//! joined between an edge of each — a wall at an angle (one bend), parallel
//! faces as a Z (Fix Edges, 45 Degree) or a U (90 Degree, Full Radius) —
//! against the results Inventor gives for the same cases: the flat length
//! (the volume is flat length × width × thickness) and the folded part's box.

use cadcraft3d::kernel::{Point3, Real};
use cadcraft3d::part::sheet_metal::{DoubleBendType, SheetMetalDefaults, SheetOp, SheetOpFeature};
use cadcraft3d::part::{
    OriginPlane, PartDocument, PlaneRef, ProfileRef, SketchOptions, SketchPlacement, WorkDef,
    WorkFeature, WorkPlaneDef,
};
use cadcraft3d::sketch::Point2;

const T: Real = 2.0;

/// A rectangle (plane coordinates) made a Face on `plane` offset `d`.
fn face(p: &mut PartDocument, plane: OriginPlane, d: Real, a: (Real, Real), b: (Real, Real)) {
    let o = SketchOptions {
        auto_project_origin: false,
        ..Default::default()
    };
    let placement = if d == 0.0 {
        SketchPlacement::on_origin_plane(plane)
    } else {
        let wp = p
            .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
                base: PlaneRef::Origin(plane),
                distance: d,
            })))
            .unwrap();
        SketchPlacement {
            plane: PlaneRef::Work(wp),
            ..Default::default()
        }
    };
    let i = p.create_sketch(placement, &o).unwrap();
    let sk = &mut p.sketches[i].sketch;
    sk.add_rectangle_two_point(Point2::new(a.0, a.1), Point2::new(b.0, b.1))
        .unwrap();
    let regions = sk.regions();
    let r = &regions[0];
    let prof = vec![ProfileRef::of(r, r.interior_point())];
    p.add_sheet_face(i, prof, false).unwrap();
}

/// The second face: a wall in the plane x = `x` from z0 to z1 (`Wall`) or
/// a face parallel to the first at height h from x0 to x1 (`Level`).
#[derive(Clone, Copy, Debug)]
enum Second {
    Wall(Real, Real, Real),
    Level(Real, Real, Real),
}

/// The 100 × 50 face on XY (T 2, K 0.5, R = T), the second face, and a Bend
/// between the first's edge at x = 100 and the second's edge nearest it.
fn bend(second: Second, double: DoubleBendType, flip_fixed: bool) -> Result<PartDocument, String> {
    let mut p = PartDocument::default();
    p.make_sheet_metal(Some(SheetMetalDefaults {
        thickness: T,
        k_factor: 0.5,
        ..Default::default()
    }));
    face(&mut p, OriginPlane::XY, 0.0, (0.0, 0.0), (100.0, 50.0));
    let e2 = match second {
        Second::Wall(x, z0, z1) => {
            face(&mut p, OriginPlane::YZ, x, (0.0, z0), (50.0, z1));
            Point3::new(x, 25.0, if z0 > 0.0 { z0 } else { z1 })
        }
        Second::Level(h, x0, x1) => {
            face(&mut p, OriginPlane::XY, h, (x0, 0.0), (x1, 50.0));
            // A face running on away (x1 past the first) is joined at x0, one running back at x1.
            Point3::new(if x1 > 100.0 { x0 } else { x1 }, 25.0, h)
        }
    };
    let (body, edge1) = p
        .sheet_edge_at(Point3::new(100.0, 25.0, T))
        .ok_or("no first edge")?;
    let (other, edge2) = p.sheet_edge_at(e2).ok_or("no second edge")?;
    if body == other {
        return Err(format!("both picks found {body}"));
    }
    p.add_sheet_op(SheetOpFeature::new(SheetOp::Bend {
        body,
        edge1,
        other,
        edge2,
        double,
        flip_fixed,
        radius: None,
        aligned: false,
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

use DoubleBendType::*;
use Second::*;

/// (second face, type, Flip Fixed Edge) → Inventor's flat length, x range
/// and z range of the part.
#[allow(clippy::type_complexity)]
const CASES: [(
    Second,
    DoubleBendType,
    bool,
    Real,
    (Real, Real),
    (Real, Real),
); 18] = [
    (
        Wall(120.0, 10.0, 60.0),
        FixEdges,
        false,
        178.712_388_980_385,
        (0.0, 122.0),
        (0.0, 60.0),
    ),
    (
        Wall(120.0, -60.0, -10.0),
        FixEdges,
        false,
        180.712_388_980_385,
        (0.0, 122.0),
        (-60.0, 2.0),
    ),
    (
        Level(20.0, 120.0, 200.0),
        FixEdges,
        false,
        208.818_556_158_708,
        (0.0, 200.0),
        (0.0, 22.0),
    ),
    (
        Level(20.0, 120.0, 200.0),
        Deg45,
        false,
        208.026_097_479_369,
        (0.0, 200.0),
        (0.0, 22.0),
    ),
    (
        Level(20.0, 120.0, 200.0),
        Deg45,
        true,
        208.026_097_479_370,
        (0.0, 200.0),
        (0.0, 22.0),
    ),
    (
        Level(6.0, 104.0, 200.0),
        FixEdges,
        false,
        201.861_006_652_760,
        (0.0, 200.0),
        (0.0, 8.0),
    ),
    (
        Level(6.0, 104.0, 200.0),
        FixEdges,
        true,
        201.861_006_652_760,
        (0.0, 200.0),
        (0.0, 8.0),
    ),
    (
        Level(10.0, 104.0, 200.0),
        FixEdges,
        false,
        205.540_368_827_739,
        (0.0, 200.0),
        (0.0, 12.0),
    ),
    (
        Level(8.0, 105.0, 200.0),
        FixEdges,
        false,
        203.006_101_980_553,
        (0.0, 200.0),
        (0.0, 10.0),
    ),
    (
        Level(10.0, 102.0, 200.0),
        FixEdges,
        false,
        206.978_970_450_245,
        (0.0, 200.0),
        (0.0, 12.0),
    ),
    (
        Level(10.0, 103.0, 200.0),
        FixEdges,
        false,
        206.182_313_697_001,
        (0.0, 200.0),
        (0.0, 12.0),
    ),
    (
        Level(3.0, 108.0, 200.0),
        FixEdges,
        false,
        200.601_430_313_249,
        (0.0, 200.0),
        (0.0, 5.0),
    ),
    (
        Level(6.0, 110.0, 200.0),
        FixEdges,
        false,
        201.861_006_652_760,
        (0.0, 200.0),
        (0.0, 8.0),
    ),
    (
        Level(20.0, 90.0, 200.0),
        FixEdges,
        false,
        241.408_167_230_175,
        (0.0, 200.0),
        (0.0, 22.0),
    ),
    (
        Level(20.0, 0.0, 80.0),
        Deg90,
        false,
        223.424_777_960_769,
        (0.0, 104.0),
        (0.0, 22.0),
    ),
    (
        Level(20.0, 0.0, 80.0),
        Deg90,
        true,
        183.424_777_960_769,
        (0.0, 84.0),
        (0.0, 22.0),
    ),
    (
        Level(20.0, 0.0, 80.0),
        FullRadius,
        false,
        231.415_926_535_898,
        (0.0, 111.0),
        (0.0, 22.0),
    ),
    (
        Level(20.0, 0.0, 80.0),
        FullRadius,
        true,
        191.415_926_535_898,
        (0.0, 91.0),
        (0.0, 22.0),
    ),
];

#[test]
fn bends_match_inventor() {
    let mut bad = Vec::new();
    for (second, double, flip, flat, xs, zs) in CASES {
        let what = format!("{second:?} {} flip {flip}", double.label());
        let p = match bend(second, double, flip) {
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
        let fps = p.flat_patterns();
        if fps.len() != 1 || p.exact_bodies.0.len() != 1 {
            bad.push(format!(
                "{what}: {} flat patterns, {} bodies (one sheet expected)",
                fps.len(),
                p.exact_bodies.0.len()
            ));
            continue;
        }
        let len = fps[0].size.0.max(fps[0].size.1);
        let (lo, hi) = bbox(&p);
        let v = volume(&p);
        let want_v = flat * 50.0 * T;
        if (len - flat).abs() > 1e-3
            || (lo.x - xs.0).abs() > 0.02
            || (hi.x - xs.1).abs() > 0.02
            || (lo.z - zs.0).abs() > 0.02
            || (hi.z - zs.1).abs() > 0.02
            || (v - want_v).abs() > want_v * 1e-3
        {
            bad.push(format!(
                "{what}: flat {len:.4} (Inventor {flat:.4}), x {:.3}..{:.3} (Inventor {}..{}), \
                 z {:.3}..{:.3} (Inventor {}..{}), volume {v:.1} (Inventor {want_v:.1})",
                lo.x, hi.x, xs.0, xs.1, lo.z, hi.z, zs.0, zs.1
            ));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// What Inventor refuses: a U with Fix Edges / 45 Degree, a Z with 90
/// Degree / Full Radius, Z faces with no room for two bends; the part keeps
/// its two sheets (the Bend is not added).
#[test]
fn bends_refuse() {
    for (second, double) in [
        (Level(20.0, 0.0, 80.0), FixEdges),
        (Level(20.0, 0.0, 80.0), Deg45),
        (Level(20.0, 120.0, 200.0), Deg90),
        (Level(20.0, 120.0, 200.0), FullRadius),
        (Level(6.0, 100.0, 200.0), FixEdges),
    ] {
        assert!(
            bend(second, double, false).is_err(),
            "{second:?} {}",
            double.label()
        );
    }
}

/// Edges of different lengths (Inventor's behaviour): the 100 × 50 face on
/// XY and a wall at x = 110 (z 10..60) whose edge runs from y0 to y1. The
/// bend covers the part both edges share; *Perpendicular to Side Face*
/// extends each face to it only there, *Aligned to Side Face* over its whole
/// edge. Inventor's volumes; the flat is 168.712 long in every case.
#[test]
fn edges_of_different_lengths() {
    // (y0, y1, aligned, volume, flat width)
    let cases = [
        (10.0, 40.0, true, 14_442.743_338_823_1, 50.0),
        (10.0, 40.0, false, 14_122.743_338_823_1, 50.0),
        (-10.0, 60.0, true, 19_111.238_898_038_4, 70.0),
        (-10.0, 60.0, false, 18_871.238_898_038_4, 70.0),
        (20.0, 70.0, true, 16_682.743_338_823_1, 70.0),
        (20.0, 70.0, false, 16_122.743_338_823_1, 70.0),
    ];
    let mut bad = Vec::new();
    for (y0, y1, aligned, want, width) in cases {
        let what = format!("y {y0}..{y1}, aligned {aligned}");
        let mut p = PartDocument::default();
        p.make_sheet_metal(Some(SheetMetalDefaults {
            thickness: T,
            k_factor: 0.5,
            ..Default::default()
        }));
        face(&mut p, OriginPlane::XY, 0.0, (0.0, 0.0), (100.0, 50.0));
        face(&mut p, OriginPlane::YZ, 110.0, (y0, 10.0), (y1, 60.0));
        let (body, edge1) = p.sheet_edge_at(Point3::new(100.0, 25.0, T)).unwrap();
        let (other, edge2) = p
            .sheet_edge_at(Point3::new(110.0, (y0 + y1) / 2.0, 10.0))
            .unwrap();
        let r = p.add_sheet_op(SheetOpFeature::new(SheetOp::Bend {
            body,
            edge1,
            other,
            edge2,
            double: FixEdges,
            flip_fixed: false,
            radius: None,
            aligned,
        }));
        if let Err(e) = r {
            bad.push(format!("{what}: {e}"));
            continue;
        }
        let v = volume(&p);
        let fp = &p.flat_patterns()[0];
        let (len, wid) = (fp.size.0.max(fp.size.1), fp.size.0.min(fp.size.1));
        if (v - want).abs() > 1e-3 * want
            || (len - 168.712_388_980_385).abs() > 1e-3
            || (wid - width).abs() > 1e-3
        {
            bad.push(format!(
                "{what}: volume {v:.3} (Inventor {want:.3}), flat {len:.4} x {wid:.4} (168.7124 x {width})"
            ));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// The second face's sheet may have flanges when the first is a plain face:
/// the sheets swap (the plain face joins the other).
#[test]
fn a_plain_face_joins_a_sheet_with_flanges() {
    use cadcraft3d::part::sheet_metal::SheetFlangeFeature;
    let mut p = PartDocument::default();
    p.make_sheet_metal(Some(SheetMetalDefaults {
        thickness: T,
        k_factor: 0.5,
        ..Default::default()
    }));
    face(&mut p, OriginPlane::XY, 0.0, (0.0, 0.0), (100.0, 50.0));
    let (b, e) = p.sheet_edge_at(Point3::new(0.0, 25.0, T)).unwrap();
    p.add_sheet_flange(SheetFlangeFeature::new(&b, vec![e], 20.0))
        .unwrap();
    face(&mut p, OriginPlane::YZ, 120.0, (0.0, 10.0), (50.0, 60.0));
    let (body, edge1) = p.sheet_edge_at(Point3::new(120.0, 25.0, 10.0)).unwrap();
    let (other, edge2) = p.sheet_edge_at(Point3::new(100.0, 25.0, T)).unwrap();
    p.add_sheet_op(SheetOpFeature::new(SheetOp::Bend {
        body,
        edge1,
        other,
        edge2,
        double: FixEdges,
        flip_fixed: false,
        radius: None,
        aligned: false,
    }))
    .unwrap();
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    assert_eq!(p.exact_bodies.0.len(), 1);
    assert_eq!(p.flat_patterns()[0].bends.len(), 2);
}
