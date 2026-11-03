//! Application Options › Colors and the Color Scheme editor (spec 0.18).

use super::scheme::{
    CanvasColors, CanvasScheme, DEFAULT_SCHEME, ELEMENTS, builtin_schemes, canvas, from_hex,
    load_schemes, remove_scheme, save_scheme, schemes_dir, set_canvas, to_hex, unique_name,
};
use super::{Tokens, UiTheme, current, set_current};
use eframe::egui::{self, Color32, CornerRadius, Rect, RichText, Sense, Stroke, Vec2};
use serde_json::{Value, json};
use std::sync::mpsc::{Receiver, channel};

/// A picture's longest side in memory: a photo's full size only costs memory here.
const MOST_PICTURE_SIDE: u32 = 4096;

/// Which picture of the view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageSlot {
    Background,
    Cap,
}

/// Decoded pixels: sRGB, straight alpha.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Reads a PNG, JPEG or BMP file.
pub fn read_picture(path: &str) -> Result<Picture, String> {
    let img = image::ImageReader::open(path)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| e.to_string())?;
    let img = if img.width().max(img.height()) > MOST_PICTURE_SIDE {
        img.thumbnail(MOST_PICTURE_SIDE, MOST_PICTURE_SIDE)
    } else {
        img
    };
    let rgba = img.to_rgba8();
    Ok(Picture {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    })
}

/// A picture's setting: in use, and its file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PictureSetting {
    pub on: bool,
    pub path: String,
}

/// A file being read on a worker thread, with the setting to go back to.
#[derive(Debug)]
struct PictureJob {
    slot: ImageSlot,
    before: PictureSetting,
    answer: Receiver<Result<Picture, String>>,
}

/// What the tab asks of the application.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TabOut {
    /// The view's colours changed: rebuild the scene.
    pub scene: bool,
    pub export: bool,
    pub import: bool,
    /// Asks for a picture's file.
    pub pick: Option<ImageSlot>,
}

/// The state of the In-canvas schemes.
#[derive(Debug)]
pub struct ColorsUi {
    /// The user's schemes.
    pub custom: Vec<CanvasScheme>,
    /// The scheme in use.
    pub active: String,
    /// Background: gradient (else 1 Color).
    pub gradient: bool,
    /// Background Image.
    pub background: PictureSetting,
    /// Section Capping Plane Texture: Bitmap Image.
    pub cap: PictureSetting,
    /// Enable Enhanced Highlighting, Highlight Hidden Faces.
    pub enhanced: bool,
    pub hidden_faces: bool,
    /// Preview Image Background: a fixed colour for saved previews.
    pub preview_fixed: bool,
    pub preview_color: Color32,
    /// The preview box shows the drawing environment.
    drafting: bool,
    jobs: Vec<PictureJob>,
    /// The file each slot's picture last came from.
    read: [Option<String>; 2],
    /// The editor window is open.
    pub editor: bool,
    /// A scheme's new name being typed.
    rename: Option<String>,
    /// The last file problem, shown in the editor.
    pub message: Option<String>,
    /// A scheme changed and is not on disk yet.
    unsaved: Option<String>,
}

impl Default for ColorsUi {
    fn default() -> Self {
        Self {
            custom: Vec::new(),
            active: DEFAULT_SCHEME.to_owned(),
            gradient: true,
            background: PictureSetting::default(),
            cap: PictureSetting::default(),
            enhanced: false,
            hidden_faces: false,
            preview_fixed: false,
            preview_color: Color32::WHITE,
            drafting: false,
            jobs: Vec::new(),
            read: [None, None],
            editor: false,
            rename: None,
            message: None,
            unsaved: None,
        }
    }
}

impl ColorsUi {
    /// Built-in schemes, then the user's.
    pub fn all(&self) -> Vec<CanvasScheme> {
        let mut v = builtin_schemes();
        v.extend(self.custom.iter().cloned());
        v
    }

    fn names(&self) -> Vec<String> {
        self.all().into_iter().map(|s| s.name).collect()
    }

    /// The colours of the active scheme; the default when it no longer exists.
    pub fn colors(&self) -> CanvasColors {
        let mut c = self
            .all()
            .into_iter()
            .find(|s| s.name == self.active)
            .map_or_else(CanvasColors::light, |s| s.colors);
        c.gradient = self.gradient;
        c
    }

    /// Reads the user's schemes and puts the chosen one in use.
    pub fn load(&mut self, active: &str, gradient: bool) {
        if let Some(dir) = schemes_dir() {
            self.custom = load_schemes(&dir);
        }
        self.gradient = gradient;
        self.active = if self.all().iter().any(|s| s.name == active) {
            active.to_owned()
        } else {
            DEFAULT_SCHEME.to_owned()
        };
        set_canvas(self.colors());
    }

    fn setting(&mut self, slot: ImageSlot) -> &mut PictureSetting {
        match slot {
            ImageSlot::Background => &mut self.background,
            ImageSlot::Cap => &mut self.cap,
        }
    }

    /// The saved setting at start: read the file only when it is in use.
    pub fn load_picture(&mut self, slot: ImageSlot, on: bool, path: &str) {
        let before = self.setting(slot).clone();
        *self.setting(slot) = PictureSetting {
            on,
            path: path.to_owned(),
        };
        if on && !path.is_empty() {
            self.start_reading(slot, before);
        }
    }

    /// The file chosen in the dialog: in use once it has been read (rule 5: a file
    /// that cannot be read leaves the setting as it was).
    pub fn choose_picture(&mut self, slot: ImageSlot, path: &str) {
        let before = self.setting(slot).clone();
        *self.setting(slot) = PictureSetting {
            on: true,
            path: path.to_owned(),
        };
        self.start_reading(slot, before);
    }

    fn start_reading(&mut self, slot: ImageSlot, before: PictureSetting) {
        let path = self.setting(slot).path.clone();
        if self.read[slot as usize].as_deref() == Some(path.as_str()) {
            return;
        }
        let (tell, answer) = channel();
        std::thread::spawn(move || {
            let _ = tell.send(read_picture(&path));
        });
        self.jobs.push(PictureJob {
            slot,
            before,
            answer,
        });
    }

    /// A worker is still reading: keep the frames coming.
    pub fn pictures_pending(&self) -> bool {
        !self.jobs.is_empty()
    }

    /// The pictures read since the last call. A file that failed puts back what
    /// was set before and says why.
    pub fn finished_pictures(&mut self) -> Vec<(ImageSlot, Picture)> {
        let mut out = Vec::new();
        let mut jobs = std::mem::take(&mut self.jobs);
        jobs.retain_mut(|job| {
            let Ok(answer) = job.answer.try_recv() else {
                return true;
            };
            match answer {
                Ok(p) => {
                    self.read[job.slot as usize] = Some(self.setting(job.slot).path.clone());
                    out.push((job.slot, p));
                }
                Err(e) => {
                    let s = self.setting(job.slot);
                    let message = format!("Picture: {}: {e}", s.path);
                    log::warn!("{message}");
                    *s = job.before.clone();
                    self.message = Some(message);
                }
            }
            false
        });
        self.jobs = jobs;
        out
    }

    /// Preview Image Background as the options store it.
    pub fn preview_text(&self) -> String {
        if self.preview_fixed {
            to_hex(self.preview_color)
        } else {
            String::new()
        }
    }

    pub fn set_preview_text(&mut self, text: &str) {
        let color = from_hex(text);
        self.preview_fixed = color.is_some();
        self.preview_color = color.unwrap_or(Color32::WHITE);
    }

    /// The colour behind a saved preview; `None`: transparent.
    pub fn preview_back(&self) -> Option<[u8; 3]> {
        let c = self.preview_color;
        self.preview_fixed.then(|| [c.r(), c.g(), c.b()])
    }

    /// Puts a scheme in use.
    pub fn select(&mut self, name: &str) {
        name.clone_into(&mut self.active);
        set_canvas(self.colors());
    }

    /// New: a copy of the active scheme, made active.
    pub fn new_scheme(&mut self) -> String {
        let name = unique_name(&format!("{} copy", self.active), &self.names());
        let colors = self.colors();
        self.custom.push(CanvasScheme {
            name: name.clone(),
            builtin: false,
            colors,
        });
        self.custom.sort_by(|a, b| a.name.cmp(&b.name));
        self.active.clone_from(&name);
        self.write(&name);
        name
    }

    /// Remove: only the user's; the default scheme takes over when it was in use.
    pub fn remove(&mut self, name: &str) -> bool {
        let Some(k) = self.custom.iter().position(|s| s.name == name) else {
            return false;
        };
        self.custom.remove(k);
        if let Some(dir) = schemes_dir()
            && let Err(e) = remove_scheme(&dir, name)
        {
            self.message = Some(format!("Remove: {e}"));
        }
        if self.active == name {
            DEFAULT_SCHEME.clone_into(&mut self.active);
        }
        set_canvas(self.colors());
        true
    }

    /// Rename: only the user's; the name must be free and not empty.
    pub fn rename(&mut self, old: &str, new: &str) -> Result<(), String> {
        let new = new.trim();
        if new.is_empty() {
            return Err("Rename: type a name".to_owned());
        }
        if self
            .names()
            .iter()
            .any(|n| n.eq_ignore_ascii_case(new) && n != old)
        {
            return Err(format!("Rename: '{new}' already exists"));
        }
        let Some(s) = self.custom.iter_mut().find(|s| s.name == old) else {
            return Err("Rename: a built-in scheme keeps its name".to_owned());
        };
        new.clone_into(&mut s.name);
        if let Some(dir) = schemes_dir() {
            let _ = remove_scheme(&dir, old);
        }
        if self.active == old {
            new.clone_into(&mut self.active);
        }
        self.custom.sort_by(|a, b| a.name.cmp(&b.name));
        self.write(new);
        Ok(())
    }

    fn write(&mut self, name: &str) {
        let Some(dir) = schemes_dir() else {
            return;
        };
        if let Some(s) = self.custom.iter().find(|s| s.name == name)
            && let Err(e) = save_scheme(&dir, s)
        {
            self.message = Some(format!("Save scheme: {e}"));
        }
    }

    /// What Export writes besides the options.
    pub fn export_json(&self) -> Value {
        Value::Array(self.custom.iter().map(CanvasScheme::to_json).collect())
    }

    /// Takes the schemes of an imported file: a name in use is replaced. The number taken.
    pub fn import_json(&mut self, list: &Value) -> usize {
        let mut n = 0;
        for v in list.as_array().into_iter().flatten() {
            let Some((scheme, _)) = CanvasScheme::from_json(v) else {
                continue;
            };
            // A built-in name stays the built-in's.
            if builtin_schemes().iter().any(|b| b.name == scheme.name) {
                continue;
            }
            let name = scheme.name.clone();
            self.custom.retain(|s| s.name != name);
            self.custom.push(scheme);
            self.write(&name);
            n += 1;
        }
        self.custom.sort_by(|a, b| a.name.cmp(&b.name));
        n
    }
}

/// An exported settings file.
pub fn export_file(options: Value, schemes: Value) -> Value {
    json!({ "options": options, "color_schemes": schemes })
}

/// A drawing sheet in small: paper in any theme (spec 0.18 §5).
fn preview_sheet(p: &egui::Painter, view: Rect) {
    p.rect_filled(view, CornerRadius::ZERO, Color32::from_gray(150));
    let sheet = view.shrink(12.0);
    p.rect_filled(sheet, CornerRadius::ZERO, Color32::WHITE);
    let pen = Stroke::new(1.0, Color32::BLACK);
    p.rect_stroke(
        sheet.shrink(5.0),
        CornerRadius::ZERO,
        pen,
        egui::StrokeKind::Inside,
    );
    let block = Rect::from_min_max(
        sheet.right_bottom() - Vec2::new(60.0, 24.0),
        sheet.right_bottom() - Vec2::new(5.0, 5.0),
    );
    p.rect_stroke(block, CornerRadius::ZERO, pen, egui::StrokeKind::Inside);
    let part = Rect::from_center_size(sheet.center() - Vec2::new(18.0, 6.0), Vec2::new(70.0, 34.0));
    p.rect_stroke(part, CornerRadius::ZERO, pen, egui::StrokeKind::Inside);
}

/// The file's name after the row of a picture, or the Browse button's hint.
fn picture_row(ui: &mut egui::Ui, setting: &PictureSetting, slot: ImageSlot, out: &mut TabOut) {
    ui.horizontal(|ui| {
        if ui
            .button("Browse...")
            .on_hover_text("A PNG, JPEG or BMP file")
            .clicked()
        {
            out.pick = Some(slot);
        }
        let name = std::path::Path::new(&setting.path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("No file chosen");
        ui.label(RichText::new(name).weak())
            .on_hover_text(&setting.path);
    });
}

/// The frame of a theme and the view of a scheme in small.
fn preview(ui: &mut egui::Ui, k: &Tokens, c: &CanvasColors, drafting: bool, picture: bool) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(300.0, 170.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(4), k.panel);
    let band = |top: f32, h: f32| {
        Rect::from_min_size(rect.min + Vec2::new(0.0, top), Vec2::new(rect.width(), h))
    };
    p.hline(rect.x_range(), rect.top() + 34.0, Stroke::new(1.0, k.line));
    let tab = Rect::from_min_size(rect.min + Vec2::new(8.0, 8.0), Vec2::new(44.0, 16.0));
    p.rect_filled(tab, CornerRadius::same(3), k.accent);
    for (i, w) in [36.0_f32, 30.0, 40.0].iter().enumerate() {
        let x = 62.0 + i as f32 * 46.0;
        let r = Rect::from_min_size(rect.min + Vec2::new(x, 13.0), Vec2::new(*w, 5.0));
        p.rect_filled(r, CornerRadius::same(1), k.text_weak);
    }
    let body = band(35.0, rect.height() - 53.0);
    let browser = Rect::from_min_max(body.min, egui::pos2(body.left() + 62.0, body.bottom()));
    for i in 0..4 {
        let r = Rect::from_min_size(
            browser.min + Vec2::new(10.0, 10.0 + i as f32 * 14.0),
            Vec2::new(40.0, 5.0),
        );
        p.rect_filled(r, CornerRadius::same(1), k.text_weak);
    }
    let view = Rect::from_min_max(egui::pos2(browser.right(), body.top()), body.max);
    // The gradient in bands, the way the renderer shades it.
    let bottom = if c.gradient {
        c.background_bottom
    } else {
        c.background_top
    };
    const BANDS: usize = 12;
    for i in 0..BANDS {
        let t = i as f32 / BANDS as f32;
        let r = Rect::from_min_size(
            view.min + Vec2::new(0.0, t * view.height()),
            Vec2::new(view.width(), view.height() / BANDS as f32 + 1.0),
        );
        p.rect_filled(
            r,
            CornerRadius::ZERO,
            c.background_top.lerp_to_gamma(bottom, t),
        );
    }
    if drafting {
        preview_sheet(p, view);
        return;
    }
    if picture {
        p.text(
            view.left_top() + Vec2::new(6.0, 4.0),
            egui::Align2::LEFT_TOP,
            "Background Image",
            egui::FontId::proportional(10.0),
            c.work_label,
        );
    }
    let part = Rect::from_center_size(view.center(), Vec2::new(90.0, 46.0));
    p.rect_filled(
        part,
        CornerRadius::same(3),
        Color32::from_rgb(0x8f, 0xa6, 0xc4),
    );
    p.rect_stroke(
        part,
        CornerRadius::same(3),
        Stroke::new(1.0, c.model_edges),
        egui::StrokeKind::Inside,
    );
    let sketch = part.expand(9.0);
    p.rect_stroke(
        sketch,
        CornerRadius::ZERO,
        Stroke::new(1.5, c.sketch_open),
        egui::StrokeKind::Inside,
    );
    p.hline(
        rect.x_range(),
        rect.bottom() - 18.0,
        Stroke::new(1.0, k.line),
    );
    let status = Rect::from_min_size(
        rect.left_bottom() + Vec2::new(8.0, -12.0),
        Vec2::new(120.0, 5.0),
    );
    p.rect_filled(status, CornerRadius::same(1), k.text_weak);
}

/// The tab's content.
pub fn colors_tab(ui: &mut egui::Ui, s: &mut ColorsUi, prehighlight: &mut bool) -> TabOut {
    let mut out = TabOut::default();
    ui.horizontal(|ui| {
        ui.label("Show:");
        ui.radio_value(&mut s.drafting, false, "Design")
            .on_hover_text("The preview shows the model's view");
        ui.radio_value(&mut s.drafting, true, "Drafting")
            .on_hover_text("The preview shows a drawing: sheets are paper in every scheme");
    });
    let mut theme = current();
    preview(ui, &theme.tokens(), &canvas(), s.drafting, s.background.on);
    ui.add_space(6.0);
    ui.label(RichText::new("UI Theme").strong());
    ui.horizontal(|ui| {
        for t in UiTheme::ALL {
            ui.radio_value(&mut theme, t, t.label()).on_hover_text(
                "The colours of the frame around the view: ribbon, browser, panels, dialogs, icons",
            );
        }
    });
    if theme != current() {
        set_current(theme);
    }
    ui.add_space(6.0);
    ui.label(RichText::new("Highlight").strong());
    ui.checkbox(prehighlight, "Enable Prehighlight")
        .on_hover_text("Light what is under the cursor before you click it");
    out.scene |= ui
        .checkbox(&mut s.enhanced, "Enable Enhanced Highlighting")
        .on_hover_text("In an assembly: selected and prehighlighted components show through others")
        .changed();
    out.scene |= ui
        .checkbox(&mut s.hidden_faces, "Highlight Hidden Faces")
        .on_hover_text("A selected face behind others shows through them, with a thin outline")
        .changed();
    ui.add_space(6.0);
    ui.label(RichText::new("In-canvas Color Scheme").strong());
    ui.horizontal(|ui| {
        let mut chosen = None;
        egui::ComboBox::from_id_salt("canvas_scheme")
            .selected_text(s.active.clone())
            .width(190.0)
            .show_ui(ui, |ui| {
                for scheme in s.all() {
                    if ui
                        .selectable_label(s.active == scheme.name, &scheme.name)
                        .clicked()
                    {
                        chosen = Some(scheme.name);
                    }
                }
            });
        if let Some(n) = chosen {
            s.select(&n);
            out.scene = true;
        }
        if ui
            .button("Customize Schemes...")
            .on_hover_text("New, Remove, Rename; the colour of each graphical element")
            .clicked()
        {
            s.editor = true;
        }
    });
    ui.add_space(6.0);
    ui.label(RichText::new("Background").strong());
    ui.horizontal(|ui| {
        if ui
            .radio(!s.background.on && !s.gradient, "1 Color")
            .clicked()
        {
            s.background.on = false;
            s.gradient = false;
            set_canvas(s.colors());
        }
        if ui
            .radio(!s.background.on && s.gradient, "Gradient")
            .clicked()
        {
            s.background.on = false;
            s.gradient = true;
            set_canvas(s.colors());
        }
        if ui
            .radio(s.background.on, "Background Image")
            .on_hover_text(
                "A picture behind the model; it fills the view and keeps its proportions",
            )
            .clicked()
        {
            if s.background.path.is_empty() {
                out.pick = Some(ImageSlot::Background);
            } else {
                let path = s.background.path.clone();
                s.choose_picture(ImageSlot::Background, &path);
            }
        }
    });
    if s.background.on {
        let setting = s.background.clone();
        picture_row(ui, &setting, ImageSlot::Background, &mut out);
    }
    ui.add_space(6.0);
    ui.label(RichText::new("Section Capping Plane Texture").strong());
    ui.horizontal(|ui| {
        if ui.radio(!s.cap.on, "Default - Gray").clicked() {
            s.cap.on = false;
        }
        if ui
            .radio(s.cap.on, "Bitmap Image")
            .on_hover_text("A picture on the section's cut faces, repeated at its own size")
            .clicked()
        {
            if s.cap.path.is_empty() {
                out.pick = Some(ImageSlot::Cap);
            } else {
                let path = s.cap.path.clone();
                s.choose_picture(ImageSlot::Cap, &path);
            }
        }
    });
    if s.cap.on {
        let setting = s.cap.clone();
        picture_row(ui, &setting, ImageSlot::Cap, &mut out);
    }
    if let Some(m) = &s.message {
        ui.colored_label(super::tokens().error, m);
    }
    ui.add_space(6.0);
    ui.label(RichText::new("Preview Image Background").strong());
    ui.horizontal(|ui| {
        ui.checkbox(
            &mut s.preview_fixed,
            "Use a fixed background when saving previews",
        )
        .on_hover_text("Off: the saved preview's background is transparent");
        ui.add_enabled_ui(s.preview_fixed, |ui| {
            egui::color_picker::color_edit_button_srgba(
                ui,
                &mut s.preview_color,
                egui::color_picker::Alpha::Opaque,
            );
        });
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        out.export = ui
            .button("Export...")
            .on_hover_text("The Application Options and your colour schemes, to a file")
            .clicked();
        out.import = ui
            .button("Import...")
            .on_hover_text("From a file; a partial file changes only what it lists")
            .clicked();
    });
    out
}

/// The Color Scheme editor's content: true when the view's colours changed.
pub fn scheme_editor(ui: &mut egui::Ui, s: &mut ColorsUi) -> bool {
    let mut scene = false;
    let active_builtin = builtin_schemes().iter().any(|b| b.name == s.active);
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(190.0);
            ui.label(RichText::new("Color Schemes").strong());
            let mut chosen = None;
            egui::ScrollArea::vertical()
                .id_salt("scheme_list")
                .max_height(260.0)
                .show(ui, |ui| {
                    for scheme in s.all() {
                        if ui
                            .selectable_label(s.active == scheme.name, &scheme.name)
                            .clicked()
                        {
                            chosen = Some(scheme.name);
                        }
                    }
                });
            if let Some(n) = chosen {
                s.select(&n);
                s.rename = None;
                scene = true;
            }
            ui.horizontal(|ui| {
                if ui
                    .button("New")
                    .on_hover_text("A copy of the selected scheme")
                    .clicked()
                {
                    s.new_scheme();
                    scene = true;
                }
                let tip = "Built-in schemes cannot be removed";
                if ui
                    .add_enabled(!active_builtin, egui::Button::new("Remove"))
                    .on_disabled_hover_text(tip)
                    .clicked()
                {
                    let name = s.active.clone();
                    s.remove(&name);
                    scene = true;
                }
                if ui
                    .add_enabled(!active_builtin, egui::Button::new("Rename"))
                    .on_disabled_hover_text("Built-in schemes keep their names")
                    .clicked()
                {
                    s.rename = Some(s.active.clone());
                }
            });
            if let Some(mut text) = s.rename.clone() {
                let r = ui.text_edit_singleline(&mut text);
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                s.rename = Some(text.clone());
                if enter {
                    let old = s.active.clone();
                    match s.rename(&old, &text) {
                        Ok(()) => {
                            s.rename = None;
                            s.message = None;
                        }
                        Err(e) => s.message = Some(e),
                    }
                }
            }
            if let Some(m) = &s.message {
                ui.colored_label(super::tokens().error, m);
            }
        });
        ui.separator();
        ui.vertical(|ui| {
            ui.label(RichText::new("Graphical Elements").strong());
            if active_builtin {
                ui.label(
                    RichText::new("A built-in scheme: press New to change a copy of it.").weak(),
                );
            }
            ui.add_enabled_ui(!active_builtin, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("scheme_elements")
                    .max_height(380.0)
                    .show(ui, |ui| {
                        scene |= element_rows(ui, s);
                    });
            });
        });
    });
    // One file write per gesture, not per frame of a colour drag.
    if !ui.input(|i| i.pointer.any_down())
        && let Some(name) = s.unsaved.take()
    {
        s.write(&name);
    }
    scene
}

/// The elements, by group, of the active (user's) scheme.
fn element_rows(ui: &mut egui::Ui, s: &mut ColorsUi) -> bool {
    let mut changed = false;
    let mut colors = s.colors();
    let mut group = "";
    egui::Grid::new("scheme_element_grid")
        .num_columns(3)
        .spacing([10.0, 4.0])
        .show(ui, |ui| {
            for e in ELEMENTS {
                if e.group != group {
                    group = e.group;
                    ui.label(RichText::new(group).strong());
                    ui.end_row();
                }
                ui.label(e.label);
                let mut c = (e.get)(&colors);
                let picked = egui::color_picker::color_edit_button_srgba(
                    ui,
                    &mut c,
                    if e.translucent {
                        egui::color_picker::Alpha::OnlyBlend
                    } else {
                        egui::color_picker::Alpha::Opaque
                    },
                )
                .changed();
                if e.translucent {
                    ui.horizontal(|ui| {
                        ui.label("Translucency");
                        let [r, g, b, a] = c.to_srgba_unmultiplied();
                        let mut percent = ((1.0 - f64::from(a) / 255.0) * 100.0).round();
                        let before = percent;
                        ui.add(
                            crate::gui::valuefield::ExprValue::new(&mut percent)
                                .range(0.0..=100.0)
                                .suffix(" %"),
                        );
                        if (percent - before).abs() > 1e-6 {
                            let alpha = ((1.0 - percent / 100.0) * 255.0).round() as u8;
                            c = Color32::from_rgba_unmultiplied(r, g, b, alpha);
                            (e.set)(&mut colors, c);
                            changed = true;
                        }
                    });
                } else {
                    ui.label("");
                }
                if picked {
                    (e.set)(&mut colors, c);
                    changed = true;
                }
                ui.end_row();
            }
        });
    if changed {
        let name = s.active.clone();
        if let Some(x) = s.custom.iter_mut().find(|x| x.name == name) {
            x.colors = colors;
            s.unsaved = Some(name);
        }
        set_canvas(s.colors());
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ui_with_copy() -> (ColorsUi, String) {
        let mut s = ColorsUi::default();
        let name = s.new_scheme();
        (s, name)
    }

    /// Waits for the workers' answers.
    fn pictures(s: &mut ColorsUi) -> Vec<(ImageSlot, Picture)> {
        let mut got = Vec::new();
        for _ in 0..500 {
            got.extend(s.finished_pictures());
            if !s.pictures_pending() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        got
    }

    fn temp_png(tag: &str, w: u32, h: u32) -> String {
        let p = std::env::temp_dir().join(format!("cadcraft-pic-{tag}-{}.png", std::process::id()));
        let rgba: Vec<u8> = [9, 8, 7, 255].repeat((w * h) as usize);
        let bytes =
            crate::part::thumbnail::encode_png(&rgba, w, h).unwrap_or_else(|| panic!("no png"));
        std::fs::write(&p, bytes).unwrap_or_else(|e| panic!("{e}"));
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn a_chosen_picture_is_read_and_comes_into_use() {
        let path = temp_png("ok", 3, 2);
        let mut s = ColorsUi::default();
        s.choose_picture(ImageSlot::Background, &path);
        let got = pictures(&mut s);
        assert_eq!(got.len(), 1);
        assert_eq!((got[0].1.width, got[0].1.height), (3, 2));
        assert_eq!(got[0].1.rgba[..4], [9, 8, 7, 255]);
        assert!(s.background.on);
        assert_eq!(s.message, None);
        // Choosing it again reads nothing.
        s.choose_picture(ImageSlot::Background, &path);
        assert!(!s.pictures_pending());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_file_that_cannot_be_read_leaves_the_setting_as_it_was() {
        let path = temp_png("good", 1, 1);
        let mut s = ColorsUi::default();
        s.choose_picture(ImageSlot::Cap, &path);
        pictures(&mut s);
        s.choose_picture(ImageSlot::Cap, "no such file.png");
        assert!(pictures(&mut s).is_empty());
        assert_eq!(
            s.cap,
            PictureSetting {
                on: true,
                path: path.clone()
            }
        );
        assert!(
            s.message
                .as_deref()
                .is_some_and(|m| m.contains("no such file"))
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn the_preview_background_comes_back_from_the_options() {
        let mut s = ColorsUi::default();
        assert_eq!(s.preview_back(), None);
        assert_eq!(s.preview_text(), "");
        s.set_preview_text("#102030");
        assert_eq!(s.preview_back(), Some([0x10, 0x20, 0x30]));
        assert_eq!(s.preview_text(), "#102030");
        s.set_preview_text("not a colour");
        assert_eq!(s.preview_back(), None);
    }

    #[test]
    fn new_copies_the_active_scheme_and_takes_over() {
        let (s, name) = ui_with_copy();
        assert_eq!(s.active, name);
        assert_eq!(name, format!("{DEFAULT_SCHEME} copy"));
        assert_eq!(s.colors(), CanvasColors::light());
    }

    #[test]
    fn a_builtin_scheme_cannot_be_removed_or_renamed() {
        let mut s = ColorsUi::default();
        assert!(!s.remove(DEFAULT_SCHEME));
        assert!(s.rename(DEFAULT_SCHEME, "Other").is_err());
        assert_eq!(s.active, DEFAULT_SCHEME);
    }

    #[test]
    fn removing_the_scheme_in_use_falls_back_to_the_default() {
        let (mut s, name) = ui_with_copy();
        assert!(s.remove(&name));
        assert_eq!(s.active, DEFAULT_SCHEME);
        assert!(s.custom.is_empty());
    }

    #[test]
    fn rename_keeps_the_scheme_in_use_and_refuses_a_taken_name() {
        let (mut s, name) = ui_with_copy();
        assert!(s.rename(&name, "  ").is_err());
        assert!(s.rename(&name, DEFAULT_SCHEME).is_err());
        assert!(s.rename(&name, "Mine").is_ok());
        assert_eq!(s.active, "Mine");
        // Changing only its case is the same scheme.
        assert!(s.rename("Mine", "MINE").is_ok());
    }

    #[test]
    fn an_unknown_scheme_means_the_default() {
        let mut s = ColorsUi::default();
        s.load("Gone since", true);
        assert_eq!(s.active, DEFAULT_SCHEME);
        assert_eq!(s.colors(), CanvasColors::light());
    }

    #[test]
    fn one_color_is_a_setting_of_its_own() {
        let mut s = ColorsUi::default();
        s.select("3DCadCraft Graphite");
        s.gradient = false;
        assert!(!s.colors().gradient);
        assert_eq!(
            s.colors().background_top,
            CanvasColors::graphite().background_top
        );
    }

    #[test]
    fn import_takes_schemes_replaces_by_name_and_skips_builtins() {
        let (mut s, name) = ui_with_copy();
        let list = json!([
            { "name": name, "colors": { "dimension": "#010203" } },
            { "name": "From a colleague" },
            { "name": DEFAULT_SCHEME, "colors": { "dimension": "#010203" } },
            { "colors": {} }
        ]);
        assert_eq!(s.import_json(&list), 2);
        assert_eq!(s.custom.len(), 2);
        let mine = s.custom.iter().find(|x| x.name == name);
        assert_eq!(
            mine.map(|x| x.colors.dimension),
            Some(Color32::from_rgb(1, 2, 3))
        );
        let builtin = s.all().into_iter().find(|x| x.name == DEFAULT_SCHEME);
        assert_eq!(builtin.map(|x| x.colors), Some(CanvasColors::light()));
    }

    #[test]
    fn an_export_comes_back_through_import() {
        let (s, name) = ui_with_copy();
        let file = export_file(json!({ "ui_theme": "Light" }), s.export_json());
        let mut other = ColorsUi::default();
        assert_eq!(other.import_json(&file["color_schemes"]), 1);
        assert_eq!(other.custom[0].name, name);
    }
}
