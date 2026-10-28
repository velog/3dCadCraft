//! The display list of a sheet (spec §6.3 b): border, title block, the views'
//! lines and the annotations as [`Prim`]s in sheet mm. PDF (and later DXF /
//! SVG) write this list, so paper and screen show the same thing.

use std::collections::HashMap;
use std::sync::Arc;

use super::annot::{self, Placement};
use super::hlr::ViewLines;
use super::sheet::Weight;
use super::sheet::{self, Anchor, Prim, TitleField};
use super::{DrawingDocument, Projection, ViewKind, ViewStyle};

/// Visible line width (ISO 128: 0.5 mm).
pub const VISIBLE_MM: f64 = 0.5;
/// Hidden line width (0.35 mm, dashed).
pub const HIDDEN_MM: f64 = 0.35;

/// Where each view of sheet `i` sits, with its lines (views whose lines are
/// missing are left out).
pub fn placements(
    doc: &DrawingDocument,
    i: usize,
    lines: &dyn Fn(u32) -> Option<Arc<ViewLines>>,
) -> HashMap<u32, (Placement, ViewStyle, Arc<ViewLines>)> {
    let mut out = HashMap::new();
    let Some(sheet) = doc.sheets.get(i) else {
        return out;
    };
    for v in &sheet.views {
        let Some(l) = lines(v.id) else {
            continue;
        };
        let (scale, style) = doc.effective(sheet, v.id).unwrap_or((v.scale, v.style));
        out.insert(
            v.id,
            (
                Placement {
                    position: v.position,
                    scale,
                    center: l.center(),
                },
                style,
                l,
            ),
        );
    }
    out
}

/// The whole sheet `i` as primitives; `title` fills the title block,
/// `lines` gives each view's lines. Orphaned annotations are left out.
pub fn sheet_list(
    doc: &DrawingDocument,
    i: usize,
    title: &dyn Fn(TitleField) -> String,
    lines: &dyn Fn(u32) -> Option<Arc<ViewLines>>,
    tables: &super::parts::Tables,
) -> Vec<Prim> {
    let st = &doc.styles;
    let mut out = Vec::new();
    if let Some(pen) = st.pen(&st.defaults.border_layer) {
        out.push(pen);
        out.extend(sheet::format_prims(doc, i, title));
    }
    let Some(sheet) = doc.sheets.get(i) else {
        return out;
    };
    let placed = placements(doc, i, lines);
    let visible = st.layer(&st.defaults.visible_layer);
    let hidden = st.layer(&st.defaults.hidden_layer);
    for v in &sheet.views {
        let Some((pl, style, l)) = placed.get(&v.id) else {
            continue;
        };
        if *style == ViewStyle::HiddenShown
            && let Some(pen) = st.pen(&hidden.name)
        {
            out.push(pen);
            for h in &l.hidden {
                out.push(Prim::Path {
                    pts: h.iter().map(|q| pl.to_sheet(*q)).collect(),
                    width: hidden.weight,
                    dashed: true,
                });
            }
        }
        if let Some(pen) = st.pen(&visible.name) {
            out.push(pen);
            for p in &l.visible {
                out.push(Prim::Path {
                    pts: p.iter().map(|q| pl.to_sheet(*q)).collect(),
                    width: visible.weight,
                    dashed: false,
                });
            }
        }
    }
    let view = |id: u32| placed.get(&id).map(|(pl, _, l)| (*pl, l.as_ref()));
    out.extend(view_extras(doc, i, &view));
    let look = st.look();
    for a in &sheet.annotations {
        let Some(pen) = st.pen(a.layer_name(&st.defaults)) else {
            continue;
        };
        if let Ok(s) = annot::resolve_in(&a.kind, &view, &look, tables) {
            out.push(pen);
            out.extend(s.prims);
        }
    }
    out
}

/// What views add around their lines: section hatching, section lines and
/// detail fences on the parents (Definition in Base View), detail
/// boundaries and the view labels.
pub fn view_extras<'a>(
    doc: &DrawingDocument,
    i: usize,
    view: &dyn Fn(u32) -> Option<(Placement, &'a ViewLines)>,
) -> Vec<Prim> {
    let mut out = Vec::new();
    let Some(sheet) = doc.sheets.get(i) else {
        return out;
    };
    let st = &doc.styles;
    let hs = st.hatch(&st.defaults.hatch);
    let hatch_layer = st.layer(&st.defaults.hatch_layer);
    let center_w = st.layer(&st.defaults.section_layer).weight;
    let va = st.view_annotation(&st.defaults.view_annotation);
    let (bend_c, bend_x) = st.bend_layers();
    let pen = |l: &super::style::LayerStyle| {
        l.on.then_some(Prim::Pen {
            color: l.color,
            plot: l.plot,
        })
    };
    let section_pen = st.pen(&st.defaults.section_layer);
    let detail_pen = st.pen(&st.defaults.detail_layer);
    let symbol_pen = st.pen(&st.defaults.symbol_layer);
    for v in &sheet.views {
        let Some((pl, l)) = view(v.id) else {
            continue;
        };
        // Flat patterns: bend centre lines and bend extents.
        for b in &l.bends {
            if let Some(p) = pen(&bend_c)
                && v.display.bend_centerlines
            {
                out.push(p);
                out.push(Prim::Center {
                    a: pl.to_sheet(b.centre[0]),
                    b: pl.to_sheet(b.centre[1]),
                    width: bend_c.weight,
                    chain: bend_c.line_type != super::style::LineType::Continuous,
                });
            }
            if let Some(p) = pen(&bend_x)
                && v.display.bend_extents
            {
                out.push(p);
                for z in &b.zone {
                    out.push(Prim::Path {
                        pts: vec![pl.to_sheet(z[0]), pl.to_sheet(z[1])],
                        width: bend_x.weight,
                        dashed: bend_x.line_type == super::style::LineType::Dashed,
                    });
                }
            }
        }
        // Hatching of cut faces (the hatch style: angle, spacing, double).
        if v.display.hatching
            && let Some(p) = pen(&hatch_layer)
        {
            out.push(p);
            let loops: Vec<Vec<[f64; 2]>> = l
                .section
                .iter()
                .map(|lp| lp.iter().map(|q| pl.to_sheet(*q)).collect())
                .collect();
            let angles: &[f64] = if hs.double {
                &[hs.angle, hs.angle + 90.0]
            } else {
                &[hs.angle]
            };
            for ang in angles {
                for (a, b) in hatch(&loops, ang.to_radians(), hs.spacing) {
                    out.push(Prim::Path {
                        pts: vec![a, b],
                        width: hatch_layer.weight,
                        dashed: false,
                    });
                }
            }
        }
        // The detail view's boundary.
        if let ViewKind::Detail { radius, .. } = v.kind
            && let Some(p) = detail_pen.clone()
        {
            out.push(p);
            out.push(circle(v.position, radius * pl.scale));
        }
        // Definitions drawn on the parent.
        if v.display.definition_in_base
            && let Some(pid) = v.parent()
            && let Some((ppl, _)) = view(pid)
        {
            match v.kind {
                ViewKind::Section { a, b, side, .. } if section_pen.is_some() => {
                    out.extend(section_pen.clone());
                    section_line(
                        &mut out,
                        doc,
                        &ppl,
                        (a, b, side),
                        &v.identifier,
                        center_w,
                        va.identifier_height,
                    );
                }
                ViewKind::Detail { center, radius, .. } if detail_pen.is_some() => {
                    out.extend(detail_pen.clone());
                    let c = ppl.to_sheet(center);
                    let r = radius * ppl.scale;
                    out.push(circle(c, r));
                    let k = std::f64::consts::FRAC_1_SQRT_2;
                    out.push(Prim::Text {
                        at: [c[0] + (r + 2.0) * k, c[1] + (r + 2.0) * k],
                        height: va.identifier_height,
                        text: v.identifier.clone(),
                        anchor: Anchor::Left,
                        angle: 0.0,
                    });
                }
                _ => {}
            }
        }
        // The label under the view.
        if v.label_visible
            && let Some(p) = symbol_pen.clone()
        {
            out.push(p);
            let (_, h) = l.size();
            let below = match v.kind {
                ViewKind::Detail { radius, .. } => v.position[1] - radius * pl.scale,
                _ => {
                    let c = l.center();
                    pl.to_sheet([c[0], l.min[1]])[1]
                }
            } - 3.0;
            let _ = h;
            let h = va.label_height;
            for (k, t) in v.label_lines(pl.scale).into_iter().enumerate() {
                out.push(Prim::Text {
                    at: [v.position[0], below - h - k as f64 * h * 1.45],
                    height: h,
                    text: t,
                    anchor: Anchor::Center,
                    angle: 0.0,
                });
            }
        }
    }
    out
}

/// A thin circle as a polyline.
fn circle(c: [f64; 2], r: f64) -> Prim {
    let n = 96;
    Prim::Path {
        pts: (0..=n)
            .map(|k| {
                let a = std::f64::consts::TAU * k as f64 / n as f64;
                [c[0] + r * a.cos(), c[1] + r * a.sin()]
            })
            .collect(),
        width: 0.25,
        dashed: false,
    }
}

/// The section line on the parent: a thin chain line, thick ends, arrows in
/// the viewing direction and the letter at each end (ISO 128-44).
fn section_line(
    out: &mut Vec<Prim>,
    doc: &DrawingDocument,
    pl: &Placement,
    (a, b, side): ([f64; 2], [f64; 2], i8),
    letter: &str,
    width: f64,
    letter_height: f64,
) {
    let (pa, pb) = (pl.to_sheet(a), pl.to_sheet(b));
    let d = [pb[0] - pa[0], pb[1] - pa[1]];
    let len = d[0].hypot(d[1]);
    if len < 1e-9 {
        return;
    }
    let l = [d[0] / len, d[1] / len];
    let k = f64::from(side.signum());
    let s2 = [-l[1] * k, l[0] * k];
    // The viewer looks along −toward: third angle towards the far side of
    // the view, first angle towards the view.
    let look = match doc.standard.projection {
        Projection::ThirdAngle => [-s2[0], -s2[1]],
        Projection::FirstAngle => s2,
    };
    out.push(Prim::Center {
        a: pa,
        b: pb,
        width,
        chain: true,
    });
    let thick = 5.0f64.min(len * 0.25);
    for (end, inward) in [(pa, l), (pb, [-l[0], -l[1]])] {
        out.push(Prim::Line {
            a: end,
            b: [end[0] + inward[0] * thick, end[1] + inward[1] * thick],
            weight: Weight::Thick,
        });
        // Arrow from the line's end in the viewing direction.
        let tip = [end[0] + look[0] * 7.0, end[1] + look[1] * 7.0];
        out.push(Prim::Line {
            a: end,
            b: tip,
            weight: Weight::Thin,
        });
        let back = [tip[0] - look[0] * 3.0, tip[1] - look[1] * 3.0];
        let n = [-look[1] * 0.5, look[0] * 0.5];
        out.push(Prim::Fill {
            pts: vec![
                tip,
                [back[0] + n[0], back[1] + n[1]],
                [back[0] - n[0], back[1] - n[1]],
            ],
        });
        let out_of_line = [-inward[0], -inward[1]];
        out.push(Prim::Text {
            at: [
                end[0] + out_of_line[0] * 3.0 + look[0] * 3.0 - 1.5,
                end[1] + out_of_line[1] * 3.0 + look[1] * 3.0 - 2.5,
            ],
            height: letter_height,
            text: letter.to_string(),
            anchor: Anchor::Left,
            angle: 0.0,
        });
    }
}

/// Hatch lines (sheet mm) filling the even-odd region of `loops` at
/// `angle`, `spacing` apart.
pub fn hatch(loops: &[Vec<[f64; 2]>], angle: f64, spacing: f64) -> Vec<([f64; 2], [f64; 2])> {
    let mut out = Vec::new();
    if loops.is_empty() || spacing <= 0.0 {
        return out;
    }
    let (s, c) = angle.sin_cos();
    // Rotate so the hatch lines are horizontal: v = −s·x + c·y.
    let rot = |p: [f64; 2]| [c * p[0] + s * p[1], -s * p[0] + c * p[1]];
    let back = |q: [f64; 2]| [c * q[0] - s * q[1], s * q[0] + c * q[1]];
    let edges: Vec<([f64; 2], [f64; 2])> = loops
        .iter()
        .flat_map(|lp| lp.windows(2).map(|w| (rot(w[0]), rot(w[1]))))
        .collect();
    let (lo, hi) = edges.iter().fold((f64::MAX, f64::MIN), |(lo, hi), (a, b)| {
        (lo.min(a[1]).min(b[1]), hi.max(a[1]).max(b[1]))
    });
    if lo > hi {
        return out;
    }
    let mut y = (lo / spacing).floor() * spacing + spacing * 0.5;
    while y < hi {
        let mut xs: Vec<f64> = edges
            .iter()
            .filter(|(a, b)| (a[1] > y) != (b[1] > y))
            .map(|(a, b)| a[0] + (y - a[1]) / (b[1] - a[1]) * (b[0] - a[0]))
            .collect();
        xs.sort_by(f64::total_cmp);
        for pair in xs.chunks(2) {
            if let [x0, x1] = pair {
                out.push((back([*x0, y]), back([*x1, y])));
            }
        }
        y += spacing;
        if out.len() > 200_000 {
            break;
        }
    }
    out
}
