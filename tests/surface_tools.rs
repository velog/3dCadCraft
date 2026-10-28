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
