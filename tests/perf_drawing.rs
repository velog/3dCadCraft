//! Speed of drawing views of large assemblies and complex parts
//! (`docs/PERF.md`, spec 4.7 §3.7): 5 000 to 100 000 components, with and
//! without the simplifications (Hide small parts, minimum edge length).
//!
//! `cargo test --no-default-features --test perf_drawing -- --nocapture`;
//! `CADCRAFT3D_NO_BUDGETS=1` reports without failing (shared CI runners).

use std::sync::Arc;
use std::time::Instant;

use cadcraft3d::drawing::ViewOrientation;
use cadcraft3d::drawing::hlr::{
    Instance, Simplify, ViewModel, assembly_model, part_model, view_lines_simplified,
};
use cadcraft3d::kernel::{Point3, Real, Transform3};
use cadcraft3d::part::{
    ExtentDirection, ExtrudeFeature, Operation, PartDocument, ProfileRef, SketchOptions,
    SketchPlacement,
};

/// A box with a circle cut through it or a boss on top.
fn box_with_circle(size: [Real; 3], x: Real, y: Real, r: Real, cut: bool, h: Real) -> PartDocument {
    let mut part = PartDocument::with_box("Box1", size).unwrap();
    let opts = SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    };
    let j = part
        .create_sketch(SketchPlacement::on_face("Box1:Top"), &opts)
        .unwrap();
    let c = part.sketches[j]
        .sketch
        .plane
        .project(&Point3::new(x, y, size[2]));
    part.sketches[j].sketch.add_circle(c, r).unwrap();
    let rg = part.sketches[j].sketch.regions();
    let mut ex = ExtrudeFeature::new(vec![ProfileRef::of(&rg[0], rg[0].interior_point())], h);
    if cut {
        ex.operation = Operation::Cut;
        ex.extent = cadcraft3d::part::Extent::ThroughAll;
        ex.direction = ExtentDirection::Flipped;
    } else {
        ex.operation = Operation::Join;
    }
    part.add_extrude(j, ex).unwrap();
    part
}

/// `n` occurrences on a grid: one plate (80 × 60 × 6, a hole) in every
/// 20, the others bolts (a 10 × 10 head with a Ø6 × 20 shank) — as large
/// assemblies are mostly fasteners.
fn assembly(n: usize, plate: &Arc<ViewModel>, bolt: &Arc<ViewModel>) -> Vec<Instance> {
    let side = (n as f64).sqrt().ceil() as usize;
    (0..n)
        .map(|k| {
            let (i, j) = ((k % side) as f64, (k / side) as f64);
            let is_plate = k % 20 == 0;
            Instance {
                id: k as u32 + 1,
                transform: Transform3::translation(
                    i * 90.0,
                    j * 70.0,
                    if is_plate { 0.0 } else { 6.0 },
                ),
                model: if is_plate {
                    plate.clone()
                } else {
                    bolt.clone()
                },
            }
        })
        .collect()
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

#[test]
fn drawing_views_of_large_models() {
    let plate = Arc::new(part_model(&box_with_circle(
        [80.0, 60.0, 6.0],
        40.0,
        30.0,
        8.0,
        true,
        1.0,
    )));
    let bolt = Arc::new(part_model(&box_with_circle(
        [10.0, 10.0, 2.0],
        5.0,
        5.0,
        3.0,
        false,
        20.0,
    )));
    let front = ViewOrientation::Front.frame();
    let iso = ViewOrientation::IsoTopRight.frame();
    // (case, budget ms, components, simplification)
    let keep = Simplify::default();
    let small = Simplify {
        min_part: 30.0,
        ..keep
    };
    let cases = [
        (
            "assembly view: 5 000 components, front + iso",
            6000.0,
            5_000,
            keep,
        ),
        (
            "assembly view: 20 000 components, front + iso",
            20000.0,
            20_000,
            keep,
        ),
        (
            "assembly view: 20 000, no hidden lines",
            15000.0,
            20_000,
            Simplify {
                no_hidden: true,
                ..keep
            },
        ),
        (
            "assembly view: 20 000, hide parts < 30 mm",
            4000.0,
            20_000,
            small,
        ),
        (
            "assembly view: 100 000, hide parts < 30 mm",
            15000.0,
            100_000,
            small,
        ),
        (
            "assembly view: 100 000, hide < 30 mm, edges < 2 mm",
            15000.0,
            100_000,
            Simplify {
                min_edge: 2.0,
                ..small
            },
        ),
    ];
    let mut over = Vec::new();
    for (case, budget, n, simplify) in cases {
        if let Ok(only) = std::env::var("CADCRAFT3D_PERF_ONLY")
            && !case.contains(only.as_str())
        {
            continue;
        }
        let inst = assembly(n, &plate, &bolt);
        let t = Instant::now();
        let model = assembly_model(&inst, simplify);
        let compose = ms(t);
        for frame in [&front, &iso] {
            let l = view_lines_simplified(&model, frame, None, simplify);
            assert!(!l.visible.is_empty(), "{case}");
        }
        let total = ms(t);
        let flag = if total > budget { "  OVER" } else { "" };
        println!(
            "{case:<56} {budget:>7.0} ms {total:>9.1} ms (compose {compose:.1} ms, {} meshes, {} edges){flag}",
            model.meshes.len(),
            model.edges.len()
        );
        if total > budget {
            over.push(case);
        }
    }
    // A complex part: a plate with 2 500 holes, three views.
    {
        let model = part_model(&holes_plate(50));
        let t = Instant::now();
        for frame in [&front, &ViewOrientation::Top.frame(), &iso] {
            let l = view_lines_simplified(&model, frame, None, Simplify::default());
            assert!(!l.visible.is_empty());
        }
        let total = ms(t);
        let budget = 8000.0;
        let flag = if total > budget { "  OVER" } else { "" };
        println!(
            "{:<56} {budget:>7.0} ms {total:>9.1} ms ({} edges){flag}",
            "complex part: 2 500-hole plate, front + top + iso",
            model.edges.len()
        );
        if total > budget {
            over.push("complex part");
        }
    }
    // A sheet full of toleranced dimensions: 2 000 labels with ISO fits
    // (Edit Dimension), laid out and placed every frame.
    {
        use cadcraft3d::drawing::dimtext::{DimFormat, Label, TolMethod};
        let look = cadcraft3d::drawing::style::AnnotationLook::default();
        let f = DimFormat {
            tol: TolMethod::FitsTolerances,
            hole: "H7".into(),
            ..Default::default()
        };
        let t = Instant::now();
        let mut n = 0;
        for k in 0..2000 {
            let l = Label::build(1.0 + k as f64 * 0.2, "Ø", &f, &look.dim, 3.5, 1.0);
            n += l.prims([100.0, 100.0], 0.3).len();
        }
        let total = ms(t);
        let budget = 50.0;
        let flag = if total > budget { "  OVER" } else { "" };
        println!(
            "{:<56} {budget:>7.0} ms {total:>9.1} ms ({n} prims){flag}",
            "dimension labels: 2 000 with H7 fits"
        );
        if total > budget {
            over.push("dimension labels");
        }
    }
    // A hole table of 2 500 holes in two kinds (a 50 × 50 grid of circles
    // seen face-on), with its tags.
    {
        use cadcraft3d::drawing::annot::Placement;
        use cadcraft3d::drawing::hlr::{ViewEdge, ViewLines};
        use cadcraft3d::drawing::hole_table::{self, HoleScope, HoleTable};
        let edges: Vec<ViewEdge> = (0..2500)
            .map(|k| {
                let c = [5.0 + 5.0 * (k % 50) as f64, 5.0 + 5.0 * (k / 50) as f64];
                let r = if k % 3 == 0 { 1.5 } else { 1.0 };
                ViewEdge {
                    name: Some(format!("Hole{}:{}:Bore", k % 3, k)),
                    a: [c[0] + r, c[1]],
                    b: [c[0] + r, c[1]],
                    circle: Some((c, r)),
                    full_circle: true,
                    straight: false,
                    visible: true,
                    note: Some(format!("Ø{} THRU", 2.0 * r)),
                    hole: Some(format!("Hole{}:{}", k % 3, k)),
                }
            })
            .collect();
        let lines = ViewLines {
            edges,
            min: [0.0, 0.0],
            max: [255.0, 255.0],
            ..Default::default()
        };
        let pl = Placement {
            position: [200.0, 150.0],
            scale: 0.5,
            center: lines.center(),
        };
        let ht = HoleTable::new(1, lines.min, [20.0, 280.0], HoleScope::View);
        let look = cadcraft3d::drawing::style::AnnotationLook::default();
        let t = Instant::now();
        let prims = hole_table::prims(&ht, &lines, pl, &look);
        let total = ms(t);
        let n = ht.tagged(&lines).len();
        assert_eq!(n, 2500);
        let budget = 50.0;
        let flag = if total > budget { "  OVER" } else { "" };
        println!(
            "{:<56} {budget:>7.0} ms {total:>9.1} ms ({n} holes, {} prims){flag}",
            "hole table: 2 500 holes of two kinds",
            prims.len()
        );
        if total > budget {
            over.push("hole table");
        }
    }
    // 1 000 chamfer notes on a view of 1 000 chamfered corners (4 000
    // straight edges): each note finds its two edges by name.
    {
        use cadcraft3d::drawing::AnnotationKind;
        use cadcraft3d::drawing::annot::{Placement, resolve};
        use cadcraft3d::drawing::chamfer_note::ChamferNote;
        use cadcraft3d::drawing::hlr::{ViewEdge, ViewLines};
        let line = |name: String, a: [f64; 2], b: [f64; 2]| ViewEdge {
            name: Some(name),
            a,
            b,
            circle: None,
            full_circle: false,
            straight: true,
            visible: true,
            note: None,
            hole: None,
        };
        let mut edges = Vec::new();
        for k in 0..1000 {
            let o = [10.0 * (k % 40) as f64, 10.0 * (k / 40) as f64];
            edges.push(line(
                format!("C{k}"),
                [o[0] + 2.0, o[1]],
                [o[0], o[1] + 2.0],
            ));
            edges.push(line(
                format!("R{k}"),
                [o[0] + 2.0, o[1]],
                [o[0] + 8.0, o[1]],
            ));
            edges.push(line(
                format!("U{k}"),
                [o[0], o[1] + 2.0],
                [o[0], o[1] + 8.0],
            ));
            edges.push(line(
                format!("X{k}"),
                [o[0] + 9.0, o[1]],
                [o[0] + 9.0, o[1] + 8.0],
            ));
        }
        let lines = ViewLines {
            edges,
            min: [0.0, 0.0],
            max: [400.0, 250.0],
            ..Default::default()
        };
        let pl = Placement {
            position: [200.0, 150.0],
            scale: 0.5,
            center: lines.center(),
        };
        let notes: Vec<AnnotationKind> = (0..1000)
            .map(|k| {
                AnnotationKind::ChamferNote(ChamferNote::new(
                    1,
                    &format!("C{k}"),
                    &format!("R{k}"),
                    [10.0, 10.0],
                ))
            })
            .collect();
        let view = |id: u32| (id == 1).then_some((pl, &lines));
        let t = Instant::now();
        let shown = notes.iter().filter(|n| resolve(n, &view).is_ok()).count();
        let total = ms(t);
        assert_eq!(shown, 1000);
        let budget = 50.0;
        let flag = if total > budget { "  OVER" } else { "" };
        println!(
            "{:<56} {budget:>7.0} ms {total:>9.1} ms{flag}",
            "chamfer notes: 1 000 on 4 000 edges"
        );
        if total > budget {
            over.push("chamfer notes");
        }
    }
    // Arrange Dimensions over 1 000 horizontal dimensions above a view.
    {
        use cadcraft3d::drawing::annot::Placement;
        use cadcraft3d::drawing::{DimKind, Dimension, PointRef, PointRole};
        let model = part_model(&box_with_circle(
            [80.0, 50.0, 10.0],
            20.0,
            10.0,
            5.0,
            true,
            10.0,
        ));
        let lines = view_lines_simplified(&model, &front, None, Simplify::default());
        let edges: Vec<String> = lines
            .edges
            .iter()
            .filter(|e| e.straight && e.visible)
            .filter_map(|e| e.name.clone())
            .collect();
        let pl = Placement {
            position: [200.0, 150.0],
            scale: 1.0,
            center: lines.center(),
        };
        let view = |v: u32| (v == 1).then_some((pl, &lines));
        let dims: Vec<Dimension> = (0..1000)
            .map(|k| {
                let e = edges[k % edges.len()].clone();
                Dimension {
                    dim: DimKind::Horizontal,
                    a: PointRef {
                        view: 1,
                        edge: e.clone(),
                        role: PointRole::Start,
                    },
                    b: Some(PointRef {
                        view: 1,
                        edge: e,
                        role: PointRole::End,
                    }),
                    text: [0.0, 40.0 + (k % 37) as f64],
                    precision: 2,
                    format: Default::default(),
                }
            })
            .collect();
        let refs: Vec<(u32, &Dimension)> = dims
            .iter()
            .enumerate()
            .map(|(k, d)| (k as u32, d))
            .collect();
        let t = Instant::now();
        let moved = cadcraft3d::drawing::arrange::arrange(&refs, &view, 10.0, 6.0);
        let total = ms(t);
        let budget = 100.0;
        let flag = if total > budget { "  OVER" } else { "" };
        println!(
            "{:<56} {budget:>7.0} ms {total:>9.1} ms ({} moved){flag}",
            "arrange dimensions: 1 000 above one view",
            moved.len()
        );
        if total > budget {
            over.push("arrange dimensions");
        }
    }
    if std::env::var_os("CADCRAFT3D_NO_BUDGETS").is_none() {
        assert!(over.is_empty(), "over budget: {over:?}");
    }
}

/// A plate with an n × n grid of Ø3 holes (one cut patterned).
fn holes_plate(n: u32) -> PartDocument {
    use cadcraft3d::part::{LineRef, PatternDirection, PatternFeature, PatternKind, PullRef};
    let s = 5.0;
    let size = s * f64::from(n) + 5.0;
    let mut part = box_with_circle([size, size, 5.0], 5.0, 5.0, 1.5, true, 1.0);
    let mut p = PatternFeature::rectangular();
    p.features = vec!["Extrusion1".into()];
    if let PatternKind::Rectangular { first, second } = &mut p.kind {
        (first.dir, first.count, first.spacing) = (Some(PullRef::Line(LineRef::Origin(0))), n, s);
        *second = Some(PatternDirection {
            dir: Some(PullRef::Line(LineRef::Origin(1))),
            count: n,
            spacing: s,
            ..PatternDirection::new()
        });
    }
    part.add_pattern(p).unwrap();
    part
}
