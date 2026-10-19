//! Shell 2027 options (`docs/spec/2.6-shell.md`): Center (saved as
//! `Both`) and Rounded Corners. Volumes are analytic.

mod common;

use cadcraft3d::part::ShellCorners;
use common::*;

fn shelled(
    mut p: PartDocument,
    open: &[(String, [Real; 3])],
    t: Real,
    dir: ShellDirection,
    corners: ShellCorners,
) -> Result<PartDocument, String> {
    let mut def = ShellFeature::new(t);
    def.direction = dir;
    def.corners = corners;
    for (n, at) in open {
        def.remove.push(face_ref(&p, n, *at));
    }
    p.add_shell(def).map_err(|e| e.to_string())?;
    Ok(p)
}

/// The box's outward offset by `r` below its top plane (a true offset:
/// slabs, quarter cylinders along the bottom and vertical edges, sphere
/// octants at the bottom corners).
fn outer_rounded(a: Real, b: Real, c: Real, r: Real) -> Real {
    a * b * c
        + r * (a * b + 2.0 * (a + b) * c)
        + PI * r * r / 4.0 * (2.0 * (a + b) + 4.0 * c)
        + 2.0 / 3.0 * PI * r * r * r
}

#[test]
fn rounded_corners_outside() {
    for (t, w, d, h) in [
        (2.0, 40.0, 30.0, 20.0),
        (1.0, 20.0, 20.0, 10.0),
        (3.0, 30.0, 25.0, 15.0),
    ] {
        let p = plate(w, d, h);
        let top = top(&p, h);
        let s = shelled(
            p,
            &[(top, [w / 2.0, d / 2.0, h])],
            t,
            ShellDirection::Outside,
            ShellCorners::Rounded,
        )
        .unwrap();
        check(&s, outer_rounded(w, d, h, t) - w * d * h, 1e-3);
    }
}

/// Inside: the cavity's concave corner (the L's inner corner) is rounded
/// by the thickness, as a true inward offset (adds 1 − π/4 per t² of
/// plan area to the cavity).
#[test]
fn rounded_corners_inside_l_prism() {
    let mut p = PartDocument::default();
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    {
        let sk = &mut p.sketches[i].sketch;
        let pts = [
            (0.0, 0.0),
            (30.0, 0.0),
            (30.0, 10.0),
            (10.0, 10.0),
            (10.0, 30.0),
            (0.0, 30.0),
        ];
        let ids: Vec<_> = pts
            .iter()
            .map(|(x, y)| sk.add_point(Point2::new(*x, *y)))
            .collect();
        for j in 0..ids.len() {
            sk.add_line_between(ids[j], ids[(j + 1) % ids.len()])
                .unwrap();
        }
    }
    let r = p.sketches[i].sketch.regions();
    p.add_extrude(i, ExtrudeFeature::new(region_profiles(&r, |_| true), 10.0))
        .unwrap();
    let t = top(&p, 10.0);
    let sharp = shelled(
        p.clone(),
        &[(t.clone(), [5.0, 5.0, 10.0])],
        1.0,
        ShellDirection::Inside,
        ShellCorners::Sharp,
    )
    .unwrap();
    check(&sharp, 5000.0 - 9.0 * 384.0, 1e-6);
    let round = shelled(
        p,
        &[(t, [5.0, 5.0, 10.0])],
        1.0,
        ShellDirection::Inside,
        ShellCorners::Rounded,
    )
    .unwrap();
    check(&round, 5000.0 - 9.0 * (385.0 - PI / 4.0), 1e-4);
}

/// A box has no concave edge: Inside Rounded equals Sharp.
#[test]
fn rounded_corners_inside_box_is_sharp() {
    let p = plate(40.0, 30.0, 20.0);
    let t = top(&p, 20.0);
    let s = shelled(
        p,
        &[(t, [20.0, 15.0, 20.0])],
        2.0,
        ShellDirection::Inside,
        ShellCorners::Rounded,
    )
    .unwrap();
    check(&s, 24_000.0 - 36.0 * 26.0 * 18.0, 1e-6);
}

/// Center: half out (rounded by t/2), half in.
#[test]
fn rounded_corners_center() {
    let p = plate(40.0, 30.0, 20.0);
    let t = top(&p, 20.0);
    let s = shelled(
        p,
        &[(t, [20.0, 15.0, 20.0])],
        2.0,
        ShellDirection::Center,
        ShellCorners::Rounded,
    )
    .unwrap();
    check(
        &s,
        outer_rounded(40.0, 30.0, 20.0, 1.0) - 38.0 * 28.0 * 19.0,
        1e-3,
    );
}

/// Center is saved as `Both` (older builds read it); both names load.
#[test]
fn center_keeps_the_old_name_in_files() {
    let mut s = ShellFeature::new(2.0);
    s.direction = ShellDirection::Center;
    let j = serde_json::to_string(&s).unwrap();
    assert!(j.contains("\"Both\""), "{j}");
    let back: ShellFeature = serde_json::from_str(&j).unwrap();
    assert_eq!(back.direction, ShellDirection::Center);
    let alias: ShellFeature = serde_json::from_str(&j.replace("\"Both\"", "\"Center\"")).unwrap();
    assert_eq!(alias.direction, ShellDirection::Center);
    // Sharp is not written (old files stay as they were).
    assert!(!j.contains("corners"));
}
