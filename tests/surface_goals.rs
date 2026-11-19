//! Three plastic products built step by step through the part API (spec 5.10): an oil bottle,
//! a hair dryer and a car bumper. Each step reports what it did instead of failing the run, so
//! the table shows which tools hold and which do not:
//! `cargo test --no-default-features --test surface_goals -- --ignored --nocapture --test-threads=1`

mod common;

use std::sync::mpsc;
use std::time::{Duration, Instant};

use cadcraft3d::part::emboss::{EmbossKind, EmbossOp, EmbossSide};
use cadcraft3d::part::freeform::FreeformOp;
use cadcraft3d::part::plastic::boss::BossOp;
use cadcraft3d::part::plastic::grill::GrillOp;
use cadcraft3d::part::plastic::lip::{LipKind, LipOp};
use cadcraft3d::part::surface_op::{PatchOp, StitchOp, SurfEdge, TrimOp, TrimTool};
use cadcraft3d::part::{
    BodyOp, BodyOpFeature, LoftFeature, LoftSection, Output, SplitFeature, SplitMethod,
    SplitToolRef, ThickenOp, WorkDef, WorkFeature, WorkPlaneDef,
};
use cadcraft3d::sketch::Sketch;
use common::*;

const LIMIT: Duration = Duration::from_secs(90);

/// One step: run on a copy, keep the result only when it is healthy.
fn step(
    part: &mut PartDocument,
    what: &str,
    f: impl FnOnce(&mut PartDocument) -> Result<(), String> + Send + 'static,
) -> bool {
    let (tx, rx) = mpsc::channel();
    let mut copy = part.clone();
    let start = Instant::now();
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(move || {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&mut copy)))
                .unwrap_or_else(|_| Err("PANIC".into()));
            let _ = tx.send(r.map(|()| copy));
        })
        .unwrap();
    let ms = || start.elapsed().as_millis();
    match rx.recv_timeout(LIMIT) {
        Ok(Ok(done)) if done.feature_health.is_empty() && done.exact => {
            let (solid, sheets) = totals(&done);
            eprintln!(
                "OK    {what:<46} {:>7} ms  V {solid:.0}  sheets {sheets}",
                ms()
            );
            *part = done;
            true
        }
        Ok(Ok(done)) => {
            let why = format!("exact {} {:?}", done.exact, done.feature_health);
            eprintln!("SICK  {what:<46} {:>7} ms  {}", ms(), short(&why));
            false
        }
        Ok(Err(e)) => {
            eprintln!("FAIL  {what:<46} {:>7} ms  {}", ms(), short(&e));
            false
        }
        Err(_) => {
            eprintln!("HANG  {what:<46} > {} s", LIMIT.as_secs());
            false
        }
    }
}

fn short(s: &str) -> String {
    s.chars().take(240).collect()
}

fn totals(p: &PartDocument) -> (Real, usize) {
    let solid = p
        .exact_bodies
        .0
        .iter()
        .filter(|s| !s.is_open())
        .filter_map(|s| s.volume_at(1e-2))
        .sum();
    let sheets = p.exact_bodies.0.iter().filter(|s| s.is_open()).count();
    (solid, sheets)
}

fn plane_at(part: &mut PartDocument, base: OriginPlane, d: Real) -> PlaneRef {
    let id = part
        .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
            base: PlaneRef::Origin(base),
            distance: d,
        })))
        .unwrap();
    PlaneRef::Work(id)
}

fn sketch_on(part: &mut PartDocument, plane: PlaneRef, draw: impl FnOnce(&mut Sketch)) -> usize {
    let k = part
        .create_sketch(
            SketchPlacement {
                plane,
                ..Default::default()
            },
            &opts(),
        )
        .unwrap();
    draw(&mut part.sketches[k].sketch);
    k
}

/// A closed interpolation spline through `pts`.
fn closed(s: &mut Sketch, pts: &[(Real, Real)]) {
    let mut ids: Vec<_> = pts
        .iter()
        .map(|&(x, y)| s.add_point(Point2::new(x, y)))
        .collect();
    ids.push(ids[0]);
    s.add_interp_spline(ids).unwrap();
}

/// A super-ellipse about (cx, cy): `n` 2 = an ellipse, larger = a rounded rectangle.
fn oval(cx: Real, cy: Real, a: Real, b: Real, n: Real) -> Vec<(Real, Real)> {
    (0..16)
        .map(|k| {
            let t = k as Real * PI / 8.0;
            let (c, s) = (t.cos(), t.sin());
            (
                cx + a * c.signum() * c.abs().powf(2.0 / n),
                cy + b * s.signum() * s.abs().powf(2.0 / n),
            )
        })
        .collect()
}

/// A teardrop in the plane, tip up.
fn drop_shape(cx: Real, cy: Real, h: Real) -> Vec<(Real, Real)> {
    [
        (0.0, 1.0),
        (0.18, 0.55),
        (0.4, 0.2),
        (0.46, -0.1),
        (0.3, -0.42),
        (0.0, -0.55),
        (-0.3, -0.42),
        (-0.46, -0.1),
        (-0.4, 0.2),
        (-0.18, 0.55),
    ]
    .iter()
    .map(|&(x, y)| (cx + x * h, cy + y * h))
    .collect()
}

fn section(part: &PartDocument, k: usize) -> LoftSection {
    let r = part.sketches[k].sketch.regions();
    LoftSection::Profile {
        sketch: part.sketches[k].id.clone(),
        profile: ProfileRef::of(&r[0], r[0].interior_point()),
    }
}

fn perr(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Centre of a face's display triangles and its name, for every named face.
fn faces(p: &PartDocument) -> Vec<(String, [Real; 3])> {
    p.body
        .faces()
        .filter_map(|(_, f)| {
            let (name, m) = (f.name.clone()?, f.mesh.as_ref()?);
            let n = m.positions.len().max(1) as Real;
            let c = m.positions.iter().fold([0.0; 3], |a, q| {
                [a[0] + q.x / n, a[1] + q.y / n, a[2] + q.z / n]
            });
            Some((name, c))
        })
        .collect()
}

/// The face whose centre is highest along `dir`.
fn extreme_face(p: &PartDocument, dir: [Real; 3]) -> Option<FaceRef> {
    let dot = |c: &[Real; 3]| c[0] * dir[0] + c[1] * dir[1] + c[2] * dir[2];
    faces(p)
        .into_iter()
        .max_by(|a, b| dot(&a.1).total_cmp(&dot(&b.1)))
        .map(|(face, hint)| FaceRef { face, hint })
}

fn emboss(k: usize, p: &PartDocument, kind: EmbossKind, depth: Real, side: EmbossSide) -> BodyOp {
    let mut op = EmbossOp::new(p.sketches[k].id.clone(), depth);
    op.style = kind;
    op.side = side;
    BodyOp::Emboss(op)
}

fn body_op(p: &mut PartDocument, o: BodyOp) -> Result<String, String> {
    p.add_body_op(BodyOpFeature::new(o)).map_err(perr)
}

fn last_sheet(p: &PartDocument) -> Option<String> {
    p.exact_bodies
        .0
        .iter()
        .rev()
        .find(|s| s.is_open())
        .and_then(|s| s.name.clone())
}

fn thicken_quilt(p: &mut PartDocument, t: Real) -> Result<(), String> {
    let name = last_sheet(p).ok_or("no surface body")?;
    let mut op = ThickenOp::new();
    op.distance = t;
    op.select = cadcraft3d::part::ThickenSelect::Quilt;
    op.quilts = vec![name];
    op.operation = Operation::NewSolid;
    body_op(p, BodyOp::Thicken(op)).map(|_| ())
}

/// Every free edge of the surface `name` as references.
fn free_edges(p: &PartDocument, near: impl Fn([Real; 3]) -> bool) -> Vec<SurfEdge> {
    p.body
        .edges()
        .filter(|(e, _)| p.body.edge_faces(*e).len() == 1)
        .filter_map(|(e, _)| {
            let pts = p
                .body
                .sample_edge(e, &TessellationOptions::default())
                .ok()?;
            let q = pts[pts.len() / 2];
            let hint = [q.x, q.y, q.z];
            let f = p.body.edge_faces(e)[0];
            let face = p.body.face(f)?.name.clone()?;
            near(hint).then_some(SurfEdge { face, hint })
        })
        .collect()
}

fn bottle_sections(p: &mut PartDocument) -> Vec<usize> {
    let rows = [
        (0.0, 36.0, 24.0, 4.0),
        (50.0, 38.0, 26.0, 3.0),
        (110.0, 30.0, 22.0, 2.4),
        (170.0, 34.0, 28.0, 2.0),
        (205.0, 15.0, 15.0, 2.0),
        (235.0, 14.0, 14.0, 2.0),
    ];
    rows.iter()
        .map(|&(z, a, b, n)| {
            let plane = plane_at(p, OriginPlane::XY, z);
            sketch_on(p, plane, |s| closed(s, &oval(0.0, 0.0, a, b, n)))
        })
        .collect()
}

#[test]
#[ignore = "a report of what the tools can do, not a check"]
fn bottle() {
    eprintln!("\n=== OIL BOTTLE: solid route ===");
    let mut p = PartDocument::default();
    step(&mut p, "loft body through 6 sections", |p| {
        let ks = bottle_sections(p);
        let def = LoftFeature::new(ks.iter().map(|&k| section(p, k)).collect());
        p.add_loft(def).map(|_| ()).map_err(perr)
    });
    step(&mut p, "emboss a drop from the front (curved)", |p| {
        let plane = PlaneRef::Origin(OriginPlane::XZ);
        let k = sketch_on(p, plane, |s| closed(s, &drop_shape(0.0, 60.0, 40.0)));
        let o = emboss(
            k,
            p,
            EmbossKind::EmbossFromFace,
            1.0,
            EmbossSide::Direction1,
        );
        body_op(p, o).map(|_| ())
    });
    step(&mut p, "engrave an oval logo on the back", |p| {
        let plane = PlaneRef::Origin(OriginPlane::XZ);
        let k = sketch_on(p, plane, |s| closed(s, &oval(0.0, 150.0, 18.0, 9.0, 2.0)));
        let o = emboss(
            k,
            p,
            EmbossKind::EngraveFromFace,
            0.8,
            EmbossSide::Direction2,
        );
        body_op(p, o).map(|_| ())
    });
    step(&mut p, "fillet the base rim r6", |p| {
        let e = edges_where(p, |h| h[2].abs() < 1e-3);
        p.add_blend(BlendFeature::fillet(e, 6.0))
            .map(|_| ())
            .map_err(perr)
    });
    step(&mut p, "shell 1.2 mm, open at the neck", |p| {
        let top = extreme_face(p, [0.0, 0.0, 1.0]).ok_or("no top")?;
        let mut def = ShellFeature::new(1.2);
        def.remove = vec![top];
        p.add_shell(def).map(|_| ()).map_err(perr)
    });

    eprintln!("=== OIL BOTTLE: surface route (cut a window, patch it, stitch, thicken) ===");
    let mut s = PartDocument::default();
    step(&mut s, "loft Output Surface through 6 sections", |p| {
        let ks = bottle_sections(p);
        let mut def = LoftFeature::new(ks.iter().map(|&k| section(p, k)).collect());
        def.output = Output::Surface;
        p.add_loft(def).map(|_| ()).map_err(perr)
    });
    let trimmed = step(&mut s, "trim a label window by a closed sketch", |p| {
        let plane = PlaneRef::Origin(OriginPlane::XZ);
        let k = sketch_on(p, plane, |s| closed(s, &oval(0.0, 60.0, 18.0, 28.0, 3.0)));
        let id = p.sketches[k].id.clone();
        let surface = last_sheet(p).ok_or("no surface body")?;
        let tris = surface_point_towards(p, -1.0, 60.0).ok_or("no point")?;
        let op = TrimOp {
            surface,
            tool: TrimTool::Sketch(id),
            remove: tris,
            more: Vec::new(),
            invert: false,
        };
        body_op(p, BodyOp::Trim(op)).map(|_| ())
    });
    if trimmed {
        step(&mut s, "boundary patch of the window, G1", |p| {
            let edges = free_edges(p, |h| (h[2] - 60.0).abs() < 35.0);
            if edges.is_empty() {
                return Err("no free edges at the window".into());
            }
            eprintln!("      window edges: {}", edges.len());
            let op = PatchOp {
                edges,
                tangent: Some(1.0),
                ..Default::default()
            };
            body_op(p, BodyOp::Patch(op)).map(|_| ())
        });
        step(&mut s, "stitch patch and body", |p| {
            let surfaces: Vec<_> = p
                .exact_bodies
                .0
                .iter()
                .filter(|s| s.is_open())
                .filter_map(|s| s.name.clone())
                .collect();
            let op = StitchOp {
                surfaces,
                tolerance: 0.01,
                keep_surface: true,
            };
            body_op(p, BodyOp::Stitch(op)).map(|_| ())
        });
    }
    step(&mut s, "thicken the quilt 1.2 mm", |p| {
        thicken_quilt(p, 1.2)
    });
}

/// A point of the first surface body on the side `sign` of Y, near height `z`.
fn surface_point_towards(p: &PartDocument, sign: Real, z: Real) -> Option<[Real; 3]> {
    let s = p.exact_bodies.0.iter().rev().find(|s| s.is_open())?;
    let t = s.triangles().ok()?;
    t.iter()
        .flatten()
        .filter(|q| q.y * sign > 0.0)
        .min_by(|a, b| {
            let d = |q: &Point3| (q.z - z).abs() + q.x.abs();
            d(a).total_cmp(&d(b))
        })
        .map(|q| [q.x, q.y, q.z])
}

#[test]
#[ignore = "a report of what the tools can do, not a check"]
fn hair_dryer() {
    eprintln!("\n=== HAIR DRYER ===");
    let mut p = PartDocument::default();
    step(&mut p, "barrel: loft of 4 circles", |p| {
        let mut ks = Vec::new();
        for (z, r) in [(0.0, 30.0), (60.0, 38.0), (150.0, 36.0), (205.0, 22.0)] {
            let plane = plane_at(p, OriginPlane::XY, z);
            ks.push(sketch_on(p, plane, |s| {
                s.add_circle(Point2::new(0.0, 0.0), r).unwrap();
            }));
        }
        let def = LoftFeature::new(ks.iter().map(|&k| section(p, k)).collect());
        p.add_loft(def).map(|_| ()).map_err(perr)
    });
    step(&mut p, "handle: loft of 3 ovals, Join", |p| {
        let mut ks = Vec::new();
        for (y, a, b) in [(10.0, 30.0, 18.0), (80.0, 26.0, 17.0), (150.0, 30.0, 20.0)] {
            let plane = plane_at(p, OriginPlane::XZ, y);
            ks.push(sketch_on(p, plane, |s| {
                closed(s, &oval(0.0, 100.0, b, a, 2.4))
            }));
        }
        let mut def = LoftFeature::new(ks.iter().map(|&k| section(p, k)).collect());
        def.operation = Operation::Join;
        p.add_loft(def).map(|_| ()).map_err(perr)
    });
    step(&mut p, "fillet barrel-handle junction r8", |p| {
        let e: Vec<_> = edges_where(p, |_| true)
            .into_iter()
            .filter(|r| {
                let prefix = |s: &str| s.split(':').next().unwrap_or("").to_string();
                prefix(&r.faces[0]) != prefix(&r.faces[1])
            })
            .collect();
        eprintln!("      junction edges: {}", e.len());
        if e.is_empty() {
            return Err("no junction edges".into());
        }
        p.add_blend(BlendFeature::fillet(e, 8.0))
            .map(|_| ())
            .map_err(perr)
    });
    step(&mut p, "shell 2 mm, open at the nozzle", |p| {
        let top = extreme_face(p, [0.0, 0.0, 1.0]).ok_or("no top")?;
        let mut def = ShellFeature::new(2.0);
        def.remove = vec![top];
        p.add_shell(def).map(|_| ()).map_err(perr)
    });
    step(&mut p, "grill in the back wall (circle r20, 3 ribs)", |p| {
        let mut lines = Vec::new();
        let k = sketch_on(p, PlaneRef::Origin(OriginPlane::XY), |s| {
            s.add_circle(Point2::new(0.0, 0.0), 20.0).unwrap();
            for y in [-10.0, 0.0, 10.0] {
                lines.push(s.add_line(Point2::new(-25.0, y), Point2::new(25.0, y)));
            }
        });
        let id = p.sketches[k].id.clone();
        let mut op = GrillOp::new(id.clone());
        op.ribs = Some(cadcraft3d::part::plastic::grill::GrillCurves {
            sketch: id,
            curves: lines,
            thickness: 1.5,
            height: 2.0,
            top_offset: 0.0,
            bottom_offset: 0.0,
        });
        body_op(p, BodyOp::Grill(op)).map(|_| ())
    });
    step(&mut p, "split into two halves by YZ", |p| {
        let mut def = SplitFeature::new(SplitMethod::SplitSolid);
        def.tool = Some(SplitToolRef::Plane(PlaneRef::Origin(OriginPlane::YZ)));
        p.add_split(def).map(|_| ()).map_err(perr)
    });
    let halves = p.clone();
    step(&mut p, "lip on the outer rim of one half", |p| {
        let (outer, _) = parting_rim(p, "#2:");
        eprintln!("      outer rim edges: {}", outer.len());
        let op = LipOp::new(outer, 1.5, 1.0);
        body_op(p, BodyOp::Lip(op)).map(|_| ())
    });
    step(&mut p, "groove on the outer rim of the other half", |p| {
        let (outer, _) = parting_rim(p, "");
        eprintln!("      outer rim edges: {}", outer.len());
        let mut op = LipOp::new(outer, 1.6, 1.1);
        op.kind = LipKind::Groove;
        body_op(p, BodyOp::Lip(op)).map(|_| ())
    });
    for flip in [false, true] {
        let mut q = halves.clone();
        step(
            &mut q,
            &format!("screw boss in the handle (flip {flip})"),
            move |p| {
                let k = sketch_on(p, PlaneRef::Origin(OriginPlane::YZ), |s| {
                    s.add_center_point(Point2::new(110.0, 100.0));
                });
                let mut op = BossOp::new(p.sketches[k].id.clone());
                op.flip = flip;
                body_op(p, BodyOp::Boss(op)).map(|_| ())
            },
        );
    }
    step(&mut p, "emboss a vent oval on a curved wall", |p| {
        let plane = PlaneRef::Origin(OriginPlane::YZ);
        let k = sketch_on(p, plane, |s| closed(s, &oval(-5.0, 170.0, 8.0, 14.0, 3.0)));
        let o = emboss(
            k,
            p,
            EmbossKind::EmbossFromFace,
            1.0,
            EmbossSide::Direction1,
        );
        body_op(p, o).map(|_| ())
    });
}

/// The parting rim of a split housing (edges at x = 0 on a face whose name holds `body`; an
/// empty `body` takes the first body's, the faces without `#2:`): outer and inner loop, told
/// apart by the other face (the shell's inner faces carry `Inner` or the shell's id).
fn parting_rim(p: &PartDocument, body: &str) -> (Vec<EdgeRef>, Vec<EdgeRef>) {
    let mut outer = Vec::new();
    let mut inner = Vec::new();
    for r in edges_where(p, |h| h[0].abs() < 1e-4) {
        let cut = r.faces.iter().position(|f| {
            f.ends_with(":Cut")
                && if body.is_empty() {
                    !f.contains("#2:")
                } else {
                    f.contains(body)
                }
        });
        let Some(c) = cut else { continue };
        let other = &r.faces[1 - c];
        if other.contains("Shell") || other.contains("Inner") {
            inner.push(r);
        } else {
            outer.push(r);
        }
    }
    (outer, inner)
}

fn bumper_op() -> FreeformOp {
    let mut op = FreeformOp::plane(PlaneRef::Origin(OriginPlane::XY), 1600.0, 500.0);
    op.length_faces = 8;
    op.width_faces = 4;
    for i in 0..=8usize {
        let u = i as Real / 8.0 * 2.0 - 1.0;
        for j in 0..=4usize {
            let v = j as Real / 4.0 * 2.0 - 1.0;
            let wrap = -220.0 * u * u * u * u;
            let bulge = 70.0 * (1.0 - v * v);
            let sweep = 60.0 * u * u * v;
            op.set_offset(i, j, [0.0, sweep, wrap + bulge]);
        }
    }
    op
}

#[test]
#[ignore = "a report of what the tools can do, not a check"]
fn bumper() {
    eprintln!("\n=== CAR BUMPER ===");
    let mut p = PartDocument::default();
    step(&mut p, "double-curved skin: Freeform cage 8x4", |p| {
        body_op(p, BodyOp::Freeform(bumper_op())).map(|_| ())
    });
    let opening = |p: &mut PartDocument, a: Real, b: Real, cx: Real| {
        let plane = plane_at(p, OriginPlane::XY, 400.0);
        let k = sketch_on(p, plane, |s| closed(s, &oval(cx, 40.0, a, b, 4.0)));
        let id = p.sketches[k].id.clone();
        let surface = last_sheet(p).ok_or("no surface body")?;
        let t = p
            .exact_bodies
            .0
            .iter()
            .rev()
            .find(|s| s.is_open())
            .ok_or("no surface")?
            .triangles()
            .map_err(perr)?;
        let q = t
            .iter()
            .flatten()
            .min_by(|l, r| {
                let d = |q: &Point3| (q.x - cx).abs() + (q.y - 40.0).abs();
                d(l).total_cmp(&d(r))
            })
            .ok_or("no point")?;
        let op = TrimOp {
            surface,
            tool: TrimTool::Sketch(id),
            remove: [q.x, q.y, q.z],
            more: Vec::new(),
            invert: false,
        };
        body_op(p, BodyOp::Trim(op)).map(|_| ())
    };
    step(&mut p, "trim the grille opening", move |p| {
        opening(p, 240.0, 55.0, 0.0)
    });
    step(&mut p, "trim the left fog lamp", move |p| {
        opening(p, 60.0, 35.0, -480.0)
    });
    step(&mut p, "trim the right fog lamp", move |p| {
        opening(p, 60.0, 35.0, 480.0)
    });
    let before = p.clone();
    step(&mut p, "thicken 3 mm", |p| thicken_quilt(p, 3.0));
    step(&mut p, "fillet the grille rim r2", |p| {
        let e = edges_where(p, |h| h[0].abs() < 260.0 && h[1].abs() < 120.0);
        eprintln!("      rim edges: {}", e.len());
        if e.is_empty() {
            return Err("no rim edges".into());
        }
        p.add_blend(BlendFeature::fillet(e, 2.0))
            .map(|_| ())
            .map_err(perr)
    });
    for flip in [false, true] {
        let mut q = p.clone();
        step(
            &mut q,
            &format!("boss under the skin (flip {flip})"),
            move |p| {
                let k = sketch_on(p, PlaneRef::Origin(OriginPlane::XY), |s| {
                    s.add_center_point(Point2::new(300.0, 0.0));
                });
                let mut op = BossOp::new(p.sketches[k].id.clone());
                op.flip = flip;
                body_op(p, BodyOp::Boss(op)).map(|_| ())
            },
        );
    }
    let mut s = before;
    step(&mut s, "patch the grille back, G1", |p| {
        let edges = free_edges(p, |h| h[0].abs() < 260.0 && (h[1] - 40.0).abs() < 70.0);
        eprintln!("      grille edges: {}", edges.len());
        if edges.is_empty() {
            return Err("no free edges".into());
        }
        let op = PatchOp {
            edges,
            tangent: Some(1.0),
            ..Default::default()
        };
        body_op(p, BodyOp::Patch(op)).map(|_| ())
    });
}

/// SMLib numbers a Freeform surface's corners along its loop, unlike the surface's record, and
/// every split of such a surface was refused for it.
#[test]
fn a_freeform_surface_can_be_split() {
    use cadcraft3d::kernel::exact::{self, surface_ops};
    use cadcraft3d::kernel::{Plane, Point3, Vector3};

    let net: Vec<Vec<Point3>> = (0..4)
        .map(|i| {
            (0..4)
                .map(|j| {
                    let (x, y) = (f64::from(i) * 10.0, f64::from(j) * 10.0);
                    Point3::new(x, y, ((x - 15.0).powi(2) + (y - 15.0).powi(2)) * 0.02)
                })
                .collect()
        })
        .collect();
    let sheet = surface_ops::freeform_sheet(&net, "F").unwrap_or_else(|e| panic!("{e}"));
    let plane = Plane::from_normal(Point3::new(15.0, 0.0, 0.0), Vector3::x())
        .unwrap_or_else(|e| panic!("{e:?}"));
    let split = exact::split_faces(&sheet, &[0], &exact::SplitBy::Plane(plane), "s", None)
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(surface_ops::face_count(&split), 2);
}

/// Removes the piece of the last surface around (cx, cy), cut by a sketch 400 mm above XY.
fn cut_hole(
    p: &mut PartDocument,
    cx: Real,
    cy: Real,
    draw: impl FnOnce(&mut Sketch),
) -> Result<(), String> {
    let plane = plane_at(p, OriginPlane::XY, 400.0);
    let k = sketch_on(p, plane, draw);
    let id = p.sketches[k].id.clone();
    let surface = last_sheet(p).ok_or("no surface body")?;
    let t = p
        .exact_bodies
        .0
        .iter()
        .rev()
        .find(|s| s.is_open())
        .ok_or("no surface")?
        .triangles()
        .map_err(perr)?;
    let q = t
        .iter()
        .flatten()
        .min_by(|l, r| {
            let d = |q: &Point3| (q.x - cx).abs() + (q.y - cy).abs();
            d(l).total_cmp(&d(r))
        })
        .ok_or("no point")?;
    let op = TrimOp {
        surface,
        tool: TrimTool::Sketch(id),
        remove: [q.x, q.y, q.z],
        more: Vec::new(),
        invert: false,
    };
    body_op(p, BodyOp::Trim(op)).map(|_| ())
}

#[test]
#[ignore = "a report of what the tools can do, not a check"]
fn thicken_cases() {
    let skin = |name: &str| {
        eprintln!("--- {name}");
        let mut p = PartDocument::default();
        step(&mut p, "skin", |p| {
            body_op(p, BodyOp::Freeform(bumper_op())).map(|_| ())
        });
        p
    };
    let thicken = |p: &PartDocument, t: Real| {
        let mut q = p.clone();
        step(&mut q, &format!("thicken {t}"), move |p| {
            thicken_quilt(p, t)
        });
    };

    let mut p = skin("plane cut");
    step(&mut p, "trim by a plane", |p| {
        let plane = plane_at(p, OriginPlane::YZ, 300.0);
        let surface = last_sheet(p).ok_or("no surface body")?;
        let op = TrimOp {
            surface,
            tool: TrimTool::Plane(plane),
            remove: [700.0, 0.0, 0.0],
            more: Vec::new(),
            invert: false,
        };
        body_op(p, BodyOp::Trim(op)).map(|_| ())
    });
    thicken(&p, 3.0);

    let mut p = skin("one circle");
    step(&mut p, "hole r40", |p| {
        cut_hole(p, 0.0, 40.0, |s| {
            s.add_circle(Point2::new(0.0, 40.0), 40.0).unwrap();
        })
    });
    thicken(&p, 3.0);
    thicken(&p, 1.0);
    thicken(&p, -3.0);

    let mut p = skin("one oval spline");
    step(&mut p, "oval hole", |p| {
        cut_hole(p, 0.0, 40.0, |s| {
            closed(s, &oval(0.0, 40.0, 240.0, 55.0, 4.0))
        })
    });
    thicken(&p, 3.0);
    thicken(&p, 1.0);
    thicken(&p, -3.0);
}

/// A small skin whose oval hole Thicken refuses: "Bad Surf Discontinuity" on a side wall.
fn small_skin_with_oval_hole() -> PartDocument {
    let mut op = FreeformOp::plane(PlaneRef::Origin(OriginPlane::XY), 400.0, 200.0);
    op.length_faces = 3;
    op.width_faces = 3;
    for i in 0..=3usize {
        for j in 0..=3usize {
            let (u, v) = (i as Real / 3.0 * 2.0 - 1.0, j as Real / 3.0 * 2.0 - 1.0);
            op.set_offset(
                i,
                j,
                [0.0, 0.0, 20.0 * (1.0 - u * u) + 10.0 * (1.0 - v * v)],
            );
        }
    }
    let mut p = PartDocument::default();
    body_op(&mut p, BodyOp::Freeform(op)).unwrap();
    cut_hole(&mut p, 0.0, 0.0, |s| {
        closed(s, &oval(0.0, 0.0, 80.0, 40.0, 4.0))
    })
    .unwrap();
    let rim = free_edges(&p, |h| h[0].abs() < 120.0 && h[1].abs() < 60.0);
    assert_eq!(rim.len(), 1, "the hole's rim");
    p
}

/// A closed spline cuts as one curve: its Bézier pieces, cut one by one, left a loop of many
/// edges.
#[test]
fn a_closed_spline_cuts_one_rim_edge() {
    small_skin_with_oval_hole();
}

#[test]
#[ignore = "a report of what the tools can do, not a check"]
fn thicken_small_oval_hole() {
    let mut p = small_skin_with_oval_hole();
    step(&mut p, "thicken 2", |p| thicken_quilt(p, 2.0));
}

/// The owner's part (spec 5.4a F16-F18) as it was saved, built again.
fn owner_part() -> PartDocument {
    let path = std::path::Path::new("tests/data/surface_cut_owner.ccpart");
    let mut project = cadcraft3d::document::Project::load(path).unwrap();
    let id = project
        .documents()
        .iter()
        .find(|d| matches!(d.data, cadcraft3d::document::DocumentData::Part(_)))
        .map(|d| d.id)
        .unwrap();
    let Some(cadcraft3d::document::DocumentData::Part(part)) =
        project.document_mut(id).map(|d| &mut d.data)
    else {
        panic!("no part");
    };
    part.regenerate().unwrap();
    part.clone()
}

/// A point on each display face of the body or surface `name`.
fn points_on(p: &PartDocument, name: &str) -> Vec<[Real; 3]> {
    faces(p)
        .into_iter()
        .filter(|(f, _)| f.starts_with(name))
        .map(|(_, c)| c)
        .collect()
}

fn split_by(p: &mut PartDocument, m: SplitMethod, tool: &str, solid: &str) -> Result<(), String> {
    let mut f = SplitFeature::new(m);
    f.tool = Some(SplitToolRef::Surface(tool.into()));
    f.solid = solid.into();
    p.add_split(f).map(|_| ()).map_err(perr)
}

fn shell(p: &mut PartDocument, t: Real, remove: Vec<FaceRef>) -> Result<(), String> {
    let mut f = cadcraft3d::part::ShellFeature::new(t);
    f.chain = false;
    f.remove = remove;
    p.add_shell(f).map(|_| ()).map_err(perr)
}

#[test]
#[ignore = "a report of what the tools can do, not a check"]
fn owner_surface_cut() {
    let p = owner_part();
    eprintln!("exact {} failure {:?}", p.exact, p.exact_failure);
    eprintln!("health {:?}", p.feature_health);
    for k in 1..=p.features.len() {
        let mut q = p.clone();
        q.build_cache = Default::default();
        q.features.truncate(k);
        let r = q.regenerate();
        eprintln!(
            "  first {k} ({}): exact {} {:?} {:?}",
            q.features[k - 1].id(),
            q.exact,
            r.is_ok(),
            q.exact_failure
        );
    }
    for s in &p.exact_bodies.0 {
        eprintln!(
            "body {:?} open {} V {:?} size {:.1}",
            s.name,
            s.is_open(),
            s.volume_at(1e-2),
            s.size()
        );
    }
    for (f, c) in faces(&p) {
        eprintln!("  {f} @ {c:.1?}");
    }
    let solid = p
        .exact_bodies
        .0
        .iter()
        .find(|s| !s.is_open())
        .and_then(|s| s.name.clone())
        .unwrap_or_default();

    // F18: a surface made from a spline as the tool.
    let t = |what: &str, m: SplitMethod, tool: &'static str| {
        let mut q = p.clone();
        let solid = solid.clone();
        step(&mut q, what, move |x| split_by(x, m, tool, &solid));
    };
    t(
        "F18 Split Solid by Extrusion3",
        SplitMethod::SplitSolid,
        "Extrusion3",
    );
    t(
        "F18 Trim Solid by Extrusion3",
        SplitMethod::TrimSolid,
        "Extrusion3",
    );
    t(
        "F17 Split Solid by Extrusion2",
        SplitMethod::SplitSolid,
        "Extrusion2",
    );
    t(
        "F17 Trim Solid by Extrusion2",
        SplitMethod::TrimSolid,
        "Extrusion2",
    );

    let trim = |surface: &'static str, tool: &'static str| {
        for (k, at) in points_on(&p, surface).into_iter().enumerate() {
            let mut q = p.clone();
            let what = format!("trim {surface} by {tool} at face {k}");
            step(&mut q, &what, move |x| {
                let o = TrimOp {
                    surface: surface.into(),
                    tool: TrimTool::Surface(tool.into()),
                    remove: at,
                    more: Vec::new(),
                    invert: false,
                };
                body_op(x, BodyOp::Trim(o)).map(|_| ())
            });
        }
    };
    trim("Extrusion3", "Extrusion2"); // F17: the tool is a closed tube
    trim("Extrusion2", "Extrusion3"); // F18: the tool comes from a spline

    // F16: Shell of the shape, closed and with each outer face removed in turn.
    for t in [1.0, 2.0] {
        let mut q = p.clone();
        step(&mut q, &format!("F16 shell {t} closed"), move |x| {
            shell(x, t, Vec::new())
        });
    }
    for (face, hint) in faces(&p) {
        if face.starts_with("Extrusion2") || face.starts_with("Extrusion3") {
            continue;
        }
        let mut q = p.clone();
        let what: String = format!("F16 shell 1 open {face}")
            .chars()
            .take(46)
            .collect();
        step(&mut q, &what, move |x| {
            shell(x, 1.0, vec![FaceRef { face, hint }])
        });
    }
}

/// The owner's part: arcs and lines joined by constraints end a hair apart, which the kernel
/// refused for a surface and the whole part went faceted.
#[test]
fn the_owners_surface_part_builds_exact() {
    let p = owner_part();
    assert!(p.exact, "{:?}", p.exact_failure);
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
}

fn z_and_x_range(p: &PartDocument, name: &str) -> [Real; 4] {
    let s = p
        .exact_bodies
        .0
        .iter()
        .find(|s| s.name.as_deref() == Some(name))
        .unwrap();
    let mut r = [Real::MAX, Real::MIN, Real::MAX, Real::MIN];
    for q in s.triangles().unwrap().iter().flatten() {
        r = [r[0].min(q.z), r[1].max(q.z), r[2].min(q.x), r[3].max(q.x)];
    }
    r
}

fn trim_owner(surface: &str, tool: &str, remove: [Real; 3]) -> PartDocument {
    let mut p = owner_part();
    let op = TrimOp {
        surface: surface.into(),
        tool: TrimTool::Surface(tool.into()),
        remove,
        more: Vec::new(),
        invert: false,
    };
    body_op(&mut p, BodyOp::Trim(op)).unwrap();
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    p
}

/// Spec 5.4a F18: a surface extruded from a spline is several faces; the first alone did not
/// reach the surface ("the tool does not cross the selected faces").
#[test]
fn a_spline_surface_trims_along_all_its_faces() {
    let p = trim_owner("Extrusion2", "Extrusion3", [-8.6, 0.0, -32.4]);
    let [zmin, ..] = z_and_x_range(&p, "Extrusion2");
    assert!(zmin > -5.0, "the part below the spline is gone: {zmin}");
}

/// Spec 5.4a F17: a closed tube as the tool cuts along the whole tube.
#[test]
fn a_closed_tube_trims_a_surface() {
    let before = z_and_x_range(&owner_part(), "Extrusion3");
    assert!(before[3] > 60.0, "{before:?}");
    let p = trim_owner("Extrusion3", "Extrusion2", [69.8, 0.0, -2.2]);
    let r = z_and_x_range(&p, "Extrusion3");
    assert!(
        r[3] < 53.0 && r[2] < -40.0,
        "what is outside the tube goes: {r:?}"
    );
}

/// Shell of lofted bodies, from the simplest up: where the kernel's shell starts to refuse.
#[test]
#[ignore = "a report of what the tools can do, not a check"]
fn shell_of_lofts() {
    let circles = |rs: &'static [(Real, Real)]| {
        move |p: &mut PartDocument| -> Vec<usize> {
            rs.iter()
                .map(|&(z, r)| {
                    let plane = plane_at(p, OriginPlane::XY, z);
                    sketch_on(p, plane, |s| {
                        s.add_circle(Point2::new(0.0, 0.0), r).unwrap();
                    })
                })
                .collect()
        }
    };
    let ovals = |rs: &'static [(Real, Real, Real)]| {
        move |p: &mut PartDocument| -> Vec<usize> {
            rs.iter()
                .map(|&(z, a, b)| {
                    let plane = plane_at(p, OriginPlane::XY, z);
                    sketch_on(p, plane, |s| closed(s, &oval(0.0, 0.0, a, b, 2.4)))
                })
                .collect()
        }
    };
    let splines = |n: usize, rs: &'static [(Real, Real, Real)]| {
        move |p: &mut PartDocument| -> Vec<usize> {
            rs.iter()
                .map(|&(z, a, b)| {
                    let plane = plane_at(p, OriginPlane::XY, z);
                    let pts: Vec<(Real, Real)> = (0..n)
                        .map(|k| {
                            let t = k as Real * 2.0 * PI / n as Real;
                            (a * t.cos(), b * t.sin())
                        })
                        .collect();
                    sketch_on(p, plane, |s| closed(s, &pts))
                })
                .collect()
        }
    };
    type Make = Box<dyn Fn(&mut PartDocument) -> Vec<usize> + Send + Sync>;
    let only = std::env::var("SHELL_CASE").unwrap_or_default();
    let cases: Vec<(&str, Make)> = vec![
        (
            "circle splines 4 pt",
            Box::new(splines(4, &[(0.0, 30.0, 30.0), (60.0, 20.0, 20.0)])),
        ),
        (
            "circle splines 8 pt",
            Box::new(splines(8, &[(0.0, 30.0, 30.0), (60.0, 20.0, 20.0)])),
        ),
        (
            "circle splines 16 pt",
            Box::new(splines(16, &[(0.0, 30.0, 30.0), (60.0, 20.0, 20.0)])),
        ),
        (
            "ellipse splines 4 pt",
            Box::new(splines(4, &[(0.0, 30.0, 18.0), (80.0, 26.0, 17.0)])),
        ),
        (
            "ellipse splines 16 pt",
            Box::new(splines(16, &[(0.0, 30.0, 18.0), (80.0, 26.0, 17.0)])),
        ),
        (
            "2 circles (cone)",
            Box::new(circles(&[(0.0, 30.0), (60.0, 20.0)])),
        ),
        (
            "3 circles",
            Box::new(circles(&[(0.0, 30.0), (60.0, 38.0), (120.0, 25.0)])),
        ),
        (
            "4 circles (barrel)",
            Box::new(circles(&[
                (0.0, 30.0),
                (60.0, 38.0),
                (150.0, 36.0),
                (205.0, 22.0),
            ])),
        ),
        (
            "2 ovals",
            Box::new(ovals(&[(0.0, 30.0, 18.0), (80.0, 26.0, 17.0)])),
        ),
        (
            "3 ovals",
            Box::new(ovals(&[
                (0.0, 30.0, 18.0),
                (80.0, 26.0, 17.0),
                (150.0, 30.0, 20.0),
            ])),
        ),
        (
            "bottle sections",
            Box::new(|p: &mut PartDocument| bottle_sections(p)),
        ),
    ];
    for (name, make) in cases {
        if !only.is_empty() && !name.contains(only.as_str()) {
            continue;
        }
        eprintln!("--- {name}");
        let mut p = PartDocument::default();
        let make = std::sync::Arc::new(make);
        let m = make.clone();
        if !step(&mut p, "loft", move |p| {
            let ks = m(p);
            let def = LoftFeature::new(ks.iter().map(|&k| section(p, k)).collect());
            p.add_loft(def).map(|_| ()).map_err(perr)
        }) {
            continue;
        }
        eprintln!("      faces {}", faces(&p).len());
        {
            let mut q = p.clone();
            step(&mut q, "thicken all faces but the top 2 inward", |p| {
                let top = extreme_face(p, [0.0, 0.0, 1.0]).ok_or("no top")?;
                let mut op = ThickenOp::new();
                op.distance = 2.0;
                op.direction = cadcraft3d::part::ThickenDirection::Direction2;
                op.faces = faces(p)
                    .into_iter()
                    .filter(|(n, _)| *n != top.face)
                    .map(|(face, hint)| FaceRef { face, hint })
                    .collect();
                op.operation = Operation::NewSolid;
                body_op(p, BodyOp::Thicken(op)).map(|_| ())
            });
        }
        for (t, open) in [(2.0, true), (2.0, false), (0.5, true)] {
            let mut q = p.clone();
            step(&mut q, &format!("shell {t} open top {open}"), move |p| {
                let mut def = ShellFeature::new(t);
                if open {
                    def.remove = vec![extreme_face(p, [0.0, 0.0, 1.0]).ok_or("no top")?];
                }
                p.add_shell(def).map(|_| ()).map_err(perr)
            });
        }
    }
}

/// A surface loft through the bottle's oval sections (16-point splines): the kernel's sheet loft
/// refuses the stitch of its columns, so the solid loft's walls are the surface (spec 5.4a F21).
#[test]
fn a_surface_loft_through_ovals_builds() {
    let mut p = PartDocument::default();
    let ks = bottle_sections(&mut p);
    let mut def = LoftFeature::new(ks.iter().map(|&k| section(&p, k)).collect());
    def.output = Output::Surface;
    p.add_loft(def).unwrap_or_else(|e| panic!("{e}"));
    assert!(
        p.exact && p.feature_health.is_empty(),
        "{:?}",
        p.feature_health
    );
    let sheets: Vec<_> = p.exact_bodies.0.iter().filter(|s| s.is_open()).collect();
    assert_eq!(sheets.len(), 1);
    // The walls only: every face is named a side of the loft.
    let names: Vec<String> = faces(&p).into_iter().map(|(n, _)| n).collect();
    assert!(
        !names.is_empty() && names.iter().all(|n| n.contains(":Side:")),
        "{names:?}"
    );
}

/// Writes the bottle's surface loft to `SURFACE_GOALS_OUT` for a look in the app.
#[test]
#[ignore = "writes a file for a visual check"]
fn save_bottle_surface() {
    let Ok(out) = std::env::var("SURFACE_GOALS_OUT") else {
        return;
    };
    let mut p = PartDocument::default();
    let ks = bottle_sections(&mut p);
    let mut def = LoftFeature::new(ks.iter().map(|&k| section(&p, k)).collect());
    def.output = Output::Surface;
    p.add_loft(def).unwrap_or_else(|e| panic!("{e}"));
    let mut project = cadcraft3d::document::Project::new("Bottle");
    project.add_document("Bottle", cadcraft3d::document::DocumentData::Part(p));
    project
        .save(std::path::Path::new(&out))
        .unwrap_or_else(|e| panic!("{e}"));
}
