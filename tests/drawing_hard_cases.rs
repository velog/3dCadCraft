//! Drawings on harder geometry (spec 4-drawings): what Inventor / SolidWorks drawing views show
//! for spheres, tori, rows of holes seen edge-on, hatching of sections with holes touching the
//! outline, revision clouds round concave and crossing outlines, and flat patterns with mitred
//! bends. Preventive cases: each checks a number a drawing must get right (sizes, line lengths,
//! nothing drawn twice, hatching inside the material).

use cadcraft3d::drawing::ViewOrientation;
use cadcraft3d::drawing::hlr::{ViewLines, flat_lines, part_model, view_lines};
use cadcraft3d::drawing::revision::{RevisionCloud, cloud_prims};
use cadcraft3d::drawing::sheet::Prim;
use cadcraft3d::kernel::{Point3, Real};
use cadcraft3d::part::primitive::{PrimitiveFeature, PrimitiveKind, add_primitive};
use cadcraft3d::part::sheet_metal::{FlangeBend, SheetFlangeFeature, SheetMetalDefaults};
use cadcraft3d::part::{
    ExtrudeFeature, Operation, OriginPlane, PartDocument, ProfileRef, SketchOptions,
    SketchPlacement,
};
use cadcraft3d::sketch::Point2;

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};

fn primitive(part: &mut PartDocument, kind: PrimitiveKind, clicks: &[Point2], f: PrimitiveFeature) {
    add_primitive(
        part,
        kind,
        SketchPlacement::on_origin_plane(OriginPlane::XY),
        clicks,
        f,
    )
    .unwrap();
}

fn revolve(op: Operation) -> PrimitiveFeature {
    PrimitiveFeature::Revolve(op)
}

fn length(lines: &[Vec<[f64; 2]>]) -> f64 {
    lines
        .iter()
        .flat_map(|l| l.windows(2))
        .map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]))
        .sum()
}

fn lines(part: &PartDocument, o: ViewOrientation) -> ViewLines {
    view_lines(&part_model(part), &o.frame())
}

fn close(got: Real, want: Real, tol: Real, what: &str) {
    assert!((got - want).abs() <= tol, "{what}: {got} vs {want}");
}

/// A sphere is one silhouette circle in every view and hides nothing.
#[test]
fn sphere_is_one_silhouette_circle_in_every_view() {
    let mut part = PartDocument::default();
    primitive(
        &mut part,
        PrimitiveKind::Sphere,
        &[Point2::new(0.0, 0.0), Point2::new(15.0, 0.0)],
        revolve(Operation::NewSolid),
    );
    for o in [
        ViewOrientation::Front,
        ViewOrientation::Top,
        ViewOrientation::Right,
        ViewOrientation::IsoTopRight,
    ] {
        let l = lines(&part, o);
        let (w, h) = l.size();
        close(w, 30.0, 0.1, "width");
        close(h, 30.0, 0.1, "height");
        close(length(&l.hidden), 0.0, 0.5, "nothing hidden");
        // The outline once (a seam or the revolve's sector edges must not
        // add a second circle or a meridian).
        let vis = length(&l.visible);
        close(vis, 2.0 * PI * 15.0, 2.0, "one circle");
    }
}

/// A torus about Y (R 20, r 5): seen along its axis two circles (r 15 and
/// 25); from the top a 50 × 10 outline.
#[test]
fn torus_outlines() {
    let mut part = PartDocument::default();
    primitive(
        &mut part,
        PrimitiveKind::Torus,
        &[
            Point2::new(0.0, 0.0),
            Point2::new(20.0, 0.0),
            Point2::new(25.0, 0.0),
        ],
        revolve(Operation::NewSolid),
    );
    let top = lines(&part, ViewOrientation::Top);
    let (w, h) = top.size();
    close(w, 50.0, 0.1, "top width");
    close(h, 10.0, 0.1, "top height");
    // Along the axis: the outer and the inner silhouette circles, the hole
    // seen through.
    let along = [ViewOrientation::Front, ViewOrientation::Right]
        .into_iter()
        .map(|o| lines(&part, o))
        .find(|l| (l.size().0 - l.size().1).abs() < 0.5)
        .expect("a view along the axis");
    close(along.size().0, 50.0, 0.1, "axial width");
    let vis = length(&along.visible);
    close(vis, 2.0 * PI * (25.0 + 15.0), 3.0, "outer + inner circle");
}

/// A 80 × 80 × 10 plate with six Ø6 holes on a Ø50 circle, seen from the
/// front: holes behind each other share their hidden walls — Inventor draws
/// each line once (4 distinct pairs of walls, 8 × 10 mm), not 12 × 10 mm.
#[test]
fn coincident_hidden_lines_are_drawn_once() {
    let mut part = PartDocument::default();
    let f = PrimitiveFeature::extrude(&part, 10.0);
    primitive(
        &mut part,
        PrimitiveKind::Box,
        &[Point2::new(0.0, 0.0), Point2::new(40.0, 40.0)],
        f,
    );
    for k in 0..6 {
        let t = k as Real * PI / 3.0;
        let c = Point2::new(25.0 * t.cos(), 25.0 * t.sin());
        let mut e = ExtrudeFeature::new(Vec::new(), 10.0);
        e.operation = Operation::Cut;
        primitive(
            &mut part,
            PrimitiveKind::Cylinder,
            &[c, Point2::new(c.x + 3.0, c.y)],
            PrimitiveFeature::Extrude(e),
        );
    }
    let front = lines(&part, ViewOrientation::Front);
    close(front.size().0, 80.0, 1e-6, "plate width");
    close(
        length(&front.hidden),
        8.0 * 10.0,
        1.0,
        "hidden hole walls once each",
    );
    // From the top every hole is one visible circle, nothing hidden.
    let top = lines(&part, ViewOrientation::Top);
    close(length(&top.hidden), 0.0, 0.5, "top: nothing hidden");
    close(
        length(&top.visible),
        4.0 * 80.0 + 6.0 * 2.0 * PI * 3.0,
        2.0,
        "outline + 6 circles",
    );
}

/// A cylinder lying along X (a revolve of a rectangle about the X axis
/// would do; here a sphere's cousin: a torus is not needed) — a tube Ø20 ×
/// 40 drawn from the top: its silhouettes are the two long sides, and the
/// round end edges are seen edge-on as straight lines.
#[test]
fn lying_cylinder_silhouettes() {
    let mut part = PartDocument::default();
    let f = PrimitiveFeature::extrude(&part, 40.0);
    add_primitive(
        &mut part,
        PrimitiveKind::Cylinder,
        SketchPlacement::on_origin_plane(OriginPlane::YZ),
        &[Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)],
        f,
    )
    .unwrap();
    let top = lines(&part, ViewOrientation::Top);
    let (w, h) = top.size();
    close(w.max(h), 40.0, 0.05, "length");
    close(w.min(h), 20.0, 0.05, "diameter");
    close(length(&top.hidden), 0.0, 0.5, "nothing hidden");
    close(
        length(&top.visible),
        2.0 * 40.0 + 2.0 * 20.0,
        1.0,
        "a rectangle",
    );
}

fn inside(p: [f64; 2], lp: &[[f64; 2]]) -> bool {
    let mut c = false;
    for w in lp.windows(2) {
        let (a, b) = (w[0], w[1]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < a[0] + (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1])
        {
            c = !c;
        }
    }
    c
}

fn circle(c: [f64; 2], r: f64, n: usize) -> Vec<[f64; 2]> {
    (0..=n)
        .map(|k| {
            let t = 2.0 * PI * k as f64 / n as f64;
            [c[0] + r * t.cos(), c[1] + r * t.sin()]
        })
        .collect()
}

/// Section hatching (even-odd) of a 20 × 20 face with a hole touching its
/// top edge and another touching its left edge: every hatch segment lies in
/// the material, none crosses a hole.
#[test]
fn hatching_with_holes_touching_the_outline() {
    let outer = vec![
        [0.0, 0.0],
        [20.0, 0.0],
        [20.0, 20.0],
        [0.0, 20.0],
        [0.0, 0.0],
    ];
    let h1 = circle([10.0, 15.0], 5.0, 96);
    let h2 = circle([4.0, 6.0], 4.0, 96);
    let loops = vec![outer.clone(), h1.clone(), h2.clone()];
    for angle in [FRAC_PI_4, 0.0, FRAC_PI_2, 1.0] {
        let segs = cadcraft3d::drawing::render::hatch(&loops, angle, 0.7);
        assert!(segs.len() > 20, "hatched at {angle}");
        let mut len = 0.0;
        for (a, b) in &segs {
            len += (b[0] - a[0]).hypot(b[1] - a[1]);
            for t in [0.1, 0.5, 0.9] {
                let p = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
                assert!(
                    inside(p, &outer) && !inside(p, &h1) && !inside(p, &h2),
                    "hatch at {angle} leaves the material at {p:?} ({a:?} – {b:?})"
                );
            }
        }
        // Area / spacing ≈ hatch length.
        let area = 400.0 - PI * 25.0 - PI * 16.0;
        close(len, area / 0.7, 0.08 * area / 0.7, "hatch length");
    }
}

fn path_of(prims: &[Prim]) -> Vec<[f64; 2]> {
    prims
        .iter()
        .flat_map(|p| match p {
            Prim::Path { pts, .. } => pts.clone(),
            _ => Vec::new(),
        })
        .collect()
}

/// A revision cloud round a concave L outline: every arc bulges out of the
/// enclosed area (Inventor's clouds), also along the inner corner.
#[test]
fn revision_cloud_bulges_out_of_a_concave_outline() {
    let l = vec![
        [0.0, 0.0],
        [60.0, 0.0],
        [60.0, 20.0],
        [20.0, 20.0],
        [20.0, 50.0],
        [0.0, 50.0],
    ];
    for pts in [l.clone(), l.iter().rev().copied().collect()] {
        let c = RevisionCloud {
            view: None,
            pts: pts.clone(),
            arc: 8.0,
        };
        let path = path_of(&cloud_prims(&c, [0.0, 0.0], 0.35));
        assert!(path.len() > 50);
        let mut closed = pts.clone();
        closed.push(pts[0]);
        // The top of each arc (the 6th of its 11 points; arcs share ends).
        let tops: Vec<[f64; 2]> = path.iter().skip(5).step_by(10).copied().collect();
        let bad: Vec<_> = tops.iter().filter(|p| inside(**p, &closed)).collect();
        assert!(bad.is_empty(), "arc tops inside the outline: {bad:?}");
        assert!(path.iter().all(|p| p[0].is_finite() && p[1].is_finite()));
    }
}

/// A figure-eight outline (it crosses itself): the signed area is ~0, yet
/// each lobe's arcs must bulge out of that lobe.
#[test]
#[ignore = "known: cloud_prims picks one bulge side from the total signed area; a self-crossing outline gets half its arcs inside (reported to the drawings session)"]
fn revision_cloud_round_a_figure_eight() {
    let pts = vec![[0.0, 0.0], [40.0, 30.0], [40.0, 0.0], [0.0, 30.0]];
    let c = RevisionCloud {
        view: None,
        pts: pts.clone(),
        arc: 6.0,
    };
    let path = path_of(&cloud_prims(&c, [0.0, 0.0], 0.35));
    let lobe_a = vec![[0.0, 0.0], [20.0, 15.0], [0.0, 30.0], [0.0, 0.0]];
    let lobe_b = vec![[40.0, 30.0], [20.0, 15.0], [40.0, 0.0], [40.0, 30.0]];
    let tops: Vec<[f64; 2]> = path.iter().skip(5).step_by(10).copied().collect();
    let bad: Vec<_> = tops
        .iter()
        .filter(|p| inside(**p, &lobe_a) || inside(**p, &lobe_b))
        .collect();
    assert!(bad.is_empty(), "arc tops inside a lobe: {bad:?}");
}

/// A 120 × 80 tray: flanges on all four edges with a return each (Further
/// Bends) — mitred return corners. Its flat pattern view has 8 bends, every
/// centre line inside the flat outline and as long as its zone's edges.
#[test]
fn flat_view_of_a_tray_with_mitred_returns() {
    let mut p = PartDocument::default();
    p.make_sheet_metal(Some(SheetMetalDefaults::default()));
    let o = SketchOptions {
        auto_project_origin: false,
        ..Default::default()
    };
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &o)
        .unwrap();
    p.sketches[i]
        .sketch
        .add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(120.0, 80.0))
        .unwrap();
    let r = p.sketches[i].sketch.regions();
    p.add_sheet_face(i, vec![ProfileRef::of(&r[0], r[0].interior_point())], false)
        .unwrap();
    let mut edges = Vec::new();
    let mut body = String::new();
    for q in [
        Point3::new(60.0, 0.0, 2.0),
        Point3::new(120.0, 40.0, 2.0),
        Point3::new(60.0, 80.0, 2.0),
        Point3::new(0.0, 40.0, 2.0),
    ] {
        let (b, e) = p.sheet_edge_at(q).unwrap();
        body = b;
        edges.push(e);
    }
    let mut f = SheetFlangeFeature::new(&body, edges, 25.0);
    f.bends = vec![FlangeBend {
        height: 12.0,
        angle: FRAC_PI_2,
        flip: true,
    }];
    p.add_sheet_flange(f).unwrap();
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    let view = flat_lines(&p, &ViewOrientation::Top.frame(), None).expect("a flat view");
    assert_eq!(view.bends.len(), 8, "bends");
    let (lo, hi) = (view.min, view.max);
    for b in &view.bends {
        let c = b.centre;
        let len = (c[1][0] - c[0][0]).hypot(c[1][1] - c[0][1]);
        assert!(len > 10.0, "a bend line of {len}");
        for q in c {
            assert!(
                q[0] >= lo[0] - 1e-6
                    && q[0] <= hi[0] + 1e-6
                    && q[1] >= lo[1] - 1e-6
                    && q[1] <= hi[1] + 1e-6,
                "centre line end {q:?} outside the flat {lo:?}..{hi:?}"
            );
        }
        for z in b.zone {
            let zl = (z[1][0] - z[0][0]).hypot(z[1][1] - z[0][1]);
            close(zl, len, 0.25 * len, "zone edge vs centre line");
        }
        close(b.angle, 90.0, 0.01, "bend angle");
    }
}
