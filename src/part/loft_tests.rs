use super::*;
use crate::part::extrude::ProfileRef;
use crate::part::placement::{OriginPlane, PlaneRef, SketchPlacement};
use crate::part::{
    ExtrudeFeature, PartDocument, SketchOptions, WorkDef, WorkFeature, WorkPlaneDef,
};
use crate::sketch::{Point2, Sketch};

fn opts() -> SketchOptions {
    SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    }
}

/// A sketch on a plane parallel to XY at height `z` (an offset work plane
/// above 0), drawn by `draw`; returns its index.
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

/// The region of sketch `k` containing `at` (the only one when `None`).
fn section(p: &PartDocument, k: usize) -> LoftSection {
    let ps = &p.sketches[k];
    let regions = ps.sketch.regions();
    let r = regions
        .iter()
        .max_by(|a, b| {
            (a.holes.len(), a.area())
                .partial_cmp(&(b.holes.len(), b.area()))
                .unwrap()
        })
        .unwrap();
    LoftSection::Profile {
        sketch: ps.id.clone(),
        profile: ProfileRef::of(r, r.interior_point()),
    }
}

fn square(s: &mut Sketch, w: Real) {
    s.add_rectangle_two_point(
        Point2::new(-w / 2.0, -w / 2.0),
        Point2::new(w / 2.0, w / 2.0),
    )
    .unwrap();
}

fn volume(p: &PartDocument) -> Real {
    p.exact_bodies
        .0
        .iter()
        .map(|b| b.measure_full().unwrap().volume)
        .sum()
}

fn healthy(p: &PartDocument) {
    assert!(p.exact, "built with the faceted kernel");
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    for (sid, _) in p.body.solids() {
        p.body.validate_solid(sid).unwrap();
    }
}

fn loft_of(
    p: &mut PartDocument,
    ks: &[usize],
    f: impl FnOnce(&mut LoftFeature),
) -> Result<String, String> {
    let mut def = LoftFeature::new(ks.iter().map(|&k| section(p, k)).collect());
    f(&mut def);
    p.add_loft(def).map_err(|e| e.to_string())
}

#[test]
fn two_squares_make_a_prism() {
    for (w, h) in [
        (20.0, 30.0),
        (10.0, 5.0),
        (40.0, 12.0),
        (6.0, 50.0),
        (15.0, 15.0),
    ] {
        let mut p = PartDocument::default();
        let a = at_height(&mut p, 0.0, |s| square(s, w));
        let b = at_height(&mut p, h, |s| square(s, w));
        let id = loft_of(&mut p, &[a, b], |_| {}).unwrap();
        assert_eq!(id, "Loft1");
        healthy(&p);
        let v = volume(&p);
        assert!((v - w * w * h).abs() < 1e-3 * w * w * h, "{v}");
        let names: Vec<String> = p.body.faces().filter_map(|(_, f)| f.name.clone()).collect();
        for n in ["Loft1:Start", "Loft1:End"] {
            assert!(names.iter().any(|x| x == n), "{names:?}");
        }
        assert!(
            names.iter().any(|x| x.starts_with("Loft1:Side:")),
            "{names:?}"
        );
        assert!(!p.sketches[a].visible && !p.sketches[b].visible);
    }
}

#[test]
fn square_to_circle() {
    for (w, r, h) in [
        (40.0, 15.0, 40.0),
        (20.0, 12.0, 10.0),
        (10.0, 3.0, 30.0),
        (30.0, 20.0, 25.0),
        (8.0, 5.0, 8.0),
    ] {
        let mut p = PartDocument::default();
        let a = at_height(&mut p, 0.0, |s| square(s, w));
        let b = at_height(&mut p, h, |s| {
            s.add_circle(Point2::new(0.0, 0.0), r).unwrap();
        });
        loft_of(&mut p, &[a, b], |_| {}).unwrap();
        healthy(&p);
        // Below the larger end prism, and a little below
        // the average of the end areas (the middle section of a ruled
        // transition is smaller than the average).
        let (a0, a1) = (w * w, std::f64::consts::PI * r * r);
        let v = volume(&p);
        assert!(v < a0.max(a1) * h, "{v}");
        let mid = (a0 + a1) / 2.0 * h;
        assert!(v < mid && v > 0.85 * mid, "{v} vs {mid}");
    }
}

#[test]
fn three_sections_are_smooth() {
    for (w0, w1, h) in [
        (20.0, 30.0, 20.0),
        (10.0, 6.0, 10.0),
        (30.0, 40.0, 15.0),
        (8.0, 16.0, 30.0),
        (12.0, 10.0, 5.0),
    ] {
        let mut p = PartDocument::default();
        let a = at_height(&mut p, 0.0, |s| square(s, w0));
        let b = at_height(&mut p, h, |s| square(s, w1));
        let c = at_height(&mut p, 2.0 * h, |s| square(s, w0));
        loft_of(&mut p, &[a, b, c], |_| {}).unwrap();
        healthy(&p);
        let v = volume(&p);
        let (lo, hi) = (w0.min(w1).powi(2) * 2.0 * h, w0.max(w1).powi(2) * 2.0 * h);
        assert!(v > lo && v < hi, "{v} not in {lo}..{hi}");
        // A smooth bulge: more than the two ruled frusta.
        let frustum = |a: Real, b: Real| h / 3.0 * (a * a + a * b + b * b);
        let ruled = 2.0 * frustum(w0, w1);
        if w1 > w0 {
            assert!(v > ruled, "{v} vs ruled {ruled}");
        } else {
            assert!(v < ruled, "{v} vs ruled {ruled}");
        }
        // Only the two caps are planar faces across: one side face per
        // segment, no edges at the middle section.
        let sides = p
            .body
            .faces()
            .filter(|(_, f)| f.name.as_deref().is_some_and(|n| n.contains(":Side:")))
            .count();
        assert_eq!(sides, 4);
    }
}

#[test]
fn loft_to_a_point_is_a_pyramid() {
    for (w, h) in [
        (40.0, 30.0),
        (10.0, 10.0),
        (20.0, 5.0),
        (6.0, 40.0),
        (30.0, 18.0),
    ] {
        let mut p = PartDocument::default();
        let a = at_height(&mut p, 0.0, |s| square(s, w));
        let b = at_height(&mut p, h, |s| {
            s.add_point(Point2::new(0.0, 0.0));
        });
        let point = p.sketches[b]
            .sketch
            .points()
            .next()
            .map(|(id, _)| id)
            .unwrap();
        let def = LoftFeature::new(vec![
            section(&p, a),
            LoftSection::Point {
                sketch: p.sketches[b].id.clone(),
                point,
            },
        ]);
        p.add_loft(def).unwrap();
        healthy(&p);
        let v = volume(&p);
        assert!((v - w * w * h / 3.0).abs() < 1e-6 * w * w * h, "{v}");
    }
}

#[test]
fn ring_sections_stay_hollow() {
    // SolidCraft #1184: a ring section lofted solid. Here the hole is
    // lofted too and cut out.
    for (ro, ri, h) in [
        (20.0, 10.0, 30.0),
        (10.0, 4.0, 10.0),
        (15.0, 14.0, 20.0),
        (30.0, 5.0, 8.0),
        (8.0, 6.0, 40.0),
    ] {
        let mut p = PartDocument::default();
        let ring = |s: &mut Sketch| {
            s.add_circle(Point2::new(0.0, 0.0), ro).unwrap();
            s.add_circle(Point2::new(0.0, 0.0), ri).unwrap();
        };
        let a = at_height(&mut p, 0.0, ring);
        let b = at_height(&mut p, h, ring);
        loft_of(&mut p, &[a, b], |_| {}).unwrap();
        healthy(&p);
        let want = std::f64::consts::PI * (ro * ro - ri * ri) * h;
        let v = volume(&p);
        assert!((v - want).abs() < 5e-3 * want, "{v} vs {want}");
    }
}

#[test]
fn rotated_polygons_pair_their_vertices() {
    // SolidCraft #881 / #2847: equal polygons turned against each other
    // (also by more than 90°, the same as a small turn for a square).
    for deg in [10.0, 20.0, 100.0, -15.0, 185.0] {
        let mut p = PartDocument::default();
        let a = at_height(&mut p, 0.0, |s| square(s, 20.0));
        let t: Real = Real::to_radians(deg);
        let b = at_height(&mut p, 20.0, |s| {
            let pts: Vec<_> = (0..4)
                .map(|i| {
                    let u =
                        t + std::f64::consts::FRAC_PI_4 + std::f64::consts::FRAC_PI_2 * i as Real;
                    s.add_point(Point2::new(
                        10.0 * 2f64.sqrt() * u.cos(),
                        10.0 * 2f64.sqrt() * u.sin(),
                    ))
                })
                .collect();
            for i in 0..4 {
                s.add_line_between(pts[i], pts[(i + 1) % 4]).unwrap();
            }
        });
        loft_of(&mut p, &[a, b], |_| {}).unwrap();
        healthy(&p);
        let v = volume(&p);
        // A small twist loses a little volume, never more than a few %.
        assert!(v < 8000.0 + 1.0 && v > 0.93 * 8000.0, "{deg}: {v}");
    }
}

#[test]
fn closed_loop_ring() {
    let mut p = PartDocument::default();
    // Circles r = 4 about the Z axis at radius 30, in the XZ and YZ planes.
    let xz = |p: &mut PartDocument, x: Real| {
        let k = p
            .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XZ), &opts())
            .unwrap();
        p.sketches[k]
            .sketch
            .add_circle(Point2::new(x, 0.0), 4.0)
            .unwrap();
        k
    };
    let yz = |p: &mut PartDocument, y: Real| {
        let k = p
            .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::YZ), &opts())
            .unwrap();
        p.sketches[k]
            .sketch
            .add_circle(Point2::new(y, 0.0), 4.0)
            .unwrap();
        k
    };
    let s0 = xz(&mut p, 30.0);
    let s1 = yz(&mut p, 30.0);
    let s2 = xz(&mut p, -30.0);
    let s3 = yz(&mut p, -30.0);
    let r = loft_of(&mut p, &[s0, s1, s2, s3], |d| d.closed_loop = true);
    match r {
        Ok(_) => {
            healthy(&p);
            let torus = std::f64::consts::PI * 16.0 * std::f64::consts::TAU * 30.0;
            let v = volume(&p);
            assert!((v - torus).abs() < 0.25 * torus, "{v} vs {torus}");
        }
        // A clear refusal is acceptable, never a wrong body.
        Err(e) => assert!(e.contains("kernel"), "{e}"),
    }
}

#[test]
fn loft_join_and_cut() {
    for op in [Operation::Join, Operation::Cut] {
        let mut p = PartDocument::default();
        let base = at_height(&mut p, 0.0, |s| square(s, 40.0));
        let pr = {
            let r = p.sketches[base].sketch.regions();
            vec![ProfileRef::of(&r[0], r[0].interior_point())]
        };
        p.add_extrude(base, ExtrudeFeature::new(pr, 10.0)).unwrap();
        let (z0, z1) = if op == Operation::Join {
            (10.0, 30.0)
        } else {
            (2.0, 10.0)
        };
        let a = at_height(&mut p, z0, |s| square(s, 20.0));
        let b = at_height(&mut p, z1, |s| square(s, 10.0));
        loft_of(&mut p, &[a, b], |d| d.operation = op).unwrap();
        healthy(&p);
        let h = z1 - z0;
        let frustum = h / 3.0 * (400.0 + 200.0 + 100.0);
        let want = if op == Operation::Join {
            16000.0 + frustum
        } else {
            16000.0 - frustum
        };
        let v = volume(&p);
        assert!((v - want).abs() < 1e-3 * want, "{op:?}: {v} vs {want}");
        assert_eq!(p.exact_bodies.0.len(), 1);
    }
}

#[test]
fn refusals_and_edits() {
    let mut p = PartDocument::default();
    let a = at_height(&mut p, 0.0, |s| square(s, 20.0));
    let b = at_height(&mut p, 20.0, |s| square(s, 10.0));
    for (f, text) in [
        (
            Box::new(|d: &mut LoftFeature| d.output = Output::Surface)
                as Box<dyn Fn(&mut LoftFeature)>,
            "Surface",
        ),
        (
            Box::new(|d: &mut LoftFeature| {
                d.rails = vec![PathRef::Sketch {
                    sketch: "x".into(),
                    curves: Vec::new(),
                }]
            }),
            "Rails",
        ),
        (
            Box::new(|d: &mut LoftFeature| d.guide = LoftGuide::CenterLine),
            "Center Line",
        ),
        (
            Box::new(|d: &mut LoftFeature| d.guide = LoftGuide::AreaLoft),
            "Area Loft",
        ),
        (
            Box::new(|d: &mut LoftFeature| {
                d.conditions = vec![LoftCondition {
                    kind: ConditionKind::Tangent,
                    ..Default::default()
                }]
            }),
            "Tangent",
        ),
        (
            Box::new(|d: &mut LoftFeature| d.closed_loop = true),
            "three sections",
        ),
        (
            Box::new(|d: &mut LoftFeature| d.operation = Operation::Cut),
            "body",
        ),
    ] {
        let e = loft_of(&mut p, &[a, b], |d| f(d)).unwrap_err();
        assert!(e.contains(text), "{e}");
        assert!(
            p.features
                .iter()
                .all(|f| !matches!(f, crate::part::Feature::Loft(_)))
        );
    }
    // Two sections on one plane.
    let c = at_height(&mut p, 0.0, |s| square(s, 5.0));
    let e = loft_of(&mut p, &[a, c], |_| {}).unwrap_err();
    assert!(e.contains("same plane"), "{e}");
    // One section is not a loft.
    assert!(loft_of(&mut p, &[a], |_| {}).is_err());
    // A point in the middle.
    let pt = at_height(&mut p, 10.0, |s| {
        s.add_point(Point2::new(0.0, 0.0));
    });
    let point = p.sketches[pt]
        .sketch
        .points()
        .next()
        .map(|(id, _)| id)
        .unwrap();
    let def = LoftFeature::new(vec![
        section(&p, a),
        LoftSection::Point {
            sketch: p.sketches[pt].id.clone(),
            point,
        },
        section(&p, b),
    ]);
    let e = p.add_loft(def).unwrap_err().to_string();
    assert!(e.contains("first or the last"), "{e}");
    // A real loft; then the top section grows: the loft follows.
    loft_of(&mut p, &[a, b], |_| {}).unwrap();
    let v0 = volume(&p);
    let sk = &mut p.sketches[b].sketch;
    let ids: Vec<_> = sk.points().map(|(id, q)| (id, q.position)).collect();
    for (id, q) in ids {
        sk.move_point(id, Point2::new(q.x * 2.0, q.y * 2.0))
            .unwrap();
    }
    p.regenerate().unwrap();
    healthy(&p);
    let v1 = volume(&p);
    assert!((v1 - 8000.0).abs() < 1.0, "{v0} → {v1}");
    // Saved and loaded.
    let json = serde_json::to_string(&p).unwrap();
    let mut q: PartDocument = serde_json::from_str(&json).unwrap();
    q.regenerate().unwrap();
    assert!((volume(&q) - v1).abs() < 1e-6);
}
