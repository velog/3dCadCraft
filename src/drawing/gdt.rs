//! Symbols of machined part drawings (spec 4.7, Symbols panel; format
//! revision 76): Surface Texture (ISO 1302 / ISO 21920-1 graphic symbol and
//! its requirement fields), Feature Control Frame (ISO 1101) and Datum
//! Identifier (ISO 5459). Each is placed on the sheet or attached to a view
//! (then its points are relative to the view's position and follow it),
//! with an optional leader to an edge. Every glyph is drawn as lines: the
//! sheet font has no GD&T characters.

use serde::{Deserialize, Serialize};

use super::annot::{line, terminator, text_width};
use super::sheet::{Anchor, Prim};
use super::style::AnnotationLook;

/// One placed symbol.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MarkSymbol {
    /// The view it belongs to (`at` and `tip` then relative to the view's
    /// position).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<u32>,
    /// The symbol's reference point (sheet mm, or relative to the view):
    /// Surface Texture: the root of the check mark; Feature Control Frame:
    /// the frame's left middle; Datum Identifier: the box's centre.
    pub at: [f64; 2],
    /// The leader's end on the geometry; `None`: no leader.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tip: Option<[f64; 2]>,
    /// What it is.
    pub mark: Mark,
}

/// The symbol kinds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "symbol")]
pub enum Mark {
    /// Surface Texture.
    SurfaceTexture(SurfaceTexture),
    /// Feature Control Frame.
    ControlFrame(ControlFrame),
    /// Datum Identifier.
    Datum {
        /// The datum letter.
        letter: String,
    },
}

/// Surface Type of a surface texture symbol.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SurfaceType {
    /// Any manufacturing process.
    #[default]
    Basic,
    /// Material removal required (a bar closes the V).
    Removal,
    /// Material removal prohibited (a circle in the V).
    NoRemoval,
}

impl SurfaceType {
    /// Every type, in the dialog's order.
    pub const ALL: [Self; 3] = [Self::Basic, Self::Removal, Self::NoRemoval];

    /// Its name in the dialog.
    pub fn label(self) -> &'static str {
        match self {
            Self::Basic => "Basic",
            Self::Removal => "Material removal required",
            Self::NoRemoval => "Material removal prohibited",
        }
    }
}

/// A surface texture symbol's settings and requirement fields (ISO 1302
/// positions a–e).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SurfaceTexture {
    /// The graphic symbol's kind.
    #[serde(default)]
    pub kind: SurfaceType,
    /// The same texture all round the outline (a circle at the corner).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub all_round: bool,
    /// The long leg's horizontal line even with no requirement over it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub force_tail: bool,
    /// Position a: the first requirement (e.g. `Ra 3.2`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub a: String,
    /// Position b: a second requirement.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub b: String,
    /// Position c: the production method.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub c: String,
    /// Position d: the lay symbol (`=`, `⊥`, `X`, `M`, `C`, `R`, `P`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub lay: String,
    /// Position e: the machining allowance.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub e: String,
}

/// The geometric characteristic of a control frame row (ISO 1101).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Characteristic {
    /// No symbol.
    #[default]
    None,
    /// Straightness.
    Straightness,
    /// Flatness.
    Flatness,
    /// Circularity (roundness).
    Circularity,
    /// Cylindricity.
    Cylindricity,
    /// Profile of a line.
    LineProfile,
    /// Profile of a surface.
    SurfaceProfile,
    /// Angularity.
    Angularity,
    /// Perpendicularity.
    Perpendicularity,
    /// Parallelism.
    Parallelism,
    /// Position.
    Position,
    /// Concentricity / coaxiality.
    Concentricity,
    /// Symmetry.
    Symmetry,
    /// Circular run-out.
    CircularRunout,
    /// Total run-out.
    TotalRunout,
}

impl Characteristic {
    /// Every characteristic, in the dialog's order.
    pub const ALL: [Self; 15] = [
        Self::None,
        Self::Straightness,
        Self::Flatness,
        Self::Circularity,
        Self::Cylindricity,
        Self::LineProfile,
        Self::SurfaceProfile,
        Self::Angularity,
        Self::Perpendicularity,
        Self::Parallelism,
        Self::Position,
        Self::Concentricity,
        Self::Symmetry,
        Self::CircularRunout,
        Self::TotalRunout,
    ];

    /// Its name in the dialog.
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "(none)",
            Self::Straightness => "Straightness",
            Self::Flatness => "Flatness",
            Self::Circularity => "Circularity",
            Self::Cylindricity => "Cylindricity",
            Self::LineProfile => "Profile of a line",
            Self::SurfaceProfile => "Profile of a surface",
            Self::Angularity => "Angularity",
            Self::Perpendicularity => "Perpendicularity",
            Self::Parallelism => "Parallelism",
            Self::Position => "Position",
            Self::Concentricity => "Concentricity",
            Self::Symmetry => "Symmetry",
            Self::CircularRunout => "Circular runout",
            Self::TotalRunout => "Total runout",
        }
    }

    /// A tolerance of form needs no datum (ISO 1101 Table 2).
    pub fn is_form(self) -> bool {
        matches!(
            self,
            Self::Straightness | Self::Flatness | Self::Circularity | Self::Cylindricity
        )
    }
}

/// One row of a feature control frame.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FrameRow {
    /// The characteristic.
    #[serde(default)]
    pub symbol: Characteristic,
    /// The tolerance (`0.05`, `Ø0.1`, with modifiers as text `(M)`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tolerance: String,
    /// Datum references, primary to tertiary (empty ones left out).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub datums: Vec<String>,
}

/// A feature control frame: rows, a note above.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ControlFrame {
    /// The rows (one or more).
    pub rows: Vec<FrameRow>,
    /// The note above the frame (e.g. `2x`, `6x Ø8`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

/// What a frame row's cells hold (for its layout): the symbol cell, then
/// the tolerance, then each datum.
fn row_cells(r: &FrameRow) -> Vec<String> {
    let mut out = vec![r.tolerance.clone()];
    out.extend(r.datums.iter().filter(|d| !d.trim().is_empty()).cloned());
    out
}

fn add(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] + b[0], a[1] + b[1]]
}

fn circle(c: [f64; 2], r: f64, n: usize) -> Vec<Prim> {
    let pts: Vec<[f64; 2]> = (0..=n)
        .map(|k| {
            let t = std::f64::consts::TAU * k as f64 / n as f64;
            [c[0] + r * t.cos(), c[1] + r * t.sin()]
        })
        .collect();
    pts.windows(2).map(|w| line(w[0], w[1])).collect()
}

fn text(at: [f64; 2], h: f64, t: &str, anchor: Anchor) -> Prim {
    Prim::Text {
        at,
        height: h,
        text: t.to_string(),
        anchor,
        angle: 0.0,
    }
}

/// The surface texture symbol's lines and texts about its root `o` (text
/// height `h`): ISO 1302's proportions (short leg 1.4 h, long leg 3 h, both
/// at 60° to the surface).
pub fn surface_prims(s: &SurfaceTexture, o: [f64; 2], h: f64) -> Vec<Prim> {
    let (c60, s60) = (0.5, 3f64.sqrt() * 0.5);
    let h1 = 1.4 * h;
    let h2 = 3.0 * h;
    let short = [o[0] - h1 / s60 * c60, o[1] + h1];
    let long = [o[0] + h2 / s60 * c60, o[1] + h2];
    let mut out = vec![line(o, short), line(o, long)];
    match s.kind {
        SurfaceType::Basic => {}
        SurfaceType::Removal => {
            // The bar closing the V at the short leg's height.
            let r = [o[0] + h1 / s60 * c60, o[1] + h1];
            out.push(line(short, r));
        }
        SurfaceType::NoRemoval => {
            // The circle touching both legs, its top at the short leg's
            // height.
            let r = h1 / (1.0 + 1.0 / c60) * 1.0;
            out.extend(circle([o[0], o[1] + h1 - r], r, 24));
        }
    }
    // The horizontal line from the long leg, as long as the texts over and
    // under it.
    let w = [&s.a, &s.b, &s.c]
        .iter()
        .map(|t| text_width(t, h))
        .fold(0.0, f64::max);
    if w > 0.0 || s.force_tail || s.all_round {
        let end = add(long, [w.max(h) + h * 0.5, 0.0]);
        out.push(line(long, end));
        if s.all_round {
            out.extend(circle(long, h * 0.35, 16));
        }
    }
    let x = long[0] + h * 0.3;
    if !s.c.is_empty() {
        out.push(text([x, long[1] + h * 0.4], h, &s.c, Anchor::Left));
    }
    if !s.a.is_empty() {
        out.push(text([x, long[1] - h * 1.4], h, &s.a, Anchor::Left));
    }
    if !s.b.is_empty() {
        out.push(text([x, long[1] - h * 2.8], h, &s.b, Anchor::Left));
    }
    if !s.lay.is_empty() {
        out.push(text(
            [o[0] + h * 1.1, o[1] + h * 0.2],
            h,
            &s.lay,
            Anchor::Left,
        ));
    }
    if !s.e.is_empty() {
        let w = text_width(&s.e, h);
        out.push(text(
            [short[0] - h * 0.3 - w, o[1] + h * 0.2],
            h,
            &s.e,
            Anchor::Left,
        ));
    }
    out
}

/// The characteristic's glyph in a cell `size` high centred on `c`.
pub fn characteristic_prims(k: Characteristic, c: [f64; 2], size: f64) -> Vec<Prim> {
    let u = size * 0.32;
    let p = |x: f64, y: f64| [c[0] + x * u, c[1] + y * u];
    let l = |a: [f64; 2], b: [f64; 2]| line(a, b);
    match k {
        Characteristic::None => Vec::new(),
        Characteristic::Straightness => vec![l(p(-1.0, 0.0), p(1.0, 0.0))],
        Characteristic::Flatness => vec![
            l(p(-1.0, -0.5), p(0.6, -0.5)),
            l(p(0.6, -0.5), p(1.0, 0.5)),
            l(p(1.0, 0.5), p(-0.6, 0.5)),
            l(p(-0.6, 0.5), p(-1.0, -0.5)),
        ],
        Characteristic::Circularity => circle(c, u * 0.9, 24),
        Characteristic::Cylindricity => {
            let mut v = circle(c, u * 0.7, 24);
            v.push(l(p(-1.0, -1.0), p(-0.3, 1.0)));
            v.push(l(p(0.3, -1.0), p(1.0, 1.0)));
            v
        }
        Characteristic::LineProfile | Characteristic::SurfaceProfile => {
            let pts: Vec<[f64; 2]> = (0..=12)
                .map(|i| {
                    let t = std::f64::consts::PI * f64::from(i) / 12.0;
                    p(-t.cos(), -0.5 + t.sin())
                })
                .collect();
            let mut v: Vec<Prim> = pts.windows(2).map(|w| l(w[0], w[1])).collect();
            if k == Characteristic::SurfaceProfile {
                v.push(l(p(-1.0, -0.5), p(1.0, -0.5)));
            }
            v
        }
        Characteristic::Angularity => vec![
            l(p(-1.0, -0.8), p(1.0, -0.8)),
            l(p(-1.0, -0.8), p(0.8, 0.9)),
        ],
        Characteristic::Perpendicularity => {
            vec![l(p(-1.0, -0.8), p(1.0, -0.8)), l(p(0.0, -0.8), p(0.0, 1.0))]
        }
        Characteristic::Parallelism => {
            vec![l(p(-0.9, -0.9), p(-0.1, 0.9)), l(p(0.1, -0.9), p(0.9, 0.9))]
        }
        Characteristic::Position => {
            let mut v = circle(c, u * 0.65, 24);
            v.push(l(p(-1.0, 0.0), p(1.0, 0.0)));
            v.push(l(p(0.0, -1.0), p(0.0, 1.0)));
            v
        }
        Characteristic::Concentricity => {
            let mut v = circle(c, u * 0.95, 24);
            v.extend(circle(c, u * 0.45, 16));
            v
        }
        Characteristic::Symmetry => vec![
            l(p(-0.6, 0.6), p(0.6, 0.6)),
            l(p(-1.0, 0.0), p(1.0, 0.0)),
            l(p(-0.6, -0.6), p(0.6, -0.6)),
        ],
        Characteristic::CircularRunout | Characteristic::TotalRunout => {
            let mut v = Vec::new();
            let arrow = |v: &mut Vec<Prim>, x: f64| {
                let (a, b) = (p(x - 0.5, -1.0), p(x + 0.5, 1.0));
                v.push(l(a, b));
                let back = [b[0] - (b[0] - a[0]) * 0.35, b[1] - (b[1] - a[1]) * 0.35];
                let d = [b[0] - a[0], b[1] - a[1]];
                let n = [-d[1] * 0.12, d[0] * 0.12];
                v.push(Prim::Fill {
                    pts: vec![b, add(back, n), [back[0] - n[0], back[1] - n[1]]],
                });
            };
            if k == Characteristic::CircularRunout {
                arrow(&mut v, 0.0);
            } else {
                arrow(&mut v, -0.4);
                arrow(&mut v, 0.6);
                v.push(l(p(-1.2, -1.0), p(1.4, -1.0)));
            }
            v
        }
    }
}

/// A frame's cell widths per row (sheet mm) and its height per row.
fn frame_layout(f: &ControlFrame, h: f64) -> (Vec<Vec<f64>>, f64) {
    let row_h = 2.0 * h;
    let widths = f
        .rows
        .iter()
        .map(|r| {
            let mut w = vec![row_h];
            w.extend(row_cells(r).iter().map(|t| text_width(t, h).max(h) + h));
            w
        })
        .collect();
    (widths, row_h)
}

/// The control frame's lines and texts, its left middle at `o`.
pub fn frame_prims(f: &ControlFrame, o: [f64; 2], h: f64) -> Vec<Prim> {
    let (widths, row_h) = frame_layout(f, h);
    let n = f.rows.len().max(1) as f64;
    let top = o[1] + row_h * n * 0.5;
    let mut out = Vec::new();
    for (k, (r, w)) in f.rows.iter().zip(&widths).enumerate() {
        let y1 = top - row_h * k as f64;
        let y0 = y1 - row_h;
        let mut x = o[0];
        let total: f64 = w.iter().sum();
        out.push(line([x, y1], [x + total, y1]));
        out.push(line([x, y0], [x + total, y0]));
        out.push(line([x, y0], [x, y1]));
        let cells = row_cells(r);
        for (i, cw) in w.iter().enumerate() {
            let mid = [x + cw * 0.5, (y0 + y1) * 0.5];
            if i == 0 {
                out.extend(characteristic_prims(r.symbol, mid, row_h));
            } else if let Some(t) = cells.get(i - 1) {
                out.push(text([mid[0], mid[1] - h * 0.5], h, t, Anchor::Center));
            }
            x += cw;
            out.push(line([x, y0], [x, y1]));
        }
    }
    if !f.note.is_empty() {
        out.push(text([o[0], top + h * 0.4], h, &f.note, Anchor::Left));
    }
    out
}

/// The frame's outline box (sheet mm): lower left, upper right.
pub fn frame_box(f: &ControlFrame, o: [f64; 2], h: f64) -> ([f64; 2], [f64; 2]) {
    let (widths, row_h) = frame_layout(f, h);
    let n = f.rows.len().max(1) as f64;
    let w = widths
        .iter()
        .map(|w| w.iter().sum::<f64>())
        .fold(row_h, f64::max);
    (
        [o[0], o[1] - row_h * n * 0.5],
        [o[0] + w, o[1] + row_h * n * 0.5],
    )
}

/// The datum identifier's box half size.
fn datum_half(letter: &str, h: f64) -> f64 {
    (text_width(letter, h) * 0.5).max(h * 0.5) + h * 0.5
}

/// Every line and text of a placed symbol; `origin` is the view's
/// position (or zero).
pub fn prims(m: &MarkSymbol, origin: [f64; 2], look: &AnnotationLook) -> Vec<Prim> {
    let h = look.leader_text.height;
    let at = add(origin, m.at);
    let tip = m.tip.map(|t| add(origin, t));
    let mut out = match &m.mark {
        Mark::SurfaceTexture(s) => surface_prims(s, at, h),
        Mark::ControlFrame(f) => frame_prims(f, at, h),
        Mark::Datum { letter } => {
            let s = datum_half(letter, h);
            let mut v = vec![
                line([at[0] - s, at[1] - s], [at[0] + s, at[1] - s]),
                line([at[0] + s, at[1] - s], [at[0] + s, at[1] + s]),
                line([at[0] + s, at[1] + s], [at[0] - s, at[1] + s]),
                line([at[0] - s, at[1] + s], [at[0] - s, at[1] - s]),
            ];
            v.push(text([at[0], at[1] - h * 0.5], h, letter, Anchor::Center));
            v
        }
    };
    let Some(tip) = tip else {
        return out;
    };
    // Where the leader leaves the symbol.
    let start = match &m.mark {
        Mark::SurfaceTexture(_) => at,
        Mark::ControlFrame(f) => {
            let (lo, hi) = frame_box(f, m.at, h);
            let (lo, hi) = (add(origin, lo), add(origin, hi));
            let y = (lo[1] + hi[1]) * 0.5;
            if tip[0] > hi[0] {
                [hi[0], y]
            } else {
                [lo[0], y]
            }
        }
        Mark::Datum { letter } => {
            let s = datum_half(letter, h);
            let d = [tip[0] - at[0], tip[1] - at[1]];
            if d[1].abs() >= d[0].abs() {
                [at[0], at[1] + s * d[1].signum()]
            } else {
                [at[0] + s * d[0].signum(), at[1]]
            }
        }
    };
    out.push(line(start, tip));
    let d = [tip[0] - start[0], tip[1] - start[1]];
    let len = d[0].hypot(d[1]);
    if len > 1e-9 {
        let dir = [d[0] / len, d[1] / len];
        match &m.mark {
            // ISO 5459: a filled triangle on the datum feature.
            Mark::Datum { .. } => {
                let s = h * 0.9;
                let n = [-dir[1] * s * 0.6, dir[0] * s * 0.6];
                let back = [tip[0] - dir[0] * s, tip[1] - dir[1] * s];
                out.push(Prim::Fill {
                    pts: vec![add(tip, n), [tip[0] - n[0], tip[1] - n[1]], back],
                });
            }
            _ => {
                let l = &look.leader;
                terminator(
                    &mut out,
                    tip,
                    dir,
                    l.terminator,
                    l.arrow_size,
                    l.arrow_height,
                );
            }
        }
    }
    out
}

/// A rough box round the symbol (sheet mm, `origin` added): picking.
pub fn bounds(m: &MarkSymbol, origin: [f64; 2], h: f64) -> ([f64; 2], [f64; 2]) {
    let at = add(origin, m.at);
    match &m.mark {
        Mark::SurfaceTexture(s) => {
            let w = [&s.a, &s.b, &s.c]
                .iter()
                .map(|t| text_width(t, h))
                .fold(h, f64::max);
            (
                [at[0] - h * 1.5, at[1] - h * 2.0],
                [at[0] + 2.0 * h + w, at[1] + 4.5 * h],
            )
        }
        Mark::ControlFrame(f) => {
            let (lo, hi) = frame_box(f, m.at, h);
            (add(origin, lo), add(origin, hi))
        }
        Mark::Datum { letter } => {
            let s = datum_half(letter, h);
            ([at[0] - s, at[1] - s], [at[0] + s, at[1] + s])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(p: &[Prim]) -> usize {
        p.iter().filter(|x| matches!(x, Prim::Line { .. })).count()
    }

    #[test]
    fn surface_symbol_types_and_fields() {
        let h = 3.5;
        let mut s = SurfaceTexture::default();
        assert_eq!(lines(&surface_prims(&s, [0.0, 0.0], h)), 2, "the bare V");
        s.kind = SurfaceType::Removal;
        assert_eq!(lines(&surface_prims(&s, [0.0, 0.0], h)), 3, "the bar");
        s.a = "Ra 3.2".into();
        let p = surface_prims(&s, [0.0, 0.0], h);
        assert_eq!(lines(&p), 4, "the horizontal line under the text");
        assert!(
            p.iter()
                .any(|x| matches!(x, Prim::Text { text, .. } if text == "Ra 3.2"))
        );
        // The long leg rises 3 h at 60°.
        let tops: Vec<f64> = p
            .iter()
            .filter_map(|x| match x {
                Prim::Line { b, .. } => Some(b[1]),
                _ => None,
            })
            .collect();
        assert!(tops.iter().any(|y| (y - 10.5).abs() < 1e-9));
        s.kind = SurfaceType::NoRemoval;
        assert!(lines(&surface_prims(&s, [0.0, 0.0], h)) > 20, "the circle");
    }

    #[test]
    fn control_frame_layout() {
        let f = ControlFrame {
            rows: vec![FrameRow {
                symbol: Characteristic::Position,
                tolerance: "Ø0.1".into(),
                datums: vec!["A".into(), "B".into(), String::new()],
            }],
            note: "4x".into(),
        };
        let h = 3.5;
        let (lo, hi) = frame_box(&f, [10.0, 20.0], h);
        assert!((hi[1] - lo[1] - 7.0).abs() < 1e-9, "one row 2 h high");
        assert!(
            hi[0] - lo[0] > 7.0 + 3.0 * h,
            "symbol + tolerance + two datums"
        );
        let p = frame_prims(&f, [10.0, 20.0], h);
        let texts: Vec<&str> = p
            .iter()
            .filter_map(|x| match x {
                Prim::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, ["Ø0.1", "A", "B", "4x"]);
        assert!(Characteristic::Flatness.is_form());
        for k in Characteristic::ALL {
            let g = characteristic_prims(k, [0.0, 0.0], 7.0);
            assert_eq!(g.is_empty(), k == Characteristic::None, "{k:?}");
        }
    }

    #[test]
    fn symbols_round_trip_and_follow_the_view() {
        let m = MarkSymbol {
            view: Some(3),
            at: [5.0, 5.0],
            tip: Some([0.0, 0.0]),
            mark: Mark::Datum { letter: "A".into() },
        };
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("\"symbol\":\"Datum\""));
        let back: MarkSymbol = serde_json::from_str(&json).unwrap();
        assert_eq!(back, m);
        let look = super::super::style::DrawingStyles::default().look();
        let a = prims(&m, [0.0, 0.0], &look);
        let b = prims(&m, [100.0, 0.0], &look);
        assert_eq!(a.len(), b.len());
        assert!(
            b.iter().any(|x| matches!(x, Prim::Fill { .. })),
            "the datum triangle"
        );
    }
}
