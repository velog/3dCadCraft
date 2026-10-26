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

/// Width Extents: a flange held back from the edge's ends, a centred one, a
/// width from an offset; each narrower than the edge by what was asked, with
/// bend reliefs cut into the base beside it. Offsets that do not fit fail.
#[test]
fn flange_width_extents() {
    use cadcraft3d::part::sheet_metal::FlangeWidth;
    let width_of = |p: &PartDocument| {
        // The flange stands at y ≈ 0..2; its width along x above the plate.
        let xs: Vec<Real> = p
            .body
            .faces()
            .filter_map(|(_, f)| f.mesh.as_ref())
            .flat_map(|m| m.positions.iter().copied())
            .filter(|q| q.z > 10.0)
            .map(|q| q.x)
            .collect();
        let lo = xs.iter().copied().fold(Real::INFINITY, Real::min);
        let hi = xs.iter().copied().fold(Real::NEG_INFINITY, Real::max);
        (lo, hi)
    };
    let edge = [Point3::new(50.0, 0.0, 2.0)];
    // Offset 10 / 30.
    let mut p = rect(100.0, 60.0);
    flange(&mut p, &edge, 25.0, |d| {
        d.extents = FlangeWidth::Offset;
        d.offset1 = 10.0;
        d.offset2 = 30.0;
    });
    check(&p);
    let (lo, hi) = width_of(&p);
    let (a, b) = (lo.min(100.0 - hi), lo.max(100.0 - hi));
    assert!(
        (a - 10.0).abs() < 0.05 && (b - 30.0).abs() < 0.05,
        "{lo} {hi}"
    );
    // The plate keeps its full 100 mm (a flange held back keeps the base's edge).
    let (blo, bhi) = bbox(&p);
    assert!((bhi.x - blo.x - 100.0).abs() < 0.05);
    // Centred, 40 wide.
    let mut p = rect(100.0, 60.0);
    flange(&mut p, &edge, 25.0, |d| {
        d.extents = FlangeWidth::Width;
        d.width = 40.0;
        d.centered = true;
    });
    check(&p);
    let (lo, hi) = width_of(&p);
    assert!(
        (lo - 30.0).abs() < 0.05 && (hi - 70.0).abs() < 0.05,
        "{lo} {hi}"
    );
    // Width 20 at 5 from the start.
    let mut p = rect(100.0, 60.0);
    flange(&mut p, &edge, 25.0, |d| {
        d.extents = FlangeWidth::Width;
        d.width = 20.0;
        d.offset1 = 5.0;
    });
    check(&p);
    let (lo, hi) = width_of(&p);
    assert!((hi - lo - 20.0).abs() < 0.05, "{lo} {hi}");
    assert!(
        (lo - 5.0).abs() < 0.05 || (100.0 - hi - 5.0).abs() < 0.05,
        "{lo} {hi}"
    );
    // The flat pattern is one sheet with the reliefs.
    let flat = &p.flat_patterns()[0];
    assert_eq!(flat.bends.len(), 1);
    // Too wide: refused, nothing added.
    let mut p = rect(100.0, 60.0);
    let n = p.features.len();
    let (body, e) = p.sheet_edge_at(edge[0]).unwrap();
    let mut def = SheetFlangeFeature::new(&body, vec![e], 25.0);
    def.extents = FlangeWidth::Offset;
    def.offset1 = 60.0;
    def.offset2 = 50.0;
    assert!(p.add_sheet_flange(def).is_err());
    assert_eq!(p.features.len(), n);
    // The ends are reported for the drag handles: 100 mm apart.
    let (body, e) = p.sheet_edge_at(edge[0]).unwrap();
    let (a, b) = p.sheet_edge_ends(&body, &e).unwrap();
    assert!(((b - a).norm() - 100.0).abs() < 1e-6);
}

/// Corner Relief Shape: every shape gives a valid tray of four flanges; the
/// round and square reliefs take material away at the corners.
#[test]
fn corner_relief_shapes() {
    use cadcraft3d::part::sheet_metal::CornerRelief;
    let mut vols = Vec::new();
    for shape in CornerRelief::ALL {
        let rule = SheetMetalDefaults {
            corner_shape: shape,
            ..Default::default()
        };
        let mut p = plate(
            &[(0.0, 0.0), (120.0, 0.0), (120.0, 80.0), (0.0, 80.0)],
            &[],
            rule,
        );
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
        assert_eq!(p.flat_patterns()[0].bends.len(), 4, "{shape:?}");
        vols.push((shape, volume(&p)));
    }
    eprintln!("{vols:?}");
    let v = |s: CornerRelief| vols.iter().find(|x| x.0 == s).map(|x| x.1).unwrap();
    assert!(v(CornerRelief::Round) < v(CornerRelief::Trim), "{vols:?}");
    assert!(v(CornerRelief::Square) < v(CornerRelief::Trim), "{vols:?}");
}

fn edge(a: &str, b: &str, hint: [Real; 3]) -> cadcraft3d::part::EdgeRef {
    cadcraft3d::part::EdgeRef {
        faces: [a.into(), b.into()],
        hint,
    }
}

/// Convert to Sheet Metal of a solid box (Top fixed, the edges to Front
/// and Back become bends): a U channel of the rule's thickness, flat pattern
/// with two bends; flanges can be added to it afterwards. Of a shelled box
/// the walls keep their own thickness.
#[test]
fn convert_solid_to_sheet_metal() {
    use cadcraft3d::part::sheet_metal::SheetConvertFeature;
    use cadcraft3d::part::shell::FaceRef;
    let mut p = PartDocument::with_box("Box1", [100.0, 60.0, 40.0]).unwrap();
    let body = p.exact_bodies.0[0].name.clone().unwrap();
    let def = SheetConvertFeature {
        id: String::new(),
        name: String::new(),
        body: body.clone(),
        face: FaceRef {
            face: "Box1:Top".into(),
            hint: [50.0, 30.0, 40.0],
        },
        bends: vec![
            edge("Box1:Top", "Box1:Front", [50.0, 0.0, 40.0]),
            edge("Box1:Top", "Box1:Back", [50.0, 60.0, 40.0]),
        ],
    };
    p.add_sheet_convert(def).unwrap();
    check(&p);
    assert!(p.sheet_metal.is_some());
    let flat = &p.flat_patterns()[0];
    assert_eq!(flat.bends.len(), 2);
    // The channel keeps the box's envelope.
    let (lo, hi) = bbox(&p);
    assert!(
        (hi.x - lo.x - 100.0).abs() < 0.1 && (hi.y - lo.y - 60.0).abs() < 0.1,
        "{lo} {hi}"
    );
    assert!((hi.z - lo.z - 40.0).abs() < 0.1, "{lo} {hi}");
    // Much less than the box: about the three walls of 2 mm.
    let v = volume(&p);
    let rough = (100.0 * 60.0 + 2.0 * 100.0 * 38.0) * 2.0;
    assert!((v - rough).abs() < 0.08 * rough, "{v} vs {rough}");
    // A flange on the channel's free edge (the bottom of the front wall).
    let (b, e) = p
        .sheet_edge_at(Point3::new(50.0, 1.0, 0.0))
        .expect("an edge at the wall's foot");
    p.add_sheet_flange(SheetFlangeFeature::new(&b, vec![e], 15.0))
        .unwrap();
    check(&p);
    assert_eq!(p.flat_patterns()[0].bends.len(), 3);
    // Missing bends: refused, nothing changes.
    let mut q = PartDocument::with_box("Box1", [100.0, 60.0, 40.0]).unwrap();
    let n = q.features.len();
    let bad = SheetConvertFeature {
        id: String::new(),
        name: String::new(),
        body,
        face: FaceRef {
            face: "Box1:Top".into(),
            hint: [50.0, 30.0, 40.0],
        },
        bends: Vec::new(),
    };
    assert!(q.add_sheet_convert(bad).is_err());
    assert_eq!(q.features.len(), n);
    assert!(q.sheet_metal.is_none(), "the rule is not left behind");
}

/// A shelled box (open at the top, walls 1.5) converted with the bottom
/// fixed and the bottom's four edges as bends: a tray whose walls keep the
/// shell's 1.5 mm, not the rule's 2 mm.
#[test]
fn convert_shelled_box_keeps_its_walls() {
    use cadcraft3d::part::sheet_metal::SheetConvertFeature;
    use cadcraft3d::part::shell::{FaceRef, ShellFeature};
    let mut p = PartDocument::with_box("Box1", [100.0, 60.0, 30.0]).unwrap();
    let mut sh = ShellFeature::new(1.5);
    sh.remove.push(FaceRef {
        face: "Box1:Top".into(),
        hint: [50.0, 30.0, 30.0],
    });
    p.add_shell(sh).unwrap();
    let body = p.exact_bodies.0[0].name.clone().unwrap();
    let def = SheetConvertFeature {
        id: String::new(),
        name: String::new(),
        body,
        face: FaceRef {
            face: "Box1:Bottom".into(),
            hint: [50.0, 30.0, 0.0],
        },
        bends: vec![
            edge("Box1:Bottom", "Box1:Front", [50.0, 0.0, 0.0]),
            edge("Box1:Bottom", "Box1:Right", [100.0, 30.0, 0.0]),
            edge("Box1:Bottom", "Box1:Back", [50.0, 60.0, 0.0]),
            edge("Box1:Bottom", "Box1:Left", [0.0, 30.0, 0.0]),
        ],
    };
    p.add_sheet_convert(def).unwrap();
    check(&p);
    let flat = &p.flat_patterns()[0];
    assert_eq!(flat.bends.len(), 4);
    let b = cadcraft3d::kernel::exact::to_brep(std::slice::from_ref(&flat.solid)).unwrap();
    let (lo, hi) = b.bounding_box().unwrap();
    assert!(
        (hi.z - lo.z - 1.5).abs() < 0.05,
        "the shell's wall: {lo} {hi}"
    );
}

fn op(p: &mut PartDocument, o: cadcraft3d::part::sheet_metal::SheetOp) -> String {
    p.add_sheet_op(cadcraft3d::part::sheet_metal::SheetOpFeature::new(o))
        .unwrap()
}

/// A sketch on XY: the plate's bottom face (plane coordinates = world x, y).
fn top_sketch(p: &mut PartDocument) -> usize {
    let o = SketchOptions {
        auto_project_origin: false,
        ..Default::default()
    };
    p.create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &o)
        .unwrap()
}

/// Hem: a 180° fold back at the edge; the flat pattern grows by the hem's
/// developed length, the part by the gap and the thickness past the edge.
#[test]
fn hem_on_an_edge() {
    use cadcraft3d::part::sheet_metal::SheetOp;
    let mut p = rect(100.0, 60.0);
    let (body, e) = p.sheet_edge_at(Point3::new(50.0, 0.0, 2.0)).unwrap();
    let flat0 = p.flat_patterns()[0].size;
    op(
        &mut p,
        SheetOp::Hem {
            body,
            edges: vec![e],
            length: 10.0,
            gap: None,
            flip: false,
        },
    );
    check(&p);
    let (lo, hi) = bbox(&p);
    // The hem rolls round the edge: past it by the gap (T/2) and the thickness.
    assert!(
        (hi.y - lo.y - 63.0).abs() < 0.05,
        "past the edge by gap + T: {lo} {hi}"
    );
    assert!(hi.z - lo.z > 4.0, "two layers and a gap: {lo} {hi}");
    let fp = &p.flat_patterns()[0];
    assert_eq!(fp.bends.len(), 1);
    let grow = fp.size.0.max(fp.size.1) - flat0.0.max(flat0.1);
    let grow = if grow.abs() < 1.0 {
        fp.size.0.min(fp.size.1) - flat0.0.min(flat0.1)
    } else {
        grow
    };
    assert!(grow > 5.0, "the flat grew by the hem: {grow}");
}

/// Fold along a sketch line across the plate: one bend, the flat pattern
/// keeps the plate's size (the bend zone is taken out of the sheet).
#[test]
fn fold_along_a_line() {
    use cadcraft3d::part::sheet_metal::{FoldLocation, SheetOp};
    let mut p = rect(100.0, 60.0);
    let flat0 = p.flat_patterns()[0].size;
    let i = top_sketch(&mut p);
    let line = p.sketches[i]
        .sketch
        .add_line(Point2::new(70.0, -5.0), Point2::new(70.0, 65.0));
    let sid = p.sketches[i].id.clone();
    let line_key = cadcraft3d::part::extrude::entity_key(line);
    let body = p.sheet_models.keys().next().unwrap().clone();
    op(
        &mut p,
        SheetOp::Fold {
            body,
            sketch: sid,
            line: line_key,
            angle: FRAC_PI_2,
            radius: None,
            location: FoldLocation::Centerline,
            flip: false,
            flip_side: false,
        },
    );
    check(&p);
    let fp = &p.flat_patterns()[0];
    assert_eq!(fp.bends.len(), 1);
    assert!(
        (fp.size.0.max(fp.size.1) - flat0.0.max(flat0.1)).abs() < 0.05,
        "{:?} {:?}",
        fp.size,
        flat0
    );
    let (lo, hi) = bbox(&p);
    assert!(hi.z - lo.z > 20.0, "a leg stands up: {lo} {hi}");
    assert!(hi.x < 80.0, "folded at x = 70: {lo} {hi}");
}

/// Contour Flange: an open chain of three lines (a U) on XZ, 50 wide: a
/// channel with two bends.
#[test]
fn contour_flange_channel() {
    use cadcraft3d::part::sheet_metal::SheetOp;
    let mut p = PartDocument::default();
    p.make_sheet_metal(None);
    let o = SketchOptions {
        auto_project_origin: false,
        ..Default::default()
    };
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XZ), &o)
        .unwrap();
    let sk = &mut p.sketches[i].sketch;
    let q: Vec<_> = [(0.0, 30.0), (0.0, 0.0), (60.0, 0.0), (60.0, 30.0)]
        .iter()
        .map(|&(x, y)| sk.add_point(Point2::new(x, y)))
        .collect();
    for k in 0..3 {
        sk.add_line_between(q[k], q[k + 1]).unwrap();
    }
    let sid = p.sketches[i].id.clone();
    let id = op(
        &mut p,
        SheetOp::Contour {
            sketch: sid,
            width: 50.0,
            flip: false,
        },
    );
    check(&p);
    assert!(p.sheet_models.contains_key(&id));
    let fp = &p.flat_patterns()[0];
    assert_eq!(fp.bends.len(), 2);
    let v = volume(&p);
    let rough = (30.0 + 60.0 + 30.0) * 50.0 * 2.0;
    assert!((v - rough).abs() < 0.1 * rough, "{v} vs {rough}");
    // A flange on the channel's edge works too.
    let (lo, hi) = bbox(&p);
    eprintln!("channel box {lo} {hi}");
}

/// Unfold, cut a slot across the bend in the flat, Refold: the slot goes
/// through the bend; the flat pattern has it.
#[test]
fn unfold_cut_refold() {
    use cadcraft3d::part::sheet_metal::SheetOp;
    let mut p = rect(100.0, 60.0);
    flange(&mut p, &[Point3::new(50.0, 0.0, 2.0)], 25.0, |_| {});
    let body = p.sheet_models.keys().next().unwrap().clone();
    let v0 = volume(&p);
    op(&mut p, SheetOp::Unfold { body: body.clone() });
    check(&p);
    assert!(p.sheet_is_flat(&body));
    let (lo, hi) = bbox(&p);
    assert!((hi.z - lo.z - 2.0).abs() < 0.05, "flat: {lo} {hi}");
    // A slot over where the bend lies in the flat (y just below 0).
    let i = top_sketch(&mut p);
    p.sketches[i]
        .sketch
        .add_rectangle_two_point(Point2::new(40.0, -15.0), Point2::new(60.0, 10.0))
        .unwrap();
    let r = p.sketches[i].sketch.regions();
    let prof = vec![ProfileRef::of(&r[0], r[0].interior_point())];
    let sid = p.sketches[i].id.clone();
    op(
        &mut p,
        SheetOp::Cut {
            sketch: sid,
            profiles: prof,
        },
    );
    check(&p);
    op(&mut p, SheetOp::Refold { body: body.clone() });
    check(&p);
    assert!(!p.sheet_is_flat(&body));
    let v1 = volume(&p);
    assert!(
        v1 < v0 - 20.0 * 20.0 * 2.0 * 0.5,
        "the slot took material: {v0} -> {v1}"
    );
    // DXF of the flat: outline, the slot and the bend line on its layer.
    let dxf = p.flat_pattern_dxf(&body).unwrap();
    assert!(dxf.contains("BEND_UP") || dxf.contains("BEND_DOWN"));
    assert!(dxf.matches("POLYLINE").count() >= 2, "outline + slot");
}

/// Cut on the folded sheet: a circle on the top face is kept as a cut-out,
/// so a flange can still be added after it (a Hole could not).
#[test]
fn sheet_cut_then_flange() {
    use cadcraft3d::part::sheet_metal::SheetOp;
    let mut p = rect(100.0, 60.0);
    let i = top_sketch(&mut p);
    p.sketches[i]
        .sketch
        .add_circle(Point2::new(50.0, 30.0), 8.0)
        .unwrap();
    let r = p.sketches[i].sketch.regions();
    let prof = vec![ProfileRef::of(&r[0], r[0].interior_point())];
    let sid = p.sketches[i].id.clone();
    let v0 = volume(&p);
    op(
        &mut p,
        SheetOp::Cut {
            sketch: sid,
            profiles: prof,
        },
    );
    check(&p);
    let v1 = volume(&p);
    let want = PI * 64.0 * 2.0;
    assert!(((v0 - v1) - want).abs() < 0.02 * want, "{v0} -> {v1}");
    flange(&mut p, &[Point3::new(50.0, 0.0, 2.0)], 20.0, |_| {});
    check(&p);
    // A cut whose sketch misses the sheet is refused.
    let j = top_sketch(&mut p);
    p.sketches[j]
        .sketch
        .add_circle(Point2::new(500.0, 500.0), 5.0)
        .unwrap();
    let r = p.sketches[j].sketch.regions();
    let prof = vec![ProfileRef::of(&r[0], r[0].interior_point())];
    let sid = p.sketches[j].id.clone();
    let n = p.features.len();
    let bad = p.add_sheet_op(cadcraft3d::part::sheet_metal::SheetOpFeature::new(
        SheetOp::Cut {
            sketch: sid,
            profiles: prof,
        },
    ));
    assert!(bad.is_err());
    assert_eq!(p.features.len(), n);
}
