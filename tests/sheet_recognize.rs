//! Convert to Sheet Metal of an imported sheet (`docs/spec/5.6-sheet-metal.md`
//! §6.1e): sheet parts made here are written to STEP, imported into an empty
//! part as a plain solid and converted with their bends recognised; the flat
//! pattern must come out as the original's (same rule, so the same K-factor),
//! with the same number of bends.

use cadcraft3d::kernel::{Point3, Real, Surface};
use cadcraft3d::part::import::ImportOp;
use cadcraft3d::part::sheet_metal::{SheetConvertFeature, SheetFlangeFeature, SheetMetalDefaults};
use cadcraft3d::part::shell::FaceRef;
use cadcraft3d::part::{
    BodyOp, BodyOpFeature, OriginPlane, PartDocument, ProfileRef, SketchOptions, SketchPlacement,
};
use cadcraft3d::sketch::Point2;

fn rule() -> SheetMetalDefaults {
    SheetMetalDefaults {
        thickness: 2.0,
        k_factor: 0.44,
        ..Default::default()
    }
}

/// A 100 × 60 Face on XY with circular holes.
fn plate(holes: &[(Real, Real, Real)]) -> PartDocument {
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
    sk.add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(100.0, 60.0))
        .unwrap();
    for &(x, y, r) in holes {
        sk.add_circle(Point2::new(x, y), r).unwrap();
    }
    let regions = sk.regions();
    let r = regions
        .iter()
        .max_by(|a, b| a.area().total_cmp(&b.area()))
        .unwrap();
    p.add_sheet_face(i, vec![ProfileRef::of(r, r.interior_point())], false)
        .unwrap();
    p
}

/// Flanges on the plate's edges at these points (on its top face).
fn flanged(mut p: PartDocument, at: &[(Real, Real)], height: Real, angle: Real) -> PartDocument {
    let mut edges = Vec::new();
    let mut body = String::new();
    for &(x, y) in at {
        let (b, e) = p.sheet_edge_at(Point3::new(x, y, 2.0)).unwrap();
        body = b;
        edges.push(e);
    }
    let mut f = SheetFlangeFeature::new(&body, edges, height);
    f.angle = angle.to_radians();
    p.add_sheet_flange(f).unwrap();
    p
}

/// `src` written to STEP, imported and converted from its bottom face
/// (z = 0, the plate's underside).
fn round_trip(src: &PartDocument) -> Result<PartDocument, String> {
    round_trip_with(src, rule(), [30.0, 30.0, 0.0])
}

/// `src` written to STEP, imported under `rule` and converted from the
/// planar face through `hint`.
fn round_trip_with(
    src: &PartDocument,
    rule: SheetMetalDefaults,
    hint: [Real; 3],
) -> Result<PartDocument, String> {
    let text = cadcraft3d::kernel::exact::step_text(&src.exact_bodies.0, "Sheet")?;
    let mut p = PartDocument::default();
    p.make_sheet_metal(Some(rule));
    p.add_body_op(BodyOpFeature::new(BodyOp::Import(ImportOp::new(
        "sheet.step",
        text.as_str(),
    ))))
    .map_err(|e| e.to_string())?;
    let h = Point3::from(hint);
    let name = p
        .body
        .faces()
        .find_map(|(id, f)| {
            let Some(Surface::Plane(pl)) = p.body.surface(f.surface) else {
                return None;
            };
            if (h - pl.origin).dot(&pl.normal).abs() > 1e-6 {
                return None;
            }
            let _ = id;
            let pts: Vec<Point3> = f
                .outer
                .coedges
                .iter()
                .filter_map(|c| p.body.edge(c.edge))
                .filter_map(|e| p.body.vertex(e.start).map(|v| v.point))
                .collect();
            let lo = pts
                .iter()
                .fold(Point3::new(f64::MAX, f64::MAX, f64::MAX), |a, q| a.inf(q));
            let hi = pts
                .iter()
                .fold(Point3::new(f64::MIN, f64::MIN, f64::MIN), |a, q| a.sup(q));
            let inside = (0..3).all(|k| h[k] >= lo[k] - 1e-6 && h[k] <= hi[k] + 1e-6);
            inside.then(|| f.name.clone()).flatten()
        })
        .ok_or("no face through the hint")?;
    let body = p
        .exact_bodies
        .0
        .first()
        .and_then(|s| s.name.clone())
        .ok_or("no body")?;
    p.add_sheet_convert(SheetConvertFeature {
        id: String::new(),
        name: String::new(),
        body,
        face: FaceRef { face: name, hint },
        bends: Vec::new(),
        recognize: true,
    })
    .map_err(|e| e.to_string())?;
    Ok(p)
}

fn check(what: &str, src: &PartDocument) -> Result<(), String> {
    let p = round_trip(src).map_err(|e| format!("{what}: {e}"))?;
    if !p.feature_health.is_empty() {
        return Err(format!("{what}: {:?}", p.feature_health));
    }
    let (a, b) = (&src.flat_patterns()[0], &p.flat_patterns()[0]);
    let (sa, sb) = (
        (a.size.0.max(a.size.1), a.size.0.min(a.size.1)),
        (b.size.0.max(b.size.1), b.size.0.min(b.size.1)),
    );
    if (sa.0 - sb.0).abs() > 1e-3 || (sa.1 - sb.1).abs() > 1e-3 || a.bends.len() != b.bends.len() {
        return Err(format!(
            "{what}: flat {:.4} × {:.4} with {} bends, the original {:.4} × {:.4} with {}",
            sb.0,
            sb.1,
            b.bends.len(),
            sa.0,
            sa.1,
            a.bends.len()
        ));
    }
    Ok(())
}

#[test]
fn imported_sheets_unfold_as_the_originals() {
    let cases: Vec<(&str, PartDocument)> = vec![
        (
            "one flange",
            flanged(plate(&[]), &[(100.0, 30.0)], 30.0, 90.0),
        ),
        (
            "one flange at 120°",
            flanged(plate(&[]), &[(100.0, 30.0)], 25.0, 120.0),
        ),
        (
            "one flange at 45°",
            flanged(plate(&[]), &[(50.0, 60.0)], 20.0, 45.0),
        ),
        (
            "two opposite flanges, holes",
            flanged(
                plate(&[(30.0, 30.0, 5.0), (70.0, 20.0, 3.0)]),
                &[(100.0, 30.0), (0.0, 30.0)],
                20.0,
                90.0,
            ),
        ),
    ];
    let bad: Vec<String> = cases
        .iter()
        .filter_map(|(w, p)| check(w, p).err())
        .collect();
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// A plain block is not sheet metal: refused with the reason, nothing changes.
#[test]
fn a_block_is_refused() {
    let mut src = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
    src.regenerate().unwrap();
    assert!(round_trip(&src).is_err());
}

/// Further shapes: a return on a flange, a hem, a tray of four flanges.
#[test]
fn imported_sheets_more_shapes() {
    use cadcraft3d::part::sheet_metal::{HemType, SheetOp, SheetOpFeature};
    let mut cases: Vec<(&str, PartDocument)> = Vec::new();
    // A return: a flange on the far edge of a flange.
    let mut p = flanged(plate(&[]), &[(100.0, 30.0)], 30.0, 90.0);
    let (b, e) = p.sheet_edge_at(Point3::new(102.0, 30.0, 32.0)).unwrap();
    let mut f = SheetFlangeFeature::new(&b, vec![e], 15.0);
    f.angle = 90f64.to_radians();
    p.add_sheet_flange(f).unwrap();
    cases.push(("a return", p));
    // A hem.
    let mut p = plate(&[]);
    let (b, e) = p.sheet_edge_at(Point3::new(100.0, 30.0, 2.0)).unwrap();
    p.add_sheet_op(SheetOpFeature::new(SheetOp::Hem {
        body: b,
        edges: vec![e],
        length: 10.0,
        gap: Some(1.0),
        flip: false,
        hem_type: Some(HemType::Single),
        radius: None,
        angle: None,
    }))
    .unwrap();
    cases.push(("a single hem", p));
    cases.push((
        "a tray",
        flanged(
            plate(&[]),
            &[(100.0, 30.0), (0.0, 30.0), (50.0, 0.0), (50.0, 60.0)],
            20.0,
            90.0,
        ),
    ));
    let bad: Vec<String> = cases
        .iter()
        .filter_map(|(w, p)| check(w, p).err())
        .collect();
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// A body that never was a sheet here: an L section (legs 60 and 40 outside,
/// T 2, inner radius 3) extruded 50, written to STEP and converted. Its flat
/// is 60 + π/2·(R + K·T) + (40 − R − T) long (K 0.44) and 50 wide.
#[test]
fn an_extruded_l_section_unfolds() {
    use cadcraft3d::part::ExtrudeFeature;
    let mut src = PartDocument::default();
    let o = SketchOptions {
        auto_project_origin: false,
        ..Default::default()
    };
    let i = src
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XZ), &o)
        .unwrap();
    let sk = &mut src.sketches[i].sketch;
    let pt =
        |sk: &mut cadcraft3d::sketch::Sketch, x: Real, y: Real| sk.add_point(Point2::new(x, y));
    let p: Vec<_> = [
        (0.0, 0.0),
        (60.0, 0.0),
        (65.0, 5.0),
        (65.0, 40.0),
        (63.0, 40.0),
        (63.0, 5.0),
        (60.0, 2.0),
        (0.0, 2.0),
    ]
    .iter()
    .map(|&(x, y)| pt(sk, x, y))
    .collect();
    let c = pt(sk, 60.0, 5.0);
    for (a, b) in [(0, 1), (2, 3), (3, 4), (4, 5), (6, 7), (7, 0)] {
        sk.add_line_between(p[a], p[b]).unwrap();
    }
    sk.add_arc(c, p[1], p[2]).unwrap();
    sk.add_arc(c, p[6], p[5]).unwrap();
    let regions = sk.regions();
    let r = regions
        .iter()
        .max_by(|a, b| a.area().total_cmp(&b.area()))
        .unwrap();
    src.add_extrude(
        i,
        ExtrudeFeature::new(vec![ProfileRef::of(r, r.interior_point())], 50.0),
    )
    .unwrap();
    let p = round_trip_with(&src, rule(), [30.0, -25.0, 0.0]).unwrap();
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    let fp = &p.flat_patterns()[0];
    let want = 60.0 + std::f64::consts::FRAC_PI_2 * (3.0 + 0.44 * 2.0) + (40.0 - 5.0);
    let (len, wid) = (fp.size.0.max(fp.size.1), fp.size.0.min(fp.size.1));
    assert!(
        (len - want).abs() < 1e-3 && (wid - 50.0).abs() < 1e-3,
        "{len} x {wid}, want {want} x 50"
    );
    assert_eq!(fp.bends.len(), 1);
    assert!((fp.bends[0].radius - 3.0).abs() < 1e-6, "{:?}", fp.bends[0]);
}

/// The saved sheet of format revision 20 (a box converted to a tray, a flange on it):
/// its STEP, converted, unfolds to the same flat.
#[test]
fn a_saved_sheet_round_trips() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/compat/r20/sheet_r20.3dcadcraft");
    let project = cadcraft3d::document::Project::load(&path).unwrap();
    let mut src = project
        .documents()
        .iter()
        .find_map(|d| match &d.data {
            cadcraft3d::document::DocumentData::Part(p) => Some(p.clone()),
            _ => None,
        })
        .unwrap();
    src.regenerate().unwrap();
    // The fixture's rule, so both flats use the same K-factor.
    let p = round_trip_with(&src, src.sheet_metal.clone().unwrap(), [50.0, 30.0, 40.0]).unwrap();
    let (a, b) = (&src.flat_patterns()[0], &p.flat_patterns()[0]);
    assert_eq!(a.bends.len(), b.bends.len());
    let (sa, sb) = (a.size.0.max(a.size.1), b.size.0.max(b.size.1));
    assert!((sa - sb).abs() < 1e-3, "{sb} vs the original {sa}");
}
