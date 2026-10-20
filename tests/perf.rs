//! Speed budgets of typical operations (`docs/PERF.md`): each case runs a
//! few times and its median must stay within its budget. A new feature
//! adds its case here; a case over budget means looking for a faster way,
//! not raising the number.
//!
//! The budgets are for the test profile (our crate at opt-level 1,
//! dependencies at 3); release builds are faster.

use std::time::Instant;

use cadcraft3d::kernel::{Point3, Real};
use cadcraft3d::part::blend::{edge_ref, tangent_chain};
use cadcraft3d::part::{
    BlendFeature, EdgeRef, ExtentDirection, ExtrudeFeature, Operation, PartDocument, ProfileRef,
    SketchOptions, SketchPlacement,
};

/// A coil of a Ø2 circle at radius 10 about Z (5 mm pitch): `turns`,
/// closed ends or not; with `rod`, a triangle cut 6 turns into a Ø20 rod.
fn coil_part(turns: Real, closed: bool, rod: bool) -> PartDocument {
    use cadcraft3d::part::work::LineRef;
    use cadcraft3d::part::{CoilAxis, CoilFeature, OriginPlane};
    use cadcraft3d::sketch::Point2;
    let o = SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    };
    let mut part = PartDocument::default();
    if rod {
        let k = part
            .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &o)
            .unwrap();
        part.sketches[k]
            .sketch
            .add_circle(Point2::new(0.0, 0.0), 10.0)
            .unwrap();
        let r = part.sketches[k].sketch.regions();
        let pr = vec![ProfileRef::of(&r[0], r[0].interior_point())];
        part.add_extrude(k, ExtrudeFeature::new(pr, 30.0)).unwrap();
    }
    let k = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XZ), &o)
        .unwrap();
    let sk = &mut part.sketches[k].sketch;
    if rod {
        let a = sk.add_point(Point2::new(10.5, 4.0));
        let b = sk.add_point(Point2::new(9.0, 5.0));
        let c = sk.add_point(Point2::new(10.5, 6.0));
        sk.add_line_between(a, b).unwrap();
        sk.add_line_between(b, c).unwrap();
        sk.add_line_between(c, a).unwrap();
    } else {
        sk.add_circle(Point2::new(10.0, 0.0), 1.0).unwrap();
    }
    let r = sk.regions();
    let pr = vec![ProfileRef::of(&r[0], r[0].interior_point())];
    let mut c = CoilFeature::new(pr, CoilAxis::Line(LineRef::Origin(2)));
    (c.pitch, c.revolutions) = (if rod { 3.0 } else { 5.0 }, turns);
    c.start.closed = closed;
    c.end.closed = closed;
    if rod {
        c.operation = Operation::Cut;
    }
    part.add_coil(k, c).unwrap();
    part
}

/// An open box 60 × 40 × 20 (2 mm walls) with a web network (3 webs) or
/// one web with two bosses, sketched on the rim plane.
fn rib_part(network: bool) -> PartDocument {
    use cadcraft3d::part::{OriginPlane, PlaneRef, RibFeature, WorkDef, WorkFeature, WorkPlaneDef};
    use cadcraft3d::sketch::Point2;
    let o = SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    };
    let mut part = PartDocument::default();
    let k = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &o)
        .unwrap();
    part.sketches[k]
        .sketch
        .add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(60.0, 40.0))
        .unwrap();
    let r = part.sketches[k].sketch.regions();
    let pr = vec![ProfileRef::of(&r[0], r[0].interior_point())];
    part.add_extrude(k, ExtrudeFeature::new(pr, 20.0)).unwrap();
    let wp = part
        .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
            base: PlaneRef::Origin(OriginPlane::XY),
            distance: 20.0,
        })))
        .unwrap();
    let on = SketchPlacement {
        plane: PlaneRef::Work(wp),
        ..Default::default()
    };
    let k = part.create_sketch(on.clone(), &o).unwrap();
    part.sketches[k]
        .sketch
        .add_rectangle_two_point(Point2::new(2.0, 2.0), Point2::new(58.0, 38.0))
        .unwrap();
    let r = part.sketches[k].sketch.regions();
    let pr = vec![ProfileRef::of(&r[0], r[0].interior_point())];
    let mut cut = ExtrudeFeature::new(pr, 18.0);
    cut.operation = Operation::Cut;
    cut.direction = ExtentDirection::Flipped;
    part.add_extrude(k, cut).unwrap();
    let k = part.create_sketch(on, &o).unwrap();
    let sk = &mut part.sketches[k].sketch;
    let mut lines = vec![sk.add_line(Point2::new(2.0, 20.0), Point2::new(58.0, 20.0))];
    let mut web = if network {
        lines.push(sk.add_line(Point2::new(20.0, 2.0), Point2::new(20.0, 38.0)));
        lines.push(sk.add_line(Point2::new(40.0, 2.0), Point2::new(40.0, 38.0)));
        RibFeature::new(lines, 2.0)
    } else {
        let a = sk.add_point(Point2::new(20.0, 20.0));
        let b = sk.add_point(Point2::new(40.0, 20.0));
        let mut w = RibFeature::new(lines, 2.0);
        w.boss.centers = vec![a, b];
        w
    };
    web.extend = true;
    part.add_rib(k, web).unwrap();
    part
}

/// Median wall time (ms) of `runs` calls.
/// Budgets of the Face Draft Symmetric and Shell Rounded Corners cases (ms).
const BUDGET_SYMMETRIC_DRAFT: f64 = 700.0;
const BUDGET_ROUNDED_SHELL: f64 = 400.0;
const BUDGET_COMBINE: f64 = 300.0;
const BUDGET_SPLIT: f64 = 500.0;
/// One kernel call (`split_body`, the section through four rounds at an
/// angle) is ~0.8 s; classifying the parts by volume added 1.4 s (removed).
const BUDGET_SPLIT_INCLINED: f64 = 1500.0;

fn median_ms(runs: usize, mut f: impl FnMut()) -> f64 {
    let mut t: Vec<f64> = (0..runs)
        .map(|_| {
            let t0 = Instant::now();
            f();
            t0.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    t.sort_by(f64::total_cmp);
    t[t.len() / 2]
}

/// The fillet/chamfer acceptance part: box, 8 fillets r5, top-loop chamfer 2.
fn rounded_box() -> PartDocument {
    let (a, b, h, r) = (40.0, 30.0, 20.0, 5.0);
    let mut part = PartDocument::with_box("Box1", [a, b, h]).unwrap();
    let f = |x: &str, y: &str, hint: [Real; 3]| EdgeRef {
        faces: [format!("Box1:{x}"), format!("Box1:{y}")],
        hint,
    };
    part.add_blend(BlendFeature::fillet(
        vec![
            f("Front", "Right", [a, 0.0, h / 2.0]),
            f("Right", "Back", [a, b, h / 2.0]),
            f("Back", "Left", [0.0, b, h / 2.0]),
            f("Left", "Front", [0.0, 0.0, h / 2.0]),
            f("Bottom", "Front", [a / 2.0, 0.0, 0.0]),
            f("Bottom", "Right", [a, b / 2.0, 0.0]),
            f("Bottom", "Back", [a / 2.0, b, 0.0]),
            f("Bottom", "Left", [0.0, b / 2.0, 0.0]),
        ],
        r,
    ))
    .unwrap();
    let top = part
        .body
        .edges()
        .find(|(e, _)| {
            edge_ref(&part.body, *e).is_some_and(|x| {
                x.faces.contains(&"Box1:Top".to_owned())
                    && x.faces.contains(&"Box1:Front".to_owned())
            })
        })
        .map(|(e, _)| e)
        .unwrap();
    part.add_blend(BlendFeature::chamfer(tangent_chain(&part.body, top), 2.0))
        .unwrap();
    part
}

/// A 40 mm cube with a 32×32 pocket 36 deep (4 mm walls).
fn hollow() -> PartDocument {
    let s = 40.0;
    let mut part = PartDocument::with_box("Box1", [s, s, s]).unwrap();
    let opts = SketchOptions {
        auto_project_origin: false,
        ..Default::default()
    };
    let i = part
        .create_sketch(SketchPlacement::on_face("Box1:Top"), &opts)
        .unwrap();
    let plane = part.sketches[i].sketch.plane.clone();
    let a = plane.project(&Point3::new(4.0, 4.0, s));
    let b = plane.project(&Point3::new(36.0, 36.0, s));
    let sk = &mut part.sketches[i].sketch;
    sk.add_rectangle_two_point(a, b).unwrap();
    let r = sk.regions();
    let pocket = r.iter().find(|x| x.holes.is_empty()).unwrap();
    let mut ex = ExtrudeFeature::new(vec![ProfileRef::of(pocket, pocket.interior_point())], 36.0);
    ex.direction = ExtentDirection::Flipped;
    ex.operation = Operation::Cut;
    part.add_extrude(i, ex).unwrap();
    part
}

/// The hollow box's edges below the rim (outer and inner).
fn below_rim(part: &PartDocument) -> Vec<EdgeRef> {
    part.body
        .edges()
        .filter_map(|(e, _)| edge_ref(&part.body, e))
        .filter(|r| r.hint[2] < 40.0 - 1e-6)
        .collect()
}

/// A box with one round feature on its top at (x, y): a through hole
/// (`cut`) or a boss `h` high (`Extrusion1`).
fn box_with_circle(size: [Real; 3], x: Real, y: Real, r: Real, cut: bool, h: Real) -> PartDocument {
    let mut part = PartDocument::with_box("Box1", size).unwrap();
    let opts = SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    };
    let j = part
        .create_sketch(SketchPlacement::on_face("Box1:Top"), &opts)
        .unwrap();
    let c = part.sketches[j]
        .sketch
        .plane
        .project(&Point3::new(x, y, size[2]));
    part.sketches[j].sketch.add_circle(c, r).unwrap();
    let rg = part.sketches[j].sketch.regions();
    let mut ex = ExtrudeFeature::new(vec![ProfileRef::of(&rg[0], rg[0].interior_point())], h);
    if cut {
        ex.operation = Operation::Cut;
        ex.extent = cadcraft3d::part::Extent::ThroughAll;
        ex.direction = ExtentDirection::Flipped;
    } else {
        ex.operation = Operation::Join;
    }
    part.add_extrude(j, ex).unwrap();
    part
}

/// A rectangular pattern of `Extrusion1`, n1 × n2 at `s` along X and Y.
fn pattern_grid(n1: u32, n2: u32, s: Real) -> cadcraft3d::part::PatternFeature {
    use cadcraft3d::part::{LineRef, PatternDirection, PatternFeature, PatternKind, PullRef};
    let mut p = PatternFeature::rectangular();
    p.features = vec!["Extrusion1".into()];
    if let PatternKind::Rectangular { first, second } = &mut p.kind {
        (first.dir, first.count, first.spacing) = (Some(PullRef::Line(LineRef::Origin(0))), n1, s);
        *second = Some(PatternDirection {
            dir: Some(PullRef::Line(LineRef::Origin(1))),
            count: n2,
            spacing: s,
            ..PatternDirection::new()
        });
    }
    p
}

/// A 30-feature part: a box, 15 blocks (New Solid) side by side and a
/// hole cut into each of the first 14 (Cut limited to its block).
fn blocks_30() -> PartDocument {
    use cadcraft3d::part::OriginPlane;
    use cadcraft3d::sketch::Point2;
    let mut part = PartDocument::with_box("Box1", [10.0, 10.0, 5.0]).unwrap();
    let opts = SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    };
    let xy = || SketchPlacement::on_origin_plane(OriginPlane::XY);
    for k in 0..15 {
        let x0 = 20.0 + 20.0 * k as Real;
        let j = part.create_sketch(xy(), &opts).unwrap();
        part.sketches[j]
            .sketch
            .add_rectangle_two_point(Point2::new(x0, 0.0), Point2::new(x0 + 10.0, 10.0))
            .unwrap();
        let rg = part.sketches[j].sketch.regions();
        let mut ex = ExtrudeFeature::new(vec![ProfileRef::of(&rg[0], rg[0].interior_point())], 5.0);
        ex.operation = Operation::NewSolid;
        let block = part.add_extrude(j, ex).unwrap();
        if k == 14 {
            break;
        }
        let j = part.create_sketch(xy(), &opts).unwrap();
        let c = part.sketches[j]
            .sketch
            .plane
            .project(&Point3::new(x0 + 5.0, 5.0, 0.0));
        part.sketches[j].sketch.add_circle(c, 2.0).unwrap();
        let rg = part.sketches[j].sketch.regions();
        let mut ex = ExtrudeFeature::new(vec![ProfileRef::of(&rg[0], rg[0].interior_point())], 1.0);
        ex.operation = Operation::Cut;
        ex.extent = cadcraft3d::part::Extent::ThroughAll;
        ex.solids = vec![block];
        part.add_extrude(j, ex).unwrap();
    }
    part
}

/// Sweep / Loft cases (`docs/spec/5.1-sweep.md`, `5.2-loft.md`): a profile
/// on XY and a path on XZ, or sections on planes parallel to XY.
fn sweep_part(twist_deg: Real, square: bool) -> PartDocument {
    use cadcraft3d::part::{OriginPlane, PathRef, SweepFeature};
    use cadcraft3d::sketch::Point2;
    let o = SketchOptions {
        auto_project_origin: false,
        ..Default::default()
    };
    let mut part = PartDocument::default();
    let a = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &o)
        .unwrap();
    if square {
        part.sketches[a]
            .sketch
            .add_rectangle_two_point(Point2::new(-5.0, -5.0), Point2::new(5.0, 5.0))
            .unwrap();
    } else {
        part.sketches[a]
            .sketch
            .add_circle(Point2::new(0.0, 0.0), 4.0)
            .unwrap();
    }
    let b = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XZ), &o)
        .unwrap();
    {
        let s = &mut part.sketches[b].sketch;
        if square {
            s.add_line(Point2::new(0.0, 0.0), Point2::new(0.0, 40.0));
        } else {
            let c = s.add_point(Point2::new(30.0, 0.0));
            let t = s.add_point(Point2::new(30.0, 30.0));
            let z = s.add_point(Point2::new(0.0, 0.0));
            s.add_arc(c, t, z).unwrap();
        }
    }
    let r = part.sketches[a].sketch.regions();
    let path = PathRef::Sketch {
        sketch: part.sketches[b].id.clone(),
        curves: part.sketches[b]
            .sketch
            .entities()
            .map(|(id, _)| id)
            .collect(),
    };
    let mut def = SweepFeature::new(vec![ProfileRef::of(&r[0], r[0].interior_point())], path);
    def.twist = twist_deg.to_radians();
    part.add_sweep(a, def).unwrap();
    part
}

/// The owner's session (2026-10-20): a circle R17.72 on XY along an
/// interpolation spline on YZ (a sweep along points, smooth B-spline walls).
fn sweep_spline_part() -> PartDocument {
    use cadcraft3d::part::{OriginPlane, PathRef, SweepFeature};
    use cadcraft3d::sketch::Point2;
    let o = SketchOptions {
        auto_project_origin: false,
        ..Default::default()
    };
    let mut part = PartDocument::default();
    let a = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &o)
        .unwrap();
    part.sketches[a]
        .sketch
        .add_circle(Point2::new(0.0, 0.0), 17.72)
        .unwrap();
    let b = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::YZ), &o)
        .unwrap();
    {
        let s = &mut part.sketches[b].sketch;
        let ids = [(0.0, 0.0), (0.0, 16.73), (9.20, 44.53), (36.42, 80.02)]
            .iter()
            .map(|(x, y)| s.add_point(Point2::new(*x, *y)))
            .collect();
        s.add_interp_spline(ids).unwrap();
    }
    let r = part.sketches[a].sketch.regions();
    let path = PathRef::Sketch {
        sketch: part.sketches[b].id.clone(),
        curves: part.sketches[b]
            .sketch
            .entities()
            .map(|(id, _)| id)
            .collect(),
    };
    let def = SweepFeature::new(vec![ProfileRef::of(&r[0], r[0].interior_point())], path);
    part.add_sweep(a, def).unwrap();
    part
}

fn loft_part(n: usize) -> PartDocument {
    use cadcraft3d::part::{
        LoftFeature, LoftSection, OriginPlane, PlaneRef, WorkDef, WorkFeature, WorkPlaneDef,
    };
    use cadcraft3d::sketch::Point2;
    let o = SketchOptions {
        auto_project_origin: false,
        ..Default::default()
    };
    let mut part = PartDocument::default();
    let mut secs = Vec::new();
    for k in 0..n {
        let placement = if k == 0 {
            SketchPlacement::on_origin_plane(OriginPlane::XY)
        } else {
            let wp = part
                .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
                    base: PlaneRef::Origin(OriginPlane::XY),
                    distance: 15.0 * k as Real,
                })))
                .unwrap();
            SketchPlacement {
                plane: PlaneRef::Work(wp),
                ..Default::default()
            }
        };
        let i = part.create_sketch(placement, &o).unwrap();
        let sk = &mut part.sketches[i].sketch;
        if k % 2 == 0 {
            sk.add_rectangle_two_point(Point2::new(-10.0, -10.0), Point2::new(10.0, 10.0))
                .unwrap();
        } else {
            sk.add_circle(Point2::new(0.0, 0.0), 9.0).unwrap();
        }
        let r = sk.regions();
        secs.push(LoftSection::Profile {
            sketch: part.sketches[i].id.clone(),
            profile: ProfileRef::of(&r[0], r[0].interior_point()),
        });
    }
    part.add_loft(LoftFeature::new(secs)).unwrap();
    part
}

/// Guided lofts (`docs/spec/5.2-loft.md`): `rails` a Ø20 bottle on two arc
/// rails, `center` a Ø10 circle along a quarter arc R50, `area` an area loft
/// of squares 20 → 40 over 100 mm, `direction` two squares with a 60° start,
/// `ring` a closed loop of six circles.
fn loft_guided_part(kind: &str) -> PartDocument {
    use cadcraft3d::kernel::Vector3;
    use cadcraft3d::part::{
        ConditionKind, LineRef, LoftCondition, LoftFeature, LoftGuide, LoftSection, OriginPlane,
        PathRef, PlaneRef, WorkDef, WorkFeature, WorkPlaneDef,
    };
    use cadcraft3d::sketch::Point2;
    let o = SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    };
    let mut part = PartDocument::default();
    let on = |part: &mut PartDocument, plane: PlaneRef| -> usize {
        part.create_sketch(
            SketchPlacement {
                plane,
                ..Default::default()
            },
            &o,
        )
        .unwrap()
    };
    let height = |part: &mut PartDocument, z: Real| -> PlaneRef {
        if z == 0.0 {
            return PlaneRef::Origin(OriginPlane::XY);
        }
        PlaneRef::Work(
            part.add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
                base: PlaneRef::Origin(OriginPlane::XY),
                distance: z,
            })))
            .unwrap(),
        )
    };
    let through_z = |part: &mut PartDocument, theta: Real| -> PlaneRef {
        let want = Vector3::new(theta.cos(), theta.sin(), 0.0);
        if want.y.abs() < 1e-12 {
            return PlaneRef::Origin(OriginPlane::XZ);
        }
        for angle in [theta, -theta] {
            let wp = part
                .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Angle {
                    base: PlaneRef::Origin(OriginPlane::XZ),
                    axis: LineRef::Origin(2),
                    angle,
                })))
                .unwrap();
            if part.work.planes[&wp].normal.dot(&want).abs() < 1e-9 {
                return PlaneRef::Work(wp);
            }
        }
        panic!("no plane");
    };
    let uv = |part: &PartDocument, k: usize, q: Point3| {
        let c = part.sketches[k].sketch.plane.project(&q);
        Point2::new(c.x, c.y)
    };
    // An arc on sketch k from a to b about centre c (model points), CCW
    // or not.
    let arc = |part: &mut PartDocument, k: usize, c: Point3, a: Point3, b: Point3, ccw: bool| {
        let (c, a, b) = (uv(part, k, c), uv(part, k, a), uv(part, k, b));
        let s = &mut part.sketches[k].sketch;
        let (pc, pa, pb) = (s.add_point(c), s.add_point(a), s.add_point(b));
        let e = if ccw {
            s.add_arc(pc, pa, pb)
        } else {
            s.add_arc(pc, pb, pa)
        }
        .unwrap();
        PathRef::Sketch {
            sketch: part.sketches[k].id.clone(),
            curves: vec![e],
        }
    };
    let section = |part: &PartDocument, k: usize| {
        let r = part.sketches[k].sketch.regions();
        LoftSection::Profile {
            sketch: part.sketches[k].id.clone(),
            profile: ProfileRef::of(&r[0], r[0].interior_point()),
        }
    };
    let square = |part: &mut PartDocument, k: usize, w: Real| {
        part.sketches[k]
            .sketch
            .add_rectangle_two_point(
                Point2::new(-w / 2.0, -w / 2.0),
                Point2::new(w / 2.0, w / 2.0),
            )
            .unwrap();
    };
    let def = match kind {
        "rails" => {
            let mut ks = Vec::new();
            for z in [0.0, 100.0] {
                let pl = height(&mut part, z);
                let k = on(&mut part, pl);
                part.sketches[k]
                    .sketch
                    .add_circle(Point2::new(0.0, 0.0), 10.0)
                    .unwrap();
                ks.push(k);
            }
            let xz = on(&mut part, PlaneRef::Origin(OriginPlane::XZ));
            // Arcs through (±10, 0), (±20, 50), (±10, 100): centres (∓110, 50).
            let r1 = arc(
                &mut part,
                xz,
                Point3::new(-110.0, 0.0, 50.0),
                Point3::new(10.0, 0.0, 0.0),
                Point3::new(10.0, 0.0, 100.0),
                true,
            );
            let r2 = arc(
                &mut part,
                xz,
                Point3::new(110.0, 0.0, 50.0),
                Point3::new(-10.0, 0.0, 0.0),
                Point3::new(-10.0, 0.0, 100.0),
                false,
            );
            let mut d = LoftFeature::new(ks.iter().map(|&k| section(&part, k)).collect());
            d.rails = vec![r1, r2];
            d
        }
        "center" => {
            let mut ks = Vec::new();
            for theta in [0.0, std::f64::consts::FRAC_PI_2] {
                let pl = through_z(&mut part, theta);
                let k = on(&mut part, pl);
                let c = uv(
                    &part,
                    k,
                    Point3::new(50.0 * theta.cos(), 50.0 * theta.sin(), 0.0),
                );
                part.sketches[k].sketch.add_circle(c, 5.0).unwrap();
                ks.push(k);
            }
            let xy = on(&mut part, PlaneRef::Origin(OriginPlane::XY));
            let c = arc(
                &mut part,
                xy,
                Point3::origin(),
                Point3::new(50.0, 0.0, 0.0),
                Point3::new(0.0, 50.0, 0.0),
                true,
            );
            let mut d = LoftFeature::new(ks.iter().map(|&k| section(&part, k)).collect());
            d.guide = LoftGuide::CenterLine;
            d.center_line = Some(c);
            d
        }
        "area" | "direction" => {
            let mut ks = Vec::new();
            for (z, w) in [
                (0.0, 20.0),
                (100.0, if kind == "area" { 40.0 } else { 20.0 }),
            ] {
                let pl = height(&mut part, z);
                let k = on(&mut part, pl);
                square(&mut part, k, w);
                ks.push(k);
            }
            let mut d = LoftFeature::new(ks.iter().map(|&k| section(&part, k)).collect());
            if kind == "area" {
                let xz = on(&mut part, PlaneRef::Origin(OriginPlane::XZ));
                let (a, b) = (
                    uv(&part, xz, Point3::origin()),
                    uv(&part, xz, Point3::new(0.0, 0.0, 100.0)),
                );
                let e = part.sketches[xz].sketch.add_line(a, b);
                d.guide = LoftGuide::AreaLoft;
                d.center_line = Some(PathRef::Sketch {
                    sketch: part.sketches[xz].id.clone(),
                    curves: vec![e],
                });
            } else {
                d.conditions = vec![LoftCondition {
                    kind: ConditionKind::Direction,
                    angle: 60f64.to_radians(),
                    weight: 0.5,
                }];
            }
            d
        }
        _ => {
            let mut ks = Vec::new();
            for i in 0..6 {
                let theta = std::f64::consts::TAU * i as Real / 6.0;
                let pl = through_z(&mut part, theta);
                let k = on(&mut part, pl);
                let c = uv(
                    &part,
                    k,
                    Point3::new(40.0 * theta.cos(), 40.0 * theta.sin(), 0.0),
                );
                part.sketches[k].sketch.add_circle(c, 5.0).unwrap();
                ks.push(k);
            }
            let mut d = LoftFeature::new(ks.iter().map(|&k| section(&part, k)).collect());
            d.closed_loop = true;
            d
        }
    };
    part.add_loft(def).unwrap();
    part
}

/// Project Geometry: `n` circles on XY projected (as circles) into a
/// sketch on a work plane 20 above, then (`sweep`) the owner's case: a Ø8
/// circle swept along a spline between two projected centres.
fn links_part(n: usize, sweep: bool) -> PartDocument {
    use cadcraft3d::part::{
        OriginPlane, PathRef, PlaneRef, SketchGeo, SweepFeature, WorkDef, WorkFeature, WorkPlaneDef,
    };
    use cadcraft3d::sketch::{Constraint, Entity, Point2};
    let o = SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    };
    let mut part = PartDocument::default();
    let a = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &o)
        .unwrap();
    for k in 0..n {
        let (x, y) = ((k % 20) as Real * 10.0, (k / 20) as Real * 10.0);
        part.sketches[a]
            .sketch
            .add_circle(Point2::new(x, y), 4.0)
            .unwrap();
    }
    let wp = part
        .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
            base: PlaneRef::Origin(OriginPlane::XY),
            distance: if sweep { 55.0 } else { 20.0 },
        })))
        .unwrap();
    let on_wp = SketchPlacement {
        plane: PlaneRef::Work(wp),
        ..Default::default()
    };
    let b = part.create_sketch(on_wp, &o).unwrap();
    let id = part.sketches[a].id.clone();
    if !sweep {
        let circles: Vec<_> = part.sketches[a].sketch.entities().map(|(e, _)| e).collect();
        for e in circles {
            part.project_sketch_geometry(b, &id, SketchGeo::Curve { entity: e }, false)
                .unwrap();
        }
        part.regenerate().unwrap();
        return part;
    }
    part.sketches[b]
        .sketch
        .add_circle(Point2::new(0.0, 30.0), 4.0)
        .unwrap();
    let c = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::YZ), &o)
        .unwrap();
    let center = |p: &PartDocument, k: usize| {
        p.sketches[k]
            .sketch
            .entities()
            .find_map(|(_, e)| match e {
                Entity::Circle { center, .. } => Some(*center),
                _ => None,
            })
            .unwrap()
    };
    let (ca, cb) = (center(&part, a), center(&part, b));
    let idb = part.sketches[b].id.clone();
    let ka = part
        .project_sketch_geometry(c, &id, SketchGeo::Point { point: ca }, false)
        .unwrap();
    let kb = part
        .project_sketch_geometry(c, &idb, SketchGeo::Point { point: cb }, false)
        .unwrap();
    let (pa, pb) = (
        part.sketches[c].links[ka].points[0],
        part.sketches[c].links[kb].points[0],
    );
    let sk = &mut part.sketches[c].sketch;
    let mid = sk.add_point(Point2::new(4.0, 30.0));
    sk.add_constraint(Constraint::Fix {
        point: mid,
        at: Point2::new(4.0, 30.0),
    })
    .unwrap();
    let spline = sk.add_interp_spline(vec![pa, mid, pb]).unwrap();
    // Only the first circle (n = 1) is the profile.
    let r = part.sketches[a].sketch.regions();
    let pr = vec![ProfileRef::of(&r[0], r[0].interior_point())];
    let path = PathRef::Sketch {
        sketch: part.sketches[c].id.clone(),
        curves: vec![spline],
    };
    part.add_sweep(a, SweepFeature::new(pr, path)).unwrap();
    part
}

#[test]
fn speed_budgets() {
    // (case, budget ms, median ms)
    let mut rows: Vec<(&str, f64, f64)> = Vec::new();

    // Selection: filtering, resolving and ordering 10k candidates under the
    // cursor (Select Other over a dense faceted part) stays far below a frame.
    {
        use cadcraft3d::command::selection::{
            Candidate, Category, Filter, Priority, filter_candidates,
        };
        let cands: Vec<Candidate> = (0..10_000)
            .map(|i| Candidate {
                cat: Category::ALL[i % Category::ALL.len()],
                dist: ((i * 7919) % 1000) as f32,
                surface: i % 5 == 0,
            })
            .collect();
        let mut f = Filter::all();
        f.set(Category::Edge, false);
        rows.push((
            "selection: filter + order 10k candidates (x10)",
            20.0,
            median_ms(5, || {
                for p in [Priority::FacesEdges, Priority::Features] {
                    for _ in 0..5 {
                        let r = filter_candidates(&cands, p, f);
                        assert!(!r.is_empty());
                    }
                }
            }),
        ));
    }

    let rb = rounded_box();
    rows.push((
        "regenerate the rounded box (box, 8 fillets, chamfer chain)",
        400.0,
        median_ms(3, || {
            let mut p = rb.clone();
            p.build_cache = Default::default(); // measure the build, not the cache
            p.regenerate().unwrap();
            assert!(p.exact);
        }),
    ));

    rows.push((
        "build the hollow box (box, sketch on face, cut)",
        50.0,
        median_ms(3, || {
            let _ = hollow();
        }),
    ));

    let h = hollow();
    let e = below_rim(&h);
    rows.push((
        "fillet r2 of the hollow box's 20 edges below the rim",
        600.0,
        median_ms(3, || {
            let mut p = h.clone();
            p.build_cache = Default::default();
            p.add_blend(BlendFeature::fillet(e.clone(), 2.0)).unwrap();
        }),
    ));
    rows.push((
        "chamfer 2 of the same edges (wedge fallback)",
        1200.0,
        median_ms(3, || {
            let mut p = h.clone();
            p.build_cache = Default::default();
            p.add_blend(BlendFeature::chamfer(e.clone(), 2.0)).unwrap();
        }),
    ));

    // Select Tangencies (double click) on the filleted pocket: every inner
    // face (walls, rounds, bottom).
    let mut filleted = h.clone();
    filleted
        .add_blend(BlendFeature::fillet(e.clone(), 2.0))
        .unwrap();
    let wall = filleted
        .body
        .faces()
        .find(|(_, f)| {
            f.name
                .as_deref()
                .is_some_and(|n| n.starts_with("Extrusion1:Side"))
        })
        .map(|(id, _)| id)
        .unwrap();
    rows.push((
        "Select Tangencies on the filleted pocket's wall",
        50.0,
        median_ms(3, || {
            let faces = cadcraft3d::part::select::tangent_faces(&filleted.body, wall);
            assert!(faces.len() > 10, "{}", faces.len());
        }),
    ));

    // File > Export of the filleted hollow box: STL, STEP, GLB.
    for f in cadcraft3d::part::export::ExportFormat::ALL {
        rows.push((
            match f {
                cadcraft3d::part::export::ExportFormat::Stl => {
                    "export STL of the filleted hollow box"
                }
                cadcraft3d::part::export::ExportFormat::Step => {
                    "export STEP of the filleted hollow box"
                }
                cadcraft3d::part::export::ExportFormat::Glb => {
                    "export GLB of the filleted hollow box"
                }
            },
            200.0,
            median_ms(3, || {
                let b = cadcraft3d::part::export::export_bytes(&filleted, "Hollow", f).unwrap();
                assert!(b.len() > 1000);
            }),
        ));
    }

    // Materials: Physical Properties (mass, COG, inertia) and the face
    // appearances the viewport and the exports resolve.
    {
        let mut styled = filleted.clone();
        let lib = cadcraft3d::materials::builtin();
        styled.set_material(Some("Steel S235"), lib).unwrap();
        styled
            .set_appearance(
                &cadcraft3d::part::appearance::LookTarget::Feature("Extrusion1".into()),
                Some("ABS - Red"),
                lib,
            )
            .unwrap();
        rows.push((
            "Physical Properties of the filleted hollow box (Low)",
            100.0,
            median_ms(3, || {
                let p = cadcraft3d::part::physical::physical_properties(
                    &styled,
                    cadcraft3d::part::physical::Accuracy::Low,
                )
                .unwrap();
                assert!(p.total.mass > 0.0);
            }),
        ));
        rows.push((
            "resolve every face's appearance of the filleted hollow box",
            20.0,
            median_ms(3, || {
                let looks = cadcraft3d::part::appearance::Resolver::new(&styled).face_looks();
                assert!(looks.len() > 10);
            }),
        ));
    }

    // Select Mode Solid (All Rounds): every edge of the filleted pocket
    // classified convex / concave.
    rows.push((
        "All Rounds: classify the filleted hollow box's edges",
        100.0,
        median_ms(3, || {
            let r = cadcraft3d::part::select::solid_edges(
                &filleted.body,
                None,
                cadcraft3d::part::select::SolidEdges::Rounds,
            );
            assert!(!r.is_empty());
        }),
    ));

    // A sketch pattern dialog rebuilds its preview on a sketch copy; a
    // 10 × 10 grid of a rectangle must stay interactive.
    let mut grid = cadcraft3d::sketch::Sketch::default();
    let axis_x = grid.add_line(
        cadcraft3d::sketch::Point2::new(0.0, -5.0),
        cadcraft3d::sketch::Point2::new(10.0, -5.0),
    );
    let axis_y = grid.add_line(
        cadcraft3d::sketch::Point2::new(-5.0, 0.0),
        cadcraft3d::sketch::Point2::new(-5.0, 10.0),
    );
    let rect = grid
        .add_rectangle_two_point(
            cadcraft3d::sketch::Point2::new(0.0, 0.0),
            cadcraft3d::sketch::Point2::new(4.0, 6.0),
        )
        .unwrap();
    let dir = |line, spacing| cadcraft3d::sketch::PatternDirection {
        line,
        flip: false,
        count: 10,
        spacing,
        fitted: false,
    };
    rows.push((
        "sketch pattern preview: 10 x 10 rectangles",
        150.0,
        median_ms(3, || {
            let mut sk = grid.clone();
            sk.rectangular_pattern(&rect.entities, dir(axis_x, 8.0), Some(dir(axis_y, 10.0)))
                .unwrap();
            sk.solve().unwrap();
            assert_eq!(sk.patterns().next().unwrap().1.instances.len(), 99);
        }),
    ));

    let mut solved = grid.clone();
    solved
        .rectangular_pattern(&rect.entities, dir(axis_x, 8.0), Some(dir(axis_y, 10.0)))
        .unwrap();
    solved.solve().unwrap();
    rows.push((
        "sketch DOF analysis (glyphs, colours) of the 10 x 10 pattern",
        100.0,
        median_ms(3, || {
            let _ = solved.dof_analysis();
        }),
    ));

    // Dragging: a corner of the original moves, all 99 copies follow.
    let corner = rect.points[2];
    rows.push((
        "sketch drag step in the 10 x 10 pattern (solve after a move)",
        50.0,
        median_ms(3, || {
            let mut sk = solved.clone();
            let q = sk.point(corner).unwrap().position;
            sk.move_point(corner, q + cadcraft3d::sketch::Vector2::new(0.5, 0.3))
                .unwrap();
            sk.solve().unwrap();
        }),
    ));

    // Shell of the rounded box (8 fillets and the chamfer chain before
    // it), the chamfered top removed: walls follow the rounds.
    rows.push((
        "shell 1.5 of the rounded box, top removed (add_shell)",
        600.0,
        median_ms(3, || {
            let mut p = rb.clone();
            p.build_cache = Default::default();
            let top = p.body.find_face("Box1:Top").unwrap();
            let mut def = cadcraft3d::part::ShellFeature::new(1.5);
            def.remove
                .push(cadcraft3d::part::shell::face_ref(&p.body, top, None).unwrap());
            p.add_shell(def).unwrap();
        }),
    ));

    // Face Draft of a box's sides and vertical rounds (one tangent chain
    // from the front), fixed plane XY.
    let mut vertical_rounds = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
    let corner = |x: Real, w: Real| x.abs() < 1e-6 || (x - w).abs() < 1e-6;
    let vertical: Vec<EdgeRef> = vertical_rounds
        .body
        .edges()
        .filter_map(|(e, _)| edge_ref(&vertical_rounds.body, e))
        .filter(|r| corner(r.hint[0], 40.0) && corner(r.hint[1], 30.0))
        .collect();
    vertical_rounds
        .add_blend(BlendFeature::fillet(vertical, 5.0))
        .unwrap();
    rows.push((
        "face draft 3° of a box's sides and vertical rounds (add_draft)",
        200.0,
        median_ms(3, || {
            let mut p = vertical_rounds.clone();
            p.build_cache = Default::default();
            let mut def = cadcraft3d::part::DraftFeature::new(3f64.to_radians());
            def.draft_type = cadcraft3d::part::DraftType::FixedPlane;
            def.plane = Some(cadcraft3d::part::PlaneRef::Origin(
                cadcraft3d::part::OriginPlane::XY,
            ));
            let front = p.body.find_face("Box1:Front").unwrap();
            let body = p.body.clone();
            def.toggle(&body, front, None);
            p.add_draft(def).unwrap();
        }),
    ));

    // Symmetric Face Draft (split by the plane, both halves drafted, joined
    // again) of the same box's sides and rounds about z = 10.
    let mut mid = vertical_rounds.clone();
    let wp = mid
        .add_work(cadcraft3d::part::WorkFeature::new(
            cadcraft3d::part::WorkDef::Plane(cadcraft3d::part::WorkPlaneDef::Offset {
                base: cadcraft3d::part::PlaneRef::Origin(cadcraft3d::part::OriginPlane::XY),
                distance: 10.0,
            }),
        ))
        .unwrap();
    rows.push((
        "face draft 3° symmetric about z = 10, sides and rounds",
        BUDGET_SYMMETRIC_DRAFT,
        median_ms(3, || {
            let mut p = mid.clone();
            p.build_cache = Default::default();
            let mut def = cadcraft3d::part::DraftFeature::new(3f64.to_radians());
            def.draft_type = cadcraft3d::part::DraftType::FixedPlane;
            def.plane = Some(cadcraft3d::part::PlaneRef::Work(wp.clone()));
            def.extent = cadcraft3d::part::DraftExtent::Symmetric;
            let front = p.body.find_face("Box1:Front").unwrap();
            let body = p.body.clone();
            def.toggle(&body, front, None);
            p.add_draft(def).unwrap();
        }),
    ));

    // Combine: a box and a cylinder sunk 10 into it as a second body, cut;
    // Split Solid of a filleted box by a plane at an angle.
    let two_bodies = {
        let mut part = PartDocument::with_box("Box1", [60.0, 40.0, 20.0]).unwrap();
        let opts = SketchOptions {
            auto_project_origin: false,
            auto_reference_edges: false,
            ..Default::default()
        };
        let j = part
            .create_sketch(SketchPlacement::on_face("Box1:Top"), &opts)
            .unwrap();
        let c = part.sketches[j]
            .sketch
            .plane
            .project(&Point3::new(30.0, 20.0, 20.0));
        part.sketches[j].sketch.add_circle(c, 8.0).unwrap();
        let rg = part.sketches[j].sketch.regions();
        let mut ex =
            ExtrudeFeature::new(vec![ProfileRef::of(&rg[0], rg[0].interior_point())], 10.0);
        ex.direction = ExtentDirection::Flipped;
        let tool = part.add_extrude(j, ex).unwrap();
        (part, tool)
    };
    rows.push((
        "combine: cut a cylinder body out of a box body (add_combine)",
        BUDGET_COMBINE,
        median_ms(3, || {
            let mut p = two_bodies.0.clone();
            p.build_cache = Default::default();
            let mut c = cadcraft3d::part::CombineFeature::new();
            c.base = "Box1".into();
            c.tools = vec![two_bodies.1.clone()];
            c.operation = Operation::Cut;
            p.add_combine(c).unwrap();
        }),
    ));
    let mut tilted = vertical_rounds.clone();
    let tw = tilted
        .add_work(cadcraft3d::part::WorkFeature::new(
            cadcraft3d::part::WorkDef::Plane(cadcraft3d::part::WorkPlaneDef::Angle {
                base: cadcraft3d::part::PlaneRef::Origin(cadcraft3d::part::OriginPlane::YZ),
                axis: cadcraft3d::part::LineRef::Origin(1),
                angle: 0.3,
            }),
        ))
        .unwrap();
    rows.push((
        "split solid of a box with rounds by a plane at an angle (add_split)",
        BUDGET_SPLIT_INCLINED,
        median_ms(3, || {
            let mut p = tilted.clone();
            p.build_cache = Default::default();
            let mut s =
                cadcraft3d::part::SplitFeature::new(cadcraft3d::part::SplitMethod::SplitSolid);
            s.tool = Some(cadcraft3d::part::SplitToolRef::Plane(
                cadcraft3d::part::PlaneRef::Work(tw.clone()),
            ));
            p.add_split(s).unwrap();
        }),
    ));
    rows.push((
        "split face: all faces of a box with rounds by a plane (add_split)",
        BUDGET_SPLIT,
        median_ms(3, || {
            let mut p = mid.clone();
            p.build_cache = Default::default();
            let mut s =
                cadcraft3d::part::SplitFeature::new(cadcraft3d::part::SplitMethod::SplitFace);
            s.tool = Some(cadcraft3d::part::SplitToolRef::Plane(
                cadcraft3d::part::PlaneRef::Work(wp.clone()),
            ));
            s.all_faces = true;
            p.add_split(s).unwrap();
        }),
    ));

    // Split Solid of a box by an extruded arc surface, and by an open
    // spline (prism of its front side).
    let (by_surface, by_spline) = {
        use cadcraft3d::sketch::Point2;
        let opts = SketchOptions {
            auto_project_origin: false,
            auto_reference_edges: false,
            ..Default::default()
        };
        let mut part = PartDocument::with_box("Box1", [60.0, 40.0, 20.0]).unwrap();
        let j = part
            .create_sketch(
                SketchPlacement::on_origin_plane(cadcraft3d::part::OriginPlane::XY),
                &opts,
            )
            .unwrap();
        let sk = &mut part.sketches[j].sketch;
        let c = sk.add_point(Point2::new(30.0, -60.0));
        let a = sk.add_point(Point2::new(
            30.0 + 90.0 * 1.0f64.cos(),
            -60.0 + 90.0 * 1.0f64.sin(),
        ));
        let b = sk.add_point(Point2::new(
            30.0 + 90.0 * 2.1f64.cos(),
            -60.0 + 90.0 * 2.1f64.sin(),
        ));
        let arc = sk.add_arc(c, a, b).unwrap();
        let mut ex = ExtrudeFeature::new(Vec::new(), 60.0);
        ex.direction = ExtentDirection::Symmetric;
        ex.output = cadcraft3d::part::Output::Surface;
        ex.operation = Operation::NewSolid;
        ex.open = Some(cadcraft3d::part::OpenProfile {
            curves: vec![arc],
            flip: false,
            match_shape: false,
        });
        let srf = part.add_extrude(j, ex).unwrap();
        let mut s = PartDocument::with_box("Box1", [60.0, 40.0, 20.0]).unwrap();
        let k = s
            .create_sketch(
                SketchPlacement::on_origin_plane(cadcraft3d::part::OriginPlane::XY),
                &opts,
            )
            .unwrap();
        let sk = &mut s.sketches[k].sketch;
        let ids = [(-10.0, 5.0), (20.0, 5.0), (40.0, 35.0), (70.0, 35.0)]
            .iter()
            .map(|q| sk.add_point(Point2::new(q.0, q.1)))
            .collect();
        sk.add_spline(ids).unwrap();
        let sid = s.sketches[k].id.clone();
        ((part, srf), (s, sid))
    };
    rows.push((
        "split solid of a box by an extruded arc surface (add_split)",
        BUDGET_SPLIT,
        median_ms(3, || {
            let mut p = by_surface.0.clone();
            p.build_cache = Default::default();
            let mut s =
                cadcraft3d::part::SplitFeature::new(cadcraft3d::part::SplitMethod::SplitSolid);
            s.tool = Some(cadcraft3d::part::SplitToolRef::Surface(
                by_surface.1.clone(),
            ));
            p.add_split(s).unwrap();
        }),
    ));
    rows.push((
        "split solid of a box by an open spline (add_split)",
        BUDGET_SPLIT,
        median_ms(3, || {
            let mut p = by_spline.0.clone();
            p.build_cache = Default::default();
            let mut s =
                cadcraft3d::part::SplitFeature::new(cadcraft3d::part::SplitMethod::SplitSolid);
            s.tool = Some(cadcraft3d::part::SplitToolRef::Sketch(by_spline.1.clone()));
            p.add_split(s).unwrap();
        }),
    ));

    // Shell with Rounded Corners (outside: the offset's convex edges
    // rounded) of a box open at the top.
    rows.push((
        "shell 2 outside, rounded corners, box open at the top",
        BUDGET_ROUNDED_SHELL,
        median_ms(3, || {
            let mut p = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
            let top = p.body.find_face("Box1:Top").unwrap();
            let mut def = cadcraft3d::part::ShellFeature::new(2.0);
            def.direction = cadcraft3d::part::ShellDirection::Outside;
            def.corners = cadcraft3d::part::ShellCorners::Rounded;
            def.remove
                .push(cadcraft3d::part::shell::face_ref(&p.body, top, None).unwrap());
            p.add_shell(def).unwrap();
        }),
    ));

    // Patterns: the whole part is rebuilt (empty cache), so the time is
    // the box, the source feature and the pattern.
    let holed = box_with_circle([60.0, 60.0, 6.0], 5.0, 5.0, 2.0, true, 1.0);
    rows.push((
        "rectangular pattern 6×6 of a hole on a plate (add_pattern)",
        2500.0,
        median_ms(3, || {
            let mut p = holed.clone();
            p.build_cache = Default::default();
            p.add_pattern(pattern_grid(6, 6, 10.0)).unwrap();
        }),
    ));
    let bossed = box_with_circle([200.0, 200.0, 5.0], 10.0, 10.0, 1.5, false, 3.0);
    rows.push((
        "rectangular pattern 10×10 of a joined boss (SolidCraft #2076)",
        3000.0,
        median_ms(3, || {
            let mut p = bossed.clone();
            p.build_cache = Default::default();
            p.add_pattern(pattern_grid(10, 10, 20.0)).unwrap();
        }),
    ));
    // Large patterns (docs/PERF.md, "Масиви с хиляди копия"): the whole part
    // is rebuilt (empty cache; the plate and its hole take milliseconds),
    // plus the preview's edge polylines (what the dialog waits for; OK
    // reuses the preview's part).
    for (n, budget) in [(32u32, 3000.0), (71, 15000.0)] {
        let side = 5.0 * n as Real;
        let base = box_with_circle([side, side, 3.0], 2.5, 2.5, 1.0, true, 1.0);
        let runs = if n > 50 { 1 } else { 3 };
        rows.push((
            if n == 32 {
                "rectangular pattern 32×32 = 1024 holes + preview edges"
            } else {
                "rectangular pattern 71×71 = 5041 holes + preview edges"
            },
            budget,
            median_ms(runs, || {
                let mut p = base.clone();
                p.build_cache = Default::default();
                let id = p.add_pattern(pattern_grid(n, n, 5.0)).unwrap();
                let o = cadcraft3d::kernel::TessellationOptions::default();
                let lines = p
                    .body
                    .feature_edges(&id)
                    .into_iter()
                    .filter_map(|e| p.body.sample_edge(e, &o).ok())
                    .count();
                assert!(p.exact && lines >= 6 * (n * n) as usize - 6);
            }),
        ));
    }
    let mut patterned = holed.clone();
    patterned.add_pattern(pattern_grid(6, 6, 10.0)).unwrap();
    rows.push((
        "mirror the 36-hole plate as a solid, Join across its right face",
        1500.0,
        {
            use cadcraft3d::part::{PatternFeature, PatternKind, PlaneRef};
            // The steps before the mirror come from a fresh cache filled
            // outside the timing: only the mirror is measured.
            let mut t: Vec<f64> = (0..3)
                .map(|_| {
                    let mut p = patterned.clone();
                    p.build_cache = Default::default();
                    p.regenerate().unwrap();
                    let mut m = PatternFeature::mirror();
                    m.solid_mode = true;
                    m.kind = PatternKind::Mirror {
                        plane: Some(PlaneRef::Face("Box1:Right".into())),
                    };
                    let t0 = Instant::now();
                    p.add_pattern(m).unwrap();
                    t0.elapsed().as_secs_f64() * 1000.0
                })
                .collect();
            t.sort_by(f64::total_cmp);
            t[1]
        },
    ));
    // The Preset row's model work in one frame (list with built-ins and the
    // asterisk check), 100 frames.
    {
        use cadcraft3d::presets::{CommandPresets, PresetData};
        let mut presets = CommandPresets::new("Fillet");
        for k in 0..20 {
            presets
                .create(
                    &format!("R{k}"),
                    BlendFeature::fillet(Vec::new(), k as Real).capture(),
                )
                .unwrap();
        }
        let def = BlendFeature::fillet(Vec::new(), 3.0);
        rows.push((
            "preset row: 100 frames of list + modified check (20 presets)",
            30.0,
            median_ms(3, || {
                for _ in 0..100 {
                    let data = presets.clone();
                    let list = data.list();
                    let p = data.find("R7").unwrap();
                    assert!(list.len() > 20);
                    assert!(def.differs_from(&p.settings));
                }
            }),
        ));
    }
    // Hole: 36 counterbored M5 holes in one feature (two Booleans: the seats,
    // then the bores; a stepped tool per hole took 43 s), and one
    // modeled M8 thread 10 deep (faceted zone only).
    let plate = PartDocument::with_box("Box1", [100.0, 100.0, 10.0]).unwrap();
    rows.push((
        "hole: 36 counterbored Ø5.5 holes in one feature (add_hole)",
        2500.0,
        median_ms(3, || {
            use cadcraft3d::part::{HoleCenter, HoleFeature, HoleSpec, SeatType, Termination};
            let mut p = plate.clone();
            p.build_cache = Default::default();
            let pts: Vec<Point3> = (0..36)
                .map(|i| {
                    Point3::new(
                        10.0 + 16.0 * (i % 6) as Real,
                        10.0 + 16.0 * (i / 6) as Real,
                        10.0,
                    )
                })
                .collect();
            let (k, ids) = p.create_hole_sketch("Box1:Top", &pts).unwrap();
            let mut s = HoleSpec::simple(5.5, 10.0);
            s.termination = Termination::ThroughAll;
            s.seat = SeatType::Counterbore;
            s.seat_diameter = 10.0;
            s.seat_depth = 5.4;
            let mut def = HoleFeature::new(s);
            def.sketch = p.sketches[k].id.clone();
            def.centers = ids.into_iter().map(HoleCenter::Point).collect();
            p.add_hole(def).unwrap();
        }),
    ));
    rows.push((
        "hole: tapped M8 modeled thread 10 deep (add_hole)",
        12000.0,
        median_ms(1, || {
            use cadcraft3d::part::standards::{self, Standards};
            use cadcraft3d::part::{HoleCenter, HoleFeature, HoleSpec, ThreadSpec};
            let mut p = PartDocument::with_box("Box1", [30.0, 30.0, 20.0]).unwrap();
            p.build_cache = Default::default();
            let (k, ids) = p
                .create_hole_sketch("Box1:Top", &[Point3::new(15.0, 15.0, 20.0)])
                .unwrap();
            let mut t =
                ThreadSpec::from_table(&Standards::builtin(), standards::ISO_METRIC, "M8", true)
                    .unwrap();
            t.modeled = true;
            t.full_depth = false;
            t.depth = 10.0;
            let mut def = HoleFeature::new(HoleSpec::tapped(t, 15.0));
            def.sketch = p.sketches[k].id.clone();
            def.centers = ids.into_iter().map(HoleCenter::Point).collect();
            p.add_hole(def).unwrap();
        }),
    ));
    // Thread texture (2.9): the display mesh of 100 tapped M8 holes (a 10×10
    // grid in one feature), built once per scene rebuild.
    {
        use cadcraft3d::part::standards::{self, Standards};
        use cadcraft3d::part::{HoleCenter, HoleFeature, HoleSpec, ThreadBands, ThreadSpec};
        let mut p = PartDocument::with_box("Box1", [200.0, 200.0, 20.0]).unwrap();
        let pts: Vec<Point3> = (0..100)
            .map(|i| {
                Point3::new(
                    10.0 + 20.0 * (i % 10) as Real,
                    10.0 + 20.0 * (i / 10) as Real,
                    20.0,
                )
            })
            .collect();
        let (k, ids) = p.create_hole_sketch("Box1:Top", &pts).unwrap();
        let t = ThreadSpec::from_table(&Standards::builtin(), standards::ISO_METRIC, "M8", true)
            .unwrap();
        let mut def = HoleFeature::new(HoleSpec::tapped(t, 15.0));
        def.sketch = p.sketches[k].id.clone();
        def.centers = ids.into_iter().map(HoleCenter::Point).collect();
        p.add_hole(def).unwrap();
        assert_eq!(p.cosmetic_threads.len(), 100);
        rows.push((
            "thread texture: mesh of 100 tapped M8 holes (ThreadBands::of)",
            5.0,
            median_ms(5, || {
                let b = ThreadBands::of(&p.cosmetic_threads);
                assert!(b.triangles.len() >= 100 * 32);
            }),
        ));
    }
    // The owner's session (`tests/data/m8_modeled_blind_journal.jsonl`): a
    // tapped M8 10 deep (= the thickness, 118° point) in a 283 × 136 × 10
    // plate. The plain hole took 6 s (a cap on the far face, the point in
    // the air), its modeled thread failed after 150 s (release).
    {
        use cadcraft3d::diagnostics::replay;
        use cadcraft3d::part::Feature;
        let owner = replay::part_at(
            std::path::Path::new("tests/data/m8_modeled_blind_journal.jsonl"),
            8,
        )
        .unwrap();
        let k = owner
            .features
            .iter()
            .position(|f| f.id() == "Hole1")
            .unwrap();
        rows.push((
            "hole: owner's M8 10 deep through a 283 mm plate (regenerate)",
            500.0,
            median_ms(3, || {
                let mut p = owner.clone();
                p.build_cache = Default::default();
                p.regenerate().unwrap();
                assert!(p.exact && p.feature_health.is_empty());
            }),
        ));
        rows.push((
            "hole: owner's M8 made modeled (replace_feature, slabs)",
            8000.0,
            median_ms(1, || {
                let mut p = owner.clone();
                p.build_cache = Default::default();
                let Feature::Hole(mut h) = p.features[k].clone() else {
                    panic!("Hole1 is not a hole")
                };
                h.spec.thread.as_mut().unwrap().modeled = true;
                p.replace_feature(k, Feature::Hole(h)).unwrap();
                assert!(p.exact && p.feature_health.is_empty());
            }),
        ));
    }

    // Extrude / Revolve options (2027): a Box primitive, a Cylinder
    // primitive cut To an inclined work plane, a Sphere primitive joined.
    rows.push((
        "primitives: box, cylinder cut to an inclined plane, sphere join",
        4000.0,
        median_ms(3, || {
            use cadcraft3d::part::primitive::add_primitive;
            use cadcraft3d::part::{
                Extent, ExtrudeFeature, LineRef, Operation, OriginPlane, PlaneRef,
                PrimitiveFeature, PrimitiveKind, SketchPlacement, TargetRef, WorkDef, WorkFeature,
                WorkPlaneDef,
            };
            use cadcraft3d::sketch::Point2;
            let mut p = PartDocument::default();
            let xy = || SketchPlacement::on_origin_plane(OriginPlane::XY);
            let f = PrimitiveFeature::extrude(&p, 40.0);
            add_primitive(
                &mut p,
                PrimitiveKind::Box,
                xy(),
                &[Point2::new(40.0, 30.0), Point2::new(80.0, 60.0)],
                f,
            )
            .unwrap();
            let wp = p
                .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Angle {
                    base: PlaneRef::Origin(OriginPlane::XY),
                    axis: LineRef::Origin(1),
                    angle: 0.3,
                })))
                .unwrap();
            let top = p
                .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
                    base: PlaneRef::Origin(OriginPlane::XY),
                    distance: 40.0,
                })))
                .unwrap();
            let on_top = || SketchPlacement {
                plane: PlaneRef::Work(top.clone()),
                ..Default::default()
            };
            let mut e = ExtrudeFeature::new(Vec::new(), 1.0);
            e.extent = Extent::To(TargetRef::Plane(PlaneRef::Work(wp)));
            e.operation = Operation::Cut;
            add_primitive(
                &mut p,
                PrimitiveKind::Cylinder,
                on_top(),
                &[Point2::new(40.0, 30.0), Point2::new(50.0, 30.0)],
                PrimitiveFeature::Extrude(e),
            )
            .unwrap();
            add_primitive(
                &mut p,
                PrimitiveKind::Sphere,
                on_top(),
                &[Point2::new(65.0, 45.0), Point2::new(70.0, 45.0)],
                PrimitiveFeature::Revolve(Operation::Join),
            )
            .unwrap();
        }),
    ));

    // Diagnostics: 1000 user actions journaled (a checkpoint line and the
    // part's state: a delta snapshot + volume / faces / health) on the
    // rounded box.
    let dir = tempfile::tempdir().unwrap();
    let mut jp = rb.clone();
    rows.push((
        "journal 1000 actions (checkpoint + state delta, rounded box)",
        1000.0,
        median_ms(1, || {
            use cadcraft3d::diagnostics::journal::Journal;
            let mut j = Journal::create(&dir.path().join("perf-journal.jsonl")).unwrap();
            for k in 0..1000 {
                j.entry("checkpoint", "Edit", serde_json::json!({}));
                if let Some(cadcraft3d::part::Feature::Box(b)) = jp.features.first_mut() {
                    b.size[0] = 40.0 + (k % 7) as Real;
                }
                j.state("Edit", &mut jp);
            }
        }),
    ));
    // History editing (2.16): the last of 30 features moved to the top,
    // so the 28 steps after it are rebuilt (the dependency check runs over
    // all 30); then the End of Part up and back (cache hits).
    let p30 = blocks_30();
    assert_eq!(p30.features.len(), 30);
    rows.push((
        "reorder: feature 30 of 30 to the top + regenerate",
        4000.0,
        // Once: clones share the cache, a second run would hit it.
        median_ms(1, || {
            let mut p = p30.clone();
            p.move_feature(29, 1).unwrap();
            assert!(p.feature_health.is_empty() && p.exact);
        }),
    ));
    rows.push((
        "End of Part: to 15 of 30 and back to the end (cache warm)",
        300.0,
        median_ms(3, || {
            let mut p = p30.clone();
            p.set_eop(15).unwrap();
            p.set_eop(30).unwrap();
        }),
    ));

    // 30 work features on a box: 10 offset planes, 10 axes (plane x
    // origin plane), 10 points (axis x plane), each built on the previous.
    let mut wf = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
    {
        use cadcraft3d::part::{
            LineRef, OriginPlane, PlaneRef, WorkAxisDef, WorkDef, WorkFeature, WorkPlaneDef,
            WorkPointDef,
        };
        let mut defs = Vec::new();
        for k in 0..10 {
            defs.push(WorkDef::Plane(WorkPlaneDef::Offset {
                base: PlaneRef::Face("Box1:Top".into()),
                distance: 2.0 + k as Real,
            }));
            defs.push(WorkDef::Axis(WorkAxisDef::TwoPlanes {
                a: PlaneRef::Work(format!("WorkPlane{}", k + 1)),
                b: PlaneRef::Origin(OriginPlane::XZ),
            }));
            defs.push(WorkDef::Point(WorkPointDef::PlaneLine {
                plane: PlaneRef::Origin(OriginPlane::YZ),
                line: LineRef::Work(format!("WorkAxis{}", k + 1)),
            }));
        }
        for d in defs {
            wf.features
                .push(cadcraft3d::part::Feature::Work(WorkFeature::new(d)));
        }
        // Ids as add_work gives them.
        let mut counts = [0; 3];
        for f in wf.features.iter_mut() {
            if let cadcraft3d::part::Feature::Work(w) = f {
                let (idp, namep) = w.def.prefix();
                let slot = match idp {
                    "WorkPlane" => 0,
                    "WorkAxis" => 1,
                    _ => 2,
                };
                counts[slot] += 1;
                w.id = format!("{idp}{}", counts[slot]);
                w.name = format!("{namep}{}", counts[slot]);
            }
        }
        wf.regenerate().unwrap();
        assert!(wf.feature_health.is_empty(), "{:?}", wf.feature_health);
        assert_eq!(wf.work.points.len(), 10);
    }
    rows.push((
        "regenerate a box with 30 work features",
        20.0,
        median_ms(5, || {
            let mut p = wf.clone();
            p.build_cache = Default::default();
            p.regenerate().unwrap();
        }),
    ));

    let links = links_part(200, false);
    assert_eq!(links.sketches[1].links.len(), 200);
    rows.push((
        "Project Geometry: regenerate 200 circles projected from a sketch",
        10.0,
        median_ms(5, || {
            let mut p = links.clone();
            p.build_cache = Default::default();
            p.regenerate().unwrap();
            assert_eq!(p.sketches[1].health, None);
        }),
    ));

    for (case, budget, part) in [
        (
            "sweep a circle along a spline between projected centres",
            400.0,
            links_part(1, true),
        ),
        (
            "sweep a circle along a quarter arc (exact sweep)",
            150.0,
            sweep_part(0.0, false),
        ),
        (
            "sweep a square 40 with a 90 deg twist (3 lofted pieces)",
            400.0,
            sweep_part(90.0, true),
        ),
        (
            "sweep a circle R17.7 along a spline (owner's session)",
            300.0,
            sweep_spline_part(),
        ),
        (
            "loft square -> circle (2 sections, ruled)",
            60.0,
            loft_part(2),
        ),
        ("loft through 4 sections (smooth)", 100.0, loft_part(4)),
        (
            "loft with 2 arc rails (Ø20 bottle, 21 stations)",
            120.0,
            loft_guided_part("rails"),
        ),
        (
            "loft along a quarter-arc centre line (Ø10)",
            120.0,
            loft_guided_part("center"),
        ),
        (
            "area loft, squares 20 -> 40 over 100 mm",
            60.0,
            loft_guided_part("area"),
        ),
        (
            "loft with a 60 deg Direction condition",
            120.0,
            loft_guided_part("direction"),
        ),
        (
            "closed-loop loft of 6 circles (ring)",
            250.0,
            loft_guided_part("ring"),
        ),
        (
            "coil: Ø2 spring, 5 turns (exact helical sweep)",
            1000.0,
            coil_part(5.0, false, false),
        ),
        (
            "coil: Ø2 spring, 5 turns, closed ends (sweep along the path)",
            1000.0,
            coil_part(5.0, true, false),
        ),
        (
            "coil: thread cut 6 turns into a Ø20 rod (faceted zone)",
            12000.0,
            coil_part(6.0, false, true),
        ),
        (
            "rib: network of 3 webs in an open box",
            1500.0,
            rib_part(true),
        ),
        (
            "rib: web with 2 bosses in an open box",
            1500.0,
            rib_part(false),
        ),
    ] {
        rows.push((
            case,
            budget,
            median_ms(3, || {
                let mut p = part.clone();
                p.build_cache = Default::default();
                p.regenerate().unwrap();
                assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
            }),
        ));
    }

    println!("{:<62} {:>9} {:>9}", "case", "budget", "median");
    let mut over = Vec::new();
    for (case, budget, ms) in &rows {
        let flag = if ms > budget { "  OVER" } else { "" };
        println!("{case:<62} {budget:>7.0} ms {ms:>7.1} ms{flag}");
        if ms > budget {
            over.push(format!("{case}: {ms:.0} ms > {budget:.0} ms"));
        }
    }
    assert!(over.is_empty(), "over budget:\n{}", over.join("\n"));
}

/// Alias input / command search (spec 0.15 §5.1, §5.3): ranking 300
/// commands on every keystroke stays under 1 ms (the budget of a frame's
/// share; `docs/PERF.md`).
#[test]
fn command_search_per_keystroke() {
    use cadcraft3d::command::matcher::{Fields, Folded, Query, rank};
    let words = [
        "Extrude",
        "Revolve",
        "Sweep",
        "Loft",
        "Point",
        "Center",
        "Two",
        "Three",
        "Arc",
        "Line",
        "Rectangle",
        "Pattern",
        "Circular",
        "Work",
        "Plane",
        "Axis",
        "Slot",
        "Spline",
        "Fillet",
        "Chamfer",
    ];
    let bg = [
        "Изтегляне",
        "Завъртане",
        "Точка",
        "Масив",
        "Равнина",
        "Дъга",
    ];
    let cmds: Vec<Folded> = (0..300)
        .map(|k| {
            let name = format!(
                "{} {} {}",
                words[k % 20],
                words[(k / 20) % 20],
                words[(k * 7) % 20]
            );
            let b = format!("{} {}", bg[k % 6], bg[(k / 6) % 6]);
            let alias = format!("{}{}", &words[k % 20][..1], k % 7);
            let tip =
                format!("{name}: pick the geometry, then set the options in Properties ({b})");
            Folded::new(&Fields {
                aliases: &[alias.as_str()],
                names: &[name.as_str(), b.as_str()],
                tip: &tip,
            })
        })
        .collect();
    let keys = [
        "e", "ex", "ext", "extr", "extru", "и", "из", "изт", "two p", "rect*", "c?rc", "zzz",
    ];
    let ms = median_ms(5, || {
        let mut hits = 0;
        for q in keys {
            let q = Query::new(q);
            let recent = [3usize, 17, 42];
            let mut r: Vec<_> = cmds
                .iter()
                .enumerate()
                .filter_map(|(k, c)| rank(&q, c, recent.iter().position(|&x| x == k)))
                .collect();
            r.sort_unstable();
            hits += r.len();
        }
        assert!(hits > 0);
    }) / keys.len() as f64;
    println!("command search: {ms:.3} ms per keystroke (300 commands)");
    assert!(ms < 1.0, "command search: {ms:.3} ms per keystroke > 1 ms");
}
