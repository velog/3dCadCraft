//! Formatted note text (`docs/spec/4.9-format-text.md`, stage D-S2): the note
//! keeps its plain text (so older builds still show it) and adds style spans
//! over character ranges (size, colour, bold, italic, underline,
//! strikethrough) and settings of the whole note (justification, line
//! spacing, width factor, rotation, border, background fill). Drawing
//! symbols are standard Unicode characters in the text; the sheet and the PDF
//! draw them as our own vector glyphs (`glyph`), so they print the same
//! everywhere without a symbol font.

use serde::{Deserialize, Serialize};

use super::annot::{line, text_width};
use super::sheet::Prim;

/// Formatting of a character range (unset fields keep the note's value).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RunStyle {
    /// Text height (mm).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<f64>,
    /// Colour (else the layer's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[u8; 3]>,
    /// Bold.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    /// Italic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    /// Underline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub underline: Option<bool>,
    /// Strikethrough.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strike: Option<bool>,
}

impl RunStyle {
    /// `self` with the fields `over` sets.
    fn merged(&self, over: &RunStyle) -> RunStyle {
        RunStyle {
            size: over.size.or(self.size),
            color: over.color.or(self.color),
            bold: over.bold.or(self.bold),
            italic: over.italic.or(self.italic),
            underline: over.underline.or(self.underline),
            strike: over.strike.or(self.strike),
        }
    }
}

/// A style over the characters `start..end` (char indices of the text).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Span {
    /// First character.
    pub start: usize,
    /// One past the last character.
    pub end: usize,
    /// What it sets.
    pub style: RunStyle,
}

/// Horizontal justification of the note's lines.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum HAlign {
    /// Lines start at the insertion point.
    #[default]
    Left,
    /// Lines are centred on it.
    Center,
    /// Lines end at it.
    Right,
}

/// Vertical justification of the note's block.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum VAlign {
    /// The first line's baseline is at the insertion point (plain notes).
    #[default]
    Top,
    /// The block is centred on it.
    Middle,
    /// The last line's baseline is at it.
    Bottom,
}

/// Line spacing.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum LineSpace {
    /// A factor of the text style's spacing (1 = single, 1.5, 2 = double).
    Multiple(f64),
    /// A fixed distance between baselines (mm).
    Exactly(f64),
}

impl Default for LineSpace {
    fn default() -> Self {
        LineSpace::Multiple(1.0)
    }
}

fn one() -> f64 {
    1.0
}

fn is_one(v: &f64) -> bool {
    (*v - 1.0).abs() < 1e-12
}

fn is_zero(v: &f64) -> bool {
    v.abs() < 1e-12
}

/// The formatting of a note.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RichText {
    /// Style spans, later ones win where they overlap.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spans: Vec<Span>,
    /// Horizontal justification.
    #[serde(default)]
    pub h: HAlign,
    /// Vertical justification.
    #[serde(default)]
    pub v: VAlign,
    /// Line spacing.
    #[serde(default)]
    pub line_space: LineSpace,
    /// Width factor of the characters (1 = 100 %).
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub stretch: f64,
    /// Rotation about the insertion point (degrees, counter-clockwise).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rotation: f64,
    /// A frame round the text.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub border: bool,
    /// A colour behind the text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<[u8; 3]>,
}

impl Default for RichText {
    fn default() -> Self {
        Self {
            spans: Vec::new(),
            h: HAlign::Left,
            v: VAlign::Top,
            line_space: LineSpace::default(),
            stretch: 1.0,
            rotation: 0.0,
            border: false,
            fill: None,
        }
    }
}

impl RichText {
    /// Sets `style`'s fields over `start..end`, merging with what is there.
    pub fn apply(&mut self, start: usize, end: usize, style: RunStyle) {
        if end > start {
            self.spans.push(Span { start, end, style });
        }
    }

    /// The style of every character of `text` (`base` under the spans).
    pub fn styles(&self, text: &str, base: &RunStyle) -> Vec<RunStyle> {
        let n = text.chars().count();
        let mut out = vec![base.clone(); n];
        for s in &self.spans {
            for st in out.iter_mut().take(s.end.min(n)).skip(s.start) {
                *st = st.merged(&s.style);
            }
        }
        out
    }

    /// The spans after the text changed from `old` to `new`: the edited part
    /// (between the common prefix and suffix) takes the style before it;
    /// ranges after it move with the text.
    pub fn follow_edit(&mut self, old: &str, new: &str) {
        let a: Vec<char> = old.chars().collect();
        let b: Vec<char> = new.chars().collect();
        let pre = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
        let max_suf = a.len().min(b.len()) - pre;
        let suf = a
            .iter()
            .rev()
            .zip(b.iter().rev())
            .take(max_suf)
            .take_while(|(x, y)| x == y)
            .count();
        let old_end = a.len() - suf;
        let new_end = b.len() - suf;
        let map = |i: usize, end: bool| -> usize {
            if i < pre || (i == pre && !end) {
                i
            } else if i >= old_end {
                i + new_end - old_end
            } else {
                // Inside the replaced part.
                if end { new_end } else { pre }
            }
        };
        for s in &mut self.spans {
            s.start = map(s.start, false);
            s.end = map(s.end, true);
        }
        self.spans.retain(|s| s.end > s.start);
    }
}

/// Symbol groups of the palette (Inventor's Format Text groups).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymbolGroup {
    /// Diameter, degree, plus-minus.
    MostCommon,
    /// Counterbore, countersink, depth, square, arc, slope, taper.
    Dimensioning,
    /// The 14 geometric characteristics of GD&T.
    Geometric,
    /// Material condition and other modifiers.
    Modifier,
}

impl SymbolGroup {
    /// Every group in the palette's order.
    pub const ALL: [SymbolGroup; 4] = [
        SymbolGroup::MostCommon,
        SymbolGroup::Dimensioning,
        SymbolGroup::Geometric,
        SymbolGroup::Modifier,
    ];

    /// Display name.
    pub fn label(self) -> &'static str {
        match self {
            SymbolGroup::MostCommon => "Most Common",
            SymbolGroup::Dimensioning => "Dimensioning",
            SymbolGroup::Geometric => "Geometric Characteristic",
            SymbolGroup::Modifier => "Modifiers",
        }
    }
}

/// The palette: character, name, group. The characters are standard Unicode
/// (ISO / ASME symbol code points); `glyph` draws the ones fonts lack.
pub const SYMBOLS: [(char, &str, SymbolGroup); 34] = [
    ('Ø', "Diameter", SymbolGroup::MostCommon),
    ('°', "Degree", SymbolGroup::MostCommon),
    ('±', "Plus-Minus", SymbolGroup::MostCommon),
    ('⌴', "Counterbore / Spotface", SymbolGroup::Dimensioning),
    ('⌵', "Countersink", SymbolGroup::Dimensioning),
    ('↧', "Depth", SymbolGroup::Dimensioning),
    ('□', "Square", SymbolGroup::Dimensioning),
    ('⌒', "Arc Length", SymbolGroup::Dimensioning),
    ('⌳', "Slope", SymbolGroup::Dimensioning),
    ('⌲', "Conical Taper", SymbolGroup::Dimensioning),
    ('↔', "Between", SymbolGroup::Dimensioning),
    ('×', "By (times)", SymbolGroup::Dimensioning),
    ('⏤', "Straightness", SymbolGroup::Geometric),
    ('⏥', "Flatness", SymbolGroup::Geometric),
    ('○', "Circularity", SymbolGroup::Geometric),
    ('⌭', "Cylindricity", SymbolGroup::Geometric),
    ('⌓', "Profile of a Surface", SymbolGroup::Geometric),
    ('⌔', "Profile of a Line", SymbolGroup::Geometric),
    ('∠', "Angularity", SymbolGroup::Geometric),
    ('⊥', "Perpendicularity", SymbolGroup::Geometric),
    ('∥', "Parallelism", SymbolGroup::Geometric),
    ('⌖', "Position", SymbolGroup::Geometric),
    ('◎', "Concentricity", SymbolGroup::Geometric),
    ('⌯', "Symmetry", SymbolGroup::Geometric),
    ('↗', "Circular Runout", SymbolGroup::Geometric),
    ('⌰', "Total Runout", SymbolGroup::Geometric),
    ('Ⓜ', "Maximum Material Condition", SymbolGroup::Modifier),
    ('Ⓛ', "Least Material Condition", SymbolGroup::Modifier),
    ('Ⓟ', "Projected Tolerance Zone", SymbolGroup::Modifier),
    ('Ⓕ', "Free State", SymbolGroup::Modifier),
    ('Ⓣ', "Tangent Plane", SymbolGroup::Modifier),
    ('Ⓢ', "Regardless of Feature Size", SymbolGroup::Modifier),
    ('Ⓤ', "Unequally Disposed Profile", SymbolGroup::Modifier),
    ('Ⓒ', "Continuous Feature", SymbolGroup::Modifier),
];

/// The symbol's name, when `c` is one of the palette's drawn symbols.
pub fn symbol_name(c: char) -> Option<&'static str> {
    SYMBOLS.iter().find(|(s, ..)| *s == c).map(|(_, n, _)| *n)
}

/// Whether `text` holds a symbol drawn as a glyph (it then needs the
/// formatted layout even without formatting).
pub fn has_glyphs(text: &str) -> bool {
    text.chars()
        .any(|c| !in_fonts(c) && symbol_name(c).is_some())
}

/// Characters every font has (drawn as text, not as a glyph).
fn in_fonts(c: char) -> bool {
    matches!(c, 'Ø' | '°' | '±' | '×')
}

/// The vector glyph of symbol `c` with its baseline left corner at `o`,
/// cap height `h`, width factor `k`: its lines and its advance (mm).
pub fn glyph(c: char, o: [f64; 2], h: f64, k: f64) -> Option<(Vec<Prim>, f64)> {
    if in_fonts(c) || symbol_name(c).is_none() {
        return None;
    }
    let w = h * k;
    let p = |x: f64, y: f64| [o[0] + x * w, o[1] + y * h];
    let poly = |pts: &[(f64, f64)]| -> Vec<Prim> {
        pts.windows(2)
            .map(|s| line(p(s[0].0, s[0].1), p(s[1].0, s[1].1)))
            .collect()
    };
    let arc = |cx: f64, cy: f64, r: f64, a0: f64, a1: f64| -> Vec<Prim> {
        let n = 16;
        let pts: Vec<(f64, f64)> = (0..=n)
            .map(|i| {
                let a = a0 + (a1 - a0) * i as f64 / n as f64;
                // Round in sheet mm: r is a fraction of the height.
                (cx + r * h / w * a.cos(), cy + r * a.sin())
            })
            .collect();
        poly(&pts)
    };
    let tau = std::f64::consts::TAU;
    let pi = std::f64::consts::PI;
    let mut out: Vec<Prim> = match c {
        '⌴' => poly(&[(0.1, 0.9), (0.1, 0.1), (0.9, 0.1), (0.9, 0.9)]),
        '⌵' => poly(&[(0.05, 0.9), (0.5, 0.1), (0.95, 0.9)]),
        '↧' => {
            let mut v = poly(&[(0.5, 1.0), (0.5, 0.05)]);
            v.extend(poly(&[(0.15, 0.05), (0.85, 0.05)]));
            v.extend(poly(&[(0.25, 0.35), (0.5, 0.05), (0.75, 0.35)]));
            v
        }
        '□' => poly(&[(0.1, 0.1), (0.9, 0.1), (0.9, 0.9), (0.1, 0.9), (0.1, 0.1)]),
        '⌒' => arc(0.5, 0.25, 0.45, 0.0, pi),
        '⌳' => poly(&[(0.05, 0.1), (0.95, 0.1), (0.05, 0.75), (0.05, 0.1)]),
        '⌲' => poly(&[(0.05, 0.85), (0.95, 0.5), (0.05, 0.15), (0.05, 0.85)]),
        '↔' => {
            let mut v = poly(&[(0.05, 0.5), (0.95, 0.5)]);
            v.extend(poly(&[(0.25, 0.7), (0.05, 0.5), (0.25, 0.3)]));
            v.extend(poly(&[(0.75, 0.7), (0.95, 0.5), (0.75, 0.3)]));
            v
        }
        '⏤' => poly(&[(0.05, 0.5), (0.95, 0.5)]),
        '⏥' => poly(&[
            (0.25, 0.75),
            (0.95, 0.75),
            (0.75, 0.25),
            (0.05, 0.25),
            (0.25, 0.75),
        ]),
        '○' => arc(0.5, 0.5, 0.42, 0.0, tau),
        '⌭' => {
            let mut v = arc(0.5, 0.5, 0.32, 0.0, tau);
            v.extend(poly(&[(0.05, 0.1), (0.35, 0.95)]));
            v.extend(poly(&[(0.65, 0.05), (0.95, 0.9)]));
            v
        }
        '⌓' => {
            let mut v = arc(0.5, 0.25, 0.45, 0.0, pi);
            v.extend(poly(&[(0.05, 0.25), (0.95, 0.25)]));
            v
        }
        '⌔' => arc(0.5, 0.25, 0.45, 0.0, pi),
        '∠' => poly(&[(0.9, 0.85), (0.1, 0.15), (0.95, 0.15)]),
        '⊥' => {
            let mut v = poly(&[(0.05, 0.1), (0.95, 0.1)]);
            v.extend(poly(&[(0.5, 0.1), (0.5, 0.95)]));
            v
        }
        '∥' => {
            let mut v = poly(&[(0.15, 0.1), (0.45, 0.9)]);
            v.extend(poly(&[(0.55, 0.1), (0.85, 0.9)]));
            v
        }
        '⌖' => {
            let mut v = arc(0.5, 0.5, 0.3, 0.0, tau);
            v.extend(poly(&[(0.5, 0.0), (0.5, 1.0)]));
            v.extend(poly(&[(0.05, 0.5), (0.95, 0.5)]));
            v
        }
        '◎' => {
            let mut v = arc(0.5, 0.5, 0.42, 0.0, tau);
            v.extend(arc(0.5, 0.5, 0.22, 0.0, tau));
            v
        }
        '⌯' => {
            let mut v = poly(&[(0.05, 0.5), (0.95, 0.5)]);
            v.extend(poly(&[(0.2, 0.75), (0.8, 0.75)]));
            v.extend(poly(&[(0.2, 0.25), (0.8, 0.25)]));
            v
        }
        '↗' => {
            let mut v = poly(&[(0.1, 0.1), (0.9, 0.9)]);
            v.push(Prim::Fill {
                pts: vec![p(0.9, 0.9), p(0.55, 0.8), p(0.8, 0.55)],
            });
            v
        }
        '⌰' => {
            let mut v = poly(&[(0.05, 0.1), (0.5, 0.9)]);
            v.extend(poly(&[(0.45, 0.1), (0.9, 0.9)]));
            v.extend(poly(&[(0.05, 0.1), (0.75, 0.1)]));
            v.push(Prim::Fill {
                pts: vec![p(0.5, 0.9), p(0.3, 0.7), p(0.5, 0.65)],
            });
            v.push(Prim::Fill {
                pts: vec![p(0.9, 0.9), p(0.7, 0.7), p(0.9, 0.65)],
            });
            v
        }
        'Ⓜ' | 'Ⓛ' | 'Ⓟ' | 'Ⓕ' | 'Ⓣ' | 'Ⓢ' | 'Ⓤ' | 'Ⓒ' => {
            let letter = match c {
                'Ⓜ' => "M",
                'Ⓛ' => "L",
                'Ⓟ' => "P",
                'Ⓕ' => "F",
                'Ⓣ' => "T",
                'Ⓢ' => "S",
                'Ⓤ' => "U",
                _ => "C",
            };
            let mut v = arc(0.5, 0.45, 0.55, 0.0, tau);
            v.push(Prim::RunText {
                at: p(0.5, 0.15),
                height: h * 0.6,
                text: letter.into(),
                angle: 0.0,
                bold: false,
                italic: false,
                stretch: k,
                center: true,
            });
            v
        }
        _ => return None,
    };
    if out.is_empty() {
        out.push(line(o, o));
    }
    Some((out, w * 1.15))
}

/// Font size (em) per cap height of formatted runs, on the screen and in
/// the PDF.
pub const EM_PER_CAP: f64 = 1.35;

/// Measures a text run's advance (mm) for a cap height, from the font the
/// sheet is drawn with (`None`: the estimate).
pub type Measure = dyn Fn(&str, f64) -> Option<f64> + Send + Sync;

static MEASURE: std::sync::OnceLock<Box<Measure>> = std::sync::OnceLock::new();

/// Sets how runs are measured (the GUI passes its drawing font, so runs of
/// different styles line up as drawn); without it the 0.6 × height
/// estimate of `text_width` is used.
pub fn set_measure(m: Box<Measure>) {
    let _ = MEASURE.set(m);
}

/// Advance of a text run (mm).
fn run_width(s: &str, size: f64, stretch: f64) -> f64 {
    MEASURE
        .get()
        .and_then(|m| m(s, size))
        .unwrap_or_else(|| text_width(s, size))
        * stretch
}

/// One piece of a line: text in one style, or a symbol.
enum Piece {
    Text(String, RunStyle),
    Sym(char, RunStyle),
}

/// The note's lines and texts: `text` formatted by `rich` at insertion point
/// `at`, base height `height`, the text style's line spacing `spacing`.
pub fn layout(text: &str, rich: &RichText, at: [f64; 2], height: f64, spacing: f64) -> Vec<Prim> {
    let base = RunStyle::default();
    let styles = rich.styles(text, &base);
    let k = rich.stretch.clamp(0.1, 10.0);
    // Split into lines of pieces.
    let mut lines: Vec<Vec<Piece>> = vec![Vec::new()];
    for (ch, st) in text.chars().zip(styles) {
        if ch == '\n' {
            lines.push(Vec::new());
            continue;
        }
        let Some(cur) = lines.last_mut() else {
            continue;
        };
        if symbol_name(ch).is_some() && !in_fonts(ch) {
            cur.push(Piece::Sym(ch, st));
            continue;
        }
        match cur.last_mut() {
            Some(Piece::Text(s, s_st)) if *s_st == st => s.push(ch),
            _ => cur.push(Piece::Text(ch.to_string(), st)),
        }
    }
    let size_of = |st: &RunStyle| st.size.unwrap_or(height).max(0.1);
    let width_of = |p: &Piece| match p {
        Piece::Text(s, st) => run_width(s, size_of(st), k),
        Piece::Sym(_, st) => size_of(st) * k * 1.15,
    };
    let line_h = |l: &[Piece]| {
        let big = l
            .iter()
            .map(|p| match p {
                Piece::Text(_, st) | Piece::Sym(_, st) => size_of(st),
            })
            .fold(height, f64::max);
        match rich.line_space {
            LineSpace::Multiple(f) => big * spacing * f.max(0.1),
            LineSpace::Exactly(d) => d.max(0.1),
        }
    };
    // Baselines relative to the first one.
    let mut base_y = vec![0.0];
    for l in &lines[1..] {
        let y = base_y.last().copied().unwrap_or(0.0) - line_h(l);
        base_y.push(y);
    }
    let last = base_y.last().copied().unwrap_or(0.0);
    let top = height;
    let shift_y = match rich.v {
        VAlign::Top => 0.0,
        VAlign::Middle => -(top + last) * 0.5,
        VAlign::Bottom => -last,
    };
    let mut body = Vec::new();
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for (l, by) in lines.iter().zip(&base_y) {
        let w: f64 = l.iter().map(width_of).sum();
        let mut x = at[0]
            + match rich.h {
                HAlign::Left => 0.0,
                HAlign::Center => -w * 0.5,
                HAlign::Right => -w,
            };
        let y = at[1] + by + shift_y;
        lo = [lo[0].min(x), lo[1].min(y - height * 0.3)];
        hi = [hi[0].max(x + w), hi[1].max(y + height)];
        for p in l {
            let pw = width_of(p);
            let st = match p {
                Piece::Text(_, st) | Piece::Sym(_, st) => st,
            };
            let size = size_of(st);
            if let Some(c) = st.color {
                body.push(Prim::Ink { color: Some(c) });
            }
            match p {
                Piece::Text(s, _) => body.push(Prim::RunText {
                    at: [x, y],
                    height: size,
                    text: s.clone(),
                    angle: 0.0,
                    bold: st.bold.unwrap_or(false),
                    italic: st.italic.unwrap_or(false),
                    stretch: k,
                    center: false,
                }),
                Piece::Sym(c, _) => {
                    if let Some((g, _)) = glyph(*c, [x, y], size, k) {
                        body.extend(g);
                    }
                }
            }
            if st.underline.unwrap_or(false) {
                body.push(line([x, y - size * 0.18], [x + pw, y - size * 0.18]));
            }
            if st.strike.unwrap_or(false) {
                body.push(line([x, y + size * 0.4], [x + pw, y + size * 0.4]));
            }
            if st.color.is_some() {
                body.push(Prim::Ink { color: None });
            }
            x += pw;
        }
    }
    let mut out = Vec::new();
    if lo[0] <= hi[0] {
        let m = height * 0.3;
        let (a, b) = ([lo[0] - m, lo[1] - m], [hi[0] + m, hi[1] + m]);
        let corners = vec![a, [b[0], a[1]], b, [a[0], b[1]]];
        if let Some(c) = rich.fill {
            out.push(Prim::Ink { color: Some(c) });
            out.push(Prim::Fill {
                pts: corners.clone(),
            });
            out.push(Prim::Ink { color: None });
        }
        out.extend(body);
        if rich.border {
            for i in 0..4 {
                out.push(line(corners[i], corners[(i + 1) % 4]));
            }
        }
    } else {
        out.extend(body);
    }
    if !is_zero(&rich.rotation) {
        let a = rich.rotation.to_radians();
        for p in &mut out {
            rotate(p, at, a);
        }
    }
    out
}

/// Turns `p` by `a` (radians) about `c`.
pub fn rotate(p: &mut Prim, c: [f64; 2], a: f64) {
    let (s, co) = a.sin_cos();
    let r = |q: &mut [f64; 2]| {
        let d = [q[0] - c[0], q[1] - c[1]];
        *q = [c[0] + d[0] * co - d[1] * s, c[1] + d[0] * s + d[1] * co];
    };
    match p {
        Prim::Line { a: p0, b: p1, .. } | Prim::Center { a: p0, b: p1, .. } => {
            r(p0);
            r(p1);
        }
        Prim::Path { pts, .. } | Prim::Fill { pts } => pts.iter_mut().for_each(r),
        Prim::Text { at, angle, .. } | Prim::RunText { at, angle, .. } => {
            r(at);
            *angle += a;
        }
        Prim::Pen { .. } | Prim::Ink { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(p: &[Prim]) -> Vec<(String, bool, bool, [f64; 2])> {
        p.iter()
            .filter_map(|q| match q {
                Prim::RunText {
                    text,
                    bold,
                    italic,
                    at,
                    ..
                } => Some((text.clone(), *bold, *italic, *at)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn spans_split_a_line_into_runs() {
        let mut r = RichText::default();
        r.apply(
            0,
            4,
            RunStyle {
                bold: Some(true),
                ..Default::default()
            },
        );
        r.apply(
            2,
            7,
            RunStyle {
                italic: Some(true),
                ..Default::default()
            },
        );
        let p = layout("NOTE 12", &r, [10.0, 20.0], 3.5, 1.6);
        let t = texts(&p);
        let parts: Vec<(&str, bool, bool)> =
            t.iter().map(|(s, b, i, _)| (s.as_str(), *b, *i)).collect();
        assert_eq!(
            parts,
            [
                ("NO", true, false),
                ("TE", true, true),
                (" 12", false, true)
            ]
        );
        // Runs follow each other on the baseline.
        assert!((t[0].3[1] - 20.0).abs() < 1e-9);
        assert!(t[1].3[0] > t[0].3[0]);
    }

    #[test]
    fn the_default_is_unformatted() {
        let r = RichText::default();
        assert_eq!(r.stretch, 1.0);
        let back: RichText = serde_json::from_str("{}").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(back, r);
    }

    #[test]
    fn plain_layout_matches_old_notes() {
        let p = layout("A\nB", &RichText::default(), [10.0, 20.0], 3.5, 1.6);
        let t = texts(&p);
        assert_eq!(t[0].3, [10.0, 20.0]);
        assert!((t[1].3[1] - (20.0 - 3.5 * 1.6)).abs() < 1e-9);
    }

    #[test]
    fn justification_border_fill_rotation() {
        let r = RichText {
            h: HAlign::Right,
            v: VAlign::Bottom,
            border: true,
            fill: Some([255, 255, 200]),
            rotation: 90.0,
            ..Default::default()
        };
        let p = layout("AB\nC", &r, [0.0, 0.0], 3.5, 1.6);
        assert!(matches!(p.first(), Some(Prim::Ink { color: Some(_) })));
        assert!(p.iter().any(|q| matches!(q, Prim::Fill { .. })));
        let lines = p.iter().filter(|q| matches!(q, Prim::Line { .. })).count();
        assert_eq!(lines, 4, "the border");
        // Rotated by 90°: texts lie left of the insertion point's vertical.
        for (_, _, _, at) in texts(&p) {
            assert!(at[0] <= 1e-9, "{at:?}");
        }
        let unrot = layout(
            "AB\nC",
            &RichText {
                h: HAlign::Right,
                v: VAlign::Bottom,
                ..Default::default()
            },
            [0.0, 0.0],
            3.5,
            1.6,
        );
        let t = texts(&unrot);
        // Right: the line ends at x = 0; Bottom: the last baseline at y = 0.
        let w = text_width("AB", 3.5);
        assert!((t[0].3[0] + w).abs() < 1e-9);
        assert!(t[1].3[1].abs() < 1e-9);
    }

    #[test]
    fn symbols_are_drawn_as_glyphs() {
        let p = layout("⌴Ø11 ↧6.4", &RichText::default(), [0.0, 0.0], 3.5, 1.6);
        let t: Vec<String> = texts(&p).into_iter().map(|x| x.0).collect();
        assert_eq!(t, ["Ø11 ", "6.4"], "Ø stays text, ⌴ and ↧ are glyphs");
        assert!(p.iter().filter(|q| matches!(q, Prim::Line { .. })).count() >= 3 + 4);
        for (c, ..) in SYMBOLS {
            if !in_fonts(c) {
                assert!(glyph(c, [0.0, 0.0], 3.5, 1.0).is_some(), "{c}");
            }
        }
    }

    #[test]
    fn spans_follow_text_edits() {
        let mut r = RichText::default();
        r.apply(
            6,
            11,
            RunStyle {
                bold: Some(true),
                ..Default::default()
            },
        );
        // "Hello world" → "Hi world": the bold word moves left by 3.
        r.follow_edit("Hello world", "Hi world");
        assert_eq!((r.spans[0].start, r.spans[0].end), (3, 8));
        // Typing inside the bold word widens it.
        r.follow_edit("Hi world", "Hi wor-ld");
        assert_eq!((r.spans[0].start, r.spans[0].end), (3, 9));
        // Deleting it removes the span.
        r.follow_edit("Hi wor-ld", "Hi ");
        assert!(r.spans.is_empty());
    }

    #[test]
    fn underline_strike_and_colour() {
        let mut r = RichText::default();
        r.apply(
            0,
            2,
            RunStyle {
                underline: Some(true),
                strike: Some(true),
                color: Some([200, 0, 0]),
                size: Some(7.0),
                ..Default::default()
            },
        );
        let p = layout("AB", &r, [0.0, 0.0], 3.5, 1.6);
        assert_eq!(
            p.iter()
                .filter(|q| matches!(
                    q,
                    Prim::Ink {
                        color: Some([200, 0, 0])
                    }
                ))
                .count(),
            1
        );
        assert!(matches!(p.last(), Some(Prim::Ink { color: None })));
        assert_eq!(
            p.iter().filter(|q| matches!(q, Prim::Line { .. })).count(),
            2
        );
        assert!(texts(&p).iter().all(|_| true));
        assert!(
            p.iter()
                .any(|q| matches!(q, Prim::RunText { height, .. } if *height == 7.0))
        );
    }
}
