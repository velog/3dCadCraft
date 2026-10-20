//! Guided lofts: Rails, Center Line, Area Loft, conditions, map points,
//! Closed Loop (`docs/spec/5.2-loft.md`), against analytic volumes.

use std::f64::consts::{FRAC_PI_2, PI};

use super::*;
use crate::kernel::Vector3;
use crate::part::LineRef;
use crate::part::placement::{OriginPlane, PlaneRef, SketchPlacement};
use crate::part::{Feature, PathRef, WorkDef, WorkFeature, WorkPlaneDef};
use crate::sketch::{EntityId, Point2, Sketch};

fn rel(v: Real, want: Real, tol: Real) {
    assert!(
        (v - want).abs() <= tol * want.abs(),
        "{v} vs {want} ({:.3} %)",
        100.0 * (v - want) / want
    );
}

/// A sketch on `plane` (an origin plane or a work plane id).
fn sketch_on(p: &mut PartDocument, plane: PlaneRef, draw: impl FnOnce(&mut Sketch)) -> usize {
    let k = p
        .create_sketch(
            SketchPlacement {
                plane,
                ..Default::default()
            },
            &opts(),
        )
        .unwrap();
    draw(&mut p.sketches[k].sketch);
    k
}

/// Sketch coordinates of a model point on sketch `k`'s plane.
fn uv(p: &PartDocument, k: usize, q: Point3) -> Point2 {
    let c = p.sketches[k].sketch.plane.project(&q);
    Point2::new(c.x, c.y)
}

/// A rectangle `w × d` centred on the Z axis at height `z`.
fn rect_at(p: &mut PartDocument, z: Real, w: Real, d: Real) -> usize {
    at_height(p, z, |s| {
        s.add_rectangle_two_point(
            Point2::new(-w / 2.0, -d / 2.0),
            Point2::new(w / 2.0, d / 2.0),
        )
        .unwrap();
    })
}

fn circle_at(p: &mut PartDocument, z: Real, r: Real) -> usize {
    at_height(p, z, |s| {
        s.add_circle(Point2::new(0.0, 0.0), r).unwrap();
    })
}

/// A line on the XZ plane between two model points (y = 0); its path.
fn xz_line(p: &mut PartDocument, a: Point3, b: Point3) -> PathRef {
    let k = sketch_on(p, PlaneRef::Origin(OriginPlane::XZ), |_| {});
    let (ua, ub) = (uv(p, k, a), uv(p, k, b));
    let e = p.sketches[k].sketch.add_line(ua, ub);
    PathRef::Sketch {
        sketch: p.sketches[k].id.clone(),
        curves: vec![e],
    }
}

/// An arc on sketch `k` through three model points (start, middle, end).
fn arc_through(p: &mut PartDocument, k: usize, a: Point3, m: Point3, b: Point3) -> EntityId {
    let (a, m, b) = (uv(p, k, a), uv(p, k, m), uv(p, k, b));
    // Circumcentre.
    let d = 2.0 * (a.x * (m.y - b.y) + m.x * (b.y - a.y) + b.x * (a.y - m.y));
    let sq = |q: Point2| q.x * q.x + q.y * q.y;
    let c = Point2::new(
        (sq(a) * (m.y - b.y) + sq(m) * (b.y - a.y) + sq(b) * (a.y - m.y)) / d,
        (sq(a) * (b.x - m.x) + sq(m) * (a.x - b.x) + sq(b) * (m.x - a.x)) / d,
    );
    // Counter-clockwise from start to end through the middle?
    let ang = |q: Point2| (q.y - c.y).atan2(q.x - c.x);
    let ccw = |x: Real, y: Real| (y - x).rem_euclid(2.0 * PI);
    let (sa, sm, sb) = (ang(a), ang(m), ang(b));
    let through = ccw(sa, sm) < ccw(sa, sb);
    let s = &mut p.sketches[k].sketch;
    let pc = s.add_point(c);
    let (pa, pb) = (s.add_point(a), s.add_point(b));
    if through {
        s.add_arc(pc, pa, pb).unwrap()
    } else {
        s.add_arc(pc, pb, pa).unwrap()
    }
}

fn path(p: &PartDocument, k: usize, curves: Vec<EntityId>) -> PathRef {
    PathRef::Sketch {
        sketch: p.sketches[k].id.clone(),
        curves,
    }
}

fn rails_loft(
    p: &mut PartDocument,
    ks: &[usize],
    rails: Vec<PathRef>,
    f: impl FnOnce(&mut LoftFeature),
) -> Result<String, String> {
    loft_of(p, ks, |d| {
        d.rails = rails;
        f(d)
    })
}

// --------------------------------------------------------------- Rails ---

#[test]
fn two_rectangles_with_straight_rails() {
    // (w, d, h, growth): the end rectangle is the start one scaled by g; the
    // rails run along the short sides' midpoints. Area at t: w·d·(1 + (g−1)t)².
    for (w, d, h, g) in [
        (40.0, 20.0, 50.0, 1.5),
        (30.0, 10.0, 20.0, 0.5),
        (10.0, 10.0, 40.0, 2.0),
        (60.0, 25.0, 15.0, 1.0),
        (16.0, 8.0, 30.0, 1.25),
    ] {
        let mut p = PartDocument::default();
        let a = rect_at(&mut p, 0.0, w, d);
        let b = rect_at(&mut p, h, w * g, d * g);
        let r1 = xz_line(
            &mut p,
            Point3::new(-w / 2.0, 0.0, 0.0),
            Point3::new(-w * g / 2.0, 0.0, h),
        );
        let r2 = xz_line(
            &mut p,
            Point3::new(w / 2.0, 0.0, 0.0),
            Point3::new(w * g / 2.0, 0.0, h),
        );
        rails_loft(&mut p, &[a, b], vec![r1, r2], |_| {}).unwrap();
        healthy(&p);
        let want = w * d * h * (1.0 + (g - 1.0) + (g - 1.0) * (g - 1.0) / 3.0);
        rel(volume(&p), want, 2e-3);
        let names: Vec<String> = p.body.faces().filter_map(|(_, f)| f.name.clone()).collect();
        assert!(names.iter().any(|n| n == "Loft1:Start"), "{names:?}");
        assert!(
            names.iter().any(|n| n.starts_with("Loft1:Side:")),
            "{names:?}"
        );
    }
}

/// π ∫ x(z)² dz for a rail through (r, 0), (r + b, h/2), (r, h) (an arc).
fn bottle_volume(r: Real, b: Real, h: Real) -> Real {
    let big = (b * b + h * h / 4.0) / (2.0 * b);
    let x = |z: Real| r + b - big + big.signum() * (big * big - (z - h / 2.0).powi(2)).sqrt();
    let n = 2000;
    let dz = h / n as Real;
    let mut s = 0.0;
    for i in 0..=n {
        let w = if i == 0 || i == n {
            1.0
        } else if i % 2 == 1 {
            4.0
        } else {
            2.0
        };
        s += w * x(i as Real * dz).powi(2);
    }
    PI * s * dz / 3.0
}

/// Circles r at z = 0 and z = h, two arc rails bulging by `b` at mid
/// height on both sides (XZ plane).
fn bottle(r: Real, b: Real, h: Real) -> (PartDocument, usize, usize, Vec<PathRef>) {
    let mut p = PartDocument::default();
    let s0 = circle_at(&mut p, 0.0, r);
    let s1 = circle_at(&mut p, h, r);
    let k = sketch_on(&mut p, PlaneRef::Origin(OriginPlane::XZ), |_| {});
    let mut rails = Vec::new();
    for side in [-1.0, 1.0] {
        let e = arc_through(
            &mut p,
            k,
            Point3::new(side * r, 0.0, 0.0),
            Point3::new(side * (r + b), 0.0, h / 2.0),
            Point3::new(side * r, 0.0, h),
        );
        rails.push(path(&p, k, vec![e]));
    }
    (p, s0, s1, rails)
}

#[test]
fn bottle_with_curved_rails() {
    for (r, b, h) in [
        (10.0, 10.0, 100.0),
        (10.0, 4.0, 60.0),
        (5.0, 3.0, 30.0),
        (20.0, -5.0, 80.0),
        (8.0, 12.0, 50.0),
    ] {
        let (mut p, s0, s1, rails) = bottle(r, b, h);
        rails_loft(&mut p, &[s0, s1], rails, |_| {}).unwrap();
        healthy(&p);
        rel(volume(&p), bottle_volume(r, b, h), 5e-3);
        // Without the rails: a cylinder.
        let mut q = bottle(r, b, h).0;
        loft_of(&mut q, &[0, 1], |_| {}).unwrap();
        rel(volume(&q), PI * r * r * h, 1e-3);
    }
}

#[test]
fn one_and_three_rails() {
    // One rail: a square slides along a slanted line (a sheared prism, the
    // volume of a prism).
    for (w, h, dx) in [(20.0, 30.0, 10.0), (10.0, 50.0, -15.0), (6.0, 10.0, 3.0)] {
        let mut p = PartDocument::default();
        let a = rect_at(&mut p, 0.0, w, w);
        let b = at_height(&mut p, h, |s| {
            s.add_rectangle_two_point(
                Point2::new(dx - w / 2.0, -w / 2.0),
                Point2::new(dx + w / 2.0, w / 2.0),
            )
            .unwrap();
        });
        let r = xz_line(
            &mut p,
            Point3::new(w / 2.0, 0.0, 0.0),
            Point3::new(dx + w / 2.0, 0.0, h),
        );
        rails_loft(&mut p, &[a, b], vec![r], |_| {}).unwrap();
        healthy(&p);
        rel(volume(&p), w * w * h, 2e-3);
    }
    // Three rails on a rectangle growing in x only: rails at the middle of
    // the left and right sides and at the middle of the back side.
    for (w, d, h, g) in [(20.0, 10.0, 30.0, 2.0), (30.0, 20.0, 40.0, 0.5)] {
        let mut p = PartDocument::default();
        let a = rect_at(&mut p, 0.0, w, d);
        let b = rect_at(&mut p, h, w * g, d);
        let r1 = xz_line(
            &mut p,
            Point3::new(-w / 2.0, 0.0, 0.0),
            Point3::new(-w * g / 2.0, 0.0, h),
        );
        let r2 = xz_line(
            &mut p,
            Point3::new(w / 2.0, 0.0, 0.0),
            Point3::new(w * g / 2.0, 0.0, h),
        );
        let k = sketch_on(&mut p, PlaneRef::Origin(OriginPlane::YZ), |_| {});
        let (ua, ub) = (
            uv(&p, k, Point3::new(0.0, d / 2.0, 0.0)),
            uv(&p, k, Point3::new(0.0, d / 2.0, h)),
        );
        let e = p.sketches[k].sketch.add_line(ua, ub);
        let r3 = path(&p, k, vec![e]);
        rails_loft(&mut p, &[a, b], vec![r1, r2, r3], |_| {}).unwrap();
        healthy(&p);
        // Width linear, depth constant.
        rel(volume(&p), w * d * h * (1.0 + g) / 2.0, 3e-3);
    }
}

#[test]
fn rail_refusals_leave_the_part_unchanged() {
    let (mut p, s0, s1, rails) = bottle(10.0, 5.0, 40.0);
    // A rail that misses the top section.
    let miss = xz_line(
        &mut p,
        Point3::new(10.0, 0.0, 0.0),
        Point3::new(25.0, 0.0, 40.0),
    );
    let before = p.features.len();
    let e = rails_loft(&mut p, &[s0, s1], vec![miss], |_| {}).unwrap_err();
    assert!(e.contains("rail 1 does not touch section 2"), "{e}");
    // A rail that stops short.
    let short = xz_line(
        &mut p,
        Point3::new(10.0, 0.0, 0.0),
        Point3::new(10.0, 0.0, 20.0),
    );
    let e = rails_loft(&mut p, &[s0, s1], vec![short], |_| {}).unwrap_err();
    assert!(e.contains("does not reach section 2"), "{e}");
    // Conditions together with rails.
    let e = rails_loft(&mut p, &[s0, s1], rails.clone(), |d| {
        d.conditions = vec![LoftCondition {
            kind: ConditionKind::Direction,
            ..Default::default()
        }]
    })
    .unwrap_err();
    assert!(e.contains("conditions together with rails"), "{e}");
    assert_eq!(p.features.len(), before);
    // The good one builds; then Edit Feature drops a rail, then the other.
    rails_loft(&mut p, &[s0, s1], rails.clone(), |_| {}).unwrap();
    let k = p.last_inserted();
    let v2 = volume(&p);
    rel(v2, bottle_volume(10.0, 5.0, 40.0), 5e-3);
    let crate::part::Feature::Loft(mut def) = p.features[k].clone() else {
        panic!("not a loft");
    };
    def.rails.truncate(1);
    p.replace_feature(k, crate::part::Feature::Loft(def.clone()))
        .unwrap();
    healthy(&p);
    // One rail moves the circle sideways: a sheared cylinder.
    rel(volume(&p), PI * 100.0 * 40.0, 2e-3);
    def.rails.clear();
    p.replace_feature(k, crate::part::Feature::Loft(def))
        .unwrap();
    rel(volume(&p), PI * 100.0 * 40.0, 1e-3);
    // Saved and loaded with its rails.
    let crate::part::Feature::Loft(mut def) = p.features[k].clone() else {
        panic!("not a loft");
    };
    def.rails = rails;
    p.replace_feature(k, crate::part::Feature::Loft(def))
        .unwrap();
    let json = serde_json::to_string(&p).unwrap();
    let mut q: PartDocument = serde_json::from_str(&json).unwrap();
    q.regenerate().unwrap();
    rel(volume(&q), volume(&p), 1e-9);
}

// ---------------------------------------------------------- Center Line ---

/// A plane through the Z axis holding the direction at `theta` from X.
fn plane_through_z(p: &mut PartDocument, theta: Real) -> PlaneRef {
    let want = Vector3::new(theta.cos(), theta.sin(), 0.0);
    if want.y.abs() < 1e-12 {
        return PlaneRef::Origin(OriginPlane::XZ);
    }
    for angle in [theta, -theta] {
        let wp = p
            .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Angle {
                base: PlaneRef::Origin(OriginPlane::XZ),
                axis: LineRef::Origin(2),
                angle,
            })))
            .unwrap();
        let n = p.work.planes.get(&wp).map(|pl| pl.normal.into_inner());
        if n.is_some_and(|n| n.dot(&want).abs() < 1e-9) {
            return PlaneRef::Work(wp);
        }
    }
    panic!("no plane through Z at {theta}");
}

/// Circles `r` at radius `big` on planes through the Z axis at 0 and `angle`
/// (radians), and the XY arc between their centres.
fn arc_center_line(big: Real, r: Real, angle: Real) -> (PartDocument, usize, usize, PathRef) {
    let mut p = PartDocument::default();
    let s0 = sketch_on(&mut p, PlaneRef::Origin(OriginPlane::XZ), |_| {});
    let c0 = uv(&p, s0, Point3::new(big, 0.0, 0.0));
    p.sketches[s0].sketch.add_circle(c0, r).unwrap();
    let plane = plane_through_z(&mut p, angle);
    let s1 = sketch_on(&mut p, plane, |_| {});
    let dir = Vector3::new(angle.cos(), angle.sin(), 0.0);
    let end = Point3::from(dir * big);
    let c1 = uv(&p, s1, end);
    p.sketches[s1].sketch.add_circle(c1, r).unwrap();
    let k = sketch_on(&mut p, PlaneRef::Origin(OriginPlane::XY), |_| {});
    let mid_dir = (Vector3::x() + dir).normalize();
    let e = arc_through(
        &mut p,
        k,
        Point3::new(big, 0.0, 0.0),
        Point3::from(mid_dir * big),
        end,
    );
    let c = path(&p, k, vec![e]);
    (p, s0, s1, c)
}

#[test]
fn center_line_along_an_arc_is_pappus() {
    for (big, r, deg) in [
        (50.0, 5.0, 90.0),
        (30.0, 8.0, 60.0),
        (80.0, 10.0, 45.0),
        (20.0, 3.0, 120.0),
        (40.0, 12.0, 30.0),
    ] {
        let angle = Real::to_radians(deg);
        let (mut p, s0, s1, c) = arc_center_line(big, r, angle);
        loft_of(&mut p, &[s0, s1], |d| {
            d.guide = LoftGuide::CenterLine;
            d.center_line = Some(c);
        })
        .unwrap();
        healthy(&p);
        rel(volume(&p), PI * r * r * big * angle, 3e-3);
        // A plain loft between the same circles is a straight-sided
        // (smaller) body.
        let (mut q, s0, s1, _) = arc_center_line(big, r, angle);
        loft_of(&mut q, &[s0, s1], |_| {}).unwrap();
        assert!(volume(&q) < volume(&p));
    }
}

#[test]
fn center_line_needs_a_curve() {
    let (mut p, s0, s1, _) = arc_center_line(50.0, 5.0, FRAC_PI_2);
    let e = loft_of(&mut p, &[s0, s1], |d| d.guide = LoftGuide::CenterLine).unwrap_err();
    assert!(e.contains("select the centre line"), "{e}");
    assert!(p.features.iter().all(|f| !matches!(f, Feature::Loft(_))));
}

#[test]
fn a_folding_loft_is_refused() {
    // Circles larger than the bend of the centre line: the stations would
    // cross each other on the inner side — a self-intersecting solid.
    for (big, r, deg) in [(8.0, 10.0, 90.0), (5.0, 6.0, 60.0), (12.0, 15.0, 120.0)] {
        let (mut p, s0, s1, c) = arc_center_line(big, r, Real::to_radians(deg));
        let e = loft_of(&mut p, &[s0, s1], |d| {
            d.guide = LoftGuide::CenterLine;
            d.center_line = Some(c);
        })
        .unwrap_err();
        assert!(e.contains("would cross itself"), "{e}");
        assert!(p.features.iter().all(|f| !matches!(f, Feature::Loft(_))));
    }
}

// ------------------------------------------------------------ Area Loft ---

/// Squares `a` at z = 0 and `b` at z = h, the Z axis as the centre line.
fn area_part(a: Real, b: Real, h: Real) -> (PartDocument, usize, usize, PathRef) {
    let mut p = PartDocument::default();
    let s0 = rect_at(&mut p, 0.0, a, a);
    let s1 = rect_at(&mut p, h, b, b);
    let c = xz_line(&mut p, Point3::origin(), Point3::new(0.0, 0.0, h));
    (p, s0, s1, c)
}

#[test]
fn area_loft_follows_a_linear_area_law() {
    for (a, b, h) in [
        (20.0, 40.0, 100.0),
        (10.0, 30.0, 40.0),
        (30.0, 10.0, 60.0),
        (12.0, 16.0, 20.0),
        (8.0, 24.0, 70.0),
    ] {
        let (mut p, s0, s1, c) = area_part(a, b, h);
        loft_of(&mut p, &[s0, s1], |d| {
            d.guide = LoftGuide::AreaLoft;
            d.center_line = Some(c.clone());
        })
        .unwrap();
        healthy(&p);
        // Area linear from a² to b².
        rel(volume(&p), h * (a * a + b * b) / 2.0, 5e-3);
        // A Center Line loft keeps the side linear instead (a frustum).
        let (mut q, s0, s1, c) = area_part(a, b, h);
        loft_of(&mut q, &[s0, s1], |d| {
            d.guide = LoftGuide::CenterLine;
            d.center_line = Some(c);
        })
        .unwrap();
        rel(volume(&q), h / 3.0 * (a * a + a * b + b * b), 3e-3);
    }
}

#[test]
fn area_loft_placed_sections() {
    // Equal squares, a placed section in the middle with twice the area
    // (or a scale factor): the area runs 400 → 800 → 400 linearly.
    for (pos, proportional, size, scale, want) in [
        (0.5, true, 800.0, false, 100.0 * 600.0),
        (50.0, false, 800.0, false, 100.0 * 600.0),
        (0.25, true, 1200.0, false, 25.0 * 800.0 + 75.0 * 800.0),
        (0.5, true, 2f64.sqrt(), true, 100.0 * 600.0),
        (0.5, true, 200.0, false, 100.0 * 300.0),
    ] {
        let (mut p, s0, s1, c) = area_part(20.0, 20.0, 100.0);
        loft_of(&mut p, &[s0, s1], |d| {
            d.guide = LoftGuide::AreaLoft;
            d.center_line = Some(c);
            d.placed = vec![PlacedSection {
                position: pos,
                proportional,
                size,
                scale,
                driving: true,
            }];
        })
        .unwrap();
        healthy(&p);
        rel(volume(&p), want, 2e-2);
    }
    // A placed section off the loft is refused, the part unchanged.
    let (mut p, s0, s1, c) = area_part(20.0, 20.0, 100.0);
    let e = loft_of(&mut p, &[s0, s1], |d| {
        d.guide = LoftGuide::AreaLoft;
        d.center_line = Some(c);
        d.placed = vec![PlacedSection {
            position: 1.5,
            proportional: true,
            size: 100.0,
            scale: false,
            driving: true,
        }];
    })
    .unwrap_err();
    assert!(e.contains("outside the loft"), "{e}");
    assert!(p.features.iter().all(|f| !matches!(f, Feature::Loft(_))));
}

// ---------------------------------------------------------- Conditions ---

fn direction(angle_deg: Real, weight: Real) -> LoftCondition {
    LoftCondition {
        kind: ConditionKind::Direction,
        angle: angle_deg.to_radians(),
        weight,
    }
}

#[test]
fn direction_condition_leans_the_start() {
    for (w, h) in [
        (20.0, 50.0),
        (10.0, 30.0),
        (40.0, 40.0),
        (16.0, 20.0),
        (30.0, 60.0),
    ] {
        let vol = |c: Option<LoftCondition>| {
            let mut p = PartDocument::default();
            let a = at_height(&mut p, 0.0, |s| square(s, w));
            let b = at_height(&mut p, h, |s| square(s, w));
            loft_of(&mut p, &[a, b], |d| d.conditions = c.into_iter().collect()).unwrap();
            healthy(&p);
            volume(&p)
        };
        let prism = w * w * h;
        rel(vol(None), prism, 1e-3);
        // Square to the section: still the prism.
        rel(vol(Some(direction(90.0, 1.0))), prism, 2e-3);
        // Below 90° the loft leans inwards, above it outwards.
        let inward = vol(Some(direction(60.0, 0.5)));
        let outward = vol(Some(direction(120.0, 0.5)));
        assert!(inward < prism * 0.995, "{inward} vs {prism}");
        assert!(outward > prism * 1.005, "{outward} vs {prism}");
        // 0° (flat inwards) leans more than 60°.
        let flat = vol(Some(direction(0.0, 0.2)));
        let steep = vol(Some(direction(60.0, 0.2)));
        assert!(flat < steep, "{flat} vs {steep}");
        // A heavier weight leans further.
        let heavy = vol(Some(direction(60.0, 1.0)));
        assert!(heavy < inward, "{heavy} vs {inward}");
    }
}

#[test]
fn tangent_condition_continues_a_tapered_face() {
    for taper in [-8.0, 8.0, -4.0, 12.0, -12.0] {
        let vol = |tangent: bool| {
            let mut p = PartDocument::default();
            let base = at_height(&mut p, 0.0, |s| square(s, 20.0));
            let pr = {
                let r = p.sketches[base].sketch.regions();
                vec![ProfileRef::of(&r[0], r[0].interior_point())]
            };
            let mut ex = ExtrudeFeature::new(pr, 10.0);
            ex.taper = Real::to_radians(taper);
            p.add_extrude(base, ex).unwrap();
            let top = p
                .body
                .faces()
                .find(|(f, _)| {
                    p.body
                        .face_plane(*f)
                        .is_some_and(|pl| pl.normal.z > 0.99 && (pl.origin.z - 10.0).abs() < 1e-6)
                })
                .and_then(|(f, x)| x.name.clone().map(|n| (f, n)))
                .unwrap();
            let face = crate::part::FaceRef {
                face: top.1,
                hint: [0.0, 0.0, 10.0],
            };
            // The far section as large as the top face: without a condition
            // a prism.
            let top_w = 20.0 + 2.0 * 10.0 * Real::to_radians(taper).tan();
            let b = at_height(&mut p, 40.0, |s| square(s, top_w));
            let mut def = LoftFeature::new(vec![LoftSection::FaceLoop(face), section(&p, b)]);
            def.operation = Operation::Join;
            if tangent {
                def.conditions = vec![LoftCondition {
                    kind: ConditionKind::Tangent,
                    ..Default::default()
                }];
            }
            p.add_loft(def).unwrap();
            healthy(&p);
            volume(&p)
        };
        let (free, tan) = (vol(false), vol(true));
        let w = 20.0 + 2.0 * 10.0 * Real::to_radians(taper).tan();
        let frustum = 10.0 / 3.0 * (400.0 + 20.0 * w + w * w);
        rel(free, frustum + w * w * 30.0, 2e-3);
        // A negative taper narrows the walls upwards: continuing it makes the
        // loft thinner near its start; a positive one wider.
        if taper < 0.0 {
            assert!(tan < free, "{taper}: {tan} vs {free}");
        } else {
            assert!(tan > free, "{taper}: {tan} vs {free}");
        }
    }
}

#[test]
fn condition_refusals() {
    let mut p = PartDocument::default();
    let a = at_height(&mut p, 0.0, |s| square(s, 20.0));
    let m = at_height(&mut p, 10.0, |s| square(s, 15.0));
    let b = at_height(&mut p, 20.0, |s| square(s, 10.0));
    for (conds, text) in [
        (
            vec![LoftCondition {
                kind: ConditionKind::Smooth,
                ..Default::default()
            }],
            "Smooth (G2)",
        ),
        (
            vec![LoftCondition {
                kind: ConditionKind::Tangent,
                ..Default::default()
            }],
            "Tangent needs a face loop",
        ),
        (
            vec![LoftCondition::default(), direction(60.0, 1.0)],
            "first and the last section only",
        ),
        (vec![direction(200.0, 1.0)], "between 0 and 180"),
    ] {
        let e = loft_of(&mut p, &[a, m, b], |d| d.conditions = conds.clone()).unwrap_err();
        assert!(e.contains(text), "{e}");
    }
    assert!(p.features.iter().all(|f| !matches!(f, Feature::Loft(_))));
    // Three sections with conditions at both ends.
    loft_of(&mut p, &[a, m, b], |d| {
        d.conditions = vec![
            direction(90.0, 1.0),
            LoftCondition::default(),
            direction(90.0, 1.0),
        ]
    })
    .unwrap();
    healthy(&p);
}

// ---------------------------------------------------------- Map points ---

#[test]
fn map_points_set_the_twist() {
    for (w, h) in [
        (20.0, 50.0),
        (10.0, 20.0),
        (30.0, 30.0),
        (6.0, 40.0),
        (16.0, 8.0),
    ] {
        let vol = |turned: bool| {
            let mut p = PartDocument::default();
            let a = at_height(&mut p, 0.0, |s| square(s, w));
            let b = at_height(&mut p, h, |s| square(s, w));
            let q = w / 2.0;
            let end = if turned { [q, -q, h] } else { [q, q, h] };
            loft_of(&mut p, &[a, b], |d| {
                d.automatic_mapping = false;
                d.map_points = vec![MapPoint {
                    points: vec![[q, q, 0.0], end],
                }];
            })
            .unwrap();
            healthy(&p);
            volume(&p)
        };
        rel(vol(false), w * w * h, 1e-3);
        // A quarter turn: vertex i goes to vertex i + 1; the section at t is
        // the square through (1 − t)·Aᵢ + t·Aᵢ₊₁, area w²((1 − t)² + t²).
        // The kernel measures the twisted (saddle) sides on its display
        // mesh, which cuts each one into few flat triangles: -1.2 %.
        rel(vol(true), w * w * h * 2.0 / 3.0, 1.5e-2);
    }
}

#[test]
fn map_point_refusals() {
    let mut p = PartDocument::default();
    let a = at_height(&mut p, 0.0, |s| square(s, 20.0));
    let b = at_height(&mut p, 30.0, |s| square(s, 20.0));
    let e = loft_of(&mut p, &[a, b], |d| {
        d.automatic_mapping = false;
        d.map_points = vec![MapPoint {
            points: vec![[10.0, 10.0, 0.0]],
        }];
    })
    .unwrap_err();
    assert!(e.contains("one point on each"), "{e}");
    let e = loft_of(&mut p, &[a, b], |d| {
        d.automatic_mapping = false;
        d.map_points = vec![MapPoint {
            points: vec![[10.0, 10.0, 0.0], [30.0, 10.0, 30.0]],
        }];
    })
    .unwrap_err();
    assert!(e.contains("off section 2"), "{e}");
    assert!(p.features.iter().all(|f| !matches!(f, Feature::Loft(_))));
}

// ---------------------------------------------------------- Closed Loop ---

/// `n` circles `r` at radius `big` on planes through the Z axis.
fn ring(n: usize, big: Real, r: Real) -> (PartDocument, Vec<usize>) {
    let mut p = PartDocument::default();
    let mut ks = Vec::new();
    for i in 0..n {
        let angle = 2.0 * PI * i as Real / n as Real;
        let plane = plane_through_z(&mut p, angle);
        let k = sketch_on(&mut p, plane, |_| {});
        let c = Point3::new(big * angle.cos(), big * angle.sin(), 0.0);
        let c2 = uv(&p, k, c);
        p.sketches[k].sketch.add_circle(c2, r).unwrap();
        ks.push(k);
    }
    (p, ks)
}

#[test]
fn closed_loop_rings() {
    for (n, big, r) in [
        (4, 30.0, 4.0),
        (6, 40.0, 5.0),
        (5, 25.0, 3.0),
        (8, 50.0, 10.0),
        (3, 30.0, 4.0),
    ] {
        let (mut p, ks) = ring(n, big, r);
        loft_of(&mut p, &ks, |d| d.closed_loop = true).unwrap();
        healthy(&p);
        let torus = 2.0 * PI * PI * big * r * r;
        // Four or three sections only approximate a circle of centres
        // (measured: 3 → −2.4 %, 4 → −0.8 %, 5 → −0.34 %, 6 → −0.17 %).
        let tol = if n <= 4 { 0.03 } else { 5e-3 };
        rel(volume(&p), torus, tol);
        assert_eq!(p.exact_bodies.0.len(), 1);
        let names: Vec<String> = p.body.faces().filter_map(|(_, f)| f.name.clone()).collect();
        assert!(!names.iter().any(|x| x.ends_with(":Start")), "{names:?}");
    }
}

// --------------------------------------------------------- Cross tests ---

#[test]
fn rails_loft_then_fillet_and_pattern() {
    for (r, b, h) in [(10.0, 5.0, 40.0), (8.0, 3.0, 30.0)] {
        // Fillet of the bottom rim.
        let (mut p, s0, s1, rails) = bottle(r, b, h);
        rails_loft(&mut p, &[s0, s1], rails, |_| {}).unwrap();
        let before = volume(&p);
        let edges: Vec<crate::part::blend::EdgeRef> = p
            .body
            .edges()
            .filter_map(|(e, _)| crate::part::blend::edge_ref(&p.body, e))
            .filter(|e| e.faces.iter().any(|f| f == "Loft1:Start"))
            .collect();
        assert!(!edges.is_empty());
        match p.add_blend(crate::part::blend::BlendFeature::fillet(edges, 1.0)) {
            Ok(_) => {
                healthy(&p);
                let v = volume(&p);
                assert!(v < before && v > before * 0.97, "{v} vs {before}");
            }
            // A clear refusal leaves the loft as it was.
            Err(e) => {
                let e = e.to_string();
                assert!(!e.is_empty());
                rel(volume(&p), before, 1e-9);
            }
        }
        // A rectangular pattern of the loft: two bottles.
        let (mut p, s0, s1, rails) = bottle(r, b, h);
        rails_loft(&mut p, &[s0, s1], rails, |_| {}).unwrap();
        let one = volume(&p);
        let mut pat = crate::part::PatternFeature::rectangular();
        pat.features = vec!["Loft1".into()];
        if let crate::part::PatternKind::Rectangular { first, .. } = &mut pat.kind {
            first.dir = Some(crate::part::PullRef::Line(LineRef::Origin(0)));
            first.count = 2;
            first.spacing = 4.0 * (r + b.max(0.0));
        }
        p.add_pattern(pat).unwrap();
        assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
        rel(volume(&p), 2.0 * one, 1e-3);
    }
}

#[test]
fn rails_loft_then_shell() {
    let (mut p, s0, s1, rails) = bottle(10.0, 5.0, 40.0);
    rails_loft(&mut p, &[s0, s1], rails, |_| {}).unwrap();
    let before = volume(&p);
    let n = p.features.len();
    let top = p.body.find_face("Loft1:End").unwrap();
    let mut def = crate::part::shell::ShellFeature::new(1.0);
    def.remove
        .push(crate::part::shell::face_ref(&p.body, top, None).unwrap());
    match p.add_shell(def) {
        Ok(_) => {
            assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
            assert!(volume(&p) < before * 0.5);
        }
        // The kernel offsets analytic faces only: a clear refusal, the part
        // unchanged.
        Err(e) => {
            assert!(!e.to_string().is_empty());
            assert_eq!(p.features.len(), n);
            rel(volume(&p), before, 1e-9);
        }
    }
}
