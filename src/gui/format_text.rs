//! The Format Text editor (`docs/spec/4.9-format-text.md`): note settings
//! (justification, line spacing, width factor, rotation, border, background
//! fill), character settings on the selection (size, colour, bold, italic,
//! underline, strikethrough, text case) and the symbol palette, over a text
//! box that shows the formatting. Used by Text and Leader Text; GUI-free text
//! layout is `drawing::rich`.

use eframe::egui::{self, Color32, FontId, RichText as Label, Stroke};

use super::valuefield::ExprValue;
use crate::drawing::rich::{
    self, HAlign, LineSpace, RichText, RunStyle, SYMBOLS, SymbolGroup, VAlign,
};

/// The editor's state besides the text and its formatting.
#[derive(Clone, Debug, Default)]
pub(super) struct Editor {
    /// The last selection of the text box (char indices, start ≤ end).
    pub sel: (usize, usize),
    /// Put the cursor here on the next frame (after inserting a symbol).
    pub cursor_to: Option<usize>,
    /// Focus the text box on the next frame.
    pub focus: bool,
}

const TEXT_ID: &str = "format_text_box";

/// The whole editor: returns whether anything changed.
pub(super) fn editor(
    ui: &mut egui::Ui,
    text: &mut String,
    rich: &mut RichText,
    ed: &mut Editor,
    height: &mut f64,
) -> bool {
    let mut changed = false;
    changed |= note_row(ui, rich, height);
    ui.separator();
    changed |= char_row(ui, text, rich, ed, *height);
    ui.add_space(2.0);
    changed |= symbol_palette(ui, text, rich, ed);
    ui.add_space(2.0);
    changed |= text_box(ui, text, rich, ed, *height);
    changed
}

fn note_row(ui: &mut egui::Ui, rich: &mut RichText, height: &mut f64) -> bool {
    let before = (rich.clone(), *height);
    egui::Grid::new("ft_note").num_columns(2).show(ui, |ui| {
        ui.label("Text height");
        ui.add(ExprValue::new(height).range(0.5..=50.0).suffix(" mm"));
        ui.end_row();
        ui.label("Justification");
        ui.horizontal(|ui| {
            for (a, t) in [
                (HAlign::Left, "Left"),
                (HAlign::Center, "Center"),
                (HAlign::Right, "Right"),
            ] {
                ui.selectable_value(&mut rich.h, a, t);
            }
            ui.separator();
            for (a, t) in [
                (VAlign::Top, "Top"),
                (VAlign::Middle, "Middle"),
                (VAlign::Bottom, "Bottom"),
            ] {
                ui.selectable_value(&mut rich.v, a, t);
            }
        });
        ui.end_row();
        ui.label("Line Space");
        ui.horizontal(|ui| {
            let (mut kind, mut v) = match rich.line_space {
                LineSpace::Multiple(f) => (0, f),
                LineSpace::Exactly(d) => (1, d),
            };
            let label = match (kind, v) {
                (0, f) if (f - 1.0).abs() < 1e-9 => "Single".to_string(),
                (0, f) if (f - 1.5).abs() < 1e-9 => "1.5 Lines".to_string(),
                (0, f) if (f - 2.0).abs() < 1e-9 => "Double".to_string(),
                (0, _) => "Multiple".to_string(),
                _ => "Exactly".to_string(),
            };
            egui::ComboBox::from_id_salt("ft_line_space")
                .selected_text(label)
                .show_ui(ui, |ui| {
                    for (t, k, val) in [
                        ("Single", 0, 1.0),
                        ("1.5 Lines", 0, 1.5),
                        ("Double", 0, 2.0),
                        ("Multiple", 0, 1.25),
                        ("Exactly", 1, 6.0),
                    ] {
                        if ui.selectable_label(false, t).clicked() {
                            kind = k;
                            v = val;
                        }
                    }
                });
            if kind == 0 {
                ui.add(ExprValue::new(&mut v).range(0.5..=5.0).suffix(" ×"));
                rich.line_space = LineSpace::Multiple(v);
            } else {
                ui.add(ExprValue::new(&mut v).range(0.5..=100.0).suffix(" mm"));
                rich.line_space = LineSpace::Exactly(v);
            }
        });
        ui.end_row();
        ui.label("Stretch");
        let mut pct = rich.stretch * 100.0;
        ui.add(ExprValue::new(&mut pct).range(10.0..=500.0).suffix(" %"));
        rich.stretch = pct / 100.0;
        ui.end_row();
        ui.label("Rotation");
        ui.horizontal(|ui| {
            ui.add(
                ExprValue::new(&mut rich.rotation)
                    .range(-360.0..=360.0)
                    .suffix("°"),
            );
            for a in [0.0, 90.0, 180.0, 270.0] {
                if ui.small_button(format!("{a:.0}°")).clicked() {
                    rich.rotation = a;
                }
            }
        });
        ui.end_row();
        ui.label("Frame");
        ui.horizontal(|ui| {
            ui.checkbox(&mut rich.border, "Text Border");
            let mut fill = rich.fill.is_some();
            ui.checkbox(&mut fill, "Background Fill");
            if fill {
                let c = rich.fill.unwrap_or([255, 255, 255]);
                let mut col = Color32::from_rgb(c[0], c[1], c[2]);
                ui.color_edit_button_srgba(&mut col);
                rich.fill = Some([col.r(), col.g(), col.b()]);
            } else {
                rich.fill = None;
            }
        });
        ui.end_row();
    });
    (rich.clone(), *height) != before
}

/// Size, colour, B I U S and text case on the selection.
fn char_row(
    ui: &mut egui::Ui,
    text: &mut String,
    rich: &mut RichText,
    ed: &mut Editor,
    height: f64,
) -> bool {
    let (a, b) = ed.sel;
    let styles = rich.styles(text, &RunStyle::default());
    let picked = &styles[a.min(styles.len())..b.min(styles.len())];
    let all = |f: fn(&RunStyle) -> bool| !picked.is_empty() && picked.iter().all(f);
    let mut changed = false;
    let none = a == b;
    ui.horizontal(|ui| {
        ui.add_enabled_ui(!none, |ui| {
            let toggle = |ui: &mut egui::Ui, t: Label, on: bool, tip: &str| -> Option<bool> {
                ui.add(egui::Button::new(t).selected(on))
                    .on_hover_text(tip)
                    .clicked()
                    .then_some(!on)
            };
            if let Some(v) = toggle(
                ui,
                Label::new("B").strong(),
                all(|s| s.bold == Some(true)),
                "Bold",
            ) {
                rich.apply(
                    a,
                    b,
                    RunStyle {
                        bold: Some(v),
                        ..Default::default()
                    },
                );
                changed = true;
            }
            if let Some(v) = toggle(
                ui,
                Label::new("I").italics(),
                all(|s| s.italic == Some(true)),
                "Italic",
            ) {
                rich.apply(
                    a,
                    b,
                    RunStyle {
                        italic: Some(v),
                        ..Default::default()
                    },
                );
                changed = true;
            }
            if let Some(v) = toggle(
                ui,
                Label::new("U").underline(),
                all(|s| s.underline == Some(true)),
                "Underline",
            ) {
                rich.apply(
                    a,
                    b,
                    RunStyle {
                        underline: Some(v),
                        ..Default::default()
                    },
                );
                changed = true;
            }
            if let Some(v) = toggle(
                ui,
                Label::new("S").strikethrough(),
                all(|s| s.strike == Some(true)),
                "Strikethrough",
            ) {
                rich.apply(
                    a,
                    b,
                    RunStyle {
                        strike: Some(v),
                        ..Default::default()
                    },
                );
                changed = true;
            }
            ui.separator();
            ui.label("Size");
            let mut size = picked.first().and_then(|s| s.size).unwrap_or(height);
            if ui
                .add(ExprValue::new(&mut size).range(0.5..=50.0).suffix(" mm"))
                .changed()
            {
                rich.apply(
                    a,
                    b,
                    RunStyle {
                        size: Some(size),
                        ..Default::default()
                    },
                );
                changed = true;
            }
            ui.separator();
            let current = picked.first().and_then(|s| s.color);
            let mut col = current.map_or(Color32::BLACK, |c| Color32::from_rgb(c[0], c[1], c[2]));
            if ui.color_edit_button_srgba(&mut col).changed() {
                rich.apply(
                    a,
                    b,
                    RunStyle {
                        color: Some([col.r(), col.g(), col.b()]),
                        ..Default::default()
                    },
                );
                changed = true;
            }
            if ui
                .add(egui::Button::new("By Layer").selected(current.is_none()))
                .on_hover_text("The note's layer colour")
                .clicked()
            {
                // Spans cannot unset a field: drop the colours of the range.
                for s in &mut rich.spans {
                    if s.start < b && s.end > a {
                        s.style.color = None;
                    }
                }
                changed = true;
            }
            ui.separator();
            for (t, f) in [("ABC", 0u8), ("abc", 1), ("Abc", 2)] {
                if ui.small_button(t).on_hover_text("Text Case").clicked() {
                    let chars: Vec<char> = text.chars().collect();
                    let mid: String = chars[a.min(chars.len())..b.min(chars.len())]
                        .iter()
                        .collect();
                    let new_mid = match f {
                        0 => mid.to_uppercase(),
                        1 => mid.to_lowercase(),
                        _ => title_case(&mid),
                    };
                    if new_mid.chars().count() == mid.chars().count() {
                        let new: String = chars[..a]
                            .iter()
                            .copied()
                            .chain(new_mid.chars())
                            .chain(chars[b.min(chars.len())..].iter().copied())
                            .collect();
                        *text = new;
                        changed = true;
                    }
                }
            }
        });
        if none {
            ui.weak("Select text to format it");
        }
    });
    changed
}

fn title_case(s: &str) -> String {
    let mut out = String::new();
    let mut start = true;
    for c in s.chars() {
        if start && c.is_alphabetic() {
            out.extend(c.to_uppercase());
            start = false;
        } else {
            out.extend(c.to_lowercase());
            if !c.is_alphanumeric() {
                start = true;
            }
        }
    }
    out
}

/// One symbol button: our vector glyph (fonts lack most of them).
fn symbol_button(ui: &mut egui::Ui, c: char, name: &str) -> bool {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::click());
    let visuals = ui.style().interact(&resp);
    ui.painter().rect_filled(rect, 3.0, visuals.bg_fill);
    let ink = visuals.text_color();
    let h = 3.5;
    let k = 16.0 / h as f32;
    let o = rect.left_bottom() + egui::vec2(4.0, -5.0);
    let to = |q: [f64; 2]| egui::pos2(o.x + q[0] as f32 * k, o.y - q[1] as f32 * k);
    match rich::glyph(c, [0.0, 0.0], h, 1.0) {
        Some((prims, _)) => {
            for p in prims {
                match p {
                    crate::drawing::sheet::Prim::Line { a, b, .. } => {
                        ui.painter()
                            .line_segment([to(a), to(b)], Stroke::new(1.2, ink));
                    }
                    crate::drawing::sheet::Prim::Fill { pts } => {
                        let mut pts: Vec<egui::Pos2> = pts.iter().map(|q| to(*q)).collect();
                        let area: f32 = (0..pts.len())
                            .map(|i| {
                                let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                                a.x * b.y - b.x * a.y
                            })
                            .sum();
                        if area > 0.0 {
                            pts.reverse();
                        }
                        ui.painter()
                            .add(egui::Shape::convex_polygon(pts, ink, Stroke::NONE));
                    }
                    crate::drawing::sheet::Prim::RunText { at, text, .. } => {
                        ui.painter().text(
                            to(at) + egui::vec2(0.0, -4.0),
                            egui::Align2::CENTER_CENTER,
                            text,
                            FontId::proportional(9.0),
                            ink,
                        );
                    }
                    _ => {}
                }
            }
        }
        None => {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                c,
                FontId::proportional(15.0),
                ink,
            );
        }
    }
    resp.on_hover_text(name).clicked()
}

fn symbol_palette(
    ui: &mut egui::Ui,
    text: &mut String,
    rich: &mut RichText,
    ed: &mut Editor,
) -> bool {
    let mut insert = None;
    egui::CollapsingHeader::new("Symbols")
        .default_open(true)
        .show(ui, |ui| {
            for g in SymbolGroup::ALL {
                ui.horizontal_wrapped(|ui| {
                    ui.add_sized(
                        [150.0, 20.0],
                        egui::Label::new(Label::new(g.label()).weak()),
                    );
                    for (c, name, _) in SYMBOLS.iter().filter(|(_, _, gg)| *gg == g) {
                        if symbol_button(ui, *c, name) {
                            insert = Some(*c);
                        }
                    }
                });
            }
        });
    let Some(c) = insert else {
        return false;
    };
    let old = text.clone();
    let chars: Vec<char> = text.chars().collect();
    let (a, b) = ed.sel;
    let (a, b) = (a.min(chars.len()), b.min(chars.len()));
    *text = chars[..a]
        .iter()
        .copied()
        .chain(std::iter::once(c))
        .chain(chars[b..].iter().copied())
        .collect();
    rich.follow_edit(&old, text);
    ed.sel = (a + 1, a + 1);
    ed.cursor_to = Some(a + 1);
    ed.focus = true;
    true
}

/// The text box, showing size, colour, italic, underline and strikethrough
/// (the editor's font has no bold face: bold runs show in the strong text
/// colour).
fn text_box(
    ui: &mut egui::Ui,
    text: &mut String,
    rich: &mut RichText,
    ed: &mut Editor,
    height: f64,
) -> bool {
    let id = egui::Id::new(TEXT_ID);
    let fmt = rich.clone();
    let ink = ui.visuals().text_color();
    let strong = ui.visuals().strong_text_color();
    let mut layouter = move |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap: f32| {
        let s = buf.as_str();
        let styles = fmt.styles(s, &RunStyle::default());
        let mut job = egui::text::LayoutJob::default();
        for (c, st) in s
            .chars()
            .zip(styles.iter().chain(std::iter::repeat(&RunStyle::default())))
        {
            let size = (st.size.unwrap_or(height) / height * 15.0).clamp(8.0, 40.0) as f32;
            let color = st
                .color
                .map_or(if st.bold == Some(true) { strong } else { ink }, |c| {
                    Color32::from_rgb(c[0], c[1], c[2])
                });
            let line = Stroke::new(1.0, color);
            let mut buf = [0u8; 4];
            job.append(
                c.encode_utf8(&mut buf),
                0.0,
                egui::TextFormat {
                    font_id: FontId::proportional(size),
                    color,
                    italics: st.italic == Some(true),
                    underline: if st.underline == Some(true) {
                        line
                    } else {
                        Stroke::NONE
                    },
                    strikethrough: if st.strike == Some(true) {
                        line
                    } else {
                        Stroke::NONE
                    },
                    ..Default::default()
                },
            );
        }
        job.wrap.max_width = wrap;
        ui.fonts_mut(|f| f.layout_job(job))
    };
    let old = text.clone();
    let out = egui::TextEdit::multiline(text)
        .id(id)
        .desired_rows(4)
        .desired_width(460.0)
        .layouter(&mut layouter)
        .show(ui);
    if let Some(r) = out.cursor_range {
        let (p, q) = (r.primary.index, r.secondary.index);
        let (p, q): (usize, usize) = (p.into(), q.into());
        ed.sel = (p.min(q), p.max(q));
    }
    if let Some(at) = ed.cursor_to.take() {
        let mut state = out.state.clone();
        let c = egui::text::CCursor::new(at);
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::one(c)));
        state.store(ui.ctx(), id);
    }
    if ed.focus {
        out.response.request_focus();
        ed.focus = false;
    }
    if *text != old {
        rich.follow_edit(&old, text);
        return true;
    }
    false
}
