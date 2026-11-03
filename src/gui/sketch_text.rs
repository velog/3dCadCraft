//! The Text command of a sketch (`docs/spec/1.11-sketch-text.md`): click a place, then the panel
//! with the text, font, face and sizes; the outlines show in the view while they change. A text
//! is edited by a double click or the view's menu. GUI-free: `sketch::text`, `sketch::fonts`.

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Stroke};

use super::CadCraftApp;
use super::icons::Icon;
use super::panels::{plabel, psection};
use super::valuefield::ExprValue;
use crate::kernel::Real;
use crate::sketch::fonts::{self, LoadedFont};
use crate::sketch::text::{Glyph, TextAlign, TextSpec, layout};
use crate::sketch::{EntityId, Point2, Vector2};

/// The Text panel's state.
pub(super) struct TextDraft {
    /// Where its top left goes.
    pub at: Point2,
    /// The text being edited (`None`: a new one).
    pub edit: Option<EntityId>,
    /// The options.
    pub spec: TextSpec,
    /// Rotation (degrees).
    pub angle: Real,
    /// The installed families.
    families: Vec<String>,
    /// What the font list is narrowed to.
    filter: String,
    /// The outlines of `shown` (the preview).
    glyphs: Vec<Glyph>,
    /// The options the preview was made from.
    shown: Option<TextSpec>,
    /// Why there is no preview.
    error: Option<String>,
    /// The text box asks for the focus once.
    focused: bool,
}

/// The font a new text starts with: Inventor's default, else the first there is.
fn default_font(families: &[String]) -> String {
    ["Tahoma", "Arial", "DejaVu Sans", "Liberation Sans"]
        .iter()
        .find(|f| families.iter().any(|x| x == *f))
        .map(|f| (*f).to_owned())
        .or_else(|| families.first().cloned())
        .unwrap_or_default()
}

impl TextDraft {
    fn new(at: Point2, last: Option<&TextSpec>) -> Self {
        let families = fonts::families();
        let spec = match last {
            Some(s) => TextSpec {
                text: String::new(),
                ..s.clone()
            },
            None => TextSpec::new("", default_font(&families), 3.5),
        };
        Self {
            at,
            edit: None,
            spec,
            angle: 0.0,
            families,
            filter: String::new(),
            glyphs: Vec::new(),
            shown: None,
            error: None,
            focused: false,
        }
    }

    /// Lays the text out again when an option changed.
    fn refresh(&mut self) {
        if self.shown.as_ref() == Some(&self.spec) {
            return;
        }
        self.shown = Some(self.spec.clone());
        let r = LoadedFont::find(&self.spec.font, self.spec.bold, self.spec.italic)
            .and_then(|f| layout(&self.spec, &f));
        match r {
            Ok((g, ..)) => {
                self.glyphs = g;
                self.error = None;
            }
            Err(e) => {
                self.glyphs.clear();
                self.error = Some(e);
            }
        }
    }
}

impl CadCraftApp {
    /// The Text tool's click: the panel opens for that place (another click moves it).
    pub(super) fn text_take_click(&mut self) {
        let Some(at) = self.tools.text_click.take() else {
            return;
        };
        match &mut self.text_draft {
            Some(d) => d.at = at,
            None => self.text_draft = Some(TextDraft::new(at, self.last_text.as_ref())),
        }
    }

    /// Opens the panel on the text standing on `line`.
    pub(super) fn text_edit(&mut self, line: EntityId) {
        let Some(i) = self.editing else {
            return;
        };
        let sk = &self.part().sketches[i].sketch;
        let Some(t) = sk.texts().iter().find(|t| t.line == line) else {
            return;
        };
        let Some((at, dir)) = sk.text_frame(t) else {
            return;
        };
        let mut d = TextDraft::new(at, None);
        d.spec = t.spec.clone();
        d.edit = Some(line);
        d.angle = dir.y.atan2(dir.x).to_degrees();
        // Installed or not: its name stays in the list (the outlines stay until a new font).
        if !d.families.contains(&d.spec.font) {
            d.families.insert(0, d.spec.font.clone());
        }
        self.text_draft = Some(d);
    }

    /// The text under the view's point `p`, in the edited sketch.
    pub(super) fn text_under(&self, p: Pos2, rect: Rect) -> Option<EntityId> {
        let i = self.editing?;
        let sk = &self.part().sketches.get(i)?.sketch;
        if sk.texts().is_empty() {
            return None;
        }
        let w = self.camera.ray_plane(p, rect, &sk.plane)?;
        sk.text_at(sk.plane.project(&w), 0.0)
    }

    fn text_ok(&mut self) {
        let Some(d) = self.text_draft.as_ref() else {
            return;
        };
        let (at, edit, spec, angle) = (d.at, d.edit, d.spec.clone(), d.angle.to_radians());
        let Some(i) = self.editing else {
            return;
        };
        let font = match LoadedFont::find(&spec.font, spec.bold, spec.italic) {
            Ok(f) => f,
            Err(e) => {
                self.file_status = Some(Err(format!("Text: {e}")));
                return;
            }
        };
        self.checkpoint(if edit.is_some() { "Edit Text" } else { "Text" });
        let sk = &mut self.part_mut().sketches[i].sketch;
        let r = match edit {
            Some(line) => sk.set_text(line, spec.clone(), &font).and_then(|()| {
                let turned = sk.entity(line).and_then(|e| match e {
                    crate::sketch::Entity::Line { start, end } => {
                        let a = sk.point(*start)?.position;
                        let len = (sk.point(*end)?.position - a).norm();
                        Some((*end, a + Vector2::new(angle.cos(), angle.sin()) * len))
                    }
                    _ => None,
                });
                match turned {
                    Some((end, q)) => sk.move_point(end, q),
                    None => Ok(()),
                }
            }),
            None => sk.add_text(at, angle, spec.clone(), &font).map(|_| ()),
        };
        match r {
            Ok(()) => {
                self.last_text = Some(spec);
                self.text_draft = None;
                self.tools.set_tool(super::Tool::Select);
                self.file_status = None;
                self.solve();
            }
            Err(e) => {
                self.history.cancel();
                self.file_status = Some(Err(format!("Text: {e}")));
            }
        }
    }

    /// The panel. Returns nothing: OK and Cancel close it.
    pub(super) fn text_panel(&mut self, ui: &mut egui::Ui) {
        let title = if self.text_draft.as_ref().is_some_and(|d| d.edit.is_some()) {
            "Edit Text"
        } else {
            "Text"
        };
        self.panel_header(ui, Icon::Text, title);
        let (mut ok, mut cancel) = (false, false);
        let Some(d) = self.text_draft.as_mut() else {
            return;
        };
        let r = ui.add(
            egui::TextEdit::multiline(&mut d.spec.text)
                .desired_rows(3)
                .desired_width(f32::INFINITY)
                .hint_text("Type the text"),
        );
        if !d.focused {
            r.request_focus();
            d.focused = true;
        }
        psection(ui, "text_font", "Font", |ui| {
            plabel(ui, "Font");
            let list: Vec<String> = {
                let needle = d.filter.to_lowercase();
                d.families
                    .iter()
                    .filter(|f| f.to_lowercase().contains(&needle))
                    .cloned()
                    .collect()
            };
            egui::ComboBox::from_id_salt("text_font_list")
                .selected_text(d.spec.font.clone())
                .height(320.0)
                .show_ui(ui, |ui| {
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut d.filter)
                            .hint_text("Filter")
                            .desired_width(180.0),
                    );
                    // Typing goes to the filter, not to the app's shortcuts.
                    if ui.memory(|m| m.focused().is_none()) {
                        r.request_focus();
                    }
                    for f in &list {
                        ui.selectable_value(&mut d.spec.font, f.clone(), f.as_str());
                    }
                });
            ui.end_row();
            plabel(ui, "Style");
            ui.horizontal(|ui| {
                ui.checkbox(&mut d.spec.bold, "Bold");
                ui.checkbox(&mut d.spec.italic, "Italic");
            });
            ui.end_row();
            plabel(ui, "Size");
            ui.add(ExprValue::new(&mut d.spec.size).suffix(" mm"))
                .on_hover_text("Height of the capital letters");
            ui.end_row();
        });
        psection(ui, "text_layout", "Layout", |ui| {
            plabel(ui, "Alignment");
            ui.horizontal(|ui| {
                for (a, t) in [
                    (TextAlign::Left, "Left"),
                    (TextAlign::Center, "Center"),
                    (TextAlign::Right, "Right"),
                ] {
                    ui.selectable_value(&mut d.spec.align, a, t);
                }
            });
            ui.end_row();
            plabel(ui, "Stretch");
            let mut pct = d.spec.stretch * 100.0;
            if ui
                .add(ExprValue::new(&mut pct).suffix(" %"))
                .on_hover_text("100 % shows the letters as designed")
                .changed()
            {
                d.spec.stretch = pct / 100.0;
            }
            ui.end_row();
            plabel(ui, "Line spacing");
            ui.add(ExprValue::new(&mut d.spec.spacing).suffix(" x"));
            ui.end_row();
            plabel(ui, "Rotation");
            ui.add(ExprValue::new(&mut d.angle).suffix(" deg"));
            ui.end_row();
        });
        d.refresh();
        if let Some(e) = &d.error
            && !d.spec.text.trim().is_empty()
        {
            ui.label(RichText::new(format!("Warning: {e}")).color(Color32::from_rgb(230, 140, 0)));
        }
        let ready = !d.glyphs.is_empty() && d.error.is_none();
        ui.horizontal(|ui| {
            ok = ui.add_enabled(ready, egui::Button::new("OK")).clicked()
                || super::keys::panel_ok_key(ui, ready);
            cancel = ui.button("Cancel").clicked();
        });
        if ok {
            self.text_ok();
        } else if cancel {
            self.text_draft = None;
            self.tools.set_tool(super::Tool::Select);
        }
    }

    /// The draft's outlines in the view.
    pub(super) fn text_preview(&self, painter: &egui::Painter, rect: Rect) {
        let (Some(d), Some(i)) = (self.text_draft.as_ref(), self.editing) else {
            return;
        };
        let Some(ps) = self.part().sketches.get(i) else {
            return;
        };
        let t = crate::sketch::text::SketchText {
            line: d.edit.unwrap_or_default(),
            spec: d.spec.clone(),
            glyphs: d.glyphs.clone(),
            width: 0.0,
            height: 0.0,
        };
        let a = d.angle.to_radians();
        let stroke = Stroke::new(1.5, Color32::from_rgb(235, 130, 40));
        for line in t.polylines_at(d.at, Vector2::new(a.cos(), a.sin())) {
            let pts: Vec<Pos2> = line
                .iter()
                .filter_map(|p| {
                    let w = ps.sketch.plane.point_at(p.x, p.y);
                    self.camera.project(&w, rect)
                })
                .collect();
            painter.add(egui::Shape::line(pts, stroke));
        }
    }
}
