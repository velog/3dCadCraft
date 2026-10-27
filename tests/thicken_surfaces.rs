//! Thicken/Offset on curved and free-form surfaces (`docs/spec/2.14-thicken-offset.md`): the
//! hard cases of an offset — doubly curved B-spline faces, poles where a revolved profile meets
//! the axis, open and closed sheets, lofted and swept surfaces, the curved face of a solid.
//!
//! Checks: a sphere's shells against the exact volumes; an egg's against Steiner's formula for
//! parallel bodies (outward + inward average = A·t + 4π/3·t³ for a closed convex surface, A the
//! area; symmetric T = A·T + π/3·T³); a swept tube against Pappus (a profile swept along a
//! path has area × path length, so a 1 mm wall of an r 6 tube is 11/36 of the solid r 6 sweep).
//! Measured volumes use the display triangles (inscribed), so they run ~0.2 % low on curves.

mod common;

use cadcraft3d::kernel::Real;
use cadcraft3d::part::{
    AxisRef, BodyOp, BodyOpFeature, LoftFeature, LoftSection, Operation, OriginPlane, Output,
    PartDocument, PathRef, PlaneRef, ProfileRef, RevolveExtent, RevolveFeature, SketchPlacement,
    SweepFeature, ThickenDirection, ThickenOp, ThickenOutput, ThickenSelect, WorkDef, WorkFeature,
    WorkPlaneDef,
};
use cadcraft3d::sketch::{Point2, Sketch, Vector2};
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

/// Volume of the solids only.
fn solid_volume(part: &PartDocument) -> Real {
    part.exact_bodies
        .0
        .iter()
        .filter(|s| !s.is_open())
        .filter_map(|s| s.volume_at(1e-3))
        .sum()
}

/// Built by the exact kernel, no sick feature, every closed solid valid
/// (surface bodies are open by nature).
fn healthy(p: &PartDocument) {
    assert!(p.exact, "built with the faceted kernel");
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    for (sid, s) in p.body.solids() {
        if !s.open {
            p.body.validate_solid(sid).unwrap();
        }
    }
}

fn near(v: Real, want: Real, rel: Real, what: &str) {
    assert!(
        (v - want).abs() <= rel * want.abs(),
        "{what}: {v:.2} vs {want:.2} ({:.3} %)",
        (v / want - 1.0) * 100.0
    );
}

/// A body revolved about Z from a closed region on XZ: `profile` draws the
/// curve between two points on the axis and the axis line that closes it
/// (a surface drops it). `angle`: `None` = 360°.
fn revolved(
    angle: Option<Real>,
    output: Output,
    profile: impl FnOnce(&mut Sketch),
) -> PartDocument {
    let mut part = PartDocument::default();
    let i = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XZ), &opts())
        .unwrap();
    profile(&mut part.sketches[i].sketch);
    let r = part.sketches[i].sketch.regions();
    assert!(!r.is_empty(), "the profile closes no region");
    let mut rv = RevolveFeature::new(region_profiles(&r, |_| true), AxisRef::Origin(2));
    if let Some(a) = angle {
        rv.extent = RevolveExtent::Angle;
        rv.angle = a;
    }
    rv.output = output;
    part.add_revolve(i, rv).unwrap();
    healthy(&part);
    part
}

/// The egg's profile: an interpolation spline from the bottom pole (0, -20)
/// out to radius 16 and up to the top pole (0, 26). `smooth`: square to the
/// axis at both poles; else natural ends, which leave cone tips there.
fn egg_profile(smooth: bool) -> impl FnOnce(&mut Sketch) {
    move |sk: &mut Sketch| {
        let pts = [
            (0.0, -20.0),
            (9.0, -17.5),
            (14.5, -10.0),
            (16.0, 0.0),
            (14.0, 10.0),
            (9.0, 19.0),
            (0.0, 26.0),
        ];
        let ids: Vec<_> = pts
            .iter()
            .map(|&(x, z)| sk.add_point(Point2::new(x, z)))
            .collect();
        let s = sk.add_interp_spline(ids.clone()).unwrap();
        if smooth {
            sk.set_spline_handle(s, false, Some(Vector2::new(1.0, 0.0)))
                .unwrap();
            sk.set_spline_handle(s, true, Some(Vector2::new(-1.0, 0.0)))
                .unwrap();
        }
        sk.add_line_between(ids[6], ids[0]).unwrap();
    }
}

/// A semicircle of radius 20 about the origin, closed by the axis.
fn sphere_profile(sk: &mut Sketch) {
    let c = sk.add_point(Point2::new(0.0, 0.0));
    let s = sk.add_point(Point2::new(0.0, -20.0));
    let e = sk.add_point(Point2::new(0.0, 20.0));
    sk.add_arc(c, s, e).unwrap();
    sk.add_line_between(e, s).unwrap();
}

/// The part's (first) surface body thickened: Quilt, New Solid.
fn thicken_quilt(
    part: &mut PartDocument,
    t: Real,
    dir: ThickenDirection,
) -> Result<String, String> {
    let quilt = part
        .exact_bodies
        .0
        .iter()
        .find(|s| s.is_open())
        .and_then(|s| s.name.clone())
        .ok_or("no surface body")?;
    let mut op = ThickenOp::new();
    op.select = ThickenSelect::Quilt;
    op.quilts = vec![quilt];
    op.operation = Operation::NewSolid;
    op.distance = t;
    op.direction = dir;
    part.add_body_op(BodyOpFeature::new(BodyOp::Thicken(op)))
        .map_err(|e| e.to_string())
}

/// Every face of the part as a face reference (a point of its mesh).
fn all_faces(p: &PartDocument) -> Vec<cadcraft3d::part::FaceRef> {
    p.body
        .faces()
        .filter_map(|(_, f)| {
            // A triangle's centre: a mesh vertex can lie on the edge between
            // two faces of the same name and pick the neighbour.
            let m = f.mesh.as_ref()?;
            let [a, b, c] = m
                .triangles
                .get(m.triangles.len() / 2)?
                .map(|k| m.positions[k as usize]);
            let q = (a.coords + b.coords + c.coords) / 3.0;
            Some(face_ref(p, f.name.as_ref()?, [q.x, q.y, q.z]))
        })
        .collect()
}

fn ball(r: Real) -> Real {
    4.0 / 3.0 * PI * r.powi(3)
}

#[test]
fn sphere_shells_are_exact() {
    for (dir, want) in [
        (ThickenDirection::Direction1, ball(22.0) - ball(20.0)),
        (ThickenDirection::Direction2, ball(20.0) - ball(18.0)),
        (ThickenDirection::Symmetric, ball(21.0) - ball(19.0)),
    ] {
        let mut p = revolved(None, Output::Surface, sphere_profile);
        thicken_quilt(&mut p, 2.0, dir).unwrap_or_else(|e| panic!("{dir:?}: {e}"));
        healthy(&p);
        near(solid_volume(&p), want, 1e-3, &format!("sphere {dir:?}"));
    }
}

#[test]
fn egg_shells_follow_steiner() {
    let shell = |dir, t| {
        let mut p = revolved(None, Output::Surface, egg_profile(true));
        let a = sheet_area(&p);
        thicken_quilt(&mut p, t, dir).unwrap_or_else(|e| panic!("{dir:?}: {e}"));
        healthy(&p);
        (a, solid_volume(&p))
    };
    let t = 1.5;
    let (a, out) = shell(ThickenDirection::Direction1, t);
    let (_, inn) = shell(ThickenDirection::Direction2, t);
    assert!(
        out > a * t && inn < a * t,
        "out {out}, in {inn}, A·t {}",
        a * t
    );
    near(
        (out + inn) / 2.0,
        a * t + 4.0 / 3.0 * PI * t.powi(3),
        5e-3,
        "egg out/in average",
    );
    let big = 3.0;
    let (_, sym) = shell(ThickenDirection::Symmetric, big);
    near(sym, a * big + PI / 3.0 * big.powi(3), 5e-3, "egg symmetric");
}

#[test]
fn open_egg_pieces_thicken() {
    // Half (180°, outward) and a quarter (90°, inward) of the egg: open
    // sheets with walls along the free edges, exactly that share of the
    // whole egg's shell (the walls lie in the cut planes).
    let whole = |dir| {
        let mut p = revolved(None, Output::Surface, egg_profile(true));
        thicken_quilt(&mut p, 2.0, dir).unwrap_or_else(|e| panic!("{dir:?}: {e}"));
        solid_volume(&p)
    };
    for (angle, dir, share) in [
        (PI, ThickenDirection::Direction1, 0.5),
        (PI / 2.0, ThickenDirection::Direction2, 0.25),
    ] {
        let mut p = revolved(Some(angle), Output::Surface, egg_profile(true));
        thicken_quilt(&mut p, 2.0, dir).unwrap_or_else(|e| panic!("{angle}: {e}"));
        healthy(&p);
        near(
            solid_volume(&p),
            whole(dir) * share,
            5e-3,
            &format!("open egg {angle:.2}"),
        );
    }
}

#[test]
fn a_tipped_egg_is_refused_and_changes_nothing() {
    // Natural spline ends meet the axis at an angle: cone tips at the poles,
    // whose offset is no smooth face. Refused, with nothing added.
    let mut p = revolved(None, Output::Surface, egg_profile(false));
    let before = p.features.len();
    let e = thicken_quilt(&mut p, 1.5, ThickenDirection::Direction1).unwrap_err();
    assert!(e.contains("sharp in places") || e.contains("fitted"), "{e}");
    assert_eq!(p.features.len(), before);
}

#[test]
fn the_curved_face_of_a_solid_sphere() {
    // Face mode on a solid: Join 2 out and Cut 2 in give the bigger and the
    // smaller ball.
    for (op, dir, want) in [
        (Operation::Join, ThickenDirection::Direction1, ball(22.0)),
        (Operation::Cut, ThickenDirection::Direction2, ball(18.0)),
    ] {
        let mut p = revolved(None, Output::Solid, sphere_profile);
        let mut t = ThickenOp::new();
        t.faces = all_faces(&p);
        t.operation = op;
        t.direction = dir;
        t.distance = 2.0;
        p.add_body_op(BodyOpFeature::new(BodyOp::Thicken(t)))
            .unwrap_or_else(|e| panic!("{op:?}: {e}"));
        healthy(&p);
        near(solid_volume(&p), want, 2e-3, &format!("sphere face {op:?}"));
    }
}

#[test]
fn offset_surface_of_the_egg() {
    let mut p = revolved(None, Output::Surface, egg_profile(true));
    let a = sheet_area(&p);
    let mut t = ThickenOp::new();
    t.faces = all_faces(&p);
    t.output = ThickenOutput::Surface;
    t.distance = 2.0;
    t.automatic_face_chain = false;
    p.add_body_op(BodyOpFeature::new(BodyOp::Thicken(t)))
        .unwrap();
    healthy(&p);
    assert_eq!(p.exact_bodies.0.iter().filter(|s| s.is_open()).count(), 2);
    // The offset of a convex surface outward is larger: A' = A + 2·M·d + 4π·d².
    let a2 = sheet_area(&p) - a;
    assert!(a2 > a + 4.0 * PI * 4.0, "offset area {a2} vs {a}");
}

/// A sketch on a plane parallel to XY at height `z`.
fn at_height(p: &mut PartDocument, z: Real, draw: impl FnOnce(&mut Sketch)) -> usize {
    let placement = if z.abs() < 1e-12 {
        SketchPlacement::on_origin_plane(OriginPlane::XY)
    } else {
        let wp = p
            .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
                base: PlaneRef::Origin(OriginPlane::XY),
                distance: z,
            })))
            .unwrap();
        SketchPlacement {
            plane: PlaneRef::Work(wp),
            ..Default::default()
        }
    };
    let k = p.create_sketch(placement, &opts()).unwrap();
    draw(&mut p.sketches[k].sketch);
    k
}

fn section(p: &PartDocument, k: usize) -> LoftSection {
    let ps = &p.sketches[k];
    let r = ps.sketch.regions()[0].clone();
    LoftSection::Profile {
        sketch: ps.id.clone(),
        profile: ProfileRef::of(&r, r.interior_point()),
    }
}

/// A surface loft through sections at 0, 30 and 60 (`circle`: circles of
/// radius r, else rectangles 2r × 1.2r).
fn surface_loft(kinds: [bool; 3]) -> PartDocument {
    let mut p = PartDocument::default();
    let mut ks = Vec::new();
    for (k, circle) in kinds.into_iter().enumerate() {
        let r = [15.0, 10.0, 8.0][k];
        ks.push(at_height(
            &mut p,
            30.0 * k as Real,
            move |s: &mut Sketch| {
                if circle {
                    s.add_circle(Point2::new(0.0, 0.0), r).unwrap();
                } else {
                    s.add_rectangle_two_point(Point2::new(-r, -r * 0.6), Point2::new(r, r * 0.6))
                        .unwrap();
                }
            },
        ));
    }
    let mut def = LoftFeature::new(ks.iter().map(|&k| section(&p, k)).collect());
    def.output = Output::Surface;
    p.add_loft(def)
        .unwrap_or_else(|e| panic!("surface loft: {e}"));
    healthy(&p);
    assert_eq!(p.exact_bodies.0.iter().filter(|s| s.is_open()).count(), 1);
    assert!(solid_volume(&p) < 1e-6, "a surface loft has no volume");
    p
}

#[test]
fn lofted_surfaces_thicken() {
    // Smooth (three circles: one smooth doubly curved face per quarter) and
    // creased (three rectangles: sharp edges along their whole length).
    // Smooth (three circles, one doubly curved face per quarter), out and in
    // by 1.5: on an open surface the curvature terms cancel in the average,
    // which is A·t.
    let mut v = Vec::new();
    let mut area = 0.0;
    for dir in [ThickenDirection::Direction1, ThickenDirection::Direction2] {
        let mut p = surface_loft([true; 3]);
        area = sheet_area(&p);
        thicken_quilt(&mut p, 1.5, dir).unwrap_or_else(|e| panic!("{dir:?}: {e}"));
        healthy(&p);
        v.push(solid_volume(&p));
    }
    near(
        (v[0] + v[1]) / 2.0,
        area * 1.5,
        1e-3,
        "smooth loft out/in average",
    );
    // Creased (three rectangles, sharp edges along their whole length):
    // outward the four creases add their corners (a quarter disc each at
    // most), inward the kernel refuses the inside corners — nothing changes.
    let mut p = surface_loft([false; 3]);
    let area = sheet_area(&p);
    thicken_quilt(&mut p, 1.5, ThickenDirection::Direction1).unwrap();
    healthy(&p);
    let out = solid_volume(&p);
    // Each crease is about 61 long; a square corner would add t² per mm.
    let corners = out - area * 1.5;
    assert!(
        corners > 0.0 && corners < 4.0 * 61.0 * 1.5 * 1.5,
        "creased loft: corners add {corners:.1}"
    );
    let mut p = surface_loft([false; 3]);
    let before = p.features.len();
    let e = thicken_quilt(&mut p, 1.5, ThickenDirection::Direction2).unwrap_err();
    assert!(e.contains("inside corner"), "{e}");
    assert_eq!(p.features.len(), before);
    // Circle → rectangle → circle: the seams are sharp at the rectangle's
    // corners and smooth at the circles. Refused with the reason, nothing
    // added.
    let mut p = surface_loft([true, false, true]);
    let before = p.features.len();
    let e = thicken_quilt(&mut p, 1.5, ThickenDirection::Direction1).unwrap_err();
    assert!(e.contains("sharp in places"), "{e}");
    assert_eq!(p.features.len(), before);
}

#[test]
fn a_swept_tube_follows_pappus() {
    // A circle r 6 on XY swept along an interpolation spline in XZ: the
    // tube surface (Sweep with Output Surface) thickened 1 mm inward is
    // 11/36 of the solid r 6 sweep (area × path length).
    let build = |output: Output| {
        let mut p = PartDocument::default();
        let k = p
            .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
            .unwrap();
        p.sketches[k]
            .sketch
            .add_circle(Point2::new(0.0, 0.0), 6.0)
            .unwrap();
        let j = p
            .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XZ), &opts())
            .unwrap();
        let sk = &mut p.sketches[j].sketch;
        let ids: Vec<_> = [(0.0, 0.0), (10.0, 25.0), (-5.0, 50.0), (15.0, 80.0)]
            .iter()
            .map(|&(x, z)| sk.add_point(Point2::new(x, z)))
            .collect();
        let s = sk.add_interp_spline(ids).unwrap();
        sk.set_spline_handle(s, false, Some(Vector2::new(0.0, 1.0)))
            .unwrap();
        let profiles = p.sketches[k]
            .sketch
            .regions()
            .iter()
            .map(|r| ProfileRef::of(r, r.interior_point()))
            .collect();
        let path = PathRef::Sketch {
            sketch: p.sketches[j].id.clone(),
            curves: vec![s],
        };
        let mut def = SweepFeature::new(profiles, path);
        def.output = output;
        p.add_sweep(k, def)
            .unwrap_or_else(|e| panic!("{output:?}: {e}"));
        healthy(&p);
        p
    };
    // One direction grows the wall into the tube (r 5…6: 11/36), the other
    // out of it (r 6…7: 13/36): the surface's normals decide which.
    let solid = solid_volume(&build(Output::Solid));
    let mut walls = Vec::new();
    for dir in [ThickenDirection::Direction1, ThickenDirection::Direction2] {
        let mut tube = build(Output::Surface);
        assert_eq!(
            tube.exact_bodies.0.iter().filter(|s| s.is_open()).count(),
            1
        );
        thicken_quilt(&mut tube, 1.0, dir).unwrap_or_else(|e| panic!("thicken the tube: {e}"));
        healthy(&tube);
        walls.push(solid_volume(&tube) / solid * 36.0);
    }
    walls.sort_by(|a, b| a.total_cmp(b));
    near(walls[0], 11.0, 0.01, "swept tube wall inside (of 36)");
    near(walls[1], 13.0, 0.01, "swept tube wall outside (of 36)");
}

#[test]
fn revolved_eggs_of_any_angle() {
    // A revolve of any angle is that share of the whole egg's shell (the
    // walls lie in the cut planes). Before the kernel's span fix a 150° egg
    // came out 34 % too thick, a 190° one 6×, 120° and 180° failed.
    let whole = {
        let mut p = revolved(None, Output::Surface, egg_profile(true));
        thicken_quilt(&mut p, 2.0, ThickenDirection::Direction1).unwrap();
        solid_volume(&p)
    };
    for deg in [90.0, 120.0, 150.0, 180.0, 190.0, 240.0, 300.0] {
        let mut p = revolved(
            Some(Real::to_radians(deg)),
            Output::Surface,
            egg_profile(true),
        );
        thicken_quilt(&mut p, 2.0, ThickenDirection::Direction1)
            .unwrap_or_else(|e| panic!("{deg}°: {e}"));
        healthy(&p);
        near(
            solid_volume(&p),
            whole * deg / 360.0,
            2e-3,
            &format!("{deg}° egg"),
        );
    }
}
