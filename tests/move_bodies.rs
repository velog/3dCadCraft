//! Move Bodies (`docs/spec/2.13-move-bodies.md`): Free Drag, Move Along Ray
//! and Rotate About Line steps on bodies of a multi-body part. The other
//! bodies stay; names, volumes and later features follow the moved body.

mod common;
use common::*;

use cadcraft3d::kernel::{Point3, Real};
use cadcraft3d::part::body_op::{MoveBodiesOp, MoveStep};
use cadcraft3d::part::{
    BodyOp, BodyOpFeature, ExtrudeFeature, LineRef, OriginPlane, Output, PartDocument,
    SketchPlacement,
};
use cadcraft3d::sketch::Point2;
use std::f64::consts::FRAC_PI_2;

/// A plate 80 × 60 × 20 (body Extrusion1) and a block 10 × 10 × 10 at
/// x 100..110, y 0..10 (body Extrusion2, New Solid).
fn two_bodies() -> PartDocument {
    let mut p = plate(80.0, 60.0, 20.0);
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    let sk = &mut p.sketches[i].sketch;
    sk.add_rectangle_two_point(Point2::new(100.0, 0.0), Point2::new(110.0, 10.0))
        .unwrap();
    let r = sk.regions();
    p.add_extrude(i, ExtrudeFeature::new(region_profiles(&r, |_| true), 10.0))
        .unwrap();
    assert_eq!(bodies(&p).len(), 2);
    p
}

fn bodies(p: &PartDocument) -> Vec<String> {
    p.exact_bodies
        .0
        .iter()
        .filter_map(|s| s.name.clone())
        .collect()
}

/// The box of body `name`'s display points.
fn bbox(p: &PartDocument, name: &str) -> (Point3, Point3) {
    let mut lo = Point3::new(Real::INFINITY, Real::INFINITY, Real::INFINITY);
    let mut hi = -lo;
    for f in cadcraft3d::part::select::faces_of_body(&p.body, name) {
        if let Some(m) = p.body.face(f).and_then(|f| f.mesh.as_ref()) {
            for q in &m.positions {
                lo = lo.inf(q);
                hi = hi.sup(q);
            }
        }
    }
    (lo, hi)
}

fn near3(a: Point3, b: [Real; 3], what: &str) {
    assert!(
        (a - Point3::from(b)).norm() < 1e-6,
        "{what}: {a:?} vs {b:?}"
    );
}

fn moved(p: &mut PartDocument, names: &[&str], steps: Vec<MoveStep>) -> Result<String, String> {
    let mut m = MoveBodiesOp::new();
    m.bodies = names.iter().map(|s| (*s).to_owned()).collect();
    m.steps = steps;
    p.add_body_op(BodyOpFeature::new(BodyOp::MoveBodies(m)))
        .map_err(|e| e.to_string())
}

fn healthy(p: &PartDocument) {
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    for (s, sol) in p.body.solids() {
        if !sol.open {
            p.body.validate_solid(s).unwrap();
        }
    }
}

#[test]
fn free_drag_moves_one_body_only() {
    let mut p = two_bodies();
    let v = volume(&p);
    let plate_box = bbox(&p, "Extrusion1");
    moved(
        &mut p,
        &["Extrusion2"],
        vec![MoveStep::FreeDrag {
            x: -50.0,
            y: 20.0,
            z: 30.0,
        }],
    )
    .unwrap();
    healthy(&p);
    let (lo, hi) = bbox(&p, "Extrusion2");
    near3(lo, [50.0, 20.0, 30.0], "block low");
    near3(hi, [60.0, 30.0, 40.0], "block high");
    assert_eq!(bbox(&p, "Extrusion1"), plate_box, "the plate stays");
    assert!((volume(&p) - v).abs() < 1e-6 * v);
    assert_eq!(bodies(&p).len(), 2);
}

#[test]
fn along_a_ray_of_an_axis_or_an_edge() {
    // The X axis, flipped: −20 along X.
    let mut p = two_bodies();
    moved(
        &mut p,
        &["Extrusion2"],
        vec![MoveStep::AlongRay {
            direction: LineRef::Origin(0),
            flip: true,
            offset: 20.0,
        }],
    )
    .unwrap();
    healthy(&p);
    near3(bbox(&p, "Extrusion2").0, [80.0, 0.0, 0.0], "along -X");
    // A straight edge of the plate along Y (its x = 0 bottom edge).
    let mut q = two_bodies();
    let edge = q
        .body
        .edges()
        .find_map(|(e, ed)| {
            let pts = q.body.sample_edge(e, &Default::default()).ok()?;
            let (a, b) = (pts.first()?, pts.last()?);
            let along_y =
                a.x.abs() < 1e-9 && b.x.abs() < 1e-9 && a.z.abs() < 1e-9 && b.z.abs() < 1e-9;
            along_y.then(|| ed.name.clone()).flatten()
        })
        .expect("an edge along Y");
    let (_, d) = LineRef::Edge(edge.clone())
        .resolve(&q.body, &Default::default())
        .unwrap();
    moved(
        &mut q,
        &["Extrusion2"],
        vec![MoveStep::AlongRay {
            direction: LineRef::Edge(edge),
            flip: false,
            offset: 15.0,
        }],
    )
    .unwrap();
    healthy(&q);
    let want_y = 15.0 * d.y.signum();
    near3(
        bbox(&q, "Extrusion2").0,
        [100.0, want_y, 0.0],
        "along the edge",
    );
}

#[test]
fn rotate_about_an_axis_then_drag() {
    // 90° about Z through the origin: (x, y) → (−y, x); then 5 up.
    let mut p = two_bodies();
    moved(
        &mut p,
        &["Extrusion2"],
        vec![
            MoveStep::RotateAboutLine {
                axis: LineRef::Origin(2),
                flip: false,
                angle: FRAC_PI_2,
            },
            MoveStep::FreeDrag {
                x: 0.0,
                y: 0.0,
                z: 5.0,
            },
        ],
    )
    .unwrap();
    healthy(&p);
    let (lo, hi) = bbox(&p, "Extrusion2");
    near3(lo, [-10.0, 100.0, 5.0], "turned low");
    near3(hi, [0.0, 110.0, 15.0], "turned high");
    // Flip turns the other way: (x, y) → (y, −x).
    let mut q = two_bodies();
    moved(
        &mut q,
        &["Extrusion2"],
        vec![MoveStep::RotateAboutLine {
            axis: LineRef::Origin(2),
            flip: true,
            angle: FRAC_PI_2,
        }],
    )
    .unwrap();
    let (lo, hi) = bbox(&q, "Extrusion2");
    near3(lo, [0.0, -110.0, 0.0], "flipped low");
    near3(hi, [10.0, -100.0, 10.0], "flipped high");
}

#[test]
fn both_bodies_move_together_and_names_stay() {
    let mut p = two_bodies();
    let names: Vec<Option<String>> = p.body.faces().map(|(_, f)| f.name.clone()).collect();
    moved(
        &mut p,
        &["Extrusion1", "Extrusion2"],
        vec![MoveStep::FreeDrag {
            x: 0.0,
            y: 0.0,
            z: 100.0,
        }],
    )
    .unwrap();
    healthy(&p);
    near3(bbox(&p, "Extrusion1").0, [0.0, 0.0, 100.0], "plate");
    near3(bbox(&p, "Extrusion2").0, [100.0, 0.0, 100.0], "block");
    let mut after: Vec<Option<String>> = p.body.faces().map(|(_, f)| f.name.clone()).collect();
    let mut before = names;
    before.sort();
    after.sort();
    assert_eq!(before, after, "face names are kept");
}

#[test]
fn a_later_sketch_follows_the_moved_body() {
    // A sketch on the block's top after the move lies at the new height.
    let mut p = two_bodies();
    moved(
        &mut p,
        &["Extrusion2"],
        vec![MoveStep::FreeDrag {
            x: 0.0,
            y: 0.0,
            z: 50.0,
        }],
    )
    .unwrap();
    let top = top(&p, 60.0);
    let j = p
        .create_sketch(SketchPlacement::on_face(&top), &opts())
        .unwrap();
    let o = p.sketches[j].sketch.plane.point_at(0.0, 0.0);
    assert!((o.z - 60.0).abs() < 1e-6, "sketch at z {}", o.z);
}

#[test]
fn a_surface_body_moves_and_stays_a_surface() {
    let mut p = PartDocument::default();
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    let sk = &mut p.sketches[i].sketch;
    sk.add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(20.0, 10.0))
        .unwrap();
    let r = sk.regions();
    let mut ex = ExtrudeFeature::new(region_profiles(&r, |_| true), 5.0);
    ex.output = Output::Surface;
    p.add_extrude(i, ex).unwrap();
    let name = bodies(&p)[0].clone();
    moved(
        &mut p,
        &[&name],
        vec![MoveStep::FreeDrag {
            x: 0.0,
            y: 0.0,
            z: 7.0,
        }],
    )
    .unwrap();
    healthy(&p);
    assert!(p.exact_bodies.0.iter().all(|s| s.is_open()));
    near3(bbox(&p, &name).0, [0.0, 0.0, 7.0], "surface");
}

#[test]
fn refusals_change_nothing() {
    let mut p = two_bodies();
    let before = p.features.len();
    let drag = || {
        vec![MoveStep::FreeDrag {
            x: 1.0,
            y: 0.0,
            z: 0.0,
        }]
    };
    for (names, steps, text) in [
        (Vec::new(), drag(), "select the bodies"),
        (vec!["Extrusion2"], Vec::new(), "add a move"),
        (vec!["Extrusion9"], drag(), "no longer exists"),
        (
            vec!["Extrusion2"],
            vec![MoveStep::AlongRay {
                direction: LineRef::Edge("Nowhere:E1".into()),
                flip: false,
                offset: 3.0,
            }],
            "no longer exists",
        ),
    ] {
        let e = moved(&mut p, &names, steps).unwrap_err();
        assert!(e.contains(text), "{e}");
        assert_eq!(p.features.len(), before);
    }
    near3(bbox(&p, "Extrusion2").0, [100.0, 0.0, 0.0], "unchanged");
}

#[test]
fn values_are_fields_and_save_and_load() {
    let mut m = MoveBodiesOp::new();
    m.bodies = vec!["Extrusion2".into()];
    m.steps.push(MoveStep::RotateAboutLine {
        axis: LineRef::Origin(2),
        flip: false,
        angle: 0.5,
    });
    let mut f = BodyOpFeature::new(BodyOp::MoveBodies(m));
    let fields: Vec<&str> = f.fields().iter().map(|(n, _)| *n).collect();
    assert_eq!(fields, ["x1", "y1", "z1", "angle2"]);
    f.set("z1", 4.0);
    f.set("angle2", 1.0);
    assert_eq!(f.value("z1"), Some(4.0));
    assert_eq!(f.value("angle2"), Some(1.0));
    let mut p = two_bodies();
    p.add_body_op(f).unwrap();
    let text = serde_json::to_string(&p.features).unwrap();
    assert!(
        text.contains("\"MoveBodies\"") && text.contains("\"RotateAboutLine\""),
        "{text}"
    );
    let back: Vec<cadcraft3d::part::Feature> = serde_json::from_str(&text).unwrap();
    assert_eq!(back, p.features);
}
