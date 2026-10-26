//! Annotations of a sheet (spec 4 §3.3): centre marks, centrelines,
//! dimensions and texts. Geometry is referenced by view id + persistent edge
//! name (rule 2), resolved every time from the view's lines: a model change
//! moves the annotation with its edge, and a missing edge leaves the
//! annotation *orphaned* (kept, drawn marked — never deleted silently).
//!
//! Dimension layout follows ISO 129-1: extension lines start 1 mm off the
//! object and pass the dimension line by 2 mm, filled arrows 3 mm long at
//! 1 : 3, text 3.5 mm above the dimension line and readable from the bottom
//! or the right.

use serde::{Deserialize, Serialize};

use super::hlr::{ViewEdge, ViewLines};
use super::sheet::{Anchor, Prim, Weight};

/// One annotation of a sheet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Annotation {
    /// Identity inside the drawing (never reused).
    pub id: u32,
    /// What it is.
    #[serde(flatten)]
    pub kind: AnnotationKind,
}

/// The annotation kinds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum AnnotationKind {
    /// Center Mark on a circle or arc.
    CenterMark {
        /// The view.
        view: u32,
        /// The circular edge.
        edge: String,
    },
    /// Centerline through the centres of two circles.
    Centerline {
        /// The view.
        view: u32,
        /// First circular edge.
        a: String,
        /// Second circular edge.
        b: String,
    },
    /// A dimension.
    Dimension(Dimension),
    /// Hole and Thread note on a hole's circle (its text comes from the
    /// Hole feature, so it follows a change of the hole).
    HoleNote {
        /// The view.
        view: u32,
        /// The hole's circular edge.
        edge: String,
        /// Text position, relative to the view's position (sheet mm).
        text: [f64; 2],
    },
    /// Text with a leader arrow.
    LeaderText {
        /// The arrow's tip (sheet mm).
        tip: [f64; 2],
        /// Where the text starts (sheet mm).
        at: [f64; 2],
        /// The text (lines split by `\n`).
        text: String,
    },
    /// Text on the sheet.
    Text {
        /// Lower left of the first line (sheet mm).
        at: [f64; 2],
        /// The text (lines split by `\n`).
        text: String,
        /// Cap height (mm).
        #[serde(default = "text_height")]
        height: f64,
    },
}

fn text_height() -> f64 {
    3.5
}

/// Which point of an edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PointRole {
    /// Its start.
    Start,
    /// Its end.
    End,
    /// The centre of its circle.
    Center,
}

/// A point of a view's geometry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PointRef {
    /// The view.
    pub view: u32,
    /// The edge (persistent name).
    pub edge: String,
    /// Which of its points.
    pub role: PointRole,
}

/// Dimension types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DimKind {
    /// Horizontal distance.
    Horizontal,
    /// Vertical distance.
    Vertical,
    /// True distance along the line between the points.
    Aligned,
    /// Diameter of a circle (`a` names the circle, role Center).
    Diameter,
    /// Radius of a circle or arc.
    Radius,
}

/// A dimension between view geometry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Dimension {
    /// Type.
    pub dim: DimKind,
    /// First point (the circle for Diameter / Radius).
    pub a: PointRef,
    /// Second point (linear dimensions).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub b: Option<PointRef>,
    /// Text position, relative to the view's position (sheet mm): the text
    /// moves with the view.
    pub text: [f64; 2],
    /// Decimal places shown (trailing zeros dropped).
    #[serde(default = "precision")]
    pub precision: u8,
}

fn precision() -> u8 {
    2
}

/// Where a view sits: what maps its view-plane points onto the sheet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// The view's position on the sheet.
    pub position: [f64; 2],
    /// Its scale.
    pub scale: f64,
    /// The view-plane point placed at `position` (`ViewLines::center`).
    pub center: [f64; 2],
}

impl Placement {
    /// A view-plane point on the sheet.
    pub fn to_sheet(&self, q: [f64; 2]) -> [f64; 2] {
        [
            self.position[0] + (q[0] - self.center[0]) * self.scale,
            self.position[1] + (q[1] - self.center[1]) * self.scale,
        ]
    }
}

/// A resolved annotation: what to draw, and its value (dimensions).
#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
    /// Lines, texts, arrows (sheet mm).
    pub prims: Vec<Prim>,
    /// The dimension's value in model mm.
    pub value: Option<f64>,
}

/// Why an annotation could not be drawn.
#[derive(Clone, Debug, PartialEq)]
pub enum Orphan {
    /// The view is gone or not computed yet.
    NoView,
    /// The view has no edge of that name (or not a circle where one is
    /// needed): the annotation is orphaned.
    Missing(String),
}

/// Extension line gap from the object (mm).
const GAP: f64 = 1.0;
/// Extension line overshoot past the dimension line (mm).
const OVER: f64 = 2.0;
/// Arrow length (mm).
const ARROW: f64 = 3.0;
/// Dimension text cap height (mm).
const TEXT_H: f64 = 3.5;
/// Centre lines pass a circle by (mm).
const CENTER_OVER: f64 = 2.0;

/// The geometry of annotation `a`; `view` gives a view's placement and
/// lines.
pub fn resolve<'a>(
    a: &AnnotationKind,
    view: &dyn Fn(u32) -> Option<(Placement, &'a ViewLines)>,
) -> Result<Shown, Orphan> {
    match a {
        AnnotationKind::Text { at, text, height } => {
            let prims = text
                .lines()
                .enumerate()
                .map(|(k, line)| Prim::Text {
                    at: [at[0], at[1] - k as f64 * height * 1.6],
                    height: *height,
                    text: line.to_string(),
                    anchor: Anchor::Left,
                    angle: 0.0,
                })
                .collect();
            Ok(Shown { prims, value: None })
        }
        AnnotationKind::CenterMark { view: v, edge } => {
            let (pl, lines) = view(*v).ok_or(Orphan::NoView)?;
            let (c, r) = circle(lines, edge)?;
            let (c, r) = (pl.to_sheet(c), r * pl.scale);
            let e = r + CENTER_OVER;
            Ok(Shown {
                prims: vec![
                    Prim::Center {
                        a: [c[0] - e, c[1]],
                        b: [c[0] + e, c[1]],
                    },
                    Prim::Center {
                        a: [c[0], c[1] - e],
                        b: [c[0], c[1] + e],
                    },
                ],
                value: None,
            })
        }
        AnnotationKind::Centerline { view: v, a, b } => {
            let (pl, lines) = view(*v).ok_or(Orphan::NoView)?;
            let (c1, r1) = circle(lines, a)?;
            let (c2, r2) = circle(lines, b)?;
            let (c1, c2) = (pl.to_sheet(c1), pl.to_sheet(c2));
            let (r1, r2) = (r1 * pl.scale, r2 * pl.scale);
            let d = sub(c2, c1);
            let len = norm(d);
            if len < 1e-9 {
                return Err(Orphan::Missing(b.clone()));
            }
            let u = [d[0] / len, d[1] / len];
            Ok(Shown {
                prims: vec![Prim::Center {
                    a: add(c1, mul(u, -(r1 + CENTER_OVER))),
                    b: add(c2, mul(u, r2 + CENTER_OVER)),
                }],
                value: None,
            })
        }
        AnnotationKind::Dimension(d) => dimension(d, view),
        AnnotationKind::HoleNote {
            view: v,
            edge,
            text,
        } => {
            let (pl, lines) = view(*v).ok_or(Orphan::NoView)?;
            let e = lines
                .edge(edge)
                .ok_or_else(|| Orphan::Missing(edge.clone()))?;
            let (c, r) = e.circle.ok_or_else(|| Orphan::Missing(edge.clone()))?;
            let note = e
                .note
                .clone()
                .ok_or_else(|| Orphan::Missing(edge.clone()))?;
            let cs = pl.to_sheet(c);
            let t = add(pl.position, *text);
            let mut dir = sub(t, cs);
            let len = norm(dir);
            dir = if len < 1e-9 {
                [1.0, 0.0]
            } else {
                mul(dir, 1.0 / len)
            };
            let rim = add(cs, mul(dir, r * pl.scale));
            Ok(Shown {
                prims: leader(rim, t, &note, dir[0] >= 0.0),
                value: None,
            })
        }
        AnnotationKind::LeaderText { tip, at, text } => {
            let d = sub(*tip, *at);
            let right = d[0] <= 0.0;
            Ok(Shown {
                prims: leader(*tip, *at, text, right),
                value: None,
            })
        }
    }
}

/// A leader from `tip` (arrow) to `at`, a shoulder under the text and the
/// text's lines stacked above it (ISO 128-22 / ISO 3098 3.5 mm).
fn leader(tip: [f64; 2], at: [f64; 2], text: &str, rightwards: bool) -> Vec<Prim> {
    let mut prims = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    let w = lines
        .iter()
        .map(|l| text_width(l, TEXT_H))
        .fold(0.0, f64::max)
        .max(4.0);
    let sign = if rightwards { 1.0 } else { -1.0 };
    let end = [at[0] + sign * w, at[1]];
    prims.push(line(tip, at));
    prims.push(line(at, end));
    let d = sub(tip, at);
    let len = norm(d);
    if len > 1e-9 {
        arrow(&mut prims, tip, mul(d, 1.0 / len));
    }
    let x0 = if rightwards { at[0] } else { end[0] };
    for (k, l) in lines.iter().rev().enumerate() {
        prims.push(Prim::Text {
            at: [x0, at[1] + 0.8 + k as f64 * TEXT_H * 1.6],
            height: TEXT_H,
            text: (*l).to_string(),
            anchor: Anchor::Left,
            angle: 0.0,
        });
    }
    prims
}

fn circle(lines: &ViewLines, edge: &str) -> Result<([f64; 2], f64), Orphan> {
    lines
        .edge(edge)
        .and_then(|e| e.circle)
        .ok_or_else(|| Orphan::Missing(edge.to_string()))
}

/// A point of an edge in view-plane mm.
pub fn point_of(e: &ViewEdge, role: PointRole) -> Option<[f64; 2]> {
    match role {
        PointRole::Start => Some(e.a),
        PointRole::End => Some(e.b),
        PointRole::Center => e.circle.map(|(c, _)| c),
    }
}

fn point<'a>(
    r: &PointRef,
    view: &dyn Fn(u32) -> Option<(Placement, &'a ViewLines)>,
) -> Result<([f64; 2], [f64; 2], Placement), Orphan> {
    let (pl, lines) = view(r.view).ok_or(Orphan::NoView)?;
    let e = lines
        .edge(&r.edge)
        .ok_or_else(|| Orphan::Missing(r.edge.clone()))?;
    let q = point_of(e, r.role).ok_or_else(|| Orphan::Missing(r.edge.clone()))?;
    Ok((q, pl.to_sheet(q), pl))
}

/// A value as dimensions write it: `precision` decimals, trailing zeros off.
pub fn format_value(v: f64, precision: u8) -> String {
    let t = format!("{:.*}", usize::from(precision), v);
    if t.contains('.') {
        t.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        t
    }
}

fn dimension<'a>(
    d: &Dimension,
    view: &dyn Fn(u32) -> Option<(Placement, &'a ViewLines)>,
) -> Result<Shown, Orphan> {
    let mut prims = Vec::new();
    match d.dim {
        DimKind::Diameter | DimKind::Radius => {
            let (pl, lines) = view(d.a.view).ok_or(Orphan::NoView)?;
            let (c, r) = circle(lines, &d.a.edge)?;
            let cs = pl.to_sheet(c);
            let rs = r * pl.scale;
            let t = add(pl.position, d.text);
            let mut dir = sub(t, cs);
            let len = norm(dir);
            if len < 1e-9 {
                dir = [1.0, 0.0];
            } else {
                dir = [dir[0] / len, dir[1] / len];
            }
            let diameter = d.dim == DimKind::Diameter;
            let rim = add(cs, mul(dir, rs));
            // The leader runs from the far rim (diameter) or the centre
            // (radius) to the text, with a short shoulder under the text.
            let from = if diameter { add(cs, mul(dir, -rs)) } else { cs };
            let shoulder = if dir[0] >= 0.0 { 1.0 } else { -1.0 };
            let value_text = format!(
                "{}{}",
                if diameter { "Ø" } else { "R" },
                format_value(if diameter { 2.0 * r } else { r }, d.precision)
            );
            let w = text_width(&value_text, TEXT_H);
            let end = [t[0] + shoulder * w, t[1]];
            prims.push(line(from, t));
            prims.push(line(t, end));
            arrow(&mut prims, rim, dir);
            if diameter {
                arrow(&mut prims, add(cs, mul(dir, -rs)), mul(dir, -1.0));
            }
            let text_at = [t[0] + shoulder * w * 0.5, t[1] + 0.8];
            prims.push(text(text_at, &value_text, 0.0));
            return Ok(Shown {
                prims,
                value: Some(if diameter { 2.0 * r } else { r }),
            });
        }
        DimKind::Horizontal | DimKind::Vertical | DimKind::Aligned => {}
    }
    let b = d.b.as_ref().ok_or(Orphan::NoView)?;
    let (qa, pa, pl) = point(&d.a, view)?;
    let (qb, pb, _) = point(b, view)?;
    let t = add(pl.position, d.text);
    // Direction u along the dimension, n square to it towards the text.
    let (u, value) = match d.dim {
        DimKind::Horizontal => ([1.0, 0.0], (qb[0] - qa[0]).abs()),
        DimKind::Vertical => ([0.0, 1.0], (qb[1] - qa[1]).abs()),
        _ => {
            let dd = sub(pb, pa);
            let l = norm(dd);
            if l < 1e-12 {
                ([1.0, 0.0], 0.0)
            } else {
                ([dd[0] / l, dd[1] / l], norm(sub(qb, qa)))
            }
        }
    };
    let mut n = [-u[1], u[0]];
    // The dimension line passes through the text point.
    let off_a = dot(sub(t, pa), n);
    if off_a < 0.0 {
        n = mul(n, -1.0);
    }
    let la = add(pa, mul(n, dot(sub(t, pa), n)));
    let lb = add(pb, mul(n, dot(sub(t, pb), n)));
    // Extension lines (skipped when the point lies on the dimension line).
    for (p, l) in [(pa, la), (pb, lb)] {
        let len = norm(sub(l, p));
        if len > GAP + 1e-6 {
            let dir = mul(sub(l, p), 1.0 / len);
            prims.push(line(add(p, mul(dir, GAP)), add(l, mul(dir, OVER))));
        }
    }
    prims.push(line(la, lb));
    let along = sub(lb, la);
    let span = norm(along);
    if span > 1e-9 {
        let v = mul(along, 1.0 / span);
        arrow(&mut prims, la, mul(v, -1.0));
        arrow(&mut prims, lb, v);
    }
    // Text above the line, readable from the bottom or the right.
    let mid = mul(add(la, lb), 0.5);
    let mut ang = u[1].atan2(u[0]);
    if ang > std::f64::consts::FRAC_PI_2 + 1e-9 {
        ang -= std::f64::consts::PI;
    } else if ang <= -std::f64::consts::FRAC_PI_2 + 1e-9 {
        ang += std::f64::consts::PI;
    }
    let up = [-ang.sin(), ang.cos()];
    let at = add(mid, mul(up, 1.0));
    prims.push(text(at, &format_value(value, d.precision), ang));
    Ok(Shown {
        prims,
        value: Some(value),
    })
}

fn line(a: [f64; 2], b: [f64; 2]) -> Prim {
    Prim::Line {
        a,
        b,
        weight: Weight::Thin,
    }
}

fn text(at: [f64; 2], t: &str, angle: f64) -> Prim {
    Prim::Text {
        at,
        height: TEXT_H,
        text: t.to_string(),
        anchor: Anchor::Center,
        angle,
    }
}

/// A filled arrow with its tip at `tip`, pointing along `dir` (unit).
fn arrow(out: &mut Vec<Prim>, tip: [f64; 2], dir: [f64; 2]) {
    let back = add(tip, mul(dir, -ARROW));
    let n = [-dir[1] * ARROW / 6.0, dir[0] * ARROW / 6.0];
    out.push(Prim::Fill {
        pts: vec![tip, add(back, n), sub(back, n)],
    });
}

/// Approximate width of a text line (mm): 0.6 × height per character.
pub fn text_width(t: &str, height: f64) -> f64 {
    t.chars().count() as f64 * height * 0.6
}

fn add(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] + b[0], a[1] + b[1]]
}

fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn mul(a: [f64; 2], k: f64) -> [f64; 2] {
    [a[0] * k, a[1] * k]
}

fn dot(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

fn norm(a: [f64; 2]) -> f64 {
    a[0].hypot(a[1])
}

impl Annotation {
    /// The view it belongs to (`None`: a sheet text).
    pub fn view(&self) -> Option<u32> {
        match &self.kind {
            AnnotationKind::CenterMark { view, .. } | AnnotationKind::Centerline { view, .. } => {
                Some(*view)
            }
            AnnotationKind::Dimension(d) => Some(d.a.view),
            AnnotationKind::HoleNote { view, .. } => Some(*view),
            AnnotationKind::Text { .. } | AnnotationKind::LeaderText { .. } => None,
        }
    }
}
