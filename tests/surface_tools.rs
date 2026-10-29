//! Surface tools (`docs/spec/5.4-surfaces.md`, Inventor's 3D Model › Surface panel) and the
//! surfaces Extrude / Revolve make from open curves. Areas are checked against Pappus' rule (a
//! curve revolved about an axis sweeps its length × the path of its centroid) and plain
//! length × distance for extrusions; measured areas use the display triangles (inscribed, so
//! curved ones run a little low).

mod common;

use cadcraft3d::kernel::Real;
use cadcraft3d::part::{
    AxisRef, ExtrudeFeature, OpenProfile, OriginPlane, Output, PartDocument, RevolveFeature,
    SketchPlacement,
};
use cadcraft3d::sketch::{EntityId, Point2};
use common::*;
use std::f64::consts::PI;

/// Area of the surface bodies (display triangles).
fn sheet_area(part: &PartDocument) -> Real {
    part.exact_bodies
        .0
        .iter()
        .filter(|s| s.is_open())
        .filter_map(|s| s.triangles().ok())
        .flatten()
        .map(|[a, b, c]| (b - a).cross(&(c - a)).norm() / 2.0)
        .sum()
}

fn surfaces(part: &PartDocument) -> usize {
    part.exact_bodies.0.iter().filter(|s| s.is_open()).count()
}

/// A sketch on XZ (sketch x = world X, sketch y = world Z) with the curves
/// `draw` makes; returns the sketch index and the curves.
fn xz_sketch(
    part: &mut PartDocument,
    draw: impl FnOnce(&mut cadcraft3d::sketch::Sketch) -> Vec<EntityId>,
) -> (usize, Vec<EntityId>) {
    let i = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XZ), &opts())
        .unwrap();
    let curves = draw(&mut part.sketches[i].sketch);
    (i, curves)
}

fn revolve_open(curves: Vec<EntityId>) -> RevolveFeature {
    let mut rv = RevolveFeature::new(Vec::new(), AxisRef::Origin(2));
    rv.output = Output::Surface;
    rv.open = curves;
    rv
}

fn close(got: Real, want: Real, rel: Real) {
    assert!(
        (got - want).abs() <= rel * want.abs(),
        "{got} vs {want} (rel {rel})"
    );
}

#[test]
fn revolve_an_open_line_arc_spline_and_chain_as_surfaces() {
    // A line parallel to the axis: a cylinder, 2π·5·10.
    let mut p = PartDocument::default();
    let (i, c) = xz_sketch(&mut p, |sk| {
        vec![sk.add_line(Point2::new(5.0, 0.0), Point2::new(5.0, 10.0))]
    });
    p.add_revolve(i, revolve_open(c)).unwrap();
    assert!(
        p.exact && p.feature_health.is_empty(),
        "{:?}",
        p.feature_health
    );
    assert_eq!(surfaces(&p), 1);
    close(sheet_area(&p), 2.0 * PI * 5.0 * 10.0, 5e-3);

    // A quarter circle from the axis: a hemisphere, 2π·r².
    let mut p = PartDocument::default();
    let (i, c) = xz_sketch(&mut p, |sk| {
        let o = sk.add_point(Point2::new(0.0, 0.0));
        let a = sk.add_point(Point2::new(10.0, 0.0));
        let b = sk.add_point(Point2::new(0.0, 10.0));
        vec![sk.add_arc(o, a, b).unwrap()]
    });
    p.add_revolve(i, revolve_open(c)).unwrap();
    assert!(
        p.exact && p.feature_health.is_empty(),
        "{:?}",
        p.feature_health
    );
    close(sheet_area(&p), 2.0 * PI * 100.0, 1e-2);

    // A chain from the axis: the line on the axis sweeps nothing, the top
    // line a disc, the side a cylinder (a cup upside down).
    let mut p = PartDocument::default();
    let (i, c) = xz_sketch(&mut p, |sk| {
        vec![
            sk.add_line(Point2::new(0.0, 0.0), Point2::new(0.0, 10.0)),
            sk.add_line(Point2::new(0.0, 10.0), Point2::new(5.0, 10.0)),
            sk.add_line(Point2::new(5.0, 10.0), Point2::new(5.0, 0.0)),
        ]
    });
    p.add_revolve(i, revolve_open(c)).unwrap();
    assert!(
        p.exact && p.feature_health.is_empty(),
        "{:?}",
        p.feature_health
    );
    close(sheet_area(&p), PI * 25.0 + 2.0 * PI * 5.0 * 10.0, 5e-3);

    // A spline (control points (5, 0), (9, 5), (5, 10)): Pappus by sampling
    // the curve.
    let mut p = PartDocument::default();
    let (i, c) = xz_sketch(&mut p, |sk| {
        let pts = [(5.0, 0.0), (9.0, 5.0), (5.0, 10.0)]
            .map(|(x, y)| sk.add_point(Point2::new(x, y)))
            .to_vec();
        vec![sk.add_spline(pts).unwrap()]
    });
    p.add_revolve(i, revolve_open(c)).unwrap();
    assert!(
        p.exact && p.feature_health.is_empty(),
        "{:?}",
        p.feature_health
    );
    let q = |t: Real| {
        let (a, b, c) = ((1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t), t * t);
        Point2::new(5.0 * a + 9.0 * b + 5.0 * c, 0.0 * a + 5.0 * b + 10.0 * c)
    };
    let n = 2000;
    let want: Real = (0..n)
        .map(|k| {
            let (u, v) = (q(k as Real / n as Real), q((k + 1) as Real / n as Real));
            2.0 * PI * (u.x + v.x) / 2.0 * (v - u).norm()
        })
        .sum();
    close(sheet_area(&p), want, 1e-2);
}

#[test]
fn revolve_open_curves_need_surface_output() {
    let mut p = PartDocument::default();
    let (i, c) = xz_sketch(&mut p, |sk| {
        vec![sk.add_line(Point2::new(5.0, 0.0), Point2::new(5.0, 10.0))]
    });
    let mut rv = revolve_open(c);
    rv.output = Output::Solid;
    let e = p.add_revolve(i, rv).unwrap_err();
    assert!(e.to_string().contains("Surface"), "{e}");
    assert!(p.features.is_empty());
}

#[test]
fn extrude_an_open_spline_as_a_surface() {
    // Control points (0, 0), (10, 8), (20, 0) on XY extruded 15 along Z:
    // the curve's length × 15.
    let mut p = PartDocument::default();
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    let sk = &mut p.sketches[i].sketch;
    let pts = [(0.0, 0.0), (10.0, 8.0), (20.0, 0.0)]
        .map(|(x, y)| sk.add_point(Point2::new(x, y)))
        .to_vec();
    let s = sk.add_spline(pts).unwrap();
    let mut f = ExtrudeFeature::new(Vec::new(), 15.0);
    f.output = Output::Surface;
    f.open = Some(OpenProfile {
        curves: vec![s],
        flip: false,
        match_shape: false,
    });
    p.add_extrude(i, f).unwrap();
    assert!(
        p.exact && p.feature_health.is_empty(),
        "{:?}",
        p.feature_health
    );
    let q = |t: Real| {
        let (b, c) = (2.0 * t * (1.0 - t), t * t);
        Point2::new(10.0 * b + 20.0 * c, 8.0 * b)
    };
    let n = 2000;
    let len: Real = (0..n)
        .map(|k| (q((k + 1) as Real / n as Real) - q(k as Real / n as Real)).norm())
        .sum();
    close(sheet_area(&p), 15.0 * len, 5e-3);
}

#[test]
fn a_revolved_spline_shows_as_one_face() {
    // The kernel makes a full revolve in sectors; their seams are not
    // drawn and a pick takes the whole surface.
    let mut p = PartDocument::default();
    let (i, c) = xz_sketch(&mut p, |sk| {
        let pts = [(5.0, 0.0), (9.0, 5.0), (5.0, 10.0)]
            .map(|(x, y)| sk.add_point(Point2::new(x, y)))
            .to_vec();
        vec![sk.add_spline(pts).unwrap()]
    });
    p.add_revolve(i, revolve_open(c)).unwrap();
    let faces: Vec<_> = p.body.faces().map(|(f, _)| f).collect();
    assert!(faces.len() > 1, "sectors: {}", faces.len());
    assert_eq!(p.body.smooth_group(faces[0]).len(), faces.len());
    // Only the two rim circles stay (their arcs).
    for e in p.body.visible_edges() {
        let edge = p.body.edge(e).unwrap();
        for v in [edge.start, edge.end] {
            let z = p.body.vertex(v).unwrap().point.z;
            assert!(
                z.abs() < 1e-6 || (z - 10.0).abs() < 1e-6,
                "a seam is drawn (z {z})"
            );
        }
    }
}

// ------------------------------------------------------------ surface tools ---

use cadcraft3d::part::surface_op::{
    ExtendOp, PatchOp, ReplaceFaceOp, ReplaceWith, RuledKind, RuledOp, SculptOp, SculptOperation,
    StitchOp, SurfEdge, TrimOp, TrimTool,
};
use cadcraft3d::part::{
    BodyOp, BodyOpFeature, FaceRef, LineRef, PlaneRef, ProfileRef, WorkDef, WorkFeature,
    WorkPlaneDef,
};

fn op(p: &mut PartDocument, o: BodyOp) -> String {
    let id = p.add_body_op(BodyOpFeature::new(o)).unwrap();
    assert!(p.exact, "exact kernel");
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    id
}

fn solid_volume(p: &PartDocument) -> Real {
    p.exact_bodies
        .0
        .iter()
        .filter(|s| !s.is_open())
        .filter_map(|s| s.volume_at(1e-4))
        .sum()
}

/// A surface extruded `h` along +Z from lines through `pts` on XY (a closed
/// polygon gives its walls).
fn wall(p: &mut PartDocument, pts: &[(Real, Real)], h: Real) -> String {
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    let sk = &mut p.sketches[i].sketch;
    let curves: Vec<EntityId> = pts
        .windows(2)
        .map(|w| sk.add_line(Point2::new(w[0].0, w[0].1), Point2::new(w[1].0, w[1].1)))
        .collect();
    let closed = pts.first() == pts.last();
    let mut f = if closed {
        let r = p.sketches[i].sketch.regions();
        ExtrudeFeature::new(vec![ProfileRef::of(&r[0], r[0].interior_point())], h)
    } else {
        let mut f = ExtrudeFeature::new(Vec::new(), h);
        f.open = Some(OpenProfile {
            curves,
            flip: false,
            match_shape: false,
        });
        f
    };
    f.output = Output::Surface;
    p.add_extrude(i, f).unwrap()
}

/// A planar patch of a rectangle on XY.
fn patch_rect(p: &mut PartDocument, a: (Real, Real), b: (Real, Real)) -> String {
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    p.sketches[i]
        .sketch
        .add_rectangle_two_point(Point2::new(a.0, a.1), Point2::new(b.0, b.1))
        .unwrap();
    let r = p.sketches[i].sketch.regions();
    let id = p.sketches[i].id.clone();
    op(
        p,
        BodyOp::Patch(PatchOp {
            edges: Vec::new(),
            sketch: Some(id),
            profile: Some(ProfileRef::of(&r[0], r[0].interior_point())),
            solid: false,
        }),
    )
}

/// The name of the face of the display body nearest `q`.
fn face_near(p: &PartDocument, q: [Real; 3]) -> String {
    let q = cadcraft3d::kernel::Point3::new(q[0], q[1], q[2]);
    let d = |f: &cadcraft3d::kernel::Face| {
        f.mesh.as_ref().map_or(Real::MAX, |m| {
            m.positions
                .iter()
                .map(|x| (x - q).norm())
                .fold(Real::MAX, Real::min)
        })
    };
    p.body
        .faces()
        .filter(|(_, f)| f.name.is_some())
        .min_by(|a, b| d(a.1).total_cmp(&d(b.1)))
        .and_then(|(_, f)| f.name.clone())
        .unwrap()
}

fn edge(p: &PartDocument, on_face: [Real; 3], at: [Real; 3]) -> SurfEdge {
    SurfEdge {
        face: face_near(p, on_face),
        hint: at,
    }
}

#[test]
fn stitch_two_walls_into_one_surface() {
    let mut p = PartDocument::default();
    let a = wall(&mut p, &[(0.0, 0.0), (10.0, 0.0)], 10.0);
    let b = wall(&mut p, &[(10.0, 0.0), (10.0, 10.0)], 10.0);
    assert_eq!(surfaces(&p), 2);
    op(
        &mut p,
        BodyOp::Stitch(StitchOp {
            surfaces: vec![a, b],
            tolerance: 0.001,
            keep_surface: false,
        }),
    );
    assert_eq!(surfaces(&p), 1);
    close(sheet_area(&p), 200.0, 1e-6);
}

#[test]
fn walls_and_two_patches_stitch_into_a_solid() {
    // The walls of a 20 x 10 rectangle, 5 high; Boundary Patch of the
    // bottom and the top rims; Stitch closes a 20 x 10 x 5 box.
    let mut p = PartDocument::default();
    let w = wall(
        &mut p,
        &[
            (0.0, 0.0),
            (20.0, 0.0),
            (20.0, 10.0),
            (0.0, 10.0),
            (0.0, 0.0),
        ],
        5.0,
    );
    let rim = |p: &PartDocument, z: Real| -> Vec<SurfEdge> {
        [(10.0, 0.0), (20.0, 5.0), (10.0, 10.0), (0.0, 5.0)]
            .iter()
            .map(|&(x, y)| edge(p, [x, y, 2.5], [x, y, z]))
            .collect()
    };
    let e0 = rim(&p, 0.0);
    let bottom = op(
        &mut p,
        BodyOp::Patch(PatchOp {
            edges: e0,
            sketch: None,
            profile: None,
            solid: false,
        }),
    );
    let e5 = rim(&p, 5.0);
    let top = op(
        &mut p,
        BodyOp::Patch(PatchOp {
            edges: e5,
            sketch: None,
            profile: None,
            solid: false,
        }),
    );
    assert_eq!(surfaces(&p), 3);
    op(
        &mut p,
        BodyOp::Stitch(StitchOp {
            surfaces: vec![w, bottom, top],
            tolerance: 0.001,
            keep_surface: false,
        }),
    );
    assert_eq!(surfaces(&p), 0);
    close(solid_volume(&p), 1000.0, 1e-6);
}

#[test]
fn boundary_patch_of_a_sketch_region() {
    let mut p = PartDocument::default();
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    p.sketches[i]
        .sketch
        .add_circle(Point2::new(0.0, 0.0), 5.0)
        .unwrap();
    let r = p.sketches[i].sketch.regions();
    let id = p.sketches[i].id.clone();
    op(
        &mut p,
        BodyOp::Patch(PatchOp {
            edges: Vec::new(),
            sketch: Some(id),
            profile: Some(ProfileRef::of(&r[0], r[0].interior_point())),
            solid: false,
        }),
    );
    assert_eq!(surfaces(&p), 1);
    close(sheet_area(&p), PI * 25.0, 5e-3);
}

#[test]
fn trim_a_surface_by_a_plane_and_by_a_surface() {
    // A strip from x = -10 to 10, 10 high, trimmed by the YZ plane: the
    // piece at x = 5 goes.
    let mut p = PartDocument::default();
    let s = wall(&mut p, &[(-10.0, 0.0), (10.0, 0.0)], 10.0);
    op(
        &mut p,
        BodyOp::Trim(TrimOp {
            surface: s.clone(),
            tool: TrimTool::Plane(PlaneRef::Origin(OriginPlane::YZ)),
            remove: [5.0, 0.0, 5.0],
            more: Vec::new(),
            invert: false,
        }),
    );
    close(sheet_area(&p), 100.0, 1e-6);
    // By another surface (a wall across at x = -4).
    let t = wall(&mut p, &[(-4.0, -5.0), (-4.0, 5.0)], 10.0);
    op(
        &mut p,
        BodyOp::Trim(TrimOp {
            surface: s,
            tool: TrimTool::Surface(t),
            remove: [-8.0, 0.0, 5.0],
            more: Vec::new(),
            invert: false,
        }),
    );
    // The strip keeps x in [-4, 0] (40) and the tool wall stays (100).
    close(sheet_area(&p), 40.0 + 100.0, 1e-6);
}

#[test]
fn extend_a_planar_patch() {
    let mut p = PartDocument::default();
    patch_rect(&mut p, (0.0, 0.0), (10.0, 10.0));
    let e = edge(&p, [5.0, 5.0, 0.0], [10.0, 5.0, 0.0]);
    op(
        &mut p,
        BodyOp::Extend(ExtendOp {
            edges: vec![e],
            distance: 5.0,
            to: None,
        }),
    );
    close(sheet_area(&p), 150.0, 1e-6);
}

#[test]
fn ruled_surfaces_from_a_box_edge() {
    for (kind, vector) in [
        (RuledKind::Normal, None),
        (RuledKind::Tangent, None),
        (RuledKind::Sweep, Some(LineRef::Origin(2))),
    ] {
        let mut p = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
        op(
            &mut p,
            BodyOp::Ruled(RuledOp {
                kind,
                edges: vec![SurfEdge {
                    face: "Box1:Top".into(),
                    hint: [20.0, 0.0, 20.0],
                }],
                distance: 5.0,
                flip: false,
                vector,
                angle: 0.0,
                alternate: false,
            }),
        );
        assert_eq!(surfaces(&p), 1, "{kind:?}");
        close(sheet_area(&p), 200.0, 1e-6);
        // Normal / Sweep: above the top; Tangent: out beyond y = 0.
        let (lo, hi) = cadcraft3d::kernel::exact::bounds(
            p.exact_bodies.0.iter().find(|s| s.is_open()).unwrap(),
        )
        .unwrap();
        match kind {
            RuledKind::Normal | RuledKind::Sweep => {
                assert!(lo.z > 19.9 && hi.z < 25.1, "{kind:?} {lo} {hi}")
            }
            RuledKind::Tangent => assert!(hi.y < 0.1 && lo.y > -5.1, "{lo} {hi}"),
        }
    }
}

#[test]
fn replace_the_top_face_by_a_plane_above_and_below() {
    for (z, want) in [(25.0, 40.0 * 30.0 * 25.0), (15.0, 40.0 * 30.0 * 15.0)] {
        let mut p = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
        let wp = p
            .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
                base: PlaneRef::Origin(OriginPlane::XY),
                distance: z,
            })))
            .unwrap();
        op(
            &mut p,
            BodyOp::ReplaceFace(ReplaceFaceOp {
                faces: vec![FaceRef {
                    face: "Box1:Top".into(),
                    hint: [20.0, 15.0, 20.0],
                }],
                new: ReplaceWith::Plane(PlaneRef::Work(wp)),
            }),
        );
        close(solid_volume(&p), want, 1e-6);
        // The new top is the feature's face; a Ruled Surface on its front
        // edge (picked half way along it) runs along that edge.
        let names: Vec<String> = p.body.faces().filter_map(|(_, f)| f.name.clone()).collect();
        assert!(names.iter().any(|n| n.ends_with(":Face")), "{names:?}");
        op(
            &mut p,
            BodyOp::Ruled(RuledOp {
                kind: RuledKind::Normal,
                edges: vec![SurfEdge {
                    face: "Box1:Front".into(),
                    hint: [20.0, 0.0, z],
                }],
                distance: 5.0,
                flip: false,
                vector: None,
                angle: 0.0,
                alternate: false,
            }),
        );
        let (lo, hi) = cadcraft3d::kernel::exact::bounds(
            p.exact_bodies.0.iter().find(|s| s.is_open()).unwrap(),
        )
        .unwrap();
        assert!(
            (hi.x - lo.x - 40.0).abs() < 0.1 && (hi.z - z).abs() < 0.1,
            "{lo} {hi}"
        );
    }
}

#[test]
fn sculpt_removes_a_side() {
    // A wall across the 40 x 30 x 20 box at y = 15 (x -10..50, z -5..25):
    // Remove takes the front side (the side the wall faces) - half the box.
    let mut p = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
    let wp = p
        .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
            base: PlaneRef::Origin(OriginPlane::XY),
            distance: -5.0,
        })))
        .unwrap();
    let i = p
        .create_sketch(
            SketchPlacement {
                plane: PlaneRef::Work(wp),
                ..Default::default()
            },
            &opts(),
        )
        .unwrap();
    let l = p.sketches[i]
        .sketch
        .add_line(Point2::new(-10.0, 15.0), Point2::new(50.0, 15.0));
    let mut f = ExtrudeFeature::new(Vec::new(), 30.0);
    f.output = Output::Surface;
    f.open = Some(OpenProfile {
        curves: vec![l],
        flip: false,
        match_shape: false,
    });
    let s = p.add_extrude(i, f).unwrap();
    op(
        &mut p,
        BodyOp::Sculpt(SculptOp {
            surfaces: vec![s],
            operation: SculptOperation::Remove,
            solids: Vec::new(),
            side: None,
            flip: false,
        }),
    );
    close(solid_volume(&p), 12000.0, 1e-6);
}

/// A hemisphere surface r 8 about a vertical axis through (20, 15), its
/// rim at height `z` (a quarter arc revolved).
fn dome(p: &mut PartDocument, z: Real) -> String {
    let wp = p
        .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
            base: PlaneRef::Origin(OriginPlane::XZ),
            distance: -15.0,
        })))
        .unwrap();
    let i = p
        .create_sketch(
            SketchPlacement {
                plane: PlaneRef::Work(wp),
                ..Default::default()
            },
            &opts(),
        )
        .unwrap();
    let sk = &mut p.sketches[i].sketch;
    let pl = sk.plane.clone();
    // The sketch point of a world point (the plane's frame may run either way).
    let at = |x: Real, z: Real| {
        let q = pl.project(&cadcraft3d::kernel::Point3::new(x, 15.0, z));
        Point2::new(q.x, q.y)
    };
    let o = sk.add_point(at(20.0, z));
    let a = sk.add_point(at(28.0, z));
    let b = sk.add_point(at(20.0, z + 8.0));
    // The arc runs counter-clockwise in the sketch: from whichever end makes
    // it the quarter, not three quarters.
    let arc = match sk.add_arc(o, a, b) {
        Ok(x) if sk.circle_geometry(x).is_some() => x,
        _ => panic!("arc"),
    };
    let ax = sk.add_line(at(20.0, z - 10.0), at(20.0, z + 20.0));
    let mut rv = RevolveFeature::new(Vec::new(), AxisRef::SketchLine(ax));
    rv.output = Output::Surface;
    rv.open = vec![arc];
    let id = p.add_revolve(i, rv).unwrap();
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    id
}

#[test]
fn sculpt_adds_a_box_of_surfaces_and_makes_a_new_solid() {
    // Walls of a 20 x 10 rectangle at (30..50, 10..20), 10 high from z = 15,
    // closed by their rims' patches: Add joins the part above the box.
    let mut p = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
    let wp = p
        .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
            base: PlaneRef::Origin(OriginPlane::XY),
            distance: 15.0,
        })))
        .unwrap();
    let i = p
        .create_sketch(
            SketchPlacement {
                plane: PlaneRef::Work(wp),
                ..Default::default()
            },
            &opts(),
        )
        .unwrap();
    p.sketches[i]
        .sketch
        .add_rectangle_two_point(Point2::new(30.0, 10.0), Point2::new(50.0, 20.0))
        .unwrap();
    let r = p.sketches[i].sketch.regions();
    let mut f = ExtrudeFeature::new(vec![ProfileRef::of(&r[0], r[0].interior_point())], 10.0);
    f.output = Output::Surface;
    let w = p.add_extrude(i, f).unwrap();
    op(
        &mut p,
        BodyOp::Sculpt(SculptOp {
            surfaces: vec![w.clone()],
            operation: SculptOperation::Add,
            solids: Vec::new(),
            side: None,
            flip: false,
        }),
    );
    // Above the box: 20 x 10 x 5; beside it (x 40..50, z 15..20): 10 x 10 x 5.
    close(solid_volume(&p), 24000.0 + 1000.0 + 500.0, 1e-6);
}

#[test]
#[ignore = "a revolved dome closed by Stitch does not go through the kernel's Booleans yet"]
fn sculpt_adds_a_dome_and_makes_a_new_solid() {
    let cap = |h: Real| PI * h * h * (3.0 * 8.0 - h) / 3.0;
    // The dome's rim 5 below the top: the cap above the top is added.
    let mut p = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
    let d = dome(&mut p, 15.0);
    let area = sheet_area(&p);
    assert!(
        (area - 2.0 * PI * 64.0).abs() < 0.01 * 2.0 * PI * 64.0
            || (area - 6.0 * PI * 64.0).abs() < 0.01 * 6.0 * PI * 64.0,
        "{area}"
    );
    op(
        &mut p,
        BodyOp::Sculpt(SculptOp {
            surfaces: vec![d],
            operation: SculptOperation::Add,
            solids: Vec::new(),
            side: None,
            flip: false,
        }),
    );
    close(solid_volume(&p), 24000.0 + cap(3.0), 1e-2);

    // New Solid: the dome closed by its rim.
    let mut p = PartDocument::default();
    let d = dome(&mut p, 30.0);
    op(
        &mut p,
        BodyOp::Sculpt(SculptOp {
            surfaces: vec![d],
            operation: SculptOperation::NewSolid,
            solids: Vec::new(),
            side: None,
            flip: false,
        }),
    );
    close(solid_volume(&p), 2.0 / 3.0 * PI * 512.0, 1e-2);
}

#[test]
fn every_surface_command_survives_save_and_load() {
    // Each operation's JSON is read back into the same feature (a field
    // named like the operation's tag, `kind`, broke loading once).
    let ops = vec![
        BodyOp::Stitch(StitchOp {
            surfaces: vec!["A".into(), "B".into()],
            tolerance: 0.001,
            keep_surface: true,
        }),
        BodyOp::Patch(PatchOp {
            edges: vec![SurfEdge {
                face: "A:Side:0".into(),
                hint: [1.0, 2.0, 3.0],
            }],
            sketch: None,
            profile: None,
            solid: false,
        }),
        BodyOp::Trim(TrimOp {
            surface: "A".into(),
            tool: TrimTool::Plane(PlaneRef::Origin(OriginPlane::YZ)),
            remove: [1.0, 0.0, 0.0],
            more: Vec::new(),
            invert: true,
        }),
        BodyOp::Extend(ExtendOp {
            edges: Vec::new(),
            distance: 2.0,
            to: Some(PlaneRef::Origin(OriginPlane::YZ)),
        }),
        BodyOp::Ruled(RuledOp {
            kind: RuledKind::Sweep,
            edges: Vec::new(),
            distance: 3.0,
            flip: true,
            vector: Some(LineRef::Origin(2)),
            angle: 0.5,
            alternate: false,
        }),
        BodyOp::ReplaceFace(ReplaceFaceOp {
            faces: Vec::new(),
            new: ReplaceWith::Surface("S".into()),
        }),
        BodyOp::Sculpt(SculptOp {
            surfaces: vec!["S".into()],
            operation: SculptOperation::Remove,
            solids: vec!["Box1".into()],
            side: Some([0.0, 1.0, 2.0]),
            flip: true,
        }),
    ];
    for o in ops {
        let f = cadcraft3d::part::Feature::BodyOp(BodyOpFeature::new(o));
        let json = serde_json::to_string(&f).unwrap();
        let back: cadcraft3d::part::Feature =
            serde_json::from_str(&json).unwrap_or_else(|e| panic!("{json}: {e}"));
        assert_eq!(back, f, "{json}");
    }
}

#[test]
fn trim_a_surface_by_a_sketch_line() {
    // The strip x = -10..10 cut by a sketch line across it at x = 3 (on XY,
    // swept along Z): the piece at x = 8 goes, 13 x 10 stays.
    let mut p = PartDocument::default();
    let s = wall(&mut p, &[(-10.0, 0.0), (10.0, 0.0)], 10.0);
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    p.sketches[i]
        .sketch
        .add_line(Point2::new(3.0, -5.0), Point2::new(3.0, 5.0));
    let sid = p.sketches[i].id.clone();
    op(
        &mut p,
        BodyOp::Trim(TrimOp {
            surface: s,
            tool: TrimTool::Sketch(sid),
            remove: [8.0, 0.0, 5.0],
            more: Vec::new(),
            invert: false,
        }),
    );
    close(sheet_area(&p), 130.0, 1e-6);
}

#[test]
fn extend_a_planar_patch_up_to_a_plane() {
    // The 10 x 10 patch's edge at x = 10 up to the plane x = 17.
    let mut p = PartDocument::default();
    patch_rect(&mut p, (0.0, 0.0), (10.0, 10.0));
    let wp = p
        .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
            base: PlaneRef::Origin(OriginPlane::YZ),
            distance: 17.0,
        })))
        .unwrap();
    let e = edge(&p, [5.0, 5.0, 0.0], [10.0, 5.0, 0.0]);
    op(
        &mut p,
        BodyOp::Extend(ExtendOp {
            edges: vec![e],
            distance: 0.0,
            to: Some(PlaneRef::Work(wp)),
        }),
    );
    close(sheet_area(&p), 170.0, 1e-6);
}

#[test]
fn a_ruled_surface_leans_by_its_angle() {
    // 30 degrees outward from the top's normal along the front top edge:
    // the strip reaches 5 cos 30 up and 5 sin 30 out (y < 0).
    let mut p = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
    op(
        &mut p,
        BodyOp::Ruled(RuledOp {
            kind: RuledKind::Normal,
            edges: vec![SurfEdge {
                face: "Box1:Top".into(),
                hint: [20.0, 0.0, 20.0],
            }],
            distance: 5.0,
            flip: false,
            vector: None,
            angle: 30f64.to_radians(),
            alternate: false,
        }),
    );
    close(sheet_area(&p), 200.0, 1e-6);
    let (lo, hi) =
        cadcraft3d::kernel::exact::bounds(p.exact_bodies.0.iter().find(|s| s.is_open()).unwrap())
            .unwrap();
    assert!(
        (hi.z - 20.0 - 5.0 * 30f64.to_radians().cos()).abs() < 0.1,
        "{hi}"
    );
    assert!((lo.y + 2.5).abs() < 0.1, "{lo}");
}

#[test]
fn boundary_patch_output_solid_closes_an_open_box() {
    // Walls of a 20 x 10 rectangle (5 high) and a patch of the bottom rim
    // stitched: an open box. A Patch of the top rim with Output Solid closes it.
    let mut p = PartDocument::default();
    let w = wall(
        &mut p,
        &[
            (0.0, 0.0),
            (20.0, 0.0),
            (20.0, 10.0),
            (0.0, 10.0),
            (0.0, 0.0),
        ],
        5.0,
    );
    let rim = |p: &PartDocument, z: Real| -> Vec<SurfEdge> {
        [(10.0, 0.0), (20.0, 5.0), (10.0, 10.0), (0.0, 5.0)]
            .iter()
            .map(|&(x, y)| edge(p, [x, y, 2.5], [x, y, z]))
            .collect()
    };
    let e0 = rim(&p, 0.0);
    let bottom = op(
        &mut p,
        BodyOp::Patch(PatchOp {
            edges: e0,
            sketch: None,
            profile: None,
            solid: false,
        }),
    );
    op(
        &mut p,
        BodyOp::Stitch(StitchOp {
            surfaces: vec![w, bottom],
            tolerance: 0.001,
            keep_surface: false,
        }),
    );
    assert_eq!(surfaces(&p), 1);
    let e5 = rim(&p, 5.0);
    op(
        &mut p,
        BodyOp::Patch(PatchOp {
            edges: e5,
            sketch: None,
            profile: None,
            solid: true,
        }),
    );
    assert_eq!(surfaces(&p), 0);
    close(solid_volume(&p), 1000.0, 1e-6);
}

#[test]
fn stitch_refuses_surfaces_apart_more_than_the_tolerance() {
    // Two walls 0.05 mm apart, tolerance 0.001: Inventor leaves them apart.
    let mut p = PartDocument::default();
    let a = wall(&mut p, &[(0.0, 0.0), (10.0, 0.0)], 10.0);
    let b = wall(&mut p, &[(10.05, 0.0), (10.05, 10.0)], 10.0);
    let e = p
        .add_body_op(BodyOpFeature::new(BodyOp::Stitch(StitchOp {
            surfaces: vec![a, b],
            tolerance: 0.001,
            keep_surface: false,
        })))
        .unwrap_err();
    assert!(e.to_string().contains("tolerance"), "{e}");
    assert_eq!(surfaces(&p), 2);
}

#[test]
fn trim_a_cylinder_wall_along_its_seams() {
    // A circle r 10 extruded 10 as a surface: the kernel makes two half
    // walls meeting where XZ cuts them. XZ trims away the half at y > 0.
    let mut p = PartDocument::default();
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    p.sketches[i]
        .sketch
        .add_circle(Point2::new(0.0, 0.0), 10.0)
        .unwrap();
    let r = p.sketches[i].sketch.regions();
    let mut f = ExtrudeFeature::new(vec![ProfileRef::of(&r[0], r[0].interior_point())], 10.0);
    f.output = Output::Surface;
    let s = p.add_extrude(i, f).unwrap();
    let full = sheet_area(&p);
    for (plane, at) in [
        (OriginPlane::XZ, [0.0, 10.0, 5.0]),
        (OriginPlane::YZ, [10.0, 0.0, 5.0]),
    ] {
        let mut q = p.clone();
        op(
            &mut q,
            BodyOp::Trim(TrimOp {
                surface: s.clone(),
                tool: TrimTool::Plane(PlaneRef::Origin(plane)),
                remove: at,
                more: Vec::new(),
                invert: false,
            }),
        );
        close(sheet_area(&q), full / 2.0, 1e-3);
    }
}

#[test]
fn ruled_alternate_all_faces_uses_each_edges_other_face() {
    // The top's front edge: Normal on the top runs up; with Alternate All Faces the
    // front face decides and the strip runs out of it (-y). Two top edges in two side
    // planes: two strips stitched into one surface.
    let ruled = |edges: Vec<SurfEdge>, alternate: bool| {
        let mut p = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
        op(
            &mut p,
            BodyOp::Ruled(RuledOp {
                kind: RuledKind::Normal,
                edges,
                distance: 5.0,
                flip: false,
                vector: None,
                angle: 0.0,
                alternate,
            }),
        );
        let (lo, hi) = cadcraft3d::kernel::exact::bounds(
            p.exact_bodies.0.iter().find(|s| s.is_open()).unwrap(),
        )
        .unwrap();
        (sheet_area(&p), surfaces(&p), lo, hi)
    };
    let front = SurfEdge {
        face: "Box1:Top".into(),
        hint: [20.0, 0.0, 20.0],
    };
    let (area, n, lo, hi) = ruled(vec![front.clone()], false);
    close(area, 200.0, 1e-6);
    assert!(n == 1 && lo.z > 19.9 && hi.z < 25.1, "{lo} {hi}");
    let (area, n, lo, hi) = ruled(vec![front.clone()], true);
    close(area, 200.0, 1e-6);
    assert!(
        n == 1 && hi.y < 0.1 && lo.y > -5.1 && lo.z > 19.9,
        "{lo} {hi}"
    );
    let right = SurfEdge {
        face: "Box1:Top".into(),
        hint: [40.0, 15.0, 20.0],
    };
    // Two edges of the top, in no particular order: one surface above it.
    let (area, n, lo, hi) = ruled(vec![front.clone(), right.clone()], false);
    close(area, 200.0 + 150.0, 1e-6);
    assert!(n == 1 && lo.z > 19.8 && hi.z < 25.2, "{lo} {hi}");
    let (area, n, lo, hi) = ruled(vec![front, right], true);
    close(area, 200.0 + 150.0, 1e-6);
    assert_eq!(n, 1, "stitched into one surface");
    assert!(hi.x < 45.1 && lo.y > -5.1 && hi.z < 20.1, "{lo} {hi}");
}

/// Loft between edges (spec 5.4a L1): the top edges of two parallel walls bridged by one
/// surface — a flat strip 20 × 10 at the walls' top; edges of one surface stay one section.
#[test]
fn loft_bridges_the_edges_of_two_surfaces() {
    use cadcraft3d::part::{LoftFeature, LoftSection};
    let mut p = PartDocument::default();
    wall(&mut p, &[(0.0, 0.0), (20.0, 0.0)], 10.0);
    wall(&mut p, &[(0.0, 10.0), (20.0, 10.0)], 10.0);
    let before = sheet_area(&p);
    let a = edge(&p, [10.0, 0.0, 5.0], [10.0, 0.0, 10.0]);
    let b = edge(&p, [10.0, 10.0, 5.0], [10.0, 10.0, 10.0]);
    let mut def = LoftFeature::new(vec![
        LoftSection::Edges(vec![a]),
        LoftSection::Edges(vec![b]),
    ]);
    // A solid between open edges is refused, and nothing changes.
    let n = p.features.len();
    assert!(p.add_loft(def.clone()).is_err());
    assert_eq!(p.features.len(), n);
    def.output = Output::Surface;
    p.add_loft(def).unwrap();
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    assert_eq!(surfaces(&p), 3);
    close(sheet_area(&p) - before, 200.0, 1e-3);
    // The feature survives a save and load.
    let text = serde_json::to_string(&p.features).unwrap();
    let back: Vec<cadcraft3d::part::Feature> = serde_json::from_str(&text).unwrap();
    assert_eq!(back, p.features);
}

/// Bridging the top edge of a wall and the top edge of a lower wall: a slanted strip.
#[test]
fn loft_bridges_edges_at_different_heights() {
    use cadcraft3d::part::{LoftFeature, LoftSection};
    let mut p = PartDocument::default();
    wall(&mut p, &[(0.0, 0.0), (20.0, 0.0)], 10.0);
    wall(&mut p, &[(0.0, 10.0), (20.0, 10.0)], 4.0);
    let before = sheet_area(&p);
    let a = edge(&p, [10.0, 0.0, 5.0], [10.0, 0.0, 10.0]);
    let b = edge(&p, [10.0, 10.0, 2.0], [10.0, 10.0, 4.0]);
    let mut def = LoftFeature::new(vec![
        LoftSection::Edges(vec![a]),
        LoftSection::Edges(vec![b]),
    ]);
    def.output = Output::Surface;
    p.add_loft(def).unwrap();
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    // A plane strip from z = 10 down to z = 4 over 10: width 20 × slant length √(100 + 36).
    close(sheet_area(&p) - before, 20.0 * (136.0 as Real).sqrt(), 1e-3);
}

/// Trim with several areas (spec 5.4a T3): a zigzag wall crossing the XZ plane four times is cut
/// into five pieces; removing two of them (one on each side of the plane) leaves the other three,
/// which a one-area trim (one whole side) cannot do. Invert Selection keeps the two instead.
#[test]
fn trim_removes_several_picked_areas() {
    let zigzag = [
        (0.0, -5.0),
        (10.0, 5.0),
        (20.0, -5.0),
        (30.0, 5.0),
        (40.0, -5.0),
    ];
    let seg = (200.0 as Real).sqrt(); // each 10 × 10 diagonal
    let total = 4.0 * seg * 10.0;
    let piece = seg * 10.0; // the pieces between crossings at x = 5, 15, 25, 35
    for (invert, want) in [(false, total - 2.0 * piece), (true, 2.0 * piece)] {
        let mut p = PartDocument::default();
        let w = wall(&mut p, &zigzag, 10.0);
        op(
            &mut p,
            BodyOp::Trim(TrimOp {
                surface: w,
                tool: TrimTool::Plane(PlaneRef::Origin(OriginPlane::XZ)),
                remove: [8.0, 3.0, 5.0],
                more: vec![[22.0, -3.0, 5.0]],
                invert,
            }),
        );
        close(sheet_area(&p), want, 1e-6);
    }
}
