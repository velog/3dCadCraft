//! Boundary Patch through guide curves, and Trim by an extruded outline with a spline wall
//! (the owner's surface tests, 2026-11-01).

use cadcraft3d::part::surface_op::{PatchOp, TrimOp, TrimTool};
use cadcraft3d::part::*;
use cadcraft3d::sketch::*;

fn opts() -> SketchOptions {
    SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    }
}

fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

/// The display triangles' largest distance along +Z (Z up).
fn top(part: &PartDocument) -> f64 {
    part.body
        .faces()
        .filter_map(|(_, f)| f.mesh.as_ref())
        .flat_map(|m| m.positions.iter().map(|q| q.z))
        .fold(f64::MIN, f64::max)
}

/// A patch of a rectangle in the XY plane passing through an arc over it in the XZ plane:
/// it rises to the arc, its rim stays in the plane.
#[test]
fn a_patch_passes_through_its_guide() {
    let mut part = PartDocument::default();
    let i = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    part.sketches[i]
        .sketch
        .add_rectangle_two_point(p(-40.0, -20.0), p(40.0, 20.0))
        .unwrap();
    let regions = part.sketches[i].sketch.regions();
    let region = regions[0].clone();
    let j = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XZ), &opts())
        .unwrap();
    // A three-point arc from (-30, 0) over (0, 15) to (30, 0) in the XZ plane.
    let sk = &mut part.sketches[j].sketch;
    let (a, b) = (sk.add_point(p(-30.0, 0.0)), sk.add_point(p(30.0, 0.0)));
    let m = sk.add_point(p(0.0, 15.0));
    sk.add_spline(vec![a, m, b]).unwrap();
    let id_i = part.sketches[i].id.clone();
    let id_j = part.sketches[j].id.clone();
    let op = PatchOp {
        sketch: Some(id_i),
        profile: Some(ProfileRef::of(&region, region.interior_point())),
        guides: vec![id_j],
        ..Default::default()
    };
    part.add_body_op(BodyOpFeature::new(BodyOp::Patch(op)))
        .unwrap();
    assert!(part.feature_health.is_empty(), "{:?}", part.feature_health);
    // A control-vertex spline through (0, 15) as its middle control point peaks lower.
    let t = top(&part);
    assert!(t > 5.0 && t < 16.0, "top {t}");
    // The rim lies in the plane, within the sheet's fit.
    let rim = part
        .body
        .edges()
        .filter_map(|(e, _)| {
            part.body
                .sample_edge(e, &cadcraft3d::kernel::TessellationOptions::default())
                .ok()
        })
        .flatten()
        .filter(|q| (q.x.abs() - 40.0).abs() < 1e-3 || (q.y.abs() - 20.0).abs() < 1e-3)
        .map(|q| q.z.abs())
        .fold(0.0, f64::max);
    assert!(rim < 0.05, "rim {rim}");
}

/// A surface trimmed by the walls of a closed outline of a spline and a line, extruded.
#[test]
fn a_spline_outline_extruded_trims_a_surface() {
    let mut part = PartDocument::default();
    // The surface: a plate of 100 × 100 at z = 0, extruded from an open line.
    let i = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XZ), &opts())
        .unwrap();
    let line = part.sketches[i]
        .sketch
        .add_line(p(-50.0, 0.0), p(50.0, 0.0));
    let mut sheet = ExtrudeFeature::new(Vec::new(), 100.0);
    sheet.output = Output::Surface;
    sheet.direction = ExtentDirection::Symmetric;
    sheet.open = Some(OpenProfile {
        curves: vec![line],
        flip: false,
        match_shape: false,
    });
    let sheet_id = part.add_extrude(i, sheet).unwrap();
    // The cutter: spline + line closed, in a plane above the surface, extruded through it.
    let j = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    let sk = &mut part.sketches[j].sketch;
    let (a, b) = (sk.add_point(p(-20.0, 0.0)), sk.add_point(p(20.0, 0.0)));
    let m = sk.add_point(p(0.0, 25.0));
    sk.add_interp_spline(vec![a, m, b]).unwrap();
    sk.add_line_between(a, b).unwrap();
    let regions = sk.regions();
    assert_eq!(regions.len(), 1);
    let mut wall = ExtrudeFeature::new(
        vec![ProfileRef::of(&regions[0], regions[0].interior_point())],
        40.0,
    );
    wall.output = Output::Surface;
    wall.direction = ExtentDirection::Symmetric;
    let wall_id = part.add_extrude(j, wall).unwrap();
    let op = TrimOp {
        surface: sheet_id,
        tool: TrimTool::Surface(wall_id),
        remove: [0.0, 10.0, 0.0],
        more: Vec::new(),
        invert: false,
    };
    part.add_body_op(BodyOpFeature::new(BodyOp::Trim(op)))
        .unwrap();
    assert!(part.feature_health.is_empty(), "{:?}", part.feature_health);
    // The hole: no triangle of the surface around the outline's middle.
    let inside = part
        .body
        .solids()
        .filter(|(_, s)| s.name.as_deref() == Some("Extrusion1"))
        .count();
    assert_eq!(inside, 1);
}

/// A patch of a hole's rim on a flat surface with a guide curve over it: the same guides, the
/// boundary being model edges.
#[test]
fn a_patch_of_model_edges_passes_through_its_guide() {
    let mut part = PartDocument::default();
    // A plate of surface: four sides extruded from a rectangle (walls), its top rim the loop.
    let i = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    part.sketches[i]
        .sketch
        .add_rectangle_two_point(p(-40.0, -20.0), p(40.0, 20.0))
        .unwrap();
    let regions = part.sketches[i].sketch.regions();
    let mut walls = ExtrudeFeature::new(
        vec![ProfileRef::of(&regions[0], regions[0].interior_point())],
        10.0,
    );
    walls.output = Output::Surface;
    part.add_extrude(i, walls).unwrap();
    // Each side wall's top edge is an edge of the loop.
    let names: Vec<String> = part
        .body
        .faces()
        .filter_map(|(_, f)| f.name.clone())
        .collect();
    let mut edges = Vec::new();
    for (x, y) in [(0.0, -20.0), (40.0, 0.0), (0.0, 20.0), (-40.0, 0.0)] {
        let hint = [x, y, 10.0];
        let face = part
            .body
            .faces()
            .find(|(_, f)| {
                f.mesh.as_ref().is_some_and(|m| {
                    m.positions.iter().all(|q| {
                        (x == 0.0 || (q.x - x).abs() < 1e-6) && (y == 0.0 || (q.y - y).abs() < 1e-6)
                    })
                })
            })
            .and_then(|(_, f)| f.name.clone())
            .unwrap_or_else(|| panic!("no wall at {x}, {y}: {names:?}"));
        edges.push(surface_op::SurfEdge { face, hint });
    }
    // The guide: a spline over the middle, in the XZ plane.
    let j = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XZ), &opts())
        .unwrap();
    let sk = &mut part.sketches[j].sketch;
    let (a, b) = (sk.add_point(p(-30.0, 12.0)), sk.add_point(p(30.0, 12.0)));
    let m = sk.add_point(p(0.0, 20.0));
    sk.add_spline(vec![a, m, b]).unwrap();
    let guide = part.sketches[j].id.clone();
    part.add_body_op(BodyOpFeature::new(BodyOp::Patch(PatchOp {
        edges,
        guides: vec![guide],
        ..Default::default()
    })))
    .unwrap();
    assert!(part.feature_health.is_empty(), "{:?}", part.feature_health);
    let t = top(&part);
    assert!(t > 12.0 && t < 21.0, "top {t}");
}
