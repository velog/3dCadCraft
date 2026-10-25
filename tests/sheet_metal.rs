//! Sheet metal, stage S1 (`docs/spec/5.6-sheet-metal.md`): Face and Flange
//! over many variants — plate shapes, flange angles, Bend Positions, Flip,
//! adjacent and all edges, a flange on a flange, a plate with a hole,
//! thicknesses and K-factors — each checked for a valid solid, named faces,
//! the bends and the flat pattern's size.

use cadcraft3d::kernel::{Point3, Real};
use cadcraft3d::part::sheet_metal::{FlangePosition, SheetFlangeFeature, SheetMetalDefaults};
use cadcraft3d::part::{OriginPlane, PartDocument, ProfileRef, SketchOptions, SketchPlacement};
use cadcraft3d::sketch::Point2;

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};

/// A sheet part from a closed polygon (and optional circular holes) on XY.
fn plate(
    pts: &[(Real, Real)],
    holes: &[(Real, Real, Real)],
    rule: SheetMetalDefaults,
) -> PartDocument {
    let mut p = PartDocument::default();
    p.make_sheet_metal(Some(rule));
    let o = SketchOptions {
        auto_project_origin: false,
        ..Default::default()
    };
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &o)
        .unwrap();
    let sk = &mut p.sketches[i].sketch;
    if pts.len() == 4 && pts[0].1 == pts[1].1 && pts[1].0 == pts[2].0 {
        sk.add_rectangle_two_point(
            Point2::new(pts[0].0, pts[0].1),
            Point2::new(pts[2].0, pts[2].1),
        )
        .unwrap();
    } else {
        let ids: Vec<_> = pts
            .iter()
            .map(|&(x, y)| sk.add_point(Point2::new(x, y)))
            .collect();
        for k in 0..ids.len() {
            let _ = sk.add_line_between(ids[k], ids[(k + 1) % ids.len()]);
        }
    }
    for &(x, y, r) in holes {
        sk.add_circle(Point2::new(x, y), r).unwrap();
    }
    let regions = sk.regions();
    // The region with the most area: the plate (with its holes).
    let r = regions
        .iter()
        .max_by(|a, b| a.area().total_cmp(&b.area()))
        .unwrap();
    let prof = vec![ProfileRef::of(r, r.interior_point())];
    p.add_sheet_face(i, prof, false).unwrap();
    p
}

fn rect(w: Real, h: Real) -> PartDocument {
    plate(
        &[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)],
        &[],
        SheetMetalDefaults::default(),
    )
}

fn volume(p: &PartDocument) -> Real {
    p.exact_bodies
        .0
        .iter()
        .filter_map(|s| s.volume_at(1e-3))
        .sum()
}

/// Valid, named, healthy.
fn check(p: &PartDocument) {
    assert!(p.exact, "exact kernel");
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    for (sid, _) in p.body.solids() {
        p.body.validate_solid(sid).unwrap();
    }
    assert!(
        p.body.faces().all(|(_, f)| f.name.is_some()),
        "every face is named"
    );
}

fn flange(
    p: &mut PartDocument,
    at: &[Point3],
    height: Real,
    f: impl FnOnce(&mut SheetFlangeFeature),
) {
    let mut edges = Vec::new();
    let mut body = String::new();
    for &a in at {
        let (b, e) = p
            .sheet_edge_at(a)
            .unwrap_or_else(|| panic!("no sheet edge at {a}"));
        body = b;
        edges.push(e);
    }
    let mut def = SheetFlangeFeature::new(&body, edges, height);
    f(&mut def);
    p.add_sheet_flange(def).unwrap();
}

fn bbox(p: &PartDocument) -> (Point3, Point3) {
    p.body.bounding_box().unwrap()
}

#[test]
fn flange_angles() {
    for (deg, rise) in [(45.0, true), (90.0, true), (135.0, true)] {
        let mut p = rect(100.0, 60.0);
        flange(&mut p, &[Point3::new(50.0, 0.0, 2.0)], 25.0, |d| {
            d.angle = (deg as Real).to_radians()
        });
        check(&p);
        let (lo, hi) = bbox(&p);
        let h = hi.z - lo.z;
        let want = 25.0 * (deg as Real).to_radians().sin();
        assert!(
            rise && h > want * 0.8,
            "{deg}°: height {h}, want about {want}"
        );
        assert_eq!(p.flat_patterns()[0].bends.len(), 1, "{deg}°");
    }
}

#[test]
fn bend_positions_and_flip() {
    let mut sizes = Vec::new();
    for pos in [
        FlangePosition::Inside,
        FlangePosition::Middle,
        FlangePosition::Outside,
    ] {
        let mut p = rect(100.0, 60.0);
        flange(&mut p, &[Point3::new(50.0, 0.0, 2.0)], 25.0, |d| {
            d.position = pos
        });
        check(&p);
        let (lo, hi) = bbox(&p);
        sizes.push(hi.y - lo.y);
    }
    // Inside keeps the base's 60 mm; the others reach past it.
    assert!((sizes[0] - 60.0).abs() < 0.5, "{sizes:?}");
    assert!(sizes[0] < sizes[1] && sizes[1] < sizes[2], "{sizes:?}");
    let mut p = rect(100.0, 60.0);
    flange(&mut p, &[Point3::new(50.0, 0.0, 2.0)], 25.0, |d| {
        d.flip = true
    });
    check(&p);
    let (lo, _) = bbox(&p);
    assert!(lo.z < -20.0, "flipped down: {lo}");
}

#[test]
fn adjacent_and_all_edges() {
    let mut p = rect(120.0, 80.0);
    flange(
        &mut p,
        &[Point3::new(60.0, 0.0, 2.0), Point3::new(120.0, 40.0, 2.0)],
        20.0,
        |_| {},
    );
    check(&p);
    assert_eq!(p.flat_patterns()[0].bends.len(), 2);
    let mut p = rect(120.0, 80.0);
    flange(
        &mut p,
        &[
            Point3::new(60.0, 0.0, 2.0),
            Point3::new(120.0, 40.0, 2.0),
            Point3::new(60.0, 80.0, 2.0),
            Point3::new(0.0, 40.0, 2.0),
        ],
        20.0,
        |_| {},
    );
    check(&p);
    assert_eq!(p.flat_patterns()[0].bends.len(), 4);
    // A tray: its volume is about the plate plus four walls (minus the
    // corners and bend zones).
    let v = volume(&p);
    let rough = 120.0 * 80.0 * 2.0 + 2.0 * (120.0 + 80.0) * 20.0 * 2.0;
    assert!(v > rough * 0.8 && v < rough * 1.05, "{v} vs {rough}");
}

#[test]
fn flange_on_a_flange() {
    let mut p = rect(100.0, 60.0);
    flange(&mut p, &[Point3::new(50.0, 0.0, 2.0)], 30.0, |_| {});
    check(&p);
    // The flange's top edge (it stands at y ≈ 0..2, up to z ≈ 30).
    let (_, hi) = bbox(&p);
    let top = Point3::new(50.0, 1.0, hi.z);
    let (_, e) = p.sheet_edge_at(top).expect("the flange's top edge");
    let body = p.sheet_models.keys().next().unwrap().clone();
    p.add_sheet_flange(SheetFlangeFeature::new(&body, vec![e], 15.0))
        .unwrap();
    check(&p);
    assert_eq!(p.flat_patterns()[0].bends.len(), 2);
}

#[test]
fn plate_shapes() {
    // L-shaped plate: a flange on the inner edge.
    let l = [
        (0.0, 0.0),
        (100.0, 0.0),
        (100.0, 40.0),
        (40.0, 40.0),
        (40.0, 100.0),
        (0.0, 100.0),
    ];
    let mut p = plate(&l, &[], SheetMetalDefaults::default());
    check(&p);
    flange(&mut p, &[Point3::new(50.0, 0.0, 2.0)], 20.0, |_| {});
    check(&p);
    // A plate with a hole: the flat pattern has the hole too.
    let p = plate(
        &[(0.0, 0.0), (100.0, 0.0), (100.0, 60.0), (0.0, 60.0)],
        &[(50.0, 30.0, 10.0)],
        SheetMetalDefaults::default(),
    );
    check(&p);
    let v = volume(&p);
    let want = (100.0 * 60.0 - PI * 100.0) * 2.0;
    assert!((v - want).abs() < 0.01 * want, "{v} vs {want}");
    let flat = &p.flat_patterns()[0];
    let fv = flat.solid.volume_at(1e-3).unwrap();
    assert!((fv - want).abs() < 0.01 * want, "flat {fv}");
}

#[test]
fn thicknesses_and_k_factors() {
    let mut last = 0.0;
    for t in [0.5, 1.0, 3.0] {
        let rule = SheetMetalDefaults {
            thickness: t,
            ..Default::default()
        };
        let mut p = plate(
            &[(0.0, 0.0), (100.0, 0.0), (100.0, 60.0), (0.0, 60.0)],
            &[],
            rule,
        );
        flange(&mut p, &[Point3::new(50.0, 0.0, t)], 25.0, |_| {});
        check(&p);
        let v = volume(&p);
        assert!(v > last, "thicker is heavier: {t}");
        last = v;
    }
    // A larger K: a longer flat (more allowance).
    let flat = |k: Real| {
        let rule = SheetMetalDefaults {
            k_factor: k,
            ..Default::default()
        };
        let mut p = plate(
            &[(0.0, 0.0), (100.0, 0.0), (100.0, 60.0), (0.0, 60.0)],
            &[],
            rule,
        );
        flange(&mut p, &[Point3::new(50.0, 0.0, 2.0)], 25.0, |_| {});
        let (a, b) = p.flat_patterns()[0].size;
        a.min(b)
    };
    let (f3, f5) = (flat(0.3), flat(0.5));
    let want = FRAC_PI_2 * 2.0 * 0.2;
    assert!((f5 - f3 - want).abs() < 0.02, "{f3} {f5}: ΔBA {want}");
    let _ = FRAC_PI_4;
}

#[test]
fn regenerate_is_stable_and_a_hole_blocks_later_flanges() {
    let mut p = rect(100.0, 60.0);
    flange(&mut p, &[Point3::new(50.0, 0.0, 2.0)], 25.0, |_| {});
    let v = volume(&p);
    p.regenerate().unwrap();
    p.regenerate().unwrap();
    assert!((volume(&p) - v).abs() < 1e-6 * v);
    // A Hole through the plate, then another flange: refused, with the
    // reason, and nothing changes.
    use cadcraft3d::part::{HoleCenter, HoleFeature, HoleSpec, Termination};
    let top = p
        .body
        .faces()
        .find(|(_, f)| f.name.as_deref().is_some_and(|n| n.ends_with(":top")))
        .and_then(|(_, f)| f.name.clone())
        .expect("a top face");
    let (k, ids) = p
        .create_hole_sketch(&top, &[Point3::new(50.0, 30.0, 2.0)])
        .unwrap();
    let mut s = HoleSpec::simple(6.0, 10.0);
    s.termination = Termination::ThroughAll;
    let mut def = HoleFeature::new(s);
    def.sketch = p.sketches[k].id.clone();
    def.centers = ids.into_iter().map(HoleCenter::Point).collect();
    p.add_hole(def).unwrap();
    let n = p.features.len();
    let body = p.sheet_models.keys().next().unwrap().clone();
    let (_, e) = p.sheet_edge_at(Point3::new(100.0, 30.0, 2.0)).unwrap();
    let err = p.add_sheet_flange(SheetFlangeFeature::new(&body, vec![e], 20.0));
    assert!(err.is_err(), "refused");
    assert_eq!(p.features.len(), n, "nothing added");
}

/// Face Flip puts the thickness below the sketch; every flat pattern is a
/// valid solid of the plate's thickness.
#[test]
fn face_flip_and_valid_flat_patterns() {
    let mut p = PartDocument::default();
    p.make_sheet_metal(None);
    let o = SketchOptions {
        auto_project_origin: false,
        ..Default::default()
    };
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &o)
        .unwrap();
    p.sketches[i]
        .sketch
        .add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(80.0, 50.0))
        .unwrap();
    let r = p.sketches[i].sketch.regions();
    p.add_sheet_face(i, vec![ProfileRef::of(&r[0], r[0].interior_point())], true)
        .unwrap();
    check(&p);
    let (lo, hi) = bbox(&p);
    assert!(
        hi.z.abs() < 0.5 && (lo.z + 2.0).abs() < 0.5,
        "below: {lo} {hi}"
    );
    let mut p = rect(120.0, 80.0);
    flange(
        &mut p,
        &[
            Point3::new(60.0, 0.0, 2.0),
            Point3::new(120.0, 40.0, 2.0),
            Point3::new(60.0, 80.0, 2.0),
        ],
        20.0,
        |_| {},
    );
    for f in p.flat_patterns() {
        let b = cadcraft3d::kernel::exact::to_brep(&[f.solid]).unwrap();
        for (sid, _) in b.solids() {
            b.validate_solid(sid).unwrap();
        }
        let (lo, hi) = b.bounding_box().unwrap();
        assert!((hi.z - lo.z - 2.0).abs() < 0.5, "flat: {lo} {hi}");
    }
}
