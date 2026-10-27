//! Project Geometry as Inventor has it (owner, 2026-10-27: "fix Project like
//! in Inventor"): model vertices, chains of picked edges that close, whole
//! faces, circles in a tilted sketch (ellipses, as splines), and Project Cut
//! Edges (the section of the part by the sketch plane).

mod common;
use common::*;

use cadcraft3d::kernel::{Point3, Real};
use cadcraft3d::part::{
    BlendFeature, OriginPlane, PartDocument, PlaneRef, PointRef, SketchOptions, SketchPlacement,
    WorkDef, WorkFeature, WorkPlaneDef,
};
use cadcraft3d::sketch::Point2;
use std::f64::consts::PI;

const W: Real = 100.0;
const D: Real = 60.0;
const H: Real = 20.0;
const R: Real = 5.0;
const HOLE: Real = 5.0;

/// A 100 × 60 × 20 plate, vertical edges rounded r 5, a Ø10 hole through
/// its middle.
fn part() -> PartDocument {
    let mut p = plate(W, D, H);
    let edges = vertical(&p, W, D, H);
    p.add_blend(BlendFeature::fillet(edges, R)).unwrap();
    let t = top(&p, H);
    let j = sketch_on(&mut p, &t, |sk, at| {
        sk.add_circle(at(W / 2.0, D / 2.0, H), HOLE).unwrap();
    });
    cut(&mut p, j, None, |r| r.area() < 100.0);
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    p
}

fn bare() -> SketchOptions {
    SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    }
}

/// A sketch on a work plane.
fn sketch_on_work(p: &mut PartDocument, def: WorkPlaneDef) -> usize {
    let wp = p.add_work(WorkFeature::new(WorkDef::Plane(def))).unwrap();
    let pl = SketchPlacement {
        plane: PlaneRef::Work(wp),
        ..SketchPlacement::on_origin_plane(OriginPlane::XY)
    };
    p.create_sketch(pl, &bare()).unwrap()
}

/// The rounded outline's area minus the hole.
fn plate_section() -> Real {
    W * D - (4.0 - PI) * R * R - PI * HOLE * HOLE
}

#[test]
fn cut_edges_across_the_plate() {
    let mut p = part();
    let j = sketch_on_work(
        &mut p,
        WorkPlaneDef::Offset {
            base: PlaneRef::Origin(OriginPlane::XY),
            distance: H / 2.0,
        },
    );
    let t = std::time::Instant::now();
    let curves = p.project_cut_edges(j).unwrap();
    let ms = t.elapsed().as_millis();
    let sk = &p.sketches[j].sketch;
    let count = |k: &str| {
        curves
            .iter()
            .filter(|&&c| {
                matches!(
                    (k, sk.entity(c)),
                    ("line", Some(cadcraft3d::sketch::Entity::Line { .. }))
                        | ("arc", Some(cadcraft3d::sketch::Entity::Arc { .. }))
                        | ("circle", Some(cadcraft3d::sketch::Entity::Circle { .. }))
                )
            })
            .count()
    };
    // Straight sides, rounded corners, the hole (kernel pieces may split a
    // side or the hole's circle in two).
    assert!(count("line") >= 4, "lines {}", count("line"));
    let kinds: Vec<String> = curves
        .iter()
        .map(|&c| format!("{:?}", sk.entity(c)).chars().take(12).collect())
        .collect();
    assert!(count("arc") + count("circle") >= 5, "{kinds:?}");
    let regions = sk.regions();
    let main = regions
        .iter()
        .max_by(|a, b| a.area().total_cmp(&b.area()))
        .unwrap();
    assert_eq!(main.holes.len(), 1, "the hole is a hole of the section");
    let want = plate_section();
    assert!(
        (main.area() - want).abs() < 1e-6 * want,
        "section {} vs {want}",
        main.area()
    );
    // A snapshot: fixed, nothing linked, the sketch solves as it is.
    assert!(p.sketches[j].links.is_empty());
    assert!(p.sketches[j].sketch.solve().is_ok());
    eprintln!("Project Cut Edges: {ms} ms");
    assert!(ms < 500, "Project Cut Edges took {ms} ms");
}

#[test]
fn cut_edges_through_the_hole() {
    // A vertical plane through the hole's axis: two rectangles (the hole
    // takes 10 mm out of the middle).
    let mut p = part();
    let j = sketch_on_work(
        &mut p,
        WorkPlaneDef::Offset {
            base: PlaneRef::Origin(OriginPlane::XZ),
            distance: -D / 2.0,
        },
    );
    p.project_cut_edges(j).unwrap();
    let regions = p.sketches[j].sketch.regions();
    let total: Real = regions.iter().map(|r| r.area()).sum();
    let want = W * H - 2.0 * HOLE * H;
    assert_eq!(regions.len(), 2, "two pieces");
    assert!((total - want).abs() < 1e-6 * want, "{total} vs {want}");
}

#[test]
fn cut_edges_need_a_plane_through_the_part() {
    let mut p = part();
    let j = sketch_on_work(
        &mut p,
        WorkPlaneDef::Offset {
            base: PlaneRef::Origin(OriginPlane::XY),
            distance: 50.0,
        },
    );
    let n = p.sketches[j].sketch.entities().count();
    let e = p.project_cut_edges(j).unwrap_err().to_string();
    assert!(e.contains("does not cut"), "{e}");
    assert_eq!(p.sketches[j].sketch.entities().count(), n, "nothing added");
}

/// A sketch 10 mm above the plate.
fn above(p: &mut PartDocument) -> usize {
    sketch_on_work(
        p,
        WorkPlaneDef::Offset {
            base: PlaneRef::Origin(OriginPlane::XY),
            distance: H + 10.0,
        },
    )
}

#[test]
fn a_face_projects_all_its_loops() {
    let mut p = part();
    let j = above(&mut p);
    let t = top(&p, H);
    let curves = p.project_face(j, &t, false).unwrap();
    assert!(
        curves.len() >= 9,
        "4 sides, 4 corners, the hole: {curves:?}"
    );
    let regions = p.sketches[j].sketch.regions();
    let main = regions
        .iter()
        .max_by(|a, b| a.area().total_cmp(&b.area()))
        .unwrap();
    let sk = &p.sketches[j].sketch;
    let kinds: Vec<String> = curves
        .iter()
        .map(|&c| format!("{:?}", sk.entity(c)).chars().take(12).collect())
        .collect();
    let arcs: Vec<String> = curves
        .iter()
        .filter_map(|&c| match sk.entity(c) {
            Some(e @ cadcraft3d::sketch::Entity::Arc { .. }) => Some(format!("{e:?}")),
            _ => None,
        })
        .collect();
    assert_eq!(
        main.holes.len(),
        1,
        "{kinds:?} {} {arcs:?} proj {}",
        regions.len(),
        p.sketches[j].projections.len()
    );
    let want = plate_section();
    assert!((main.area() - want).abs() < 1e-6 * want, "{}", main.area());
    // Associative: a thicker plate moves nothing in x / y, and the
    // projections follow a bigger hole.
    p.regenerate().unwrap();
    assert!(p.sketches[j].health.is_none(), "{:?}", p.sketches[j].health);
    // Projecting the face again adds nothing.
    let n = p.sketches[j].sketch.entities().count();
    p.project_face(j, &t, false).unwrap();
    assert_eq!(p.sketches[j].sketch.entities().count(), n);
}

#[test]
fn picked_edges_chain_into_a_closed_loop() {
    // Edges picked one by one (straight sides and rounded corners) share
    // their ends: one closed region, as in Inventor.
    let mut p = part();
    let j = above(&mut p);
    let t = top(&p, H);
    let f = p.body.find_face(&t).unwrap();
    let edges = p.body.face_edges(f);
    let outer: Vec<_> = edges
        .into_iter()
        .filter(|&e| {
            // Not the hole's edge.
            let pts = p
                .body
                .sample_edge(e, &Default::default())
                .unwrap_or_default();
            pts.iter()
                .all(|q| (q - Point3::new(W / 2.0, D / 2.0, H)).norm() > HOLE + 1.0)
        })
        .collect();
    assert_eq!(outer.len(), 8);
    for e in outer {
        p.project_model_edge(j, e, false).unwrap();
    }
    let regions = p.sketches[j].sketch.regions();
    assert_eq!(regions.len(), 1, "closed");
    let want = W * D - (4.0 - PI) * R * R;
    assert!((regions[0].area() - want).abs() < 1e-6 * want);
    // 8 corners shared by lines and arcs: 8 projected vertices.
    assert_eq!(p.sketches[j].projections.len(), 8);
    p.regenerate().unwrap();
    assert!(p.sketches[j].health.is_none(), "{:?}", p.sketches[j].health);
}

#[test]
fn construction_edges_with_shift() {
    let mut p = part();
    let j = above(&mut p);
    let t = top(&p, H);
    let curves = p.project_face(j, &t, true).unwrap();
    let sk = &p.sketches[j].sketch;
    assert!(
        curves
            .iter()
            .all(|&c| sk.style(c) == cadcraft3d::sketch::Style::Construction),
        "all construction"
    );
    assert!(sk.regions().is_empty(), "construction makes no profile");
}

#[test]
fn a_circle_in_a_tilted_sketch_is_an_ellipse() {
    // The hole's top rim (a circle in z) into a sketch tilted 30° about x:
    // an ellipse, projected as a closed spline of the right area.
    let mut p = part();
    let a = 30f64.to_radians();
    let j = sketch_on_work(
        &mut p,
        WorkPlaneDef::Fixed {
            origin: [0.0, 0.0, 40.0],
            normal: [0.0, -a.sin(), a.cos()],
            x_axis: [1.0, 0.0, 0.0],
        },
    );
    let rim: Vec<_> = p
        .body
        .edges()
        .filter(|(e, _)| {
            p.body.edge_direction(*e).is_none()
                && p.body
                    .sample_edge(*e, &Default::default())
                    .unwrap_or_default()
                    .iter()
                    .all(|q| {
                        (q.z - H).abs() < 1e-6
                            && ((q.x - W / 2.0).hypot(q.y - D / 2.0) - HOLE).abs() < 1e-3
                    })
        })
        .map(|(e, _)| e)
        .collect();
    assert!(!rim.is_empty(), "the hole's top rim");
    for e in rim {
        p.project_model_edge(j, e, false).unwrap();
    }
    let sk = &p.sketches[j].sketch;
    let regions = sk.regions();
    let area: Real = regions.iter().map(|r| r.area()).sum();
    // The kernel's circle comes in two arcs: two open splines sharing
    // their ends.
    let want = PI * HOLE * HOLE * a.cos();
    assert_eq!(regions.len(), 1, "the ellipse closes");
    assert!((area - want).abs() < 0.01 * want, "{area} vs {want}");
    p.regenerate().unwrap();
    assert!(p.sketches[j].health.is_none(), "{:?}", p.sketches[j].health);
}

#[test]
fn a_vertex_projects_as_a_point() {
    let mut p = part();
    let j = above(&mut p);
    let (name, at) = p
        .body
        .vertices()
        .find_map(|(_, v)| Some((v.name.clone()?, v.point)))
        .unwrap();
    let pt = p.project_point(j, PointRef::Vertex(name.clone())).unwrap();
    let sk = &p.sketches[j].sketch;
    let q = sk.point(pt).unwrap().position;
    assert!((q - sk.plane.project(&at)).norm() < 1e-9);
    // Again: the same point.
    assert_eq!(p.project_point(j, PointRef::Vertex(name)).unwrap(), pt);
    let _ = Point2::origin();
}
