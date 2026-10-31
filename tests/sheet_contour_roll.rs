//! Contour Roll (`docs/spec/5.6-sheet-metal.md` §6.1j): open profiles on the
//! XZ plane rolled about the Z axis (a line of the same sketch). Inventor's
//! rule: the unrolled length is the rolled angle times the neutral radius
//! (Centroid Cylinder: the section's centroid; K-Factor: inner radius +
//! K · T; or the length / radius given). The rolled solid is checked
//! against the annular sector and Pappus' theorem.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use cadcraft3d::kernel::Real;
use cadcraft3d::part::extrude::entity_key;
use cadcraft3d::part::sheet_metal::{
    ContourDirection, RipSide, RollMethod, SheetMetalDefaults, SheetOp, SheetOpFeature,
};
use cadcraft3d::part::{OriginPlane, PartDocument, SketchOptions, SketchPlacement};
use cadcraft3d::sketch::Point2;

const T: Real = 2.0;

fn rule() -> SheetMetalDefaults {
    SheetMetalDefaults {
        thickness: T,
        k_factor: 0.44,
        ..Default::default()
    }
}

/// A part with the chain `pts` and the Z axis (sketch x = X, y = Z) in one
/// sketch; returns it with the sketch id and the axis' key.
fn sketch(pts: &[(Real, Real)]) -> (PartDocument, String, String) {
    let mut p = PartDocument::default();
    p.make_sheet_metal(Some(rule()));
    let o = SketchOptions {
        auto_project_origin: false,
        ..Default::default()
    };
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XZ), &o)
        .unwrap();
    let sk = &mut p.sketches[i].sketch;
    let ids: Vec<_> = pts
        .iter()
        .map(|&(x, y)| sk.add_point(Point2::new(x, y)))
        .collect();
    for w in ids.windows(2) {
        sk.add_line_between(w[0], w[1]).unwrap();
    }
    let (a, b) = (
        sk.add_point(Point2::new(0.0, -10.0)),
        sk.add_point(Point2::new(0.0, 60.0)),
    );
    let axis = entity_key(sk.add_line_between(a, b).unwrap());
    let id = p.sketches[i].id.clone();
    (p, id, axis)
}

fn roll(
    pts: &[(Real, Real)],
    angle: Real,
    direction: RipSide,
    unroll: RollMethod,
) -> Result<PartDocument, String> {
    let (mut p, sketch, axis) = sketch(pts);
    p.add_sheet_op(SheetOpFeature::new(SheetOp::ContourRoll {
        sketch,
        axis,
        angle,
        side: RipSide::Default,
        direction,
        unroll,
        radius: None,
    }))
    .map_err(|e| e.to_string())?;
    Ok(p)
}

fn volume(p: &PartDocument) -> Real {
    p.exact_bodies.0[0].volume_at(1e-4).unwrap()
}

/// The flat's two sides, sorted.
fn flat(p: &PartDocument) -> (Real, Real) {
    let s = p.flat_patterns()[0].size;
    (s.0.min(s.1), s.0.max(s.1))
}

/// The section of `pts` as a Contour Flange 1 mm wide: its volume is the
/// section's area, its flat's other side the developed profile.
fn section(pts: &[(Real, Real)]) -> (Real, Real) {
    let (mut p, sketch, axis) = sketch(pts);
    // The axis is a line of the sketch too: draw the flange without it.
    let i = p.sketches.iter().position(|s| s.id == sketch).unwrap();
    let sk = &mut p.sketches[i].sketch;
    let id = sk
        .entities()
        .map(|(id, _)| id)
        .find(|id| entity_key(*id) == axis)
        .unwrap();
    sk.remove_entity(id).unwrap();
    p.add_sheet_op(SheetOpFeature::new(SheetOp::Contour {
        sketch,
        width: 1.0,
        flip: false,
        direction: ContourDirection::Default,
        width2: 0.0,
        centered: false,
    }))
    .unwrap();
    let s = p.flat_patterns()[0].size;
    (volume(&p), s.0.max(s.1))
}

fn near(a: Real, b: Real, tol: Real) -> bool {
    (a - b).abs() <= tol
}

#[test]
fn one_line_rolls_a_cylinder_sector() {
    let pts = [(50.0, 0.0), (50.0, 40.0)];
    let p = roll(
        &pts,
        FRAC_PI_2,
        RipSide::Default,
        RollMethod::CentroidCylinder,
    )
    .unwrap();
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    let v = volume(&p);
    // The thickness lies on one side of the line: radii 50..52 or 48..50.
    let (out, inn) = (
        FRAC_PI_2 / 2.0 * (52.0f64.powi(2) - 50.0f64.powi(2)) * 40.0,
        FRAC_PI_2 / 2.0 * (50.0f64.powi(2) - 48.0f64.powi(2)) * 40.0,
    );
    assert!(
        near(v, out, 1.0) || near(v, inn, 1.0),
        "{v} vs {out} / {inn}"
    );
    let rc = if near(v, out, 1.0) { 51.0 } else { 49.0 };
    let (a, b) = flat(&p);
    assert!(
        near(a, 40.0, 1e-3) && near(b, FRAC_PI_2 * rc, 1e-3),
        "{a} x {b}"
    );
    assert!(p.flat_patterns()[0].bends.is_empty());
}

#[test]
fn a_full_turn_of_one_line() {
    let pts = [(50.0, 0.0), (50.0, 40.0)];
    let p = roll(&pts, TAU, RipSide::Default, RollMethod::CentroidCylinder).unwrap();
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    let v = volume(&p);
    let rc: Real = if near(v, PI * (52.0f64.powi(2) - 2500.0) * 40.0, 2.0) {
        51.0
    } else {
        49.0
    };
    assert!(
        near(
            v,
            PI * ((rc + 1.0).powi(2) - (rc - 1.0).powi(2)) * 40.0,
            2.0
        ),
        "{v}"
    );
    let (_, b) = flat(&p);
    assert!(near(b, TAU * rc, 1e-3), "{b}");
    // A profile of several lines cannot go all the way round.
    let l = [(30.0, 0.0), (50.0, 0.0), (50.0, 40.0)];
    assert!(roll(&l, TAU, RipSide::Default, RollMethod::CentroidCylinder).is_err());
}

#[test]
fn an_l_profile_follows_pappus() {
    let pts = [(30.0, 0.0), (50.0, 0.0), (50.0, 40.0)];
    let angle = 2.0 * PI / 3.0;
    let p = roll(&pts, angle, RipSide::Default, RollMethod::CentroidCylinder).unwrap();
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    let (area, developed) = section(&pts);
    // Pappus: V = angle · r_c · A, and the unrolled length is angle · r_c = V / A.
    let v = volume(&p);
    let (a, b) = flat(&p);
    let along = v / area;
    let (dev, roll_len) = if near(a, developed, 1e-3) {
        (a, b)
    } else {
        (b, a)
    };
    assert!(
        near(dev, developed, 1e-3),
        "{a} x {b}, developed {developed}"
    );
    assert!(near(roll_len, along, 0.05), "{roll_len} vs {along}");
    assert_eq!(p.flat_patterns()[0].bends.len(), 1);
}

#[test]
fn unroll_methods() {
    let pts = [(50.0, 0.0), (50.0, 40.0)];
    let len = |m: RollMethod| {
        let p = roll(&pts, FRAC_PI_2, RipSide::Default, m).unwrap();
        assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
        flat(&p).1.max(40.0 + 1e-9).min(flat(&p).1)
    };
    assert!(near(
        len(RollMethod::DevelopedLength { length: 77.0 }),
        77.0,
        1e-3
    ));
    assert!(near(
        len(RollMethod::NeutralRadius { radius: 60.0 }),
        FRAC_PI_2 * 60.0,
        1e-3
    ));
    let p = roll(
        &pts,
        FRAC_PI_2,
        RipSide::Default,
        RollMethod::CentroidCylinder,
    )
    .unwrap();
    let inner = if near(flat(&p).1, FRAC_PI_2 * 51.0, 1e-3) {
        50.0
    } else {
        48.0
    };
    assert!(near(
        len(RollMethod::KFactor { k: 0.3 }),
        FRAC_PI_2 * (inner + 0.3 * T),
        1e-3
    ));
}

#[test]
fn directions_and_refusals() {
    let pts = [(50.0, 0.0), (50.0, 40.0)];
    // Both: half the angle each side of the sketch plane (y = 0).
    let p = roll(&pts, FRAC_PI_2, RipSide::Both, RollMethod::CentroidCylinder).unwrap();
    let (lo, hi) = p.body.bounding_box().unwrap();
    assert!(near(lo.y, -hi.y, 1e-3), "{lo:?} {hi:?}");
    let d = roll(
        &pts,
        FRAC_PI_2,
        RipSide::Default,
        RollMethod::CentroidCylinder,
    )
    .unwrap();
    let f = roll(
        &pts,
        FRAC_PI_2,
        RipSide::Flipped,
        RollMethod::CentroidCylinder,
    )
    .unwrap();
    let (dl, dh) = d.body.bounding_box().unwrap();
    let (fl, fh) = f.body.bounding_box().unwrap();
    assert!(
        near(dl.y, -fh.y, 1e-3) && near(dh.y, -fl.y, 1e-3),
        "{dl:?}{dh:?} / {fl:?}{fh:?}"
    );
    // A profile across the axis.
    assert!(
        roll(
            &[(-10.0, 0.0), (10.0, 0.0)],
            FRAC_PI_2,
            RipSide::Default,
            RollMethod::CentroidCylinder
        )
        .is_err()
    );
    // No further sheet feature on a roll; its flat pattern works.
    let body = d.sheet_models.keys().next().unwrap().clone();
    let mut q = d.clone();
    assert!(
        q.add_sheet_op(SheetOpFeature::new(SheetOp::unfold(body)))
            .is_err()
    );
}
