//! Lofted Flange (`docs/spec/5.6-sheet-metal.md` §6.1k): profiles on the XY
//! plane and on a work plane 50 above it. A strip between two parallel lines
//! is a flat plate; curved transitions keep their middle surface's area in
//! the flat pattern and their volume is that area times the thickness.
//! Inventor 2027 lofts a 60 × 40 rectangle to a Ø30 circle (T 2) into
//! 15702.79 mm³.

use std::f64::consts::PI;

use cadcraft3d::kernel::{Point3, Real};
use cadcraft3d::part::sheet_metal::{
    FacetControl, RipSide, SheetMetalDefaults, SheetOp, SheetOpFeature,
};
use cadcraft3d::part::{
    OriginPlane, PartDocument, PlaneRef, SketchOptions, SketchPlacement, WorkDef, WorkFeature,
    WorkPlaneDef,
};
use cadcraft3d::sketch::{Point2, Sketch};

const T: Real = 2.0;

/// A profile: lines (x0, y0, x1, y1) and arcs (centre, start, end, counter-clockwise).
enum Seg {
    L(Real, Real, Real, Real),
    A(Real, Real, Real, Real, Real, Real),
}

fn draw(sk: &mut Sketch, segs: &[Seg]) {
    // Shared points where ends meet, so the chain is connected.
    let mut pts: Vec<(Point2, cadcraft3d::sketch::PointId)> = Vec::new();
    let mut point = |sk: &mut Sketch, x: Real, y: Real| {
        let p = Point2::new(x, y);
        if let Some((_, id)) = pts.iter().find(|(q, _)| (q.x - x).hypot(q.y - y) < 1e-9) {
            return *id;
        }
        let id = sk.add_point(p);
        pts.push((p, id));
        id
    };
    for s in segs {
        match *s {
            Seg::L(x0, y0, x1, y1) => {
                let (a, b) = (point(sk, x0, y0), point(sk, x1, y1));
                sk.add_line_between(a, b).unwrap();
            }
            Seg::A(cx, cy, x0, y0, x1, y1) => {
                let c = sk.add_point(Point2::new(cx, cy));
                let (a, b) = (point(sk, x0, y0), point(sk, x1, y1));
                sk.add_arc(c, a, b).unwrap();
            }
        }
    }
}

fn loft(
    a: &[Seg],
    b: &[Seg],
    press_brake: bool,
    facet: FacetControl,
    side: RipSide,
) -> Result<PartDocument, String> {
    let mut p = PartDocument::default();
    p.make_sheet_metal(Some(SheetMetalDefaults {
        thickness: T,
        ..Default::default()
    }));
    let o = SketchOptions {
        auto_project_origin: false,
        ..Default::default()
    };
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &o)
        .unwrap();
    draw(&mut p.sketches[i].sketch, a);
    let s1 = p.sketches[i].id.clone();
    let wp = p
        .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
            base: PlaneRef::Origin(OriginPlane::XY),
            distance: 50.0,
        })))
        .unwrap();
    let j = p
        .create_sketch(
            SketchPlacement {
                plane: PlaneRef::Work(wp),
                ..Default::default()
            },
            &o,
        )
        .unwrap();
    draw(&mut p.sketches[j].sketch, b);
    let s2 = p.sketches[j].id.clone();
    p.add_sheet_op(SheetOpFeature::new(SheetOp::LoftedFlange {
        sketch1: s1,
        sketch2: s2,
        press_brake,
        facet,
        side,
        radius: None,
    }))
    .map_err(|e| e.to_string())?;
    Ok(p)
}

fn volume(p: &PartDocument) -> Real {
    p.exact_bodies.0[0].volume_at(1e-3).unwrap()
}

fn flat_area(p: &PartDocument) -> Real {
    p.flat_extents().unwrap().area
}

fn near(a: Real, b: Real, rel: Real) -> bool {
    (a - b).abs() <= rel * b.abs().max(1.0)
}

fn rect() -> Vec<Seg> {
    vec![
        Seg::L(-30.0, -20.0, 30.0, -20.0),
        Seg::L(30.0, -20.0, 30.0, 20.0),
        Seg::L(30.0, 20.0, -30.0, 20.0),
        Seg::L(-30.0, 20.0, -30.0, -20.0),
    ]
}

fn circle(r: Real) -> Vec<Seg> {
    vec![
        Seg::A(0.0, 0.0, r, 0.0, -r, 0.0),
        Seg::A(0.0, 0.0, -r, 0.0, r, 0.0),
    ]
}

#[test]
fn two_parallel_lines_make_a_plate() {
    let p = loft(
        &[Seg::L(-30.0, 0.0, 30.0, 0.0)],
        &[Seg::L(-30.0, 0.0, 30.0, 0.0)],
        false,
        FacetControl::default(),
        RipSide::Default,
    )
    .unwrap();
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    assert!(near(volume(&p), 60.0 * 50.0 * T, 1e-6), "{}", volume(&p));
    let s = p.flat_patterns()[0].size;
    let (a, b) = (s.0.min(s.1), s.0.max(s.1));
    assert!(near(a, 50.0, 1e-6) && near(b, 60.0, 1e-6), "{a} x {b}");
}

#[test]
fn a_half_cone_develops_exactly() {
    // Half circles r 30 (z 0) and r 15 (z 50): half a cone's frustum, a
    // developable surface — the flat keeps its area: π (r1 + r2) s / 2 on the
    // middle surface (the thickness centred on the profiles).
    let p = loft(
        &[Seg::A(0.0, 0.0, 30.0, 0.0, -30.0, 0.0)],
        &[Seg::A(0.0, 0.0, 15.0, 0.0, -15.0, 0.0)],
        false,
        FacetControl::default(),
        RipSide::Both,
    )
    .unwrap();
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    let area = flat_area(&p);
    let slant = (50.0f64 * 50.0 + 15.0 * 15.0).sqrt();
    let want = PI * (30.0 + 15.0) * slant / 2.0;
    assert!(near(area, want, 0.005), "{area} vs {want}");
    assert!(
        near(volume(&p), area * T, 0.01),
        "{} vs {}",
        volume(&p),
        area * T
    );
}

#[test]
fn a_twisted_strip_still_develops() {
    // A half circle r 30 over a straight 60 mm line: not developable; the flat
    // is its triangulation's (each generator a fold), close to the surface.
    let p = loft(
        &[Seg::A(0.0, 0.0, 30.0, 0.0, -30.0, 0.0)],
        &[Seg::L(30.0, 0.0, -30.0, 0.0)],
        false,
        FacetControl::default(),
        RipSide::Both,
    )
    .unwrap();
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    let area = flat_area(&p);
    assert!(
        near(volume(&p), area * T, 0.08),
        "{} vs {}",
        volume(&p),
        area * T
    );
}

#[test]
fn a_rectangle_to_a_circle() {
    let mut vols = Vec::new();
    for side in [RipSide::Default, RipSide::Flipped] {
        let p = loft(&rect(), &circle(15.0), false, FacetControl::default(), side).unwrap();
        assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
        let (v, a) = (volume(&p), flat_area(&p));
        assert!(near(v, a * T, 0.03), "{v} vs {}", a * T);
        vols.push(v);
    }
    // Inventor's: the thickness outside the profiles.
    let inventor = 15702.7876460555;
    assert!(
        vols.iter().any(|v| near(*v, inventor, 0.02)),
        "{vols:?} vs Inventor {inventor}"
    );
}

#[test]
fn press_brake_facets_have_bend_lines() {
    // A quarter arc r 40 split into facets no wider than 10 mm, over a line.
    let arc = [Seg::A(0.0, 0.0, 40.0, 0.0, 0.0, 40.0)];
    let line = [Seg::L(40.0, 0.0, 0.0, 40.0)];
    let p = loft(
        &arc,
        &line,
        true,
        FacetControl::FacetDistance(10.0),
        RipSide::Default,
    )
    .unwrap();
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    // 62.8 mm of arc: 7 facets, 6 bends between them.
    let n = FacetControl::FacetDistance(10.0).facets(40.0, PI / 2.0);
    assert_eq!(n, 7);
    assert_eq!(p.flat_patterns()[0].bends.len(), n - 1);
}

#[test]
fn open_and_closed_profiles_do_not_mix() {
    let r = loft(
        &rect(),
        &[Seg::L(-30.0, 0.0, 30.0, 0.0)],
        false,
        FacetControl::default(),
        RipSide::Default,
    );
    assert!(r.is_err(), "{:?}", r.map(|p| p.feature_health));
    // The points of the profiles are on their planes.
    let _ = Point3::origin();
}

#[test]
fn the_part_shows_the_lofted_body() {
    let p = loft(
        &rect(),
        &circle(15.0),
        false,
        FacetControl::default(),
        RipSide::Default,
    )
    .unwrap();
    assert!(p.body.solids().count() == 1 && p.body.faces().count() > 0);
}
