//! Work features on real parts (`docs/spec/2.10-work-features.md`): every
//! creation method on several geometries (offset, tilted, faces of
//! Revolve, fillets and shells), checked against the exact expected plane,
//! axis or point; and their use by sketches, patterns, mirror and draft,
//! after edits, with missing inputs and through save/load.

mod common;
use common::*;

use cadcraft3d::kernel::{Curve, Plane, Surface, Vector3};
use cadcraft3d::part::{
    CurveRef, LineRef, PatternFeature, PatternKind, PointRef, UcsDef, WorkAxisDef as A, WorkDef,
    WorkFeature, WorkPlaneDef as P, WorkPointDef as Q,
};

const EPS: Real = 1e-6;

fn add(p: &mut PartDocument, d: WorkDef) -> String {
    p.add_work(WorkFeature::new(d))
        .unwrap_or_else(|e| panic!("{e}"))
}

fn plane(p: &mut PartDocument, d: P) -> Plane {
    let id = add(p, WorkDef::Plane(d));
    p.work.planes[&id].clone()
}

fn axis(p: &mut PartDocument, d: A) -> (Point3, Vector3) {
    let id = add(p, WorkDef::Axis(d));
    p.work.axes[&id]
}

fn point(p: &mut PartDocument, d: Q) -> Point3 {
    let id = add(p, WorkDef::Point(d));
    p.work.points[&id]
}

/// A work point at `at` (grounded), as an input.
fn at(p: &mut PartDocument, x: Real, y: Real, z: Real) -> PointRef {
    PointRef::Work(add(p, WorkDef::Point(Q::Fixed { x, y, z })))
}

/// A work axis through `o` along `d` (fixed), as an input.
fn line(p: &mut PartDocument, o: [Real; 3], d: [Real; 3]) -> LineRef {
    LineRef::Work(add(
        p,
        WorkDef::Axis(A::Fixed {
            origin: o,
            dir: Vector3::from(d).normalize().into(),
        }),
    ))
}

/// Names of the faces whose (exact) surface `pick` accepts.
fn faces(p: &PartDocument, pick: impl Fn(&Surface) -> bool) -> Vec<String> {
    let mut out: Vec<String> = p
        .body
        .faces()
        .filter_map(|(_, f)| {
            let s = p.body.surface(f.facet_of.unwrap_or(f.surface))?;
            if pick(s) { f.name.clone() } else { None }
        })
        .collect();
    out.dedup();
    out
}

fn cylinder_face(p: &PartDocument, r: Real) -> String {
    faces(
        p,
        |s| matches!(s, Surface::Cylinder(c) if (c.radius - r).abs() < 1e-6),
    )
    .into_iter()
    .next()
    .unwrap_or_else(|| panic!("no cylinder r {r}"))
}

fn one(p: &PartDocument, pick: impl Fn(&Surface) -> bool, what: &str) -> String {
    faces(p, pick)
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("no {what} face"))
}

/// Distance of `q` from the line (`o`, `d`).
fn off_line(q: &Point3, o: &Point3, d: &Vector3) -> Real {
    let d = d.normalize();
    let w = q - o;
    (w - d * w.dot(&d)).norm()
}

fn assert_axis(got: (Point3, Vector3), through: Point3, dir: Vector3) {
    assert!(
        (got.1.normalize().dot(&dir.normalize()).abs() - 1.0).abs() < EPS,
        "direction {:?} vs {:?}",
        got.1,
        dir
    );
    assert!(
        off_line(&through, &got.0, &got.1) < EPS,
        "{through} off the axis {:?}",
        got
    );
}

fn assert_plane(p: &Plane, through: Point3, normal: Vector3) {
    assert!(
        (p.normal.dot(&normal.normalize()) - 1.0).abs() < EPS,
        "normal {:?} vs {normal:?}",
        p.normal
    );
    assert!(
        (through - p.origin).dot(&p.normal).abs() < EPS,
        "{through} not on the plane"
    );
}

/// A circle of radius `r` at (x, y) of a sketch at `on`, extruded `h`.
fn cylinder_on(p: &mut PartDocument, on: SketchPlacement, x: Real, y: Real, r: Real, h: Real) {
    let i = p.create_sketch(on, &opts()).unwrap();
    let sk = &mut p.sketches[i].sketch;
    sk.add_circle(Point2::new(x, y), r).unwrap();
    let reg = sk.regions();
    p.add_extrude(i, ExtrudeFeature::new(region_profiles(&reg, |_| true), h))
        .unwrap();
}

fn cylinder(x: Real, y: Real, r: Real, h: Real) -> PartDocument {
    let mut p = PartDocument::default();
    cylinder_on(
        &mut p,
        SketchPlacement::on_origin_plane(OriginPlane::XY),
        x,
        y,
        r,
        h,
    );
    p
}

/// A cylinder r 10, 30 long, on a work plane turned 30° about X (its axis
/// is the plane's normal), centred at sketch (10, 5). Returns the part,
/// the axis point and direction.
fn tilted() -> (PartDocument, Point3, Vector3) {
    let mut p = PartDocument::default();
    let wp = add(
        &mut p,
        WorkDef::Plane(P::Angle {
            base: PlaneRef::Origin(OriginPlane::XY),
            axis: LineRef::Origin(0),
            angle: 30f64.to_radians(),
        }),
    );
    let pl = p.work.planes[&wp].clone();
    let on = SketchPlacement {
        plane: PlaneRef::Work(wp),
        ..Default::default()
    };
    cylinder_on(&mut p, on, 10.0, 5.0, 10.0, 30.0);
    let i = p.sketches.len() - 1;
    let c = p.sketches[i].sketch.to_world(&Point2::new(10.0, 5.0));
    (p, c, pl.normal.into_inner())
}

/// A profile in the XZ plane (sketch x = X, y = Z) revolved 360° about the
/// Z axis or, with `axis_x`, about a sketch line at x = `axis_x`.
fn revolved(
    axis_x: Option<Real>,
    draw: impl FnOnce(&mut cadcraft3d::sketch::Sketch),
) -> PartDocument {
    let mut p = PartDocument::default();
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XZ), &opts())
        .unwrap();
    let sk = &mut p.sketches[i].sketch;
    draw(sk);
    let reg = sk.regions();
    let ax = match axis_x {
        Some(x) => AxisRef::SketchLine(sk.add_line(Point2::new(x, -100.0), Point2::new(x, 100.0))),
        None => AxisRef::Origin(2),
    };
    p.add_revolve(i, RevolveFeature::new(region_profiles(&reg, |_| true), ax))
        .unwrap();
    p
}

/// A sphere of radius r centred at (x0, 0, z0) (a half disc revolved about
/// the vertical line x = x0).
fn sphere(x0: Real, z0: Real, r: Real) -> PartDocument {
    revolved((x0 != 0.0).then_some(x0), |sk| {
        let c = sk.add_point(Point2::new(x0, z0));
        let a = sk.add_point(Point2::new(x0, z0 - r));
        let b = sk.add_point(Point2::new(x0, z0 + r));
        sk.add_arc(c, a, b).unwrap();
        sk.add_line_between(b, a).unwrap();
    })
}

/// A torus: tube r around a circle R about the vertical line x = x0, at z0.
fn torus(x0: Real, z0: Real, big: Real, r: Real) -> PartDocument {
    revolved((x0 != 0.0).then_some(x0), |sk| {
        sk.add_circle(Point2::new(x0 + big, z0), r).unwrap();
    })
}

/// A cone: base radius r at z = 0, apex at (0, 0, h).
fn cone(r: Real, h: Real) -> PartDocument {
    revolved(None, |sk| {
        let a = sk.add_point(Point2::new(0.0, 0.0));
        let b = sk.add_point(Point2::new(r, 0.0));
        let c = sk.add_point(Point2::new(0.0, h));
        sk.add_line_between(a, b).unwrap();
        sk.add_line_between(b, c).unwrap();
        sk.add_line_between(c, a).unwrap();
    })
}

/// A 40×30×20 plate whose vertical edge at (40, 0) is filleted r 5 (its
/// round's axis: vertical through (35, 5)).
fn filleted() -> PartDocument {
    let mut p = plate(40.0, 30.0, 20.0);
    let e = edges_where(&p, |h| (h[0] - 40.0).abs() < 1e-6 && h[1].abs() < 1e-6);
    p.add_blend(BlendFeature::fillet(e, 5.0)).unwrap();
    p
}

/// A cylinder r 10 shelled 2 inside, open at the top (inner wall r 8).
fn shelled() -> PartDocument {
    let mut p = cylinder(0.0, 0.0, 10.0, 20.0);
    let t = top(&p, 20.0);
    shell(
        &mut p,
        &[(&t, [0.0, 0.0, 20.0])],
        2.0,
        ShellDirection::Inside,
    );
    p
}

// ------------------------------------------------------------- axes ---

/// **Through Revolved Face**: cylinders (offset, tilted, revolved, fillet
/// round, shell wall), a cone and a torus give their axes.
#[test]
fn axis_through_revolved_faces() {
    let z = Vector3::z();
    let cases: Vec<(PartDocument, String, Point3, Vector3)> = {
        let mut v = Vec::new();
        let p = cylinder(0.0, 0.0, 10.0, 30.0);
        let f = cylinder_face(&p, 10.0);
        v.push((p, f, Point3::new(0.0, 0.0, 0.0), z));
        let p = cylinder(25.0, -5.0, 4.0, 10.0);
        let f = cylinder_face(&p, 4.0);
        v.push((p, f, Point3::new(25.0, -5.0, 3.0), z));
        let (p, c, n) = tilted();
        let f = cylinder_face(&p, 10.0);
        v.push((p, f, c, n));
        let p = revolved(None, |sk| {
            sk.add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(12.0, 8.0))
                .unwrap();
        });
        let f = cylinder_face(&p, 12.0);
        v.push((p, f, Point3::origin(), z));
        let p = filleted();
        let f = cylinder_face(&p, 5.0);
        v.push((p, f, Point3::new(35.0, 5.0, 0.0), z));
        let p = shelled();
        let f = cylinder_face(&p, 8.0);
        v.push((p, f, Point3::origin(), z));
        let p = cone(10.0, 20.0);
        let f = one(&p, |s| matches!(s, Surface::Cone(_)), "cone");
        v.push((p, f, Point3::new(0.0, 0.0, 20.0), z));
        let p = torus(7.0, 3.0, 15.0, 2.0);
        let f = one(&p, |s| matches!(s, Surface::Torus(_)), "torus");
        v.push((p, f, Point3::new(7.0, 0.0, 3.0), z));
        v
    };
    for (mut p, face, through, dir) in cases {
        let got = axis(&mut p, A::RevolvedFace { face: face.clone() });
        assert_axis(got, through, dir);
    }
    // A sphere has no single axis; a plane is no revolved face.
    let mut p = sphere(0.0, 0.0, 5.0);
    let f = one(&p, |s| matches!(s, Surface::Sphere(_)), "sphere");
    refused(
        p.add_work(WorkFeature::new(WorkDef::Axis(A::RevolvedFace { face: f }))),
        "sphere",
    );
}

/// **Through Center of Circular or Elliptical Edge**: circle edges of
/// cylinders (offset, tilted, shell wall, fillet arc) and the ellipse a
/// slanted cut leaves on a cylinder.
#[test]
fn axis_through_circular_and_elliptical_edges() {
    fn circles(p: &PartDocument) -> Vec<(String, Point3, Vector3, Real)> {
        p.body
            .edges()
            .filter_map(|(_, e)| match p.body.curve(e.curve) {
                Some(Curve::Circle(k)) => {
                    Some((e.name.clone()?, k.center, k.normal.into_inner(), k.radius))
                }
                _ => None,
            })
            .collect()
    }
    let parts = [
        cylinder(0.0, 0.0, 10.0, 30.0),
        cylinder(25.0, -5.0, 4.0, 10.0),
        tilted().0,
        shelled(),
        filleted(),
    ];
    let mut n = 0;
    for p in parts {
        for (name, c, nrm, _) in circles(&p) {
            let mut q = p.clone();
            let got = axis(&mut q, A::CircularEdge { edge: name });
            assert_axis(got, c, nrm);
            n += 1;
        }
    }
    assert!(n >= 9, "only {n} circular edges");

    // An ellipse: a cylinder r 10 cut by a plane at 30° through (0, 0, 20).
    let mut p = cylinder(0.0, 0.0, 10.0, 40.0);
    let hinge = line(&mut p, [0.0, 0.0, 20.0], [1.0, 0.0, 0.0]);
    let wp = add(
        &mut p,
        WorkDef::Plane(P::Angle {
            base: PlaneRef::Origin(OriginPlane::XY),
            axis: hinge,
            angle: 30f64.to_radians(),
        }),
    );
    let j = p
        .create_sketch(
            SketchPlacement {
                plane: PlaneRef::Work(wp.clone()),
                ..Default::default()
            },
            &opts(),
        )
        .unwrap();
    let sk = &mut p.sketches[j].sketch;
    sk.add_rectangle_two_point(Point2::new(-50.0, -50.0), Point2::new(50.0, 50.0))
        .unwrap();
    let reg = sk.regions();
    let mut ex = ExtrudeFeature::new(region_profiles(&reg, |_| true), 1.0);
    ex.operation = Operation::Cut;
    ex.extent = Extent::ThroughAll;
    p.add_extrude(j, ex).unwrap();
    let pl = p.work.planes[&wp].clone();
    // The edge on the slanted face: not a circle, in the cut plane.
    let ellipse = p
        .body
        .edges()
        .filter(|(_, e)| {
            !matches!(
                p.body.curve(e.curve),
                Some(Curve::Circle(_) | Curve::Line(_))
            )
        })
        .filter_map(|(id, e)| {
            let v = p.body.vertex(e.start)?.point;
            ((v - pl.origin).dot(&pl.normal).abs() < 1e-4).then(|| (id, e.name.clone()))
        })
        .find_map(|(_, n)| n)
        .expect("no elliptical edge");
    let got = axis(&mut p, A::CircularEdge { edge: ellipse });
    // Centre where the cylinder axis meets the plane: (0, 0, 20).
    assert_axis(got, Point3::new(0.0, 0.0, 20.0), pl.normal.into_inner());
    assert!(
        (got.0 - Point3::new(0.0, 0.0, 20.0)).norm() < 1e-3,
        "centre {}",
        got.0
    );
}

/// **On Line** (sketch line) and **On Sketch Point**: they follow the
/// sketch's face when the box grows.
#[test]
fn sketch_points_and_lines_follow_their_sketch() {
    for (w, d, h) in [
        (40.0, 30.0, 20.0),
        (10.0, 10.0, 5.0),
        (60.0, 20.0, 35.0),
        (25.0, 45.0, 12.0),
        (80.0, 60.0, 1.0),
    ] {
        let mut p = plate(w, d, h);
        let t = top(&p, h);
        let mut ids = None;
        let j = sketch_on(&mut p, &t, |sk, at| {
            let l = sk.add_line(at(1.0, 2.0, h), at(w - 1.0, d - 2.0, h));
            ids = Some((l, sk.add_point(at(w / 2.0, d / 3.0, h))));
        });
        let sid = p.sketches[j].id.clone();
        let (line_e, lone) = ids.unwrap();
        let ax = axis(
            &mut p,
            A::SketchLine {
                sketch: sid.clone(),
                entity: line_e,
            },
        );
        assert_axis(
            ax,
            Point3::new(1.0, 2.0, h),
            Vector3::new(w - 2.0, d - 4.0, 0.0),
        );
        let wpt = add(
            &mut p,
            WorkDef::Point(Q::SketchPoint {
                sketch: sid,
                point: lone,
            }),
        );
        assert!((p.work.points[&wpt] - Point3::new(w / 2.0, d / 3.0, h)).norm() < EPS);
        // The plate gets 7 taller: the sketch and the point follow.
        let mut ex = match &p.features[0] {
            Feature::Extrude(e) => e.clone(),
            _ => panic!("no extrude"),
        };
        ex.distance = h + 7.0;
        p.replace_feature(0, Feature::Extrude(ex)).unwrap();
        assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
        assert!((p.work.points[&wpt].z - (h + 7.0)).abs() < EPS);
    }
}

// ----------------------------------------------------------- planes ---

/// **Midplane of Torus**: offset and moved tori.
#[test]
fn torus_midplanes_and_centres() {
    for (x0, z0, big, r) in [
        (0.0, 0.0, 15.0, 3.0),
        (0.0, 5.0, 10.0, 2.0),
        (7.0, 3.0, 15.0, 2.0),
        (-12.0, -4.0, 8.0, 1.0),
        (20.0, 10.0, 30.0, 5.0),
    ] {
        let mut p = torus(x0, z0, big, r);
        let f = one(&p, |s| matches!(s, Surface::Torus(_)), "torus");
        let m = plane(&mut p, P::TorusMidplane { face: f.clone() });
        assert!(m.normal.z.abs() > 1.0 - EPS);
        assert!((m.origin - Point3::new(x0, 0.0, z0)).norm() < EPS);
        let c = point(&mut p, Q::TorusCenter { face: f });
        assert!((c - Point3::new(x0, 0.0, z0)).norm() < EPS, "{c}");
    }
    let mut p = cylinder(0.0, 0.0, 5.0, 5.0);
    let f = cylinder_face(&p, 5.0);
    refused(
        p.add_work(WorkFeature::new(WorkDef::Plane(P::TorusMidplane {
            face: f,
        }))),
        "torus",
    );
}

/// **Center Point of Sphere**.
#[test]
fn sphere_centres() {
    for (x0, z0, r) in [
        (0.0, 0.0, 5.0),
        (0.0, 7.0, 3.0),
        (4.0, 2.0, 6.0),
        (-9.0, -3.0, 2.5),
        (15.0, 15.0, 10.0),
    ] {
        let mut p = sphere(x0, z0, r);
        let f = one(&p, |s| matches!(s, Surface::Sphere(_)), "sphere");
        let c = point(&mut p, Q::SphereCenter { face: f });
        assert!((c - Point3::new(x0, 0.0, z0)).norm() < EPS, "{c}");
    }
}

/// **Tangent to Surface and Parallel to Plane**: cylinders (offset,
/// tilted, fillet round, shell wall), a sphere and a torus; both sides.
#[test]
fn tangent_planes_parallel_to_a_plane() {
    let yz = PlaneRef::Origin(OriginPlane::YZ); // normal +X
    let xz = PlaneRef::Origin(OriginPlane::XZ); // normal −Y
    let xy = PlaneRef::Origin(OriginPlane::XY); // normal +Z
    // (part, face, base, expected point on the near / far plane, normal)
    let mut cases: Vec<(PartDocument, String, PlaneRef, Point3, Point3, Vector3)> = Vec::new();
    let p = cylinder(0.0, 0.0, 10.0, 30.0);
    let f = cylinder_face(&p, 10.0);
    cases.push((
        p,
        f,
        yz.clone(),
        Point3::new(10.0, 0.0, 0.0),
        Point3::new(-10.0, 0.0, 0.0),
        Vector3::x(),
    ));
    let p = cylinder(25.0, -5.0, 4.0, 10.0);
    let f = cylinder_face(&p, 4.0);
    cases.push((
        p,
        f,
        xz.clone(),
        Point3::new(0.0, -9.0, 0.0),
        Point3::new(0.0, -1.0, 0.0),
        -Vector3::y(),
    ));
    let (p, c, _) = tilted();
    let f = cylinder_face(&p, 10.0);
    cases.push((
        p,
        f,
        yz.clone(),
        c + Vector3::x() * 10.0,
        c - Vector3::x() * 10.0,
        Vector3::x(),
    ));
    let p = filleted();
    let f = cylinder_face(&p, 5.0);
    cases.push((
        p,
        f,
        yz.clone(),
        Point3::new(40.0, 0.0, 0.0),
        Point3::new(30.0, 0.0, 0.0),
        Vector3::x(),
    ));
    let p = shelled();
    let f = cylinder_face(&p, 8.0);
    cases.push((
        p,
        f,
        xz,
        Point3::new(0.0, -8.0, 0.0),
        Point3::new(0.0, 8.0, 0.0),
        -Vector3::y(),
    ));
    let p = sphere(4.0, 2.0, 6.0);
    let f = one(&p, |s| matches!(s, Surface::Sphere(_)), "sphere");
    cases.push((
        p,
        f,
        xy.clone(),
        Point3::new(0.0, 0.0, 8.0),
        Point3::new(0.0, 0.0, -4.0),
        Vector3::z(),
    ));
    let p = torus(0.0, 5.0, 15.0, 3.0);
    let f = one(&p, |s| matches!(s, Surface::Torus(_)), "torus");
    cases.push((
        p.clone(),
        f.clone(),
        xy,
        Point3::new(0.0, 0.0, 8.0),
        Point3::new(0.0, 0.0, 2.0),
        Vector3::z(),
    ));
    cases.push((
        p,
        f,
        yz,
        Point3::new(18.0, 0.0, 0.0),
        Point3::new(-18.0, 0.0, 0.0),
        Vector3::x(),
    ));
    for (p, face, base, near, far, n) in cases {
        for (other, want) in [(false, near), (true, far)] {
            let mut q = p.clone();
            let got = plane(
                &mut q,
                P::TangentParallel {
                    face: face.clone(),
                    base: base.clone(),
                    other,
                },
            );
            assert_plane(&got, want, n);
        }
    }
    // A plane across the cylinder's axis cannot be tangent.
    let mut p = cylinder(0.0, 0.0, 10.0, 30.0);
    let f = cylinder_face(&p, 10.0);
    refused(
        p.add_work(WorkFeature::new(WorkDef::Plane(P::TangentParallel {
            face: f,
            base: PlaneRef::Origin(OriginPlane::XY),
            other: false,
        }))),
        "not parallel",
    );
}

/// The plane through `line` tangent to the round surface: it contains the
/// line and is `r` from the axis/centre.
fn assert_tangent_through(pl: &Plane, lp: Point3, ld: Vector3, centre: Point3, r: Real) {
    assert!((lp - pl.origin).dot(&pl.normal).abs() < EPS);
    assert!(ld.normalize().dot(&pl.normal).abs() < EPS);
    assert!(
        ((centre - pl.origin).dot(&pl.normal).abs() - r).abs() < EPS,
        "distance {} vs {r}",
        (centre - pl.origin).dot(&pl.normal)
    );
}

/// **Tangent to Surface Through Edge**: work axes and a model edge along
/// cylinders (offset, tilted, fillet round), and lines past spheres.
#[test]
fn tangent_planes_through_edges() {
    // (part, face, line point, line dir, axis point / centre, r)
    let mut cases: Vec<(PartDocument, String, Point3, Vector3, Point3, Real)> = Vec::new();
    let p = cylinder(0.0, 0.0, 10.0, 30.0);
    let f = cylinder_face(&p, 10.0);
    cases.push((
        p,
        f,
        Point3::new(20.0, 0.0, 0.0),
        Vector3::z(),
        Point3::origin(),
        10.0,
    ));
    let p = cylinder(25.0, -5.0, 4.0, 10.0);
    let f = cylinder_face(&p, 4.0);
    cases.push((
        p,
        f,
        Point3::new(30.0, 3.0, 2.0),
        Vector3::z(),
        Point3::new(25.0, -5.0, 0.0),
        4.0,
    ));
    let (p, c, n) = tilted();
    let f = cylinder_face(&p, 10.0);
    cases.push((p, f, c + Vector3::x() * 25.0, n, c, 10.0));
    let p = sphere(0.0, 5.0, 8.0);
    let f = one(&p, |s| matches!(s, Surface::Sphere(_)), "sphere");
    cases.push((
        p.clone(),
        f.clone(),
        Point3::new(0.0, 0.0, 20.0),
        Vector3::x(),
        Point3::new(0.0, 0.0, 5.0),
        8.0,
    ));
    cases.push((
        p,
        f,
        Point3::new(3.0, 12.0, 0.0),
        Vector3::new(1.0, 0.0, 1.0),
        Point3::new(0.0, 0.0, 5.0),
        8.0,
    ));
    for (p, face, lp, ld, c, r) in cases {
        for other in [false, true] {
            let mut q = p.clone();
            let l = line(&mut q, lp.into(), ld.into());
            let got = plane(
                &mut q,
                P::TangentEdge {
                    face: face.clone(),
                    line: l,
                    other,
                },
            );
            assert_tangent_through(&got, lp, ld, c, r);
            // Y points from the tangency towards the edge.
            assert!(got.y_axis().dot(&(lp - got.origin)) > -EPS);
        }
    }
    // A real model edge: the plate's vertical edge at (40, 30) and the
    // fillet round (axis (35, 5), r 5).
    let mut p = filleted();
    let f = cylinder_face(&p, 5.0);
    let e = p
        .body
        .edges()
        .find_map(|(id, e)| {
            let a = p.body.vertex(e.start)?.point;
            let b = p.body.vertex(e.end)?.point;
            let at = |q: Point3| (q.x - 40.0).abs() < 1e-6 && (q.y - 30.0).abs() < 1e-6;
            (at(a) && at(b) && p.body.edge_direction(id).is_some())
                .then(|| e.name.clone())
                .flatten()
        })
        .expect("edge at (40, 30)");
    let got = plane(
        &mut p,
        P::TangentEdge {
            face: f,
            line: LineRef::Edge(e),
            other: false,
        },
    );
    assert_tangent_through(
        &got,
        Point3::new(40.0, 30.0, 0.0),
        Vector3::z(),
        Point3::new(35.0, 5.0, 0.0),
        5.0,
    );
}

/// **Tangent to Surface Through Point**: on the surface (cylinder, tilted
/// cylinder, fillet round, sphere, torus, cone) and off a cylinder.
#[test]
fn tangent_planes_through_points() {
    let s = 0.5f64.sqrt();
    let k = (10.0f64 / 20.0).atan(); // the cone's half angle
    // (part, face, point, expected normal)
    let mut cases: Vec<(PartDocument, String, Point3, Vector3)> = Vec::new();
    let p = cylinder(0.0, 0.0, 10.0, 30.0);
    let f = cylinder_face(&p, 10.0);
    cases.push((p, f, Point3::new(10.0, 0.0, 15.0), Vector3::x()));
    let (p, c, _) = tilted();
    let f = cylinder_face(&p, 10.0);
    cases.push((p, f, c + Vector3::x() * 10.0, Vector3::x()));
    let p = filleted();
    let f = cylinder_face(&p, 5.0);
    cases.push((
        p,
        f,
        Point3::new(35.0 + 5.0 * s, 5.0 - 5.0 * s, 10.0),
        Vector3::new(s, -s, 0.0),
    ));
    let p = sphere(4.0, 2.0, 6.0);
    let f = one(&p, |s| matches!(s, Surface::Sphere(_)), "sphere");
    cases.push((p, f, Point3::new(4.0, 0.0, 8.0), Vector3::z()));
    let p = torus(0.0, 0.0, 15.0, 3.0);
    let f = one(&p, |s| matches!(s, Surface::Torus(_)), "torus");
    cases.push((p, f, Point3::new(0.0, 18.0, 0.0), Vector3::y()));
    let p = cone(10.0, 20.0);
    let f = one(&p, |s| matches!(s, Surface::Cone(_)), "cone");
    // At height 10 the radius is 5; the normal leans up by k.
    cases.push((
        p,
        f,
        Point3::new(5.0, 0.0, 10.0),
        Vector3::new(k.cos(), 0.0, k.sin()),
    ));
    for (mut p, face, q, n) in cases {
        let pt = at(&mut p, q.x, q.y, q.z);
        let got = plane(
            &mut p,
            P::TangentPoint {
                face,
                point: pt,
                other: false,
            },
        );
        assert_plane(&got, q, n);
        assert!((got.origin - q).norm() < EPS);
    }
    // Off a cylinder: two planes through the axis-parallel line.
    for other in [false, true] {
        let mut p = cylinder(0.0, 0.0, 10.0, 30.0);
        let f = cylinder_face(&p, 10.0);
        let pt = at(&mut p, 0.0, 20.0, 5.0);
        let got = plane(
            &mut p,
            P::TangentPoint {
                face: f,
                point: pt,
                other,
            },
        );
        assert_tangent_through(
            &got,
            Point3::new(0.0, 20.0, 5.0),
            Vector3::z(),
            Point3::origin(),
            10.0,
        );
    }
}

/// **Normal to Curve at Point**: circular edges at their vertex, a sketch
/// arc and a sketch spline at sketch points.
#[test]
fn planes_normal_to_curves() {
    for p in [
        cylinder(0.0, 0.0, 10.0, 30.0),
        cylinder(25.0, -5.0, 4.0, 10.0),
        tilted().0,
        shelled(),
    ] {
        let (name, k, v) = p
            .body
            .edges()
            .find_map(|(_, e)| match p.body.curve(e.curve) {
                Some(Curve::Circle(k)) => Some((
                    e.name.clone()?,
                    k.clone(),
                    p.body.vertex(e.start)?.name.clone()?,
                )),
                _ => None,
            })
            .expect("a circular edge");
        let mut q = p.clone();
        let at_v = p
            .body
            .vertex(p.body.find_vertex(&v).unwrap())
            .unwrap()
            .point;
        let got = plane(
            &mut q,
            P::NormalToCurve {
                curve: CurveRef::Edge(name),
                point: PointRef::Vertex(v),
            },
        );
        let t = k.normal.cross(&(at_v - k.center)).normalize();
        assert!((got.normal.dot(&t).abs() - 1.0).abs() < EPS);
        assert!((got.origin - at_v).norm() < EPS);
    }
    // Sketch curves on a work plane offset 12 from XY.
    let mut p = PartDocument::default();
    let wp = add(
        &mut p,
        WorkDef::Plane(P::Offset {
            base: PlaneRef::Origin(OriginPlane::XY),
            distance: 12.0,
        }),
    );
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
    let c = sk.add_point(Point2::new(5.0, 5.0));
    let a = sk.add_point(Point2::new(15.0, 5.0));
    let b = sk.add_point(Point2::new(5.0, 15.0));
    let arc = sk.add_arc(c, a, b).unwrap();
    let s0 = sk.add_point(Point2::new(-20.0, 0.0));
    let s1 = sk.add_point(Point2::new(-10.0, 10.0));
    let s2 = sk.add_point(Point2::new(0.0, -5.0));
    let spl = sk.add_spline(vec![s0, s1, s2]).unwrap();
    let sid = p.sketches[i].id.clone();
    let pa = add(
        &mut p,
        WorkDef::Point(Q::SketchPoint {
            sketch: sid.clone(),
            point: a,
        }),
    );
    let got = plane(
        &mut p,
        P::NormalToCurve {
            curve: CurveRef::Sketch {
                sketch: sid.clone(),
                entity: arc,
            },
            point: PointRef::Work(pa),
        },
    );
    assert_plane(&got, Point3::new(15.0, 5.0, 12.0), Vector3::y());
    let ps = add(
        &mut p,
        WorkDef::Point(Q::SketchPoint {
            sketch: sid.clone(),
            point: s0,
        }),
    );
    let got = plane(
        &mut p,
        P::NormalToCurve {
            curve: CurveRef::Sketch {
                sketch: sid,
                entity: spl,
            },
            point: PointRef::Work(ps),
        },
    );
    // A CV spline starts along its first control leg.
    let leg = Vector3::new(10.0, 10.0, 0.0).normalize();
    assert!(
        (got.normal.dot(&leg).abs() - 1.0).abs() < 1e-4,
        "{:?}",
        got.normal
    );
    assert!((got.origin - Point3::new(-20.0, 0.0, 12.0)).norm() < EPS);
    // A point off the curve is refused.
    let off = at(&mut p, 100.0, 0.0, 0.0);
    refused(
        p.add_work(WorkFeature::new(WorkDef::Plane(P::NormalToCurve {
            curve: CurveRef::Sketch {
                sketch: p.sketches[i].id.clone(),
                entity: arc,
            },
            point: off,
        }))),
        "not on the curve",
    );
}

// ----------------------------------------------------------- points ---

/// **Intersection of Surface and Line**: through cylinders, a sphere, a
/// cone, a torus and a fillet round; every hit in order.
#[test]
fn points_where_lines_pierce_surfaces() {
    // (part, face, line point, dir, expected hits in order)
    let mut cases: Vec<(PartDocument, String, Point3, Vector3, Vec<Point3>)> = Vec::new();
    let p = cylinder(0.0, 0.0, 10.0, 30.0);
    let f = cylinder_face(&p, 10.0);
    cases.push((
        p,
        f,
        Point3::new(-50.0, 0.0, 5.0),
        Vector3::x(),
        vec![Point3::new(-10.0, 0.0, 5.0), Point3::new(10.0, 0.0, 5.0)],
    ));
    let p = sphere(0.0, 5.0, 8.0);
    let f = one(&p, |s| matches!(s, Surface::Sphere(_)), "sphere");
    cases.push((
        p,
        f,
        Point3::new(0.0, 0.0, -50.0),
        Vector3::z(),
        vec![Point3::new(0.0, 0.0, -3.0), Point3::new(0.0, 0.0, 13.0)],
    ));
    let p = cone(10.0, 20.0);
    let f = one(&p, |s| matches!(s, Surface::Cone(_)), "cone");
    cases.push((
        p,
        f,
        Point3::new(-50.0, 0.0, 10.0),
        Vector3::x(),
        vec![Point3::new(-5.0, 0.0, 10.0), Point3::new(5.0, 0.0, 10.0)],
    ));
    let p = torus(0.0, 0.0, 15.0, 3.0);
    let f = one(&p, |s| matches!(s, Surface::Torus(_)), "torus");
    cases.push((
        p,
        f,
        Point3::new(-50.0, 0.0, 0.0),
        Vector3::x(),
        [-18.0, -12.0, 12.0, 18.0]
            .map(|x| Point3::new(x, 0.0, 0.0))
            .to_vec(),
    ));
    let p = filleted();
    let f = cylinder_face(&p, 5.0);
    cases.push((
        p,
        f,
        Point3::new(0.0, 5.0, 10.0),
        Vector3::x(),
        vec![Point3::new(30.0, 5.0, 10.0), Point3::new(40.0, 5.0, 10.0)],
    ));
    for (p, face, lp, ld, hits) in cases {
        for (index, want) in hits.iter().enumerate() {
            let mut q = p.clone();
            let l = line(&mut q, lp.into(), ld.into());
            let got = point(
                &mut q,
                Q::SurfaceLine {
                    face: face.clone(),
                    line: l,
                    index,
                },
            );
            assert!((got - want).norm() < 1e-6, "{got} vs {want}");
        }
        let mut q = p.clone();
        let l = line(&mut q, lp.into(), ld.into());
        refused(
            q.add_work(WorkFeature::new(WorkDef::Point(Q::SurfaceLine {
                face,
                line: l,
                index: hits.len(),
            }))),
            "only",
        );
    }
}

// -------------------------------------------------------------- UCS ---

fn ucs_offsets(x: Real, y: Real, z: Real, rx: Real, ry: Real, rz: Real) -> WorkDef {
    WorkDef::Ucs(UcsDef::Offsets {
        x,
        y,
        z,
        rx: rx.to_radians(),
        ry: ry.to_radians(),
        rz: rz.to_radians(),
    })
}

/// A UCS relative to the origin: its planes, axes and centre.
#[test]
fn ucs_relative_to_the_origin() {
    let cases = [
        (
            (0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
            [Vector3::x(), Vector3::y(), Vector3::z()],
        ),
        (
            (10.0, 0.0, 5.0, 0.0, 0.0, 90.0),
            [Vector3::y(), -Vector3::x(), Vector3::z()],
        ),
        (
            (0.0, -3.0, 0.0, 90.0, 0.0, 0.0),
            [Vector3::x(), Vector3::z(), -Vector3::y()],
        ),
        (
            (1.0, 2.0, 3.0, 0.0, 90.0, 0.0),
            [-Vector3::z(), Vector3::y(), Vector3::x()],
        ),
        (
            (5.0, 5.0, 5.0, 90.0, 0.0, 90.0),
            [Vector3::y(), Vector3::z(), Vector3::x()],
        ),
    ];
    for ((x, y, z, rx, ry, rz), [ex, ey, ez]) in cases {
        let mut p = PartDocument::with_box("Box1", [10.0, 10.0, 10.0]).unwrap();
        let id = add(&mut p, ucs_offsets(x, y, z, rx, ry, rz));
        assert_eq!(id, "UCS1");
        let o = Point3::new(x, y, z);
        let w = &p.work;
        assert!((w.points["UCS1:Center"] - o).norm() < EPS);
        for (k, e) in [("X", ex), ("Y", ey), ("Z", ez)] {
            let (q, d) = w.axes[&format!("UCS1:{k}")];
            assert!((q - o).norm() < EPS);
            assert!((d - e).norm() < EPS, "{k}: {d:?} vs {e:?}");
        }
        assert_plane(&w.planes["UCS1:XY"], o, ez);
        assert_plane(&w.planes["UCS1:XZ"], o, -ey);
        assert_plane(&w.planes["UCS1:YZ"], o, ex);
        assert!((w.planes["UCS1:XY"].x_axis.into_inner() - ex).norm() < EPS);
    }
}

/// A UCS on three points of the model follows them; a sketch on its XY
/// plane extrudes along its Z.
#[test]
fn ucs_on_three_points_carries_a_sketch() {
    let v = |n: &str| PointRef::Vertex(format!("Box1:V{n}"));
    for (o, xp, yp, ex, ey) in [
        ("000", "100", "010", Vector3::x(), Vector3::y()),
        ("001", "101", "011", Vector3::x(), Vector3::y()),
        ("100", "110", "000", Vector3::y(), -Vector3::x()),
        ("000", "001", "100", Vector3::z(), Vector3::x()),
        ("111", "011", "101", -Vector3::x(), -Vector3::y()),
    ] {
        let mut p = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
        let id = add(
            &mut p,
            WorkDef::Ucs(UcsDef::ThreePoints {
                points: [v(o), v(xp), v(yp)],
            }),
        );
        let (_, dx) = p.work.axes[&format!("{id}:X")];
        let (_, dy) = p.work.axes[&format!("{id}:Y")];
        assert!((dx - ex).norm() < EPS && (dy - ey).norm() < EPS);
    }
    // A sketch on UCS1:XY of a UCS lifted 20 above the origin: a boss on
    // the box's top.
    let mut p = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
    add(&mut p, ucs_offsets(0.0, 0.0, 20.0, 0.0, 0.0, 0.0));
    let i = p
        .create_sketch(
            SketchPlacement {
                plane: PlaneRef::Work("UCS1:XY".into()),
                ..Default::default()
            },
            &opts(),
        )
        .unwrap();
    assert!((p.sketches[i].sketch.plane.origin.z - 20.0).abs() < EPS);
    let sk = &mut p.sketches[i].sketch;
    sk.add_rectangle_two_point(Point2::new(5.0, 5.0), Point2::new(15.0, 10.0))
        .unwrap();
    let r = sk.regions();
    let mut ex = ExtrudeFeature::new(region_profiles(&r, |_| true), 4.0);
    ex.operation = Operation::Join;
    p.add_extrude(i, ex).unwrap();
    check(&p, 40.0 * 30.0 * 20.0 + 10.0 * 5.0 * 4.0, 1e-6);
    // Moving the UCS moves the boss.
    if let Feature::Work(w) = &mut p.features[1] {
        w.def = ucs_offsets(0.0, 0.0, 10.0, 0.0, 0.0, 0.0);
    }
    p.regenerate().unwrap();
    assert!((p.sketches[i].sketch.plane.origin.z - 10.0).abs() < EPS);
    check(&p, 40.0 * 30.0 * 20.0, 1e-6); // buried in the box
}

// -------------------------------------------------- with other features ---

/// A sketch on a tangent work plane of a cylinder: a pocket cut from it
/// into the cylinder.
#[test]
fn sketch_on_a_tangent_plane_then_extrude() {
    let mut p = cylinder(0.0, 0.0, 10.0, 30.0);
    let f = cylinder_face(&p, 10.0);
    let wp = add(
        &mut p,
        WorkDef::Plane(P::TangentParallel {
            face: f,
            base: PlaneRef::Origin(OriginPlane::YZ),
            other: false,
        }),
    );
    let i = p
        .create_sketch(
            SketchPlacement {
                plane: PlaneRef::Work(wp),
                ..Default::default()
            },
            &opts(),
        )
        .unwrap();
    let plane = p.sketches[i].sketch.plane.clone();
    assert!((plane.origin.x - 10.0).abs() < EPS);
    let at = |x: Real, y: Real, z: Real| plane.project(&Point3::new(x, y, z));
    let sk = &mut p.sketches[i].sketch;
    sk.add_rectangle_two_point(at(10.0, -3.0, 10.0), at(10.0, 3.0, 20.0))
        .unwrap();
    let r = sk.regions();
    // Cut 5 deep into the cylinder: the slice x in [5, 10], |y| <= 3,
    // 10 high.
    let mut ex = ExtrudeFeature::new(region_profiles(&r, |_| true), 5.0);
    ex.operation = Operation::Cut;
    ex.direction = ExtentDirection::Flipped;
    p.add_extrude(i, ex).unwrap();
    let chord = 2.0 * (1.5 * 91f64.sqrt() + 50.0 * 0.3f64.asin());
    check(&p, PI * 100.0 * 30.0 - (chord - 30.0) * 10.0, 3e-4);
}

/// Circular pattern about a work axis through a cylinder face; mirror
/// across a UCS plane; draft from a work plane.
#[test]
fn pattern_mirror_and_draft_use_work_features() {
    // A disc r 20, 10 thick, with a boss r 3 at (15, 0) on top.
    let mut p = cylinder(0.0, 0.0, 20.0, 10.0);
    let t = top(&p, 10.0);
    let j = sketch_on(&mut p, &t, |sk, at| {
        sk.add_circle(at(15.0, 0.0, 10.0), 3.0).unwrap();
    });
    boss(&mut p, j, 5.0, |r| r.area() < 100.0);
    let boss_id = p.features.last().unwrap().id().to_owned();
    let f = cylinder_face(&p, 20.0);
    let ax = add(&mut p, WorkDef::Axis(A::RevolvedFace { face: f }));
    let mut pat = PatternFeature::circular();
    pat.features = vec![boss_id];
    if let PatternKind::Circular { axis, count, .. } = &mut pat.kind {
        *axis = Some(LineRef::Work(ax));
        *count = 4;
    }
    p.add_pattern(pat).unwrap();
    check(&p, PI * 400.0 * 10.0 + 4.0 * PI * 9.0 * 5.0, 1e-4);

    // Mirror a boss at (10, 10) across the YZ plane of a UCS at x = 20.
    let mut p = plate(40.0, 30.0, 10.0);
    let t = top(&p, 10.0);
    let j = sketch_on(&mut p, &t, |sk, at| {
        sk.add_circle(at(10.0, 10.0, 10.0), 3.0).unwrap();
    });
    boss(&mut p, j, 5.0, |r| r.area() < 100.0);
    let boss_id = p.features.last().unwrap().id().to_owned();
    add(&mut p, ucs_offsets(20.0, 0.0, 0.0, 0.0, 0.0, 0.0));
    let mut m = PatternFeature::mirror();
    m.features = vec![boss_id];
    m.kind = PatternKind::Mirror {
        plane: Some(PlaneRef::Work("UCS1:YZ".into())),
    };
    p.add_pattern(m).unwrap();
    check(&p, 12_000.0 + 2.0 * PI * 9.0 * 5.0, 1e-4);

    // Draft of the plate's sides from a work plane on XY = from XY.
    let mut a = plate(40.0, 30.0, 10.0);
    let s = sides(&a, 40.0, 30.0, 5.0);
    draft_sides(&mut a, &s, 3.0);
    let mut b = plate(40.0, 30.0, 10.0);
    let wp = add(
        &mut b,
        WorkDef::Plane(P::Offset {
            base: PlaneRef::Origin(OriginPlane::XY),
            distance: 0.0,
        }),
    );
    let mut d = DraftFeature::new(3f64.to_radians());
    d.draft_type = DraftType::FixedPlane;
    d.plane = Some(PlaneRef::Work(wp));
    for (n, at) in &s {
        d.faces.push(face_ref(&b, n, *at));
    }
    b.add_draft(d).unwrap();
    check(&b, volume(&a), 1e-9);
}

/// Edits of earlier features: the cylinder grows, the tangent plane, the
/// pierce point and the axis follow (persistent names hold); Flip Normal;
/// Redefine; rollback.
#[test]
fn work_features_follow_edits_and_rollback() {
    let mut p = cylinder(0.0, 0.0, 10.0, 30.0);
    let f = cylinder_face(&p, 10.0);
    let wp = add(
        &mut p,
        WorkDef::Plane(P::TangentParallel {
            face: f.clone(),
            base: PlaneRef::Origin(OriginPlane::YZ),
            other: false,
        }),
    );
    let l = line(&mut p, [-50.0, 0.0, 5.0], [1.0, 0.0, 0.0]);
    let wpt = add(
        &mut p,
        WorkDef::Point(Q::SurfaceLine {
            face: f.clone(),
            line: l,
            index: 1,
        }),
    );
    // Grow the circle to r 14 (sketch edit) and regenerate.
    let sk = &mut p.sketches[0].sketch;
    let (c, _) = sk.entities().next().unwrap();
    sk.add_constraint(cadcraft3d::sketch::Constraint::Radius {
        circle: c,
        value: 14.0,
    })
    .unwrap();
    sk.solve().unwrap();
    p.regenerate().unwrap();
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    assert!((p.work.planes[&wp].origin.x - 14.0).abs() < EPS);
    assert!((p.work.points[&wpt] - Point3::new(14.0, 0.0, 5.0)).norm() < EPS);
    // Flip Normal.
    let k = p.features.iter().position(|x| x.id() == wp).unwrap();
    if let Feature::Work(w) = &mut p.features[k] {
        w.flip = true;
    }
    p.regenerate().unwrap();
    assert!((p.work.planes[&wp].normal.x + 1.0).abs() < EPS);
    // Redefine: the far side, same id.
    let mut w = match &p.features[k] {
        Feature::Work(w) => w.clone(),
        _ => panic!(),
    };
    w.def = WorkDef::Plane(P::TangentParallel {
        face: f,
        base: PlaneRef::Origin(OriginPlane::YZ),
        other: true,
    });
    assert_eq!(p.replace_feature(k, Feature::Work(w)).unwrap(), wp);
    assert!((p.work.planes[&wp].origin.x + 14.0).abs() < EPS);
    // A plane cannot be redefined as an axis.
    let mut bad = WorkFeature::new(WorkDef::Axis(A::OnLine {
        line: LineRef::Origin(0),
    }));
    bad.id = "x".into();
    assert!(p.replace_feature(k, Feature::Work(bad)).is_err());
    // Rolled back before the plane: it is not built, the cylinder is.
    p.rollback = Some(k);
    p.regenerate().unwrap();
    assert!(!p.work.planes.contains_key(&wp));
    p.rollback = None;
    p.regenerate().unwrap();
    assert!(p.work.planes.contains_key(&wp));
}

/// A missing input is health (the feature and everything built on it),
/// never a silent fallback; a save/load round trip keeps every method.
#[test]
fn missing_inputs_and_save_load() {
    let mut p = torus(0.0, 0.0, 15.0, 3.0);
    let f = one(&p, |s| matches!(s, Surface::Torus(_)), "torus");
    let mid = add(&mut p, WorkDef::Plane(P::TorusMidplane { face: f.clone() }));
    let c = add(&mut p, WorkDef::Point(Q::TorusCenter { face: f.clone() }));
    let ax = add(&mut p, WorkDef::Axis(A::RevolvedFace { face: f.clone() }));
    // Save / load.
    let json = serde_json::to_string(&p).unwrap();
    let mut back: PartDocument = serde_json::from_str(&json).unwrap();
    back.regenerate().unwrap();
    assert_eq!(back.features, p.features);
    assert!((back.work.points[&c] - p.work.points[&c]).norm() < EPS);
    assert!(back.work.planes.contains_key(&mid) && back.work.axes.contains_key(&ax));
    // The face is gone: plane, point and axis are sick.
    for w in &mut p.features {
        if let Feature::Work(w) = w {
            let s = serde_json::to_string(&w.def)
                .unwrap()
                .replace(&f, "Gone:Face");
            w.def = serde_json::from_str(&s).unwrap();
        }
    }
    p.regenerate().unwrap();
    for id in [&mid, &c, &ax] {
        let h = p
            .feature_health
            .get(id)
            .unwrap_or_else(|| panic!("{id} healthy"));
        assert!(h.contains("no longer exists"), "{h}");
    }
}

/// A UCS whose three points coincide is refused and the part stays.
#[test]
fn degenerate_ucs_is_refused() {
    let mut p = PartDocument::with_box("Box1", [10.0, 10.0, 10.0]).unwrap();
    let n = p.features.len();
    refused(
        p.add_work(WorkFeature::new(WorkDef::Ucs(UcsDef::ThreePoints {
            points: [
                PointRef::PartOrigin,
                PointRef::PartOrigin,
                PointRef::PartOrigin,
            ],
        }))),
        "coincide",
    );
    assert_eq!(p.features.len(), n);
}
