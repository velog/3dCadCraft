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

use super::hlr::{ViewBend, ViewEdge, ViewLines};
use super::sheet::{Anchor, Prim, Weight};
use super::style::{AnnotationLook, Terminator, TextOrientation};

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
    /// Bend note on a bend of a flat pattern view: direction, angle and
    /// radius, on the bend's centre line.
    BendNote {
        /// The view.
        view: u32,
        /// The bend's centre-line middle when the note was placed (view
        /// plane): the note belongs to the nearest bend.
        bend: [f64; 2],
        /// The text's offset from its place on the line (sheet mm).
        #[serde(default)]
        offset: [f64; 2],
    },
    /// Parts list of a view's assembly (`parts`).
    PartsList(super::parts::PartsList),
    /// Bend table of a flat pattern view, with its bend tags (`bend_table`,
    /// format revision 28).
    BendTable(super::bend_table::BendTable),
    /// Item balloon on a component of an assembly view (`parts`).
    Balloon(super::parts::Balloon),
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
    /// Decimal places when it was placed; the dimension style's precision
    /// is drawn (a per-dimension override is to do).
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

/// The geometry of annotation `a` in the default (ISO) styles; `view` gives
/// a view's placement and lines.
pub fn resolve<'a>(
    a: &AnnotationKind,
    view: &dyn Fn(u32) -> Option<(Placement, &'a ViewLines)>,
) -> Result<Shown, Orphan> {
    resolve_with(a, view, &AnnotationLook::default())
}

/// [`resolve`] in the drawing's styles (`DrawingStyles::look`); parts
/// lists and balloons need [`resolve_in`].
pub fn resolve_with<'a>(
    a: &AnnotationKind,
    view: &dyn Fn(u32) -> Option<(Placement, &'a ViewLines)>,
    look: &AnnotationLook,
) -> Result<Shown, Orphan> {
    resolve_in(a, view, look, &super::parts::Tables::default())
}

/// [`resolve_with`] with the sheet's BOM tables (parts lists, balloons).
pub fn resolve_in<'a>(
    a: &AnnotationKind,
    view: &dyn Fn(u32) -> Option<(Placement, &'a ViewLines)>,
    look: &AnnotationLook,
    tables: &super::parts::Tables,
) -> Result<Shown, Orphan> {
    let st = Draw { look };
    match a {
        AnnotationKind::BendNote {
            view: v,
            bend,
            offset,
        } => {
            let (pl, lines) = view(*v).ok_or(Orphan::NoView)?;
            let b = nearest_bend(lines, *bend).ok_or_else(|| Orphan::Missing("bend".into()))?;
            Ok(Shown {
                prims: st.bend_note(b, pl, *offset),
                value: Some(b.angle),
            })
        }
        AnnotationKind::PartsList(pl) => {
            let t = tables.table_of(pl).ok_or(Orphan::NoView)?;
            Ok(Shown {
                prims: super::parts::table_prims(pl, &t, look),
                value: None,
            })
        }
        AnnotationKind::BendTable(bt) => {
            let (pl, lines) = view(bt.view).ok_or(Orphan::NoView)?;
            Ok(Shown {
                prims: super::bend_table::prims(bt, lines, pl, look),
                value: None,
            })
        }
        AnnotationKind::Balloon(b) => {
            let (pl, lines) = view(b.view).ok_or(Orphan::NoView)?;
            Ok(Shown {
                prims: super::parts::resolve_balloon(b, pl, lines, tables, look)?,
                value: None,
            })
        }
        AnnotationKind::Text { at, text, height } => {
            let spacing = look.leader_text.line_spacing;
            let prims = text
                .lines()
                .enumerate()
                .map(|(k, line)| Prim::Text {
                    at: [at[0], at[1] - k as f64 * height * spacing],
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
            Ok(Shown {
                prims: st.center_mark(c, r),
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
            let over = look.center.overshoot;
            Ok(Shown {
                prims: vec![Prim::Center {
                    a: add(c1, mul(u, -(r1 + over))),
                    b: add(c2, mul(u, r2 + over)),
                    width: look.center_weight,
                    chain: look.center_chain,
                }],
                value: None,
            })
        }
        AnnotationKind::Dimension(d) => st.dimension(d, view),
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
                prims: st.leader(rim, t, &note, dir[0] >= 0.0),
                value: None,
            })
        }
        AnnotationKind::LeaderText { tip, at, text } => {
            let d = sub(*tip, *at);
            let right = d[0] <= 0.0;
            Ok(Shown {
                prims: st.leader(*tip, *at, text, right),
                value: None,
            })
        }
    }
}

/// Draws annotations in a look.
struct Draw<'a> {
    look: &'a AnnotationLook,
}

impl Draw<'_> {
    /// A centre mark: the cross at the centre, a gap, then centre lines
    /// passing the circle by the overshoot.
    fn center_mark(&self, c: [f64; 2], r: f64) -> Vec<Prim> {
        let s = &self.look.center;
        let w = self.look.center_weight;
        let half = s.mark * 0.5;
        let mut out = Vec::new();
        for dir in [[1.0, 0.0], [0.0, 1.0]] {
            out.push(Prim::Line {
                a: add(c, mul(dir, -half)),
                b: add(c, mul(dir, half)),
                weight: Weight::Thin,
            });
            let from = half + s.gap;
            let to = r + s.overshoot;
            if to > from + 1e-6 {
                for sign in [1.0, -1.0] {
                    out.push(Prim::Center {
                        a: add(c, mul(dir, sign * from)),
                        b: add(c, mul(dir, sign * to)),
                        width: w,
                        chain: self.look.center_chain,
                    });
                }
            }
        }
        out
    }

    /// A leader from `tip` (terminator) to `at`, a shoulder under the text
    /// and the text's lines stacked above it.
    fn leader(&self, tip: [f64; 2], at: [f64; 2], text: &str, rightwards: bool) -> Vec<Prim> {
        let ts = &self.look.leader_text;
        let h = ts.height;
        let mut prims = Vec::new();
        let lines: Vec<&str> = text.lines().collect();
        let w = lines
            .iter()
            .map(|l| text_width(l, h) * ts.width_scale)
            .fold(0.0, f64::max)
            .max(4.0);
        let sign = if rightwards { 1.0 } else { -1.0 };
        let end = [at[0] + sign * w, at[1]];
        prims.push(line(tip, at));
        prims.push(line(at, end));
        let d = sub(tip, at);
        let len = norm(d);
        if len > 1e-9 {
            let l = &self.look.leader;
            terminator(
                &mut prims,
                tip,
                mul(d, 1.0 / len),
                l.terminator,
                l.arrow_size,
                l.arrow_height,
            );
        }
        let x0 = if rightwards { at[0] } else { end[0] };
        for (k, l) in lines.iter().rev().enumerate() {
            prims.push(Prim::Text {
                at: [x0, at[1] + 0.8 + k as f64 * h * ts.line_spacing],
                height: h,
                text: (*l).to_string(),
                anchor: Anchor::Left,
                angle: 0.0,
            });
        }
        prims
    }

    fn arrow(&self, out: &mut Vec<Prim>, tip: [f64; 2], dir: [f64; 2]) {
        let d = &self.look.dim;
        terminator(out, tip, dir, d.terminator, d.arrow_size, d.arrow_height);
    }

    fn text(&self, at: [f64; 2], t: &str, angle: f64) -> Prim {
        Prim::Text {
            at,
            height: self.look.dim_text.height,
            text: t.to_string(),
            anchor: Anchor::Center,
            angle,
        }
    }

    /// A bend note: `UP 90,00° R2,00` (the dimension style's precision and
    /// decimal marker) along the bend's centre line, above it, readable.
    fn bend_note(&self, b: &ViewBend, pl: Placement, offset: [f64; 2]) -> Vec<Prim> {
        let h = self.look.leader_text.height;
        let (a, c) = (pl.to_sheet(b.centre[0]), pl.to_sheet(b.centre[1]));
        let mut d = sub(c, a);
        // Readable from the bottom or the right.
        if d[0] < -1e-9 || (d[0].abs() <= 1e-9 && d[1] < 0.0) {
            d = mul(d, -1.0);
        }
        let len = norm(d).max(1e-9);
        let u = mul(d, 1.0 / len);
        let n = [-u[1], u[0]];
        let mid = mul(add(a, c), 0.5);
        // Above the bend zone (its half width) by 1 mm.
        let half = {
            let z = &b.zone;
            let (p, q) = (pl.to_sheet(z[0][0]), pl.to_sheet(z[1][0]));
            norm(sub(p, q)) * 0.5
        };
        let at = add(add(mid, mul(n, half + 1.0)), offset);
        let text = bend_note_text(b, &self.look.dim);
        // Dragged off the bend (past the zone or the line's ends): a
        // horizontal note with a leader to the nearest point of the line.
        let rel = sub(at, mid);
        let (s, t) = (dot(rel, u), dot(rel, n));
        let off = t.abs() > half + 1.0 + 1.5 * h || s.abs() > len * 0.5 + h;
        if off {
            let tip = add(mid, mul(u, s.clamp(-len * 0.5, len * 0.5)));
            let w = text_width(&text, h) * self.look.leader_text.width_scale;
            let right = at[0] >= tip[0];
            // The shoulder starts at the text's near end.
            let start = [
                at[0] - if right { w * 0.5 } else { -w * 0.5 },
                at[1] - h * 0.5 - 0.8,
            ];
            return self.leader(tip, start, &text, right);
        }
        vec![Prim::Text {
            at,
            height: h,
            text,
            anchor: Anchor::Center,
            angle: u[1].atan2(u[0]),
        }]
    }

    /// A value as the dimension style writes it.
    fn value(&self, v: f64, symbol: &str) -> String {
        let d = &self.look.dim;
        let mut t = format!("{:.*}", usize::from(d.precision), v);
        if !d.trailing_zeros && t.contains('.') {
            t = t.trim_end_matches('0').trim_end_matches('.').to_string();
        }
        if d.decimal_comma {
            t = t.replace('.', ",");
        }
        format!("{}{symbol}{t}{}", d.prefix, d.suffix)
    }

    fn dimension<'a>(
        &self,
        d: &Dimension,
        view: &dyn Fn(u32) -> Option<(Placement, &'a ViewLines)>,
    ) -> Result<Shown, Orphan> {
        let ds = &self.look.dim;
        let text_h = self.look.dim_text.height;
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
                let symbol = match (diameter, ds.diameter_symbol) {
                    (true, true) => "Ø",
                    (true, false) => "",
                    (false, _) => "R",
                };
                let value_text = self.value(if diameter { 2.0 * r } else { r }, symbol);
                let w = text_width(&value_text, text_h) * self.look.dim_text.width_scale;
                let end = [t[0] + shoulder * w, t[1]];
                prims.push(line(from, t));
                prims.push(line(t, end));
                self.arrow(&mut prims, rim, dir);
                if diameter {
                    self.arrow(&mut prims, add(cs, mul(dir, -rs)), mul(dir, -1.0));
                }
                let text_at = [t[0] + shoulder * w * 0.5, t[1] + ds.gap.min(1.0)];
                prims.push(self.text(text_at, &value_text, 0.0));
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
            if len > ds.origin_offset + 1e-6 {
                let dir = mul(sub(l, p), 1.0 / len);
                prims.push(line(
                    add(p, mul(dir, ds.origin_offset)),
                    add(l, mul(dir, ds.extension)),
                ));
            }
        }
        prims.push(line(la, lb));
        let along = sub(lb, la);
        let span = norm(along);
        if span > 1e-9 {
            let v = mul(along, 1.0 / span);
            self.arrow(&mut prims, la, mul(v, -1.0));
            self.arrow(&mut prims, lb, v);
        }
        // Text above the line, readable from the bottom or the right (ISO),
        // or always horizontal (ANSI).
        let mid = mul(add(la, lb), 0.5);
        let mut ang = u[1].atan2(u[0]);
        if ang > std::f64::consts::FRAC_PI_2 + 1e-9 {
            ang -= std::f64::consts::PI;
        } else if ang <= -std::f64::consts::FRAC_PI_2 + 1e-9 {
            ang += std::f64::consts::PI;
        }
        let text = self.value(value, "");
        let (at, ang) = match ds.orientation {
            TextOrientation::Aligned => {
                let up = [-ang.sin(), ang.cos()];
                (add(mid, mul(up, ds.gap)), ang)
            }
            TextOrientation::Horizontal => {
                if u[0].abs() >= u[1].abs() {
                    ([mid[0], mid[1] + ds.gap], 0.0)
                } else {
                    // Beside a vertical dimension line, on the text's side.
                    let w = text_width(&text, text_h);
                    let side = if t[0] >= mid[0] { 1.0 } else { -1.0 };
                    (
                        [mid[0] + side * (ds.gap + w * 0.5), mid[1] - text_h * 0.5],
                        0.0,
                    )
                }
            }
        };
        prims.push(self.text(at, &text, ang));
        Ok(Shown {
            prims,
            value: Some(value),
        })
    }
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

pub(super) fn line(a: [f64; 2], b: [f64; 2]) -> Prim {
    Prim::Line {
        a,
        b,
        weight: Weight::Thin,
    }
}

/// A terminator at `tip` pointing along `dir` (unit): `size` long,
/// `height` wide.
pub(super) fn terminator(
    out: &mut Vec<Prim>,
    tip: [f64; 2],
    dir: [f64; 2],
    kind: Terminator,
    size: f64,
    height: f64,
) {
    let back = add(tip, mul(dir, -size));
    let n = [-dir[1] * height * 0.5, dir[0] * height * 0.5];
    match kind {
        Terminator::Filled => out.push(Prim::Fill {
            pts: vec![tip, add(back, n), sub(back, n)],
        }),
        Terminator::Open => {
            out.push(line(tip, add(back, n)));
            out.push(line(tip, sub(back, n)));
        }
        Terminator::Dot => {
            let r = height * 0.5;
            out.push(Prim::Fill {
                pts: (0..12)
                    .map(|k| {
                        let a = std::f64::consts::TAU * k as f64 / 12.0;
                        [tip[0] + r * a.cos(), tip[1] + r * a.sin()]
                    })
                    .collect(),
            });
        }
        Terminator::Oblique => {
            // A 45° stroke through the tip.
            let k = std::f64::consts::FRAC_1_SQRT_2 * size * 0.5;
            let s = [(dir[0] - dir[1]) * k, (dir[1] + dir[0]) * k];
            out.push(Prim::Line {
                a: sub(tip, s),
                b: add(tip, s),
                weight: Weight::Medium,
            });
        }
        Terminator::None => {}
    }
}

/// The bend of `lines` whose centre-line middle is nearest `at`, within
/// a tenth of the view's size (a bend that moved a little stays found).
pub fn nearest_bend(lines: &ViewLines, at: [f64; 2]) -> Option<&ViewBend> {
    let (w, h) = lines.size();
    let tol = (w.max(h) * 0.1).max(1.0);
    lines
        .bends
        .iter()
        .map(|b| {
            let m = b.middle();
            ((m[0] - at[0]).hypot(m[1] - at[1]), b)
        })
        .filter(|(d, _)| *d <= tol)
        .min_by(|x, y| x.0.total_cmp(&y.0))
        .map(|(_, b)| b)
}

/// A bend note's text: direction, angle, radius (`DOWN 90,00° R2,00`).
pub fn bend_note_text(b: &ViewBend, d: &super::style::DimensionStyle) -> String {
    let num = |v: f64| {
        let mut t = format!("{:.*}", usize::from(d.precision), v);
        if !d.trailing_zeros && t.contains('.') {
            t = t.trim_end_matches('0').trim_end_matches('.').to_string();
        }
        if d.decimal_comma {
            t = t.replace('.', ",");
        }
        t
    };
    format!(
        "{} {}° R{}",
        if b.up { "UP" } else { "DOWN" },
        num(b.angle),
        num(b.radius)
    )
}

/// Approximate width of a text line (mm): 0.6 × height per character.
pub fn text_width(t: &str, height: f64) -> f64 {
    t.chars().count() as f64 * height * 0.6
}

pub(super) fn add(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] + b[0], a[1] + b[1]]
}

pub(super) fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

pub(super) fn mul(a: [f64; 2], k: f64) -> [f64; 2] {
    [a[0] * k, a[1] * k]
}

fn dot(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

pub(super) fn norm(a: [f64; 2]) -> f64 {
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
            AnnotationKind::BendNote { view, .. } => Some(*view),
            AnnotationKind::PartsList(p) => Some(p.view),
            AnnotationKind::BendTable(b) => Some(b.view),
            AnnotationKind::Balloon(b) => Some(b.view),
            AnnotationKind::Text { .. } | AnnotationKind::LeaderText { .. } => None,
        }
    }
}
