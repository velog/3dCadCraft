//! Texts of drawing sketches (spec 4.13 §3.2): the Text tool places a
//! single-line text with fields; Select picks one, a double-click edits it,
//! Delete removes it.

use eframe::egui::{self, Color32, Rect};

use super::CadCraftApp;
use super::drawing::SheetCam;
use super::drawing_sketch::DrwSketch;
use super::valuefield::ExprValue;
use crate::drawing::annot::Placement;
use crate::drawing::fields::{self, Field};
use crate::drawing::sheet::Prim;
use crate::drawing::view_sketch::{DrawingText, TextAnchor};

/// The Text dialog of a drawing sketch: a new text or one being edited.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct TextDraft {
    /// The text it edits (`None`: a new one).
    pub index: Option<usize>,
    /// Its point (the sketch's coordinates).
    pub at: [f64; 2],
    /// The text box: text and field tokens.
    pub text: String,
    pub height: f64,
    pub anchor: TextAnchor,
    /// Rotation in degrees.
    pub angle: f64,
    /// The title of a prompted entry to insert.
    pub prompt: String,
    /// The text box asked for focus.
    focused: bool,
}

impl TextDraft {
    /// A new text at `at`.
    pub fn new(at: [f64; 2]) -> Self {
        Self {
            index: None,
            at,
            text: String::new(),
            height: 3.5,
            anchor: TextAnchor::Left,
            angle: 0.0,
            prompt: String::new(),
            focused: false,
        }
    }

    /// Editing text `k`.
    pub fn edit(k: usize, t: &DrawingText) -> Self {
        Self {
            index: Some(k),
            at: t.at,
            text: fields::tokens(&t.pieces),
            height: t.height,
            anchor: t.anchor,
            angle: t.angle.to_degrees(),
            prompt: String::new(),
            focused: false,
        }
    }

    /// The text it makes.
    pub fn text(&self) -> DrawingText {
        DrawingText {
            at: self.at,
            height: self.height,
            anchor: self.anchor,
            angle: self.angle.to_radians(),
            pieces: fields::parse(&self.text),
        }
    }
}

/// Text `t` as the editor shows it: its tokens (a definition's fields by
/// name), at sheet point `at`.
fn shown(t: &DrawingText, pl: &Placement) -> Option<Prim> {
    let s = fields::tokens(&t.pieces);
    crate::drawing::render::text_prim(&s, t, pl.to_sheet(t.at))
}

/// The corners of text `t`'s box on the sheet.
fn text_box(t: &DrawingText, pl: &Placement) -> Option<[[f64; 2]; 4]> {
    let Some(Prim::Text {
        at,
        height,
        text,
        anchor,
        angle,
    }) = shown(t, pl)
    else {
        return None;
    };
    let w = crate::drawing::rich::text_advance(&text, height);
    let x0 = match anchor {
        crate::drawing::sheet::Anchor::Left => 0.0,
        crate::drawing::sheet::Anchor::Center => -w * 0.5,
    };
    let (s, c) = angle.sin_cos();
    let p = |u: f64, v: f64| [at[0] + u * c - v * s, at[1] + u * s + v * c];
    let pad = height * 0.25;
    Some([
        p(x0 - pad, -pad),
        p(x0 + w + pad, -pad),
        p(x0 + w + pad, height + pad),
        p(x0 - pad, height + pad),
    ])
}

/// Whether sheet point `q` lies in the convex quadrilateral `b`.
fn inside(b: &[[f64; 2]; 4], q: [f64; 2]) -> bool {
    let mut sign = 0.0;
    for k in 0..4 {
        let (a, n) = (b[k], b[(k + 1) % 4]);
        let cross = (n[0] - a[0]) * (q[1] - a[1]) - (n[1] - a[1]) * (q[0] - a[0]);
        if cross.abs() < 1e-12 {
            continue;
        }
        if sign == 0.0 {
            sign = cross.signum();
        } else if cross.signum() != sign {
            return false;
        }
    }
    true
}

/// The Insert Field menu: the drawing's Properties, the model's, the
/// sheet's values.
fn field_menu(ui: &mut egui::Ui, add: &mut Option<Field>) {
    let groups: [(&str, Vec<Field>); 3] = [
        (
            "Drawing Properties",
            Field::offered()
                .into_iter()
                .filter(|f| matches!(f, Field::Drawing(_)))
                .collect(),
        ),
        (
            "Model Properties",
            Field::offered()
                .into_iter()
                .filter(|f| matches!(f, Field::Model(_) | Field::Material))
                .collect(),
        ),
        (
            "Sheet Properties",
            Field::offered()
                .into_iter()
                .filter(|f| matches!(f, Field::Sheet(_)))
                .collect(),
        ),
    ];
    for (name, list) in groups {
        ui.menu_button(name, |ui| {
            egui::ScrollArea::vertical()
                .max_height(420.0)
                .show(ui, |ui| {
                    for f in list {
                        let label = f.name();
                        let label = label.strip_prefix("Model: ").unwrap_or(&label).to_string();
                        if ui.button(label).clicked() {
                            *add = Some(f);
                            ui.close();
                        }
                    }
                });
        });
    }
}

impl DrwSketch {
    /// The text under sheet point `q` (the last drawn wins).
    pub(super) fn text_at(&self, pl: &Placement, q: [f64; 2]) -> Option<usize> {
        (0..self.texts.len())
            .rev()
            .find(|&k| text_box(&self.texts[k], pl).is_some_and(|b| inside(&b, q)))
    }

    /// Paints the sketch's texts (the selected one highlighted, the one in
    /// the dialog as it will be).
    pub(super) fn paint_texts(
        &self,
        painter: &egui::Painter,
        cam: &SheetCam,
        rect: Rect,
        pl: &Placement,
    ) {
        let color = Color32::from_rgb(20, 90, 200);
        let sel = super::theme::canvas().hover_edge;
        for (k, t) in self.texts.iter().enumerate() {
            if self.text_edit.as_ref().is_some_and(|e| e.index == Some(k)) {
                continue;
            }
            let c = if self.text_sel == Some(k) { sel } else { color };
            if let Some(p) = shown(t, pl) {
                super::drawing_annot::paint_prims(painter, cam, rect, &[p], c);
            }
            if self.text_sel == Some(k)
                && let Some(b) = text_box(t, pl)
            {
                let pts: Vec<egui::Pos2> = b.iter().map(|q| cam.to_screen(rect, *q)).collect();
                painter.add(egui::Shape::closed_line(pts, egui::Stroke::new(1.0, sel)));
            }
        }
        if let Some(e) = &self.text_edit {
            let t = e.text();
            if let Some(p) = shown(&t, pl) {
                super::drawing_annot::paint_prims(painter, cam, rect, &[p], sel);
            }
            let at = cam.to_screen(rect, pl.to_sheet(e.at));
            painter.circle_stroke(at, 3.0, egui::Stroke::new(1.0, sel));
        }
    }

    /// Delete of the selected text; true when one went.
    pub(super) fn delete_text(&mut self) -> bool {
        match self.text_sel.take() {
            Some(k) if k < self.texts.len() => {
                self.texts.remove(k);
                true
            }
            _ => false,
        }
    }
}

impl CadCraftApp {
    /// The Text dialog of the open drawing sketch.
    pub(super) fn drawing_text_window(&mut self, ctx: &egui::Context) {
        let Some(mut e) = self.drw.sketch.as_ref().and_then(|s| s.text_edit.clone()) else {
            return;
        };
        let mut open = true;
        let (mut ok, mut cancel) = (false, false);
        let title = if e.index.is_some() {
            "Edit Text"
        } else {
            "Text"
        };
        // Beside the sheet's top right, clear of the sketch's toolbar.
        let c = ctx.content_rect();
        let win = egui::Window::new(title)
            .id(egui::Id::new("drawing_sketch_text"))
            .default_pos([c.right() - 720.0, c.top() + 260.0])
            .open(&mut open)
            .resizable(false)
            .collapsible(false);
        self.layout
            .place("drawing_text", ctx.content_rect(), false, win)
            .show(ctx, |ui| {
                let r = ui.add(
                    egui::TextEdit::singleline(&mut e.text)
                        .desired_width(320.0)
                        .hint_text("Text, fields as <Designer>, <Model: Part Number>, <?Prompt>"),
                );
                if !e.focused {
                    r.request_focus();
                    e.focused = true;
                }
                ui.horizontal(|ui| {
                    let mut add = None;
                    ui.menu_button("Insert Field", |ui| {
                        field_menu(ui, &mut add);
                    })
                    .response
                    .on_hover_text(
                        "A Property of the drawing, of the model of the first view, or a value of the sheet",
                    );
                    if let Some(f) = add {
                        e.text.push('<');
                        e.text.push_str(&f.name());
                        e.text.push('>');
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Prompted Entry");
                    ui.add(egui::TextEdit::singleline(&mut e.prompt).desired_width(150.0));
                    let can = !e.prompt.trim().is_empty() && !e.prompt.contains('>');
                    if ui
                        .add_enabled(can, egui::Button::new("Insert"))
                        .on_hover_text("A value asked when the title block or symbol is placed")
                        .clicked()
                    {
                        e.text.push_str("<?");
                        e.text.push_str(e.prompt.trim());
                        e.text.push('>');
                        e.prompt.clear();
                    }
                });
                egui::Grid::new("drawing_text_grid")
                    .num_columns(2)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        ui.label("Height");
                        ui.add(ExprValue::new(&mut e.height).range(0.5..=100.0).suffix(" mm"));
                        ui.end_row();
                        ui.label("Justification");
                        ui.horizontal(|ui| {
                            for a in TextAnchor::ALL {
                                ui.radio_value(&mut e.anchor, a, a.label());
                            }
                        });
                        ui.end_row();
                        ui.label("Rotation");
                        ui.add(ExprValue::new(&mut e.angle).range(-360.0..=360.0).suffix("°"));
                        ui.end_row();
                    });
                let preview = fields::tokens(&fields::parse(&e.text));
                if preview != e.text {
                    ui.weak("Some <…> are no field and stay text");
                }
                ui.separator();
                ui.horizontal(|ui| {
                    let can = !e.text.trim().is_empty();
                    if ui.add_enabled(can, super::theme::ok_button("OK")).clicked()
                        || (can && super::keys::panel_ok_key(ui, false))
                    {
                        ok = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        let Some(s) = self.drw.sketch.as_mut() else {
            return;
        };
        if cancel || !open {
            s.text_edit = None;
            return;
        }
        if ok {
            let t = e.text();
            match e.index {
                Some(k) if k < s.texts.len() => s.texts[k] = t,
                _ => s.texts.push(t),
            }
            s.text_edit = None;
            return;
        }
        s.text_edit = Some(e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_draft_makes_its_text_and_reads_it_back() {
        let mut d = TextDraft::new([10.0, 20.0]);
        d.text = "Part <Model: Part Number> <?Order>".into();
        d.angle = 90.0;
        d.anchor = TextAnchor::Right;
        let t = d.text();
        assert_eq!(t.pieces.len(), 4);
        assert!((t.angle - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        let back = TextDraft::edit(3, &t);
        assert_eq!(back.text, d.text);
        assert_eq!(back.index, Some(3));
        assert!((back.angle - 90.0).abs() < 1e-9);
    }

    #[test]
    fn a_click_finds_the_text_under_it() {
        let pl = Placement {
            position: [0.0, 0.0],
            scale: 1.0,
            center: [0.0, 0.0],
            breaks: Default::default(),
        };
        let t = TextDraft {
            text: "ABCDEF".into(),
            ..TextDraft::new([10.0, 10.0])
        }
        .text();
        let b = text_box(&t, &pl).unwrap_or_default();
        assert!(inside(&b, [12.0, 11.5]));
        assert!(!inside(&b, [12.0, 20.0]));
        assert!(!inside(&b, [5.0, 11.0]));
    }
}
