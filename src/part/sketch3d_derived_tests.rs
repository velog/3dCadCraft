//! Associative 3D sketch curves: they follow the model and the parameters,
//! keep their ids when their shape stays, report a lost input as health and
//! survive a save.

use super::*;
use crate::kernel::Real;
use crate::part::expr::Unit;
use crate::part::placement::OriginPlane;
use crate::part::{PartDocument, SketchOptions, SketchPlacement};

fn top_z(part: &PartDocument) -> Real {
    part.body.bounding_box().map(|(_, hi)| hi.z).unwrap_or(0.0)
}

fn ends(s: &Sketch3d, o: OutCurve) -> Vec<Point3> {
    s.out_points(o)
        .unwrap()
        .iter()
        .map(|&id| s.at(id).unwrap())
        .collect()
}

/// A box part with one 3D sketch holding the curve of `def`.
fn part_with(def: CurveDef, steps: usize) -> PartDocument {
    let mut part = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
    part.sketches3d
        .push(Sketch3d::new("Sketch3D1", "3D Sketch1"));
    let vars = |_: &str| None;
    let inp = Inputs {
        body: &part.body,
        works: &part.work,
        sketches: &part.sketches,
        vars: &vars,
    };
    let lines = compute(&def, &inp, steps).unwrap();
    let pieces = curve_points(lines, steps, 50.0);
    let mut s = part.sketches3d.pop().unwrap();
    s.add_derived(def, steps, &pieces).unwrap();
    part.sketches3d.push(s);
    part.regenerate().unwrap();
    part
}

#[test]
fn intersection_of_two_faces_follows_the_box() {
    let def = CurveDef::Intersection {
        a: SurfaceInput::Faces(vec!["Box1:Top".into()]),
        b: SurfaceInput::Faces(vec!["Box1:Front".into()]),
    };
    let mut part = part_with(def, 24);
    let s = &part.sketches3d[0];
    assert_eq!(s.derived.len(), 1);
    let out = s.derived[0].out.clone();
    assert_eq!(out.len(), 1, "{out:?}");
    assert!(matches!(out[0], OutCurve::Line(_)));
    let z0 = top_z(&part);
    for p in ends(s, out[0]) {
        assert!((p.z - z0).abs() < 1e-6, "{p:?} vs top {z0}");
    }
    // A line drawn from the curve's end follows it.
    let s = &mut part.sketches3d[0];
    let a = s.out_points(out[0]).unwrap()[0];
    let free = s.add_point(Point3::new(-30.0, -30.0, -30.0), true);
    let joined = s.add_line(a, free).unwrap();
    // The box grows: the edge moves up, the ids stay.
    part.box_feature_mut().unwrap().size[2] = 35.0;
    part.regenerate().unwrap();
    assert!(part.feature_health.is_empty(), "{:?}", part.feature_health);
    let s = &part.sketches3d[0];
    assert_eq!(s.derived[0].out, out);
    let z1 = top_z(&part);
    assert!((z1 - z0 - 15.0).abs() < 1e-6);
    for p in ends(s, out[0]) {
        assert!((p.z - z1).abs() < 1e-6, "{p:?} vs top {z1}");
    }
    let l = s.line(joined).unwrap();
    assert!((s.at(l.a).unwrap().z - z1).abs() < 1e-6);
}

#[test]
fn a_face_that_is_gone_is_health_and_keeps_the_curve() {
    let def = CurveDef::Intersection {
        a: SurfaceInput::Faces(vec!["Box1:Top".into()]),
        b: SurfaceInput::Plane(PlaneRef::Origin(OriginPlane::XZ)),
    };
    let mut part = part_with(def, 24);
    let before = part.sketches3d[0].clone();
    assert!(!before.derived[0].out.is_empty());
    if let CurveDef::Intersection { a, .. } = &mut part.sketches3d[0].derived[0].def {
        *a = SurfaceInput::Faces(vec!["Box1:Gone".into()]);
    }
    part.box_feature_mut().unwrap().size[2] = 50.0;
    part.regenerate().unwrap();
    let h = part.feature_health.get("Sketch3D1").expect("health");
    assert!(h.contains("Box1:Gone"), "{h}");
    assert_eq!(part.sketches3d[0].points, before.points);
}

#[test]
fn equation_curve_follows_a_parameter() {
    let def = CurveDef::Equation {
        coords: EquationKind::Cylindrical,
        exprs: ["rad".into(), "t * 360".into(), "5 * t".into()],
        t: (0.0, 1.0),
    };
    let mut part = PartDocument::default();
    let name = part.add_user_parameter(Unit::Mm);
    part.set_parameter_expr(&name, "12 mm").unwrap();
    let def = match def {
        CurveDef::Equation { coords, t, .. } => CurveDef::Equation {
            coords,
            exprs: [name.clone(), "t * 360".into(), "5 * t".into()],
            t,
        },
        d => d,
    };
    let mut s = Sketch3d::new("Sketch3D1", "3D Sketch1");
    let table = part.parameter_table();
    let vars = |n: &str| {
        table
            .iter()
            .find(|r| r.name == n)
            .map(crate::part::params::ParamRow::as_value)
    };
    let inp = Inputs {
        body: &part.body,
        works: &part.work,
        sketches: &part.sketches,
        vars: &vars,
    };
    let pieces = curve_points(compute(&def, &inp, 16).unwrap(), 16, 100.0);
    s.add_derived(def, 16, &pieces).unwrap();
    part.sketches3d.push(s);
    part.regenerate().unwrap();
    let radius = |part: &PartDocument| {
        let s = &part.sketches3d[0];
        let p = ends(s, s.derived[0].out[0]);
        assert_eq!(p.len(), 17);
        p.iter()
            .map(|q| (q.x * q.x + q.y * q.y).sqrt())
            .fold(0.0, Real::max)
    };
    assert!((radius(&part) - 12.0).abs() < 1e-9);
    let ids = part.sketches3d[0].points.clone();
    part.set_parameter_expr(&name, "30 mm").unwrap();
    part.regenerate().unwrap();
    assert!((radius(&part) - 30.0).abs() < 1e-9);
    let now = &part.sketches3d[0].points;
    assert_eq!(
        now.iter().map(|p| p.id).collect::<Vec<_>>(),
        ids.iter().map(|p| p.id).collect::<Vec<_>>()
    );
}

#[test]
fn projection_follows_the_face_and_the_sketch() {
    let mut part = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
    let opts = SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    };
    let i = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts)
        .unwrap();
    let sk = &mut part.sketches[i].sketch;
    let centre = sk.add_fixed_point(crate::sketch::Point2::new(20.0, 15.0));
    let c = sk.add_circle_at(centre, 5.0).unwrap();
    let sid = part.sketches[i].id.clone();
    part.sketches3d
        .push(Sketch3d::new("Sketch3D1", "3D Sketch1"));
    let def = CurveDef::Project {
        faces: vec!["Box1:Top".into()],
        sketches: vec![sid],
        how: ProjectHow::Along([0.0, 0.0, 1.0]),
    };
    let vars = |_: &str| None;
    let inp = Inputs {
        body: &part.body,
        works: &part.work,
        sketches: &part.sketches,
        vars: &vars,
    };
    let pieces = curve_points(compute(&def, &inp, 24).unwrap(), 24, 50.0);
    assert_eq!(pieces.len(), 1);
    part.sketches3d[0].add_derived(def, 24, &pieces).unwrap();
    part.regenerate().unwrap();
    let check = |part: &PartDocument, r: Real| {
        let s = &part.sketches3d[0];
        let z = top_z(part);
        for p in ends(s, s.derived[0].out[0]) {
            assert!((p.z - z).abs() < 1e-6, "{p:?} vs {z}");
            let d = ((p.x - 20.0).powi(2) + (p.y - 15.0).powi(2)).sqrt();
            assert!((d - r).abs() < 0.05, "{p:?}: {d} vs {r}");
        }
    };
    check(&part, 5.0);
    // The box grows and the circle is made larger: the curve follows both.
    part.box_feature_mut().unwrap().size[2] = 32.0;
    let sk = &mut part.sketches[i].sketch;
    sk.add_constraint(crate::sketch::Constraint::Radius {
        circle: c,
        value: 8.0,
    })
    .unwrap();
    sk.solve().unwrap();
    part.regenerate().unwrap();
    assert!(part.feature_health.is_empty(), "{:?}", part.feature_health);
    check(&part, 8.0);
}

#[test]
fn silhouette_and_shape_changes_replace_the_curves() {
    let def = CurveDef::Silhouette {
        dir: [0.0, 0.0, 1.0],
    };
    // A box seen from above has no smooth outline: nothing found.
    let mut part = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
    let vars = |_: &str| None;
    let inp = Inputs {
        body: &part.body,
        works: &part.work,
        sketches: &part.sketches,
        vars: &vars,
    };
    let pieces = curve_points(compute(&def, &inp, 8).unwrap(), 8, 50.0);
    assert!(pieces.is_empty());
    // A curve whose shape changes (a line becomes a spline) is made again,
    // the points only it used go.
    let mut s = Sketch3d::new("Sketch3D1", "3D Sketch1");
    let line = vec![Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0)];
    s.add_derived(def.clone(), 8, std::slice::from_ref(&line))
        .unwrap();
    assert_eq!(s.points.len(), 2);
    let arc: Vec<Point3> = (0..=8)
        .map(|k| {
            let a = k as Real / 8.0 * 3.0;
            Point3::new(a.cos() * 10.0, a.sin() * 10.0, 0.0)
        })
        .collect();
    s.place_derived(0, std::slice::from_ref(&arc)).unwrap();
    assert!(s.lines.is_empty());
    assert_eq!(s.splines.len(), 1);
    assert!(matches!(s.derived[0].out[0], OutCurve::Spline(_)));
    // (10, 0, 0) is shared by both shapes; (0, 0, 0) is gone.
    assert_eq!(s.points.len(), 9);
    part.sketches3d.push(s);
    // Save and load keep the definition.
    let text = serde_json::to_string(&part).unwrap();
    let back: PartDocument = serde_json::from_str(&text).unwrap();
    assert_eq!(back.sketches3d, part.sketches3d);
}

#[test]
fn an_old_sketch_without_definitions_reads_and_stays() {
    let text = r#"{"id":"Sketch3D1","name":"3D Sketch1","points":[{"id":1,"at":[0,0,0],"fixed":true},{"id":2,"at":[5,0,0],"fixed":true}],"lines":[{"id":3,"a":1,"b":2}],"next_id":4}"#;
    let s: Sketch3d = serde_json::from_str(text).unwrap();
    assert!(s.derived.is_empty());
    let mut t = s.clone();
    let vars = |_: &str| None;
    let body = crate::kernel::BRep::new();
    let works = WorkGeom::new();
    let inp = Inputs {
        body: &body,
        works: &works,
        sketches: &[],
        vars: &vars,
    };
    assert!(t.update_derived(&inp).is_empty());
    assert_eq!(t, s);
    assert!(!serde_json::to_string(&s).unwrap().contains("derived"));
}
