//! Speed budgets of typical operations (`docs/PERF.md`): each case runs a
//! few times and its median must stay within its budget. A new feature
//! adds its case here; a case over budget means looking for a faster way,
//! not raising the number.
//!
//! The budgets are for the test profile (our crate at opt-level 1,
//! dependencies at 3); release builds are faster.

use std::time::Instant;

use cadcraft3d::kernel::{Point3, Real};
use cadcraft3d::part::blend::{edge_ref, tangent_chain};
use cadcraft3d::part::{
    BlendFeature, EdgeRef, ExtentDirection, ExtrudeFeature, Operation, PartDocument, ProfileRef,
    SketchOptions, SketchPlacement,
};

/// Median wall time (ms) of `runs` calls.
fn median_ms(runs: usize, mut f: impl FnMut()) -> f64 {
    let mut t: Vec<f64> = (0..runs)
        .map(|_| {
            let t0 = Instant::now();
            f();
            t0.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    t.sort_by(f64::total_cmp);
    t[t.len() / 2]
}

/// The fillet/chamfer acceptance part: box, 8 fillets r5, top-loop chamfer 2.
fn rounded_box() -> PartDocument {
    let (a, b, h, r) = (40.0, 30.0, 20.0, 5.0);
    let mut part = PartDocument::with_box("Box1", [a, b, h]).unwrap();
    let f = |x: &str, y: &str, hint: [Real; 3]| EdgeRef {
        faces: [format!("Box1:{x}"), format!("Box1:{y}")],
        hint,
    };
    part.add_blend(BlendFeature::fillet(
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
    let top = part
        .body
        .edges()
        .find(|(e, _)| {
            edge_ref(&part.body, *e).is_some_and(|x| {
                x.faces.contains(&"Box1:Top".to_owned())
                    && x.faces.contains(&"Box1:Front".to_owned())
            })
        })
        .map(|(e, _)| e)
        .unwrap();
    part.add_blend(BlendFeature::chamfer(tangent_chain(&part.body, top), 2.0))
        .unwrap();
    part
}

/// A 40 mm cube with a 32×32 pocket 36 deep (4 mm walls).
fn hollow() -> PartDocument {
    let s = 40.0;
    let mut part = PartDocument::with_box("Box1", [s, s, s]).unwrap();
    let opts = SketchOptions {
        auto_project_origin: false,
        ..Default::default()
    };
    let i = part
        .create_sketch(SketchPlacement::on_face("Box1:Top"), &opts)
        .unwrap();
    let plane = part.sketches[i].sketch.plane.clone();
    let a = plane.project(&Point3::new(4.0, 4.0, s));
    let b = plane.project(&Point3::new(36.0, 36.0, s));
    let sk = &mut part.sketches[i].sketch;
    sk.add_rectangle_two_point(a, b).unwrap();
    let r = sk.regions();
    let pocket = r.iter().find(|x| x.holes.is_empty()).unwrap();
    let mut ex = ExtrudeFeature::new(vec![ProfileRef::of(pocket, pocket.interior_point())], 36.0);
    ex.direction = ExtentDirection::Flipped;
    ex.operation = Operation::Cut;
    part.add_extrude(i, ex).unwrap();
    part
}

/// The hollow box's edges below the rim (outer and inner).
fn below_rim(part: &PartDocument) -> Vec<EdgeRef> {
    part.body
        .edges()
        .filter_map(|(e, _)| edge_ref(&part.body, e))
        .filter(|r| r.hint[2] < 40.0 - 1e-6)
        .collect()
}

#[test]
fn speed_budgets() {
    // (case, budget ms, median ms)
    let mut rows: Vec<(&str, f64, f64)> = Vec::new();

    let rb = rounded_box();
    rows.push((
        "regenerate the rounded box (box, 8 fillets, chamfer chain)",
        400.0,
        median_ms(3, || {
            let mut p = rb.clone();
            p.build_cache = Default::default(); // measure the build, not the cache
            p.regenerate().unwrap();
            assert!(p.exact);
        }),
    ));

    rows.push((
        "build the hollow box (box, sketch on face, cut)",
        50.0,
        median_ms(3, || {
            let _ = hollow();
        }),
    ));

    let h = hollow();
    let e = below_rim(&h);
    rows.push((
        "fillet r2 of the hollow box's 20 edges below the rim",
        600.0,
        median_ms(3, || {
            let mut p = h.clone();
            p.build_cache = Default::default();
            p.add_blend(BlendFeature::fillet(e.clone(), 2.0)).unwrap();
        }),
    ));
    rows.push((
        "chamfer 2 of the same edges (wedge fallback)",
        1200.0,
        median_ms(3, || {
            let mut p = h.clone();
            p.build_cache = Default::default();
            p.add_blend(BlendFeature::chamfer(e.clone(), 2.0)).unwrap();
        }),
    ));

    // Select Tangencies (double click) on the filleted pocket: every inner
    // face (walls, rounds, bottom).
    let mut filleted = h.clone();
    filleted
        .add_blend(BlendFeature::fillet(e.clone(), 2.0))
        .unwrap();
    let wall = filleted
        .body
        .faces()
        .find(|(_, f)| {
            f.name
                .as_deref()
                .is_some_and(|n| n.starts_with("Extrusion1:Side"))
        })
        .map(|(id, _)| id)
        .unwrap();
    rows.push((
        "Select Tangencies on the filleted pocket's wall",
        50.0,
        median_ms(3, || {
            let faces = cadcraft3d::part::select::tangent_faces(&filleted.body, wall);
            assert!(faces.len() > 10, "{}", faces.len());
        }),
    ));

    // File > Export of the filleted hollow box: STL, STEP, GLB.
    for f in cadcraft3d::part::export::ExportFormat::ALL {
        rows.push((
            match f {
                cadcraft3d::part::export::ExportFormat::Stl => {
                    "export STL of the filleted hollow box"
                }
                cadcraft3d::part::export::ExportFormat::Step => {
                    "export STEP of the filleted hollow box"
                }
                cadcraft3d::part::export::ExportFormat::Glb => {
                    "export GLB of the filleted hollow box"
                }
            },
            200.0,
            median_ms(3, || {
                let b = cadcraft3d::part::export::export_bytes(&filleted, "Hollow", f).unwrap();
                assert!(b.len() > 1000);
            }),
        ));
    }

    // Select Mode Solid (All Rounds): every edge of the filleted pocket
    // classified convex / concave.
    rows.push((
        "All Rounds: classify the filleted hollow box's edges",
        100.0,
        median_ms(3, || {
            let r = cadcraft3d::part::select::solid_edges(
                &filleted.body,
                None,
                cadcraft3d::part::select::SolidEdges::Rounds,
            );
            assert!(!r.is_empty());
        }),
    ));

    // A sketch pattern dialog rebuilds its preview on a sketch copy; a
    // 10 × 10 grid of a rectangle must stay interactive.
    let mut grid = cadcraft3d::sketch::Sketch::default();
    let axis_x = grid.add_line(
        cadcraft3d::sketch::Point2::new(0.0, -5.0),
        cadcraft3d::sketch::Point2::new(10.0, -5.0),
    );
    let axis_y = grid.add_line(
        cadcraft3d::sketch::Point2::new(-5.0, 0.0),
        cadcraft3d::sketch::Point2::new(-5.0, 10.0),
    );
    let rect = grid
        .add_rectangle_two_point(
            cadcraft3d::sketch::Point2::new(0.0, 0.0),
            cadcraft3d::sketch::Point2::new(4.0, 6.0),
        )
        .unwrap();
    let dir = |line, spacing| cadcraft3d::sketch::PatternDirection {
        line,
        flip: false,
        count: 10,
        spacing,
        fitted: false,
    };
    rows.push((
        "sketch pattern preview: 10 x 10 rectangles",
        150.0,
        median_ms(3, || {
            let mut sk = grid.clone();
            sk.rectangular_pattern(&rect.entities, dir(axis_x, 8.0), Some(dir(axis_y, 10.0)))
                .unwrap();
            sk.solve().unwrap();
            assert_eq!(sk.patterns().next().unwrap().1.instances.len(), 99);
        }),
    ));

    let mut solved = grid.clone();
    solved
        .rectangular_pattern(&rect.entities, dir(axis_x, 8.0), Some(dir(axis_y, 10.0)))
        .unwrap();
    solved.solve().unwrap();
    rows.push((
        "sketch DOF analysis (glyphs, colours) of the 10 x 10 pattern",
        100.0,
        median_ms(3, || {
            let _ = solved.dof_analysis();
        }),
    ));

    // Dragging: a corner of the original moves, all 99 copies follow.
    let corner = rect.points[2];
    rows.push((
        "sketch drag step in the 10 x 10 pattern (solve after a move)",
        50.0,
        median_ms(3, || {
            let mut sk = solved.clone();
            let q = sk.point(corner).unwrap().position;
            sk.move_point(corner, q + cadcraft3d::sketch::Vector2::new(0.5, 0.3))
                .unwrap();
            sk.solve().unwrap();
        }),
    ));

    // Shell of the rounded box (8 fillets and the chamfer chain before
    // it), the chamfered top removed: walls follow the rounds.
    rows.push((
        "shell 1.5 of the rounded box, top removed (add_shell)",
        600.0,
        median_ms(3, || {
            let mut p = rb.clone();
            p.build_cache = Default::default();
            let top = p.body.find_face("Box1:Top").unwrap();
            let mut def = cadcraft3d::part::ShellFeature::new(1.5);
            def.remove
                .push(cadcraft3d::part::shell::face_ref(&p.body, top, None).unwrap());
            p.add_shell(def).unwrap();
        }),
    ));

    println!("{:<62} {:>9} {:>9}", "case", "budget", "median");
    let mut over = Vec::new();
    for (case, budget, ms) in &rows {
        let flag = if ms > budget { "  OVER" } else { "" };
        println!("{case:<62} {budget:>7.0} ms {ms:>7.1} ms{flag}");
        if ms > budget {
            over.push(format!("{case}: {ms:.0} ms > {budget:.0} ms"));
        }
    }
    assert!(over.is_empty(), "over budget:\n{}", over.join("\n"));
}
