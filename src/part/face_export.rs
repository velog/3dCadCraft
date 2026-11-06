//! Export Face As (`docs/spec/7.7-export.md` §3a): the loops of one or more
//! planar faces as a 2D drawing in **DXF** (R12) or **SVG**, for laser and
//! water-jet cutting, CNC and vector editors.
//!
//! The faces are seen from outside the body (the first face's outward
//! normal towards the viewer). Several faces must be parallel; they are
//! projected into the first face's plane, and edges two selected faces
//! share are left out, so touching faces give one outline. Lines, arcs and
//! whole circles stay exact; other curves become points within 0.01 mm.
//! As in Inventor, the drawing is moved so every coordinate is positive.

use std::collections::HashMap;

use crate::kernel::{
    BRep, Curve, FaceId, Point3, Real, Sense, Surface, TessellationOptions, Vector3, VertexId,
};

/// One piece of a loop in the drawing's plane (mm).
#[derive(Clone, Debug, PartialEq)]
pub enum Seg {
    /// A straight piece from `.0` to `.1`.
    Line([Real; 2], [Real; 2]),
    /// An arc from angle `start` turning `sweep` (radians, positive =
    /// counter-clockwise).
    Arc {
        /// Centre.
        center: [Real; 2],
        /// Radius.
        radius: Real,
        /// Start angle.
        start: Real,
        /// Turn.
        sweep: Real,
    },
    /// Any other curve, as points (both ends included).
    Curve(Vec<[Real; 2]>),
}

impl Seg {
    fn start(&self) -> [Real; 2] {
        match self {
            Seg::Line(a, _) => *a,
            Seg::Arc {
                center,
                radius,
                start,
                ..
            } => polar(*center, *radius, *start),
            Seg::Curve(p) => p.first().copied().unwrap_or([0.0; 2]),
        }
    }

    fn reversed(self) -> Seg {
        match self {
            Seg::Line(a, b) => Seg::Line(b, a),
            Seg::Arc {
                center,
                radius,
                start,
                sweep,
            } => Seg::Arc {
                center,
                radius,
                start: start + sweep,
                sweep: -sweep,
            },
            Seg::Curve(mut p) => {
                p.reverse();
                Seg::Curve(p)
            }
        }
    }

    fn shifted(&mut self, d: [Real; 2]) {
        let mv = |p: &mut [Real; 2]| {
            p[0] += d[0];
            p[1] += d[1];
        };
        match self {
            Seg::Line(a, b) => {
                mv(a);
                mv(b);
            }
            Seg::Arc { center, .. } => mv(center),
            Seg::Curve(p) => p.iter_mut().for_each(mv),
        }
    }

    /// Points along the piece (start included, end left out).
    fn points(&self, out: &mut Vec<[Real; 2]>) {
        match self {
            Seg::Line(a, _) => out.push(*a),
            Seg::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                let n = ((sweep.abs() / (std::f64::consts::PI / 36.0)).ceil() as usize).max(2);
                for k in 0..n {
                    out.push(polar(
                        *center,
                        *radius,
                        start + sweep * k as Real / n as Real,
                    ));
                }
            }
            Seg::Curve(p) => out.extend(p.iter().take(p.len().saturating_sub(1))),
        }
    }
}

fn polar(c: [Real; 2], r: Real, a: Real) -> [Real; 2] {
    [c[0] + r * a.cos(), c[1] + r * a.sin()]
}

/// A closed loop of the drawing.
#[derive(Clone, Debug, PartialEq)]
pub struct Loop2 {
    /// Pieces end to end: counter-clockwise for an outline, clockwise for a
    /// hole.
    pub segs: Vec<Seg>,
    /// `true` for a hole (inside an odd number of other loops).
    pub inner: bool,
}

impl Loop2 {
    /// The whole circle this loop is, if it is one: `(centre, radius)`.
    pub fn circle(&self) -> Option<([Real; 2], Real)> {
        match self.segs.as_slice() {
            [
                Seg::Arc {
                    center,
                    radius,
                    sweep,
                    ..
                },
            ] if (sweep.abs() - std::f64::consts::TAU).abs() < 1e-9 => Some((*center, *radius)),
            _ => None,
        }
    }

    /// A polyline of the loop (closed, last point not repeated).
    pub fn polyline(&self) -> Vec<[Real; 2]> {
        let mut out = Vec::new();
        for s in &self.segs {
            s.points(&mut out);
        }
        out
    }

    /// Signed area (positive = counter-clockwise), arcs exact.
    pub fn area(&self) -> Real {
        let mut a = 0.0;
        for s in &self.segs {
            a += match s {
                Seg::Line(p, q) => 0.5 * (p[0] * q[1] - q[0] * p[1]),
                // Green's theorem over the arc: the chord-free form.
                Seg::Arc {
                    center: c,
                    radius: r,
                    start: t0,
                    sweep: dt,
                } => {
                    let t1 = t0 + dt;
                    0.5 * (r * r * dt + c[0] * r * (t1.sin() - t0.sin())
                        - c[1] * r * (t1.cos() - t0.cos()))
                }
                Seg::Curve(p) => p
                    .windows(2)
                    .map(|w| 0.5 * (w[0][0] * w[1][1] - w[1][0] * w[0][1]))
                    .sum(),
            };
        }
        a
    }
}

/// The faces' outline in the drawing's plane, moved to positive coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct FaceOutline {
    /// Outlines first, then holes.
    pub loops: Vec<Loop2>,
    /// Width and height of the drawing (its lower left corner is 0, 0).
    pub size: [Real; 2],
}

/// The planar faces named `names` of `body` as one 2D outline. Every piece
/// of a name counts (a Boolean can leave a plane in several pieces).
pub fn outline(body: &BRep, names: &[String]) -> Result<FaceOutline, String> {
    if names.is_empty() {
        return Err("select one or more planar faces".into());
    }
    let mut faces: Vec<FaceId> = Vec::new();
    for n in names {
        let found: Vec<FaceId> = body
            .faces()
            .filter(|(_, f)| f.name.as_deref() == Some(n.as_str()))
            .map(|(id, _)| id)
            .collect();
        if found.is_empty() {
            return Err(format!("{n} is not in the model"));
        }
        for f in found {
            if !faces.contains(&f) {
                faces.push(f);
            }
        }
    }
    let name_of = |f: FaceId| {
        body.face(f)
            .and_then(|x| x.name.clone())
            .unwrap_or_else(|| "a face".into())
    };
    // The frame: the first face seen from outside.
    let planar = |f: FaceId| -> Option<Vector3> {
        let face = body.face(f)?;
        if let Some(s) = face.facet_of
            && !matches!(body.surface(s), Some(Surface::Plane(_)))
        {
            return None;
        }
        let n = body.face_plane(f)?.normal.into_inner();
        // Free-form faces carry a placeholder plane: their mesh says.
        let flat = face.mesh.as_ref().is_none_or(|m| {
            m.normals
                .first()
                .is_none_or(|n0| m.normals.iter().all(|n| n.dot(n0) > 1.0 - 1e-6))
        });
        flat.then_some(n)
    };
    let n =
        planar(faces[0]).ok_or_else(|| format!("{} is not a planar face", name_of(faces[0])))?;
    let mut x = Vector3::x() - n * n.x;
    if x.norm() < 1e-6 {
        x = Vector3::y() - n * n.y;
    }
    let x = x.normalize();
    let y = n.cross(&x);
    let to2 = |p: Point3| [p.coords.dot(&x), p.coords.dot(&y)];

    // Every coedge of every face, as a piece between two vertices; an edge
    // two selected faces share is inside the outline and goes.
    let mut uses: HashMap<crate::kernel::EdgeId, usize> = HashMap::new();
    let mut pieces: Vec<(VertexId, VertexId, Seg, crate::kernel::EdgeId)> = Vec::new();
    let opts = TessellationOptions::default();
    for &f in &faces {
        let fnorm = planar(f).ok_or_else(|| format!("{} is not a planar face", name_of(f)))?;
        let d = fnorm.dot(&n);
        if d.abs() < 1.0 - 1e-6 {
            return Err(format!(
                "{} is not parallel to {}",
                name_of(f),
                name_of(faces[0])
            ));
        }
        let Some(face) = body.face(f) else { continue };
        let loops: Vec<_> = std::iter::once(&face.outer).chain(&face.inner).collect();
        if loops.iter().all(|l| l.coedges.is_empty()) {
            return Err(format!("{} has no edges (a mesh body)", name_of(f)));
        }
        for lp in loops {
            for ce in &lp.coedges {
                let Some(e) = body.edge(ce.edge) else {
                    continue;
                };
                let Some(curve) = body.curve(e.curve) else {
                    continue;
                };
                let seg = match curve {
                    Curve::Line(_) => {
                        let (Some(a), Some(b)) = (body.vertex(e.start), body.vertex(e.end)) else {
                            continue;
                        };
                        Seg::Line(to2(a.point), to2(b.point))
                    }
                    Curve::Circle(c) => {
                        let s = c.normal.dot(&n).signum();
                        let xa = c.x_axis.into_inner();
                        let phi = xa.dot(&y).atan2(xa.dot(&x));
                        Seg::Arc {
                            center: to2(c.center),
                            radius: c.radius,
                            start: phi + s * e.range.min,
                            sweep: s * (e.range.max - e.range.min),
                        }
                    }
                    Curve::BSpline(_) => {
                        let pts = body
                            .sample_edge(ce.edge, &opts)
                            .map_err(|e| e.to_string())?;
                        Seg::Curve(pts.into_iter().map(to2).collect())
                    }
                };
                let (a, b, seg) = match ce.sense {
                    Sense::Same => (e.start, e.end, seg),
                    Sense::Reversed => (e.end, e.start, seg.reversed()),
                };
                *uses.entry(ce.edge).or_default() += 1;
                pieces.push((a, b, seg, ce.edge));
            }
        }
    }
    pieces.retain(|p| uses.get(&p.3).copied().unwrap_or(0) < 2);

    // Chain the pieces into loops by their vertices.
    let mut loops: Vec<Vec<Seg>> = Vec::new();
    let mut used = vec![false; pieces.len()];
    for i in 0..pieces.len() {
        if used[i] {
            continue;
        }
        used[i] = true;
        let first = pieces[i].0;
        let mut at = pieces[i].1;
        let mut segs = vec![pieces[i].2.clone()];
        while at != first {
            // Forwards first; a piece the other way round (an open sheet's
            // free edge of another sense) is turned.
            let next = (0..pieces.len())
                .find(|&k| !used[k] && pieces[k].0 == at)
                .map(|k| (k, false))
                .or_else(|| {
                    (0..pieces.len())
                        .find(|&k| !used[k] && pieces[k].1 == at)
                        .map(|k| (k, true))
                });
            let Some((k, turn)) = next else {
                return Err("a loop of the faces is not closed".into());
            };
            used[k] = true;
            let seg = pieces[k].2.clone();
            if turn {
                segs.push(seg.reversed());
                at = pieces[k].0;
            } else {
                segs.push(seg);
                at = pieces[k].1;
            }
        }
        loops.push(merge(segs));
    }
    if loops.is_empty() {
        return Err("the faces have no outline".into());
    }
    // Parallel faces can project onto one another (a pocket's floor onto
    // its opening): a loop lying on another is drawn once, or a cutter
    // goes round it twice.
    let mut kept: Vec<Vec<Seg>> = Vec::new();
    for segs in loops {
        let lp = Loop2 { segs, inner: false };
        let dup = kept.iter().any(|k| {
            same_loop(
                &Loop2 {
                    segs: k.clone(),
                    inner: false,
                },
                &lp,
            )
        });
        if !dup {
            kept.push(lp.segs);
        }
    }
    let loops = kept;

    // Holes are inside an odd number of other loops; outlines turn
    // counter-clockwise, holes clockwise.
    let polys: Vec<Vec<[Real; 2]>> = loops
        .iter()
        .map(|s| {
            Loop2 {
                segs: s.clone(),
                inner: false,
            }
            .polyline()
        })
        .collect();
    let mut out: Vec<Loop2> = Vec::new();
    for (i, segs) in loops.into_iter().enumerate() {
        let probe = polys[i].first().copied().unwrap_or([0.0; 2]);
        let depth = (0..polys.len())
            .filter(|&k| k != i && inside(&polys[k], probe))
            .count();
        let mut lp = Loop2 {
            segs,
            inner: depth % 2 == 1,
        };
        if (lp.area() > 0.0) == lp.inner {
            lp.segs = lp.segs.into_iter().rev().map(Seg::reversed).collect();
        }
        out.push(lp);
    }
    out.sort_by_key(|l| l.inner);

    // Positive coordinates (Inventor moves the drawing so).
    let (mut lo, mut hi) = ([Real::MAX; 2], [Real::MIN; 2]);
    for p in out.iter().flat_map(|l| l.polyline()) {
        for k in 0..2 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    // Arc extremes the polyline's chords miss.
    for l in &out {
        for s in &l.segs {
            if let Seg::Arc { center, radius, .. } = s {
                for k in 0..4 {
                    let a = k as Real * std::f64::consts::FRAC_PI_2;
                    if arc_contains(s, a) {
                        let p = polar(*center, *radius, a);
                        for j in 0..2 {
                            lo[j] = lo[j].min(p[j]);
                            hi[j] = hi[j].max(p[j]);
                        }
                    }
                }
            }
        }
    }
    let d = [-lo[0], -lo[1]];
    for l in &mut out {
        for s in &mut l.segs {
            s.shifted(d);
        }
    }
    Ok(FaceOutline {
        loops: out,
        size: [hi[0] - lo[0], hi[1] - lo[1]],
    })
}

/// Whether two loops are the same curve (either way round): equal areas and
/// every point of one on the other's polyline.
fn same_loop(a: &Loop2, b: &Loop2) -> bool {
    let tol = 1e-6;
    if (a.area().abs() - b.area().abs()).abs() > tol * (1.0 + a.area().abs()) {
        return false;
    }
    let (pa, pb) = (a.polyline(), b.polyline());
    let near = |p: [Real; 2], poly: &[[Real; 2]]| {
        let n = poly.len();
        (0..n).any(|i| {
            let (u, v) = (poly[i], poly[(i + 1) % n]);
            let d = [v[0] - u[0], v[1] - u[1]];
            let l2 = d[0] * d[0] + d[1] * d[1];
            let t = if l2 > 0.0 {
                (((p[0] - u[0]) * d[0] + (p[1] - u[1]) * d[1]) / l2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let q = [u[0] + d[0] * t, u[1] + d[1] * t];
            (p[0] - q[0]).hypot(p[1] - q[1]) < 1e-3
        })
    };
    pa.iter().all(|&p| near(p, &pb)) && pb.iter().all(|&p| near(p, &pa))
}

/// Whether arc `s` passes angle `a`.
fn arc_contains(s: &Seg, a: Real) -> bool {
    let Seg::Arc { start, sweep, .. } = s else {
        return false;
    };
    let tau = std::f64::consts::TAU;
    let (lo, len) = if *sweep >= 0.0 {
        (*start, *sweep)
    } else {
        (start + sweep, -sweep)
    };
    (a - lo).rem_euclid(tau) <= len + 1e-12
}

/// Even-odd point in polygon.
fn inside(poly: &[[Real; 2]], p: [Real; 2]) -> bool {
    let mut c = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a[1] > p[1]) != (b[1] > p[1]) {
            let x = a[0] + (p[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1]);
            if p[0] < x {
                c = !c;
            }
        }
    }
    c
}

/// Joins neighbouring lines along one line and arcs of one circle (the
/// kernel splits a circle in halves and a Boolean splits straight edges).
fn merge(segs: Vec<Seg>) -> Vec<Seg> {
    let join = |a: &Seg, b: &Seg| -> Option<Seg> {
        match (a, b) {
            (Seg::Line(p, q), Seg::Line(_, r)) => {
                let u = [q[0] - p[0], q[1] - p[1]];
                let v = [r[0] - q[0], r[1] - q[1]];
                let (lu, lv) = (u[0].hypot(u[1]), v[0].hypot(v[1]));
                let cross = (u[0] * v[1] - u[1] * v[0]).abs();
                let dot = u[0] * v[0] + u[1] * v[1];
                (dot > 0.0 && cross <= 1e-9 * lu * lv + 1e-12).then_some(Seg::Line(*p, *r))
            }
            (
                Seg::Arc {
                    center: c0,
                    radius: r0,
                    start,
                    sweep: s0,
                },
                Seg::Arc {
                    center: c1,
                    radius: r1,
                    sweep: s1,
                    ..
                },
            ) => {
                let tol = 1e-7 * (1.0 + r0);
                let same = (c0[0] - c1[0]).hypot(c0[1] - c1[1]) <= tol
                    && (r0 - r1).abs() <= tol
                    && s0.signum() == s1.signum()
                    && (s0 + s1).abs() <= std::f64::consts::TAU + 1e-9;
                same.then(|| Seg::Arc {
                    center: *c0,
                    radius: *r0,
                    start: *start,
                    sweep: s0 + s1,
                })
            }
            _ => None,
        }
    };
    let mut out: Vec<Seg> = Vec::new();
    for s in segs {
        let joined = out.last().and_then(|last| join(last, &s));
        match (joined, out.last_mut()) {
            (Some(j), Some(last)) => *last = j,
            _ => out.push(s),
        }
    }
    // Wrap round: the last piece may continue into the first.
    while out.len() > 1 {
        let (Some(last), Some(first)) = (out.last(), out.first()) else {
            break;
        };
        match join(last, first) {
            Some(j) => {
                out.pop();
                out[0] = j;
            }
            None => break,
        }
    }
    out
}

/// The output format of Export Face As.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaceFormat {
    /// AutoCAD DXF R12 (ASCII).
    Dxf,
    /// SVG 1.1, millimetres.
    Svg,
}

impl FaceFormat {
    /// The format of a file name by its extension (`None`: neither).
    pub fn of_path(path: &std::path::Path) -> Option<FaceFormat> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "dxf" => Some(FaceFormat::Dxf),
            "svg" => Some(FaceFormat::Svg),
            _ => None,
        }
    }
}

/// Layer (DXF) / group (SVG) of a loop.
fn layer(l: &Loop2) -> &'static str {
    if l.inner { "INNER" } else { "OUTER" }
}

fn num(v: Real) -> String {
    let v = if v.abs() < 5e-7 { 0.0 } else { v };
    format!("{v:.6}")
}

/// The outline as DXF R12 text: each loop a closed POLYLINE on layer
/// `OUTER` or `INNER`, arcs as bulges (exact), whole circles as CIRCLEs.
pub fn dxf(o: &FaceOutline) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    let mut put = |code: i32, v: &str| {
        let _ = writeln!(s, "{code}\n{v}");
    };
    put(0, "SECTION");
    put(2, "HEADER");
    put(9, "$INSUNITS");
    put(70, "4"); // millimetres
    put(9, "$EXTMIN");
    put(10, "0.0");
    put(20, "0.0");
    put(9, "$EXTMAX");
    put(10, &num(o.size[0]));
    put(20, &num(o.size[1]));
    put(0, "ENDSEC");
    put(0, "SECTION");
    put(2, "TABLES");
    put(0, "TABLE");
    put(2, "LAYER");
    put(70, "2");
    for (name, colour) in [("OUTER", "7"), ("INNER", "5")] {
        put(0, "LAYER");
        put(2, name);
        put(70, "0");
        put(62, colour);
        put(6, "CONTINUOUS");
    }
    put(0, "ENDTAB");
    put(0, "ENDSEC");
    put(0, "SECTION");
    put(2, "ENTITIES");
    for l in &o.loops {
        if let Some((c, r)) = l.circle() {
            put(0, "CIRCLE");
            put(8, layer(l));
            put(10, &num(c[0]));
            put(20, &num(c[1]));
            put(30, "0.0");
            put(40, &num(r));
            continue;
        }
        put(0, "POLYLINE");
        put(8, layer(l));
        put(66, "1");
        put(10, "0.0");
        put(20, "0.0");
        put(30, "0.0");
        put(70, "1"); // closed
        let mut vertex = |p: [Real; 2], bulge: Real| {
            put(0, "VERTEX");
            put(8, layer(l));
            put(10, &num(p[0]));
            put(20, &num(p[1]));
            put(30, "0.0");
            if bulge != 0.0 {
                put(42, &num(bulge));
            }
        };
        for seg in &l.segs {
            match seg {
                Seg::Line(a, _) => vertex(*a, 0.0),
                Seg::Arc { sweep, .. } => {
                    // A bulge covers less than a whole turn: halve big arcs.
                    let n = if sweep.abs() > std::f64::consts::PI * 1.5 {
                        2
                    } else {
                        1
                    };
                    let Seg::Arc {
                        center,
                        radius,
                        start,
                        ..
                    } = seg
                    else {
                        continue;
                    };
                    for k in 0..n {
                        let a = start + sweep * k as Real / n as Real;
                        vertex(polar(*center, *radius, a), (sweep / n as Real / 4.0).tan());
                    }
                }
                Seg::Curve(p) => {
                    for q in p.iter().take(p.len().saturating_sub(1)) {
                        vertex(*q, 0.0);
                    }
                }
            }
        }
        put(0, "SEQEND");
        put(8, layer(l));
    }
    put(0, "ENDSEC");
    put(0, "EOF");
    s
}

/// The outline as an SVG drawing in millimetres (1 user unit = 1 mm, y up
/// as in the model): one `<path>` per loop in groups `OUTER` and `INNER`.
pub fn svg(o: &FaceOutline) -> String {
    use std::fmt::Write;
    let (w, h) = (o.size[0], o.size[1]);
    // SVG's y points down: y' = h − y, which also turns every arc round.
    let p = |q: [Real; 2]| format!("{} {}", num(q[0]), num(h - q[1]));
    let mut s = String::new();
    let _ = writeln!(s, r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    let _ = writeln!(
        s,
        r#"<svg xmlns="http://www.w3.org/2000/svg" version="1.1" width="{w}mm" height="{hh}mm" viewBox="0 0 {w} {hh}">"#,
        w = num(w),
        hh = num(h)
    );
    for group in ["OUTER", "INNER"] {
        let loops: Vec<&Loop2> = o.loops.iter().filter(|l| layer(l) == group).collect();
        if loops.is_empty() {
            continue;
        }
        let colour = if group == "OUTER" {
            "#000000"
        } else {
            "#0000ff"
        };
        let _ = writeln!(
            s,
            r#"  <g id="{group}" fill="none" stroke="{colour}" stroke-width="0.1">"#
        );
        for l in loops {
            if let Some((c, r)) = l.circle() {
                let _ = writeln!(
                    s,
                    r#"    <circle cx="{}" cy="{}" r="{}"/>"#,
                    num(c[0]),
                    num(h - c[1]),
                    num(r)
                );
                continue;
            }
            let mut d = String::new();
            if let Some(first) = l.segs.first() {
                let _ = write!(d, "M {}", p(first.start()));
            }
            let last = l.segs.len().saturating_sub(1);
            for (i, seg) in l.segs.iter().enumerate() {
                match seg {
                    // Z draws the last line back to the start.
                    Seg::Line(..) if i == last => {}
                    Seg::Line(_, b) => {
                        let _ = write!(d, " L {}", p(*b));
                    }
                    Seg::Arc {
                        center,
                        radius,
                        start,
                        sweep,
                    } => {
                        // One command covers less than a whole turn.
                        let n = if sweep.abs() > std::f64::consts::PI * 1.5 {
                            2
                        } else {
                            1
                        };
                        for k in 1..=n {
                            let part = sweep / n as Real;
                            let end = polar(*center, *radius, start + part * k as Real);
                            let large = u8::from(part.abs() > std::f64::consts::PI);
                            let flag = u8::from(part < 0.0);
                            let _ = write!(
                                d,
                                " A {r} {r} 0 {large} {flag} {}",
                                p(end),
                                r = num(*radius)
                            );
                        }
                    }
                    Seg::Curve(pts) => {
                        for q in pts.iter().skip(1) {
                            let _ = write!(d, " L {}", p(*q));
                        }
                    }
                }
            }
            d.push_str(" Z");
            let _ = writeln!(s, r#"    <path d="{d}"/>"#);
        }
        let _ = writeln!(s, "  </g>");
    }
    let _ = writeln!(s, "</svg>");
    s
}

/// The text of `o` in `format`.
pub fn text(o: &FaceOutline, format: FaceFormat) -> String {
    match format {
        FaceFormat::Dxf => dxf(o),
        FaceFormat::Svg => svg(o),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x0: Real, y0: Real, s: Real) -> Loop2 {
        let p = [[x0, y0], [x0 + s, y0], [x0 + s, y0 + s], [x0, y0 + s]];
        Loop2 {
            segs: (0..4).map(|i| Seg::Line(p[i], p[(i + 1) % 4])).collect(),
            inner: false,
        }
    }

    #[test]
    fn arcs_of_one_circle_become_a_circle_and_lines_join() {
        let half = |a: Real| Seg::Arc {
            center: [1.0, 2.0],
            radius: 3.0,
            start: a,
            sweep: std::f64::consts::PI,
        };
        let m = merge(vec![half(0.0), half(std::f64::consts::PI)]);
        let l = Loop2 {
            segs: m,
            inner: false,
        };
        assert_eq!(l.circle(), Some(([1.0, 2.0], 3.0)));
        assert!((l.area() - std::f64::consts::PI * 9.0).abs() < 1e-9);
        let lines = merge(vec![
            Seg::Line([0.0, 0.0], [1.0, 0.0]),
            Seg::Line([1.0, 0.0], [3.0, 0.0]),
            Seg::Line([3.0, 0.0], [3.0, 1.0]),
            Seg::Line([3.0, 1.0], [0.0, 0.0]),
        ]);
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn svg_and_dxf_have_every_loop() {
        let o = FaceOutline {
            loops: vec![square(0.0, 0.0, 10.0), {
                let mut h = square(2.0, 2.0, 2.0);
                h.inner = true;
                h
            }],
            size: [10.0, 10.0],
        };
        let d = dxf(&o);
        assert_eq!(d.matches("\nPOLYLINE\n").count(), 2);
        assert!(d.contains("\nINNER\n") && d.trim_end().ends_with("EOF"));
        let s = svg(&o);
        assert_eq!(s.matches("<path").count(), 2);
        assert!(s.contains(r#"width="10.000000mm""#));
        // y is flipped: the hole's corner (2, 2) is at (2, 8) in SVG.
        assert!(s.contains("M 2.000000 8.000000"));
    }
}
