//! Shell 2027 options (`docs/spec/2.6-shell.md`): Center (saved as
//! `Both`) and Rounded Corners. Volumes are analytic.

mod common;

use cadcraft3d::part::ShellCorners;
use cadcraft3d::part::blend::{BlendFeature, EdgeRef, edge_ref, tangent_chain};
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

/// A box rounded along its bottom and upright edges (radius `r`): the plain block less the corners
/// the rounds take, the bottom's along its straight parts and swept round each upright (Pappus).
fn rounded_pan(a: Real, b: Real, h: Real, r: Real) -> Real {
    let corner = r * r * (1.0 - PI / 4.0);
    let inset = r * (10.0 - 3.0 * PI) / (3.0 * (4.0 - PI));
    h * (a * b - (4.0 - PI) * r * r)
        - corner * 2.0 * (a + b - 4.0 * r)
        - corner * 2.0 * PI * (r - inset)
}

fn pan(a: Real, b: Real, h: Real, r: Real) -> PartDocument {
    let mut p = PartDocument::with_box("Box1", [a, b, h]).unwrap();
    let f = |x: &str, y: &str, hint: [Real; 3]| EdgeRef {
        faces: [format!("Box1:{x}"), format!("Box1:{y}")],
        hint,
    };
    p.add_blend(BlendFeature::fillet(
        vec![
            f("Front", "Right", [a, 0.0, h / 2.0]),
            f("Right", "Back", [a, b, h / 2.0]),
            f("Back", "Left", [0.0, b, h / 2.0]),
            f("Left", "Front", [0.0, 0.0, h / 2.0]),
            f("Bottom", "Front", [a / 2.0, 0.0, 0.0]),
            f("Bottom", "Right", [a, b / 2.0, 0.0]),
            f("Bottom", "Back", [a / 2.0, b, 0.0]),
            f("Bottom", "Left", [0.0, b / 2.0, 0.0]),
        ],
        r,
    ))
    .unwrap();
    p
}

/// A rounded box opened at the top: its walls and floor follow the rounds, smaller by the wall.
#[test]
fn a_rounded_box_lined_at_its_own_offset() {
    let (a, b, h, r, t) = (40.0, 30.0, 20.0, 5.0, 1.5);
    let top = vec![("Box1:Top".to_owned(), [a / 2.0, b / 2.0, h])];
    let p = shelled(
        pan(a, b, h, r),
        &top,
        t,
        ShellDirection::Inside,
        ShellCorners::Sharp,
    )
    .unwrap();
    assert!(
        p.exact && p.feature_health.is_empty(),
        "{:?}",
        p.feature_health
    );
    let want = rounded_pan(a, b, h, r) - rounded_pan(a - 2.0 * t, b - 2.0 * t, h - t, r - t);
    let got = p.exact_bodies.0[0].volume_at(1e-3).unwrap();
    assert!((got - want).abs() < 1e-3 * want, "{got} vs {want}");
    // Every face named: the outer ones as they were, the inner ones after what they line.
    let names = p.body.faces().filter(|(_, f)| f.name.is_some()).count();
    assert_eq!(names, p.body.faces().count());
}

/// The same box with its top rim chamfered: the cavity's chamfer stands 2 + 1.5 sqrt(2) in from the
/// outer wall. The Cut pushed the opening out past the point where that chamfer's corners close, and
/// took 2.5 % too much (Inventor: 5070.02 mm3).
#[test]
fn a_chamfered_rim_keeps_its_wall_all_the_way_up() {
    let (a, b, h, r, t, c) = (40.0, 30.0, 20.0, 5.0, 1.5, 2.0);
    let mut p = pan(a, b, h, r);
    let top = p
        .body
        .edges()
        .find(|(e, _)| {
            edge_ref(&p.body, *e).is_some_and(|x| {
                x.faces.contains(&"Box1:Top".to_owned())
                    && x.faces.contains(&"Box1:Front".to_owned())
            })
        })
        .map(|(e, _)| e)
        .unwrap();
    p.add_blend(BlendFeature::chamfer(tangent_chain(&p.body, top), c))
        .unwrap();
    let open = vec![("Box1:Top".to_owned(), [a / 2.0, b / 2.0, h])];
    let p = shelled(p, &open, t, ShellDirection::Inside, ShellCorners::Sharp).unwrap();
    assert!(
        p.exact && p.feature_health.is_empty(),
        "{:?}",
        p.feature_health
    );
    let ring = |a: Real, b: Real, r: Real, c: Real| {
        c * c / 2.0 * (2.0 * (a + b - 4.0 * r) + 2.0 * PI * (r - c / 3.0))
    };
    let inner = c + t * 2.0_f64.sqrt() - t;
    let want = rounded_pan(a, b, h, r)
        - ring(a, b, r, c)
        - (rounded_pan(a - 2.0 * t, b - 2.0 * t, h - t, r - t)
            - ring(a - 2.0 * t, b - 2.0 * t, r - t, inner));
    let got = p.exact_bodies.0[0].volume_at(1e-3).unwrap();
    assert!((got - want).abs() < 1e-3 * want, "{got} vs {want}");
    assert!((got - 5070.02).abs() < 1.0, "{got} vs Inventor's 5070.02");
}

/// A wall as thick as the round: the offset has no round left, and the Cut still decides.
#[test]
fn a_wall_past_the_round_does_not_line_by_offset() {
    let (a, b, h, r) = (40.0, 30.0, 20.0, 2.0);
    let top = vec![("Box1:Top".to_owned(), [a / 2.0, b / 2.0, h])];
    let t = 3.0;
    let p = shelled(
        pan(a, b, h, r),
        &top,
        t,
        ShellDirection::Inside,
        ShellCorners::Sharp,
    );
    if let Ok(p) = p
        && p.feature_health.is_empty()
    {
        // The cavity is a sharp block: what the outer rounds took comes off the walls only.
        let want = rounded_pan(a, b, h, r) - (a - 2.0 * t) * (b - 2.0 * t) * (h - t);
        let got = p.exact_bodies.0[0].volume_at(1e-3).unwrap();
        assert!((got - want).abs() < 1e-3 * want, "{got} vs {want}");
    }
}
