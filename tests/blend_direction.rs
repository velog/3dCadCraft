//! Fillets and chamfers go the right way: a convex edge loses material, a
//! concave one gains it, by the amount the section says, and nothing grows
//! past the part's box. Each case prints its time (`--nocapture`).

mod common;
use common::*;

use cadcraft3d::part::blend::{BlendFeature, BlendKind, ChamferMethod, EdgeRef};
use cadcraft3d::part::frame::{Loop, draw_loop};
use std::time::Instant;

/// Area of a fillet's section and how far its centroid lies from the corner
/// (along each face), radius `r`, square corner.
fn fillet_section(r: f64) -> (f64, f64) {
    let area = r * r * (1.0 - PI / 4.0);
    (area, r * (10.0 - 3.0 * PI) / (12.0 - 3.0 * PI))
}

/// What one case makes: a base part, the edges, and the signed change of volume
/// for a chamfer of `d` and a fillet of `f`.
struct Case {
    name: &'static str,
    base: PartDocument,
    edges: Vec<EdgeRef>,
    chamfer: f64,
    fillet: f64,
    /// Relative tolerance (corners at an edge's end are not in the formula).
    tol: f64,
}

/// `corners` (counter-clockwise on XY) extruded by `h` along +Z.
fn prism(corners: &[(f64, f64)], h: f64) -> PartDocument {
    let mut p = PartDocument::default();
    let o = SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    };
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &o)
        .unwrap();
    let path = corners
        .iter()
        .map(|&(x, y)| (Point2::new(x, y), None))
        .collect();
    draw_loop(&mut p.sketches[i].sketch, Loop::Path(path)).unwrap();
    let sk = &p.sketches[i].sketch;
    let flip = sk.plane.normal.z < 0.0;
    let regions = sk.regions();
    let r = regions.first().unwrap();
    let mut e = ExtrudeFeature::new(vec![ProfileRef::of(r, r.interior_point())], h);
    if flip {
        e.direction = ExtentDirection::Flipped;
    }
    p.add_extrude(i, e).unwrap();
    p
}

fn on(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

fn cases(d: f64, f: f64) -> Vec<Case> {
    let (fa, fc) = fillet_section(f);
    let ca = d * d / 2.0;
    let mut out = Vec::new();

    // A plain outer edge of a box.
    let p = plate(40.0, 30.0, 10.0);
    let e = edges_where(&p, |h| on(h[1], 0.0) && on(h[2], 10.0));
    out.push(Case {
        name: "box edge (convex line)",
        base: p,
        edges: e,
        chamfer: -ca * 40.0,
        fillet: -fa * 40.0,
        tol: 1e-3,
    });

    // The inner corner of an L, along Z.
    let p = prism(
        &[
            (0.0, 0.0),
            (40.0, 0.0),
            (40.0, 10.0),
            (20.0, 10.0),
            (20.0, 20.0),
            (0.0, 20.0),
        ],
        30.0,
    );
    let e = edges_where(&p, |h| on(h[0], 20.0) && on(h[1], 10.0));
    out.push(Case {
        name: "L inner edge (concave line)",
        base: p,
        edges: e,
        chamfer: ca * 30.0,
        fillet: fa * 30.0,
        tol: 1e-3,
    });

    // A boss: its top rim (convex circle) and its foot (concave circle).
    let r0: f64 = 5.0;
    let boss_part = || {
        let mut p = plate(40.0, 30.0, 10.0);
        let t = top(&p, 10.0);
        let j = sketch_on(&mut p, &t, |sk, at| {
            sk.add_circle(at(20.0, 15.0, 10.0), r0).unwrap();
        });
        boss(&mut p, j, 8.0, |r| r.holes.is_empty() && r.area() < 100.0);
        p
    };
    let p = boss_part();
    let e = edges_where(&p, |h| on(h[2], 18.0));
    out.push(Case {
        name: "boss top rim (convex circle)",
        base: p,
        edges: e,
        chamfer: -2.0 * PI * (r0 - d / 3.0) * ca,
        fillet: -2.0 * PI * (r0 - fc) * fa,
        tol: 2e-3,
    });
    let p = boss_part();
    let e = edges_where(&p, |h| {
        on(h[2], 10.0) && (h[0] - 20.0).hypot(h[1] - 15.0) < r0 + 1e-3
    });
    out.push(Case {
        name: "boss foot (concave circle)",
        base: p,
        edges: e,
        chamfer: 2.0 * PI * (r0 + d / 3.0) * ca,
        fillet: 2.0 * PI * (r0 + fc) * fa,
        tol: 2e-3,
    });

    // A through hole's rim.
    let rh: f64 = 4.0;
    let mut p = plate(40.0, 30.0, 10.0);
    let t = top(&p, 10.0);
    let j = sketch_on(&mut p, &t, |sk, at| {
        sk.add_circle(at(20.0, 15.0, 10.0), rh).unwrap();
    });
    cut(&mut p, j, None, |r| r.holes.is_empty() && r.area() < 60.0);
    let e = edges_where(&p, |h| {
        on(h[2], 10.0) && (h[0] - 20.0).hypot(h[1] - 15.0) < rh + 1e-3
    });
    out.push(Case {
        name: "hole rim (convex circle)",
        base: p,
        edges: e,
        chamfer: -2.0 * PI * (rh + d / 3.0) * ca,
        fillet: -2.0 * PI * (rh + fc) * fa,
        tol: 2e-3,
    });

    // A pocket: an edge of its opening, and one of its floor.
    let pocket = || {
        let mut p = plate(40.0, 30.0, 10.0);
        let t = top(&p, 10.0);
        let j = sketch_on(&mut p, &t, |sk, at| {
            sk.add_rectangle_two_point(at(10.0, 5.0, 10.0), at(30.0, 25.0, 10.0))
                .unwrap();
        });
        cut(&mut p, j, Some(5.0), |r| {
            r.holes.is_empty() && r.area() < 500.0
        });
        p
    };
    let p = pocket();
    let e = edges_where(&p, |h| {
        on(h[1], 5.0) && on(h[2], 10.0) && h[0] > 9.0 && h[0] < 31.0
    });
    out.push(Case {
        name: "pocket opening edge (convex line)",
        base: p,
        edges: e,
        chamfer: -ca * 20.0,
        fillet: -fa * 20.0,
        tol: 0.05,
    });
    let p = pocket();
    let e = edges_where(&p, |h| on(h[1], 5.0) && on(h[2], 5.0));
    out.push(Case {
        name: "pocket floor edge (concave line)",
        base: p,
        edges: e,
        chamfer: ca * 20.0,
        fillet: fa * 20.0,
        tol: 0.05,
    });

    // A hole breaking through the plate's side: the rim is an arc running off
    // the top face at both ends.
    let mut p = plate(40.0, 30.0, 10.0);
    let t = top(&p, 10.0);
    let j = sketch_on(&mut p, &t, |sk, at| {
        sk.add_circle(at(0.0, 15.0, 10.0), rh).unwrap();
    });
    cut(&mut p, j, None, |r| r.holes.is_empty() && r.area() < 60.0);
    let e = edges_where(&p, |h| {
        on(h[2], 10.0) && h[0] > 1e-3 && h[0].hypot(h[1] - 15.0) < rh + 1e-3
    });
    out.push(Case {
        name: "arc running off a face (convex)",
        base: p,
        edges: e,
        chamfer: -PI * (rh + d / 3.0) * ca,
        fillet: -PI * (rh + fc) * fa,
        tol: 0.05,
    });

    // A wall at an angle: a triangle's acute corner along Z, and a chevron's
    // re-entrant corner. Equal setbacks cut a triangle of area d² sin(θ) / 2.
    let th = (20f64).atan2(30.0);
    let p = prism(&[(0.0, 0.0), (30.0, 0.0), (0.0, 20.0)], 10.0);
    let e = edges_where(&p, |h| on(h[0], 30.0) && on(h[1], 0.0));
    out.push(Case {
        name: "acute corner along Z (convex, not square)",
        base: p,
        edges: e,
        chamfer: -d * d * th.sin() / 2.0 * 10.0,
        fillet: f64::NAN,
        tol: 5e-3,
    });
    let open = 2.0 * (15f64).atan2(10.0);
    let p = prism(
        &[
            (0.0, 0.0),
            (30.0, 0.0),
            (30.0, 20.0),
            (15.0, 10.0),
            (0.0, 20.0),
        ],
        10.0,
    );
    let e = edges_where(&p, |h| on(h[0], 15.0) && on(h[1], 10.0));
    out.push(Case {
        name: "chevron corner along Z (concave, not square)",
        base: p,
        edges: e,
        chamfer: d * d * open.sin() / 2.0 * 10.0,
        fillet: f64::NAN,
        tol: 5e-3,
    });
    out
}

/// Builds the blend on a copy; `Err` when it is refused.
fn apply(c: &Case, bl: BlendFeature) -> Result<(PartDocument, f64), String> {
    let mut p = c.base.clone();
    p.build_cache = Default::default();
    let t = Instant::now();
    p.add_blend(bl).map_err(|e| e.to_string())?;
    let ms = t.elapsed().as_secs_f64() * 1e3;
    if !p.exact {
        return Err("fell back to the faceted kernel".into());
    }
    if let Some(h) = p.feature_health.values().next() {
        return Err(h.clone());
    }
    Ok((p, ms))
}

/// The change of volume is the expected one, the box did not grow, the solid is valid.
fn judge(c: &Case, what: &str, p: &PartDocument, expected: f64, problems: &mut Vec<String>) -> f64 {
    let (v0, v1) = (volume(&c.base), volume(p));
    let change = v1 - v0;
    if change * expected < 0.0 {
        problems.push(format!(
            "{}, {what}: WRONG WAY, volume changed by {change:.3}, expected {expected:.3}",
            c.name
        ));
    } else if (change - expected).abs() > expected.abs() * c.tol.max(0.03) {
        problems.push(format!(
            "{}, {what}: changed by {change:.3}, expected {expected:.3}",
            c.name
        ));
    }
    let (a, b) = (
        c.base.body.bounding_box().unwrap(),
        p.body.bounding_box().unwrap(),
    );
    for k in 0..3 {
        if b.0[k] < a.0[k] - 1e-6 || b.1[k] > a.1[k] + 1e-6 {
            problems.push(format!(
                "{}, {what}: material outside the part's box: {b:?}",
                c.name
            ));
            break;
        }
    }
    for (id, _) in p.body.solids() {
        if let Err(e) = p.body.validate_solid(id) {
            problems.push(format!("{}, {what}: invalid solid: {e:?}", c.name));
        }
    }
    change
}

#[test]
fn blends_go_the_right_way() {
    let (d, f) = (1.0, 1.0);
    let mut problems = Vec::new();
    println!(
        "{:<46} {:>10} {:>9} {:>10} {:>9}",
        "case", "chamfer", "ms", "fillet", "ms"
    );
    for c in cases(d, f) {
        assert!(!c.edges.is_empty(), "{}: no edge found", c.name);
        let mut row = format!("{:<46}", c.name);
        match apply(&c, BlendFeature::chamfer(c.edges.clone(), d)) {
            Ok((p, ms)) => {
                let dv = judge(&c, "chamfer", &p, c.chamfer, &mut problems);
                row += &format!(" {dv:>10.3} {ms:>9.1}");
            }
            Err(e) => {
                row += &format!(" {:>10} {:>9}", "refused", "-");
                problems.push(format!("{}, chamfer refused: {e}", c.name));
            }
        }
        if c.fillet.is_nan() {
            row += "  (no formula)";
        } else {
            match apply(&c, BlendFeature::fillet(c.edges.clone(), f)) {
                Ok((p, ms)) => {
                    let dv = judge(&c, "fillet", &p, c.fillet, &mut problems);
                    row += &format!(" {dv:>10.3} {ms:>9.1}");
                }
                Err(e) => {
                    row += &format!(" {:>10} {:>9}", "refused", "-");
                    problems.push(format!("{}, fillet refused: {e}", c.name));
                }
            }
        }
        println!("{row}");
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Two Distances: flip swaps which face takes which distance, and Distance and
/// Angle removes the triangle it says.
#[test]
fn chamfer_options_go_the_right_way() {
    let top_area = |p: &PartDocument| -> f64 {
        p.body
            .faces()
            .filter_map(|(_, x)| x.mesh.as_ref())
            .map(|m| {
                m.triangles
                    .iter()
                    .map(|t| t.map(|i| m.positions[i as usize]))
                    .filter(|t| t.iter().all(|q| (q.z - 10.0).abs() < 1e-9))
                    .map(|[a, b, c]| (b - a).cross(&(c - a)).norm() / 2.0)
                    .sum::<f64>()
            })
            .sum()
    };
    let base = plate(40.0, 30.0, 10.0);
    let e = edges_where(&base, |h| on(h[1], 0.0) && on(h[2], 10.0));
    let two = |flip: bool| {
        let mut bl = BlendFeature::chamfer(e.clone(), 2.0);
        bl.kind = BlendKind::Chamfer {
            method: ChamferMethod::TwoDistances,
            distance: 2.0,
            distance2: 1.0,
            angle: PI / 4.0,
            flip,
        };
        let mut p = base.clone();
        p.add_blend(bl).unwrap();
        assert!(p.exact && p.feature_health.is_empty());
        (volume(&p), top_area(&p))
    };
    let ((v0, a0), (v1, a1)) = (two(false), two(true));
    let whole = 12_000.0;
    assert!(
        (whole - v0 - 40.0).abs() < 0.1,
        "a triangle 2 x 1 x 40: {v0}"
    );
    assert!(
        (whole - v1 - 40.0).abs() < 0.1,
        "a triangle 2 x 1 x 40: {v1}"
    );
    // The top face loses 40 x 2 one way and 40 x 1 the other.
    let lost = |a: f64| 1200.0 - a;
    println!(
        "top face loses {:.2} and, flipped, {:.2}",
        lost(a0),
        lost(a1)
    );
    assert!(
        (lost(a0) + lost(a1) - 120.0).abs() < 0.1 && (lost(a0) - lost(a1)).abs() > 30.0,
        "flip must swap the distances: {} {}",
        lost(a0),
        lost(a1)
    );

    let mut bl = BlendFeature::chamfer(e.clone(), 2.0);
    let angle = 30f64.to_radians();
    bl.kind = BlendKind::Chamfer {
        method: ChamferMethod::DistanceAngle,
        distance: 2.0,
        distance2: 2.0,
        angle,
        flip: false,
    };
    let mut p = base.clone();
    p.add_blend(bl).unwrap();
    let removed = whole - volume(&p);
    let triangle = 2.0 * 2.0 * angle.tan() / 2.0 * 40.0;
    println!("distance and angle removes {removed:.3}, a triangle is {triangle:.3}");
    assert!((removed - triangle).abs() < 0.05, "{removed} vs {triangle}");
}
