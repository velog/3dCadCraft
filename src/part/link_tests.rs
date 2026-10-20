//! Project Geometry from other sketches and curved model edges
//! (`docs/spec/1.8-project-geometry.md`, [`super::link`]).

use super::*;
use crate::part::extrude::ProfileRef;
use crate::part::placement::{OriginPlane, PlaneRef, SketchPlacement};
use crate::part::{
    ExtrudeFeature, PartDocument, PathRef, SketchOptions, SweepFeature, WorkDef, WorkFeature,
    WorkPlaneDef,
};
use crate::sketch::{Constraint, Entity, Point2, Sketch};

fn opts() -> SketchOptions {
    SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    }
}

fn on(p: &mut PartDocument, plane: OriginPlane, draw: impl FnOnce(&mut Sketch)) -> usize {
    let k = p
        .create_sketch(SketchPlacement::on_origin_plane(plane), &opts())
        .unwrap();
    draw(&mut p.sketches[k].sketch);
    k
}

/// A work plane parallel to XY at height `z`; returns its id.
fn plane_at(p: &mut PartDocument, z: Real) -> String {
    p.add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
        base: PlaneRef::Origin(OriginPlane::XY),
        distance: z,
    })))
    .unwrap()
}

fn on_work(p: &mut PartDocument, wp: &str, draw: impl FnOnce(&mut Sketch)) -> usize {
    let placement = SketchPlacement {
        plane: PlaneRef::Work(wp.to_owned()),
        ..Default::default()
    };
    let k = p.create_sketch(placement, &opts()).unwrap();
    draw(&mut p.sketches[k].sketch);
    k
}

/// The only circle of sketch `k` and its centre point.
fn circle_of(p: &PartDocument, k: usize) -> (EntityId, PointId) {
    let sk = &p.sketches[k].sketch;
    sk.entities()
        .find_map(|(id, e)| match e {
            Entity::Circle { center, .. } => Some((id, *center)),
            _ => None,
        })
        .unwrap()
}

/// Model position of point `pt` of sketch `k`.
fn world(p: &PartDocument, k: usize, pt: PointId) -> Point3 {
    let sk = &p.sketches[k].sketch;
    sk.to_world(&sk.point(pt).unwrap().position)
}

fn close(a: Point2, b: Point2) {
    assert!((a - b).norm() < 1e-7, "{a:?} vs {b:?}");
}

fn center_geo(pt: PointId) -> SketchGeo {
    SketchGeo::Point { point: pt }
}

/// A circle centre on XY projected into a sketch on YZ: a fixed point at
/// the orthogonal projection (YZ: x = model Y, y = model Z).
#[test]
fn circle_centre_from_xy_into_yz() {
    for (cx, cy) in [
        (0.0, 0.0),
        (10.0, 20.0),
        (-5.0, 3.0),
        (40.0, -12.0),
        (1.5, 1.5),
    ] {
        let mut p = PartDocument::default();
        let a = on(&mut p, OriginPlane::XY, |s| {
            s.add_circle(Point2::new(cx, cy), 4.0).unwrap();
        });
        let b = on(&mut p, OriginPlane::YZ, |_| {});
        let (_, c) = circle_of(&p, a);
        let id = p.sketches[a].id.clone();
        let k = p
            .project_sketch_geometry(b, &id, center_geo(c), false)
            .unwrap();
        let l = p.sketches[b].links[k].clone();
        assert_eq!(l.curve, None);
        let q = p.sketches[b].sketch.point(l.points[0]).unwrap();
        assert!(q.fixed);
        let want = p.sketches[b]
            .sketch
            .plane
            .project(&Point3::new(cx, cy, 0.0));
        close(q.position, want);
        close(q.position, Point2::new(cy, 0.0));
        // Projecting it again returns the same link.
        assert_eq!(
            p.project_sketch_geometry(b, &id, center_geo(c), false)
                .unwrap(),
            k
        );
        assert_eq!(p.sketches[b].links.len(), 1);
        // The References list shows it, healthy.
        let rows = p.sketches[b].references(&p.body, &p.work);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].ok);
        assert_eq!(rows[0].label.as_deref(), Some("Sketch1 · Point"));
        p.regenerate().unwrap();
        assert_eq!(p.sketches[b].health, None);
    }
}

/// From a sketch on an offset work plane: the centre at height 55 lands at
/// (y, 55) in the YZ sketch and follows the plane's offset.
#[test]
fn centre_from_offset_work_plane_follows_the_offset() {
    for (y, z) in [
        (30.0, 55.0),
        (0.0, 10.0),
        (-20.0, 5.0),
        (12.5, 100.0),
        (3.0, 0.5),
    ] {
        let mut p = PartDocument::default();
        let wp = plane_at(&mut p, z);
        let a = on_work(&mut p, &wp, |s| {
            s.add_circle(Point2::new(0.0, y), 5.0).unwrap();
        });
        let b = on(&mut p, OriginPlane::YZ, |_| {});
        let (_, c) = circle_of(&p, a);
        let id = p.sketches[a].id.clone();
        let k = p
            .project_sketch_geometry(b, &id, center_geo(c), false)
            .unwrap();
        let tp = p.sketches[b].links[k].points[0];
        let w = world(&p, a, c);
        close(
            p.sketches[b].sketch.point(tp).unwrap().position,
            p.sketches[b].sketch.plane.project(&w),
        );
        close(
            p.sketches[b].sketch.point(tp).unwrap().position,
            Point2::new(w.y, z),
        );
        // Move the work plane up 15: the projection follows.
        if let Feature::Work(wf) = &mut p.features[0]
            && let WorkDef::Plane(WorkPlaneDef::Offset { distance, .. }) = &mut wf.def
        {
            *distance = z + 15.0;
        }
        p.regenerate().unwrap();
        let q = p.sketches[b].sketch.point(tp).unwrap().position;
        assert!((q.y - (z + 15.0)).abs() < 1e-7, "{q:?}");
        assert_eq!(p.sketches[b].health, None);
    }
}

/// A line, an arc and a circle into a parallel sketch keep their shape; a
/// circle into a perpendicular sketch is refused and changes nothing.
#[test]
fn line_arc_and_circle() {
    let mut p = PartDocument::default();
    let (mut line, mut arc, mut circ) = (None, None, None);
    let a = on(&mut p, OriginPlane::XY, |s| {
        line = Some(s.add_line(Point2::new(0.0, 0.0), Point2::new(30.0, 10.0)));
        let c = s.add_point(Point2::new(50.0, 0.0));
        let st = s.add_point(Point2::new(60.0, 0.0));
        let en = s.add_point(Point2::new(50.0, 10.0));
        arc = Some(s.add_arc(c, st, en).unwrap());
        circ = Some(s.add_circle(Point2::new(-20.0, 5.0), 7.0).unwrap());
    });
    let wp = plane_at(&mut p, 20.0);
    let b = on_work(&mut p, &wp, |_| {});
    let id = p.sketches[a].id.clone();
    for e in [line, arc, circ] {
        let geo = SketchGeo::Curve { entity: e.unwrap() };
        p.project_sketch_geometry(b, &id, geo, false).unwrap();
    }
    let sk = &p.sketches[b].sketch;
    let ls = &p.sketches[b].links;
    assert_eq!(ls.len(), 3);
    let (s0, e0) = sk.line_geometry(ls[0].curve.unwrap()).unwrap();
    close(s0, Point2::new(0.0, 0.0));
    close(e0, Point2::new(30.0, 10.0));
    let (c1, r1) = sk.circle_geometry(ls[1].curve.unwrap()).unwrap();
    close(c1, Point2::new(50.0, 0.0));
    assert!((r1 - 10.0).abs() < 1e-9);
    let (c2, r2) = sk.circle_geometry(ls[2].curve.unwrap()).unwrap();
    close(c2, Point2::new(-20.0, 5.0));
    assert!((r2 - 7.0).abs() < 1e-9);
    assert!(sk.is_radius_fixed(ls[2].curve.unwrap()));
    // Projected geometry is fully fixed: no degrees of freedom.
    let mut sk2 = sk.clone();
    assert_eq!(sk2.solve().unwrap().dof, 0);
    for e in ls.iter().filter_map(|l| l.curve) {
        assert!(p.sketches[b].is_reference(e));
    }
    // Into a YZ sketch the circle would be an ellipse: refused.
    let c = on(&mut p, OriginPlane::YZ, |_| {});
    let before = p.sketches[c].sketch.points().count();
    let err = p
        .project_sketch_geometry(
            c,
            &id,
            SketchGeo::Curve {
                entity: circ.unwrap(),
            },
            false,
        )
        .unwrap_err();
    assert!(err.to_string().contains("not parallel"), "{err}");
    assert_eq!(p.sketches[c].sketch.points().count(), before);
    assert!(p.sketches[c].links.is_empty());
    // The line projects (a segment along Y).
    p.project_sketch_geometry(
        c,
        &id,
        SketchGeo::Curve {
            entity: line.unwrap(),
        },
        false,
    )
    .unwrap();
    let (s3, e3) = p.sketches[c]
        .sketch
        .line_geometry(p.sketches[c].links[0].curve.unwrap())
        .unwrap();
    close(s3, Point2::new(0.0, 0.0));
    close(e3, Point2::new(10.0, 0.0));
}

/// Editing the source moves the projection at the next regeneration:
/// centre, radius, line ends; user geometry on it follows.
#[test]
fn projection_follows_its_source() {
    let mut p = PartDocument::default();
    let a = on(&mut p, OriginPlane::XY, |s| {
        s.add_circle(Point2::new(10.0, 10.0), 5.0).unwrap();
    });
    let wp = plane_at(&mut p, 30.0);
    let b = on_work(&mut p, &wp, |_| {});
    let (ce, c) = circle_of(&p, a);
    let id = p.sketches[a].id.clone();
    let kc = p
        .project_sketch_geometry(b, &id, SketchGeo::Curve { entity: ce }, false)
        .unwrap();
    let target_center = p.sketches[b].links[kc].points[0];
    let target_circle = p.sketches[b].links[kc].curve.unwrap();
    // A user line from the projected centre to a free point.
    let sk = &mut p.sketches[b].sketch;
    let free = sk.add_point(Point2::new(40.0, 40.0));
    sk.add_line_between(target_center, free).unwrap();
    sk.solve().unwrap();
    // Move the source circle and change its radius.
    let sk = &mut p.sketches[a].sketch;
    sk.move_point(c, Point2::new(-15.0, 4.0)).unwrap();
    sk.set_circle_radius(ce, 9.0).unwrap();
    p.regenerate().unwrap();
    let sk = &p.sketches[b].sketch;
    close(
        sk.point(target_center).unwrap().position,
        Point2::new(-15.0, 4.0),
    );
    let (_, r) = sk.circle_geometry(target_circle).unwrap();
    assert!((r - 9.0).abs() < 1e-9);
    assert_eq!(p.sketches[b].health, None);
    assert_eq!(p.link_dependents(&id), vec![b]);
}

/// A deleted source leaves the projection frozen and the sketch sick.
#[test]
fn missing_source_is_health() {
    for delete_sketch in [false, true] {
        let mut p = PartDocument::default();
        let a = on(&mut p, OriginPlane::XY, |s| {
            s.add_circle(Point2::new(3.0, 4.0), 2.0).unwrap();
        });
        let b = on(&mut p, OriginPlane::YZ, |_| {});
        let (ce, c) = circle_of(&p, a);
        let id = p.sketches[a].id.clone();
        p.project_sketch_geometry(b, &id, center_geo(c), false)
            .unwrap();
        let tp = p.sketches[b].links[0].points[0];
        let was = p.sketches[b].sketch.point(tp).unwrap().position;
        if delete_sketch {
            p.delete_sketch(a).unwrap();
        } else {
            p.sketches[a].sketch.remove_entity(ce).unwrap();
            p.sketches[a].sketch.remove_point(c).unwrap();
        }
        p.regenerate().unwrap();
        let b = p.sketches.iter().position(|s| s.links.len() == 1).unwrap();
        let ps = &p.sketches[b];
        let h = ps.health.clone().expect("sick sketch");
        assert!(h.contains("Sketch1"), "{h}");
        assert!(ps.links[0].status.is_some());
        // Frozen where it was, still fixed.
        let q = ps.sketch.point(tp).unwrap();
        close(q.position, was);
        assert!(q.fixed);
        let rows = ps.references(&p.body, &p.work);
        assert!(!rows[0].ok);
    }
}

/// Break Link keeps the geometry (now free), drops the link.
#[test]
fn break_link_keeps_geometry() {
    let mut p = PartDocument::default();
    let a = on(&mut p, OriginPlane::XY, |s| {
        s.add_circle(Point2::new(8.0, 2.0), 3.0).unwrap();
    });
    let wp = plane_at(&mut p, 10.0);
    let b = on_work(&mut p, &wp, |_| {});
    let (ce, _) = circle_of(&p, a);
    let id = p.sketches[a].id.clone();
    p.project_sketch_geometry(b, &id, SketchGeo::Curve { entity: ce }, false)
        .unwrap();
    let l = p.sketches[b].links[0].clone();
    let e = l.curve.unwrap();
    p.sketches[b].break_link(RefTarget::Curve(e)).unwrap();
    let ps = &p.sketches[b];
    assert!(ps.links.is_empty());
    assert!(!ps.is_reference(e));
    assert!(!ps.sketch.point(l.points[0]).unwrap().fixed);
    assert!(!ps.sketch.is_radius_fixed(e));
    let (c, r) = ps.sketch.circle_geometry(e).unwrap();
    close(c, Point2::new(8.0, 2.0));
    assert!((r - 3.0).abs() < 1e-9);
    // No longer follows the source.
    p.sketches[a].sketch.set_circle_radius(ce, 6.0).unwrap();
    p.regenerate().unwrap();
    let (_, r) = p.sketches[b].sketch.circle_geometry(e).unwrap();
    assert!((r - 3.0).abs() < 1e-9);
}

/// Delete removes the projection and the constraints that used it; a user
/// curve built on the projected point keeps the point (free).
#[test]
fn delete_projection_removes_its_constraints() {
    let mut p = PartDocument::default();
    let a = on(&mut p, OriginPlane::XY, |s| {
        s.add_circle(Point2::new(0.0, 10.0), 3.0).unwrap();
    });
    let b = on(&mut p, OriginPlane::YZ, |_| {});
    let (_, c) = circle_of(&p, a);
    let id = p.sketches[a].id.clone();
    p.project_sketch_geometry(b, &id, center_geo(c), false)
        .unwrap();
    let tp = p.sketches[b].links[0].points[0];
    let sk = &mut p.sketches[b].sketch;
    let q = sk.add_point(Point2::new(30.0, 20.0));
    let line = sk.add_line_between(tp, q).unwrap();
    let other = sk.add_point(Point2::new(0.0, 40.0));
    let dim = sk
        .add_constraint(Constraint::Distance {
            a: tp,
            b: other,
            value: 20.0,
        })
        .unwrap();
    sk.solve().unwrap();
    p.sketches[b]
        .delete_reference(RefTarget::Point(tp))
        .unwrap();
    let ps = &p.sketches[b];
    assert!(ps.links.is_empty());
    assert!(ps.sketch.constraint(dim).is_none(), "its dimension went");
    assert!(ps.sketch.entity(line).is_some(), "the user line stays");
    assert!(!ps.sketch.point(tp).unwrap().fixed);
    // A projected point nothing else uses goes completely.
    let b2 = on(&mut p, OriginPlane::YZ, |_| {});
    p.project_sketch_geometry(b2, &id, center_geo(c), false)
        .unwrap();
    let tp2 = p.sketches[b2].links[0].points[0];
    p.sketches[b2]
        .delete_reference(RefTarget::Point(tp2))
        .unwrap();
    assert!(p.sketches[b2].sketch.point(tp2).is_none());
}

/// Save/load keeps the links (ids, not indices) and the held radius; a
/// file without links still reads.
#[test]
fn round_trip_and_old_files() {
    let mut p = PartDocument::default();
    let a = on(&mut p, OriginPlane::XY, |s| {
        s.add_circle(Point2::new(5.0, 5.0), 2.5).unwrap();
        s.add_line(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
    });
    let wp = plane_at(&mut p, 12.0);
    let b = on_work(&mut p, &wp, |_| {});
    let id = p.sketches[a].id.clone();
    let ents: Vec<EntityId> = p.sketches[a].sketch.entities().map(|(e, _)| e).collect();
    for e in ents {
        p.project_sketch_geometry(b, &id, SketchGeo::Curve { entity: e }, false)
            .unwrap();
    }
    let json = serde_json::to_string(&p).unwrap();
    let mut q: PartDocument = serde_json::from_str(&json).unwrap();
    q.regenerate().unwrap();
    assert_eq!(q.sketches[b].links, p.sketches[b].links);
    assert_eq!(q.sketches[b].health, None);
    let circle = q.sketches[b]
        .links
        .iter()
        .find_map(|l| {
            l.curve
                .filter(|&c| q.sketches[b].sketch.circle_geometry(c).is_some())
        })
        .unwrap();
    assert!(q.sketches[b].sketch.is_radius_fixed(circle));
    // Deterministic save.
    assert_eq!(serde_json::to_string(&q).unwrap(), json);
    // No links: the field is left out and an old file reads.
    let plain = serde_json::to_string(&PartSketch::new("S", Sketch::default())).unwrap();
    assert!(!plain.contains("links") && !plain.contains("fixed_radius"));
    let old: PartSketch = serde_json::from_str(&plain).unwrap();
    assert!(old.links.is_empty());
}

/// A sketch projects only from sketches before it in the history.
#[test]
fn only_from_earlier_sketches() {
    let mut p = PartDocument::default();
    let a = on(&mut p, OriginPlane::XY, |_| {});
    let b = on(&mut p, OriginPlane::YZ, |s| {
        s.add_circle(Point2::new(1.0, 1.0), 1.0).unwrap();
    });
    let (_, c) = circle_of(&p, b);
    let id = p.sketches[b].id.clone();
    let err = p
        .project_sketch_geometry(a, &id, center_geo(c), false)
        .unwrap_err();
    assert!(err.to_string().contains("after this sketch"), "{err}");
    assert!(!p.can_project_from(a, b) && p.can_project_from(b, a));
    let own = p.sketches[a].id.clone();
    assert!(
        p.project_sketch_geometry(a, &own, center_geo(c), false)
            .is_err()
    );
}

/// Splines: a CV spline projects exactly (affine invariance); an
/// interpolation spline keeps its fit points and handles in a parallel
/// sketch.
#[test]
fn splines_project() {
    let mut p = PartDocument::default();
    let (mut cv, mut ip) = (None, None);
    let a = on(&mut p, OriginPlane::XY, |s| {
        let pts: Vec<_> = [(0.0, 0.0), (10.0, 15.0), (25.0, -5.0), (40.0, 10.0)]
            .iter()
            .map(|&(x, y)| s.add_point(Point2::new(x, y)))
            .collect();
        cv = Some(s.add_spline(pts).unwrap());
        let pts: Vec<_> = [(0.0, 30.0), (10.0, 40.0), (20.0, 35.0)]
            .iter()
            .map(|&(x, y)| s.add_point(Point2::new(x, y)))
            .collect();
        let e = s.add_interp_spline(pts).unwrap();
        s.set_spline_handle(e, false, Some(crate::sketch::Vector2::new(5.0, 0.0)))
            .unwrap();
        ip = Some(e);
    });
    let wp = plane_at(&mut p, 7.0);
    let b = on_work(&mut p, &wp, |_| {});
    let id = p.sketches[a].id.clone();
    for e in [cv, ip] {
        p.project_sketch_geometry(b, &id, SketchGeo::Curve { entity: e.unwrap() }, false)
            .unwrap();
    }
    let src = &p.sketches[a].sketch;
    let dst = &p.sketches[b].sketch;
    for (k, e) in [cv, ip].iter().enumerate() {
        let s1 = src.spline_curve(e.unwrap()).unwrap();
        let t = p.sketches[b].links[k].curve.unwrap();
        let s2 = dst.spline_curve(t).unwrap();
        for i in 0..=10 {
            let u = i as Real / 10.0;
            let (x, y) = (s1.point_at(u), s2.point_at(u));
            assert!((x - y).norm() < 1e-7, "{k}: {x:?} {y:?}");
        }
    }
    assert!(dst.is_interp_spline(p.sketches[b].links[1].curve.unwrap()));
}

/// Replace Reference: the link takes another circle centre; the
/// constraints on the projected point stay.
#[test]
fn replace_reference_keeps_constraints() {
    let mut p = PartDocument::default();
    let a = on(&mut p, OriginPlane::XY, |s| {
        s.add_circle(Point2::new(0.0, 5.0), 1.0).unwrap();
        s.add_circle(Point2::new(0.0, 25.0), 1.0).unwrap();
    });
    let b = on(&mut p, OriginPlane::YZ, |_| {});
    let centers: Vec<PointId> = p.sketches[a]
        .sketch
        .entities()
        .filter_map(|(_, e)| match e {
            Entity::Circle { center, .. } => Some(*center),
            _ => None,
        })
        .collect();
    let id = p.sketches[a].id.clone();
    p.project_sketch_geometry(b, &id, center_geo(centers[0]), false)
        .unwrap();
    let tp = p.sketches[b].links[0].points[0];
    let sk = &mut p.sketches[b].sketch;
    let q = sk.add_point(Point2::new(0.0, 30.0));
    let dim = sk
        .add_constraint(Constraint::Distance {
            a: tp,
            b: q,
            value: 30.0,
        })
        .unwrap();
    sk.solve().unwrap();
    let new = LinkSource::Sketch {
        sketch: id.clone(),
        geo: center_geo(centers[1]),
    };
    p.relink(b, 0, new.clone()).unwrap();
    let ps = &p.sketches[b];
    assert_eq!(ps.links[0].source, new);
    assert!(ps.sketch.constraint(dim).is_some());
    let at = ps.sketch.point(tp).unwrap().position;
    let want = ps.sketch.plane.project(&world(&p, a, centers[1]));
    close(at, want);
    // A curve for a point: refused, nothing changes.
    let ce = circle_of(&p, a).0;
    let wrong = LinkSource::Sketch {
        sketch: id,
        geo: SketchGeo::Curve { entity: ce },
    };
    assert!(p.relink(b, 0, wrong).is_err());
    assert_eq!(p.sketches[b].links[0].source, new);
}

/// A circular model edge (top of a cylinder) into a parallel sketch: a
/// reference circle that follows the extrusion height.
#[test]
fn curved_model_edge() {
    let mut p = PartDocument::default();
    let a = on(&mut p, OriginPlane::XY, |s| {
        s.add_circle(Point2::new(5.0, 0.0), 6.0).unwrap();
    });
    let r = p.sketches[a].sketch.regions();
    let pr = ProfileRef::of(&r[0], r[0].interior_point());
    p.add_extrude(a, ExtrudeFeature::new(vec![pr], 10.0))
        .unwrap();
    let wp = plane_at(&mut p, 30.0);
    let b = on_work(&mut p, &wp, |_| {});
    let top = p
        .body
        .edges()
        .find(|(e, x)| {
            matches!(p.body.curve(x.curve), Some(crate::kernel::Curve::Circle(c)) if c.center.z > 5.0)
                && blend::edge_ref(&p.body, *e).is_some()
        })
        .map(|(e, _)| e)
        .expect("top circular edge");
    p.project_curved_edge(b, top).unwrap();
    let l = p.sketches[b].links[0].clone();
    let (c, rad) = p.sketches[b]
        .sketch
        .circle_geometry(l.curve.unwrap())
        .unwrap();
    close(c, Point2::new(5.0, 0.0));
    assert!((rad - 6.0).abs() < 1e-6, "{rad}");
    p.regenerate().unwrap();
    assert_eq!(p.sketches[b].health, None, "{:?}", p.sketches[b].links);
    // A straight edge still goes through project_edge.
    assert!(p.project_edge(b, top).is_err());
}

/// The owner's scenario: two circles in two sketches (XY and 55 mm above),
/// a spline in a third sketch (YZ) from projected centre to projected
/// centre, and a Sweep of the first circle along it builds.
#[test]
fn owner_scenario_sweep_along_spline_between_projected_centres() {
    let mut p = PartDocument::default();
    let a = on(&mut p, OriginPlane::XY, |s| {
        s.add_circle(Point2::new(0.0, 0.0), 4.0).unwrap();
    });
    let wp = plane_at(&mut p, 55.0);
    let b = on_work(&mut p, &wp, |s| {
        s.add_circle(Point2::new(0.0, 30.0), 4.0).unwrap();
    });
    let c = on(&mut p, OriginPlane::YZ, |_| {});
    let (_, ca) = circle_of(&p, a);
    let (_, cb) = circle_of(&p, b);
    let (ida, idb) = (p.sketches[a].id.clone(), p.sketches[b].id.clone());
    let ka = p
        .project_sketch_geometry(c, &ida, center_geo(ca), false)
        .unwrap();
    let kb = p
        .project_sketch_geometry(c, &idb, center_geo(cb), false)
        .unwrap();
    let (pa, pb) = (
        p.sketches[c].links[ka].points[0],
        p.sketches[c].links[kb].points[0],
    );
    close(
        p.sketches[c].sketch.point(pa).unwrap().position,
        Point2::new(0.0, 0.0),
    );
    close(
        p.sketches[c].sketch.point(pb).unwrap().position,
        Point2::new(30.0, 55.0),
    );
    // The spline snaps to the projected points (shares them: coincident).
    let sk = &mut p.sketches[c].sketch;
    let mid = sk.add_point(Point2::new(4.0, 30.0));
    let spline = sk.add_interp_spline(vec![pa, mid, pb]).unwrap();
    sk.add_constraint(Constraint::Fix {
        point: mid,
        at: Point2::new(4.0, 30.0),
    })
    .unwrap();
    assert_eq!(sk.solve().unwrap().dof, 0);
    let regions = p.sketches[a].sketch.regions();
    let profiles = vec![ProfileRef::of(&regions[0], regions[0].interior_point())];
    let path = PathRef::Sketch {
        sketch: p.sketches[c].id.clone(),
        curves: vec![spline],
    };
    let id = p.add_sweep(a, SweepFeature::new(profiles, path)).unwrap();
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    assert!(p.exact);
    let v: Real = p
        .exact_bodies
        .0
        .iter()
        .map(|s| s.measure_full().unwrap().volume)
        .sum();
    // Volume = circle area × path length (the path leaves the profile
    // almost square to it).
    let curve = p.sketches[c].sketch.spline_curve(spline).unwrap();
    let d = curve.domain();
    let len: Real = (0..400)
        .map(|i| {
            let t = |k: usize| d.min + (d.max - d.min) * k as Real / 400.0;
            (curve.point_at(t(i + 1)) - curve.point_at(t(i))).norm()
        })
        .sum();
    let want = std::f64::consts::PI * 16.0 * len;
    assert!((v - want).abs() < 0.03 * want, "{id}: volume {v} vs {want}");
    for (sid, _) in p.body.solids() {
        p.body.validate_solid(sid).unwrap();
    }
    // Moving the second circle moves the path end: the sweep follows.
    let cb_pos = Point2::new(0.0, 20.0);
    p.sketches[b].sketch.move_point(cb, cb_pos).unwrap();
    p.regenerate().unwrap();
    close(
        p.sketches[c].sketch.point(pb).unwrap().position,
        Point2::new(20.0, 55.0),
    );
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
}
