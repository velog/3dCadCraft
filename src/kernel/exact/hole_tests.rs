//! Hole and thread tools of the exact kernel.
//!
//! Measured while choosing the thread method (debug build, deps optimised):
//! cutting a smooth `sweep_helix` groove (M8, B-spline walls) out of a
//! block took 0.35 s for half a turn inside the material, 2.5 s for half a
//! turn crossing the bore, 10 s for two turns, and over 60 s once it
//! crossed the top face; 8 turns through a 10 mm block did not finish in
//! 15 minutes. Hence `cut_threads`: a faceted cut of the thread zone only.

use super::*;

fn m8_section(bore_r: Real) -> Vec<[Real; 2]> {
    let (d, p) = (8.0, 1.25);
    let slope = (std::f64::consts::PI / 6.0).tan();
    let r0 = bore_r - 0.1 * p;
    let w0 = (p / 8.0 + 2.0 * (d / 2.0 - r0) * slope) / 2.0;
    let w1 = p / 16.0;
    vec![[r0, -w0], [d / 2.0, -w1], [d / 2.0, w1], [r0, w0]]
}

#[test]
fn revolved_tool_is_named_and_has_its_volume() {
    // A Ø6 hole 10 deep with a 118° point, 1 above the plane.
    let r: Real = 3.0;
    let tip = r / 59f64.to_radians().tan();
    let t = revolved_tool(
        Point3::new(5.0, 5.0, 10.0),
        Vector3::z(),
        0.2,
        &[[0.0, 1.0], [r, 1.0], [r, -10.0], [0.0, -10.0 - tip]],
        &["Top:1", "Bore:1", "Tip:1", "Axis:1"],
        "Hole1",
    )
    .unwrap();
    let (v, _, faces) = t.measure().unwrap();
    let want = std::f64::consts::PI * r * r * (11.0 + tip / 3.0);
    assert!((v - want).abs() < 1e-3 * want, "{v} vs {want}");
    assert!(faces >= 3, "{faces}");
    let names: Vec<String> = t.names().iter().flatten().cloned().collect();
    for n in ["Hole1:Top:1", "Hole1:Bore:1", "Hole1:Tip:1"] {
        assert!(names.iter().any(|x| x == n), "{n} in {names:?}");
    }
    assert!(
        revolved_tool(
            Point3::origin(),
            Vector3::zeros(),
            0.0,
            &[[0.0, 0.0]; 3],
            &["a"; 3],
            "H"
        )
        .is_err()
    );
}

#[test]
fn smooth_helix_tool_volume() {
    // A screw motion sweeps 2π ∫ r dA per turn (Pappus for helices).
    let sec = m8_section(3.3235);
    let g = helix_tool(
        Point3::origin(),
        Vector3::z(),
        0.0,
        &sec,
        1.25,
        2.0,
        false,
        "Thread1",
        "Thread",
    )
    .unwrap();
    let (v, _, _) = g.measure().unwrap();
    // First moment of the trapezoid about the axis.
    let (r0, w0, r1, w1) = (sec[0][0], -sec[0][1], sec[1][0], -sec[1][1]);
    let n = 2000;
    let mut s = 0.0;
    for i in 0..n {
        let r = r0 + (r1 - r0) * (i as Real + 0.5) / n as Real;
        let w = w0 + (w1 - w0) * (r - r0) / (r1 - r0);
        s += r * 2.0 * w * (r1 - r0) / n as Real;
    }
    let want = 2.0 * std::f64::consts::PI * s * 2.0;
    assert!((v - want).abs() < 3e-3 * want, "{v} vs {want}");
    assert!(
        g.names()
            .iter()
            .all(|n| n.as_deref() == Some("Thread1:Thread"))
    );
}

#[test]
fn cut_threads_facets_only_the_zone() {
    let b = box_named(
        Point3::new(-10.0, -10.0, 0.0),
        Point3::new(10.0, 10.0, 6.0),
        "Box1",
    )
    .unwrap();
    let rb = 4.0 - 5.0 * 1.25 * 3f64.sqrt() / 16.0;
    let bore = revolved_tool(
        Point3::new(0.0, 0.0, 6.0),
        Vector3::z(),
        0.2,
        &[[0.0, 1.0], [rb, 1.0], [rb, -7.0], [0.0, -7.0]],
        &["Top", "Bore", "Bottom", "Axis"],
        "Hole1",
    )
    .unwrap();
    let b = boolean(&b, &bore, ExactOp::Cut).unwrap().unwrap();
    let v0 = b.measure().unwrap().0;
    let p = 1.25;
    let sec = m8_section(rb);
    let g = ThreadGroove {
        origin: Point3::new(0.0, 0.0, 6.0),
        axis: Vector3::z(),
        section: sec.clone(),
        base: -6.0 - p,
        pitch: p,
        turns: (6.0 + 3.0 * p) / p,
        left_hand: false,
        zone: [rb - 0.2 * p, 4.0 + 0.3 * p, -6.0 - 2.0 * p, 3.0 * p],
        prefix: "Hole1".into(),
        name: "Thread:1".into(),
    };
    let pieces = cut_threads(&b, std::slice::from_ref(&g)).unwrap();
    assert_eq!(pieces.len(), 2, "the exact rest and the threaded zone");
    let v: Real = pieces.iter().map(|s| s.measure().unwrap().0).sum();
    assert!(v < v0, "{v} {v0}");
    // The rest keeps its exact faces: the box sides are 4 of its faces,
    // and it has few faces; the zone has many planar facets.
    let rest_faces = pieces[0].measure().unwrap().2;
    assert!(rest_faces < 20, "{rest_faces}");
    assert!(
        pieces[1]
            .names()
            .iter()
            .any(|n| n.as_deref() == Some("Hole1:Thread:1"))
    );
    let brep = to_brep(&pieces).unwrap();
    for (sid, _) in brep.solids() {
        brep.validate_solid(sid).unwrap();
    }
    // A left-handed thread removes the same volume.
    let lh = ThreadGroove {
        left_hand: true,
        ..g
    };
    let pl = cut_threads(&b, &[lh]).unwrap();
    let vl: Real = pl.iter().map(|s| s.measure().unwrap().0).sum();
    assert!((vl - v).abs() < 2e-3 * (v0 - v), "{vl} vs {v}");
}

#[test]
fn transformed_keeps_names() {
    let t = revolved_tool(
        Point3::origin(),
        Vector3::z(),
        0.0,
        &[[0.0, 1.0], [2.0, 1.0], [2.0, -5.0], [0.0, -5.0]],
        &["Top", "Bore", "Bottom", "Axis"],
        "Hole1",
    )
    .unwrap();
    let m = nalgebra::Matrix4::new_translation(&Vector3::new(10.0, 0.0, 0.0));
    let c = moved(&t, &m).unwrap();
    let mm = c.measure_full().unwrap();
    assert!((mm.centroid.x - 10.0).abs() < 1e-6, "{:?}", mm.centroid);
    assert!(c.names().iter().any(|n| n.as_deref() == Some("Hole1:Bore")));
}
