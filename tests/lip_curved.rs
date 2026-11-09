//! Lip and Groove on rims that are not a plain rectangle: rounded boxes, covers with a rounded
//! top, outlines of lines and arcs, splines (spec 5.8, Lip). The bead is 1.5 high and 1 wide.

use cadcraft3d::kernel::{Point3, Real};
use cadcraft3d::part::blend::edge_ref;
use cadcraft3d::part::plastic::lip::{LipKind, LipOp};
use cadcraft3d::part::shell::{FaceRef, ShellFeature};
use cadcraft3d::part::{
    BlendFeature, BodyOp, BodyOpFeature, EdgeRef, ExtrudeFeature, OriginPlane, PartDocument,
    PlaneRef, ProfileRef, SketchOptions, SketchPlacement,
};
use cadcraft3d::sketch::{Sketch, SplineFit};
use nalgebra::Point2;
use std::f64::consts::PI;

const HEIGHT: Real = 1.5;
const WIDTH: Real = 1.0;
/// Volume tolerance on spline rims. Walls follow the edge within a few microns; a groove's far
/// wall is alone on the material, ~4 um on a 1 mm groove (a bead's two walls move together).
const SPLINE_TOL: Real = 6e-3;
/// How far a bead keeps off a spline edge (mm), as `part::plastic::lip` does.
const SPLINE_CLEAR: Real = 0.01;

fn p2(x: Real, y: Real) -> Point2<Real> {
    Point2::new(x, y)
}

fn volume(p: &PartDocument) -> Real {
    p.exact_bodies
        .0
        .iter()
        .filter(|s| !s.is_open())
        .filter_map(|s| s.volume_at(2e-4))
        .sum()
}

/// A closed outline (lines, arcs or splines drawn by `draw`) extruded 20 mm up from XY.
fn extruded(draw: impl FnOnce(&mut Sketch)) -> PartDocument {
    let mut p = PartDocument::default();
    let i = p
        .create_sketch(
            SketchPlacement {
                plane: PlaneRef::Origin(OriginPlane::XY),
                ..Default::default()
            },
            &SketchOptions {
                auto_project_origin: false,
                auto_reference_edges: false,
                ..Default::default()
            },
        )
        .unwrap_or_else(|e| panic!("{e}"));
    draw(&mut p.sketches[i].sketch);
    let r = p.sketches[i].sketch.regions();
    assert_eq!(r.len(), 1, "one region");
    let ex = ExtrudeFeature::new(vec![ProfileRef::of(&r[0], r[0].interior_point())], 20.0);
    p.add_extrude(i, ex).unwrap_or_else(|e| panic!("{e}"));
    p
}

/// Shells `p` 2 mm inside with its flat face at height `z` open.
fn shelled(mut p: PartDocument, z: Real) -> PartDocument {
    let (face, hint) = p
        .body
        .faces()
        .find_map(|(_, face)| {
            let pts = &face.mesh.as_ref()?.positions;
            pts.iter()
                .all(|q| (q.z - z).abs() < 1e-6)
                .then(|| (face.name.clone(), pts[0]))
        })
        .and_then(|(n, q)| Some((n?, q)))
        .unwrap_or_else(|| panic!("no flat face at z = {z}"));
    let mut sh = ShellFeature::new(2.0);
    sh.remove.push(FaceRef {
        face,
        hint: [hint.x, hint.y, hint.z],
    });
    p.add_shell(sh).unwrap_or_else(|e| panic!("{e}"));
    p
}

/// A rim's edges with their points.
type Path = Vec<(EdgeRef, Vec<Point3>)>;

/// The edges lying at height `z`, split by where they are.
fn rim_edges(p: &PartDocument, z: Real, inner: impl Fn(&Point3) -> bool) -> (Path, Path) {
    let (mut a, mut b) = (Vec::new(), Vec::new());
    for e in p.body.visible_edges() {
        let Ok(pts) = p.body.sample_edge(e, &Default::default()) else {
            continue;
        };
        if !pts.iter().all(|q| (q.z - z).abs() < 1e-6) {
            continue;
        }
        let Some(r) = edge_ref(&p.body, e) else {
            continue;
        };
        if inner(&Point3::from(r.hint)) {
            a.push((r, pts))
        } else {
            b.push((r, pts))
        }
    }
    (a, b)
}

/// The path's points in order round the loop, counter-clockwise.
fn outline(path: &Path) -> Vec<Point2<Real>> {
    let mut left: Vec<Vec<Point2<Real>>> = path
        .iter()
        .map(|(_, pts)| pts.iter().map(|q| p2(q.x, q.y)).collect())
        .collect();
    let mut out = left.pop().unwrap_or_else(|| panic!("an empty path"));
    while !left.is_empty() {
        let tail = out[out.len() - 1];
        let near = |q: Point2<Real>| (q - tail).norm() < 1e-3;
        let k = left
            .iter()
            .position(|c| near(c[0]) || near(c[c.len() - 1]))
            .unwrap_or_else(|| panic!("the path is not one chain"));
        let mut next = left.swap_remove(k);
        if !near(next[0]) {
            next.reverse();
        }
        out.extend(next.into_iter().skip(1));
    }
    out.dedup_by(|a, b| (*a - *b).norm() < 1e-6);
    if (out[0] - out[out.len() - 1]).norm() < 1e-3 {
        out.pop();
    }
    if area(&out) < 0.0 {
        out.reverse();
    }
    out
}

fn area(pts: &[Point2<Real>]) -> Real {
    let n = pts.len();
    (0..n)
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            a.x * b.y - a.y * b.x
        })
        .sum::<Real>()
        / 2.0
}

/// The area inside a counter-clockwise outline moved `d` outwards, mitred at its corners.
fn grown(pts: &[Point2<Real>], d: Real) -> Real {
    let n = pts.len();
    let out = |a: Point2<Real>, b: Point2<Real>| {
        let t = (b - a).normalize();
        nalgebra::Vector2::new(t.y, -t.x)
    };
    let moved: Vec<Point2<Real>> = (0..n)
        .map(|i| {
            let (a, b, c) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
            let (u, v) = (out(a, b), out(b, c));
            b + (u + v) * (d / (1.0 + u.dot(&v)))
        })
        .collect();
    area(&moved)
}

/// The volume a spline path's bead adds or its groove removes, from the outline itself: `outward`
/// is the rim's side.
fn from_outline(path: &Path, outward: bool, kind: LipKind) -> Real {
    let pts = outline(path);
    let foot = if kind == LipKind::Lip {
        SPLINE_CLEAR
    } else {
        0.0
    };
    let sign = if outward { 1.0 } else { -1.0 };
    let ring = (grown(&pts, sign * WIDTH) - grown(&pts, sign * foot)).abs();
    ring * HEIGHT * if kind == LipKind::Lip { 1.0 } else { -1.0 }
}

fn apply(p: &PartDocument, path: &Path, kind: LipKind) -> Real {
    let mut q = p.clone();
    let mut op = LipOp::new(path.iter().map(|e| e.0.clone()).collect(), HEIGHT, WIDTH);
    op.kind = kind;
    q.add_body_op(BodyOpFeature::new(BodyOp::Lip(op)))
        .unwrap_or_else(|e| panic!("{kind:?}: {e}"));
    volume(&q) - volume(p)
}

fn check(name: &str, p: &PartDocument, path: &Path, want: impl Fn(LipKind) -> Real, tol: Real) {
    for kind in [LipKind::Lip, LipKind::Groove] {
        let dv = apply(p, path, kind);
        let w = want(kind);
        assert!(
            (dv - w).abs() <= w.abs() * tol + 0.05,
            "{name} {kind:?}: dV {dv:.3}, expected {w:.3}"
        );
    }
}

/// A closed ring: `area` of it times the height, added by a Lip and removed by a Groove.
fn ring(area: Real) -> impl Fn(LipKind) -> Real {
    move |k| area * HEIGHT * if k == LipKind::Lip { 1.0 } else { -1.0 }
}

fn perimeter(w: Real, h: Real, r: Real) -> Real {
    2.0 * (w + h) - 8.0 * r + 2.0 * PI * r
}

/// Inside the 80 x 50 R8 outline by more than `by` (signed distance of a rounded rectangle).
fn inside_by(q: &Point3, by: Real) -> bool {
    let (dx, dy) = ((q.x - 40.0).abs() - 32.0, (q.y - 25.0).abs() - 17.0);
    dx.max(0.0).hypot(dy.max(0.0)) + dx.max(dy).min(0.0) - 8.0 < -by
}

fn rounded_outline(sk: &mut Sketch, w: Real, h: Real, r: Real) {
    let pt = |sk: &mut Sketch, x, y| sk.add_point(p2(x, y));
    let a0 = pt(sk, r, 0.0);
    let a1 = pt(sk, w - r, 0.0);
    let b0 = pt(sk, w, r);
    let b1 = pt(sk, w, h - r);
    let c0 = pt(sk, w - r, h);
    let c1 = pt(sk, r, h);
    let d0 = pt(sk, 0.0, h - r);
    let d1 = pt(sk, 0.0, r);
    for (s, e) in [(a0, a1), (b0, b1), (c0, c1), (d0, d1)] {
        sk.add_line_between(s, e).unwrap_or_else(|e| panic!("{e}"));
    }
    for (c, s, e) in [
        ((w - r, r), a1, b0),
        ((w - r, h - r), b1, c0),
        ((r, h - r), c1, d0),
        ((r, r), d1, a0),
    ] {
        let c = pt(sk, c.0, c.1);
        sk.add_arc(c, s, e).unwrap_or_else(|e| panic!("{e}"));
    }
}

/// An 80 x 50 x 20 box with its upright edges filleted R8.
fn upright_rounds() -> PartDocument {
    let mut b =
        PartDocument::with_box("Box1", [80.0, 50.0, 20.0]).unwrap_or_else(|e| panic!("{e}"));
    let up: Vec<EdgeRef> = b
        .body
        .edges()
        .filter_map(|(e, _)| edge_ref(&b.body, e))
        .filter(|r| r.hint[2] > 1.0 && r.hint[2] < 19.0)
        .collect();
    b.add_blend(BlendFeature::fillet(up, 8.0))
        .unwrap_or_else(|e| panic!("{e}"));
    b
}

/// The edges of the outer outline at height `z`, on a body not shelled yet.
fn outer_loop(p: &PartDocument, z: Real) -> Vec<EdgeRef> {
    let (_, all) = rim_edges(p, z, |_| false);
    all.into_iter().map(|e| e.0).collect()
}

/// A wall 2 thick of the 80 x 50 R8 outline: inner outline 76 x 46 R6.
fn inner_ring() -> Real {
    perimeter(76.0, 46.0, 6.0) * WIDTH + PI * WIDTH * WIDTH
}

fn outer_ring() -> Real {
    perimeter(80.0, 50.0, 8.0) * WIDTH - PI * WIDTH * WIDTH
}

/// The owner's rounded boxes: drawn round, and filleted.
#[test]
fn rounded_boxes() {
    let drawn = shelled(extruded(|sk| rounded_outline(sk, 80.0, 50.0, 8.0)), 20.0);
    let filleted = shelled(upright_rounds(), 20.0);
    for (name, p) in [("drawn", &drawn), ("filleted", &filleted)] {
        let (inn, out) = rim_edges(p, 20.0, |q| inside_by(q, 1.0));
        assert_eq!((inn.len(), out.len()), (8, 8), "{name}");
        check(name, p, &inn, ring(inner_ring()), 1e-3);
        check(name, p, &out, ring(outer_ring()), 1e-3);
    }
}

/// Open at the bottom, the lip on the flat rim there: a cover with every top edge rounded, and
/// boxes rounded either way. Their inner outline came with joints ~1e-7 apart.
#[test]
fn open_at_the_bottom() {
    let mut cover = upright_rounds();
    let top = outer_loop(&cover, 20.0);
    cover
        .add_blend(BlendFeature::fillet(top, 3.0))
        .unwrap_or_else(|e| panic!("{e}"));
    let drawn = extruded(|sk| rounded_outline(sk, 80.0, 50.0, 8.0));
    for (name, b) in [
        ("cover", cover),
        ("drawn", drawn),
        ("filleted", upright_rounds()),
    ] {
        let p = shelled(b, 0.0);
        let (inn, out) = rim_edges(&p, 0.0, |q| inside_by(q, 1.0));
        assert_eq!((inn.len(), out.len()), (8, 8), "{name}");
        check(name, &p, &inn, ring(inner_ring()), 1e-3);
        check(name, &p, &out, ring(outer_ring()), 1e-3);
    }
}

/// A rim whose outer edge is rounded: the flat part of the rim still carries the bead, from the
/// inner edge and from the tangent edge where the round ends.
#[test]
fn rim_with_a_rounded_outer_edge() {
    let mut p = shelled(upright_rounds(), 20.0);
    let (_, out) = rim_edges(&p, 20.0, |q| inside_by(q, 1.0));
    p.add_blend(BlendFeature::fillet(
        out.into_iter().map(|e| e.0).collect(),
        0.5,
    ))
    .unwrap_or_else(|e| panic!("{e}"));
    let (inn, tangent) = rim_edges(&p, 20.0, |q| inside_by(q, 1.0));
    check("rounded rim, inner", &p, &inn, ring(inner_ring()), 1e-3);
    let at = perimeter(79.0, 49.0, 7.5) * WIDTH - PI * WIDTH * WIDTH;
    check("rounded rim, tangent", &p, &tangent, ring(at), 1e-3);
}

/// Two straight sides and two spline ones bulging out.
#[test]
fn box_with_spline_sides() {
    let p = shelled(
        extruded(|sk| {
            let a = sk.add_point(p2(0.0, 0.0));
            let b = sk.add_point(p2(80.0, 0.0));
            let c = sk.add_point(p2(80.0, 50.0));
            let d = sk.add_point(p2(0.0, 50.0));
            let m1 = sk.add_point(p2(90.0, 25.0));
            let m2 = sk.add_point(p2(-10.0, 25.0));
            sk.add_line_between(a, b).unwrap_or_else(|e| panic!("{e}"));
            sk.add_line_between(c, d).unwrap_or_else(|e| panic!("{e}"));
            sk.add_interp_spline(vec![b, m1, c])
                .unwrap_or_else(|e| panic!("{e}"));
            sk.add_interp_spline(vec![d, m2, a])
                .unwrap_or_else(|e| panic!("{e}"));
        }),
        20.0,
    );
    let inner = |q: &Point3| {
        let on_line = q.y.abs() < 1e-3 || (q.y - 50.0).abs() < 1e-3;
        q.y > 1.0
            && q.y < 49.0
            && !on_line
            && (q.x - 40.0).abs() < 40.0 + 10.0 * (1.0 - ((q.y - 25.0) / 25.0).powi(2)) - 1.0
    };
    let (inn, out) = rim_edges(&p, 20.0, inner);
    check(
        "spline sides, inner",
        &p,
        &inn,
        |k| from_outline(&inn, true, k),
        SPLINE_TOL,
    );
    check(
        "spline sides, outer",
        &p,
        &out,
        |k| from_outline(&out, false, k),
        SPLINE_TOL,
    );
}

/// One closed spline: an oval.
#[test]
fn oval_box() {
    let p = shelled(
        extruded(|sk| {
            let mut pts: Vec<_> = [
                (40.0, 0.0),
                (80.0, 25.0),
                (40.0, 50.0),
                (0.0, 25.0),
                (40.0, 0.0),
            ]
            .iter()
            .map(|&(x, y)| sk.add_point(p2(x, y)))
            .collect();
            pts[4] = pts[0];
            let id = sk.add_spline(pts).unwrap_or_else(|e| panic!("{e}"));
            sk.set_spline_fit(
                id,
                SplineFit {
                    closed: true,
                    ..Default::default()
                },
            )
            .unwrap_or_else(|e| panic!("{e}"));
        }),
        20.0,
    );
    let (inn, out) = rim_edges(&p, 20.0, |q| {
        ((q.x - 40.0) / 39.0).powi(2) + ((q.y - 25.0) / 24.0).powi(2) < 1.0
    });
    check(
        "oval, inner",
        &p,
        &inn,
        |k| from_outline(&inn, true, k),
        SPLINE_TOL,
    );
    check(
        "oval, outer",
        &p,
        &out,
        |k| from_outline(&out, false, k),
        SPLINE_TOL,
    );
}

/// The panel's own sizes (1 high, 0.75 wide) on a cover: the Join failed there in the app, with
/// the bead's wall flush with the cover's inner wall.
#[test]
fn panel_defaults_on_a_cover() {
    let mut b = upright_rounds();
    let top = outer_loop(&b, 20.0);
    b.add_blend(BlendFeature::fillet(top, 3.0))
        .unwrap_or_else(|e| panic!("{e}"));
    let p = shelled(b, 0.0);
    let (inn, out) = rim_edges(&p, 0.0, |q| inside_by(q, 1.0));
    let (h, w) = (1.0, 0.75);
    for (path, area) in [
        (&inn, perimeter(76.0, 46.0, 6.0) * w + PI * w * w),
        (&out, perimeter(80.0, 50.0, 8.0) * w - PI * w * w),
    ] {
        for (kind, sign) in [(LipKind::Lip, 1.0), (LipKind::Groove, -1.0)] {
            let mut q = p.clone();
            let mut op = LipOp::new(path.iter().map(|e| e.0.clone()).collect(), h, w);
            op.kind = kind;
            q.add_body_op(BodyOpFeature::new(BodyOp::Lip(op)))
                .unwrap_or_else(|e| panic!("{kind:?}: {e}"));
            let dv = volume(&q) - volume(&p);
            let want = sign * area * h;
            assert!(
                (dv - want).abs() < 0.05,
                "{kind:?}: dV {dv:.3}, expected {want:.3}"
            );
        }
    }
}
