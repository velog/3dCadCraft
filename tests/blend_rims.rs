//! Blends of whole circles: by editing the faces when the band they take of the face is clear,
//! through the general path when it is not. Volumes against Pappus.
mod common;
use common::*;

use cadcraft3d::part::blend::BlendFeature;
use std::f64::consts::PI;

/// A 200 x 200 x 10 plate with `n` holes of radius `r`, `pitch` apart along X, and its volume.
fn drilled(n: usize, r: f64, pitch: f64) -> (cadcraft3d::part::PartDocument, f64) {
    let mut p = plate(200.0, 200.0, 10.0);
    let t = top(&p, 10.0);
    let j = sketch_on(&mut p, &t, |sk, at| {
        for k in 0..n {
            sk.add_circle(at(20.0 + pitch * k as f64, 100.0, 10.0), r)
                .unwrap();
        }
    });
    cut(&mut p, j, None, |x| {
        x.holes.is_empty() && x.area() < PI * r * r * 1.5
    });
    let v = fine(&p);
    (p, v)
}

/// The volume on a mesh of 0.001 mm: the measure's own tolerance follows the plate's size, 0.014 mm
/// here, too coarse for a round of 1 mm (0.6 % of a ring).
fn fine(p: &cadcraft3d::part::PartDocument) -> f64 {
    p.exact_bodies.0[0].volume_at(1e-3).unwrap()
}

fn rims(p: &cadcraft3d::part::PartDocument) -> Vec<cadcraft3d::part::blend::EdgeRef> {
    edges_where(p, |h| {
        (h[2] - 10.0).abs() < 1e-6 && h[0] > 1.0 && h[0] < 199.0 && h[1] > 1.0 && h[1] < 199.0
    })
}

#[test]
fn a_chamfer_of_hole_rims_takes_rings_of_a_right_triangle() {
    let (r, d) = (4.0, 1.0);
    let (mut p, before) = drilled(6, r, 30.0);
    p.add_blend(BlendFeature::chamfer(rims(&p), d)).unwrap();
    assert!(
        p.exact && p.feature_health.is_empty(),
        "{:?}",
        p.feature_health
    );
    let want = 6.0 * 2.0 * PI * (r + d / 3.0) * d * d / 2.0;
    let got = before - fine(&p);
    assert!((got - want).abs() < 2e-3 * want, "{got} vs {want}");
}

#[test]
fn a_fillet_of_hole_rims_takes_rings_of_a_round_corner() {
    let (r, f) = (4.0, 1.0);
    let (mut p, before) = drilled(6, r, 30.0);
    p.add_blend(BlendFeature::fillet(rims(&p), f)).unwrap();
    assert!(
        p.exact && p.feature_health.is_empty(),
        "{:?}",
        p.feature_health
    );
    // The corner between a square f x f and its quarter circle: its area and its centroid's
    // distance from the wall.
    let area = (1.0 - PI / 4.0) * f * f;
    let x = f * (10.0 - 3.0 * PI) / (12.0 - 3.0 * PI);
    let want = 6.0 * 2.0 * PI * (r + x) * area;
    let got = before - fine(&p);
    assert!((got - want).abs() < 2e-3 * want, "{got} vs {want}");
}

/// A chamfer wider than the wall between two holes (pitch 10, radius 4: 2 mm of wall) must not be
/// built by the face edit, whose rings would cross: whatever the general path does (a refusal or
/// a part that lost what the bevels overlap), never more than the two whole rings.
#[test]
fn a_chamfer_reaching_the_next_hole_does_not_take_the_face_edit() {
    let (r, d) = (4.0, 1.5);
    let (mut p, before) = drilled(2, r, 10.0);
    match p.add_blend(BlendFeature::chamfer(rims(&p), d)) {
        Err(_) => assert!(
            (fine(&p) - before).abs() < 1e-6,
            "a refusal leaves the part as it was"
        ),
        Ok(_) if !p.feature_health.is_empty() => {}
        Ok(_) => {
            let got = before - fine(&p);
            let rings = 2.0 * 2.0 * PI * (r + d / 3.0) * d * d / 2.0;
            assert!(
                got > 0.0 && got <= rings * 1.001,
                "{got} vs at most {rings}"
            );
        }
    }
}

/// A chamfer past the plate's edge: the rim stands 2 mm from it.
#[test]
fn a_chamfer_reaching_the_outline_does_not_take_the_face_edit() {
    let (mut p, before) = {
        let mut p = plate(200.0, 200.0, 10.0);
        let t = top(&p, 10.0);
        let j = sketch_on(&mut p, &t, |sk, at| {
            sk.add_circle(at(6.0, 100.0, 10.0), 4.0).unwrap();
        });
        cut(&mut p, j, None, |x| x.holes.is_empty() && x.area() < 80.0);
        let v = volume(&p);
        (p, v)
    };
    let e = edges_where(&p, |h| {
        (h[2] - 10.0).abs() < 1e-6 && h[0] > 1.0 && h[0] < 199.0 && h[1] > 1.0 && h[1] < 199.0
    });
    match p.add_blend(BlendFeature::chamfer(e, 3.0)) {
        Err(_) => assert!((volume(&p) - before).abs() < 1e-6),
        Ok(_) if !p.feature_health.is_empty() => {}
        Ok(_) => {
            let got = before - volume(&p);
            let ring = 2.0 * PI * (4.0 + 1.0) * 9.0 / 2.0;
            assert!(got > 0.0 && got <= ring * 1.001, "{got} vs at most {ring}");
        }
    }
}
