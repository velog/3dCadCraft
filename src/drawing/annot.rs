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
    /// Its own layer (else the one Object Defaults gives its kind).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<String>,
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
    /// Chamfer note on a chamfer's edge, measured from a reference edge
    /// (`chamfer_note`, format revision 33).
    ChamferNote(super::chamfer_note::ChamferNote),
    /// Text with a leader arrow.
    LeaderText {
        /// The arrow's tip (sheet mm).
        tip: [f64; 2],
        /// Where the text starts (sheet mm).
        at: [f64; 2],
        /// The text (lines split by `\n`).
        text: String,
        /// Its formatting (Format Text; none: plain).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rich: Option<super::rich::RichText>,
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
    /// Baseline / chain / ordinate dimension set (`dimset`, format
    /// revision 29).
    DimSet(super::dimset::DimSet),
    /// Hole table of a view's holes, with its tags and origin indicator
    /// (`hole_table`, format revision 31).
    HoleTable(super::hole_table::HoleTable),
    /// Item balloon on a component of an assembly view (`parts`).
    Balloon(super::parts::Balloon),
    /// General table: empty or read from a CSV file (`table`, format
    /// revision 36).
    Table(super::table::GeneralTable),
    /// Revision table (`revision`, format revision 36).
    RevisionTable(super::revision::RevisionTable),
    /// Revision tag (`revision`, format revision 36).
    RevisionTag(super::revision::RevisionTag),
    /// Revision cloud (`revision`, format revision 36).
    RevisionCloud(super::revision::RevisionCloud),
    /// Text on the sheet.
    Text {
        /// Lower left of the first line (sheet mm).
        at: [f64; 2],
        /// The text (lines split by `\n`).
        text: String,
        /// Cap height (mm).
        #[serde(default = "text_height")]
        height: f64,
        /// Its formatting (Format Text; none: plain).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rich: Option<super::rich::RichText>,
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
    /// Angle between two straight edges (`a` and `b` name them); the text's
    /// place picks the sector (format revision 38).
    Angle,
}

/// Where a dimension's arrowhead sits at one end.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArrowSide {
    /// Inside the extension lines when there is room, else outside.
    #[default]
    Auto,
    /// Inside, pointing out to the extension line.
    Inside,
    /// Outside, pointing in to the extension line.
    Outside,
}

fn is_auto(a: &[ArrowSide; 2]) -> bool {
    *a == [ArrowSide::Auto; 2]
}

fn is_zero(v: &f64) -> bool {
    v.abs() < 1e-12
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
    /// Edit Dimension: text, precision, tolerance, inspection (additive).
    #[serde(default, skip_serializing_if = "super::dimtext::DimFormat::is_plain")]
    pub format: super::dimtext::DimFormat,
    /// Linear dimensions: the text's distance from the middle of the
    /// dimension line, along it (sheet mm; dragging the text sets it).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub shift: f64,
    /// Arrowheads at the first and second end.
    #[serde(default, skip_serializing_if = "is_auto")]
    pub arrows: [ArrowSide; 2],
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
        AnnotationKind::DimSet(s) => st.dim_set(s, view),
        AnnotationKind::HoleTable(ht) => {
            let (pl, lines) = view(ht.view).ok_or(Orphan::NoView)?;
            Ok(Shown {
                prims: super::hole_table::prims(ht, lines, pl, look),
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
        AnnotationKind::Table(t) => Ok(Shown {
            prims: super::table::prims(t, look),
            value: None,
        }),
        AnnotationKind::RevisionTable(t) => Ok(Shown {
            prims: super::revision::prims(t, look),
            value: None,
        }),
        AnnotationKind::RevisionTag(t) => {
            let origin = match t.view {
                Some(v) => view(v).ok_or(Orphan::NoView)?.0.position,
                None => [0.0, 0.0],
            };
            Ok(Shown {
                prims: super::revision::tag_prims(t, origin, look),
                value: None,
            })
        }
        AnnotationKind::RevisionCloud(c) => {
            let origin = match c.view {
                Some(v) => view(v).ok_or(Orphan::NoView)?.0.position,
                None => [0.0, 0.0],
            };
            Ok(Shown {
                prims: super::revision::cloud_prims(c, origin, look.dim_weight),
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
        AnnotationKind::Text {
            at,
            text,
            height,
            rich,
        } if rich.is_some() || super::rich::has_glyphs(text) => Ok(Shown {
            prims: super::rich::layout(
                text,
                &rich.clone().unwrap_or_default(),
                *at,
                *height,
                look.leader_text.line_spacing,
            ),
            value: None,
        }),
        AnnotationKind::Text {
            at, text, height, ..
        } => {
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
        AnnotationKind::ChamferNote(n) => {
            let (pl, lines) = view(n.view).ok_or(Orphan::NoView)?;
            let (c, v) = n.measure(lines).map_err(Orphan::Missing)?;
            let k = n.along.clamp(0.0, 1.0);
            let tip = pl.to_sheet(add(c[0], mul(sub(c[1], c[0]), k)));
            let t = add(pl.position, n.text);
            let text = super::chamfer_note::text(n.format(), v, &look.dim);
            Ok(Shown {
                prims: st.leader(tip, t, &text, t[0] >= tip[0]),
                value: Some(v.distance1),
            })
        }
        AnnotationKind::LeaderText {
            tip,
            at,
            text,
            rich,
        } => {
            let d = sub(*tip, *at);
            let right = d[0] <= 0.0;
            let mut prims = st.leader(*tip, *at, text, right);
            let plain = super::rich::RichText::default();
            let rich = rich
                .as_ref()
                .or_else(|| super::rich::has_glyphs(text).then_some(&plain));
            if let Some(r) = rich {
                // The leader and its shoulder as for plain text; the text
                // formatted above the shoulder, its last line lowest.
                prims.retain(|p| !matches!(p, Prim::Text { .. }));
                let h = look.leader_text.height;
                let w = text
                    .lines()
                    .map(|l| text_width(l, h) * look.leader_text.width_scale)
                    .fold(0.0, f64::max)
                    .max(4.0);
                let x0 = if right { at[0] } else { at[0] - w };
                let mut r = r.clone();
                r.h = super::rich::HAlign::Left;
                r.v = super::rich::VAlign::Bottom;
                prims.extend(super::rich::layout(
                    text,
                    &r,
                    [x0, at[1] + 0.8],
                    h,
                    look.leader_text.line_spacing,
                ));
            }
            Ok(Shown { prims, value: None })
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

    /// A baseline / chain / ordinate set: its dimensions drawn as single
    /// ones (`set_members`); ordinates write each distance at the end of the
    /// point's extension line.
    fn dim_set<'a>(
        &self,
        s: &super::dimset::DimSet,
        view: &dyn Fn(u32) -> Option<(Placement, &'a ViewLines)>,
    ) -> Result<Shown, Orphan> {
        use super::dimset::SetKind;
        let ds = &self.look.dim;
        let mut prims = Vec::new();
        if s.kind != SetKind::Ordinate {
            for d in set_members(s, view, ds.spacing)? {
                prims.extend(self.dimension(&d, view)?.prims);
            }
            return Ok(Shown { prims, value: None });
        }
        let g = SetGeom::of(s, view)?;
        let (ax, nx) = (g.ax, 1 - g.ax);
        let h = self.look.dim_text.height;
        let c0 = g.c[s.origin].unwrap_or_default();
        for &j in &g.order {
            let p = g.sheet(j);
            let mut from = p;
            from[nx] += g.sign * ds.origin_offset;
            let mut to = p;
            to[nx] = g.at[nx];
            if (to[nx] - p[nx]) * g.sign > ds.origin_offset {
                prims.push(line(from, to));
            }
            let label = super::dimtext::Label::build(
                (g.c[j].unwrap_or_default() - c0).abs(),
                "",
                &Default::default(),
                ds,
                h,
                self.look.dim_text.width_scale,
            );
            let along = g.at[nx] + g.sign * (ds.gap + label.width * 0.5);
            let (centre, angle) = if ax == 0 {
                ([p[0] + h * 0.5, along], std::f64::consts::FRAC_PI_2)
            } else {
                ([along, p[1] - h * 0.5], 0.0)
            };
            prims.extend(label.prims(centre, angle));
        }
        Ok(Shown { prims, value: None })
    }

    /// The label of dimension `d` measuring `value`.
    fn label(&self, d: &Dimension, value: f64, symbol: &str) -> super::dimtext::Label {
        super::dimtext::Label::build(
            value,
            symbol,
            &d.format,
            &self.look.dim,
            self.look.dim_text.height,
            self.look.dim_text.width_scale,
        )
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
                let label = self.label(d, if diameter { 2.0 * r } else { r }, symbol);
                let w = label.width;
                let end = [t[0] + shoulder * w, t[1]];
                prims.push(line(from, t));
                prims.push(line(t, end));
                self.arrow(&mut prims, rim, dir);
                if diameter {
                    self.arrow(&mut prims, add(cs, mul(dir, -rs)), mul(dir, -1.0));
                }
                let text_at = [t[0] + shoulder * w * 0.5, t[1] + ds.gap.min(1.0)];
                prims.extend(label.prims(text_at, 0.0));
                return Ok(Shown {
                    prims,
                    value: Some(if diameter { 2.0 * r } else { r }),
                });
            }
            DimKind::Angle => return self.angle(d, view),
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
        let along = sub(lb, la);
        let span = norm(along);
        let v = if span > 1e-9 {
            mul(along, 1.0 / span)
        } else {
            u
        };
        let label = self.label(d, value, "");
        // The text slides along the line (`shift`); past an extension line
        // the dimension line runs on under it.
        let mid = add(mul(add(la, lb), 0.5), mul(v, d.shift));
        let half = label.width * 0.5 + ds.gap;
        let s_mid = dot(sub(mid, la), v);
        let (mut s0, mut s1) = (0.0_f64, span);
        if s_mid - half < 0.0 {
            s0 = s_mid - half;
        }
        if s_mid + half > span {
            s1 = s_mid + half;
        }
        // Arrowheads: inside when there is room for both, else outside with
        // a tail (or as set per end).
        let room = span >= 2.0 * ds.arrow_size + 1.0;
        let out = |k: usize| match d.arrows[k] {
            ArrowSide::Auto => !room,
            ArrowSide::Inside => false,
            ArrowSide::Outside => true,
        };
        let tail = ds.arrow_size + 2.0;
        if out(0) {
            s0 = s0.min(-tail);
        }
        if out(1) {
            s1 = s1.max(span + tail);
        }
        prims.push(line(add(la, mul(v, s0)), add(la, mul(v, s1))));
        if span > 1e-9 {
            let (d0, d1) = (
                if out(0) { v } else { mul(v, -1.0) },
                if out(1) { mul(v, -1.0) } else { v },
            );
            self.arrow(&mut prims, la, d0);
            self.arrow(&mut prims, lb, d1);
        }
        // Text above the line, readable from the bottom or the right (ISO),
        // or always horizontal (ANSI).
        let mut ang = u[1].atan2(u[0]);
        if ang > std::f64::consts::FRAC_PI_2 + 1e-9 {
            ang -= std::f64::consts::PI;
        } else if ang <= -std::f64::consts::FRAC_PI_2 + 1e-9 {
            ang += std::f64::consts::PI;
        }
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
                    let w = label.width;
                    let side = if t[0] >= mid[0] { 1.0 } else { -1.0 };
                    (
                        [mid[0] + side * (ds.gap + w * 0.5), mid[1] - text_h * 0.5],
                        0.0,
                    )
                }
            }
        };
        prims.extend(label.prims(at, ang));
        Ok(Shown {
            prims,
            value: Some(value),
        })
    }
}

impl Draw<'_> {
    /// An angular dimension: the arc about the crossing of the two edges'
    /// lines through the text, in the sector holding the text, extension
    /// lines out to it, arrowheads at its ends, the text on its middle.
    fn angle<'a>(
        &self,
        d: &Dimension,
        view: &dyn Fn(u32) -> Option<(Placement, &'a ViewLines)>,
    ) -> Result<Shown, Orphan> {
        let ds = &self.look.dim;
        let b = d.b.as_ref().ok_or(Orphan::NoView)?;
        let g = angle_geometry(d, b, view)?;
        let mut prims = Vec::new();
        let r = g.radius;
        let c = g.vertex;
        let at = |a: f64| [c[0] + r * a.cos(), c[1] + r * a.sin()];
        // Extension lines from an edge's far end out to the arc.
        for (dir, reach) in [(g.a0, g.reach[0]), (g.a0 + g.sweep, g.reach[1])] {
            if reach + ds.origin_offset < r {
                let u = [dir.cos(), dir.sin()];
                prims.push(line(
                    add(c, mul(u, reach + ds.origin_offset)),
                    add(c, mul(u, r + ds.extension)),
                ));
            }
        }
        let n = 48;
        let pts: Vec<[f64; 2]> = (0..=n)
            .map(|k| at(g.a0 + g.sweep * k as f64 / n as f64))
            .collect();
        for w in pts.windows(2) {
            prims.push(line(w[0], w[1]));
        }
        // Arrowheads along the arc's tangent, pointing out of it.
        let s = g.sweep.signum();
        let t0 = g.a0;
        let t1 = g.a0 + g.sweep;
        self.arrow(&mut prims, at(t0), [s * t0.sin(), -s * t0.cos()]);
        self.arrow(&mut prims, at(t1), [-s * t1.sin(), s * t1.cos()]);
        // The value in degrees, the angular precision.
        let mut st = ds.clone();
        st.precision = ds.angular_precision;
        st.suffix = format!("°{}", ds.suffix);
        let label = super::dimtext::Label::build(
            g.value.to_degrees(),
            "",
            &d.format,
            &st,
            self.look.dim_text.height,
            self.look.dim_text.width_scale,
        );
        let m = g.a0 + g.sweep * 0.5;
        let pos = add(at(m), mul([m.cos(), m.sin()], ds.gap));
        // Readable: turned with the arc's tangent, never upside down (ISO);
        // horizontal for ANSI.
        let mut ang = m - std::f64::consts::FRAC_PI_2;
        while ang > std::f64::consts::FRAC_PI_2 + 1e-9 {
            ang -= std::f64::consts::PI;
        }
        while ang <= -std::f64::consts::FRAC_PI_2 + 1e-9 {
            ang += std::f64::consts::PI;
        }
        let ang = match ds.orientation {
            TextOrientation::Aligned => ang,
            TextOrientation::Horizontal => 0.0,
        };
        prims.extend(label.prims(pos, ang));
        Ok(Shown {
            prims,
            value: Some(g.value.to_degrees()),
        })
    }
}

/// The geometry of an angular dimension on the sheet.
#[derive(Clone, Copy, Debug)]
pub struct AngleGeometry {
    /// Where the two edges' lines cross.
    pub vertex: [f64; 2],
    /// Arc radius (through the text).
    pub radius: f64,
    /// Direction of the arc's start (radians).
    pub a0: f64,
    /// Signed sweep of the arc (radians, |sweep| < π).
    pub sweep: f64,
    /// The measured angle (radians, 0..π).
    pub value: f64,
    /// How far each edge reaches from the vertex along its ray.
    pub reach: [f64; 2],
}

/// The arc of angular dimension `d` between its edges `d.a` and `b`.
pub fn angle_geometry<'a>(
    d: &Dimension,
    b: &PointRef,
    view: &dyn Fn(u32) -> Option<(Placement, &'a ViewLines)>,
) -> Result<AngleGeometry, Orphan> {
    let ends = |r: &PointRef| -> Result<([f64; 2], [f64; 2], Placement), Orphan> {
        let s = PointRef {
            role: PointRole::Start,
            ..r.clone()
        };
        let e = PointRef {
            role: PointRole::End,
            ..r.clone()
        };
        let (_, p, pl) = point(&s, view)?;
        let (_, q, _) = point(&e, view)?;
        Ok((p, q, pl))
    };
    let (a0, a1, pl) = ends(&d.a)?;
    let (b0, b1, _) = ends(b)?;
    let (da, db) = (sub(a1, a0), sub(b1, b0));
    let den = da[0] * db[1] - da[1] * db[0];
    if den.abs() < 1e-9 * norm(da).max(1e-9) * norm(db).max(1e-9) {
        return Err(Orphan::Missing("parallel edges".into()));
    }
    let w = sub(b0, a0);
    let k = (w[0] * db[1] - w[1] * db[0]) / den;
    let c = add(a0, mul(da, k));
    let t = add(pl.position, d.text);
    let r = norm(sub(t, c)).max(1.0);
    let ta = (t[1] - c[1]).atan2(t[0] - c[0]);
    let unit = |v: [f64; 2]| mul(v, 1.0 / norm(v));
    let (ua, ub) = (unit(da), unit(db));
    let reach =
        |p: [f64; 2], q: [f64; 2], u: [f64; 2]| dot(sub(p, c), u).max(dot(sub(q, c), u)).max(0.0);
    // The sector holding the text: the rays ±ua, ±ub around it.
    let mut best: Option<AngleGeometry> = None;
    for sa in [1.0, -1.0] {
        for sb in [1.0, -1.0] {
            let (ra, rb) = (mul(ua, sa), mul(ub, sb));
            let a_ang = ra[1].atan2(ra[0]);
            let mut sweep = rb[1].atan2(rb[0]) - a_ang;
            while sweep > std::f64::consts::PI {
                sweep -= std::f64::consts::TAU;
            }
            while sweep < -std::f64::consts::PI {
                sweep += std::f64::consts::TAU;
            }
            let mut dt = ta - a_ang;
            while dt > std::f64::consts::PI {
                dt -= std::f64::consts::TAU;
            }
            while dt < -std::f64::consts::PI {
                dt += std::f64::consts::TAU;
            }
            let inside = if sweep >= 0.0 {
                dt >= -1e-9 && dt <= sweep + 1e-9
            } else {
                dt <= 1e-9 && dt >= sweep - 1e-9
            };
            if inside {
                best = Some(AngleGeometry {
                    vertex: c,
                    radius: r,
                    a0: a_ang,
                    sweep,
                    value: sweep.abs(),
                    reach: [reach(a0, a1, ra), reach(b0, b1, rb)],
                });
            }
        }
    }
    best.ok_or_else(|| Orphan::Missing("angle".into()))
}

/// A point in its view's plane, on the sheet, and the view's placement.
type ViewPoint = ([f64; 2], [f64; 2], Placement);

/// A linear dimension's two points on the sheet, its view's placement and
/// lines (`None`: a diameter / radius or a missing point).
pub fn dim_points<'a>(
    d: &Dimension,
    view: &dyn Fn(u32) -> Option<(Placement, &'a ViewLines)>,
) -> Option<([f64; 2], [f64; 2], Placement, &'a ViewLines)> {
    let b = d.b.as_ref()?;
    let (_, pa, pl) = point(&d.a, view).ok()?;
    let (_, pb, _) = point(b, view).ok()?;
    let (_, lines) = view(d.a.view)?;
    Some((pa, pb, pl, lines))
}

/// A linear dimension's line ends on the sheet (where its arrowheads sit)
/// and its direction from the first to the second.
pub fn linear_ends<'a>(
    d: &Dimension,
    view: &dyn Fn(u32) -> Option<(Placement, &'a ViewLines)>,
) -> Option<([f64; 2], [f64; 2], [f64; 2])> {
    let (pa, pb, pl, _) = dim_points(d, view)?;
    let u = match d.dim {
        DimKind::Horizontal => [1.0, 0.0],
        DimKind::Vertical => [0.0, 1.0],
        DimKind::Aligned => {
            let dd = sub(pb, pa);
            let l = norm(dd);
            if l < 1e-12 {
                return None;
            }
            [dd[0] / l, dd[1] / l]
        }
        _ => return None,
    };
    let n = [-u[1], u[0]];
    let t = add(pl.position, d.text);
    let la = add(pa, mul(n, dot(sub(t, pa), n)));
    let lb = add(pb, mul(n, dot(sub(t, pb), n)));
    let v = sub(lb, la);
    let l = norm(v);
    Some((la, lb, if l > 1e-12 { mul(v, 1.0 / l) } else { u }))
}

/// A dimension set's points resolved: sheet points, coordinates along the
/// direction, drawing order, placement and the side it lies on.
struct SetGeom {
    ax: usize,
    pts: Vec<Option<[f64; 2]>>,
    c: Vec<Option<f64>>,
    order: Vec<usize>,
    at: [f64; 2],
    sign: f64,
    pl: Placement,
}

impl SetGeom {
    fn of<'a>(
        s: &super::dimset::DimSet,
        view: &dyn Fn(u32) -> Option<(Placement, &'a ViewLines)>,
    ) -> Result<SetGeom, Orphan> {
        let ax = s.dir.axis();
        let nx = 1 - ax;
        let got: Vec<Option<ViewPoint>> = s.points.iter().map(|r| point(r, view).ok()).collect();
        let origin = got.get(s.origin).and_then(|p| p.as_ref()).ok_or_else(|| {
            Orphan::Missing(
                s.points
                    .get(s.origin)
                    .map(|p| p.edge.clone())
                    .unwrap_or_default(),
            )
        })?;
        let pl = origin.2;
        let at = add(pl.position, s.at);
        let c: Vec<Option<f64>> = got.iter().map(|p| p.as_ref().map(|p| p.0[ax])).collect();
        let pts: Vec<Option<[f64; 2]>> = got.iter().map(|p| p.as_ref().map(|p| p.1)).collect();
        let mut order = s.order(&c);
        // Of the points at one coordinate, the one nearest the placement
        // carries the extension line (it never crosses the part).
        for k in &mut order {
            let ck = c[*k].unwrap_or_default();
            let near = |j: usize| pts[j].map_or(f64::MAX, |p| (p[nx] - at[nx]).abs());
            if let Some(best) = (0..c.len())
                .filter(|&j| c[j].is_some_and(|cj| (cj - ck).abs() < 1e-6))
                .min_by(|&a, &b| near(a).total_cmp(&near(b)))
            {
                *k = best;
            }
        }
        let mid = order
            .iter()
            .map(|&k| pts[k].map_or(0.0, |p| p[nx]))
            .sum::<f64>()
            / order.len().max(1) as f64;
        let sign = if at[nx] >= mid { 1.0 } else { -1.0 };
        Ok(SetGeom {
            ax,
            pts,
            c,
            order,
            at,
            sign,
            pl,
        })
    }

    fn sheet(&self, k: usize) -> [f64; 2] {
        self.pts.get(k).copied().flatten().unwrap_or([0.0; 2])
    }
}

/// The single dimensions of a baseline or chain set: baseline from the
/// origin to each point, the nearest first, `spacing` apart; chain between
/// neighbours on one line. An ordinate set has none.
pub fn set_members<'a>(
    s: &super::dimset::DimSet,
    view: &dyn Fn(u32) -> Option<(Placement, &'a ViewLines)>,
    spacing: f64,
) -> Result<Vec<Dimension>, Orphan> {
    use super::dimset::SetKind;
    let g = SetGeom::of(s, view)?;
    let (ax, nx) = (g.ax, 1 - g.ax);
    let single = |a: usize, b: usize, line: f64| {
        let (pa, pb) = (g.sheet(a), g.sheet(b));
        let mut t = [0.0; 2];
        t[ax] = (pa[ax] + pb[ax]) * 0.5;
        t[nx] = line;
        Dimension {
            shift: 0.0,
            arrows: Default::default(),
            dim: if ax == 0 {
                DimKind::Horizontal
            } else {
                DimKind::Vertical
            },
            a: s.points[a].clone(),
            b: Some(s.points[b].clone()),
            text: sub(t, g.pl.position),
            precision: 2,
            format: Default::default(),
        }
    };
    Ok(match s.kind {
        SetKind::Baseline => g
            .order
            .iter()
            .skip(1)
            .enumerate()
            .map(|(k, &j)| single(g.order[0], j, g.at[nx] + g.sign * spacing * k as f64))
            .collect(),
        SetKind::Chain => g
            .order
            .windows(2)
            .map(|w| single(w[0], w[1], g.at[nx]))
            .collect(),
        SetKind::Ordinate => Vec::new(),
    })
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
    /// The layer it is drawn on: its own, else the Object Defaults' layer
    /// of its kind (Inventor: dimensions on Dimension, centre marks and
    /// centrelines on theirs, every note, table, balloon and tag on Symbol).
    pub fn layer_name<'a>(&'a self, d: &'a super::style::ObjectDefaults) -> &'a str {
        if let Some(l) = &self.layer {
            return l;
        }
        match &self.kind {
            AnnotationKind::Dimension(_) | AnnotationKind::DimSet(_) => &d.dimension_layer,
            AnnotationKind::CenterMark { .. } => &d.center_layer,
            AnnotationKind::Centerline { .. } => &d.centerline_layer,
            _ => &d.symbol_layer,
        }
    }

    /// The view it belongs to (`None`: a sheet text).
    pub fn view(&self) -> Option<u32> {
        match &self.kind {
            AnnotationKind::CenterMark { view, .. } | AnnotationKind::Centerline { view, .. } => {
                Some(*view)
            }
            AnnotationKind::Dimension(d) => Some(d.a.view),
            AnnotationKind::HoleNote { view, .. } => Some(*view),
            AnnotationKind::ChamferNote(n) => Some(n.view),
            AnnotationKind::BendNote { view, .. } => Some(*view),
            AnnotationKind::PartsList(p) => Some(p.view),
            AnnotationKind::BendTable(b) => Some(b.view),
            AnnotationKind::DimSet(s) => s.view(),
            AnnotationKind::HoleTable(h) => Some(h.view),
            AnnotationKind::Balloon(b) => Some(b.view),
            AnnotationKind::RevisionTag(t) => t.view,
            AnnotationKind::RevisionCloud(c) => c.view,
            AnnotationKind::Text { .. }
            | AnnotationKind::LeaderText { .. }
            | AnnotationKind::Table(_)
            | AnnotationKind::RevisionTable(_) => None,
        }
    }
}
