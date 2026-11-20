//! **DXF reading** for Insert AutoCAD File in a sketch (spec 1.11a): an ASCII DXF's model-space
//! entities as 2D geometry in millimetres, with their layers.
//!
//! Adapted from SolidCraft's `crates/io/src/dxf.rs` (MIT OR Apache-2.0; pinned commit and main
//! `61351c67`): LINE, CIRCLE, ARC, LWPOLYLINE and POLYLINE (bulges as exact arcs), POINT,
//! ELLIPSE, SPLINE (control points with their knots and weights, or fit points), TEXT and MTEXT,
//! scaled by `$INSUNITS`; INSERT block references expanded (scale, rotation, arrays, nested
//! blocks, entities on layer `0` taking the reference's layer); paper space left out; a DWG or a
//! binary DXF is named and refused. Our own: every curve keeps exact geometry through a block's
//! transform — a circle or arc under an unequal scale becomes an ellipse in conjugate form
//! (`c + u cos t + v sin t`), which every affine map keeps exact — and 2D entities drawn with
//! their extrusion along −Z are mirrored as the DXF's arbitrary axis rule says.

use std::collections::HashMap;

use super::{Point2, Vector2};
use crate::kernel::Real;

/// The largest file read (bytes).
pub const MAX_BYTES: usize = 256 * 1024 * 1024;
/// The most entities that come out.
pub const MAX_ENTITIES: usize = 500_000;
/// Deepest block nesting expanded.
const MAX_BLOCK_DEPTH: usize = 16;
/// Entities visited and array cells placed while expanding blocks (a block that emits nothing
/// trips no output bound: N blocks each inserting the next N times is N^depth of work).
const MAX_WORK: usize = 4 * MAX_ENTITIES;

/// One piece of a drawing, in mm.
#[derive(Clone, Debug, PartialEq)]
pub enum DxfGeom {
    /// A point.
    Point(Point2),
    /// A line.
    Line(Point2, Point2),
    /// A circle.
    Circle {
        /// The centre.
        c: Point2,
        /// The radius (mm).
        r: Real,
    },
    /// Counter-clockwise from `start` to `end` (radians).
    Arc {
        /// The centre.
        c: Point2,
        /// The radius (mm).
        r: Real,
        /// Where it starts.
        start: Real,
        /// Where it ends.
        end: Real,
    },
    /// `c + u cos t + v sin t` for t from `start` to `end` (a whole one when they are a turn
    /// apart); `u`, `v` conjugate semi-diameters (the axes for a DXF ELLIPSE).
    Ellipse {
        /// The centre.
        c: Point2,
        /// The first semi-diameter (from the centre).
        u: Vector2,
        /// The second semi-diameter, a quarter turn on.
        v: Vector2,
        /// Where it starts.
        start: Real,
        /// Where it ends.
        end: Real,
    },
    /// A B-spline by control points (`knots` empty: clamped uniform; `weights` empty:
    /// polynomial), or by `fit` points it passes through when it has no control points.
    Spline {
        /// The polynomial degree.
        degree: usize,
        /// Control points.
        control: Vec<Point2>,
        /// Knot vector (empty: clamped uniform).
        knots: Vec<Real>,
        /// Weights (empty: polynomial).
        weights: Vec<Real>,
        /// Points it passes through.
        fit: Vec<Point2>,
        /// Whether it closes on itself.
        closed: bool,
    },
    /// A line of text (MTEXT paragraphs joined by newlines), its insertion point, capital height
    /// and direction (radians).
    Text {
        /// The text.
        text: String,
        /// Where it stands.
        at: Point2,
        /// Capital height (mm).
        height: Real,
        /// Direction (radians).
        angle: Real,
    },
}

/// A drawing item and its layer.
#[derive(Clone, Debug, PartialEq)]
pub struct DxfItem {
    /// Its layer.
    pub layer: String,
    /// Its geometry.
    pub geom: DxfGeom,
}

/// What [`read`] found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DxfDrawing {
    /// The items.
    pub items: Vec<DxfItem>,
    /// The layers the items lie on, in the order first met.
    pub layers: Vec<String>,
    /// The file's length unit in mm (`$INSUNITS`; unitless = 1).
    pub unit: Real,
}

impl DxfDrawing {
    /// The box of every item's defining points (`None`: empty).
    pub fn bounds(&self) -> Option<(Point2, Point2)> {
        let mut lo = Point2::new(Real::INFINITY, Real::INFINITY);
        let mut hi = Point2::new(Real::NEG_INFINITY, Real::NEG_INFINITY);
        let mut add = |p: Point2| {
            lo = lo.inf(&p);
            hi = hi.sup(&p);
        };
        for it in &self.items {
            match &it.geom {
                DxfGeom::Point(p) => add(*p),
                DxfGeom::Line(a, b) => {
                    add(*a);
                    add(*b);
                }
                DxfGeom::Circle { c, r } | DxfGeom::Arc { c, r, .. } => {
                    add(c - Vector2::new(*r, *r));
                    add(c + Vector2::new(*r, *r));
                }
                DxfGeom::Ellipse { c, u, v, .. } => {
                    let e = Vector2::new(u.x.hypot(v.x), u.y.hypot(v.y));
                    add(c - e);
                    add(c + e);
                }
                DxfGeom::Spline { control, fit, .. } => {
                    control.iter().chain(fit).for_each(|p| add(*p))
                }
                DxfGeom::Text { at, .. } => add(*at),
            }
        }
        (lo.x <= hi.x).then_some((lo, hi))
    }
}

/// Whether `bytes` are a DWG drawing (they start with its version magic `AC1…`).
pub fn is_dwg(bytes: &[u8]) -> bool {
    bytes.starts_with(b"AC1")
}

/// Group code / value pairs (values trimmed, except text values 1 and 3).
fn pairs(text: &str) -> Vec<(i32, String)> {
    let mut out = Vec::new();
    let mut lines = text.lines();
    while let (Some(c), Some(v)) = (lines.next(), lines.next()) {
        let Ok(code) = c.trim().parse::<i32>() else {
            continue;
        };
        let v = if matches!(code, 1 | 3) {
            v.trim_end_matches('\r')
        } else {
            v.trim()
        };
        out.push((code, v.to_string()));
    }
    out
}

fn units_scale(p: &[(i32, String)]) -> Real {
    // $INSUNITS: 1 in, 2 ft, 4 mm, 5 cm, 6 m, 8 microinch, 9 mil, 10 yd, 14 dm; 0 unitless (mm).
    let i = p.iter().position(|(c, v)| *c == 9 && v == "$INSUNITS");
    match i
        .and_then(|i| p.get(i + 1))
        .and_then(|(_, v)| v.parse::<i32>().ok())
    {
        Some(1) => 25.4,
        Some(2) => 304.8,
        Some(5) => 10.0,
        Some(6) => 1000.0,
        Some(8) => 25.4e-6,
        Some(9) => 0.0254,
        Some(10) => 914.4,
        Some(14) => 100.0,
        _ => 1.0,
    }
}

#[derive(Default)]
struct Ent {
    kind: String,
    f: Vec<(i32, Real)>,
    s: Vec<(i32, String)>,
}

impl Ent {
    fn get(&self, code: i32) -> Option<Real> {
        self.f.iter().find(|(c, _)| *c == code).map(|(_, v)| *v)
    }
    fn all(&self, code: i32) -> Vec<Real> {
        self.f
            .iter()
            .filter(|(c, _)| *c == code)
            .map(|(_, v)| *v)
            .collect()
    }
    fn text(&self, code: i32) -> Option<&str> {
        self.s
            .iter()
            .find(|(c, _)| *c == code)
            .map(|(_, v)| v.as_str())
    }
    fn layer(&self) -> &str {
        self.text(8).map_or("0", str::trim)
    }
    /// The extrusion direction points down −Z (the entity is drawn mirrored in X).
    fn mirrored(&self) -> bool {
        self.get(230).is_some_and(|z| z < 0.0)
    }
}

/// The entities of the section whose name is at `start`.
fn section(p: &[(i32, String)], start: usize) -> Result<Vec<Ent>, String> {
    let mut ents: Vec<Ent> = Vec::new();
    for (c, v) in p.iter().skip(start + 1) {
        if *c == 0 {
            if v == "ENDSEC" || v == "EOF" {
                break;
            }
            ents.push(Ent {
                kind: v.clone(),
                ..Default::default()
            });
            if ents.len() > MAX_ENTITIES {
                return Err("too many entities".into());
            }
            continue;
        }
        let Some(e) = ents.last_mut() else { continue };
        let numeric = matches!(*c, 10..=59 | 60..=99 | 140..=147 | 210..=239);
        match v.parse::<Real>() {
            Ok(x) if numeric && x.is_finite() && x.abs() < 1e12 => e.f.push((*c, x)),
            _ => e.s.push((*c, v.clone())),
        }
    }
    Ok(ents)
}

/// Reads an ASCII DXF file's model space.
pub fn read(bytes: &[u8]) -> Result<DxfDrawing, String> {
    if bytes.len() > MAX_BYTES {
        return Err("the DXF file is too large".into());
    }
    if is_dwg(bytes) {
        return Err(
            "this is a DWG file, not a DXF: save the drawing as ASCII DXF and insert that".into(),
        );
    }
    if bytes.starts_with(b"AutoCAD Binary DXF") {
        return Err("binary DXF is not read: save the drawing as ASCII DXF".into());
    }
    let text = String::from_utf8_lossy(bytes);
    let p = pairs(&text);
    let k = units_scale(&p);
    let start = p
        .iter()
        .position(|(c, v)| *c == 2 && v == "ENTITIES")
        .ok_or("not a DXF drawing (no ENTITIES section)")?;
    let ents = section(&p, start)?;
    let mut blocks: HashMap<String, (Point2, Vec<Ent>)> = HashMap::new();
    if let Some(bs) = p.iter().position(|(c, v)| *c == 2 && v == "BLOCKS") {
        let mut cur: Option<(String, Point2, Vec<Ent>)> = None;
        for e in section(&p, bs)? {
            match e.kind.as_str() {
                "BLOCK" => {
                    let base =
                        Point2::new(e.get(10).unwrap_or(0.0) * k, e.get(20).unwrap_or(0.0) * k);
                    cur = Some((e.text(2).unwrap_or("").trim().to_string(), base, Vec::new()));
                }
                "ENDBLK" => {
                    if let Some((name, base, list)) = cur.take() {
                        blocks.insert(name, (base, list));
                    }
                }
                _ => {
                    if let Some(c) = cur.as_mut() {
                        c.2.push(e);
                    }
                }
            }
        }
    }
    let mut d = DxfDrawing {
        unit: k,
        ..Default::default()
    };
    let mut work = 0;
    convert(&ents, k, &blocks, 0, None, &mut d.items, &mut work)?;
    for it in &d.items {
        if !d.layers.contains(&it.layer) {
            d.layers.push(it.layer.clone());
        }
    }
    Ok(d)
}

/// p ↦ m·p + t.
#[derive(Clone, Copy, Debug)]
struct Affine {
    m: [Real; 4],
    t: Vector2,
}

impl Affine {
    const ID: Affine = Affine {
        m: [1.0, 0.0, 0.0, 1.0],
        t: Vector2::new(0.0, 0.0),
    };

    /// A block placed at `ins`: its base point moves there, scaled then rotated.
    fn insert(ins: Point2, base: Point2, sx: Real, sy: Real, rot: Real) -> Affine {
        let (s, c) = rot.sin_cos();
        let m = [c * sx, -s * sy, s * sx, c * sy];
        let a = Affine {
            m,
            t: Vector2::zeros(),
        };
        let mb = a.v(base.coords);
        Affine {
            m,
            t: ins.coords - mb,
        }
    }
    /// `self` after `inner` (inner first).
    fn then(&self, inner: &Affine) -> Affine {
        let [a, b, c, d] = self.m;
        let [e, f, g, h] = inner.m;
        Affine {
            m: [a * e + b * g, a * f + b * h, c * e + d * g, c * f + d * h],
            t: self.v(inner.t) + self.t,
        }
    }
    fn p(&self, p: Point2) -> Point2 {
        Point2::from(self.v(p.coords) + self.t)
    }
    fn v(&self, v: Vector2) -> Vector2 {
        Vector2::new(
            self.m[0] * v.x + self.m[1] * v.y,
            self.m[2] * v.x + self.m[3] * v.y,
        )
    }
    fn det(&self) -> Real {
        self.m[0] * self.m[3] - self.m[1] * self.m[2]
    }
    /// The scale when it is the same in every direction (a similarity).
    fn uniform(&self) -> Option<Real> {
        let (a, b) = (self.v(Vector2::x()), self.v(Vector2::y()));
        let (la, lb) = (a.norm(), b.norm());
        ((la - lb).abs() <= 1e-9 * la.max(lb) && a.dot(&b).abs() <= 1e-9 * la * lb).then_some(la)
    }
    fn map(&self, g: &DxfGeom) -> DxfGeom {
        let flip = self.det() < 0.0;
        let angle = |a: Real| {
            let d = self.v(Vector2::new(a.cos(), a.sin()));
            d.y.atan2(d.x)
        };
        match g {
            DxfGeom::Point(p) => DxfGeom::Point(self.p(*p)),
            DxfGeom::Line(a, b) => DxfGeom::Line(self.p(*a), self.p(*b)),
            DxfGeom::Circle { c, r } => match self.uniform() {
                Some(s) => DxfGeom::Circle {
                    c: self.p(*c),
                    r: r * s,
                },
                None => DxfGeom::Ellipse {
                    c: self.p(*c),
                    u: self.v(Vector2::new(*r, 0.0)),
                    v: self.v(Vector2::new(0.0, *r)),
                    start: 0.0,
                    end: std::f64::consts::TAU,
                },
            },
            DxfGeom::Arc { c, r, start, end } => match self.uniform() {
                // A mirror runs the arc the other way: its old end is the new start.
                Some(s) if flip => DxfGeom::Arc {
                    c: self.p(*c),
                    r: r * s,
                    start: angle(*end),
                    end: angle(*start),
                },
                Some(s) => DxfGeom::Arc {
                    c: self.p(*c),
                    r: r * s,
                    start: angle(*start),
                    end: angle(*end),
                },
                None => {
                    let sweep = (end - start).rem_euclid(std::f64::consts::TAU);
                    let (u, v) = (self.v(Vector2::new(*r, 0.0)), self.v(Vector2::new(0.0, *r)));
                    // Mirrored: the same points with t running the other way (v turned round).
                    if flip {
                        DxfGeom::Ellipse {
                            c: self.p(*c),
                            u,
                            v: -v,
                            start: -start - sweep,
                            end: -start,
                        }
                    } else {
                        DxfGeom::Ellipse {
                            c: self.p(*c),
                            u,
                            v,
                            start: *start,
                            end: start + sweep,
                        }
                    }
                }
            },
            DxfGeom::Ellipse {
                c,
                u,
                v,
                start,
                end,
            } => {
                if flip {
                    DxfGeom::Ellipse {
                        c: self.p(*c),
                        u: self.v(*u),
                        v: -self.v(*v),
                        start: -end,
                        end: -start,
                    }
                } else {
                    DxfGeom::Ellipse {
                        c: self.p(*c),
                        u: self.v(*u),
                        v: self.v(*v),
                        start: *start,
                        end: *end,
                    }
                }
            }
            DxfGeom::Spline {
                degree,
                control,
                knots,
                weights,
                fit,
                closed,
            } => DxfGeom::Spline {
                degree: *degree,
                control: control.iter().map(|p| self.p(*p)).collect(),
                knots: knots.clone(),
                weights: weights.clone(),
                fit: fit.iter().map(|p| self.p(*p)).collect(),
                closed: *closed,
            },
            DxfGeom::Text {
                text,
                at,
                height,
                angle: a,
            } => {
                let up = self.v(Vector2::new(-a.sin(), a.cos()));
                DxfGeom::Text {
                    text: text.clone(),
                    at: self.p(*at),
                    height: height * up.norm(),
                    angle: angle(*a),
                }
            }
        }
    }
}

/// MTEXT's plain text: paragraphs (`\P`) as newlines, formatting codes (`\f…;`, `\H…;`,
/// `{`, `}`, `\L`…) left out, `\\` `\{` `\}` kept as the character.
fn plain_mtext(t: &str) -> String {
    let mut out = String::new();
    let mut it = t.chars().peekable();
    while let Some(ch) = it.next() {
        match ch {
            '{' | '}' => {}
            '\\' => match it.next() {
                Some('P') | Some('X') => out.push('\n'),
                Some('~') => out.push(' '),
                Some(c @ ('\\' | '{' | '}')) => out.push(c),
                Some('U') => out.push(unicode_escape(&mut it).unwrap_or('?')),
                Some('L' | 'l' | 'O' | 'o' | 'K' | 'k') => {}
                // Codes with a value up to ';'.
                Some(_) => {
                    for c in it.by_ref() {
                        if c == ';' {
                            break;
                        }
                    }
                }
                None => {}
            },
            _ => out.push(ch),
        }
    }
    out
}

/// The character of a `\U+XXXX` escape, after its `\U` (`it` moves past the four digits).
fn unicode_escape(it: &mut std::iter::Peekable<std::str::Chars>) -> Option<char> {
    if it.peek() != Some(&'+') {
        return None;
    }
    it.next();
    let hex: String = (0..4).filter_map(|_| it.next()).collect();
    u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32)
}

/// TEXT's special codes: `%%d` degree, `%%p` plus-minus, `%%c` diameter, `%%%` percent, and
/// `\U+XXXX` characters.
fn plain_text(t: &str) -> String {
    let mut out = String::new();
    let mut it = t.chars().peekable();
    while let Some(ch) = it.next() {
        if ch == '\\' && it.peek() == Some(&'U') {
            it.next();
            out.push(unicode_escape(&mut it).unwrap_or('?'));
        } else {
            out.push(ch);
        }
    }
    out.replace("%%d", "°")
        .replace("%%D", "°")
        .replace("%%p", "±")
        .replace("%%P", "±")
        .replace("%%c", "Ø")
        .replace("%%C", "Ø")
        .replace("%%%", "%")
}

/// Bulged polyline vertices as lines and exact arcs.
fn polyline(verts: &[(Point2, Real)], closed: bool, out: &mut Vec<DxfGeom>) {
    let n = verts.len();
    let segs = if closed { n } else { n.saturating_sub(1) };
    for j in 0..segs {
        let (Some(&(p0, b)), Some(&(p1, _))) = (verts.get(j), verts.get((j + 1) % n)) else {
            continue;
        };
        let chord = p1 - p0;
        let len = chord.norm();
        if len < 1e-12 {
            continue;
        }
        if b.abs() < 1e-12 {
            out.push(DxfGeom::Line(p0, p1));
            continue;
        }
        // Bulge = tan(θ/4), positive counter-clockwise from p0 to p1.
        let theta = 4.0 * b.atan();
        let r = len / (2.0 * (theta / 2.0).sin()).abs();
        let mid = p0 + chord / 2.0;
        let normal = Vector2::new(-chord.y, chord.x) / len;
        // The centre lies to the left of the chord for a counter-clockwise bulge under a half
        // turn, to the right beyond it.
        let h = r
            * (theta / 2.0).cos().abs()
            * if (b > 0.0) == (b.abs() < 1.0) {
                1.0
            } else {
                -1.0
            };
        let c = mid + normal * h;
        let (a0, a1) = ((p0 - c).y.atan2((p0 - c).x), (p1 - c).y.atan2((p1 - c).x));
        out.push(if b > 0.0 {
            DxfGeom::Arc {
                c,
                r,
                start: a0,
                end: a1,
            }
        } else {
            DxfGeom::Arc {
                c,
                r,
                start: a1,
                end: a0,
            }
        });
    }
}

/// Entities to geometry (mm), expanding block references; `parent` = the layer of the
/// reference a block is expanded for (entities on layer 0 take it).
#[allow(clippy::too_many_arguments)]
fn convert(
    ents: &[Ent],
    k: Real,
    blocks: &HashMap<String, (Point2, Vec<Ent>)>,
    depth: usize,
    parent: Option<&str>,
    out: &mut Vec<DxfItem>,
    work: &mut usize,
) -> Result<(), String> {
    let pt = |x: Real, y: Real| Point2::new(x * k, y * k);
    let mut i = 0;
    while i < ents.len() {
        let Some(e) = ents.get(i) else { break };
        i += 1;
        *work = work.saturating_add(1);
        if out.len() > MAX_ENTITIES || *work > MAX_WORK {
            return Err("the drawing is too complex".into());
        }
        // Paper space (layouts) is not the drawing.
        if e.get(67).is_some_and(|v| v != 0.0) {
            continue;
        }
        let layer = match (e.layer(), parent) {
            ("0", Some(p)) => p.to_string(),
            (l, _) => l.to_string(),
        };
        let mut geom: Vec<DxfGeom> = Vec::new();
        match e.kind.as_str() {
            "INSERT" => {
                let Some((base, list)) = e.text(2).and_then(|n| blocks.get(n.trim())) else {
                    continue;
                };
                if depth >= MAX_BLOCK_DEPTH {
                    continue;
                }
                let mut inner = Vec::new();
                convert(list, k, blocks, depth + 1, Some(&layer), &mut inner, work)?;
                if inner.is_empty() {
                    continue;
                }
                let (sx, sy) = (e.get(41).unwrap_or(1.0), e.get(42).unwrap_or(1.0));
                let rot = e.get(50).unwrap_or(0.0).to_radians();
                let ins = pt(e.get(10).unwrap_or(0.0), e.get(20).unwrap_or(0.0));
                let cols = e.get(70).unwrap_or(1.0).clamp(1.0, 10_000.0) as usize;
                let rows = e.get(71).unwrap_or(1.0).clamp(1.0, 10_000.0) as usize;
                let cells = cols.saturating_mul(rows);
                *work = work.saturating_add(cells);
                if cells.saturating_mul(inner.len()) > MAX_ENTITIES || *work > MAX_WORK {
                    return Err("the drawing is too complex".into());
                }
                let (dc, dr) = (e.get(44).unwrap_or(0.0) * k, e.get(45).unwrap_or(0.0) * k);
                let mirror = if e.mirrored() {
                    Affine {
                        m: [-1.0, 0.0, 0.0, 1.0],
                        t: Vector2::zeros(),
                    }
                } else {
                    Affine::ID
                };
                let along = Vector2::new(rot.cos(), rot.sin());
                let across = Vector2::new(-along.y, along.x);
                for r in 0..rows {
                    for c in 0..cols {
                        let off = along * (dc * c as Real) + across * (dr * r as Real);
                        let t = mirror.then(&Affine::insert(ins + off, *base, sx, sy, rot));
                        for g in &inner {
                            out.push(DxfItem {
                                layer: g.layer.clone(),
                                geom: t.map(&g.geom),
                            });
                        }
                    }
                }
                continue;
            }
            "LINE" => {
                if let (Some(x0), Some(y0), Some(x1), Some(y1)) =
                    (e.get(10), e.get(20), e.get(11), e.get(21))
                {
                    geom.push(DxfGeom::Line(pt(x0, y0), pt(x1, y1)));
                }
            }
            "POINT" => {
                if let (Some(x), Some(y)) = (e.get(10), e.get(20)) {
                    geom.push(DxfGeom::Point(pt(x, y)));
                }
            }
            "CIRCLE" => {
                if let (Some(x), Some(y), Some(r)) = (e.get(10), e.get(20), e.get(40))
                    && r > 0.0
                {
                    geom.push(DxfGeom::Circle {
                        c: pt(x, y),
                        r: r * k,
                    });
                }
            }
            "ARC" => {
                if let (Some(x), Some(y), Some(r), Some(a0), Some(a1)) =
                    (e.get(10), e.get(20), e.get(40), e.get(50), e.get(51))
                    && r > 0.0
                {
                    geom.push(DxfGeom::Arc {
                        c: pt(x, y),
                        r: r * k,
                        start: a0.to_radians(),
                        end: a1.to_radians(),
                    });
                }
            }
            "LWPOLYLINE" => {
                let ys = e.all(20);
                // Bulges follow their vertex: walk the pairs in order.
                let mut verts: Vec<(Point2, Real)> = Vec::new();
                let mut xi = 0;
                for (c, v) in &e.f {
                    match *c {
                        10 => {
                            if let Some(y) = ys.get(xi) {
                                verts.push((pt(*v, *y), 0.0));
                            }
                            xi += 1;
                        }
                        42 => {
                            if let Some(last) = verts.last_mut() {
                                last.1 = *v;
                            }
                        }
                        _ => {}
                    }
                }
                let closed = e.get(70).is_some_and(|f| (f as i64) & 1 == 1);
                polyline(&verts, closed, &mut geom);
            }
            "POLYLINE" => {
                let flags = e.get(70).unwrap_or(0.0) as i64;
                let mut verts = Vec::new();
                while let Some(v) = ents.get(i) {
                    if v.kind != "VERTEX" {
                        break;
                    }
                    // Spline frame points (flag 16) are not on the curve; fit points are.
                    let vf = v.get(70).unwrap_or(0.0) as i64;
                    if let (Some(x), Some(y)) = (v.get(10), v.get(20))
                        && vf & 16 == 0
                    {
                        verts.push((pt(x, y), v.get(42).unwrap_or(0.0)));
                    }
                    i += 1;
                }
                if ents.get(i).is_some_and(|x| x.kind == "SEQEND") {
                    i += 1;
                }
                // 3D polylines and meshes (flags 8, 16, 64) are not 2D drawings.
                if flags & (16 | 64) == 0 {
                    polyline(&verts, flags & 1 == 1, &mut geom);
                }
            }
            "ELLIPSE" => {
                if let (Some(x), Some(y), Some(mx), Some(my), Some(ratio)) =
                    (e.get(10), e.get(20), e.get(11), e.get(21), e.get(40))
                {
                    let u = Vector2::new(mx, my) * k;
                    let v = Vector2::new(-u.y, u.x) * ratio;
                    let (t0, t1) = (
                        e.get(41).unwrap_or(0.0),
                        e.get(42).unwrap_or(std::f64::consts::TAU),
                    );
                    let mut sweep = (t1 - t0).rem_euclid(std::f64::consts::TAU);
                    if sweep < 1e-9 {
                        sweep = std::f64::consts::TAU;
                    }
                    geom.push(DxfGeom::Ellipse {
                        c: pt(x, y),
                        u,
                        v,
                        start: t0,
                        end: t0 + sweep,
                    });
                }
            }
            "SPLINE" => {
                let flags = e.get(70).unwrap_or(0.0) as i64;
                let degree = e.get(71).unwrap_or(3.0).clamp(1.0, 25.0) as usize;
                let (cx, cy) = (e.all(10), e.all(20));
                let (fx, fy) = (e.all(11), e.all(21));
                let control: Vec<Point2> = cx.iter().zip(&cy).map(|(x, y)| pt(*x, *y)).collect();
                let fit: Vec<Point2> = fx.iter().zip(&fy).map(|(x, y)| pt(*x, *y)).collect();
                let knots = e.all(40);
                let weights = e.all(41);
                // Knots that do not fit the control points are dropped (clamped uniform).
                let knots = if knots.len() == control.len() + degree + 1 {
                    knots
                } else {
                    Vec::new()
                };
                let weights = if weights.len() == control.len()
                    && weights.iter().any(|w| (w - 1.0).abs() > 1e-12)
                {
                    weights
                } else {
                    Vec::new()
                };
                if control.len() >= 2 || fit.len() >= 2 {
                    geom.push(DxfGeom::Spline {
                        degree,
                        control,
                        knots,
                        weights,
                        fit,
                        closed: flags & 1 == 1,
                    });
                }
            }
            "TEXT" | "MTEXT" => {
                if let (Some(x), Some(y), Some(h)) = (e.get(10), e.get(20), e.get(40)) {
                    // MTEXT longer than 250 characters continues in groups 3 before its 1.
                    let mut t: String =
                        e.s.iter()
                            .filter(|(c, _)| *c == 3)
                            .map(|(_, v)| v.as_str())
                            .collect();
                    t.push_str(e.text(1).unwrap_or(""));
                    let t = if e.kind == "MTEXT" {
                        plain_mtext(&t)
                    } else {
                        plain_text(&t)
                    };
                    // MTEXT gives its direction as a vector (11, 21) or an angle (50).
                    let angle = match (e.get(11), e.get(21)) {
                        (Some(dx), Some(dy)) if e.kind == "MTEXT" => dy.atan2(dx),
                        _ => e.get(50).unwrap_or(0.0).to_radians(),
                    };
                    if !t.trim().is_empty() {
                        geom.push(DxfGeom::Text {
                            text: t,
                            at: pt(x, y),
                            height: h * k,
                            angle,
                        });
                    }
                }
            }
            _ => {}
        }
        let mirror = Affine {
            m: [-1.0, 0.0, 0.0, 1.0],
            t: Vector2::zeros(),
        };
        for g in geom {
            let g = if e.mirrored() && e.kind != "LINE" && e.kind != "POINT" {
                mirror.map(&g)
            } else {
                g
            };
            out.push(DxfItem {
                layer: layer.clone(),
                geom: g,
            });
        }
    }
    Ok(())
}

/// The choices of Insert AutoCAD File (Inventor's Import Destination Options, spec 1.11a).
#[derive(Clone, Debug, PartialEq)]
pub struct InsertOptions {
    /// The layers taken (empty: all).
    pub layers: Vec<String>,
    /// Extra scale on top of the file's unit (1 = as the file says).
    pub scale: Real,
    /// Where the drawing's origin lands in the sketch.
    pub origin: Point2,
    /// Ends within this distance (mm) share one point, so the pieces join into profiles
    /// (Inventor's Constrain End Points); 0 keeps every end apart.
    pub join: Real,
    /// Bring the geometry in as construction.
    pub construction: bool,
}

impl Default for InsertOptions {
    fn default() -> Self {
        InsertOptions {
            layers: Vec::new(),
            scale: 1.0,
            origin: Point2::origin(),
            join: 1e-6,
            construction: false,
        }
    }
}

/// What an insertion made.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InsertReport {
    /// Lines made.
    pub lines: usize,
    /// Arcs made.
    pub arcs: usize,
    /// Circles made.
    pub circles: usize,
    /// Splines made.
    pub splines: usize,
    /// Points made.
    pub points: usize,
    /// Ellipses and rational / non-uniform splines, which the sketch holds as interpolation
    /// splines through points on them.
    pub approximated: usize,
    /// Texts (not inserted yet) and pieces too small to draw.
    pub skipped: usize,
}

impl InsertReport {
    /// The curves and points made.
    pub fn total(&self) -> usize {
        self.lines + self.arcs + self.circles + self.splines + self.points
    }
}

/// A point of a B-spline (de Boor), rational when `w` is given.
fn de_boor(degree: usize, ctrl: &[Point2], knots: &[Real], w: &[Real], t: Real) -> Option<Point2> {
    let n = ctrl.len();
    if n == 0 || knots.len() != n + degree + 1 {
        return None;
    }
    let t = t.clamp(knots[degree], knots[n]);
    let mut k = degree;
    while k + 1 < n && knots[k + 1] <= t {
        k += 1;
    }
    let weight = |i: usize| w.get(i).copied().unwrap_or(1.0);
    let mut d: Vec<(Vector2, Real)> = (0..=degree)
        .map(|j| {
            let i = k + j - degree;
            (ctrl[i].coords * weight(i), weight(i))
        })
        .collect();
    for r in 1..=degree {
        for j in (r..=degree).rev() {
            let i = k + j - degree;
            let den = knots[i + degree + 1 - r] - knots[i];
            let a = if den.abs() < 1e-300 {
                0.0
            } else {
                (t - knots[i]) / den
            };
            d[j] = (
                d[j - 1].0 * (1.0 - a) + d[j].0 * a,
                d[j - 1].1 * (1.0 - a) + d[j].1 * a,
            );
        }
    }
    let (p, wt) = d[degree];
    (wt.abs() > 1e-300).then(|| Point2::from(p / wt))
}

/// Whether `knots` are those of a clamped uniform B-spline of `n` control points.
fn clamped_uniform(degree: usize, n: usize, knots: &[Real]) -> bool {
    if knots.is_empty() {
        return true;
    }
    if knots.len() != n + degree + 1 || n <= degree {
        return false;
    }
    let (a, b) = (knots[0], knots[knots.len() - 1]);
    let spans = (n - degree) as Real;
    let tol = 1e-9 * (b - a).abs().max(1.0);
    knots.iter().enumerate().all(|(i, &u)| {
        let want = if i <= degree {
            a
        } else if i >= n {
            b
        } else {
            a + (b - a) * (i - degree) as Real / spans
        };
        (u - want).abs() <= tol
    })
}

/// Points made so far at curve ends, shared within the join distance.
struct Ends {
    join: Real,
    at: Vec<(Point2, super::PointId)>,
}

impl Ends {
    /// A point at `p`; an end (`shared`) reuses an end already made there.
    fn point(&mut self, sk: &mut super::Sketch, p: Point2, shared: bool) -> super::PointId {
        if shared
            && self.join > 0.0
            && let Some((_, id)) = self.at.iter().find(|(q, _)| (q - p).norm() <= self.join)
        {
            return *id;
        }
        let id = sk.add_point(p);
        if shared {
            self.at.push((p, id));
        }
        id
    }
}

impl super::Sketch {
    /// Inserts `d`'s geometry (spec 1.11a): lines, arcs and circles exact, control-point splines
    /// that are the sketch's own kind (clamped, uniform, polynomial, of degree min(3, n − 1)) as
    /// control-vertex splines, other splines and ellipses as interpolation splines through
    /// points on them, points as sketch points. Ends within `o.join` share one point.
    pub fn insert_dxf(&mut self, d: &DxfDrawing, o: &InsertOptions) -> InsertReport {
        use super::{EntityId, PointId, Style};
        let mut rep = InsertReport::default();
        let mut ends = Ends {
            join: o.join,
            at: Vec::new(),
        };
        let place = |p: Point2| o.origin + p.coords * o.scale;
        let mut made: Vec<EntityId> = Vec::new();
        let tiny = 1e-9;
        let taken = d
            .items
            .iter()
            .filter(|i| o.layers.is_empty() || o.layers.contains(&i.layer));
        for it in taken {
            match &it.geom {
                DxfGeom::Point(p) => {
                    ends.point(self, place(*p), false);
                    rep.points += 1;
                }
                DxfGeom::Line(a, b) => {
                    let (a, b) = (place(*a), place(*b));
                    if (a - b).norm() < tiny {
                        rep.skipped += 1;
                        continue;
                    }
                    let (pa, pb) = (ends.point(self, a, true), ends.point(self, b, true));
                    match self.add_line_between(pa, pb) {
                        Ok(e) => {
                            made.push(e);
                            rep.lines += 1;
                        }
                        Err(_) => rep.skipped += 1,
                    }
                }
                DxfGeom::Circle { c, r } => {
                    let c = ends.point(self, place(*c), false);
                    match self.add_circle_at(c, r * o.scale) {
                        Ok(e) => {
                            made.push(e);
                            rep.circles += 1;
                        }
                        Err(_) => rep.skipped += 1,
                    }
                }
                DxfGeom::Arc { c, r, start, end } => {
                    let r = r * o.scale;
                    let cc = place(*c);
                    let at = |a: Real| cc + Vector2::new(a.cos(), a.sin()) * r;
                    let (ps, pe) = (at(*start), at(*end));
                    if r < tiny || (ps - pe).norm() < tiny {
                        rep.skipped += 1;
                        continue;
                    }
                    let center = ends.point(self, cc, false);
                    let (s, e) = (ends.point(self, ps, true), ends.point(self, pe, true));
                    match self.add_arc(center, s, e) {
                        Ok(id) => {
                            made.push(id);
                            rep.arcs += 1;
                        }
                        Err(_) => rep.skipped += 1,
                    }
                }
                DxfGeom::Ellipse {
                    c,
                    u,
                    v,
                    start,
                    end,
                } => {
                    let sweep = end - start;
                    let whole = (sweep - std::f64::consts::TAU).abs() < 1e-9;
                    let n = if whole {
                        16
                    } else {
                        ((sweep / std::f64::consts::TAU) * 16.0).ceil().max(4.0) as usize
                    };
                    let pts: Vec<Point2> = (0..=n)
                        .map(|j| {
                            let t = start + sweep * j as Real / n as Real;
                            place(c + u * t.cos() + v * t.sin())
                        })
                        .collect();
                    if let Some(id) = self.insert_through(&pts, whole, &mut ends) {
                        made.push(id);
                        rep.approximated += 1;
                        rep.splines += 1;
                    } else {
                        rep.skipped += 1;
                    }
                }
                DxfGeom::Spline {
                    degree,
                    control,
                    knots,
                    weights,
                    fit,
                    closed,
                } => {
                    let own = !control.is_empty()
                        && weights.is_empty()
                        && *degree == 3.min(control.len() - 1)
                        && clamped_uniform(*degree, control.len(), knots)
                        && !*closed;
                    if own {
                        let n = control.len();
                        let ids: Vec<PointId> = control
                            .iter()
                            .enumerate()
                            .map(|(j, p)| ends.point(self, place(*p), j == 0 || j + 1 == n))
                            .collect();
                        match self.add_spline(ids) {
                            Ok(id) => {
                                made.push(id);
                                rep.splines += 1;
                            }
                            Err(_) => rep.skipped += 1,
                        }
                        continue;
                    }
                    let pts: Vec<Point2> = if control.len() > *degree && !knots.is_empty() {
                        let (a, b) = (knots[*degree], knots[control.len()]);
                        let n = (control.len() * 4).clamp(8, 200);
                        (0..=n)
                            .filter_map(|j| {
                                let t = a + (b - a) * j as Real / n as Real;
                                de_boor(*degree, control, knots, weights, t)
                            })
                            .map(place)
                            .collect()
                    } else if fit.len() >= 2 {
                        fit.iter().map(|p| place(*p)).collect()
                    } else {
                        control.iter().map(|p| place(*p)).collect()
                    };
                    let ring = pts.len() > 2
                        && pts
                            .first()
                            .zip(pts.last())
                            .is_some_and(|(a, b)| (a - b).norm() < tiny);
                    if let Some(id) = self.insert_through(&pts, *closed || ring, &mut ends) {
                        made.push(id);
                        rep.splines += 1;
                        rep.approximated += usize::from(fit.len() < 2);
                    } else {
                        rep.skipped += 1;
                    }
                }
                DxfGeom::Text { .. } => rep.skipped += 1,
            }
        }
        if o.construction {
            for id in made {
                let _ = self.set_style(id, Style::Construction);
            }
        }
        rep
    }

    /// An interpolation spline through `pts` (sketch coordinates; ends shared through `ends`).
    fn insert_through(
        &mut self,
        pts: &[Point2],
        closed: bool,
        ends: &mut Ends,
    ) -> Option<super::EntityId> {
        let mut pts: Vec<Point2> = pts.to_vec();
        pts.dedup_by(|a, b| (*a - *b).norm() < 1e-9);
        if closed && pts.len() > 2 && (pts[0] - pts[pts.len() - 1]).norm() < 1e-9 {
            pts.pop();
        }
        if pts.len() < 2 {
            return None;
        }
        let n = pts.len();
        let mut ids: Vec<super::PointId> = pts
            .iter()
            .enumerate()
            .map(|(j, p)| ends.point(self, *p, !closed && (j == 0 || j + 1 == n)))
            .collect();
        if closed {
            ids.push(ids[0]);
        }
        let id = self.add_interp_spline(ids).ok()?;
        if closed {
            self.set_spline_closed(id, true).ok()?;
        }
        Some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A DXF from group pairs: each `(code, value)` on two lines.
    fn dxf(header: &[(i32, &str)], blocks: &[(i32, &str)], ents: &[(i32, &str)]) -> String {
        let mut s = String::new();
        let mut put = |c: i32, v: &str| s.push_str(&format!("{c}\n{v}\n"));
        put(0, "SECTION");
        put(2, "HEADER");
        header.iter().for_each(|(c, v)| put(*c, v));
        put(0, "ENDSEC");
        put(0, "SECTION");
        put(2, "BLOCKS");
        blocks.iter().for_each(|(c, v)| put(*c, v));
        put(0, "ENDSEC");
        put(0, "SECTION");
        put(2, "ENTITIES");
        ents.iter().for_each(|(c, v)| put(*c, v));
        put(0, "ENDSEC");
        put(0, "EOF");
        s
    }

    fn close(a: Point2, b: Point2) -> bool {
        (a - b).norm() < 1e-9
    }

    #[test]
    fn lines_circles_arcs_in_inches() {
        let t = dxf(
            &[(9, "$INSUNITS"), (70, "1")],
            &[],
            &[
                (0, "LINE"),
                (8, "Cut"),
                (10, "0"),
                (20, "0"),
                (11, "2"),
                (21, "0"),
                (0, "CIRCLE"),
                (8, "Holes"),
                (10, "1"),
                (20, "1"),
                (40, "0.25"),
                (0, "ARC"),
                (10, "0"),
                (20, "0"),
                (40, "1"),
                (50, "0"),
                (51, "90"),
                (0, "POINT"),
                (10, "3"),
                (20, "4"),
                (67, "1"),
            ],
        );
        let d = read(t.as_bytes()).unwrap();
        assert_eq!(d.unit, 25.4);
        assert_eq!(d.items.len(), 3, "paper space is left out");
        assert_eq!(d.layers, ["Cut", "Holes", "0"]);
        let DxfGeom::Line(a, b) = d.items[0].geom else {
            panic!()
        };
        assert!(close(a, Point2::origin()) && close(b, Point2::new(50.8, 0.0)));
        assert_eq!(
            d.items[1].geom,
            DxfGeom::Circle {
                c: Point2::new(25.4, 25.4),
                r: 6.35
            }
        );
        let DxfGeom::Arc { r, start, end, .. } = d.items[2].geom else {
            panic!()
        };
        assert!(
            (r - 25.4).abs() < 1e-12
                && start == 0.0
                && (end - std::f64::consts::FRAC_PI_2).abs() < 1e-12
        );
    }

    /// An LWPOLYLINE with a bulge of 1 (a half circle) between its last two points.
    #[test]
    fn polyline_bulges_are_exact_arcs() {
        let t = dxf(
            &[],
            &[],
            &[
                (0, "LWPOLYLINE"),
                (90, "3"),
                (70, "1"),
                (10, "0"),
                (20, "0"),
                (10, "10"),
                (20, "0"),
                (42, "1"),
                (10, "10"),
                (20, "10"),
            ],
        );
        let d = read(t.as_bytes()).unwrap();
        assert_eq!(d.items.len(), 3);
        let DxfGeom::Arc { c, r, start, end } = d.items[1].geom else {
            panic!("{:?}", d.items[1])
        };
        assert!(
            close(c, Point2::new(10.0, 5.0)) && (r - 5.0).abs() < 1e-12,
            "{c} {r}"
        );
        // Counter-clockwise from (10, 0) round the right to (10, 10).
        assert!(
            (start + std::f64::consts::FRAC_PI_2).abs() < 1e-12
                && (end - std::f64::consts::FRAC_PI_2).abs() < 1e-12
        );
        // A negative bulge runs clockwise: the same arc stored counter-clockwise the other way.
        let t = dxf(
            &[],
            &[],
            &[
                (0, "LWPOLYLINE"),
                (90, "2"),
                (70, "0"),
                (10, "0"),
                (20, "0"),
                (42, "-1"),
                (10, "10"),
                (20, "0"),
            ],
        );
        let d = read(t.as_bytes()).unwrap();
        let DxfGeom::Arc { c, start, end, .. } = d.items[0].geom else {
            panic!()
        };
        assert!(close(c, Point2::new(5.0, 0.0)));
        // Clockwise from (0,0) to (10,0) passes over the top: stored from (10,0) to (0,0) ccw.
        // (Angles are taken modulo a turn: the centre lies a rounding error off the axis.)
        let tau = std::f64::consts::TAU;
        assert!(
            (start + 1e-9).rem_euclid(tau) < 1e-8
                && ((end - start).rem_euclid(tau) - std::f64::consts::PI).abs() < 1e-12,
            "{start} {end}"
        );
    }

    /// Nested blocks, a rotated and unequally scaled reference, an array, layer 0 taking the
    /// reference's layer.
    #[test]
    fn nested_blocks_expand_exactly() {
        let blocks = [
            (0, "BLOCK"),
            (2, "Hole"),
            (10, "0"),
            (20, "0"),
            (0, "CIRCLE"),
            (8, "0"),
            (10, "0"),
            (20, "0"),
            (40, "1"),
            (0, "ENDBLK"),
            (0, "BLOCK"),
            (2, "Plate"),
            (10, "5"),
            (20, "0"),
            (0, "LINE"),
            (8, "Edge"),
            (10, "5"),
            (20, "0"),
            (11, "15"),
            (21, "0"),
            (0, "INSERT"),
            (2, "Hole"),
            (10, "10"),
            (20, "0"),
            (0, "ENDBLK"),
        ];
        let ents = [
            (0, "INSERT"),
            (8, "Parts"),
            (2, "Plate"),
            (10, "100"),
            (20, "0"),
            (50, "90"),
            (0, "INSERT"),
            (8, "Parts"),
            (2, "Hole"),
            (10, "0"),
            (20, "0"),
            (41, "2"),
            (42, "1"),
            (0, "INSERT"),
            (8, "Grid"),
            (2, "Hole"),
            (10, "0"),
            (20, "50"),
            (70, "3"),
            (71, "2"),
            (44, "10"),
            (45, "20"),
        ];
        let d = read(dxf(&[], &blocks, &ents).as_bytes()).unwrap();
        // Plate turned 90° about its base (5, 0) placed at (100, 0): its line runs up from there.
        let DxfGeom::Line(a, b) = d.items[0].geom else {
            panic!()
        };
        assert!(
            close(a, Point2::new(100.0, 0.0)) && close(b, Point2::new(100.0, 10.0)),
            "{a} {b}"
        );
        assert_eq!(d.items[0].layer, "Edge");
        // Its hole (block base 0, at (10, 0) in the plate) lands at (100, 5), on the reference's layer.
        let DxfGeom::Circle { c, r } = d.items[1].geom else {
            panic!()
        };
        assert!(close(c, Point2::new(100.0, 5.0)) && (r - 1.0).abs() < 1e-12);
        assert_eq!(d.items[1].layer, "Parts");
        // Scaled 2 × 1: an ellipse with semi-axes 2 and 1.
        let DxfGeom::Ellipse { u, v, .. } = d.items[2].geom else {
            panic!("{:?}", d.items[2])
        };
        assert!((u.norm() - 2.0).abs() < 1e-12 && (v.norm() - 1.0).abs() < 1e-12);
        // 3 × 2 array.
        assert_eq!(d.items.len(), 3 + 6);
        assert!(d.items[3..].iter().all(|i| i.layer == "Grid"));
        let DxfGeom::Circle { c, .. } = d.items.last().unwrap().geom else {
            panic!()
        };
        assert!(close(c, Point2::new(20.0, 70.0)), "{c}");
    }

    #[test]
    fn mirrored_entities_and_ellipses_and_splines() {
        let t = dxf(
            &[],
            &[],
            &[
                // An arc drawn with its extrusion down −Z: seen from +Z it is mirrored in X.
                (0, "ARC"),
                (10, "10"),
                (20, "0"),
                (40, "2"),
                (50, "0"),
                (51, "90"),
                (210, "0"),
                (220, "0"),
                (230, "-1"),
                (0, "ELLIPSE"),
                (10, "0"),
                (20, "0"),
                (11, "4"),
                (21, "0"),
                (40, "0.5"),
                (41, "0"),
                (42, "3.141592653589793"),
                (0, "SPLINE"),
                (70, "8"),
                (71, "2"),
                (72, "6"),
                (73, "3"),
                (40, "0"),
                (40, "0"),
                (40, "0"),
                (40, "1"),
                (40, "1"),
                (40, "1"),
                (10, "0"),
                (20, "0"),
                (10, "1"),
                (20, "2"),
                (10, "2"),
                (20, "0"),
                (0, "MTEXT"),
                (10, "0"),
                (20, "0"),
                (40, "2.5"),
                (1, "{\\fArial;Part}\\PNo. 7"),
            ],
        );
        let d = read(t.as_bytes()).unwrap();
        let DxfGeom::Arc { c, start, end, .. } = d.items[0].geom else {
            panic!()
        };
        // (10,0) mirrored is (−10,0); 0..90° becomes 90..180°.
        assert!(close(c, Point2::new(-10.0, 0.0)));
        assert!(
            (start - std::f64::consts::FRAC_PI_2).abs() < 1e-12
                && (end - std::f64::consts::PI).abs() < 1e-12,
            "{start} {end}"
        );
        let DxfGeom::Ellipse {
            u, v, start, end, ..
        } = d.items[1].geom
        else {
            panic!()
        };
        assert!(
            (u - Vector2::new(4.0, 0.0)).norm() < 1e-12
                && (v - Vector2::new(0.0, 2.0)).norm() < 1e-12
        );
        assert!(start == 0.0 && (end - std::f64::consts::PI).abs() < 1e-12);
        let DxfGeom::Spline {
            degree,
            control,
            knots,
            ..
        } = &d.items[2].geom
        else {
            panic!()
        };
        assert_eq!((*degree, control.len(), knots.len()), (2, 3, 6));
        let DxfGeom::Text { text, height, .. } = &d.items[3].geom else {
            panic!()
        };
        assert_eq!((text.as_str(), *height), ("Part\nNo. 7", 2.5));
    }

    /// A plate outline of lines and a bulged corner, a hole, all on one layer, a note on
    /// another: inserted ends join, so the sketch has one region with a hole.
    #[test]
    fn inserted_outline_makes_a_profile() {
        let t = dxf(
            &[],
            &[],
            &[
                (0, "LWPOLYLINE"),
                (8, "Cut"),
                (90, "4"),
                (70, "1"),
                (10, "0"),
                (20, "0"),
                (10, "40"),
                (20, "0"),
                (42, "0.4142135623730951"),
                (10, "50"),
                (20, "10"),
                (10, "0"),
                (20, "30"),
                (0, "CIRCLE"),
                (8, "Cut"),
                (10, "15"),
                (20, "15"),
                (40, "5"),
                (0, "TEXT"),
                (8, "Notes"),
                (10, "0"),
                (20, "40"),
                (40, "3"),
                (1, "%%c10"),
                (0, "LINE"),
                (8, "Notes"),
                (10, "0"),
                (20, "50"),
                (11, "10"),
                (21, "50"),
            ],
        );
        let d = read(t.as_bytes()).unwrap();
        let mut sk = crate::sketch::Sketch::default();
        let o = InsertOptions {
            layers: vec!["Cut".into()],
            ..Default::default()
        };
        let rep = sk.clone().insert_dxf(&d, &o);
        assert_eq!(
            (rep.lines, rep.arcs, rep.circles, rep.skipped),
            (3, 1, 1, 0),
            "{rep:?}"
        );
        sk.insert_dxf(&d, &o);
        // 4 corners shared + the arc's centre + the circle's centre.
        assert_eq!(sk.points.len(), 6);
        // The plate round its hole, and the hole's disc: the outline closed only because its
        // ends were joined (apart, the disc would be the only region).
        assert_eq!(sk.regions().len(), 2);
        let all = crate::sketch::Sketch::default().insert_dxf(&d, &InsertOptions::default());
        assert_eq!(all.skipped, 1, "the text");
        assert!(matches!(&d.items[5].geom, DxfGeom::Text { text, .. } if text == "Ø10"));
    }

    /// A rational quadratic spline (a quarter circle) comes in as points on the circle; a
    /// clamped uniform cubic one exactly as the sketch's own spline.
    #[test]
    fn splines_exact_or_through_their_points() {
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let ctrl = [
            Point2::new(10.0, 0.0),
            Point2::new(10.0, 10.0),
            Point2::new(0.0, 10.0),
        ];
        let knots = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        for j in 0..=10 {
            let p = de_boor(2, &ctrl, &knots, &[1.0, w, 1.0], j as Real / 10.0).unwrap();
            assert!((p.coords.norm() - 10.0).abs() < 1e-12, "{p}");
        }
        let item = |geom| DxfItem {
            layer: "0".into(),
            geom,
        };
        let d = DxfDrawing {
            items: vec![
                item(DxfGeom::Spline {
                    degree: 2,
                    control: ctrl.to_vec(),
                    knots: knots.to_vec(),
                    weights: vec![1.0, w, 1.0],
                    fit: vec![],
                    closed: false,
                }),
                item(DxfGeom::Spline {
                    degree: 3,
                    control: (0..5)
                        .map(|i| Point2::new(i as Real * 10.0, (i % 2) as Real * 5.0))
                        .collect(),
                    knots: vec![0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0],
                    weights: vec![],
                    fit: vec![],
                    closed: false,
                }),
            ],
            ..Default::default()
        };
        let mut sk = crate::sketch::Sketch::default();
        let rep = sk.insert_dxf(&d, &InsertOptions::default());
        assert_eq!((rep.splines, rep.approximated), (2, 1), "{rep:?}");
        assert_eq!(sk.interp.len(), 1);
    }

    #[test]
    fn dwg_and_binary_and_not_dxf_are_named() {
        assert!(read(b"AC1032\0\0\0").unwrap_err().contains("DWG"));
        assert!(
            read(b"AutoCAD Binary DXF\r\n\x1a\0")
                .unwrap_err()
                .contains("binary")
        );
        assert!(read(b"hello").unwrap_err().contains("ENTITIES"));
    }
}
