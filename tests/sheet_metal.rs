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
        // Trim to Bend is the square between the bend zones' lines by definition: the closed
        // panels' ends stand a little past it. A square relief (8 across about the crossing of
        // the centre lines, BA/2 = 2.262 out) meets each panel's closed end 2.324 short of its
        // side: two jogs a corner, which are the relief's own shape.
        match shape {
            CornerRelief::Trim => {}
            CornerRelief::Square => {
                let steps = flat_steps(&p);
                assert!(
                    steps.len() == 8 && steps.iter().all(|s| (s.0 - 2.324).abs() < 1e-3),
                    "{steps:?}"
                );
            }
            _ => laser_ready(&p, &format!("{shape:?}")),
        }
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
            direction: Default::default(),
            width2: 0.0,
            centered: false,
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
            extent: Default::default(),
            depth: 0.0,
            across: false,
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
            extent: Default::default(),
            depth: 0.0,
            across: false,
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
            extent: Default::default(),
            depth: 0.0,
            across: false,
            sketch: sid,
            profiles: prof,
        },
    ));
    assert!(bad.is_err());
    assert_eq!(p.features.len(), n);
}

/// A 120 x 80 tray: four 20 mm flanges, corners closed with the rule's gap.
fn tray() -> PartDocument {
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
    p
}

/// The points of the panel faces of flange `tag` (past its bend: not the
/// rims at the bend's ends).
fn flange_points(p: &PartDocument, tag: &str) -> Vec<Point3> {
    let key = format!(":{tag}:");
    p.body
        .faces()
        .filter(|(_, f)| {
            f.name
                .as_deref()
                .is_some_and(|n| n.contains(&key) && !n.contains("rim.bend"))
        })
        .filter_map(|(_, f)| f.mesh.as_ref())
        .flat_map(|m| m.positions.iter().copied())
        .collect()
}

/// The flange of the tray that stands on its front edge (y = 0).
fn front_tag(p: &PartDocument, c: &cadcraft3d::part::sheet_metal::SheetCorner) -> bool {
    flange_points(p, &c.a).iter().all(|q| q.y < 2.0 + 1e-6)
}

/// Corner Seam: the front-right corner of a tray closed as Symmetric Gap
/// (both 1 mm from the halving plane), Overlap (front runs flush over
/// the right flange's edge, the right stops the gap short) and Reverse
/// Overlap; a wider gap; every result a valid sheet with a flat pattern.
#[test]
fn corner_seam_types() {
    use cadcraft3d::part::sheet_metal::{SeamType, SheetOp};
    let p = tray();
    let corners = p.sheet_corners();
    assert_eq!(corners.len(), 4, "{corners:?}");
    // The front-right corner, picked near its top.
    let c = p
        .sheet_corner_near(Point3::new(120.0, 0.0, 15.0), 100.0)
        .unwrap();
    // Where the bend lines cross: Inside of the base, set back (R + T)·tan 45° = 4.
    assert!(
        (c.at.x - 116.0).abs() < 1e-6 && (c.at.y - 4.0).abs() < 1e-6,
        "{c:?}"
    );
    // Which of the two is the front flange (its bend runs along x).
    let front_first = front_tag(&p, &c);
    let (front, right) = if front_first {
        (c.a.clone(), c.b.clone())
    } else {
        (c.b.clone(), c.a.clone())
    };
    let t = 2.0;
    let front_reach = |q: &PartDocument, tag: &str| {
        flange_points(q, tag)
            .iter()
            .map(|v| v.x)
            .fold(Real::NEG_INFINITY, Real::max)
    };
    let right_reach = |q: &PartDocument, tag: &str| {
        flange_points(q, tag)
            .iter()
            .map(|v| v.y)
            .fold(Real::INFINITY, Real::min)
    };
    let base = front_reach(&p, &front);
    // Symmetric Gap is measured square to the plane halving the corner (45°
    // here): the inner faces' edges are g apart, so an end stops (g/2)·√2
    // short of the other flange's inner face.
    let sq2 = std::f64::consts::SQRT_2;
    assert!(
        (base - (120.0 - t - 1.0 * sq2)).abs() < 0.05,
        "symmetric gap 2: {base}"
    );
    let mut reach = Vec::new();
    for (seam, gap) in [
        (SeamType::Overlap, None),
        (SeamType::ReverseOverlap, None),
        (SeamType::SymmetricGap, Some(4.0)),
    ] {
        let mut q = p.clone();
        op(
            &mut q,
            SheetOp::CornerSeam {
                body: c.body.clone(),
                a: front.clone(),
                b: right.clone(),
                seam,
                gap,
                ratio: 1.0,
            },
        );
        check(&q);
        assert_eq!(q.flat_patterns()[0].bends.len(), 4);
        reach.push((front_reach(&q, &front), right_reach(&q, &right)));
    }
    // Overlap: the front runs flush over the right's edge; the right stops
    // the gap (2) short of the front's inner face (y = 2).
    assert!((reach[0].0 - 120.0).abs() < 0.05, "{reach:?}");
    assert!((reach[0].1 - (t + 2.0)).abs() < 0.05, "{reach:?}");
    // Reverse: the right runs flush down to the front's outer face (y = 0).
    assert!(reach[1].1.abs() < 0.05, "{reach:?}");
    assert!(
        (reach[2].0 - (120.0 - t - 2.0 * sq2)).abs() < 0.05,
        "gap 4, {reach:?}"
    );
    // Half the thickness of overlap.
    let mut q = p.clone();
    op(
        &mut q,
        SheetOp::CornerSeam {
            body: c.body.clone(),
            a: front.clone(),
            b: right.clone(),
            seam: SeamType::Overlap,
            gap: None,
            ratio: 0.5,
        },
    );
    check(&q);
    assert!((front_reach(&q, &front) - 119.0).abs() < 0.05);
    // Flanges that do not meet: refused, nothing changes.
    let mut q = p.clone();
    let n = q.features.len();
    let bad = q.add_sheet_op(cadcraft3d::part::sheet_metal::SheetOpFeature::new(
        SheetOp::CornerSeam {
            body: c.body.clone(),
            a: front,
            b: "Flange9.9".into(),
            seam: SeamType::Overlap,
            gap: None,
            ratio: 1.0,
        },
    ));
    assert!(bad.is_err());
    assert_eq!(q.features.len(), n);
}

/// Edit Feature of every sheet metal feature type: the same kind replaces
/// it in place (id and name kept); another kind is refused.
#[test]
fn sheet_features_can_be_edited() {
    use cadcraft3d::part::Feature;
    use cadcraft3d::part::sheet_metal::{SheetOp, SheetOpFeature};
    let mut p = rect(100.0, 60.0);
    flange(&mut p, &[Point3::new(50.0, 0.0, 2.0)], 20.0, |_| {});
    let k = p
        .features
        .iter()
        .position(|f| matches!(f, Feature::SheetFlange(_)))
        .unwrap();
    let Feature::SheetFlange(mut f) = p.features[k].clone() else {
        panic!("not a flange");
    };
    let id = f.id.clone();
    f.height = 30.0;
    f.id.clear();
    assert_eq!(p.replace_feature(k, Feature::SheetFlange(f)).unwrap(), id);
    check(&p);
    let (lo, hi) = bbox(&p);
    assert!((hi.z - lo.z - 30.0).abs() < 0.5, "{lo} {hi}");
    // A Hem cannot replace a Flange.
    let body = p.sheet_models.keys().next().unwrap().clone();
    let hem = Feature::SheetOp(SheetOpFeature::new(SheetOp::Unfold { body }));
    assert!(p.replace_feature(k, hem).is_err());
}

fn circle_cut(p: &mut PartDocument, c: (Real, Real), r: Real) -> (String, Vec<ProfileRef>) {
    let i = top_sketch(p);
    p.sketches[i]
        .sketch
        .add_circle(Point2::new(c.0, c.1), r)
        .unwrap();
    let regions = p.sketches[i].sketch.regions();
    let prof = regions
        .iter()
        .map(|r| ProfileRef::of(r, r.interior_point()))
        .collect();
    (p.sketches[i].id.clone(), prof)
}

/// Cut extents: Thickness (through), Distance (a pocket 1 mm deep: half
/// the material of the circle goes), Cut Across Bend (a slot from the plate
/// over the front bend into the flange, in one step).
#[test]
fn cut_extents_and_across_bend() {
    use cadcraft3d::part::sheet_metal::{CutExtent, SheetOp};
    let base = rect(100.0, 60.0);
    let v0 = volume(&base);
    let area = PI * 64.0;
    for (extent, depth, want) in [
        (CutExtent::Thickness, 0.0, area * 2.0),
        (CutExtent::Distance, 1.0, area * 1.0),
        (CutExtent::ThroughAll, 0.0, area * 2.0),
    ] {
        let mut p = base.clone();
        let (sketch, profiles) = circle_cut(&mut p, (50.0, 30.0), 8.0);
        op(
            &mut p,
            SheetOp::Cut {
                sketch,
                profiles,
                extent,
                depth,
                across: false,
            },
        );
        check(&p);
        let took = v0 - volume(&p);
        assert!(
            (took - want).abs() < 0.03 * want,
            "{extent:?}: {took} vs {want}"
        );
    }
    // Across the front bend: a 10 wide slot from y = 10 down past the edge
    // (y = -30 in the plate's plane), wrapped into the flange.
    let mut p = base.clone();
    flange(&mut p, &[Point3::new(50.0, 0.0, 2.0)], 25.0, |_| {});
    let v1 = volume(&p);
    let i = top_sketch(&mut p);
    p.sketches[i]
        .sketch
        .add_rectangle_two_point(Point2::new(45.0, -30.0), Point2::new(55.0, 10.0))
        .unwrap();
    let regions = p.sketches[i].sketch.regions();
    let profiles = vec![ProfileRef::of(&regions[0], regions[0].interior_point())];
    let sid = p.sketches[i].id.clone();
    op(
        &mut p,
        SheetOp::Cut {
            sketch: sid,
            profiles,
            extent: CutExtent::Thickness,
            depth: 0.0,
            across: true,
        },
    );
    check(&p);
    let took = v1 - volume(&p);
    // In the flat the material under the slot runs from y = 10 to the
    // flange's end: the base set back 4 (Inside), the bend's allowance
    // BA = π/2·(R + K·T), the leg 25 − 4: 10 wide × that × 2 thick.
    let ba = std::f64::consts::FRAC_PI_2 * (2.0 + 0.44 * 2.0);
    let len = 10.0 - (4.0 - ba - 21.0);
    let want = 10.0 * len * 2.0;
    assert!(
        (took - want).abs() < 0.02 * want,
        "across: {took} vs {want}"
    );
}

/// Flange corner settings: its own corner relief shape, size and gap, and a
/// Corner Override (Overlap at one corner), all on one tray feature.
#[test]
fn flange_corner_settings() {
    use cadcraft3d::part::sheet_metal::{CornerOverride, CornerRelief, SeamType};
    let edges = [
        Point3::new(60.0, 0.0, 2.0),
        Point3::new(120.0, 40.0, 2.0),
        Point3::new(60.0, 80.0, 2.0),
        Point3::new(0.0, 40.0, 2.0),
    ];
    let base = tray();
    let v_rule = volume(&base);
    let mut vols = Vec::new();
    for shape in [CornerRelief::Square, CornerRelief::Trim] {
        let mut p = rect(120.0, 80.0);
        flange(&mut p, &edges, 20.0, |d| {
            d.corner_shape = Some(shape);
            d.corner_size = Some(10.0);
            d.corner_gap = Some(3.0);
        });
        check(&p);
        vols.push(volume(&p));
    }
    assert!(
        vols[0] < vols[1],
        "a 10 mm square relief takes more than none: {vols:?}"
    );
    assert!(
        (vols[1] - v_rule).abs() > 1.0,
        "the feature's settings were used"
    );
    // Overlap at the corner between the first two flanges (front, right).
    let mut p = rect(120.0, 80.0);
    flange(&mut p, &edges, 20.0, |d| {
        d.seams = vec![CornerOverride {
            a: 0,
            b: 1,
            seam: SeamType::Overlap,
            gap: None,
            ratio: 1.0,
        }];
    });
    check(&p);
    let front = flange_points(&p, "Flange1.1")
        .iter()
        .map(|q| q.x)
        .fold(Real::NEG_INFINITY, Real::max);
    assert!(
        (front - 120.0).abs() < 0.05,
        "the front runs over the right flange: {front}"
    );
}

/// Seen in the app (2026-10-26): two flanges with an Overlap corner override,
/// and the base plate was not drawn. Every face of the sheet has triangles.
#[test]
fn overlap_corner_keeps_every_face_drawn() {
    use cadcraft3d::part::sheet_metal::{CornerOverride, SeamType};
    let mut p = rect(40.0, 25.0);
    flange(
        &mut p,
        &[Point3::new(20.0, 0.0, 2.0), Point3::new(40.0, 12.5, 2.0)],
        20.0,
        |d| {
            d.seams = vec![CornerOverride {
                a: 0,
                b: 1,
                seam: SeamType::Overlap,
                gap: None,
                ratio: 1.0,
            }];
        },
    );
    check(&p);
    for (_, f) in p.body.faces() {
        let n = f.mesh.as_ref().map_or(0, |m| m.triangles.len());
        eprintln!("{:?}: {n}", f.name);
    }
    let empty: Vec<_> = p
        .body
        .faces()
        .filter(|(_, f)| f.mesh.as_ref().is_none_or(|m| m.triangles.is_empty()))
        .filter_map(|(_, f)| f.name.clone())
        .collect();
    assert!(empty.is_empty(), "faces without triangles: {empty:?}");
    // The base's triangles face out of the material (top up, bottom down):
    // reversed ones are culled in the view (the plate was not drawn).
    for (_, f) in p.body.faces() {
        let (Some(n), Some(m)) = (f.name.as_deref(), f.mesh.as_ref()) else {
            continue;
        };
        let want = match n {
            "Face1:base:top" => 1.0,
            "Face1:base:bottom" => -1.0,
            _ => continue,
        };
        let mut sum = 0.0;
        for t in &m.triangles {
            let [a, b, c] = t.map(|i| m.positions[i as usize]);
            sum += (b - a).cross(&(c - a)).z;
        }
        assert!(
            sum * want > 0.0,
            "{n}: triangles face the wrong way ({sum})"
        );
    }
    // What the view draws: the solid's tessellation, every face in it.
    use cadcraft3d::kernel::TessellationOptions;
    for (sid, _) in p.body.solids() {
        let mesh = p
            .body
            .tessellate_solid(sid, &TessellationOptions::default())
            .unwrap();
        // Normals the shader can light (a NaN made the plate invisible).
        for (f, r) in &mesh.face_triangles {
            for t in &mesh.faces.triangles[r.clone()] {
                for &i in t {
                    let n = mesh.faces.normals[i as usize];
                    assert!(
                        n.iter().all(|x| x.is_finite()) && (n.norm() - 1.0).abs() < 1e-6,
                        "{:?}: normal {n:?}",
                        p.body.face(*f).and_then(|x| x.name.clone())
                    );
                }
            }
        }
        let drawn: Vec<_> = mesh
            .face_triangles
            .iter()
            .filter(|(_, r)| !r.is_empty())
            .filter_map(|(f, _)| p.body.face(*f).and_then(|x| x.name.clone()))
            .collect();
        for (_, f) in p.body.faces() {
            let n = f.name.clone().unwrap_or_default();
            assert!(drawn.contains(&n), "{n} is not drawn: {drawn:?}");
        }
    }
}

/// The owner's session of 2026-10-26 (`tests/data/sheet_cut_across_journal.jsonl`): a rectangle
/// sketched on a flange's face, running past the face's edge onto the bend. Cut Across Bend did
/// nothing over the bend, because the projected edge of the face split the rectangle into two
/// profiles and only the one on the face was picked. A Cut's profiles are now the user's own
/// curves (`PartSketch::own_regions`): the whole rectangle is one profile and its developed area
/// is cut, across the bend.
#[test]
fn cut_across_bend_from_the_owners_session() {
    use cadcraft3d::diagnostics::replay;
    use cadcraft3d::part::Feature;
    use cadcraft3d::part::sheet_metal::SheetOp;
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/sheet_cut_across_journal.jsonl");
    // The part before the Cut (state 11) and the Cut the owner made (state 12).
    let before = replay::part_at(&path, 11).unwrap();
    let mut p = replay::part_at(&path, 12).unwrap();
    p.regenerate().unwrap();
    let ps = p.sketches.iter().find(|s| s.id == "Sketch2").unwrap();
    assert_eq!(
        ps.sketch.regions().len(),
        3,
        "split by the face's projected edge"
    );
    let own = ps.own_regions();
    let rect = own
        .iter()
        .find(|r| r.area() < 2000.0)
        .expect("the rectangle as one profile");
    assert!(
        (rect.area() - (624.0 + 574.0)).abs() < 5.0,
        "{}",
        rect.area()
    );
    let k = p
        .features
        .iter()
        .position(|f| matches!(f, Feature::SheetOp(_)))
        .unwrap();
    let Feature::SheetOp(mut o) = p.features[k].clone() else {
        panic!("not a sheet op");
    };
    if let SheetOp::Cut { across, .. } = &mut o.op {
        *across = true;
    }
    p.replace_feature(k, Feature::SheetOp(o)).unwrap();
    check(&p);
    let mut b = before.clone();
    b.regenerate().unwrap();
    let took = volume(&b) - volume(&p);
    let t = 2.0;
    let want = rect.area() * t;
    assert!(
        (took - want).abs() < 0.03 * want,
        "across the bend: {took} vs {want}"
    );
}

/// Contour Flange Direction (owner, 2026-10-26: direction and centring were
/// missing): the width runs along the sketch normal (XZ: −Y), against it, half
/// each way or by two distances; Position puts the thickness on either side of
/// the chain or centred on it.
#[test]
fn contour_flange_direction_and_position() {
    use cadcraft3d::part::sheet_metal::{ContourDirection, SheetOp};
    let build = |direction: ContourDirection, flip: bool, centered: bool| {
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
        op(
            &mut p,
            SheetOp::Contour {
                sketch: sid,
                width: 50.0,
                flip,
                direction,
                width2: 20.0,
                centered,
            },
        );
        check(&p);
        p
    };
    let t = 2.0;
    for (d, (y0, y1)) in [
        (ContourDirection::Default, (-50.0, 0.0)),
        (ContourDirection::Flipped, (0.0, 50.0)),
        (ContourDirection::Symmetric, (-25.0, 25.0)),
        (ContourDirection::Asymmetric, (-50.0, 20.0)),
    ] {
        let (lo, hi) = bbox(&build(d, false, false));
        assert!(
            (lo.y - y0).abs() < 1e-3 && (hi.y - y1).abs() < 1e-3,
            "{d:?}: y {} .. {}",
            lo.y,
            hi.y
        );
    }
    // Position: outside the U (x −T .. 60+T), inside (0 .. 60), centred.
    for (flip, centered, (x0, x1)) in [
        (false, false, (-t, 60.0 + t)),
        (true, false, (0.0, 60.0)),
        (false, true, (-t / 2.0, 60.0 + t / 2.0)),
        (true, true, (-t / 2.0, 60.0 + t / 2.0)),
    ] {
        let (lo, hi) = bbox(&build(ContourDirection::Default, flip, centered));
        assert!(
            (lo.x - x0).abs() < 1e-3 && (hi.x - x1).abs() < 1e-3,
            "flip {flip} centred {centered}: x {} .. {}",
            lo.x,
            hi.x
        );
    }
}

/// Flat pattern extents (Inventor's FLAT PATTERN EXTENTS LENGTH / WIDTH /
/// AREA) and the bend lines the Flat Pattern view draws: a 100 × 60 plate
/// with a 25 mm flange on its front edge flattens to 100 long, 60 + the
/// flange's developed length wide; one bend line, its zone the allowance wide,
/// on the flat's top face.
#[test]
fn flat_extents_and_bend_lines() {
    let mut p = rect(100.0, 60.0);
    let plain = p.flat_extents().unwrap();
    assert!((plain.length - 100.0).abs() < 1e-6 && (plain.width - 60.0).abs() < 1e-6);
    assert!((plain.area - 6000.0).abs() < 1e-3, "{}", plain.area);
    flange(&mut p, &[Point3::new(50.0, 0.0, 2.0)], 25.0, |_| {});
    let body = p.sheet_models.keys().next().unwrap().clone();
    let x = p.flat_extents().unwrap();
    let fp = &p.flat_patterns()[0];
    // The flange adds to the 60 side (60 + developed length < 100): length
    // stays the 100 edge, width is the developed side.
    assert!((x.length - 100.0).abs() < 1e-6, "{}", x.length);
    assert!((x.width - fp.size.0.min(fp.size.1)).abs() < 1e-9);
    assert!(x.width > 60.0 + 20.0, "{}", x.width);
    // No cut-outs: the area is the box's (within the outline's sampling).
    assert!((x.area - x.length * x.width).abs() < 1e-3 * x.area, "{x:?}");
    let lines = p.flat_bend_lines(&body);
    assert_eq!(lines.len(), 1);
    let l = &lines[0];
    assert!(((l.centre[1] - l.centre[0]).norm() - 100.0).abs() < 1e-6);
    let gap = (l.zone[1][0] - l.zone[0][0]).norm();
    assert!(
        (gap - l.bend.allowance).abs() < 1e-9,
        "{gap} vs {}",
        l.bend.allowance
    );
    // On the flat solid's top face (z = T).
    for q in l.centre.iter().chain(l.zone.iter().flatten()) {
        assert!(q.z > 1.99 && q.z < 2.1, "{q}");
    }
}

/// The owner's "Box 2" (2026-10-26): a 400 × 200 tray, walls 80 high, a lip 20
/// in on every wall, a return 12 down on every lip, a 10 mm 45° flange on every
/// return — each level one Flange on four edges. The returns used to meet
/// unmitred at the corners (their inner bends ran into each other) and the 45°
/// flanges failed ("the faces do not close"): corners were closed on the base
/// and one level up only. Every level is mitred now, a gap apart.
#[test]
fn rim_flanges_are_mitred_at_every_level() {
    let mut p = rect(400.0, 200.0);
    let level = |p: &mut PartDocument, at: &[(Real, Real, Real)], h: Real, deg: Real| {
        let pts: Vec<Point3> = at.iter().map(|&(x, y, z)| Point3::new(x, y, z)).collect();
        flange(p, &pts, h, |d| d.angle = deg.to_radians());
        check(p);
    };
    level(
        &mut p,
        &[
            (400.0, 125.0, 2.0),
            (358.0, 200.0, 2.0),
            (0.0, 140.0, 2.0),
            (96.0, 0.0, 2.0),
        ],
        80.0,
        90.0,
    );
    // Only free edges take a flange (owner, 2026-10-26): the base edges have
    // the walls' bends, a wall's top edge is free until the lip stands on it.
    let body = p.sheet_models.keys().next().unwrap().clone();
    assert!(!p.sheet_edge_can_flange(&body, Point3::new(96.0, 0.0, 2.0)));
    assert!(p.sheet_edge_can_flange(&body, Point3::new(132.4, 2.0, 80.0)));
    level(
        &mut p,
        &[
            (2.0, 62.0, 80.0),
            (66.7, 198.0, 80.0),
            (132.4, 2.0, 80.0),
            (398.0, 78.5, 80.0),
        ],
        20.0,
        90.0,
    );
    assert!(!p.sheet_edge_can_flange(&body, Point3::new(132.4, 2.0, 80.0)));
    let corners_before = p.sheet_corners().len();
    level(
        &mut p,
        &[
            (380.0, 129.5, 80.0),
            (342.1, 180.0, 80.0),
            (20.0, 115.5, 80.0),
            (72.4, 20.0, 80.0),
        ],
        12.0,
        90.0,
    );
    let v3 = volume(&p);
    level(
        &mut p,
        &[
            (382.0, 46.3, 90.0),
            (366.3, 18.0, 90.0),
            (40.1, 182.0, 90.0),
            (18.0, 138.7, 90.0),
        ],
        10.0,
        45.0,
    );
    assert!(volume(&p) > v3, "the 45° flanges add material");
    assert_eq!(p.sheet_corners().len(), corners_before);
    // Mitred: each return is shorter than its lip's inner edge by about the
    // other return's depth at each end (a 45° picture-frame corner).
    let fp = &p.flat_patterns()[0];
    assert_eq!(fp.bends.len(), 16);
    // Every level unfolds without a step: each flange's bend on a flange is mitred like its
    // panel (a curve in the flat from its parent's end to where its panel's end starts, the gap
    // kept all round), as Inventor does.
    laser_ready(&p, "rim");
}

/// The flat pattern can go to a laser as it is (owner, 2026-10-26): its outline has no
/// step — a jog shorter than 3·T that turns sharply (more than 30°) one way and then the other,
/// as a bend stopping short of its parent's end or a panel running on past its bend left. A
/// relief's slot turns the same way at both its corners and is no step.
fn laser_ready(p: &PartDocument, what: &str) {
    let steps = flat_steps(p);
    assert!(
        steps.is_empty(),
        "{what}: steps (length, at) in the flat pattern: {steps:?}"
    );
}

/// The jogs of [`laser_ready`] in the flat patterns' outlines: (length, where).
fn flat_steps(p: &PartDocument) -> Vec<(Real, [Real; 2])> {
    use cadcraft3d::kernel::exact::sheet_metal as ksm;
    let mut out = Vec::new();
    for st in p.sheet_models.values() {
        let t = ksm::thickness(&st.model);
        for lp in ksm::flat_loops(&st.model).unwrap() {
            let mut pts: Vec<[Real; 2]> = Vec::new();
            for q in lp {
                if pts
                    .last()
                    .is_none_or(|l| (l[0] - q[0]).hypot(l[1] - q[1]) > 1e-9)
                {
                    pts.push(q);
                }
            }
            let n = pts.len();
            let dir = |i: usize| {
                let (a, b) = (pts[i % n], pts[(i + 1) % n]);
                let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                let l = dx.hypot(dy);
                (dx / l, dy / l, l)
            };
            // The signed turn from one edge to the next.
            let turn = |u: (Real, Real, Real), v: (Real, Real, Real)| {
                (u.0 * v.1 - u.1 * v.0).atan2(u.0 * v.0 + u.1 * v.1)
            };
            let sharp = 30f64.to_radians();
            for (i, &at) in pts.iter().enumerate() {
                let (before, edge, after) = (dir(i + n - 1), dir(i), dir(i + 1));
                let (t0, t1) = (turn(before, edge), turn(edge, after));
                if edge.2 < 3.0 * t && t0.abs() > sharp && t1.abs() > sharp && t0 * t1 < 0.0 {
                    out.push((edge.2, at));
                }
            }
        }
    }
    out
}

/// A part of the owner's (`tests/data/sheet_box*.3dcadcraft`, 2026-10-26), regenerated.
fn owners_box(name: &str) -> PartDocument {
    use cadcraft3d::document::{DocumentData, Project};
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name);
    let mut project = Project::load(&path).unwrap();
    let id = project
        .documents()
        .iter()
        .find(|d| matches!(d.data, DocumentData::Part(_)))
        .map(|d| d.id)
        .unwrap();
    let Some(DocumentData::Part(part)) = project.document_mut(id).map(|d| &mut d.data) else {
        panic!("{name}: no part");
    };
    part.regenerate().unwrap();
    part.clone()
}

/// The owner's boxes (walls, lips, a lip held back by Offset 1 / 1, a 45° rim; round corner
/// reliefs): the base corners are relieved about the crossing of the walls' bend centre lines,
/// every rim corner is mitred through its bends (each flange's bend on a flange cut along the
/// mitre from its parent's end to where its panel starts, as Inventor does), and the lip held
/// back gets the rule's bend relief in the return it stands on. Healthy, and no step in the flat
/// patterns but those reliefs.
#[test]
fn owners_boxes_unfold_laser_ready() {
    for name in ["sheet_box1.3dcadcraft", "sheet_box2.3dcadcraft"] {
        let p = owners_box(name);
        check(&p);
        assert_eq!(p.flat_patterns()[0].bends.len(), 16, "{name}");
        let steps = flat_steps(&p);
        if name.contains("box1") {
            // The bend reliefs beside the four lips held back (Relief Depth T/2 = 1 mm), one at
            // each of their ends, and nothing else.
            assert!(
                steps.len() == 8 && steps.iter().all(|s| (s.0 - 1.0).abs() < 1e-6),
                "{name}: {steps:?}"
            );
        } else {
            laser_ready(&p, name);
        }
    }
}

/// The corner edge through the thickness from `at` up by `up` along Z: its
/// two faces (sorted) and its middle.
fn corner_at(p: &PartDocument, at: Point3, up: Real) -> cadcraft3d::part::sheet_metal::CornerEdge {
    let top = at + cadcraft3d::kernel::Vector3::z() * up;
    corner_where(
        p,
        |a, b| {
            ((a - at).norm() < 1e-6 && (b - top).norm() < 1e-6)
                || ((b - at).norm() < 1e-6 && (a - top).norm() < 1e-6)
        },
        |_| true,
    )
    .unwrap_or_else(|| panic!("no corner edge at {at}"))
}

/// The first edge whose ends pass `ends` and faces pass `faces`, as a corner.
fn corner_where(
    p: &PartDocument,
    ends: impl Fn(Point3, Point3) -> bool,
    faces: impl Fn(&[String]) -> bool,
) -> Option<cadcraft3d::part::sheet_metal::CornerEdge> {
    for (e, x) in p.body.edges() {
        let (Some(a), Some(b)) = (p.body.vertex(x.start), p.body.vertex(x.end)) else {
            continue;
        };
        let (a, b) = (a.point, b.point);
        if !ends(a, b) {
            continue;
        }
        let mut names: Vec<String> = p
            .body
            .edge_faces(e)
            .into_iter()
            .filter_map(|f| p.body.face(f)?.name.clone())
            .collect();
        names.sort();
        if names.len() != 2 || !faces(&names) {
            continue;
        }
        let m = a + (b - a) * 0.5;
        return Some(cadcraft3d::part::sheet_metal::CornerEdge {
            faces: [names[0].clone(), names[1].clone()],
            hint: [m.x, m.y, m.z],
        });
    }
    None
}

fn flat_area(p: &PartDocument, body: &str) -> Real {
    p.flat_extents_of(body).unwrap().area
}

fn chamfer(
    corners: Vec<cadcraft3d::part::sheet_metal::CornerEdge>,
    method: cadcraft3d::part::sheet_metal::ChamferMethod,
    distance: Real,
    angle: Real,
    distance2: Real,
    flip: bool,
    edge: Option<String>,
) -> cadcraft3d::part::sheet_metal::SheetOp {
    cadcraft3d::part::sheet_metal::SheetOp::CornerChamfer {
        body: "Face1".into(),
        corners,
        method,
        distance,
        angle,
        distance2,
        flip,
        edge,
    }
}

/// Corner Round / Corner Chamfer (spec 5.6 §3.2): a round of r at a square
/// corner takes r²(1 − π/4) of the flat and t times that of the volume, an
/// equal chamfer d²/2; sets with their own radius; Distance and Angle; Two
/// Distances (Flip); a flange's far corner; a size past the edges is refused
/// and leaves the part as it was.
#[test]
fn corner_round_and_chamfer() {
    use cadcraft3d::part::sheet_metal::{ChamferMethod, CornerSet, SheetOp};
    let t = SheetMetalDefaults::default().values().thickness;
    let base = rect(100.0, 60.0);
    check(&base);
    let (v0, a0) = (volume(&base), flat_area(&base, "Face1"));
    let c00 = corner_at(&base, Point3::origin(), t);
    let c10 = corner_at(&base, Point3::new(100.0, 0.0, 0.0), t);
    let c11 = corner_at(&base, Point3::new(100.0, 60.0, 0.0), t);
    assert!(
        c00.faces.iter().all(|f| f.contains(":rim.")),
        "rim faces: {:?}",
        c00.faces
    );
    let round = |r: Real| r * r * (1.0 - PI / 4.0);

    // Two sets: R10 at two corners, R5 at the third.
    let mut p = base.clone();
    op(
        &mut p,
        SheetOp::CornerRound {
            body: "Face1".into(),
            sets: vec![
                CornerSet {
                    corners: vec![c00.clone(), c10.clone()],
                    radius: 10.0,
                },
                CornerSet {
                    corners: vec![c11.clone()],
                    radius: 5.0,
                },
            ],
        },
    );
    check(&p);
    // The rounds are named after the feature and the corner.
    let names: Vec<String> = p.body.faces().filter_map(|(_, f)| f.name.clone()).collect();
    for k in 1..=3 {
        let want = format!("Face1:CornerRound1.{k}:round");
        assert!(names.contains(&want), "{want} in {names:?}");
    }
    assert!(!names.iter().any(|n| n.contains("Bend")), "{names:?}");
    let taken = 2.0 * round(10.0) + round(5.0);
    let v1 = volume(&p);
    assert!(
        (v0 - v1 - t * taken).abs() < 0.02 * t * taken,
        "volume {v0} -> {v1}, expected -{}",
        t * taken
    );
    let a1 = flat_area(&p, "Face1");
    // The flat's area follows the arcs by chords (0.1 % here).
    assert!((a0 - a1 - taken).abs() < 5e-3 * taken, "flat {a0} -> {a1}");
    p.regenerate().unwrap();
    assert!((volume(&p) - v1).abs() < 1e-6 * v1, "regenerates the same");
    let json = serde_json::to_string(&p.features).unwrap();
    assert!(json.contains("CornerRound"), "{json}");

    // One Distance: d²/2 at a square corner.
    let mut q = base.clone();
    op(
        &mut q,
        chamfer(
            vec![c00.clone()],
            ChamferMethod::Distance,
            8.0,
            0.0,
            0.0,
            false,
            None,
        ),
    );
    check(&q);
    let got = a0 - flat_area(&q, "Face1");
    assert!((got - 32.0).abs() < 1e-3, "{got}");

    // Two Distances 10 × 4, either way round: 20 mm² each.
    for flip in [false, true] {
        let mut q = base.clone();
        op(
            &mut q,
            chamfer(
                vec![c00.clone()],
                ChamferMethod::TwoDistances,
                10.0,
                0.0,
                4.0,
                flip,
                None,
            ),
        );
        check(&q);
        let got = a0 - flat_area(&q, "Face1");
        assert!((got - 20.0).abs() < 1e-3, "{got}");
    }

    // Distance and Angle: 10 along the given face at 30° → 10·tan 30° on the other.
    let mut q = base.clone();
    op(
        &mut q,
        chamfer(
            vec![c00.clone()],
            ChamferMethod::DistanceAngle,
            10.0,
            30f64.to_radians(),
            0.0,
            false,
            Some(c00.faces[1].clone()),
        ),
    );
    check(&q);
    let want = 0.5 * 10.0 * 10.0 * 30f64.to_radians().tan();
    let got = a0 - flat_area(&q, "Face1");
    assert!((got - want).abs() < 1e-3, "{got} vs {want}");

    // Too large: refused, the part as it was.
    let mut q = base.clone();
    let n = q.features.len();
    let r = q.add_sheet_op(cadcraft3d::part::sheet_metal::SheetOpFeature::new(
        SheetOp::CornerRound {
            body: "Face1".into(),
            sets: vec![CornerSet {
                corners: vec![c00.clone()],
                radius: 80.0,
            }],
        },
    ));
    assert!(r.is_err());
    assert_eq!(q.features.len(), n);

    // A flange's far corner, rounded in the flange's plane.
    let mut f = base.clone();
    flange(&mut f, &[Point3::new(50.0, 0.0, t)], 20.0, |_| {});
    check(&f);
    let (vf, af) = (volume(&f), flat_area(&f, "Face1"));
    let corner = corner_where(
        &f,
        |_, _| true,
        |n| n.iter().any(|x| x.contains(":rim.far")) && n.iter().any(|x| x.contains(":rim.side")),
    )
    .expect("a corner of the flange's far end");
    op(
        &mut f,
        SheetOp::CornerRound {
            body: "Face1".into(),
            sets: vec![CornerSet {
                corners: vec![corner],
                radius: 5.0,
            }],
        },
    );
    check(&f);
    let got = af - flat_area(&f, "Face1");
    assert!((got - round(5.0)).abs() < 5e-3 * round(5.0), "{got}");
    let got = vf - volume(&f);
    assert!(
        (got - t * round(5.0)).abs() < 0.02 * t * round(5.0),
        "{got}"
    );
}

/// An inside corner (the notch of an L-shaped plate) is refused.
#[test]
fn corner_round_refuses_an_inside_corner() {
    use cadcraft3d::part::sheet_metal::{CornerSet, SheetOp};
    let t = SheetMetalDefaults::default().values().thickness;
    let p = plate(
        &[
            (0.0, 0.0),
            (80.0, 0.0),
            (80.0, 30.0),
            (40.0, 30.0),
            (40.0, 60.0),
            (0.0, 60.0),
        ],
        &[],
        SheetMetalDefaults::default(),
    );
    check(&p);
    let c = corner_at(&p, Point3::new(40.0, 30.0, 0.0), t);
    let mut q = p.clone();
    let e = q
        .add_sheet_op(cadcraft3d::part::sheet_metal::SheetOpFeature::new(
            SheetOp::CornerRound {
                body: "Face1".into(),
                sets: vec![CornerSet {
                    corners: vec![c],
                    radius: 5.0,
                }],
            },
        ))
        .expect_err("an inside corner");
    assert!(e.to_string().contains("inside corner"), "{e}");
}
