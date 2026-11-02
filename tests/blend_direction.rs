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

/// Where a blend's time goes, route by route (`--ignored --nocapture`).
#[test]
#[ignore = "diagnostic"]
fn blend_routes_with_times() {
    let time = |name: &str, base: &PartDocument, bl: BlendFeature| {
        let mut v: Vec<f64> = (0..5)
            .map(|_| {
                let mut p = base.clone();
                p.build_cache = Default::default();
                let t = Instant::now();
                let r = p.add_blend(bl.clone());
                let ms = t.elapsed().as_secs_f64() * 1e3;
                assert!(r.is_ok(), "{name}: {r:?}");
                assert!(
                    p.feature_health.is_empty(),
                    "{name}: {:?}",
                    p.feature_health
                );
                ms
            })
            .collect();
        v.sort_by(f64::total_cmp);
        println!("{name:<52} {:>8.1} ms", v[2]);
    };
    // 8 edges of a box: the top's four and the four uprights.
    let b = plate(100.0, 60.0, 30.0);
    let eight = edges_where(&b, |h| on(h[2], 30.0) || (h[2] > 1.0 && h[2] < 29.0));
    assert_eq!(eight.len(), 8);
    time(
        "fillet r5, 8 edges of a box (kernel)",
        &b,
        BlendFeature::fillet(eight.clone(), 5.0),
    );
    let top4 = edges_where(&b, |h| on(h[2], 30.0));
    time(
        "chamfer 2, 4 top edges of a box (kernel)",
        &b,
        BlendFeature::chamfer(top4.clone(), 2.0),
    );

    // A rounded box: the chain round its top.
    let mut r = plate(100.0, 60.0, 30.0);
    let up = edges_where(&r, |h| h[2] > 1.0 && h[2] < 29.0);
    r.add_blend(BlendFeature::fillet(up, 5.0)).unwrap();
    let chain = edges_where(&r, |h| on(h[2], 30.0));
    assert_eq!(chain.len(), 8);
    time(
        "chamfer 2, chain of 8 round a rounded top (kernel)",
        &r,
        BlendFeature::chamfer(chain.clone(), 2.0),
    );
    time(
        "fillet 2, chain of 8 round a rounded top (kernel)",
        &r,
        BlendFeature::fillet(chain.clone(), 2.0),
    );
    time(
        "chamfer 7, past the rounds (loop_tools)",
        &r,
        BlendFeature::chamfer(chain, 7.0),
    );

    // General Booleans with a turned body (no blend): a hexagon joined to a shank, and a cut.
    let revolve = |p: &mut PartDocument, outline: &[(f64, f64)], op: Operation| {
        let o = SketchOptions {
            auto_project_origin: false,
            auto_reference_edges: false,
            ..Default::default()
        };
        let i = p
            .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XZ), &o)
            .unwrap();
        let v = outline
            .iter()
            .map(|&(r, z)| (Point2::new(r, z), None))
            .collect();
        draw_loop(&mut p.sketches[i].sketch, Loop::Path(v)).unwrap();
        let regions = p.sketches[i].sketch.regions();
        let reg = regions.first().unwrap();
        let mut f = RevolveFeature::new(
            vec![ProfileRef::of(reg, reg.interior_point())],
            AxisRef::Origin(2),
        );
        f.operation = op;
        p.add_revolve(i, f).unwrap();
    };
    let across = 13.0 / 3f64.sqrt();
    let hexagon: Vec<(f64, f64)> = (0..6)
        .map(|k| {
            let a = k as f64 * PI / 3.0;
            (across * a.cos(), across * a.sin())
        })
        .collect();
    let head = prism(&hexagon, 5.3);
    let shank = [
        (0.0, 2.65),
        (4.0, 2.65),
        (4.0, -29.2),
        (3.2, -30.0),
        (0.0, -30.0),
    ];
    let mut v: Vec<f64> = (0..5)
        .map(|_| {
            let mut p = head.clone();
            p.build_cache = Default::default();
            let t = Instant::now();
            revolve(&mut p, &shank, Operation::Join);
            let ms = t.elapsed().as_secs_f64() * 1e3;
            assert!(p.exact && p.feature_health.is_empty());
            ms
        })
        .collect();
    v.sort_by(f64::total_cmp);
    println!(
        "{:<52} {:>8.1} ms",
        "Join: a hexagon and a turned shank", v[2]
    );
    let block = plate(40.0, 30.0, 10.0);
    let tool = [
        (0.0, -1.0),
        (6.0, -1.0),
        (6.0, 4.0),
        (3.0, 7.0),
        (3.0, 11.0),
        (0.0, 11.0),
    ];
    let moved: Vec<(f64, f64)> = tool.to_vec();
    let mut v: Vec<f64> = (0..5)
        .map(|_| {
            let mut p = block.clone();
            p.build_cache = Default::default();
            let t = Instant::now();
            revolve(&mut p, &moved, Operation::Cut);
            let ms = t.elapsed().as_secs_f64() * 1e3;
            assert!(
                p.exact && p.feature_health.is_empty(),
                "{:?}",
                p.feature_health
            );
            ms
        })
        .collect();
    v.sort_by(f64::total_cmp);
    println!(
        "{:<52} {:>8.1} ms",
        "Cut: a turned tool with a cone at a plate's corner", v[2]
    );

    // Hole rims through the general path.
    for n in [6usize, 12, 36] {
        let mut p = plate(200.0, 200.0, 10.0);
        let t = top(&p, 10.0);
        let centre = |k: usize| (10.0 + 30.0 * (k % 6) as f64, 10.0 + 30.0 * (k / 6) as f64);
        let j = sketch_on(&mut p, &t, |sk, at| {
            for k in 0..n {
                let (x, y) = centre(k);
                sk.add_circle(at(x, y, 10.0), 4.0).unwrap();
            }
        });
        cut(&mut p, j, None, |r| r.holes.is_empty() && r.area() < 60.0);
        let e = edges_where(&p, |h| {
            on(h[2], 10.0) && h[0] > 1.0 && h[0] < 199.0 && h[1] > 1.0 && h[1] < 199.0
        });
        time(
            &format!("chamfer 1, {n} hole rims (kernel)"),
            &p,
            BlendFeature::chamfer(e, 1.0),
        );
    }
}

/// A chamfer wider than the rounds of its loop: the bevel planes run on through the corners and
/// meet sharp. Against the slices of the kept shape, with the bevels' names kept.
#[test]
fn a_chamfer_wider_than_the_rounds_removes_what_it_says() {
    let (w, l, h, r, d) = (100.0f64, 60.0f64, 30.0f64, 5.0f64, 7.0f64);
    let mut p = plate(w, l, h);
    let up = edges_where(&p, |q| q[2] > 1.0 && q[2] < h - 1.0);
    p.add_blend(BlendFeature::fillet(up, r)).unwrap();
    let before = volume(&p);
    let chain = edges_where(&p, |q| on(q[2], h));
    p.add_blend(BlendFeature::chamfer(chain, d)).unwrap();
    assert!(
        p.exact && p.feature_health.is_empty(),
        "{:?}",
        p.feature_health
    );
    for (id, _) in p.body.solids() {
        p.body.validate_solid(id).unwrap();
    }
    // Rounded rectangle less the frame between it and the inset rectangle, per slice.
    let in_round = |x: f64, y: f64| {
        let (ax, ay) = ((x - w / 2.0).abs(), (y - l / 2.0).abs());
        let (cx, cy) = (w / 2.0 - r, l / 2.0 - r);
        ax <= cx || ay <= cy || (ax - cx).hypot(ay - cy) <= r
    };
    let (slices, grid) = (70, 500);
    let mut removed = 0.0;
    for k in 0..slices {
        let inset = d * (k as f64 + 0.5) / slices as f64;
        let (dx, dy) = (w / grid as f64, l / grid as f64);
        let mut area = 0.0;
        for i in 0..grid {
            for j in 0..grid {
                let (x, y) = ((i as f64 + 0.5) * dx, (j as f64 + 0.5) * dy);
                let inside = x >= inset && x <= w - inset && y >= inset && y <= l - inset;
                if in_round(x, y) && !inside {
                    area += dx * dy;
                }
            }
        }
        removed += area * d / slices as f64;
    }
    let measured = before - volume(&p);
    println!("removed {measured:.1}, slices {removed:.1}");
    assert!(
        (measured - removed).abs() < removed * 0.01,
        "{measured} vs {removed}"
    );
    let names: Vec<String> = p
        .body
        .faces()
        .filter_map(|(_, f)| f.name.clone())
        .filter(|n| n.contains("Bevel"))
        .collect();
    assert_eq!(names.len(), 4, "{names:?}");
    let mut unique = names.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), 4, "a bevel per straight edge: {names:?}");
    assert!(
        names.iter().all(|n| n.starts_with("Chamfer1:Bevel[")),
        "{names:?}"
    );
    assert!(
        !p.body
            .faces()
            .filter_map(|(_, f)| f.name.as_ref())
            .any(|n| n.contains("#r")),
        "a tool's name is left"
    );
}

/// A cylinder cut by an inclined plane: its rim is an exact ellipse (`heal.rs`, no longer an
/// intersection curve). The volume follows the mean height, and the rim takes a chamfer.
#[test]
fn an_inclined_cut_is_an_exact_ellipse() {
    let (r, h) = (5.0f64, 40.0f64);
    let mut p = PartDocument::default();
    let o = SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    };
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &o)
        .unwrap();
    draw_loop(
        &mut p.sketches[i].sketch,
        Loop::Circle(Point2::new(0.0, 0.0), r, false),
    )
    .unwrap();
    let sk = &p.sketches[i].sketch;
    let flip = sk.plane.normal.z < 0.0;
    let regions = sk.regions();
    let reg = regions.first().unwrap();
    let mut e = ExtrudeFeature::new(vec![ProfileRef::of(reg, reg.interior_point())], h);
    if flip {
        e.direction = ExtentDirection::Flipped;
    }
    p.add_extrude(i, e).unwrap();
    // Everything above z = 20 + x / 2.
    let j = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XZ), &o)
        .unwrap();
    let wedge = [(-30.0, 5.0), (30.0, 35.0), (30.0, 80.0), (-30.0, 80.0)]
        .iter()
        .map(|&(x, z)| (Point2::new(x, z), None))
        .collect();
    draw_loop(&mut p.sketches[j].sketch, Loop::Path(wedge)).unwrap();
    let regions = p.sketches[j].sketch.regions();
    let reg = regions.first().unwrap();
    let mut cut = ExtrudeFeature::new(vec![ProfileRef::of(reg, reg.interior_point())], 40.0);
    cut.operation = Operation::Cut;
    cut.direction = ExtentDirection::Symmetric;
    p.add_extrude(j, cut).unwrap();
    assert!(
        p.exact && p.feature_health.is_empty(),
        "{:?}",
        p.feature_health
    );
    let expected = PI * r * r * 20.0;
    let v = volume(&p);
    assert!((v - expected).abs() < expected * 1e-3, "{v} vs {expected}");
    for (id, _) in p.body.solids() {
        p.body.validate_solid(id).unwrap();
    }
    // A hint lies on the edge's display polyline: on the plane, within its chord of the cylinder.
    let rim = edges_where(&p, |q| q[2] > 16.0 && q[2] < 24.0);
    assert!(!rim.is_empty(), "no rim edge");
    for e in &rim {
        let q = e.hint;
        assert!(
            (q[2] - (20.0 + q[0] / 2.0)).abs() < 1e-6,
            "{q:?} off the plane"
        );
        assert!(
            (q[0].hypot(q[1]) - r).abs() < 0.01,
            "{q:?} off the cylinder"
        );
    }
    // A chamfer on it, as the kernel takes an ellipse.
    let before = volume(&p);
    p.add_blend(BlendFeature::chamfer(rim, 0.5)).unwrap();
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    assert!(volume(&p) < before, "the chamfer must take material away");
}
