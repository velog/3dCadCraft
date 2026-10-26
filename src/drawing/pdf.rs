//! PDF export of drawing sheets (spec §3.5, §6.3 d) with `krilla`: one page
//! per sheet at its paper size, vector lines in ISO weights, texts in an
//! embedded (subsetted) TrueType font, so Cyrillic title blocks print.

use krilla::Document;
use krilla::color::rgb;
use krilla::geom::{PathBuilder, Point, Transform};
use krilla::num::NormalizedF32;
use krilla::page::PageSettings;
use krilla::paint::{Fill, FillRule, LineCap, Stroke, StrokeDash};
use krilla::text::{Font, TextDirection};

use super::annot::text_width;
use super::sheet::{Anchor, Prim};

/// Options of the PDF export (Inventor's PDF Drawing options).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PdfOptions {
    /// Every line at one weight (0.25 mm).
    pub remove_line_weights: bool,
}

/// One page: paper size (mm) and what is on it.
pub struct Page {
    /// Width and height in mm.
    pub size: (f64, f64),
    /// The sheet's display list.
    pub prims: Vec<Prim>,
}

/// PDF points per mm.
const PT: f64 = 72.0 / 25.4;

/// Writes `pages` as a PDF. `font` is a TrueType / OpenType font for the
/// texts (without one, texts are left out).
pub fn write(pages: &[Page], font: Option<Vec<u8>>, opts: PdfOptions) -> Result<Vec<u8>, String> {
    let mut doc = Document::new();
    let font = font.and_then(|f| Font::new(f.into(), 0));
    let black: krilla::paint::Paint = rgb::Color::new(0, 0, 0).into();
    for page in pages {
        let (w, h) = page.size;
        let settings = PageSettings::from_wh((w * PT) as f32, (h * PT) as f32)
            .ok_or_else(|| format!("PDF: bad page size {w} × {h} mm"))?;
        let mut p = doc.start_page_with(settings);
        let mut s = p.surface();
        // Sheet mm (y up) → PDF points (y down from the top).
        let at = |q: [f64; 2]| Point::from_xy((q[0] * PT) as f32, ((h - q[1]) * PT) as f32);
        let stroke = |width_mm: f64, dash: Option<Vec<f64>>| Stroke {
            paint: black.clone(),
            width: (if opts.remove_line_weights {
                0.25
            } else {
                width_mm
            } * PT) as f32,
            line_cap: LineCap::Butt,
            dash: dash.map(|d| StrokeDash {
                array: d.iter().map(|v| (v * PT) as f32).collect(),
                offset: 0.0,
            }),
            ..Stroke::default()
        };
        let path = |pts: &[[f64; 2]], close: bool| {
            let mut pb = PathBuilder::new();
            for (k, q) in pts.iter().enumerate() {
                let p = at(*q);
                if k == 0 {
                    pb.move_to(p.x, p.y);
                } else {
                    pb.line_to(p.x, p.y);
                }
            }
            if close {
                pb.close();
            }
            pb.finish()
        };
        for prim in &page.prims {
            match prim {
                Prim::Line { a, b, weight } => {
                    if let Some(pa) = path(&[*a, *b], false) {
                        s.set_fill(None);
                        s.set_stroke(Some(stroke(weight.mm(), None)));
                        s.draw_path(&pa);
                    }
                }
                Prim::Path { pts, width, dashed } => {
                    if let Some(pa) = path(pts, false) {
                        s.set_fill(None);
                        s.set_stroke(Some(stroke(*width, dashed.then(|| vec![3.0, 1.5]))));
                        s.draw_path(&pa);
                    }
                }
                Prim::Center { a, b } => {
                    if let Some(pa) = path(&[*a, *b], false) {
                        s.set_fill(None);
                        s.set_stroke(Some(stroke(0.25, Some(vec![12.0, 1.5, 1.5, 1.5]))));
                        s.draw_path(&pa);
                    }
                }
                Prim::Fill { pts } => {
                    if let Some(pa) = path(pts, true) {
                        s.set_stroke(None);
                        s.set_fill(Some(Fill {
                            paint: black.clone(),
                            opacity: NormalizedF32::ONE,
                            rule: FillRule::NonZero,
                        }));
                        s.draw_path(&pa);
                    }
                }
                Prim::Text {
                    at: q,
                    height,
                    text,
                    anchor,
                    angle,
                } => {
                    let Some(f) = &font else {
                        continue;
                    };
                    // Cap height ≈ 0.7 of the font size.
                    let size = (height / 0.7 * PT) as f32;
                    let shift = match anchor {
                        Anchor::Left => 0.0,
                        Anchor::Center => -text_width(text, *height) * 0.5,
                    };
                    let origin = at(*q);
                    s.set_stroke(None);
                    s.set_fill(Some(Fill {
                        paint: black.clone(),
                        opacity: NormalizedF32::ONE,
                        rule: FillRule::NonZero,
                    }));
                    // y is down in PDF space: a counter-clockwise angle on
                    // the sheet is a negative rotation here.
                    s.push_transform(&Transform::from_rotate_at(
                        -angle.to_degrees() as f32,
                        origin.x,
                        origin.y,
                    ));
                    s.draw_text(
                        Point::from_xy(origin.x + (shift * PT) as f32, origin.y),
                        f.clone(),
                        size,
                        text,
                        false,
                        TextDirection::Auto,
                    );
                    s.pop();
                }
            }
        }
        s.finish();
        p.finish();
    }
    doc.finish().map_err(|e| format!("PDF: {e:?}"))
}
