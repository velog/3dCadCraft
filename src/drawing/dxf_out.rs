//! DXF export of a drawing sheet (spec 4.5, Inventor's Save Copy As DXF): the sheet's display list
//! (the same `Prim`s the screen and the PDF draw) as an ASCII DXF in mm, one layer per drawing
//! layer (Visible, Hidden, Center, Dimension, Border, Title…), so a layer of the drawing is a layer
//! in AutoCAD; hidden lines on the HIDDEN line type, centre lines on CENTER; texts as TEXT, filled
//! arrowheads as SOLID.
//!
//! The file is DXF R12 (`AC1009`): the format every DXF reader takes, with no handles or object
//! sections to keep consistent. The layout follows SolidCraft's drawing DXF writer
//! (`crates/io/src/dxf_out.rs`, MIT OR Apache-2.0: a layer per kind of line, nine decimals); the
//! code is ours, over our display list.

use std::fmt::Write;

use super::sheet::{Anchor, Prim};

/// A number as DXF writes it: nine decimals, trailing zeros cut.
fn num(v: f64) -> String {
    let v = if v.abs() < 5e-10 { 0.0 } else { v };
    let mut s = format!("{v:.9}");
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.push('0');
    }
    s
}

/// The AutoCAD Color Index nearest an sRGB colour (the 255 standard colours; 7 is black on a
/// white sheet, white on a black one: used for black and white).
fn aci(c: [u8; 3]) -> u8 {
    let full = |i: u8| -> [u8; 3] {
        match i {
            1 => [255, 0, 0],
            2 => [255, 255, 0],
            3 => [0, 255, 0],
            4 => [0, 255, 255],
            5 => [0, 0, 255],
            6 => [255, 0, 255],
            8 => [128, 128, 128],
            9 => [192, 192, 192],
            _ => [0, 0, 0],
        }
    };
    let dark = c.iter().all(|v| *v < 40);
    let light = c.iter().all(|v| *v > 215);
    if dark || light {
        return 7;
    }
    let d = |a: [u8; 3]| -> i32 {
        a.iter()
            .zip(&c)
            .map(|(x, y)| (*x as i32 - *y as i32).pow(2))
            .sum()
    };
    [1u8, 2, 3, 4, 5, 6, 8, 9]
        .into_iter()
        .min_by_key(|i| d(full(*i)))
        .unwrap_or(7)
}

/// DXF R12 text: `°`, `±`, `Ø` as its `%%` codes, other characters outside ASCII as `\U+XXXX`.
fn text(t: &str) -> String {
    let mut out = String::new();
    for ch in t.chars() {
        match ch {
            '°' => out.push_str("%%d"),
            '±' => out.push_str("%%p"),
            'Ø' | '⌀' => out.push_str("%%c"),
            '%' => out.push('%'),
            c if c.is_ascii() && !c.is_ascii_control() => out.push(c),
            c if c.is_ascii_control() => out.push(' '),
            c => {
                let _ = write!(out, "\\U+{:04X}", c as u32);
            }
        }
    }
    out
}

/// A layer name DXF takes (no `<>/\":;?*|=,` characters).
fn layer_name(n: &str) -> String {
    let s: String = n
        .chars()
        .map(|c| {
            if "<>/\\\":;?*|=,`".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    if s.trim().is_empty() { "0".into() } else { s }
}

/// One sheet as DXF text; `size` is the sheet's paper size (mm).
pub fn sheet(prims: &[Prim], size: (f64, f64)) -> String {
    // The layers in the order their pens come, with their colours.
    let mut layers: Vec<(String, u8)> = Vec::new();
    for p in prims {
        if let Prim::Pen { color, layer, .. } = p {
            let n = layer_name(layer);
            if !layers.iter().any(|(m, _)| *m == n) {
                layers.push((n, aci(*color)));
            }
        }
    }
    if !layers.iter().any(|(n, _)| n == "0") {
        layers.insert(0, ("0".into(), 7));
    }
    let mut s = String::new();
    let put = |s: &mut String, code: i32, v: &str| {
        let _ = writeln!(s, "{code}\n{v}");
    };
    put(&mut s, 0, "SECTION");
    put(&mut s, 2, "HEADER");
    put(&mut s, 9, "$ACADVER");
    put(&mut s, 1, "AC1009");
    put(&mut s, 9, "$INSUNITS");
    put(&mut s, 70, "4");
    put(&mut s, 9, "$MEASUREMENT");
    put(&mut s, 70, "1");
    put(&mut s, 9, "$EXTMIN");
    put(&mut s, 10, "0.0");
    put(&mut s, 20, "0.0");
    put(&mut s, 9, "$EXTMAX");
    put(&mut s, 10, &num(size.0));
    put(&mut s, 20, &num(size.1));
    put(&mut s, 9, "$LIMMIN");
    put(&mut s, 10, "0.0");
    put(&mut s, 20, "0.0");
    put(&mut s, 9, "$LIMMAX");
    put(&mut s, 10, &num(size.0));
    put(&mut s, 20, &num(size.1));
    put(&mut s, 0, "ENDSEC");
    put(&mut s, 0, "SECTION");
    put(&mut s, 2, "TABLES");
    // Line types: dashes in mm on paper (ISO 128 proportions for 0.35 mm lines).
    let types: [(&str, &str, &[f64]); 3] = [
        ("CONTINUOUS", "Solid line", &[]),
        ("HIDDEN", "Hidden __ __ __", &[3.0, -1.5]),
        ("CENTER", "Center ____ _ ____", &[12.0, -3.0, 2.0, -3.0]),
    ];
    put(&mut s, 0, "TABLE");
    put(&mut s, 2, "LTYPE");
    put(&mut s, 70, &types.len().to_string());
    for (name, desc, dashes) in types {
        put(&mut s, 0, "LTYPE");
        put(&mut s, 2, name);
        put(&mut s, 70, "0");
        put(&mut s, 3, desc);
        put(&mut s, 72, "65");
        put(&mut s, 73, &dashes.len().to_string());
        put(&mut s, 40, &num(dashes.iter().map(|d| d.abs()).sum()));
        for d in dashes {
            put(&mut s, 49, &num(*d));
        }
    }
    put(&mut s, 0, "ENDTAB");
    put(&mut s, 0, "TABLE");
    put(&mut s, 2, "LAYER");
    put(&mut s, 70, &layers.len().to_string());
    for (name, color) in &layers {
        put(&mut s, 0, "LAYER");
        put(&mut s, 2, name);
        put(&mut s, 70, "0");
        put(&mut s, 62, &color.to_string());
        put(&mut s, 6, "CONTINUOUS");
    }
    put(&mut s, 0, "ENDTAB");
    put(&mut s, 0, "ENDSEC");
    put(&mut s, 0, "SECTION");
    put(&mut s, 2, "ENTITIES");
    let mut layer = "0".to_string();
    let mut plot = true;
    let head = |s: &mut String, kind: &str, layer: &str, ltype: Option<&str>| {
        let _ = writeln!(s, "0\n{kind}\n8\n{layer}");
        if let Some(t) = ltype {
            let _ = writeln!(s, "6\n{t}");
        }
    };
    let xy = |s: &mut String, k: i32, p: [f64; 2]| {
        let _ = writeln!(
            s,
            "{}\n{}\n{}\n{}\n{}\n0.0",
            10 + k,
            num(p[0]),
            20 + k,
            num(p[1]),
            30 + k
        );
    };
    let polyline = |s: &mut String, layer: &str, pts: &[[f64; 2]], ltype: Option<&str>| {
        if pts.len() == 2 {
            head(s, "LINE", layer, ltype);
            xy(s, 0, pts[0]);
            xy(s, 1, pts[1]);
            return;
        }
        let closed = pts.len() > 3
            && pts
                .first()
                .zip(pts.last())
                .is_some_and(|(a, b)| (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-9);
        let pts = if closed { &pts[..pts.len() - 1] } else { pts };
        head(s, "POLYLINE", layer, ltype);
        let _ = writeln!(s, "66\n1");
        xy(s, 0, [0.0, 0.0]);
        let _ = writeln!(s, "70\n{}", if closed { 1 } else { 0 });
        for p in pts {
            head(s, "VERTEX", layer, None);
            xy(s, 0, *p);
        }
        head(s, "SEQEND", layer, None);
    };
    let text_at = |s: &mut String,
                   layer: &str,
                   at: [f64; 2],
                   height: f64,
                   t: &str,
                   center: bool,
                   angle: f64| {
        head(s, "TEXT", layer, None);
        xy(s, 0, at);
        let _ = writeln!(s, "40\n{}\n1\n{}", num(height), text(t));
        if angle.abs() > 1e-12 {
            let _ = writeln!(s, "50\n{}", num(angle.to_degrees()));
        }
        if center {
            // Centred on the baseline: the alignment point (11, 21) is the anchor.
            let _ = writeln!(s, "72\n1");
            xy(s, 1, at);
        }
    };
    for p in prims {
        match p {
            Prim::Pen {
                layer: l, plot: pl, ..
            } => {
                layer = layer_name(l);
                plot = *pl;
            }
            _ if !plot => {}
            Prim::Ink { .. } => {}
            Prim::Line { a, b, .. } => polyline(&mut s, &layer, &[*a, *b], None),
            Prim::Path { pts, dashed, .. } if pts.len() >= 2 => {
                polyline(&mut s, &layer, pts, dashed.then_some("HIDDEN"));
            }
            Prim::Path { .. } => {}
            Prim::Center { a, b, chain, .. } => {
                polyline(&mut s, &layer, &[*a, *b], chain.then_some("CENTER"));
            }
            Prim::Fill { pts } if pts.len() >= 3 => {
                // SOLID takes three or four corners (the fourth in Z order): a fan of them.
                for k in 1..pts.len() - 1 {
                    let (a, b, c) = (pts[0], pts[k], pts[k + 1]);
                    head(&mut s, "SOLID", &layer, None);
                    xy(&mut s, 0, a);
                    xy(&mut s, 1, b);
                    xy(&mut s, 2, c);
                    xy(&mut s, 3, c);
                }
            }
            Prim::Fill { .. } => {}
            Prim::Text {
                at,
                height,
                text: t,
                anchor,
                angle,
            } => {
                for (k, line) in t.lines().enumerate() {
                    let down = k as f64 * height * 1.6;
                    let q = [at[0] + down * angle.sin(), at[1] - down * angle.cos()];
                    text_at(
                        &mut s,
                        &layer,
                        q,
                        *height,
                        line,
                        *anchor == Anchor::Center,
                        *angle,
                    );
                }
            }
            Prim::RunText {
                at,
                height,
                text: t,
                angle,
                center,
                ..
            } => text_at(&mut s, &layer, *at, *height, t, *center, *angle),
        }
    }
    put(&mut s, 0, "ENDSEC");
    put(&mut s, 0, "EOF");
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drawing::sheet::Weight;

    fn pen(layer: &str, color: [u8; 3]) -> Prim {
        Prim::Pen {
            color,
            plot: true,
            layer: layer.into(),
        }
    }

    /// A small sheet: visible and hidden lines on their layers, a centre line, an arrowhead, a
    /// text; read back with our own DXF reader, everything is where it was drawn.
    #[test]
    fn a_sheet_reads_back_layer_by_layer() {
        let prims = vec![
            pen("Border (ISO)", [0, 0, 0]),
            Prim::Line {
                a: [0.0, 0.0],
                b: [420.0, 0.0],
                weight: Weight::Thick,
            },
            pen("Visible (ISO)", [0, 0, 0]),
            Prim::Path {
                pts: vec![[10.0, 10.0], [50.0, 10.0], [50.0, 30.0], [10.0, 10.0]],
                width: 0.5,
                dashed: false,
            },
            pen("Hidden (ISO)", [0, 0, 255]),
            Prim::Path {
                pts: vec![[10.0, 20.0], [40.0, 20.0], [40.0, 25.0]],
                width: 0.35,
                dashed: true,
            },
            pen("Centerline (ISO)", [255, 0, 0]),
            Prim::Center {
                a: [0.0, 5.0],
                b: [60.0, 5.0],
                width: 0.25,
                chain: true,
            },
            pen("Dimension (ISO)", [0, 128, 0]),
            Prim::Fill {
                pts: vec![[0.0, 0.0], [3.0, 1.0], [3.0, -1.0]],
            },
            Prim::Text {
                at: [30.0, 40.0],
                height: 3.5,
                text: "Ø20 ±0.1 Вал".into(),
                anchor: Anchor::Center,
                angle: 0.0,
            },
            Prim::Pen {
                color: [0, 0, 0],
                plot: false,
                layer: "Sketch".into(),
            },
            Prim::Line {
                a: [0.0, 0.0],
                b: [1.0, 1.0],
                weight: Weight::Thin,
            },
        ];
        let t = sheet(&prims, (420.0, 297.0));
        assert!(t.contains("AC1009") && t.contains("$INSUNITS"));
        assert!(t.contains("\n6\nHIDDEN\n") && t.contains("\n6\nCENTER\n"));
        assert!(t.contains("%%c20 %%p0.1 \\U+0412"), "texts in DXF codes");
        let d = crate::sketch::dxf::read(t.as_bytes()).unwrap();
        assert_eq!(d.unit, 1.0);
        let on = |l: &str| d.items.iter().filter(|i| i.layer == l).count();
        assert_eq!(on("Border (ISO)"), 1);
        // The closed triangle: three edges.
        assert_eq!(on("Visible (ISO)"), 3);
        assert_eq!(on("Hidden (ISO)"), 2);
        assert_eq!(on("Centerline (ISO)"), 1);
        // The text (the SOLID is not read back as geometry).
        assert_eq!(on("Dimension (ISO)"), 1);
        assert_eq!(on("Sketch"), 0, "a layer that does not plot is left out");
        assert!(d.items.iter().any(|i| matches!(&i.geom,
            crate::sketch::dxf::DxfGeom::Text { text, .. } if text == "Ø20 ±0.1 Вал")));
        let crate::sketch::dxf::DxfGeom::Line(a, b) = d.items[0].geom else {
            panic!()
        };
        assert_eq!((a.x, b.x), (0.0, 420.0));
    }

    #[test]
    fn numbers_colours_and_names() {
        assert_eq!(num(1.0), "1.0");
        assert_eq!(num(0.123456789123), "0.123456789");
        assert_eq!(num(-1e-12), "0.0");
        assert_eq!(aci([0, 0, 0]), 7);
        assert_eq!(aci([250, 10, 10]), 1);
        assert_eq!(aci([10, 10, 240]), 5);
        assert_eq!(layer_name("A/B:C"), "A_B_C");
    }
}
