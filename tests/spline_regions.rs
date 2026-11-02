//! A closed spline crossing other curves divides the regions like a circle does; the flat rings
//! of a full Revolve show as one face.

use cadcraft3d::part::*;
use cadcraft3d::sketch::*;

fn opts() -> SketchOptions {
    SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    }
}

fn volume(part: &PartDocument) -> f64 {
    let mut v = 0.0;
    for (id, _) in part.body.solids() {
        let m = part.body.tessellate_solid(id, &Default::default()).unwrap();
        for t in &m.faces.triangles {
            let [a, b, c] = t.map(|i| m.faces.positions[i as usize].coords);
            v += a.dot(&b.cross(&c)) / 6.0;
        }
    }
    v
}

#[test]
fn an_extrude_of_a_rectangle_and_a_closed_spline_across_it_is_one_solid() {
    let mut part = PartDocument::default();
    let i = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    let sk = &mut part.sketches[i].sketch;
    sk.add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(40.0, 25.0))
        .unwrap();
    let ids: Vec<_> = [(5.0, -10.0), (15.0, -5.0), (17.0, 8.0), (8.0, 12.0)]
        .iter()
        .map(|&(x, y)| sk.add_point(Point2::new(x, y)))
        .collect();
    let mut closed = ids.clone();
    closed.push(ids[0]);
    sk.add_interp_spline(closed).unwrap();
    let r = sk.regions();
    assert_eq!(r.len(), 3);
    let area: f64 = r.iter().map(Region::area).sum();
    let ex = ExtrudeFeature::new(
        r.iter()
            .map(|x| ProfileRef::of(x, x.interior_point()))
            .collect(),
        5.0,
    );
    part.add_extrude(i, ex).unwrap();
    assert!(part.feature_health.is_empty(), "{:?}", part.feature_health);
    assert_eq!(part.body.solids().count(), 1);
    let v = volume(&part);
    assert!(
        (v - area * 5.0).abs() < area * 5.0 * 2e-3,
        "volume {v} vs {}",
        area * 5.0
    );
    for (id, _) in part.body.solids() {
        part.body.validate_solid(id).unwrap();
    }
}

#[test]
fn a_full_revolve_shows_its_flat_rings_whole() {
    let mut part = PartDocument::default();
    let i = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XZ), &opts())
        .unwrap();
    part.sketches[i]
        .sketch
        .add_rectangle_two_point(Point2::new(10.0, 0.0), Point2::new(20.0, 5.0))
        .unwrap();
    let r = part.sketches[i].sketch.regions();
    let rv = RevolveFeature::new(
        vec![ProfileRef::of(&r[0], r[0].interior_point())],
        AxisRef::Origin(2),
    );
    part.add_revolve(i, rv).unwrap();
    // No straight edge across a flat ring: every visible edge keeps its radius.
    let opts = cadcraft3d::kernel::TessellationOptions::default();
    for e in part.body.visible_edges() {
        let q = part.body.sample_edge(e, &opts).unwrap();
        let radius = |p: &cadcraft3d::kernel::Point3| (p.x * p.x + p.y * p.y).sqrt();
        let (a, b) = (radius(&q[0]), radius(&q[q.len() - 1]));
        let flat = (q[0].z - q[q.len() - 1].z).abs() < 1e-9;
        assert!(
            !(flat && (a - b).abs() > 1.0),
            "a seam across a ring: radius {a} to {b}"
        );
    }
}

#[test]
fn a_surface_revolve_shows_its_flat_rings_whole() {
    let mut part = PartDocument::default();
    let i = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XZ), &opts())
        .unwrap();
    let line = part.sketches[i]
        .sketch
        .add_line(Point2::new(10.0, 3.0), Point2::new(20.0, 3.0));
    let mut rv = RevolveFeature::new(Vec::new(), AxisRef::Origin(2));
    rv.output = Output::Surface;
    rv.open = vec![line];
    part.add_revolve(i, rv).unwrap();
    assert!(part.feature_health.is_empty(), "{:?}", part.feature_health);
    let opts = cadcraft3d::kernel::TessellationOptions::default();
    let names: Vec<_> = part.body.faces().map(|(_, f)| f.name.clone()).collect();
    println!("faces {names:?}");
    for e in part.body.visible_edges() {
        let q = part.body.sample_edge(e, &opts).unwrap();
        let radius = |p: &cadcraft3d::kernel::Point3| (p.x * p.x + p.y * p.y).sqrt();
        let (a, b) = (radius(&q[0]), radius(&q[q.len() - 1]));
        assert!(
            (a - b).abs() < 1.0,
            "a seam across a ring: radius {a} to {b}"
        );
    }
}
