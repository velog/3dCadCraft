//! Edit Dimension (`docs/spec/4.7-drawing-commands.md`): a drawing
//! dimension's own text, precision and tolerance, and how its label is laid
//! out — the value with the text around it, the tolerance after it (on one
//! line or stacked), the box of a basic dimension, the brackets of a
//! reference one and the outline of an inspection dimension.

use serde::{Deserialize, Serialize};

use super::sheet::{Anchor, Prim, Weight};
use super::style::DimensionStyle;

/// The placeholder for the measured value in a dimension's text.
pub const VALUE: &str = "<<>>";

/// Tolerance methods of Edit Dimension › Precision and Tolerance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TolMethod {
    /// The value only.
    #[default]
    Default,
    /// Theoretically exact: the value in a box.
    Basic,
    /// `80 ± 0,1`.
    Symmetric,
    /// The value with the upper and lower deviation stacked after it.
    Deviation,
    /// The upper limit above the lower one.
    LimitsStacked,
    /// `79,95 - 80,1`.
    LimitsLinear,
    /// `80 MAX`.
    Max,
    /// `80 MIN`.
    Min,
    /// The fit after the value; hole above shaft when both are set.
    FitsStacked,
    /// The fit after the value on one line (`H7/g6`).
    FitsLinear,
    /// The fit and its size limits in brackets.
    FitsSizeLimits,
    /// The fit and its deviations in brackets.
    FitsTolerances,
    /// Reference: the value in brackets.
    Reference,
}

impl TolMethod {
    /// All, in the dialog's order.
    pub const ALL: [TolMethod; 13] = [
        TolMethod::Default,
        TolMethod::Basic,
        TolMethod::Symmetric,
        TolMethod::Deviation,
        TolMethod::LimitsStacked,
        TolMethod::LimitsLinear,
        TolMethod::Max,
        TolMethod::Min,
        TolMethod::FitsStacked,
        TolMethod::FitsLinear,
        TolMethod::FitsSizeLimits,
        TolMethod::FitsTolerances,
        TolMethod::Reference,
    ];

    /// The dialog's name.
    pub fn label(self) -> &'static str {
        match self {
            TolMethod::Default => "Default",
            TolMethod::Basic => "Basic",
            TolMethod::Symmetric => "Symmetric",
            TolMethod::Deviation => "Deviation",
            TolMethod::LimitsStacked => "Limits - Stacked",
            TolMethod::LimitsLinear => "Limits - Linear",
            TolMethod::Max => "Max",
            TolMethod::Min => "Min",
            TolMethod::FitsStacked => "Limits / Fits - Stacked",
            TolMethod::FitsLinear => "Limits / Fits - Linear",
            TolMethod::FitsSizeLimits => "Limits / Fits - Show Size Limits",
            TolMethod::FitsTolerances => "Limits / Fits - Show Tolerances",
            TolMethod::Reference => "Reference",
        }
    }

    /// Takes Upper / Lower values.
    pub fn uses_values(self) -> bool {
        matches!(
            self,
            TolMethod::Symmetric
                | TolMethod::Deviation
                | TolMethod::LimitsStacked
                | TolMethod::LimitsLinear
        )
    }

    /// Takes a hole / shaft fit.
    pub fn uses_fits(self) -> bool {
        matches!(
            self,
            TolMethod::FitsStacked
                | TolMethod::FitsLinear
                | TolMethod::FitsSizeLimits
                | TolMethod::FitsTolerances
        )
    }
}

/// Outline of an inspection dimension.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum InspectionShape {
    /// Only the separators.
    None,
    /// Round ends.
    #[default]
    Round,
    /// Angled ends.
    Angled,
}

/// Edit Dimension's settings of one dimension (all default: the style's
/// text).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DimFormat {
    /// The text, `<<>>` standing for the value (empty: the value alone).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub text: String,
    /// Hide Dimension Value: `<<>>` draws nothing.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub hide_value: bool,
    /// Decimal places of the value (none: the style's).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub precision: Option<u8>,
    /// Decimal places of the tolerance (none: the value's).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tol_precision: Option<u8>,
    /// Tolerance method.
    #[serde(skip_serializing_if = "is_default")]
    pub tol: TolMethod,
    /// Upper deviation (mm).
    #[serde(skip_serializing_if = "is_zero")]
    pub upper: f64,
    /// Lower deviation (mm, signed: −0.05).
    #[serde(skip_serializing_if = "is_zero")]
    pub lower: f64,
    /// Hole fit (`H7`).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub hole: String,
    /// Shaft fit (`g6`).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub shaft: String,
    /// Inspection dimension outline, label and rate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inspection: Option<Inspection>,
}

/// Inspection Dimension tab.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Inspection {
    /// Outline.
    pub shape: InspectionShape,
    /// Text left of the value.
    pub label: String,
    /// Text right of the value (`100%`).
    pub rate: String,
}

fn is_default(m: &TolMethod) -> bool {
    *m == TolMethod::Default
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

impl DimFormat {
    /// Nothing set: the style alone draws the dimension.
    pub fn is_plain(&self) -> bool {
        *self == DimFormat::default()
    }

    /// The (upper, lower) deviations of the tolerance at `nominal` (mm),
    /// from the fields or the fit; `None` when it has no deviations.
    pub fn deviations(&self, nominal: f64) -> Option<(f64, f64)> {
        match self.tol {
            TolMethod::Symmetric => Some((self.upper.abs(), -self.upper.abs())),
            TolMethod::Deviation | TolMethod::LimitsStacked | TolMethod::LimitsLinear => {
                Some((self.upper, self.lower))
            }
            m if m.uses_fits() => {
                let code = if self.hole.is_empty() {
                    &self.shaft
                } else {
                    &self.hole
                };
                super::fits::deviations(code, nominal)
            }
            _ => None,
        }
    }
}

/// One text of a label, in the label's frame (x from its left end, y from
/// the value's baseline).
#[derive(Clone, Debug, PartialEq)]
struct Piece {
    x: f64,
    y: f64,
    height: f64,
    text: String,
}

/// A dimension's label laid out in its own frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Label {
    pieces: Vec<Piece>,
    /// Lines in the label's frame (box, fraction bar, outline).
    lines: Vec<[[f64; 2]; 2]>,
    /// Width (mm).
    pub width: f64,
}

/// Width of a text line (mm): the drawing fonts' digits are about 0.75 of
/// the cap height wide (screen and PDF), so a label's pieces never touch.
fn text_width(t: &str, height: f64) -> f64 {
    t.chars().count() as f64 * height * 0.75
}

/// A number with the style's decimal marker and trailing zeros.
fn number(v: f64, precision: u8, s: &DimensionStyle) -> String {
    let mut t = format!("{:.*}", usize::from(precision), v);
    if !s.trailing_zeros && t.contains('.') {
        t = t.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    if t.starts_with('-') && t.trim_start_matches(['-', '0', '.']).is_empty() {
        t.remove(0);
    }
    if s.decimal_comma {
        t = t.replace('.', ",");
    }
    t
}

impl Label {
    fn push(&mut self, x: f64, y: f64, height: f64, text: String, width_scale: f64) -> f64 {
        let w = text_width(&text, height) * width_scale;
        if !text.is_empty() {
            self.pieces.push(Piece { x, y, height, text });
        }
        x + w
    }

    /// Lays out the label of a dimension measuring `value` (mm) whose
    /// value is written `symbol` + number (Ø, R).
    pub fn build(
        value: f64,
        symbol: &str,
        f: &DimFormat,
        s: &DimensionStyle,
        h: f64,
        width_scale: f64,
    ) -> Label {
        let mut l = Label::default();
        let p = f.precision.unwrap_or(s.precision);
        let tp = f.tol_precision.unwrap_or(p);
        let num = |v: f64| number(v, p, s);
        let tol = |v: f64| number(v, tp, s);
        let signed = |v: f64, lower: bool| {
            let t = tol(v.abs());
            let neg = if lower { v <= 0.0 } else { v < 0.0 };
            format!("{}{t}", if neg { "-" } else { "+" })
        };
        let value_text = format!("{}{symbol}{}{}", s.prefix, num(value), s.suffix);
        let text = if f.text.is_empty() {
            VALUE
        } else {
            f.text.as_str()
        };
        let (before, after) = text.split_once(VALUE).unwrap_or((text, ""));
        let gap = h * 0.3;
        let stack = |l: &mut Label, x: f64, top: &str, bottom: &str| -> f64 {
            let a = l.push(x, h * 0.625, h, top.to_string(), width_scale);
            let b = l.push(x, -h * 0.625, h, bottom.to_string(), width_scale);
            a.max(b)
        };
        let mut x = 0.0;
        if let Some(i) = &f.inspection {
            // Clear of the outline's round / angled end (radius 1.1 h).
            x = l.push(x + h * 1.1, 0.0, h, i.label.clone(), width_scale);
            if !i.label.is_empty() {
                x += h * 0.6;
            }
        }
        let start = x;
        x = l.push(x, 0.0, h, before.to_string(), width_scale);
        let shown = !f.hide_value && text.contains(VALUE);
        if shown {
            let dev = f.deviations(value);
            let fit = match (f.hole.is_empty(), f.shaft.is_empty()) {
                (false, false) => format!("{}/{}", f.hole, f.shaft),
                (false, true) => f.hole.clone(),
                (true, false) => f.shaft.clone(),
                (true, true) => String::new(),
            };
            match f.tol {
                TolMethod::Default | TolMethod::Basic => {
                    let x0 = x;
                    x = l.push(x, 0.0, h, value_text, width_scale);
                    if f.tol == TolMethod::Basic {
                        let (a, b) = ([x0 - 0.6, -0.8], [x + 0.6, h + 0.8]);
                        l.rect(a, b);
                        x += 0.6;
                    }
                }
                TolMethod::Reference => {
                    x = l.push(x, 0.0, h, format!("({value_text})"), width_scale);
                }
                TolMethod::Symmetric => {
                    let (u, _) = dev.unwrap_or_default();
                    x = l.push(x, 0.0, h, format!("{value_text} ± {}", tol(u)), width_scale);
                }
                TolMethod::Deviation => {
                    let (u, lo) = dev.unwrap_or_default();
                    x = l.push(x, 0.0, h, value_text, width_scale) + gap;
                    x = stack(&mut l, x, &signed(u, false), &signed(lo, true));
                }
                TolMethod::LimitsStacked => {
                    let (u, lo) = dev.unwrap_or_default();
                    let (a, b) = (num(value + u), num(value + lo));
                    x = stack(&mut l, x, &a, &b);
                }
                TolMethod::LimitsLinear => {
                    let (u, lo) = dev.unwrap_or_default();
                    x = l.push(
                        x,
                        0.0,
                        h,
                        format!("{} - {}", num(value + lo), num(value + u)),
                        width_scale,
                    );
                }
                TolMethod::Max | TolMethod::Min => {
                    let word = if f.tol == TolMethod::Max {
                        "MAX"
                    } else {
                        "MIN"
                    };
                    x = l.push(x, 0.0, h, format!("{value_text} {word}"), width_scale);
                }
                TolMethod::FitsStacked if !f.hole.is_empty() && !f.shaft.is_empty() => {
                    x = l.push(x, 0.0, h, value_text, width_scale) + gap;
                    let x0 = x;
                    x = stack(&mut l, x, &f.hole, &f.shaft);
                    l.lines.push([[x0, h * 0.5], [x, h * 0.5]]);
                }
                TolMethod::FitsStacked | TolMethod::FitsLinear => {
                    let t = if fit.is_empty() {
                        value_text
                    } else {
                        format!("{value_text} {fit}")
                    };
                    x = l.push(x, 0.0, h, t, width_scale);
                }
                TolMethod::FitsSizeLimits | TolMethod::FitsTolerances => {
                    let code = if f.hole.is_empty() { &f.shaft } else { &f.hole };
                    let t = if code.is_empty() {
                        value_text
                    } else {
                        format!("{value_text} {code}")
                    };
                    x = l.push(x, 0.0, h, t, width_scale);
                    if let Some((u, lo)) = dev {
                        let (a, b) = if f.tol == TolMethod::FitsSizeLimits {
                            (num(value + u), num(value + lo))
                        } else {
                            (signed(u, false), signed(lo, true))
                        };
                        x += gap;
                        x = l.bracket(x, h, true) + gap * 0.5;
                        x = stack(&mut l, x, &a, &b) + gap * 0.5;
                        x = l.bracket(x, h, false);
                    }
                }
            }
        }
        x = l.push(x, 0.0, h, after.to_string(), width_scale);
        if let Some(i) = &f.inspection {
            // Separators between label, value and rate, then the outline.
            let (lo, hi) = (-h * 0.6, h * 1.6);
            let end_l = start - h * 0.3;
            if !i.label.is_empty() {
                l.lines.push([[end_l, lo], [end_l, hi]]);
            }
            let mut right = x + h * 0.3;
            if !i.rate.is_empty() {
                l.lines.push([[right, lo], [right, hi]]);
                right = l.push(right + h * 0.4, 0.0, h, i.rate.clone(), width_scale);
            }
            let r = (hi - lo) * 0.5;
            let mid = (hi + lo) * 0.5;
            let (a, b) = (0.0, right + h * 1.1);
            match i.shape {
                InspectionShape::None => {}
                InspectionShape::Angled => {
                    let pts = [
                        [a, mid],
                        [a + r, hi],
                        [b - r, hi],
                        [b, mid],
                        [b - r, lo],
                        [a + r, lo],
                        [a, mid],
                    ];
                    l.lines.extend(pts.windows(2).map(|w| [w[0], w[1]]));
                }
                InspectionShape::Round => {
                    l.lines.push([[a + r, hi], [b - r, hi]]);
                    l.lines.push([[a + r, lo], [b - r, lo]]);
                    for (c, from) in [(a + r, 0.5), (b - r, -0.5)] {
                        let pts: Vec<[f64; 2]> = (0..=12)
                            .map(|k| {
                                let t = std::f64::consts::PI * (from + k as f64 / 12.0);
                                [c + r * t.cos(), mid + r * t.sin()]
                            })
                            .collect();
                        l.lines.extend(pts.windows(2).map(|w| [w[0], w[1]]));
                    }
                }
            }
            x = b;
        }
        l.width = x;
        // A stack, a box or an outline reaching below the value: lift
        // everything so the label stays above the dimension line.
        let low = l
            .pieces
            .iter()
            .map(|p| p.y)
            .chain(l.lines.iter().flat_map(|s| [s[0][1], s[1][1]]))
            .fold(0.0, f64::min);
        if low < 0.0 {
            for p in &mut l.pieces {
                p.y -= low;
            }
            for s in &mut l.lines {
                s[0][1] -= low;
                s[1][1] -= low;
            }
        }
        l
    }

    fn rect(&mut self, a: [f64; 2], b: [f64; 2]) {
        let c = [a, [b[0], a[1]], b, [a[0], b[1]], a];
        self.lines.extend(c.windows(2).map(|w| [w[0], w[1]]));
    }

    /// A tall bracket round a stacked pair, starting at `x`; its right end.
    fn bracket(&mut self, x: f64, h: f64, open: bool) -> f64 {
        let (lo, hi) = (-h * 0.6, h * 1.6);
        let w = h * 0.3;
        let mid = (lo + hi) * 0.5;
        let r = (hi - lo) * 0.5;
        let pts: Vec<[f64; 2]> = (0..=10)
            .map(|k| {
                let t = -std::f64::consts::FRAC_PI_2 + std::f64::consts::PI * k as f64 / 10.0;
                let dx = w * t.cos();
                [if open { x + w - dx } else { x + dx }, mid + r * t.sin()]
            })
            .collect();
        self.lines.extend(pts.windows(2).map(|w| [w[0], w[1]]));
        x + w
    }

    /// The label on the sheet: its middle at `at` on the value's baseline,
    /// turned by `angle`.
    pub fn prims(&self, at: [f64; 2], angle: f64) -> Vec<Prim> {
        let (s, c) = angle.sin_cos();
        let half = self.width * 0.5;
        let place = |q: [f64; 2]| {
            let (x, y) = (q[0] - half, q[1]);
            [at[0] + x * c - y * s, at[1] + x * s + y * c]
        };
        let mut out: Vec<Prim> = self
            .pieces
            .iter()
            .map(|p| Prim::Text {
                at: place([p.x, p.y]),
                height: p.height,
                text: p.text.clone(),
                anchor: Anchor::Left,
                angle,
            })
            .collect();
        out.extend(self.lines.iter().map(|[a, b]| Prim::Line {
            a: place(*a),
            b: place(*b),
            weight: Weight::Thin,
        }));
        out
    }

    /// The texts in reading order (tests, search).
    pub fn texts(&self) -> Vec<&str> {
        self.pieces.iter().map(|p| p.text.as_str()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style() -> DimensionStyle {
        let mut s = super::super::style::AnnotationLook::default().dim;
        s.precision = 2;
        s.trailing_zeros = true;
        s.decimal_comma = true;
        s.prefix.clear();
        s.suffix.clear();
        s
    }

    fn texts(f: &DimFormat) -> Vec<String> {
        Label::build(80.0, "", f, &style(), 3.5, 1.0)
            .texts()
            .iter()
            .map(|t| t.to_string())
            .collect()
    }

    fn with(tol: TolMethod) -> DimFormat {
        DimFormat {
            tol,
            upper: 0.1,
            lower: -0.05,
            ..Default::default()
        }
    }

    #[test]
    fn texts_of_every_method() {
        // Inventor's texts for an 80 mm dimension, ISO, two decimals.
        assert_eq!(texts(&DimFormat::default()), ["80,00"]);
        assert_eq!(texts(&with(TolMethod::Symmetric)), ["80,00 ± 0,10"]);
        assert_eq!(
            texts(&with(TolMethod::Deviation)),
            ["80,00", "+0,10", "-0,05"]
        );
        assert_eq!(texts(&with(TolMethod::LimitsStacked)), ["80,10", "79,95"]);
        assert_eq!(texts(&with(TolMethod::LimitsLinear)), ["79,95 - 80,10"]);
        assert_eq!(texts(&with(TolMethod::Max)), ["80,00 MAX"]);
        assert_eq!(texts(&with(TolMethod::Min)), ["80,00 MIN"]);
        assert_eq!(texts(&with(TolMethod::Basic)), ["80,00"]);
        assert_eq!(texts(&with(TolMethod::Reference)), ["(80,00)"]);
        let fit = |tol, hole: &str, shaft: &str| DimFormat {
            tol,
            hole: hole.into(),
            shaft: shaft.into(),
            ..Default::default()
        };
        assert_eq!(texts(&fit(TolMethod::FitsStacked, "H7", "")), ["80,00 H7"]);
        assert_eq!(
            texts(&fit(TolMethod::FitsStacked, "H7", "g6")),
            ["80,00", "H7", "g6"]
        );
        assert_eq!(
            texts(&fit(TolMethod::FitsLinear, "H7", "g6")),
            ["80,00 H7/g6"]
        );
        assert_eq!(
            texts(&fit(TolMethod::FitsSizeLimits, "H7", "g6")),
            ["80,00 H7", "80,03", "80,00"]
        );
        assert_eq!(
            texts(&fit(TolMethod::FitsTolerances, "H7", "")),
            ["80,00 H7", "+0,03", "-0,00"]
        );
    }

    #[test]
    fn text_precision_and_hidden_value() {
        let f = DimFormat {
            text: "4x <<>> THRU".into(),
            precision: Some(1),
            ..Default::default()
        };
        assert_eq!(texts(&f), ["4x ", "80,0", " THRU"]);
        let f = DimFormat {
            text: "SEE NOTE <<>>".into(),
            hide_value: true,
            ..Default::default()
        };
        assert_eq!(texts(&f), ["SEE NOTE "]);
        let f = DimFormat {
            tol: TolMethod::Deviation,
            upper: 0.1,
            lower: -0.05,
            tol_precision: Some(3),
            ..Default::default()
        };
        assert_eq!(texts(&f), ["80,00", "+0,100", "-0,050"]);
    }

    #[test]
    fn layout_boxes_and_stacks() {
        let s = style();
        let plain = Label::build(80.0, "", &DimFormat::default(), &s, 3.5, 1.0);
        let basic = Label::build(80.0, "", &with(TolMethod::Basic), &s, 3.5, 1.0);
        assert!(basic.width > plain.width);
        assert_eq!(basic.lines.len(), 4);
        let dev = Label::build(80.0, "", &with(TolMethod::Deviation), &s, 3.5, 1.0);
        let p = dev.prims([100.0, 50.0], 0.0);
        let ys: Vec<f64> = p
            .iter()
            .filter_map(|q| match q {
                Prim::Text { at, .. } => Some(at[1]),
                _ => None,
            })
            .collect();
        // The deviations above and below the value, the lowest on the
        // baseline (the stack stays above the dimension line).
        assert!(ys[1] > ys[0] && ys[2] < ys[0], "{ys:?}");
        assert!((ys[2] - 50.0).abs() < 1e-9, "{ys:?}");
        // Centred on `at`.
        let xs: Vec<f64> = p
            .iter()
            .filter_map(|q| match q {
                Prim::Text { at, .. } => Some(at[0]),
                _ => None,
            })
            .collect();
        assert!((xs[0] - (100.0 - dev.width / 2.0)).abs() < 1e-9);
        // Inspection: an outline round everything.
        let f = DimFormat {
            inspection: Some(Inspection {
                shape: InspectionShape::Round,
                label: "A".into(),
                rate: "100%".into(),
            }),
            ..Default::default()
        };
        let i = Label::build(80.0, "", &f, &s, 3.5, 1.0);
        assert_eq!(i.texts(), ["A", "80,00", "100%"]);
        assert!(i.lines.len() > 20);
        assert!(i.width > plain.width + 10.0);
    }

    #[test]
    fn plain_format_is_not_saved() {
        let f = DimFormat::default();
        assert!(f.is_plain());
        assert_eq!(serde_json::to_string(&f).unwrap(), "{}");
        let g = with(TolMethod::Deviation);
        let back: DimFormat = serde_json::from_str(&serde_json::to_string(&g).unwrap()).unwrap();
        assert_eq!(back, g);
    }
}
