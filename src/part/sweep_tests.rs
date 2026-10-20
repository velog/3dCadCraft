use std::f64::consts::{FRAC_PI_2, PI};

use super::*;
use crate::part::extrude::ProfileRef;
use crate::part::placement::{OriginPlane, SketchPlacement};
use crate::part::{ExtrudeFeature, PartDocument, SketchOptions};
use crate::sketch::{Point2, Sketch};

fn opts() -> SketchOptions {
    SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    }
}

/// A sketch on an origin plane, drawn by `draw`; returns its index.
fn sketch(p: &mut PartDocument, plane: OriginPlane, draw: impl FnOnce(&mut Sketch)) -> usize {
    let k = p
        .create_sketch(SketchPlacement::on_origin_plane(plane), &opts())
        .unwrap();
    draw(&mut p.sketches[k].sketch);
    k
}

fn all_profiles(p: &PartDocument, k: usize) -> Vec<ProfileRef> {
    p.sketches[k]
        .sketch
        .regions()
        .iter()
        .map(|r| ProfileRef::of(r, r.interior_point()))
        .collect()
}

/// Every curve of sketch `k` as a path.
fn path_of(p: &PartDocument, k: usize) -> PathRef {
    let ps = &p.sketches[k];
    PathRef::Sketch {
        sketch: ps.id.clone(),
        curves: ps.sketch.entities().map(|(id, _)| id).collect(),
    }
}

fn volume(p: &PartDocument) -> Real {
    p.exact_bodies
        .0
        .iter()
        .map(|b| b.measure_full().unwrap().volume)
        .sum()
}

fn assert_vol(p: &PartDocument, want: Real, rel: Real) {
    assert!(p.exact, "built with the faceted kernel");
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    for (sid, _) in p.body.solids() {
        p.body.validate_solid(sid).unwrap();
    }
    let v = volume(p);
    assert!((v - want).abs() <= rel * want, "volume {v} vs {want}");
}

/// A circle (radius r) on XY at the origin and a path drawn on XZ (model
/// x/z = sketch x/y) by `path`.
fn circle_and_path(r: Real, path: impl FnOnce(&mut Sketch)) -> (PartDocument, usize, usize) {
    let mut p = PartDocument::default();
    let a = sketch(&mut p, OriginPlane::XY, |s| {
        s.add_circle(Point2::new(0.0, 0.0), r).unwrap();
    });
    let b = sketch(&mut p, OriginPlane::XZ, path);
    (p, a, b)
}

fn sweep_of(
    p: &mut PartDocument,
    a: usize,
    b: usize,
    f: impl FnOnce(&mut SweepFeature),
) -> Result<String, String> {
    let mut def = SweepFeature::new(all_profiles(p, a), path_of(p, b));
    f(&mut def);
    p.add_sweep(a, def).map_err(|e| e.to_string())
}

/// A quarter arc from the origin up to (R, 0, R) about (R, 0, 0).
fn quarter(s: &mut Sketch, r: Real) {
    let c = s.add_point(Point2::new(r, 0.0));
    let top = s.add_point(Point2::new(r, r));
    let o = s.add_point(Point2::new(0.0, 0.0));
    s.add_arc(c, top, o).unwrap();
}

#[test]
fn circle_along_a_line_is_a_cylinder() {
    for (r, h) in [
        (5.0, 50.0),
        (2.0, 10.0),
        (10.0, 3.0),
        (1.5, 80.0),
        (7.0, 25.0),
    ] {
        let (mut p, a, b) = circle_and_path(r, |s| {
            s.add_line(Point2::new(0.0, 0.0), Point2::new(0.0, h));
        });
        let id = sweep_of(&mut p, a, b, |_| {}).unwrap();
        assert_eq!(id, "Sweep1");
        assert_vol(&p, PI * r * r * h, 2e-3);
        let names: Vec<String> = p.body.faces().filter_map(|(_, f)| f.name.clone()).collect();
        assert!(names.iter().any(|n| n == "Sweep1:Start"), "{names:?}");
        assert!(names.iter().any(|n| n == "Sweep1:End"), "{names:?}");
        assert!(
            names.iter().any(|n| n.starts_with("Sweep1:Side:")),
            "{names:?}"
        );
        // Both sketches are consumed.
        assert!(!p.sketches[a].visible && !p.sketches[b].visible);
        assert_eq!(p.consumers(&p.sketches[b].id.clone()), vec![0]);
    }
}

#[test]
fn circle_along_an_arc_is_a_torus_segment() {
    for (r, big) in [
        (5.0, 30.0),
        (2.0, 20.0),
        (3.0, 50.0),
        (1.0, 8.0),
        (4.0, 12.0),
    ] {
        let (mut p, a, b) = circle_and_path(r, |s| quarter(s, big));
        sweep_of(&mut p, a, b, |_| {}).unwrap();
        // Pappus: area × the centre's path.
        assert_vol(&p, PI * r * r * big * FRAC_PI_2, 2e-3);
    }
}

#[test]
fn l_path_is_mitred() {
    for (w, l1, l2) in [
        (10.0, 40.0, 30.0),
        (4.0, 20.0, 20.0),
        (6.0, 50.0, 15.0),
        (2.0, 10.0, 30.0),
        (8.0, 30.0, 60.0),
    ] {
        let mut p = PartDocument::default();
        let a = sketch(&mut p, OriginPlane::XY, |s| {
            s.add_rectangle_two_point(
                Point2::new(-w / 2.0, -w / 2.0),
                Point2::new(w / 2.0, w / 2.0),
            )
            .unwrap();
        });
        let b = sketch(&mut p, OriginPlane::XZ, |s| {
            let o = s.add_point(Point2::new(0.0, 0.0));
            let c = s.add_point(Point2::new(0.0, l1));
            let e = s.add_point(Point2::new(l2, l1));
            s.add_line_between(o, c).unwrap();
            s.add_line_between(c, e).unwrap();
        });
        sweep_of(&mut p, a, b, |_| {}).unwrap();
        // A mitred bend keeps area × centre-line length.
        assert_vol(&p, w * w * (l1 + l2), 1e-3);
    }
}

#[test]
fn rectangle_along_an_arc() {
    for (w, d, big) in [
        (6.0, 4.0, 30.0),
        (2.0, 2.0, 10.0),
        (10.0, 3.0, 40.0),
        (4.0, 8.0, 25.0),
        (1.0, 5.0, 12.0),
    ] {
        let mut p = PartDocument::default();
        let a = sketch(&mut p, OriginPlane::XY, |s| {
            s.add_rectangle_two_point(
                Point2::new(-w / 2.0, -d / 2.0),
                Point2::new(w / 2.0, d / 2.0),
            )
            .unwrap();
        });
        let b = sketch(&mut p, OriginPlane::XZ, |s| quarter(s, big));
        sweep_of(&mut p, a, b, |_| {}).unwrap();
        assert_vol(&p, w * d * big * FRAC_PI_2, 2e-3);
    }
}

#[test]
fn taper_makes_a_frustum() {
    for (r, h, deg) in [
        (5.0, 40.0, 5.0),
        (8.0, 30.0, -6.0),
        (3.0, 20.0, 10.0),
        (6.0, 50.0, 2.0),
        (10.0, 10.0, -20.0),
    ] {
        let (mut p, a, b) = circle_and_path(r, |s| {
            s.add_line(Point2::new(0.0, 0.0), Point2::new(0.0, h));
        });
        let t: Real = Real::to_radians(deg);
        sweep_of(&mut p, a, b, |d| d.taper = t).unwrap();
        let r1 = r + h * t.tan();
        assert_vol(&p, PI * h / 3.0 * (r * r + r * r1 + r1 * r1), 5e-3);
    }
}

#[test]
fn twist_keeps_the_section_area() {
    for (w, h, deg) in [
        (10.0, 40.0, 90.0),
        (6.0, 30.0, 45.0),
        (8.0, 60.0, 180.0),
        (4.0, 20.0, -60.0),
        (12.0, 50.0, 30.0),
    ] {
        let mut p = PartDocument::default();
        let a = sketch(&mut p, OriginPlane::XY, |s| {
            s.add_rectangle_two_point(
                Point2::new(-w / 2.0, -w / 2.0),
                Point2::new(w / 2.0, w / 2.0),
            )
            .unwrap();
        });
        let b = sketch(&mut p, OriginPlane::XZ, |s| {
            s.add_line(Point2::new(0.0, 0.0), Point2::new(0.0, h));
        });
        sweep_of(&mut p, a, b, |d| d.twist = Real::to_radians(deg)).unwrap();
        // Every section is the same square, turned.
        assert_vol(&p, w * w * h, 1e-2);
    }
}

#[test]
fn sweep_cut_makes_a_groove() {
    for (r, len) in [
        (3.0, 60.0),
        (2.0, 40.0),
        (4.0, 50.0),
        (1.0, 20.0),
        (5.0, 70.0),
    ] {
        let mut p = PartDocument::default();
        let base = sketch(&mut p, OriginPlane::XY, |s| {
            s.add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(len, 40.0))
                .unwrap();
        });
        p.add_extrude(base, ExtrudeFeature::new(all_profiles(&p, base), 20.0))
            .unwrap();
        // Profile on YZ (sketch x = Y, y = Z) centred on the top edge.
        let a = sketch(&mut p, OriginPlane::YZ, |s| {
            s.add_circle(Point2::new(20.0, 20.0), r).unwrap();
        });
        let b = sketch(&mut p, OriginPlane::XY, |s| {
            s.add_line(Point2::new(0.0, 20.0), Point2::new(len, 20.0));
        });
        sweep_of(&mut p, a, b, |d| d.operation = Operation::Cut).unwrap();
        assert_vol(&p, len * 40.0 * 20.0 - PI * r * r / 2.0 * len, 2e-3);
    }
}

#[test]
fn guide_rail_scales_the_profile() {
    for (r, h, end) in [
        (5.0, 40.0, 10.0),
        (4.0, 30.0, 2.0),
        (6.0, 20.0, 9.0),
        (3.0, 50.0, 6.0),
        (8.0, 25.0, 4.0),
    ] {
        let k = end / r;
        for (scaling, want) in [
            (ProfileScaling::XY, PI * r * r * h * (1.0 + k + k * k) / 3.0),
            (ProfileScaling::X, PI * r * r * h * (1.0 + k) / 2.0),
            (ProfileScaling::None, PI * r * r * h),
        ] {
            let (mut p, a, b) = circle_and_path(r, |s| {
                s.add_line(Point2::new(0.0, 0.0), Point2::new(0.0, h));
            });
            let rail = sketch(&mut p, OriginPlane::XZ, |s| {
                s.add_line(Point2::new(r, 0.0), Point2::new(end, h));
            });
            let rail = path_of(&p, rail);
            sweep_of(&mut p, a, b, |d| {
                d.sweep_type = SweepType::PathAndGuideRail;
                d.rail = Some(rail);
                d.scaling = scaling;
            })
            .unwrap();
            assert_vol(&p, want, 1e-2);
        }
    }
}

#[test]
fn parallel_orientation_translates_the_profile() {
    for (r, big) in [
        (3.0, 30.0),
        (2.0, 20.0),
        (1.0, 10.0),
        (4.0, 40.0),
        (2.5, 15.0),
    ] {
        let (mut p, a, b) = circle_and_path(r, |s| quarter(s, big));
        sweep_of(&mut p, a, b, |d| d.orientation = SweepOrientation::Parallel).unwrap();
        // The section stays horizontal and climbs `big`.
        assert_vol(&p, PI * r * r * big, 1e-2);
    }
}

#[test]
fn model_edges_as_path() {
    let mut p = PartDocument::default();
    let base = sketch(&mut p, OriginPlane::XY, |s| {
        s.add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(40.0, 30.0))
            .unwrap();
    });
    p.add_extrude(base, ExtrudeFeature::new(all_profiles(&p, base), 10.0))
        .unwrap();
    // A rod around the vertical edge at (40, 30).
    let a = sketch(&mut p, OriginPlane::XY, |s| {
        s.add_circle(Point2::new(40.0, 30.0), 2.0).unwrap();
    });
    let e = p
        .body
        .edges()
        .find_map(|(e, _)| {
            let r = crate::part::blend::edge_ref(&p.body, e)?;
            let h = r.hint;
            ((h[0] - 40.0).abs() < 1e-6 && (h[1] - 30.0).abs() < 1e-6).then_some(r)
        })
        .unwrap();
    // The circle overlaps the corner: a region per piece; take the whole disc.
    let regions = p.sketches[a].sketch.regions();
    let disc: Vec<ProfileRef> = regions
        .iter()
        .map(|r| ProfileRef::of(r, r.interior_point()))
        .collect();
    let mut def = SweepFeature::new(disc, PathRef::Edges(vec![e]));
    def.operation = Operation::Join;
    p.add_sweep(a, def).unwrap();
    assert_vol(&p, 12000.0 + 0.75 * PI * 4.0 * 10.0, 3e-3);
}

#[test]
fn refusals_leave_the_part_unchanged() {
    let (mut p, a, b) = circle_and_path(5.0, |s| {
        s.add_line(Point2::new(0.0, 0.0), Point2::new(0.0, 30.0));
    });
    for (f, text) in [
        (
            Box::new(|d: &mut SweepFeature| d.solid_sweep = true) as Box<dyn Fn(&mut SweepFeature)>,
            "Solid Sweep",
        ),
        (
            Box::new(|d: &mut SweepFeature| d.output = Output::Surface),
            "Surface",
        ),
        (
            Box::new(|d: &mut SweepFeature| d.sweep_type = SweepType::PathAndGuideSurface),
            "Guide Surface",
        ),
        (
            Box::new(|d: &mut SweepFeature| d.sweep_type = SweepType::PathAndGuideRail),
            "guide rail",
        ),
        (
            Box::new(|d: &mut SweepFeature| d.operation = Operation::Cut),
            "body",
        ),
        (Box::new(|d: &mut SweepFeature| d.taper = 2.0), "taper"),
    ] {
        let e = sweep_of(&mut p, a, b, |d| f(d)).unwrap_err();
        assert!(e.contains(text), "{e}");
        assert!(p.features.is_empty());
        assert!(p.sketches[a].visible && p.sketches[b].visible);
    }
    // A path in the profile's own plane.
    let c = sketch(&mut p, OriginPlane::XY, |s| {
        s.add_line(Point2::new(10.0, 0.0), Point2::new(30.0, 0.0));
    });
    let mut def = SweepFeature::new(all_profiles(&p, a), path_of(&p, c));
    def.operation = Operation::NewSolid;
    let e = p.add_sweep(a, def).unwrap_err().to_string();
    assert!(e.contains("profile plane"), "{e}");
    // Two curves that do not meet.
    let d = sketch(&mut p, OriginPlane::XZ, |s| {
        s.add_line(Point2::new(0.0, 0.0), Point2::new(0.0, 10.0));
        s.add_line(Point2::new(5.0, 20.0), Point2::new(5.0, 30.0));
    });
    let def = SweepFeature::new(all_profiles(&p, a), path_of(&p, d));
    let e = p.add_sweep(a, def).unwrap_err().to_string();
    assert!(e.contains("not connected"), "{e}");
    assert!(p.features.is_empty());
}

#[test]
fn editing_the_path_sketch_rebuilds_the_sweep() {
    let (mut p, a, b) = circle_and_path(4.0, |s| {
        s.add_line(Point2::new(0.0, 0.0), Point2::new(0.0, 20.0));
    });
    sweep_of(&mut p, a, b, |_| {}).unwrap();
    assert_vol(&p, PI * 16.0 * 20.0, 2e-3);
    // Make the path 35 long.
    let sk = &mut p.sketches[b].sketch;
    let end = sk
        .points()
        .find(|(_, q)| (q.position.y - 20.0).abs() < 1e-9)
        .map(|(id, _)| id)
        .unwrap();
    sk.move_point(end, Point2::new(0.0, 35.0)).unwrap();
    p.regenerate().unwrap();
    assert_vol(&p, PI * 16.0 * 35.0, 2e-3);
    // Saved and loaded, it rebuilds the same.
    let json = serde_json::to_string(&p).unwrap();
    let mut q: PartDocument = serde_json::from_str(&json).unwrap();
    q.regenerate().unwrap();
    assert_vol(&q, PI * 16.0 * 35.0, 2e-3);
    // Edit Feature: a taper; a failing edit keeps the old one.
    let mut f = p.features[0].clone();
    if let crate::part::Feature::Sweep(s) = &mut f {
        s.taper = Real::to_radians(3.0);
    }
    p.replace_feature(0, f).unwrap();
    assert!(volume(&p) > PI * 16.0 * 35.0);
    let mut bad = p.features[0].clone();
    if let crate::part::Feature::Sweep(s) = &mut bad {
        s.taper = -1.5;
    }
    assert!(p.replace_feature(0, bad).is_err());
    assert!(volume(&p) > PI * 16.0 * 35.0);
}

#[test]
fn sweep_parameters() {
    let (mut p, a, b) = circle_and_path(4.0, |s| {
        s.add_line(Point2::new(0.0, 0.0), Point2::new(0.0, 20.0));
    });
    sweep_of(&mut p, a, b, |d| d.twist = 0.5).unwrap();
    let f = &p.features[0];
    assert_eq!(
        crate::part::params::feature_field_names(f),
        vec!["taper", "twist"]
    );
    assert_eq!(crate::part::params::feature_value(f, "twist"), Some(0.5));
}

#[test]
fn twisted_sweep_cut_through_a_block() {
    // SolidCraft #2954: a sweep cut with a 30° twist.
    for deg in [30.0, 15.0, 45.0, -30.0, 60.0] {
        let mut p = PartDocument::default();
        let base = sketch(&mut p, OriginPlane::XY, |s| {
            s.add_rectangle_two_point(Point2::new(-20.0, -20.0), Point2::new(20.0, 20.0))
                .unwrap();
        });
        p.add_extrude(base, ExtrudeFeature::new(all_profiles(&p, base), 40.0))
            .unwrap();
        let a = sketch(&mut p, OriginPlane::XY, |s| {
            s.add_rectangle_two_point(Point2::new(-5.0, -5.0), Point2::new(5.0, 5.0))
                .unwrap();
        });
        let b = sketch(&mut p, OriginPlane::XZ, |s| {
            s.add_line(Point2::new(0.0, 0.0), Point2::new(0.0, 50.0));
        });
        // The profile sketch has the square only (the base sketch is apart).
        sweep_of(&mut p, a, b, |d| {
            d.twist = Real::to_radians(deg);
            d.operation = Operation::Cut;
        })
        .unwrap();
        assert_vol(&p, 64000.0 - 100.0 * 40.0, 1e-2);
    }
}

/// A circle along an interpolation spline (a sampled path): the kernel's
/// path sweep needs the circle as quarter arcs (one whole-circle edge has a
/// single vertex and failed with "two same vertices").
#[test]
fn circle_along_a_spline() {
    for (r, top) in [
        (4.0, (30.0, 55.0)),
        (2.0, (10.0, 40.0)),
        (1.0, (-15.0, 30.0)),
    ] {
        let (mut p, a, b) = circle_and_path(r, |s| {
            let p0 = s.add_point(Point2::new(0.0, 0.0));
            let p1 = s.add_point(Point2::new(top.0 * 0.15, top.1 * 0.5));
            let p2 = s.add_point(Point2::new(top.0, top.1));
            s.add_interp_spline(vec![p0, p1, p2]).unwrap();
        });
        sweep_of(&mut p, a, b, |_| {}).unwrap();
        let len = p.sketches[b]
            .sketch
            .entities()
            .find_map(|(e, _)| p.sketches[b].sketch.spline_curve(e))
            .map(|c| {
                let d = c.domain();
                (0..400)
                    .map(|i| {
                        let t = |k: usize| d.min + (d.max - d.min) * k as Real / 400.0;
                        (c.point_at(t(i + 1)) - c.point_at(t(i))).norm()
                    })
                    .sum::<Real>()
            })
            .unwrap();
        assert_vol(&p, PI * r * r * len, 0.03);

// ------------------------------------------------- spline paths (session) ---

/// The owner's session: the spline through these sketch points on YZ.
const SESSION_PATH: [(Real, Real); 4] = [(0.0, 0.0), (0.0, 16.73), (9.20, 44.53), (36.42, 80.02)];

/// A circle (radius `r`, centre `c`) on XY and an interpolation spline
/// through `pts` on `plane`.
fn circle_and_spline(
    r: Real,
    c: (Real, Real),
    plane: OriginPlane,
    pts: &[(Real, Real)],
) -> (PartDocument, usize, usize) {
    let mut p = PartDocument::default();
    let a = sketch(&mut p, OriginPlane::XY, |s| {
        s.add_circle(Point2::new(c.0, c.1), r).unwrap();
    });
    let b = sketch(&mut p, plane, |s| {
        let ids = pts
            .iter()
            .map(|q| s.add_point(Point2::new(q.0, q.1)))
            .collect();
        s.add_interp_spline(ids).unwrap();
    });
    (p, a, b)
}

/// The resolved path of sketch `b` (from its start at the origin).
fn path_geom(p: &PartDocument, b: usize) -> PathGeom {
    path_of(p, b)
        .resolve(&p.sketches, &p.body, Point3::origin())
        .unwrap()
}

/// The path's length times the cosine of its start angle to the profile
/// (on XY): the kernel keeps that angle, so every section square to the
/// path has the profile's area times it (an interpolation spline's end
/// tangent leans towards its next points).
fn square_len(path: &PathGeom) -> Real {
    path.length() * path.eval(0.0).1.z.abs()
}

/// The signed turn of a planar path about `axis` (start to end tangent).
fn planar_turn(path: &PathGeom, axis: Vector3) -> Real {
    let pts = path.polyline();
    pts.windows(3)
        .map(|w| {
            let (a, b) = (w[1] - w[0], w[2] - w[1]);
            a.cross(&b).dot(&axis).atan2(a.dot(&b))
        })
        .sum()
}

#[test]
fn circle_along_a_spline_session_and_smaller_radii() {
    // The owner's sweep (R17.72) failed with "internal error": the whole
    // circle is one arc with one vertex and the point-path sweep panicked.
    for r in [17.721586226130526, 5.0, 10.0, 2.0, 14.0] {
        let (mut p, a, b) = circle_and_spline(r, (0.0, 0.0), OriginPlane::YZ, &SESSION_PATH);
        let len = square_len(&path_geom(&p, b));
        sweep_of(&mut p, a, b, |_| {}).unwrap();
        // Pappus: the centroid follows the path.
        assert_vol(&p, PI * r * r * len, 5e-3);
        let names: Vec<String> = p.body.faces().filter_map(|(_, f)| f.name.clone()).collect();
        for n in ["Sweep1:Start", "Sweep1:End"] {
            assert!(names.iter().any(|x| x == n), "{names:?}");
        }
        assert!(
            names.iter().any(|n| n.starts_with("Sweep1:Side:")),
            "{names:?}"
        );
    }
}

#[test]
fn circle_along_an_s_shaped_spline() {
    let s_path = [
        (0.0, 0.0),
        (0.0, 20.0),
        (20.0, 40.0),
        (0.0, 70.0),
        (0.0, 90.0),
    ];
    // The tightest bend (near (20, 40)) has a radius of about 5.7 mm.
    for r in [2.0, 4.0, 1.0, 3.0, 5.0] {
        let (mut p, a, b) = circle_and_spline(r, (0.0, 0.0), OriginPlane::XZ, &s_path);
        let len = square_len(&path_geom(&p, b));
        sweep_of(&mut p, a, b, |_| {}).unwrap();
        assert_vol(&p, PI * r * r * len, 5e-3);
    }
}

#[test]
fn spline_leaving_the_profile_at_an_angle() {
    // Path orientation keeps the angle between profile and path: the
    // section square to the path has area A·cos(θ0).
    for (r, pts) in [
        (4.0, vec![(0.0, 0.0), (10.0, 20.0), (15.0, 50.0)]),
        (3.0, vec![(0.0, 0.0), (8.0, 15.0), (10.0, 40.0)]),
        (5.0, vec![(0.0, 0.0), (-10.0, 25.0), (-5.0, 60.0)]),
        (
            2.0,
            vec![(0.0, 0.0), (5.0, 10.0), (20.0, 30.0), (25.0, 60.0)],
        ),
        (6.0, vec![(0.0, 0.0), (12.0, 30.0), (30.0, 60.0)]),
    ] {
        let (mut p, a, b) = circle_and_spline(r, (0.0, 0.0), OriginPlane::XZ, &pts);
        let path = path_geom(&p, b);
        let (_, t0) = path.eval(0.0);
        let cos = t0.z.abs();
        assert!(cos < 0.97, "the start is tilted: {cos}");
        let len = path.length();
        sweep_of(&mut p, a, b, |_| {}).unwrap();
        assert_vol(&p, PI * r * r * len * cos, 1e-2);
    }
}

#[test]
fn profile_off_the_spline_start() {
    for (r, (cx, cy)) in [
        (3.0, (8.0, 0.0)),
        (2.0, (-5.0, 0.0)),
        (3.0, (0.0, 6.0)),
        (2.5, (0.0, -6.0)),
        (2.0, (4.0, 4.0)),
    ] {
        let (mut p, a, b) = circle_and_spline(r, (cx, cy), OriginPlane::YZ, &SESSION_PATH);
        let path = path_geom(&p, b);
        // The path lies in YZ; the centroid's offset along X (square to the
        // path's plane) keeps the path length, one in the plane changes it
        // by offset × turn (Pappus along a planar curve).
        let turn = planar_turn(&path, Vector3::x());
        let centroid = (path.length() + cy * turn) * path.eval(0.0).1.z.abs();
        sweep_of(&mut p, a, b, |_| {}).unwrap();
        assert_vol(&p, PI * r * r * centroid, 5e-3);
    }
}

#[test]
fn profile_too_big_for_the_bend_is_refused() {
    // A tight bend: the curvature radius near (10, 30) is about 10 mm.
    let tight = [(0.0, 0.0), (0.0, 20.0), (10.0, 30.0), (30.0, 30.0)];
    for r in [30.0, 25.0, 40.0, 20.0, 50.0] {
        let (mut p, a, b) = circle_and_spline(r, (0.0, 0.0), OriginPlane::XZ, &tight);
        let e = sweep_of(&mut p, a, b, |_| {}).unwrap_err();
        assert!(e.contains("larger than the path's bend"), "{e}");
        assert!(p.features.is_empty());
        assert!(p.sketches[a].visible && p.sketches[b].visible);
    }
    // The same path with a small profile builds.
    let (mut p, a, b) = circle_and_spline(2.0, (0.0, 0.0), OriginPlane::XZ, &tight);
    let len = square_len(&path_geom(&p, b));
    sweep_of(&mut p, a, b, |_| {}).unwrap();
    assert_vol(&p, PI * 4.0 * len, 5e-3);
}

#[test]
fn spline_sweep_then_shell_and_fillet() {
    for r in [8.0, 10.0, 6.0, 12.0, 17.721586226130526] {
        // Shell: the kernel offsets analytic faces only, and the walls along
        // a spline are B-spline surfaces: a clear refusal, part unchanged.
        let (mut p, a, b) = circle_and_spline(r, (0.0, 0.0), OriginPlane::YZ, &SESSION_PATH);
        let len = square_len(&path_geom(&p, b));
        sweep_of(&mut p, a, b, |_| {}).unwrap();
        let end = p.body.find_face("Sweep1:End").unwrap();
        let mut def = crate::part::shell::ShellFeature::new(1.5);
        def.remove
            .push(crate::part::shell::face_ref(&p.body, end, None).unwrap());
        let e = p.add_shell(def).unwrap_err().to_string();
        assert!(e.contains("free-form faces"), "{e}");
        assert_eq!(p.features.len(), 1);
        assert_vol(&p, PI * r * r * len, 5e-3);

        // Fillet: the start cap's round edge, radius 1.
        let (mut p, a, b) = circle_and_spline(r, (0.0, 0.0), OriginPlane::YZ, &SESSION_PATH);
        sweep_of(&mut p, a, b, |_| {}).unwrap();
        let before = volume(&p);
        let edges: Vec<crate::part::blend::EdgeRef> = p
            .body
            .edges()
            .filter_map(|(e, _)| crate::part::blend::edge_ref(&p.body, e))
            .filter(|r| r.faces.iter().any(|f| f == "Sweep1:Start"))
            .collect();
        assert!(!edges.is_empty());
        let rho: Real = 1.0;
        p.add_blend(crate::part::blend::BlendFeature::fillet(edges, rho))
            .unwrap();
        // The spandrel (1 − π/4)ρ² turned about the axis at its centroid.
        let removed = 2.0 * PI * (r - 0.2234 * rho) * (1.0 - PI / 4.0) * rho * rho;
        assert_vol(&p, before - removed, 1e-3);
    }
}
