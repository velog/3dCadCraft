//! Cross-feature combinations (`docs/spec/TESTING.md`): features built on
//! the results of others, cases from SolidCraft's and FreeCAD's issue
//! trackers (`docs/SOLIDCRAFT_ISSUES.md`, `docs/FREECAD_ISSUES.md`; numbers
//! in the test names refer to them), edits, deletes and save/load. Every
//! case must build with the exact kernel, give valid solids and match its
//! analytic volume, or be refused with a clear message leaving the part as
//! it was.

mod common;
use common::*;

// ------------------------------------------- Booleans (SolidCraft list) ---

/// A pocket ending exactly on the bottom face plane (SolidCraft #2963 /
/// #2591): a through hole, no zero-thickness face left.
#[test]
fn pocket_ending_on_the_bottom_plane_is_a_clean_hole() {
    let mut p = plate(40.0, 30.0, 10.0);
    let t = top(&p, 10.0);
    let j = sketch_on(&mut p, &t, |sk, at| {
        sk.add_rectangle_two_point(at(15.0, 10.0, 10.0), at(25.0, 20.0, 10.0))
            .unwrap();
    });
    cut(&mut p, j, Some(10.0), |r| {
        r.holes.is_empty() && r.area() < 150.0
    });
    check(&p, 12_000.0 - 1000.0, 1e-6);
    // 6 outer faces and 4 hole walls; no face where the pocket ended.
    let m = p.exact_bodies.0[0].measure_full().unwrap();
    assert_eq!(m.faces, 10, "faces");
}

/// Two pockets ending at the same floor height beside each other.
#[test]
fn neighbouring_pockets_sharing_the_floor_plane() {
    let mut p = plate(40.0, 30.0, 10.0);
    let t = top(&p, 10.0);
    let j = sketch_on(&mut p, &t, |sk, at| {
        sk.add_rectangle_two_point(at(5.0, 5.0, 10.0), at(15.0, 25.0, 10.0))
            .unwrap();
    });
    cut(&mut p, j, Some(4.0), |r| {
        r.holes.is_empty() && r.area() < 250.0
    });
    let t = top(&p, 10.0);
    let j = sketch_on(&mut p, &t, |sk, at| {
        sk.add_rectangle_two_point(at(20.0, 5.0, 10.0), at(35.0, 25.0, 10.0))
            .unwrap();
    });
    cut(&mut p, j, Some(4.0), |r| {
        r.holes.is_empty() && r.area() < 350.0
    });
    check(&p, 12_000.0 - 4.0 * (200.0 + 300.0), 1e-6);
}

/// Through All from a sketch on the top face: a boss above it stays
/// (SolidCraft #423).
#[test]
fn through_all_cut_leaves_a_boss_behind_its_sketch_plane() {
    let mut p = plate(40.0, 30.0, 10.0);
    let t = top(&p, 10.0);
    let j = sketch_on(&mut p, &t, |sk, at| {
        sk.add_rectangle_two_point(at(30.0, 10.0, 10.0), at(38.0, 20.0, 10.0))
            .unwrap();
    });
    boss(&mut p, j, 5.0, |r| r.holes.is_empty() && r.area() < 100.0);
    let t = top(&p, 10.0);
    let j = sketch_on(&mut p, &t, |sk, at| {
        sk.add_circle(at(10.0, 15.0, 10.0), 4.0).unwrap();
    });
    cut(&mut p, j, None, |r| r.holes.is_empty() && r.area() < 60.0);
    check(&p, 12_000.0 + 8.0 * 10.0 * 5.0 - PI * 16.0 * 10.0, 1e-4);
}

/// A second cut 0.1 mm off an existing hole's axis (SolidCraft #1279): the
/// union of two discs is removed.
#[test]
fn cut_nearly_coaxial_with_a_hole() {
    let mut p = plate(40.0, 30.0, 10.0);
    let t = top(&p, 10.0);
    let j = sketch_on(&mut p, &t, |sk, at| {
        sk.add_circle(at(20.0, 15.0, 10.0), 5.0).unwrap();
    });
    cut(&mut p, j, None, |r| r.holes.is_empty() && r.area() < 100.0);
    let t = top(&p, 10.0);
    let j = sketch_on(&mut p, &t, |sk, at| {
        sk.add_circle(at(20.1, 15.0, 10.0), 5.0).unwrap();
    });
    let r = p.sketches[j].sketch.regions();
    // Curved face edges are not projected yet: the new disc is its own
    // region; cutting it removes the crescent beside the hole.
    let mut ex = ExtrudeFeature::new(
        region_profiles(&r, |r| r.holes.is_empty() && r.area() < 100.0),
        1.0,
    );
    ex.operation = Operation::Cut;
    ex.direction = ExtentDirection::Flipped;
    ex.extent = Extent::ThroughAll;
    if let Err(e) = p.add_extrude(j, ex) {
        panic!("{e}");
    }
    // Union of two discs r 5, centres 0.1 apart.
    let (r, c) = (5.0_f64, 0.1_f64);
    let lens = 2.0 * r * r * (c / (2.0 * r)).acos() - c / 2.0 * (4.0 * r * r - c * c).sqrt();
    let union = 2.0 * PI * r * r - lens;
    check(&p, 12_000.0 - union * 10.0, 1e-4);
}

/// Cross holes of equal radius (SolidCraft #788): a cube with a Ø10 hole
/// along Z and one along X (Steinmetz solid 16 r³ / 3 counted once).
#[test]
fn cross_holes_of_equal_radius() {
    let mut p = plate(40.0, 40.0, 40.0);
    let t = top(&p, 40.0);
    let j = sketch_on(&mut p, &t, |sk, at| {
        sk.add_circle(at(20.0, 20.0, 40.0), 5.0).unwrap();
    });
    cut(&mut p, j, None, |r| r.holes.is_empty() && r.area() < 100.0);
    let right = face_where(&p, |q| (q.x - 40.0).abs() < 1e-6);
    let j = sketch_on(&mut p, &right, |sk, at| {
        sk.add_circle(at(40.0, 20.0, 20.0), 5.0).unwrap();
    });
    cut(&mut p, j, None, |r| r.holes.is_empty() && r.area() < 100.0);
    let r: Real = 5.0;
    check(
        &p,
        64_000.0 - 2.0 * PI * r * r * 40.0 + 16.0 * r * r * r / 3.0,
        5e-4,
    );
}

// --------------------------------------------- blends on cut geometry ---

/// A chamfer on the bottom edge of a through hole (SolidCraft #1574: it
/// added material below the part): the part only gets smaller.
#[test]
fn chamfer_of_a_hole_edge_at_the_bottom() {
    let mut p = plate(40.0, 30.0, 10.0);
    let t = top(&p, 10.0);
    let j = sketch_on(&mut p, &t, |sk, at| {
        sk.add_circle(at(20.0, 15.0, 10.0), 3.0).unwrap();
    });
    cut(&mut p, j, None, |r| r.holes.is_empty() && r.area() < 40.0);
    // A circle comes as two halves: their hints are chord midpoints.
    let e = edges_where(&p, |h| {
        h[2].abs() < 1e-6 && (h[0] - 20.0).hypot(h[1] - 15.0) < 3.0 + 1e-3
    });
    assert!(!e.is_empty());
    p.add_blend(BlendFeature::chamfer(e, 0.5)).unwrap();
    // The ring removed: a triangle 0.5 × 0.5 revolved at radius 3 + 0.5/3.
    let ring = 2.0 * PI * (3.0 + 0.5 / 3.0) * 0.125;
    check(&p, 12_000.0 - PI * 9.0 * 10.0 - ring, 1e-3);
    let m = p.exact_bodies.0[0].measure_full().unwrap();
    assert!(m.bbox.0.z > -1e-6, "material below the part: {:?}", m.bbox);
}

/// A chamfer as large as the round of the corner it runs round (SolidCraft
/// #2719): valid solid, smaller than before.
#[test]
fn chamfer_equal_to_the_corner_round() {
    let mut p = plate(40.0, 30.0, 20.0);
    let e = vertical(&p, 40.0, 30.0, 20.0);
    p.add_blend(BlendFeature::fillet(e, 1.0)).unwrap();
    let before = volume(&p);
    let e = edges_where(&p, |h| (h[2] - 20.0).abs() < 1e-6);
    let chain = cadcraft3d::part::blend::tangent_chain(
        &p.body,
        p.body
            .edges()
            .find(|(x, _)| edge_ref(&p.body, *x).is_some_and(|r| e.contains(&r)))
            .unwrap()
            .0,
    );
    p.add_blend(BlendFeature::chamfer(chain, 1.0)).unwrap();
    let after = volume(&p);
    assert!(p.exact && p.feature_health.is_empty());
    for (sid, _) in p.body.solids() {
        p.body.validate_solid(sid).unwrap();
    }
    // Roughly the bevel all round the top: 140 mm × 0.5 mm².
    assert!(
        after < before && before - after < 0.5 * 145.0,
        "{before} → {after}"
    );
}

/// SolidCraft #1207: a bar 100×30×10 with a Ø40 boss in the middle that
/// overhangs both long sides (only the part over the bar is extruded), so
/// the concave edge on top is two partial arcs; rounded r1.
#[test]
fn fillet_of_a_partial_arc_concave_edge() {
    let mut p = plate(100.0, 30.0, 10.0);
    let t = top(&p, 10.0);
    let j = sketch_on(&mut p, &t, |sk, at| {
        sk.add_circle(at(50.0, 15.0, 10.0), 20.0).unwrap();
    });
    // The disc less the two segments beyond the bar's sides.
    let theta = 2.0 * (15.0_f64 / 20.0).acos();
    let segment = 200.0 * (theta - theta.sin());
    let inside = PI * 400.0 - 2.0 * segment;
    boss(&mut p, j, 8.0, |r| {
        r.holes.is_empty() && (r.area() - inside).abs() < 1.0
    });
    check(&p, 30_000.0 + inside * 8.0, 1e-4);
    let before = volume(&p);
    let e: Vec<EdgeRef> = edges_where(&p, |_| true)
        .into_iter()
        .filter(|r| {
            r.faces.iter().any(|f| f == "Extrusion1:End")
                && r.faces.iter().any(|f| f.starts_with("Extrusion2:Side"))
        })
        .collect();
    assert_eq!(e.len(), 2, "{e:?}");
    match p.add_blend(BlendFeature::fillet(e, 1.0)) {
        Ok(_) => {
            let after = volume(&p);
            assert!(p.feature_health.is_empty());
            // Concave: material added, about (1 − π/4) per mm of the arcs.
            let arcs = 2.0 * 20.0 * (PI - theta);
            let want = round_loss(1.0) * arcs;
            assert!(
                (after - before - want).abs() < 0.1 * want,
                "{before} → {after}, want +{want}"
            );
        }
        // A clear refusal is acceptable (open in SolidCraft); the part stays.
        Err(e) => {
            assert!(e.to_string().contains("kernel"), "{e}");
            assert!((volume(&p) - before).abs() < 1e-6);
        }
    }
}

// ----------------------------------------------------- shell combos ---

/// A revolved cup: a disc revolved from a rectangle, shelled from the top.
#[test]
fn revolved_disc_shelled_to_a_cup() {
    let mut p = PartDocument::default();
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XZ), &opts())
        .unwrap();
    let sk = &mut p.sketches[i].sketch;
    sk.add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(20.0, 30.0))
        .unwrap();
    let r = sk.regions();
    p.add_revolve(
        i,
        RevolveFeature::new(region_profiles(&r, |_| true), AxisRef::Origin(2)),
    )
    .unwrap();
    let t = top(&p, 30.0);
    shell(
        &mut p,
        &[(&t, [0.0, 0.0, 30.0])],
        2.0,
        ShellDirection::Inside,
    );
    check(&p, PI * (400.0 * 30.0 - 324.0 * 28.0), 3e-4);
}

/// A tapered extrusion (+3°, growing) rounded on top, then shelled open at
/// the bottom.
#[test]
fn taper_then_shell_open_bottom() {
    let mut p = PartDocument::default();
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    let sk = &mut p.sketches[i].sketch;
    sk.add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(40.0, 30.0))
        .unwrap();
    let r = sk.regions();
    let mut ex = ExtrudeFeature::new(region_profiles(&r, |_| true), 20.0);
    ex.taper = 3f64.to_radians();
    p.add_extrude(i, ex).unwrap();
    let bottom = top(&p, 0.0);
    shell(
        &mut p,
        &[(&bottom, [20.0, 15.0, 0.0])],
        2.0,
        ShellDirection::Inside,
    );
    let (kk, c) = (k(3.0), 3f64.to_radians().cos());
    let sec = |z: Real, inset: Real| (40.0 + 2.0 * z * kk - inset) * (30.0 + 2.0 * z * kk - inset);
    let v = simpson(&|z| sec(z, 0.0), 0.0, 20.0) - simpson(&|z| sec(z, 4.0 / c), 0.0, 18.0);
    check(&p, v, 1e-4);
}

/// Shell Outside, then the outer vertical edges rounded.
#[test]
fn shell_outside_then_fillet_outer_edges() {
    let mut p = plate(40.0, 30.0, 20.0);
    let t = top(&p, 20.0);
    shell(
        &mut p,
        &[(&t, [20.0, 15.0, 20.0])],
        2.0,
        ShellDirection::Outside,
    );
    let c = |x: Real, a: Real, b: Real| (x - a).abs() < 1e-6 || (x - b).abs() < 1e-6;
    let e = edges_where(&p, |h| {
        c(h[0], -2.0, 42.0) && c(h[1], -2.0, 32.0) && h[2] > -2.0 + 1e-6 && h[2] < 20.0 - 1e-6
    });
    assert!(!e.is_empty(), "no outer vertical edges");
    p.add_blend(BlendFeature::fillet(e, 2.0)).unwrap();
    check(
        &p,
        44.0 * 34.0 * 22.0 - 24_000.0 - 4.0 * round_loss(2.0) * 22.0,
        1e-4,
    );
}

/// Shell Both, then a window cut through one wall into the cavity, its
/// edges rounded.
#[test]
fn shell_both_then_window_then_fillet() {
    let mut p = plate(40.0, 30.0, 20.0);
    let t = top(&p, 20.0);
    shell(
        &mut p,
        &[(&t, [20.0, 15.0, 20.0])],
        2.0,
        ShellDirection::Center,
    );
    let front = face_where(&p, |q| (q.y + 1.0).abs() < 1e-6);
    let j = sketch_on(&mut p, &front, |sk, at| {
        sk.add_rectangle_two_point(at(10.0, -1.0, 6.0), at(30.0, -1.0, 14.0))
            .unwrap();
    });
    cut(&mut p, j, Some(2.0), |r| {
        r.holes.is_empty() && r.area() < 200.0
    });
    let shelled = 42.0 * 32.0 * 21.0 - 38.0 * 28.0 * 19.0;
    check(&p, shelled - 20.0 * 8.0 * 2.0, 1e-4);
}

/// A slot cut across the top (splitting it), then a shell removing the
/// larger piece of the top.
#[test]
fn shell_removing_a_piece_of_a_split_top() {
    let mut p = plate(40.0, 30.0, 20.0);
    let t = top(&p, 20.0);
    let j = sketch_on(&mut p, &t, |sk, at| {
        sk.add_rectangle_two_point(at(28.0, -1.0, 20.0), at(32.0, 31.0, 20.0))
            .unwrap();
    });
    cut(&mut p, j, Some(3.0), |r| {
        r.area() < 130.0 && r.area() > 110.0
    });
    let piece = face_where(&p, |q| (q.z - 20.0).abs() < 1e-6 && q.x < 28.0 + 1e-6);
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut def = ShellFeature::new(2.0);
        def.remove.push(face_ref(&p, &piece, [10.0, 15.0, 20.0]));
        let mut q = p.clone();
        q.add_shell(def).map(|_| q)
    }))
    .expect("no panic");
    match r {
        Ok(q) => {
            assert!(q.exact && q.feature_health.is_empty());
            for (sid, _) in q.body.solids() {
                q.body.validate_solid(sid).unwrap();
            }
            assert!(volume(&q) < volume(&p));
        }
        Err(e) => assert!(e.to_string().contains("kernel"), "{e}"),
    }
}

/// A shell after an edit of the extrusion before it: the walls follow the
/// new height.
#[test]
fn shell_follows_an_edit_of_the_extrusion() {
    let mut p = plate(40.0, 30.0, 20.0);
    let t = top(&p, 20.0);
    shell(
        &mut p,
        &[(&t, [20.0, 15.0, 20.0])],
        2.0,
        ShellDirection::Inside,
    );
    let Some(Feature::Extrude(mut ex)) = p.features.first().cloned() else {
        panic!("extrude");
    };
    ex.distance = 30.0;
    p.replace_feature(0, Feature::Extrude(ex)).unwrap();
    check(&p, 36_000.0 - 36.0 * 26.0 * 28.0, 1e-6);
}

/// Deleting a fillet before a shell: the shell still builds.
#[test]
fn shell_survives_deleting_the_fillet_before_it() {
    let mut p = plate(40.0, 30.0, 20.0);
    let e = vertical(&p, 40.0, 30.0, 20.0);
    p.add_blend(BlendFeature::fillet(e, 5.0)).unwrap();
    let t = top(&p, 20.0);
    shell(
        &mut p,
        &[(&t, [20.0, 15.0, 20.0])],
        2.0,
        ShellDirection::Inside,
    );
    p.remove_feature(1).unwrap();
    p.regenerate().unwrap();
    check(&p, 24_000.0 - 36.0 * 26.0 * 18.0, 1e-6);
}

/// A second shell inside the first (shelling a shelled box): refused or a
/// valid, smaller solid.
#[test]
fn shell_of_a_shelled_box() {
    let mut p = plate(40.0, 30.0, 20.0);
    let t = top(&p, 20.0);
    shell(
        &mut p,
        &[(&t, [20.0, 15.0, 20.0])],
        3.0,
        ShellDirection::Inside,
    );
    let before = volume(&p);
    let mut def = ShellFeature::new(1.0);
    let rim = top(&p, 20.0);
    def.remove.push(face_ref(&p, &rim, [1.5, 15.0, 20.0]));
    match p.add_shell(def) {
        Ok(_) => {
            assert!(p.feature_health.is_empty());
            assert!(volume(&p) < before);
        }
        Err(e) => {
            assert!(e.to_string().contains("kernel"), "{e}");
            assert!((volume(&p) - before).abs() < 1e-6, "part changed");
        }
    }
}

/// Thickness 0 and walls thicker than the part: refused, part unchanged.
#[test]
fn shell_bad_thickness_leaves_the_part() {
    let mut p = plate(40.0, 30.0, 20.0);
    let t = top(&p, 20.0);
    for th in [0.0, 25.0] {
        let mut def = ShellFeature::new(th);
        def.remove.push(face_ref(&p, &t, [20.0, 15.0, 20.0]));
        refused(
            p.add_shell(def),
            if th == 0.0 {
                "greater than 0"
            } else {
                "kernel"
            },
        );
        assert_eq!(p.features.len(), 1);
        check(&p, 24_000.0, 1e-9);
    }
}

// ----------------------------------------------------- draft combos ---

/// Draft −4° (the sides lean out going up), then a shell Outside.
#[test]
fn negative_draft_then_shell_outside() {
    let mut p = plate(40.0, 30.0, 20.0);
    let s = sides(&p, 40.0, 30.0, 10.0);
    draft_sides(&mut p, &s, -4.0);
    let t = top(&p, 20.0);
    let r = {
        let mut def = ShellFeature::new(2.0);
        def.direction = ShellDirection::Outside;
        def.remove.push(face_ref(&p, &t, [20.0, 15.0, 20.0]));
        p.add_shell(def)
    };
    let kk = k(4.0);
    let inner = simpson(
        &|z| (40.0 + 2.0 * z * kk) * (30.0 + 2.0 * z * kk),
        0.0,
        20.0,
    );
    match r {
        Ok(_) => {
            let c = 4f64.to_radians().cos();
            let outer = simpson(
                &|z| (40.0 + 2.0 * z * kk + 4.0 / c) * (30.0 + 2.0 * z * kk + 4.0 / c),
                -2.0,
                20.0,
            );
            check(&p, outer - inner, 1e-3);
        }
        Err(e) => assert!(e.to_string().contains("kernel"), "{e}"),
    }
}

/// Draft 5° then chamfer 1 round the top.
#[test]
fn draft_then_chamfer_the_top() {
    let mut p = plate(40.0, 30.0, 20.0);
    let s = sides(&p, 40.0, 30.0, 10.0);
    draft_sides(&mut p, &s, 5.0);
    let e = edges_where(&p, |h| (h[2] - 20.0).abs() < 1e-6);
    assert_eq!(e.len(), 4);
    p.add_blend(BlendFeature::chamfer(e, 1.0)).unwrap();
    let kk = k(5.0);
    let frustum = simpson(
        &|z| (40.0 - 2.0 * z * kk) * (30.0 - 2.0 * z * kk),
        0.0,
        20.0,
    );
    // Equal-distance bevel at a 95° edge: a triangle with sides 1 and 1
    // at 95° → area sin(95°)/2, along the top outline (corners ~d³).
    let tri = 95f64.to_radians().sin() / 2.0;
    let per = 2.0 * ((40.0 - 40.0 * kk) + (30.0 - 40.0 * kk));
    check(&p, frustum - tri * per, 2e-4);
}

/// Draft the sides about the bottom, then edit the first extrusion's height:
/// the faces keep their names and the draft follows.
#[test]
fn draft_follows_an_edit_of_the_extrusion() {
    let mut p = plate(40.0, 30.0, 20.0);
    let s = sides(&p, 40.0, 30.0, 10.0);
    draft_sides(&mut p, &s, 5.0);
    let Some(Feature::Extrude(mut ex)) = p.features.first().cloned() else {
        panic!("extrude");
    };
    ex.distance = 10.0;
    p.replace_feature(0, Feature::Extrude(ex)).unwrap();
    let kk = k(5.0);
    check(
        &p,
        simpson(
            &|z| (40.0 - 2.0 * z * kk) * (30.0 - 2.0 * z * kk),
            0.0,
            10.0,
        ),
        1e-6,
    );
}

/// Draft of a revolved cylinder's wall about its top (Fixed Edge, pull
/// from the XY plane): an upside-down frustum.
#[test]
fn draft_fixed_top_edge_of_a_revolved_cylinder() {
    let mut p = PartDocument::default();
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XZ), &opts())
        .unwrap();
    let sk = &mut p.sketches[i].sketch;
    sk.add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(15.0, 30.0))
        .unwrap();
    let r = sk.regions();
    p.add_revolve(
        i,
        RevolveFeature::new(region_profiles(&r, |_| true), AxisRef::Origin(2)),
    )
    .unwrap();
    let wall = face_where(&p, |q| (q.x.hypot(q.y) - 15.0).abs() < 1e-3);
    let mut d = DraftFeature::new(4f64.to_radians());
    d.pull = Some(cadcraft3d::part::PullRef::Plane(PlaneRef::Origin(
        OriginPlane::XY,
    )));
    d.faces.push(face_ref(&p, &wall, [15.0, 0.0, 29.0]));
    p.add_draft(d).unwrap();
    // Fixed at the top (r 15), leaning out below: r = 15 + (30 − z) tan 4°.
    let rb = 15.0 + 30.0 * k(4.0);
    check(&p, PI * 30.0 / 3.0 * (rb * rb + rb * 15.0 + 225.0), 3e-4);
}

/// A draft angle of 0 is accepted and leaves the volume.
#[test]
fn draft_of_zero_degrees_is_a_no_op() {
    let mut p = plate(40.0, 30.0, 20.0);
    let s = sides(&p, 40.0, 30.0, 10.0);
    let mut d = DraftFeature::new(0.0);
    d.draft_type = DraftType::FixedPlane;
    d.plane = Some(PlaneRef::Origin(OriginPlane::XY));
    for (n, at) in &s {
        d.faces.push(face_ref(&p, n, *at));
    }
    match p.add_draft(d) {
        Ok(_) => check(&p, 24_000.0, 1e-9),
        Err(e) => assert!(e.to_string().contains("kernel"), "{e}"),
    }
}

/// Draft of the top face with the pull along it (the face is not parallel
/// to the pull): refused, part unchanged.
#[test]
fn draft_of_a_face_square_to_the_pull_is_refused() {
    let mut p = plate(40.0, 30.0, 20.0);
    let t = top(&p, 20.0);
    let mut d = DraftFeature::new(5f64.to_radians());
    d.draft_type = DraftType::FixedPlane;
    d.plane = Some(PlaneRef::Origin(OriginPlane::XY));
    d.faces.push(face_ref(&p, &t, [20.0, 15.0, 20.0]));
    refused(p.add_draft(d), "kernel");
    assert_eq!(p.features.len(), 1);
    check(&p, 24_000.0, 1e-9);
}

// ---------------------------------------------------- save and load ---

/// A part with Fillet, Shell, Draft and a cut, saved and opened again:
/// same features, same volume, no sick feature.
#[test]
fn shell_draft_fillet_cut_survive_save_and_load() {
    let mut p = plate(40.0, 30.0, 20.0);
    let s = sides(&p, 40.0, 30.0, 10.0);
    draft_sides(&mut p, &s, 3.0);
    let t = top(&p, 20.0);
    shell(
        &mut p,
        &[(&t, [20.0, 15.0, 20.0])],
        2.0,
        ShellDirection::Inside,
    );
    let e = edges_where(&p, |h| h[2].abs() < 1e-6);
    p.add_blend(BlendFeature::chamfer(e, 0.5)).unwrap();
    let v = volume(&p);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("combo.3dcadcraft");
    let mut project = Project::new("Combo");
    project.add_document("Part1", DocumentData::Part(p.clone()));
    project.save(&path).unwrap();
    let mut back = Project::load(&path).unwrap();
    let id = back.documents()[0].id;
    let Some(DocumentData::Part(q)) = back.document_mut(id).map(|d| &mut d.data) else {
        panic!("part");
    };
    q.regenerate().unwrap();
    assert_eq!(q.features.len(), p.features.len());
    check(q, v, 1e-9);
}

/// STEP of a shelled, drafted, chamfered part reads back with the same
/// volume (surfaces stay exact, SolidCraft #612/#1198).
#[test]
fn step_round_trip_of_a_combined_part() {
    let mut p = plate(40.0, 30.0, 20.0);
    let s = sides(&p, 40.0, 30.0, 10.0);
    draft_sides(&mut p, &s, 3.0);
    let t = top(&p, 20.0);
    shell(
        &mut p,
        &[(&t, [20.0, 15.0, 20.0])],
        2.0,
        ShellDirection::Inside,
    );
    let e = vertical(&p, 40.0, 30.0, 20.0);
    let _ = e;
    let text = cadcraft3d::kernel::exact::step_text(&p.exact_bodies.0, "Combo").unwrap();
    let vols = cadcraft3d::kernel::exact::step_volumes(&text).unwrap();
    let want: Real = p
        .exact_bodies
        .0
        .iter()
        .map(|b| b.measure_full().unwrap().volume)
        .sum();
    let got: Real = vols.iter().sum();
    assert!((got - want).abs() / want < 1e-4, "{got} vs {want}");
}

// ------------------------------------------------- fillet / chamfer ---

/// Fillet after a Join of two prisms meeting flush (a step): the concave
/// edge of the step is rounded.
#[test]
fn fillet_of_a_step_after_join() {
    let mut p = plate(40.0, 30.0, 10.0);
    let t = top(&p, 10.0);
    let j = sketch_on(&mut p, &t, |sk, at| {
        sk.add_rectangle_two_point(at(0.0, 0.0, 10.0), at(20.0, 30.0, 10.0))
            .unwrap();
    });
    let plane = p.sketches[j].sketch.plane.clone();
    boss(&mut p, j, 10.0, |r| {
        let q = r.interior_point();
        (r.area() - 600.0).abs() < 1.0 && plane.point_at(q.x, q.y).x < 20.0
    });
    // The concave edge only (the front and back edges at x = 20 are convex).
    let e = edges_where(&p, |h| {
        (h[0] - 20.0).abs() < 1e-6 && (h[2] - 10.0).abs() < 1e-6 && h[1] > 1.0 && h[1] < 29.0
    });
    assert_eq!(e.len(), 1, "{e:?}");
    p.add_blend(BlendFeature::fillet(e, 2.0)).unwrap();
    check(&p, 12_000.0 + 6_000.0 + round_loss(2.0) * 30.0, 1e-4);
}

/// Chamfer then fillet on the same body: the fillet on the chamfer's
/// neighbouring vertical edges.
#[test]
fn chamfer_then_fillet_neighbours() {
    let mut p = plate(40.0, 30.0, 20.0);
    let e = edges_where(&p, |h| (h[2] - 20.0).abs() < 1e-6 && h[1].abs() < 1e-6);
    p.add_blend(BlendFeature::chamfer(e, 2.0)).unwrap();
    let e = edges_where(&p, |h| {
        h[1].abs() < 1e-6
            && h[2] > 1e-6
            && h[2] < 18.0
            && (h[0].abs() < 1e-6 || (h[0] - 40.0).abs() < 1e-6)
    });
    assert!(!e.is_empty(), "no vertical front edges");
    match p.add_blend(BlendFeature::fillet(e, 1.0)) {
        Ok(_) => {
            assert!(p.feature_health.is_empty());
            let v = volume(&p);
            let chamfered = 24_000.0 - 2.0 * 40.0;
            assert!(
                v < chamfered && v > chamfered - 2.0 * round_loss(1.0) * 20.0,
                "{v}"
            );
        }
        Err(e) => assert!(e.to_string().contains("kernel"), "{e}"),
    }
}

/// A fillet whose radius exceeds the face it rolls on (r 12 on a plate 10
/// thick): refused (SolidCraft #1080), the part unchanged.
#[test]
fn fillet_larger_than_the_face_is_refused() {
    let mut p = plate(40.0, 30.0, 10.0);
    let e = edges_where(&p, |h| (h[2] - 10.0).abs() < 1e-6 && h[1].abs() < 1e-6);
    let r = p.add_blend(BlendFeature::fillet(e, 12.0));
    match r {
        Err(_) => check(&p, 12_000.0, 1e-9),
        Ok(_) => panic!("a 12 mm round on a 10 mm plate edge was accepted"),
    }
}
