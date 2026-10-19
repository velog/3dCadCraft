//! Reference cases from SolidCraft (`tests/oracle/README.md`), built
//! through our exact-kernel wrapper (`kernel::exact`) and measured against
//! analytic / FreeCAD values. Run with `--nocapture` for the table.

use std::collections::HashMap;
use std::f64::consts::{PI, TAU};
use std::fmt::Write as _;
use std::path::Path;

use cadcraft3d::kernel::exact::{self, ChamferSecond, ExactOp, ExactSolid};
use cadcraft3d::kernel::{Plane, Point3, ProfileSegment, Real, Segment2, Vector3};
use cadcraft3d::sketch::{
    Constraint, Entity, EntityId, PointId, Sketch as Sk, SketchError, SolveStatus,
};
use nalgebra::Point2;
use serde_json::Value;

type P2 = Point2<Real>;

/// What a case came to.
enum Outcome {
    Pass,
    /// Uses something we do not build yet.
    Skip(String),
    Fail(String),
}

/// A sketch: its plane and curves by id.
struct Sketch {
    plane: Plane,
    curves: HashMap<String, Curve>,
}

#[derive(Clone)]
enum Curve {
    Seg(Segment2),
    Circle(P2, Real),
}

struct Ctx {
    params: HashMap<String, Real>,
    sketches: HashMap<String, Sketch>,
    bodies: Vec<(String, ExactSolid)>,
}

fn err<T>(msg: impl Into<String>) -> Result<T, String> {
    Err(msg.into())
}

fn num(ctx: &Ctx, v: &Value) -> Result<Real, String> {
    match v {
        Value::Number(n) => n.as_f64().ok_or_else(|| "bad number".into()),
        Value::String(s) => {
            let vars = |n: &str| ctx.params.get(n).copied();
            cadcraft3d::part::expr::eval(s, cadcraft3d::part::expr::Unit::Mm, &vars)
                .map(|(v, _)| v)
                .map_err(|e| format!("cannot evaluate '{s}': {e}"))
        }
        _ => err(format!("not a number: {v}")),
    }
}

fn v3(v: &Value) -> Result<Vector3, String> {
    let a = v.as_array().ok_or("not a vector")?;
    let c = |i: usize| a.get(i).and_then(Value::as_f64).ok_or("bad vector");
    Ok(Vector3::new(c(0)?, c(1)?, c(2)?))
}

/// A 2D point: `{"at": [x, y]}` or `[x, y]`.
fn p2(v: &Value) -> Result<P2, String> {
    let a = v.get("at").unwrap_or(v).as_array().ok_or("not a point")?;
    let c = |i: usize| a.get(i).and_then(Value::as_f64).ok_or("bad point");
    Ok(P2::new(c(0)?, c(1)?))
}

fn plane_of(v: &Value) -> Result<Plane, String> {
    let o = v3(&v["origin"])?;
    let x = v3(&v["x_dir"])?;
    let n = v3(&v["normal"])?;
    let p = Plane::new(Point3::from(o), n, x).map_err(|e| e.to_string())?;
    if let Ok(y) = v3(&v["y_dir"])
        && (p.y_axis().into_inner() - y).norm() > 1e-6
    {
        return err("sketch plane is left-handed");
    }
    Ok(p)
}

fn arc_ends(c: P2, r: Real, a0: Real, sweep: Real) -> (P2, P2) {
    let at = |a: Real| P2::new(c.x + r * a.cos(), c.y + r * a.sin());
    (at(a0), at(a0 + sweep))
}

fn ends(s: &Segment2) -> Option<(P2, P2)> {
    match s {
        Segment2::Line { a, b } => Some((*a, *b)),
        Segment2::Arc { c, r, a0, sweep } => Some(arc_ends(*c, *r, *a0, *sweep)),
        _ => None,
    }
}

fn reversed(s: &Segment2) -> Segment2 {
    match s {
        Segment2::Line { a, b } => Segment2::Line { a: *b, b: *a },
        Segment2::Arc { c, r, a0, sweep } => Segment2::Arc {
            c: *c,
            r: *r,
            a0: a0 + sweep,
            sweep: -sweep,
        },
        other => other.clone(),
    }
}

fn sketch(ctx: &Ctx, f: &Value) -> Result<Sketch, String> {
    let plane = plane_of(&f["plane"])?;
    let mut curves = HashMap::new();
    for e in f["entities"].as_array().ok_or("no entities")? {
        let id = e["id"].as_str().unwrap_or_default().to_owned();
        let c = match e["type"].as_str().unwrap_or_default() {
            "line" => Curve::Seg(Segment2::Line {
                a: p2(&e["start"])?,
                b: p2(&e["end"])?,
            }),
            "circle" => Curve::Circle(p2(&e["center"])?, num(ctx, &e["radius"])?),
            "arc_three_point" => {
                let (s, m, t) = (p2(&e["start"])?, p2(&e["mid"])?, p2(&e["end"])?);
                // Circumcentre.
                let d = 2.0 * (s.x * (m.y - t.y) + m.x * (t.y - s.y) + t.x * (s.y - m.y));
                if d.abs() < 1e-12 {
                    return err("collinear 3-point arc");
                }
                let sq = |p: P2| p.x * p.x + p.y * p.y;
                let c = P2::new(
                    (sq(s) * (m.y - t.y) + sq(m) * (t.y - s.y) + sq(t) * (s.y - m.y)) / d,
                    (sq(s) * (t.x - m.x) + sq(m) * (s.x - t.x) + sq(t) * (m.x - s.x)) / d,
                );
                let ang = |p: P2| (p.y - c.y).atan2(p.x - c.x);
                let a0 = ang(s);
                let de = (ang(t) - a0).rem_euclid(TAU);
                let dm = (ang(m) - a0).rem_euclid(TAU);
                let sweep = if dm < de { de } else { de - TAU };
                Curve::Seg(Segment2::Arc {
                    c,
                    r: (s - c).norm(),
                    a0,
                    sweep,
                })
            }
            "arc_center_start_sweep" => {
                let (c, s) = (p2(&e["center"])?, p2(&e["start"])?);
                Curve::Seg(Segment2::Arc {
                    c,
                    r: (s - c).norm(),
                    a0: (s.y - c.y).atan2(s.x - c.x),
                    sweep: num(ctx, &e["sweep_deg"])?.to_radians(),
                })
            }
            "point" => continue,
            // Only paths of sweeps use them; a sweep is not built yet.
            "fitted_spline" => continue,
            other => return err(format!("sketch entity '{other}'")),
        };
        curves.insert(id, c);
    }
    let has = |k: &str| f[k].as_array().is_some_and(|a| !a.is_empty());
    if has("constraints") || has("dimensions") {
        solve(ctx, f, &mut curves)?;
    }
    Ok(Sketch { plane, curves })
}

/// Rebuilds the sketch in our solver (shared points, constraints,
/// dimensions), solves it and replaces `curves` with the solved geometry.
fn solve(ctx: &Ctx, f: &Value, curves: &mut HashMap<String, Curve>) -> Result<(), String> {
    let e_err = |e: SketchError| e.to_string();
    let mut sk = Sk::default();
    let mut pts: HashMap<String, PointId> = HashMap::new();
    let mut ents: HashMap<String, EntityId> = HashMap::new();
    let origin = sk.add_fixed_point(P2::new(0.0, 0.0));
    pts.insert("origin".into(), origin);
    fn point(
        sk: &mut Sk,
        pts: &mut HashMap<String, PointId>,
        key: String,
        v: &Value,
    ) -> Result<PointId, String> {
        if let Some(r) = v.get("ref").and_then(Value::as_str)
            && let Some(&id) = pts.get(r)
        {
            pts.insert(key, id);
            return Ok(id);
        }
        let id = sk.add_point(p2(v)?);
        pts.insert(key, id);
        Ok(id)
    }
    for e in f["entities"].as_array().ok_or("entities")? {
        let id = e["id"].as_str().unwrap_or_default().to_owned();
        match curves.get(&id).cloned() {
            Some(Curve::Seg(Segment2::Line { .. })) => {
                let a = point(&mut sk, &mut pts, format!("{id}.start"), &e["start"])?;
                let b = point(&mut sk, &mut pts, format!("{id}.end"), &e["end"])?;
                ents.insert(id, sk.add_line_between(a, b).map_err(e_err)?);
            }
            Some(Curve::Circle(_, r)) => {
                let c = point(&mut sk, &mut pts, format!("{id}.center"), &e["center"])?;
                ents.insert(id, sk.add_circle_at(c, r).map_err(e_err)?);
            }
            Some(Curve::Seg(g @ Segment2::Arc { .. })) => {
                let Segment2::Arc { c, sweep, .. } = g else {
                    return err("arc");
                };
                let cv = e
                    .get("center")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({ "at": [c.x, c.y] }));
                let cp = point(&mut sk, &mut pts, format!("{id}.center"), &cv)?;
                let s0 = point(&mut sk, &mut pts, format!("{id}.start"), &e["start"])?;
                let e0 = match e.get("end") {
                    Some(v) => point(&mut sk, &mut pts, format!("{id}.end"), v)?,
                    None => {
                        let (_, t) = ends(&g).ok_or("arc")?;
                        let p = sk.add_point(t);
                        pts.insert(format!("{id}.end"), p);
                        p
                    }
                };
                // Our arcs run counter-clockwise from start to end.
                let (a, b) = if sweep >= 0.0 { (s0, e0) } else { (e0, s0) };
                ents.insert(id, sk.add_arc(cp, a, b).map_err(e_err)?);
            }
            _ => {
                if e["type"].as_str() == Some("point") {
                    let p = point(&mut sk, &mut pts, id.clone(), e)?;
                    pts.insert(format!("{id}.center"), p);
                }
            }
        }
    }
    let pt = |r: &Value| -> Result<PointId, String> {
        let r = r.as_str().ok_or("ref")?;
        if let Some(&p) = pts.get(r) {
            return Ok(p);
        }
        // A circle / arc given by name stands for its centre.
        pts.get(&format!("{r}.center"))
            .copied()
            .ok_or_else(|| format!("unknown point {r}"))
    };
    let ent = |r: &Value| -> Result<EntityId, String> {
        let r = r.as_str().ok_or("ref")?;
        ents.get(r)
            .copied()
            .ok_or_else(|| format!("unknown curve {r}"))
    };
    let is_ent = |r: &Value| r.as_str().is_some_and(|r| ents.contains_key(r));
    let pos = |sk: &Sk, p: PointId| sk.point(p).map(|x| x.position).unwrap_or(P2::origin());
    let mut list: Vec<Constraint> = Vec::new();
    let mut extra: Vec<Constraint> = Vec::new();
    let mut tangents = Vec::new();
    for c in f["constraints"].as_array().into_iter().flatten() {
        let r = c["refs"].as_array().ok_or("refs")?;
        let t = c["type"].as_str().unwrap_or_default();
        list.push(match t {
            "coincident" if is_ent(&r[1]) => {
                let (point, curve) = (pt(&r[0])?, ent(&r[1])?);
                match sk.entity(curve) {
                    Some(Entity::Line { .. }) => Constraint::PointOnLine { point, line: curve },
                    _ => Constraint::PointOnCircle {
                        point,
                        circle: curve,
                    },
                }
            }
            "coincident" => Constraint::Coincident {
                a: pt(&r[0])?,
                b: pt(&r[1])?,
            },
            "horizontal" => Constraint::Horizontal { line: ent(&r[0])? },
            "vertical" => Constraint::Vertical { line: ent(&r[0])? },
            "horizontal_points" => Constraint::HorizontalPoints {
                a: pt(&r[0])?,
                b: pt(&r[1])?,
            },
            "vertical_points" => Constraint::VerticalPoints {
                a: pt(&r[0])?,
                b: pt(&r[1])?,
            },
            "tangent" => {
                tangents.push((ent(&r[0])?, ent(&r[1])?));
                continue;
            }
            "equal" => Constraint::Equal {
                a: ent(&r[0])?,
                b: ent(&r[1])?,
            },
            "symmetric" => {
                // Two circles mirror each other: also the same size.
                if is_ent(&r[0]) && is_ent(&r[1]) {
                    let (ea, eb) = (ent(&r[0])?, ent(&r[1])?);
                    if !matches!(sk.entity(ea), Some(Entity::Line { .. })) {
                        extra.push(Constraint::Equal { a: ea, b: eb });
                    }
                }
                Constraint::Symmetric {
                    a: pt(&r[0])?,
                    b: pt(&r[1])?,
                    line: ent(&r[2])?,
                }
            }
            "midpoint" => Constraint::Midpoint {
                point: pt(&r[0])?,
                line: ent(&r[1])?,
            },
            "concentric" => Constraint::Concentric {
                a: ent(&r[0])?,
                b: ent(&r[1])?,
            },
            "perpendicular" => Constraint::Perpendicular {
                a: ent(&r[0])?,
                b: ent(&r[1])?,
            },
            "parallel" => Constraint::Parallel {
                a: ent(&r[0])?,
                b: ent(&r[1])?,
            },
            other => return err(format!("constraint '{other}'")),
        });
    }
    for d in f["dimensions"].as_array().into_iter().flatten() {
        let r = d["refs"].as_array().ok_or("refs")?;
        let v = num(ctx, &d["value"])?;
        let t = d["type"].as_str().unwrap_or_default();
        list.push(match t {
            "radius" => Constraint::Radius {
                circle: ent(&r[0])?,
                value: v,
            },
            "diameter" => Constraint::Diameter {
                circle: ent(&r[0])?,
                value: v,
            },
            "angle" => {
                let v = v.to_radians();
                let (a, b) = (ent(&r[0])?, ent(&r[1])?);
                // The angle between the lines' directions or its supplement,
                // whichever the sketched position is closer to.
                let dir = |l: EntityId| match sk.entity(l) {
                    Some(Entity::Line { start, end }) => pos(&sk, *end) - pos(&sk, *start),
                    _ => nalgebra::Vector2::new(1.0, 0.0),
                };
                // Signed, like our Angle: pick the sign and the supplement
                // the sketched position is closest to.
                let (da, db) = (dir(a), dir(b));
                let signed = |d: nalgebra::Vector2<Real>| da.perp(&d).atan2(da.dot(&d));
                let wrap = |x: Real| (x + PI).rem_euclid(TAU) - PI;
                let mut best = (Real::MAX, v, false);
                for supplement in [false, true] {
                    let now = signed(if supplement { -db } else { db });
                    for value in [v, -v] {
                        let d = wrap(now - value).abs();
                        if d < best.0 {
                            best = (d, value, supplement);
                        }
                    }
                }
                Constraint::Angle {
                    a,
                    b,
                    value: best.1,
                    supplement: best.2,
                }
            }
            "offset" => {
                let (line, point) = if is_ent(&r[0]) {
                    (ent(&r[0])?, pt(&r[1])?)
                } else {
                    (ent(&r[1])?, pt(&r[0])?)
                };
                Constraint::PointLineDistance {
                    point,
                    line,
                    value: v,
                }
            }
            "distance" => {
                let (a, b) = if r.len() == 1 {
                    match sk.entity(ent(&r[0])?) {
                        Some(Entity::Line { start, end }) => (*start, *end),
                        _ => return err("distance on a non-line"),
                    }
                } else if is_ent(&r[1]) && !is_ent(&r[0]) {
                    list.push(Constraint::PointLineDistance {
                        point: pt(&r[0])?,
                        line: ent(&r[1])?,
                        value: v,
                    });
                    continue;
                } else {
                    (pt(&r[0])?, pt(&r[1])?)
                };
                let (pa, pb) = (pos(&sk, a), pos(&sk, b));
                match d["orientation"].as_str() {
                    Some("horizontal") => {
                        let (a, b) = if pb.x >= pa.x { (a, b) } else { (b, a) };
                        Constraint::HorizontalDistance { a, b, value: v }
                    }
                    Some("vertical") => {
                        let (a, b) = if pb.y >= pa.y { (a, b) } else { (b, a) };
                        Constraint::VerticalDistance { a, b, value: v }
                    }
                    _ => Constraint::Distance { a, b, value: v },
                }
            }
            other => return err(format!("dimension '{other}'")),
        });
    }
    for c in list.into_iter().chain(extra) {
        sk.add_constraint(c).map_err(e_err)?;
    }
    for (a, b) in tangents {
        sk.add_tangent(a, b).map_err(e_err)?;
    }
    let rep = sk.solve().map_err(e_err)?;
    if rep.status != SolveStatus::Solved {
        return err(format!(
            "the sketch did not solve (residual {:.2e}, {} iterations, dof {}, redundant {})",
            rep.max_residual, rep.iterations, rep.dof, rep.redundant
        ));
    }
    // Read the solved geometry back.
    for (id, e) in &ents {
        let g = match sk.entity(*e) {
            Some(Entity::Line { start, end }) => Curve::Seg(Segment2::Line {
                a: pos(&sk, *start),
                b: pos(&sk, *end),
            }),
            Some(Entity::Circle { center, radius }) => Curve::Circle(pos(&sk, *center), *radius),
            Some(Entity::Arc { center, start, end }) => {
                let (c, s, t) = (pos(&sk, *center), pos(&sk, *start), pos(&sk, *end));
                let a0 = (s.y - c.y).atan2(s.x - c.x);
                let mut sweep = ((t.y - c.y).atan2(t.x - c.x) - a0).rem_euclid(TAU);
                if sweep < 1e-9 {
                    sweep = TAU;
                }
                Curve::Seg(Segment2::Arc {
                    c,
                    r: (s - c).norm(),
                    a0,
                    sweep,
                })
            }
            _ => continue,
        };
        curves.insert(id.clone(), g);
    }
    Ok(())
}

/// One loop of curves, chained end to start.
fn chain(sk: &Sketch, ids: &[Value]) -> Result<Vec<ProfileSegment>, String> {
    let named = |g: Segment2, id: &str| ProfileSegment {
        geometry: g,
        name: format!("{id}:0"),
    };
    let mut rest: Vec<(String, Segment2)> = Vec::new();
    for id in ids {
        let id = id.as_str().ok_or("curve id")?;
        match sk
            .curves
            .get(id)
            .ok_or_else(|| format!("unknown curve {id}"))?
        {
            Curve::Circle(c, r) => {
                if ids.len() != 1 {
                    return err("a circle in a loop of several curves");
                }
                return Ok(vec![
                    named(
                        Segment2::Arc {
                            c: *c,
                            r: *r,
                            a0: 0.0,
                            sweep: PI,
                        },
                        id,
                    ),
                    ProfileSegment {
                        geometry: Segment2::Arc {
                            c: *c,
                            r: *r,
                            a0: PI,
                            sweep: PI,
                        },
                        name: format!("{id}:1"),
                    },
                ]);
            }
            Curve::Seg(s) => rest.push((id.to_owned(), s.clone())),
        }
    }
    let close = |a: P2, b: P2| (a - b).norm() < 1e-6;
    let (id0, s0) = rest.remove(0);
    let (_, mut at) = ends(&s0).ok_or("curve kind")?;
    let mut out = vec![named(s0, &id0)];
    while !rest.is_empty() {
        let k = rest
            .iter()
            .position(|(_, s)| ends(s).is_some_and(|(a, b)| close(a, at) || close(b, at)))
            .ok_or_else(|| {
                let gaps: Vec<String> = rest
                    .iter()
                    .filter_map(|(id, s)| {
                        ends(s).map(|(a, b)| {
                            format!(
                                "{id} {:.3?}->{:.3?}",
                                a.coords.as_slice(),
                                b.coords.as_slice()
                            )
                        })
                    })
                    .collect();
                format!(
                    "the loop is not closed at {:.3?}; left: {}",
                    at.coords.as_slice(),
                    gaps.join(", ")
                )
            })?;
        let (id, s) = rest.remove(k);
        let s = match ends(&s) {
            Some((a, _)) if close(a, at) => s,
            _ => reversed(&s),
        };
        at = ends(&s).map(|e| e.1).ok_or("curve kind")?;
        out.push(named(s, &id));
    }
    Ok(out)
}

/// The regions of a profile entry: each outer loop with the holes after it.
fn regions(ctx: &Ctx, prof: &Value) -> Result<(String, Vec<exact::ExactProfile>), String> {
    let name = prof["sketch"].as_str().ok_or("profile sketch")?.to_owned();
    let sk = ctx
        .sketches
        .get(&name)
        .ok_or_else(|| format!("unknown sketch {name}"))?;
    let mut groups: Vec<(Vec<ProfileSegment>, Vec<Vec<ProfileSegment>>)> = Vec::new();
    for lp in prof["loops"].as_array().ok_or("loops")? {
        let segs = chain(sk, lp["curves"].as_array().ok_or("curves")?)?;
        if lp["outer"].as_bool().unwrap_or(true) {
            groups.push((segs, Vec::new()));
        } else {
            groups
                .last_mut()
                .ok_or("a hole before its outer loop")?
                .1
                .push(segs);
        }
    }
    let regions = groups
        .iter()
        .map(|(o, h)| exact::profile(o, h))
        .collect::<Result<_, _>>()?;
    Ok((name, regions))
}

fn union_all(mut v: Vec<ExactSolid>) -> Result<ExactSolid, String> {
    let mut acc = v.pop().ok_or("nothing built")?;
    for s in v {
        acc = exact::boolean(&acc, &s, ExactOp::Union)?.ok_or("empty union")?;
    }
    Ok(acc)
}

/// Applies a feature's operation to the bodies.
fn apply(ctx: &mut Ctx, tool: ExactSolid, op: &str, names: &[String]) -> Result<(), String> {
    match op {
        "new" => {
            let name = names
                .first()
                .cloned()
                .unwrap_or_else(|| format!("Body{}", ctx.bodies.len() + 1));
            ctx.bodies.push((name, tool));
        }
        "join" => {
            let hits: Vec<usize> = (0..ctx.bodies.len())
                .filter(|&i| exact::meets(&ctx.bodies[i].1, &tool))
                .collect();
            if hits.is_empty() {
                let name = names.first().cloned().unwrap_or_else(|| "Joined".into());
                ctx.bodies.push((name, tool));
                return Ok(());
            }
            let mut acc = exact::boolean(&ctx.bodies[hits[0]].1, &tool, ExactOp::Union)?
                .ok_or("empty join")?;
            for &i in hits.iter().skip(1) {
                acc =
                    exact::boolean(&acc, &ctx.bodies[i].1, ExactOp::Union)?.ok_or("empty join")?;
            }
            for &i in hits.iter().skip(1).rev() {
                ctx.bodies.remove(i);
            }
            ctx.bodies[hits[0]].1 = acc;
        }
        "cut" | "intersect" => {
            let mut keep = Vec::new();
            for (name, b) in ctx.bodies.drain(..) {
                let touches = exact::meets(&b, &tool);
                let r = match (op, touches) {
                    ("cut", false) => Some(b),
                    ("cut", true) => exact::boolean(&b, &tool, ExactOp::Cut)?,
                    (_, false) => None,
                    (_, true) => exact::boolean(&b, &tool, ExactOp::Intersect)?,
                };
                if let Some(r) = r {
                    keep.push((name, r));
                }
            }
            ctx.bodies = keep;
        }
        other => return err(format!("operation '{other}'")),
    }
    Ok(())
}

fn names(f: &Value) -> Vec<String> {
    f["result_bodies"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// The range (start, end) along the sketch normal.
fn extent(ctx: &Ctx, plane: &Plane, e: &Value) -> Result<(Real, Real), String> {
    let sign = if e["direction"].as_str() == Some("negative") {
        -1.0
    } else {
        1.0
    };
    Ok(match e["type"].as_str().unwrap_or_default() {
        "distance" => (0.0, sign * num(ctx, &e["distance"])?),
        "symmetric" => {
            let d = num(ctx, &e["distance"])?;
            let d = if e["is_full_length"].as_bool() == Some(true) {
                d / 2.0
            } else {
                d
            };
            (-d, d)
        }
        "two_sides" => (-num(ctx, &e["side2_opposite"])?, num(ctx, &e["side1"])?),
        "through_all" => {
            let n = plane.normal.into_inner();
            let mut far: Real = 0.0;
            for (_, b) in &ctx.bodies {
                let m = b.measure_full()?;
                for i in 0..8 {
                    let c = Point3::new(
                        if i & 1 == 0 { m.bbox.0.x } else { m.bbox.1.x },
                        if i & 2 == 0 { m.bbox.0.y } else { m.bbox.1.y },
                        if i & 4 == 0 { m.bbox.0.z } else { m.bbox.1.z },
                    );
                    far = far.max((c - plane.origin).dot(&n) * sign);
                }
            }
            if far <= 0.0 {
                return err("through all: no body in that direction");
            }
            (0.0, sign * (far + 1.0))
        }
        other => return err(format!("extent '{other}'")),
    })
}

/// Index of the body nearest `p` (inside its box, grown a little).
fn body_at(ctx: &Ctx, p: Point3) -> Result<usize, String> {
    for (i, (_, b)) in ctx.bodies.iter().enumerate() {
        let m = b.measure_full()?;
        let tol = 1e-3 * (m.bbox.1 - m.bbox.0).norm() + 1e-6;
        let inside = (0..3).all(|k| p[k] >= m.bbox.0[k] - tol && p[k] <= m.bbox.1[k] + tol);
        if inside {
            return Ok(i);
        }
    }
    err("no body at the edge point")
}

fn edge_points(f: &Value) -> Result<Vec<Point3>, String> {
    f["edges"]
        .as_array()
        .ok_or("edges")?
        .iter()
        .map(|e| v3(&e["point"]).map(Point3::from))
        .collect()
}

fn blend(ctx: &mut Ctx, f: &Value, chamfer: bool) -> Result<(), String> {
    let pts = edge_points(f)?;
    let mut by_body: HashMap<usize, Vec<Point3>> = HashMap::new();
    for p in pts {
        by_body.entry(body_at(ctx, p)?).or_default().push(p);
    }
    let name = f["name"].as_str().unwrap_or("Blend").to_owned();
    for (i, pts) in by_body {
        let b = &ctx.bodies[i].1;
        let r = if chamfer {
            exact::chamfer(
                b,
                &pts,
                num(ctx, &f["distance"])?,
                ChamferSecond::Same,
                false,
                &name,
            )?
        } else {
            exact::fillet(b, &pts, num(ctx, &f["radius"])?, &name)?
        };
        ctx.bodies[i].1 = r;
    }
    Ok(())
}

/// Builds the recipe. `Ok(None)`: done; `Ok(Some(why))`: unsupported.
fn build(ctx: &mut Ctx, recipe: &Value) -> Result<Option<String>, String> {
    for p in recipe["parameters"].as_array().into_iter().flatten() {
        let v = num(ctx, &p["expression"])?;
        ctx.params
            .insert(p["name"].as_str().unwrap_or_default().to_owned(), v);
    }
    for f in recipe["features"].as_array().ok_or("features")? {
        let op = f["op"].as_str().unwrap_or_default();
        let name = f["name"].as_str().unwrap_or(op).to_owned();
        match op {
            "sketch" => {
                let sk = sketch(ctx, f)?;
                ctx.sketches.insert(name, sk);
            }
            "construction_plane" => {}
            "extrude" => {
                let mut tools = Vec::new();
                for prof in f["profiles"].as_array().ok_or("profiles")? {
                    let (sk, regs) = regions(ctx, prof)?;
                    let plane = ctx.sketches[&sk].plane.clone();
                    let (a, b) = extent(ctx, &plane, &f["extent"])?;
                    let taper = match f["extent"].get("taper_deg") {
                        Some(t) => num(ctx, t)?.to_radians(),
                        None => 0.0,
                    };
                    for r in &regs {
                        tools.push(exact::extrude(&plane, r, a, b, taper, &name)?);
                    }
                }
                let tool = union_all(tools)?;
                apply(
                    ctx,
                    tool,
                    f["operation"].as_str().unwrap_or("new"),
                    &names(f),
                )?;
            }
            "revolve" => {
                let (sk, regs) = regions(ctx, &f["profile"])?;
                let plane = ctx.sketches[&sk].plane.clone();
                let o = Point3::from(v3(&f["axis"]["origin"])?);
                let d = v3(&f["axis"]["dir"])?;
                let (o2, e2) = (plane.project(&o), plane.project(&(o + d)));
                let deg = num(ctx, &f["angle"])?;
                let angle = if deg >= 360.0 - 1e-9 {
                    TAU
                } else {
                    deg.to_radians()
                };
                let mut tools = Vec::new();
                for r in &regs {
                    tools.push(exact::revolve(&plane, r, o2, e2 - o2, 0.0, angle, &name)?);
                }
                let tool = union_all(tools)?;
                apply(
                    ctx,
                    tool,
                    f["operation"].as_str().unwrap_or("new"),
                    &names(f),
                )?;
            }
            "fillet" => {
                if f["type"].as_str() != Some("constant_radius") {
                    return Ok(Some(format!("fillet type {}", f["type"])));
                }
                blend(ctx, f, false)?;
            }
            "chamfer" => {
                if f["type"].as_str() != Some("equal_distance") {
                    return Ok(Some(format!("chamfer type {}", f["type"])));
                }
                blend(ctx, f, true)?;
            }
            "combine" => {
                let target = f["target_body"].as_str().ok_or("target_body")?;
                let tools: Vec<String> = f["tool_bodies"]
                    .as_array()
                    .ok_or("tool_bodies")?
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect();
                let kop = match f["operation"].as_str() {
                    Some("join") => ExactOp::Union,
                    Some("cut") => ExactOp::Cut,
                    Some("intersect") => ExactOp::Intersect,
                    other => return err(format!("combine {other:?}")),
                };
                let ti = ctx
                    .bodies
                    .iter()
                    .position(|(n, _)| n == target)
                    .ok_or_else(|| format!("no body {target}"))?;
                let mut acc = Some(ctx.bodies[ti].1.clone());
                for t in &tools {
                    let b = &ctx
                        .bodies
                        .iter()
                        .find(|(n, _)| n == t)
                        .ok_or_else(|| format!("no body {t}"))?
                        .1;
                    if let Some(a) = acc {
                        acc = exact::boolean(&a, b, kop)?;
                    }
                }
                match acc {
                    Some(a) => ctx.bodies[ti].1 = a,
                    None => {
                        ctx.bodies.remove(ti);
                    }
                }
                if f["keep_tools"].as_bool() != Some(true) {
                    ctx.bodies.retain(|(n, _)| !tools.contains(n));
                }
            }
            other => return Ok(Some(format!("op '{other}'"))),
        }
    }
    Ok(None)
}

/// Compares the built bodies with `expected.json`.
fn compare(ctx: &Ctx, exp: &Value) -> Result<(), String> {
    let mut volume = 0.0;
    let mut area = 0.0;
    let mut moment = Vector3::zeros();
    let mut lo = Point3::new(Real::MAX, Real::MAX, Real::MAX);
    let mut hi = Point3::new(Real::MIN, Real::MIN, Real::MIN);
    let mut faces = 0;
    let mut count = 0;
    for (_, b) in &ctx.bodies {
        let m = b.measure_full()?;
        volume += m.volume;
        area += m.area;
        moment += m.centroid.coords * m.volume;
        lo = lo.inf(&m.bbox.0);
        hi = hi.sup(&m.bbox.1);
        faces += m.faces;
        count += m.lumps;
    }
    let only: Option<Vec<&str>> = exp["compare"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect());
    let want = |k: &str| only.as_ref().is_none_or(|v| v.contains(&k)) && !exp[k].is_null();
    let rel = exp["tolerance"]["rel"].as_f64().unwrap_or(1e-3);
    let mut bad = String::new();
    if want("body_count") && exp["body_count"].as_u64() != Some(count as u64) {
        let _ = write!(bad, " bodies {count} (want {})", exp["body_count"]);
    }
    let close = |g: Real, w: Real| (g - w).abs() <= rel * w.abs().max(1.0);
    for (k, g) in [("volume_mm3", volume), ("area_mm2", area)] {
        if want(k)
            && let Some(w) = exp[k].as_f64()
            && !close(g, w)
        {
            let _ = write!(bad, " {k} {g:.3} (want {w:.3})");
        }
    }
    if count > 0 {
        let diag = (hi - lo).norm();
        let tol = exp["tolerance"]["abs_mm"].as_f64().unwrap_or(1e-3 * diag);
        let c = moment / volume;
        if want("center_of_mass")
            && let Ok(w) = v3(&exp["center_of_mass"])
            && (c - w).amax() > tol
        {
            let _ = write!(
                bad,
                " centre {:.3?} (want {:.3?})",
                c.as_slice(),
                w.as_slice()
            );
        }
        if want("bbox")
            && let (Ok(wl), Ok(wh)) = (v3(&exp["bbox"]["min"]), v3(&exp["bbox"]["max"]))
            && ((lo.coords - wl).amax() > tol || (hi.coords - wh).amax() > tol)
        {
            let _ = write!(
                bad,
                " bbox {:.3?}..{:.3?}",
                lo.coords.as_slice(),
                hi.coords.as_slice()
            );
        }
    }
    if want("faces")
        && let Some(w) = exp["faces"].as_u64()
        && w != faces as u64
    {
        let _ = write!(bad, " faces {faces} (want {w})");
    }
    if bad.is_empty() {
        Ok(())
    } else {
        err(bad.trim().to_owned())
    }
}

fn run_case(dir: &Path) -> Outcome {
    let read = |n: &str| -> Result<Value, String> {
        let t = std::fs::read_to_string(dir.join(n)).map_err(|e| e.to_string())?;
        serde_json::from_str(&t).map_err(|e| e.to_string())
    };
    let (recipe, exp) = match (read("recipe.json"), read("expected.json")) {
        (Ok(r), Ok(e)) => (r, e),
        (Err(e), _) | (_, Err(e)) => return Outcome::Fail(e),
    };
    let mut ctx = Ctx {
        params: HashMap::new(),
        sketches: HashMap::new(),
        bodies: Vec::new(),
    };
    match build(&mut ctx, &recipe) {
        Ok(Some(why)) => Outcome::Skip(why),
        Err(e) => Outcome::Fail(format!("build: {e}")),
        Ok(None) => match compare(&ctx, &exp) {
            Ok(()) => Outcome::Pass,
            Err(e) => Outcome::Fail(e),
        },
    }
}

#[test]
fn oracle_cases() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/oracle");
    let known: HashMap<String, String> = std::fs::read_to_string(root.join("known_failures.txt"))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .map(|l| match l.split_once('#') {
            Some((id, why)) => (id.trim().to_owned(), why.trim().to_owned()),
            None => (l.trim().to_owned(), String::new()),
        })
        .collect();
    let mut dirs: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    let (mut pass, mut skip, mut fail) = (0, 0, 0);
    let mut problems = Vec::new();
    for d in &dirs {
        let id = d.file_name().unwrap().to_string_lossy().to_string();
        let out = run_case(d);
        let (tag, why) = match &out {
            Outcome::Pass => ("pass", String::new()),
            Outcome::Skip(w) => ("skip", w.clone()),
            Outcome::Fail(w) => ("FAIL", w.clone()),
        };
        println!("{tag:4} {id:34} {why}");
        match out {
            Outcome::Pass => {
                pass += 1;
                if known.contains_key(&id) {
                    problems.push(format!(
                        "{id} passes now: remove it from known_failures.txt"
                    ));
                }
            }
            Outcome::Skip(_) => skip += 1,
            Outcome::Fail(w) => {
                fail += 1;
                if !known.contains_key(&id) {
                    problems.push(format!("{id}: {w}"));
                }
            }
        }
    }
    println!(
        "oracle: {pass} pass, {fail} fail ({} known), {skip} skipped",
        known.len()
    );
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
