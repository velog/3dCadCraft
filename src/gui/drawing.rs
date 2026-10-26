//! The drawing environment (`docs/spec/4-drawings.md`, stage D1): the sheet
//! in the view (pan, zoom, views with their lines), Base View and Projected
//! View, the Drawing View and Edit Sheet dialogs, the browser tree and the
//! properties of a drawing. The model lives in `crate::drawing`.
//!
//! A view's lines are computed on a worker thread (`hlr::view_lines` on a
//! copy of the part) and cached by model signature and view direction; the
//! frame only draws what is ready.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke, Vec2,
};

use super::CadCraftApp;
use super::icons::Icon;
use super::valuefield::ExprValue;
use crate::document::{DocRef, DocumentData, DocumentKind};
use crate::drawing::hlr::{self, ViewLines};
use crate::drawing::sheet::{self as fmt, Anchor, Prim, TitleField};
use crate::drawing::{
    self as drw, DisplayOptions, DrawingDocument, Orientation, ProjectDir, RecoveryOptions,
    SheetSize, ViewFrame, ViewKind, ViewOrientation, ViewStyle,
};

/// Drawing commands of the ribbon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrwCmd {
    /// Place Views › Base View.
    BaseView,
    /// Place Views › Projected View.
    ProjectedView,
    /// Place Views › New Sheet.
    NewSheet,
    /// Place Views › Edit Sheet.
    EditSheet,
}

/// What the sheet view shows: the sheet point at the middle and the zoom.
#[derive(Clone, Copy, Debug)]
pub(super) struct SheetCam {
    /// Sheet mm at the middle of the view.
    center: [f64; 2],
    /// Pixels per sheet mm.
    zoom: f64,
    /// Fit the sheet on the next frame (Zoom All, a new sheet).
    fit: bool,
}

impl Default for SheetCam {
    fn default() -> Self {
        Self {
            center: [0.0, 0.0],
            zoom: 2.0,
            fit: true,
        }
    }
}

impl SheetCam {
    fn to_screen(self, rect: Rect, p: [f64; 2]) -> Pos2 {
        let c = rect.center();
        Pos2::new(
            c.x + ((p[0] - self.center[0]) * self.zoom) as f32,
            c.y - ((p[1] - self.center[1]) * self.zoom) as f32,
        )
    }

    fn to_sheet(self, rect: Rect, q: Pos2) -> [f64; 2] {
        let c = rect.center();
        [
            self.center[0] + f64::from(q.x - c.x) / self.zoom,
            self.center[1] - f64::from(q.y - c.y) / self.zoom,
        ]
    }
}

/// A running placement command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DrwTool {
    /// The Drawing View dialog is open for a new base view: a click on the
    /// sheet places it.
    Base,
    /// Projected View: the parent (picked, or the base just placed).
    Projected(Option<u32>),
}

/// Tabs of the Drawing View dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DlgTab {
    Component,
    Model,
    Display,
    Recovery,
}

/// The Drawing View dialog (Base View, Edit View).
#[derive(Clone, Debug)]
pub(super) struct ViewDialog {
    /// `Some(id)`: Edit View of that view.
    edit: Option<u32>,
    /// The view is a projected one (orientation from its parent).
    projected: bool,
    tab: DlgTab,
    model: Option<DocRef>,
    orientation: ViewOrientation,
    style: ViewStyle,
    style_from_base: bool,
    scale: f64,
    scale_from_base: bool,
    /// The scale follows the model size until the user sets one.
    auto_scale: bool,
    identifier: String,
    label_visible: bool,
    display: DisplayOptions,
    recovery: RecoveryOptions,
}

/// The Edit Sheet dialog.
#[derive(Clone, Debug)]
pub(super) struct SheetDialog {
    index: usize,
    name: String,
    size: SheetSize,
    width: f64,
    height: f64,
    orientation: Orientation,
    revision: String,
    exclude_count: bool,
    exclude_print: bool,
    border: bool,
    title_block: bool,
}

/// Cache key of a view's lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct LinesKey {
    model: DocRef,
    signature: u64,
    frame: [i64; 6],
}

/// Drawing UI state of the app.
#[derive(Default)]
pub(super) struct DrwUi {
    pub cam: SheetCam,
    /// Selected view.
    pub sel: Option<u32>,
    hover: Option<u32>,
    /// Dragged view and the grab offset (view centre − pointer, sheet mm).
    drag: Option<(u32, [f64; 2])>,
    pub tool: Option<DrwTool>,
    pub dialog: Option<ViewDialog>,
    pub sheet_dialog: Option<SheetDialog>,
    /// Finished lines.
    done: Arc<Mutex<HashMap<LinesKey, Arc<ViewLines>>>>,
    /// Jobs started (finished ones stay until the cache is cleared).
    running: HashSet<LinesKey>,
    /// The sheet the camera was fitted to (index, size in µm): another
    /// sheet or a new size (Edit Sheet, Undo) fits again.
    fitted: Option<(usize, [i64; 2])>,
}

impl DrwUi {
    /// Forgets the running command and the selection (document change).
    pub(super) fn reset(&mut self) {
        self.sel = None;
        self.hover = None;
        self.drag = None;
        self.tool = None;
        self.dialog = None;
        self.sheet_dialog = None;
        self.cam.fit = true;
        self.fitted = None;
    }
}

const PAPER: Color32 = Color32::from_rgb(252, 252, 250);
const BACKDROP: Color32 = Color32::from_rgb(112, 118, 126);
const INK: Color32 = Color32::from_rgb(20, 20, 24);
const HIDDEN_INK: Color32 = Color32::from_rgb(70, 70, 80);
const SEL: Color32 = Color32::from_rgb(40, 120, 230);
const HOVER: Color32 = Color32::from_rgb(230, 120, 40);
const PREVIEW: Color32 = Color32::from_rgb(60, 140, 90);

fn quant(f: &ViewFrame) -> [i64; 6] {
    let q = |v: f64| (v * 1e6).round() as i64;
    [
        q(f.toward.x),
        q(f.toward.y),
        q(f.toward.z),
        q(f.up.x),
        q(f.up.y),
        q(f.up.z),
    ]
}

fn hash_str(s: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

/// A view row of the browser: id, identifier, parent, model name.
type TreeView = (u32, String, Option<u32>, String);

/// A view drawn this frame.
struct ShownView {
    id: u32,
    identifier: String,
    /// Sheet rectangle (lower left, upper right).
    lo: [f64; 2],
    hi: [f64; 2],
    lines: Option<Arc<ViewLines>>,
    position: [f64; 2],
    scale: f64,
    style: ViewStyle,
    label: bool,
}

impl CadCraftApp {
    // ---- document access ---------------------------------------------------

    /// The active drawing.
    pub(super) fn drawing(&self) -> Option<&DrawingDocument> {
        match self.project.document(self.part_id).map(|d| &d.data) {
            Some(DocumentData::Drawing(d)) => Some(d),
            _ => None,
        }
    }

    fn drawing_mut(&mut self) -> Option<&mut DrawingDocument> {
        match self.project.document_mut(self.part_id).map(|d| &mut d.data) {
            Some(DocumentData::Drawing(d)) => Some(d),
            _ => None,
        }
    }

    /// The active document is a drawing.
    pub(super) fn is_drawing(&self) -> bool {
        self.drawing().is_some()
    }

    /// Changes the active drawing after an Undo checkpoint `label`.
    fn edit_drawing<R>(
        &mut self,
        label: &str,
        f: impl FnOnce(&mut DrawingDocument) -> R,
    ) -> Option<R> {
        if !self.is_drawing() {
            return None;
        }
        self.checkpoint(label);
        let r = self.drawing_mut().map(f);
        self.scene_dirty = true;
        r
    }

    /// File → New Drawing: a drawing document in the open project (its
    /// parts are offered to the Base View); it becomes the active document.
    pub(super) fn new_drawing(&mut self) {
        self.checkpoint("New Drawing");
        let n = (1..)
            .find(|k| {
                let name = format!("Drawing{k}");
                !self.project.documents().iter().any(|d| d.name == name)
            })
            .unwrap_or(1);
        let name = format!("Drawing{n}");
        let id = self
            .project
            .add_document(&name, DocumentData::Drawing(DrawingDocument::default()));
        if let Some(d) = self.project.document_mut(id) {
            d.properties = crate::document::iprops::IProperties::new_document();
        }
        self.activate_document(id);
        self.file_status = Some(Ok(format!(
            "{name}: Place Views > Base View (BV) places a view of a part"
        )));
    }

    // ---- commands ------------------------------------------------------------

    /// Runs a drawing command.
    pub(super) fn drw_apply(&mut self, cmd: DrwCmd) {
        if !self.is_drawing() {
            return;
        }
        match cmd {
            DrwCmd::BaseView => self.open_base_view(),
            DrwCmd::ProjectedView => {
                self.drw.dialog = None;
                let parent = self.drw.sel;
                self.drw.tool = Some(DrwTool::Projected(parent));
            }
            DrwCmd::NewSheet => {
                self.edit_drawing("New Sheet", |d| d.new_sheet());
                self.drw.sel = None;
                self.drw.cam.fit = true;
            }
            DrwCmd::EditSheet => {
                let i = self.drawing().map_or(0, |d| d.active_sheet);
                self.open_sheet_dialog(i);
            }
        }
    }

    /// The parts a view can show: those of this project, then those of
    /// linked files already used by the drawing.
    fn drawing_models(&self) -> Vec<(DocRef, String)> {
        let mut out: Vec<(DocRef, String)> = self
            .project
            .documents()
            .iter()
            .filter(|d| d.data.kind() == DocumentKind::Part)
            .map(|d| (DocRef::local(d.id), d.name.clone()))
            .collect();
        if let Some(d) = self.drawing() {
            for r in d.references() {
                if r.library.is_some() && !out.iter().any(|(x, _)| *x == r) {
                    out.push((r, self.model_name(r)));
                }
            }
        }
        out
    }

    /// Display name of a model reference.
    fn model_name(&self, r: DocRef) -> String {
        match r.library {
            None => self
                .project
                .document(r.document)
                .map_or_else(|| "(missing)".into(), |d| d.name.clone()),
            Some(lib) => self
                .project
                .library(lib)
                .map_or_else(|| "(missing link)".into(), |l| l.path.clone()),
        }
    }

    fn open_base_view(&mut self) {
        let models = self.drawing_models();
        let model = self
            .drw
            .dialog
            .as_ref()
            .and_then(|d| d.model)
            .or_else(|| models.first().map(|m| m.0));
        let identifier = self.drawing().map_or_else(String::new, |d| {
            let used: Vec<&str> = d
                .sheets
                .iter()
                .flat_map(|s| &s.views)
                .map(|v| v.identifier.as_str())
                .collect();
            (1..)
                .map(|k| format!("VIEW{k}"))
                .find(|s| !used.contains(&s.as_str()))
                .unwrap_or_default()
        });
        self.drw.dialog = Some(ViewDialog {
            edit: None,
            projected: false,
            tab: DlgTab::Component,
            model,
            orientation: ViewOrientation::Front,
            style: ViewStyle::HiddenShown,
            style_from_base: false,
            scale: 1.0,
            scale_from_base: false,
            auto_scale: true,
            identifier,
            label_visible: true,
            display: DisplayOptions::default(),
            recovery: RecoveryOptions::default(),
        });
        self.drw.tool = Some(DrwTool::Base);
        if model.is_none() {
            self.file_status = Some(Err(
                "Base View: this project has no part; Browse... links a part file".into(),
            ));
        }
    }

    /// Edit View of view `id` of the active sheet.
    fn open_edit_view(&mut self, id: u32) {
        let Some(d) = self.drawing() else {
            return;
        };
        let Some(sheet) = d.sheet() else {
            return;
        };
        let Some(v) = sheet.view(id) else {
            return;
        };
        let (model, orientation) = match &v.kind {
            ViewKind::Base { model, orientation } => (Some(*model), *orientation),
            ViewKind::Projected { .. } => (d.model_of(sheet, id), ViewOrientation::Front),
        };
        self.drw.dialog = Some(ViewDialog {
            edit: Some(id),
            projected: matches!(v.kind, ViewKind::Projected { .. }),
            tab: DlgTab::Component,
            model,
            orientation,
            style: v.style,
            style_from_base: v.style_from_base,
            scale: v.scale,
            scale_from_base: v.scale_from_base,
            auto_scale: false,
            identifier: v.identifier.clone(),
            label_visible: v.label_visible,
            display: v.display,
            recovery: v.recovery,
        });
        self.drw.tool = None;
    }

    fn open_sheet_dialog(&mut self, index: usize) {
        let Some(s) = self.drawing().and_then(|d| d.sheets.get(index)) else {
            return;
        };
        let (w, h) = s.size.landscape();
        self.drw.sheet_dialog = Some(SheetDialog {
            index,
            name: s.name.clone(),
            size: s.size,
            width: w,
            height: h,
            orientation: s.orientation,
            revision: s.revision.clone(),
            exclude_count: s.exclude_from_count,
            exclude_print: s.exclude_from_printing,
            border: s.border,
            title_block: s.title_block,
        });
    }

    /// Esc: closes the running drawing command.
    pub(super) fn drw_cancel(&mut self) -> bool {
        if self.drw.tool.is_some() || self.drw.dialog.is_some() || self.drw.sheet_dialog.is_some() {
            self.drw.tool = None;
            self.drw.dialog = None;
            self.drw.sheet_dialog = None;
            true
        } else {
            false
        }
    }

    /// A drawing command or dialog is open.
    pub(super) fn drw_open(&self) -> bool {
        self.drw.tool.is_some() || self.drw.dialog.is_some() || self.drw.sheet_dialog.is_some()
    }

    /// Status-bar prompt.
    pub(super) fn drw_prompt(&self) -> String {
        match self.drw.tool {
            Some(DrwTool::Base) => {
                "Base View: set the options, then click on the sheet to place the view".into()
            }
            Some(DrwTool::Projected(None)) => "Projected View: select the parent view".into(),
            Some(DrwTool::Projected(Some(_))) => {
                "Projected View: click beside the view for an orthographic view, diagonally for an isometric one (Esc ends)".into()
            }
            None if self.drw.sel.is_some() => {
                "Drag the view to move it, double-click to edit it, Delete removes it".into()
            }
            None => "Place Views > Base View (BV) places a view of a part".into(),
        }
    }

    // ---- view lines ------------------------------------------------------------

    /// The lines of `model` seen in `frame`: from the cache, or `None` while
    /// a worker computes them.
    fn view_lines(
        &mut self,
        ctx: &egui::Context,
        model: DocRef,
        frame: &ViewFrame,
    ) -> Option<Arc<ViewLines>> {
        let sig = self.with_part(model, |p| p.body_signature())?;
        let key = LinesKey {
            model,
            signature: hash_str(&sig),
            frame: quant(frame),
        };
        if let Ok(done) = self.drw.done.lock()
            && let Some(l) = done.get(&key)
        {
            return Some(l.clone());
        }
        if self.drw.running.insert(key) {
            let part = self.with_part(model, |p| p.clone())?;
            let done = self.drw.done.clone();
            let ctx = ctx.clone();
            let frame = *frame;
            let spawned = std::thread::Builder::new()
                .name("drawing-view".into())
                .spawn(move || {
                    let t = std::time::Instant::now();
                    let lines = hlr::view_lines(&hlr::part_meshes(&part), &frame);
                    log::info!(
                        "drawing view: {} visible, {} hidden pieces in {:.0} ms",
                        lines.visible.len(),
                        lines.hidden.len(),
                        t.elapsed().as_secs_f64() * 1e3
                    );
                    if let Ok(mut d) = done.lock() {
                        if d.len() > 256 {
                            d.clear();
                        }
                        d.insert(key, Arc::new(lines));
                    }
                    ctx.request_repaint();
                });
            if let Err(e) = spawned {
                log::error!("drawing view: no worker thread: {e}");
            }
        }
        None
    }

    /// Every view of the active sheet with its lines and sheet rectangle.
    fn shown_views(&mut self, ctx: &egui::Context) -> Vec<ShownView> {
        let Some(d) = self.drawing() else {
            return Vec::new();
        };
        let Some(sheet) = d.sheet() else {
            return Vec::new();
        };
        let jobs: Vec<_> = sheet
            .views
            .iter()
            .map(|v| {
                let model = d.model_of(sheet, v.id);
                let frame = d.frame_of(sheet, v.id);
                let (scale, style) = d.effective(sheet, v.id).unwrap_or((v.scale, v.style));
                (
                    v.id,
                    v.identifier.clone(),
                    v.position,
                    v.label_visible,
                    model,
                    frame,
                    scale,
                    style,
                )
            })
            .collect();
        let mut out = Vec::new();
        for (id, identifier, position, label, model, frame, scale, style) in jobs {
            let lines = match (model, frame) {
                (Some(m), Some(f)) => self.view_lines(ctx, m, &f),
                _ => None,
            };
            let (w, h) = lines.as_ref().map_or((40.0, 30.0), |l| {
                let (w, h) = l.size();
                (w * scale, h * scale)
            });
            let m = 3.0;
            out.push(ShownView {
                id,
                identifier,
                lo: [position[0] - w * 0.5 - m, position[1] - h * 0.5 - m],
                hi: [position[0] + w * 0.5 + m, position[1] + h * 0.5 + m],
                lines,
                position,
                scale,
                style,
                label,
            });
        }
        out
    }

    // ---- the sheet view ----------------------------------------------------------

    /// The central area while a drawing is active.
    pub(super) fn drawing_viewport(&mut self, ui: &mut egui::Ui) {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        self.viewport_rect = rect;
        let ctx = ui.ctx().clone();
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, BACKDROP);
        let Some((sheet_w, sheet_h, active)) = self
            .drawing()
            .and_then(|d| d.sheet().map(|s| (s.dims(), d.active_sheet)))
            .map(|((w, h), i)| (w, h, i))
        else {
            return;
        };
        // Fit on request and when another sheet becomes active.
        let fit_key = (active, [(sheet_w * 1e3) as i64, (sheet_h * 1e3) as i64]);
        if self.drw.cam.fit || self.drw.fitted != Some(fit_key) {
            let z =
                (f64::from(rect.width()) / sheet_w).min(f64::from(rect.height()) / sheet_h) * 0.94;
            self.drw.cam = SheetCam {
                center: [sheet_w * 0.5, sheet_h * 0.5],
                zoom: z.max(0.05),
                fit: false,
            };
            self.drw.fitted = Some(fit_key);
        }
        // Zoom at the pointer, pan with the middle button.
        let pointer = response.hover_pos();
        if let Some(p) = pointer {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                let before = self.drw.cam.to_sheet(rect, p);
                self.drw.cam.zoom =
                    (self.drw.cam.zoom * f64::from((scroll * 0.0015).exp())).clamp(0.05, 200.0);
                let after = self.drw.cam.to_sheet(rect, p);
                self.drw.cam.center[0] += before[0] - after[0];
                self.drw.cam.center[1] += before[1] - after[1];
            }
        }
        if response.dragged_by(egui::PointerButton::Middle) {
            let d = response.drag_delta();
            self.drw.cam.center[0] -= f64::from(d.x) / self.drw.cam.zoom;
            self.drw.cam.center[1] += f64::from(d.y) / self.drw.cam.zoom;
        }
        let cam = self.drw.cam;
        let to = |p: [f64; 2]| cam.to_screen(rect, p);
        let px = |mm: f64| ((mm * cam.zoom) as f32).clamp(1.0, 6.0);

        // Paper with a shadow.
        let paper = Rect::from_two_pos(to([0.0, 0.0]), to([sheet_w, sheet_h]));
        painter.rect_filled(
            paper.translate(Vec2::new(4.0, 4.0)),
            0.0,
            Color32::from_black_alpha(70),
        );
        painter.rect_filled(paper, 0.0, PAPER);

        // Border and title block.
        let prims = self.format_prims_active();
        for p in &prims {
            match p {
                Prim::Line { a, b, weight } => {
                    painter.line_segment([to(*a), to(*b)], Stroke::new(px(weight.mm()), INK));
                }
                Prim::Text {
                    at,
                    height,
                    text,
                    anchor,
                } => {
                    let size = (*height * cam.zoom * 1.35) as f32;
                    if size >= 4.0 {
                        let align = match anchor {
                            Anchor::Left => Align2::LEFT_BOTTOM,
                            Anchor::Center => Align2::CENTER_BOTTOM,
                        };
                        painter.text(to(*at), align, text, FontId::proportional(size), INK);
                    }
                }
            }
        }

        // Views.
        let views = self.shown_views(&ctx);
        let at_sheet = pointer.map(|p| cam.to_sheet(rect, p));
        let hit = |q: [f64; 2]| {
            views
                .iter()
                .rev()
                .find(|v| q[0] >= v.lo[0] && q[0] <= v.hi[0] && q[1] >= v.lo[1] && q[1] <= v.hi[1])
                .map(|v| v.id)
        };
        self.drw.hover = at_sheet.and_then(hit);
        for v in &views {
            draw_view(&painter, &cam, rect, v);
            let frame_color = if self.drw.sel == Some(v.id) {
                Some(SEL)
            } else if self.drw.hover == Some(v.id) && self.drw.drag.is_none() {
                Some(HOVER)
            } else {
                None
            };
            if let Some(c) = frame_color {
                dashed_rect(&painter, to(v.lo), to(v.hi), c);
            }
            if v.lines.is_none() {
                painter.text(
                    to(v.position),
                    Align2::CENTER_CENTER,
                    "Computing view...",
                    FontId::proportional(13.0),
                    HIDDEN_INK,
                );
            }
            if v.label {
                let size = (3.5 * cam.zoom * 1.35) as f32;
                if size >= 5.0 {
                    let text = format!(
                        "{}\nSCALE {}",
                        v.identifier,
                        drw::scale_text(v.scale).replace(' ', "")
                    );
                    painter.text(
                        to([v.position[0], v.lo[1] - 1.5]),
                        Align2::CENTER_TOP,
                        text,
                        FontId::proportional(size),
                        INK,
                    );
                }
            }
        }

        // The running command.
        match self.drw.tool {
            Some(DrwTool::Base) => self.base_view_place(&ctx, &painter, rect, &response, at_sheet),
            Some(DrwTool::Projected(parent)) => {
                self.projected_place(&ctx, &painter, rect, &response, at_sheet, parent, &views)
            }
            None => self.sheet_select(rect, &response, at_sheet, &views),
        }
        if response.secondary_clicked() && self.drw.tool.is_some() {
            // Right click ends Projected View (Inventor's Create).
            if matches!(self.drw.tool, Some(DrwTool::Projected(_))) {
                self.drw.tool = None;
            }
        }
        // Keys over the sheet.
        if response.hovered() || response.has_focus() {
            let (del, esc) = ui.input(|i| {
                (
                    i.key_pressed(egui::Key::Delete),
                    i.key_pressed(egui::Key::Escape),
                )
            });
            if esc {
                if !self.drw_cancel() {
                    self.drw.sel = None;
                }
            } else if del
                && self.drw.tool.is_none()
                && let Some(id) = self.drw.sel
            {
                self.delete_drawing_view(id);
            }
        }
    }

    fn format_prims_active(&self) -> Vec<Prim> {
        let Some(d) = self.drawing() else {
            return Vec::new();
        };
        let i = d.active_sheet;
        // The model of the sheet's first view gives the model properties.
        let model = d
            .sheet()
            .and_then(|s| s.views.first().and_then(|v| d.model_of(s, v.id)));
        let drawing_props = self
            .project
            .document(self.part_id)
            .map(|x| x.properties.clone());
        let model_props = model.and_then(|m| match m.library {
            None => self
                .project
                .document(m.document)
                .map(|x| (x.properties.clone(), x.name.clone())),
            Some(_) => None,
        });
        let material = model
            .and_then(|m| self.with_part(m, |p| p.material.clone()))
            .flatten();
        let get = |f: TitleField| -> String {
            let dp = |k: &str| {
                drawing_props
                    .as_ref()
                    .map_or(String::new(), |p| p.get(k).to_string())
            };
            let mp = |k: &str| {
                model_props
                    .as_ref()
                    .map_or(String::new(), |(p, _)| p.get(k).to_string())
            };
            let first = |a: String, b: String| if a.is_empty() { b } else { a };
            match f {
                TitleField::Title => first(
                    first(mp("title"), mp("description")),
                    model_props
                        .as_ref()
                        .map_or(String::new(), |(_, n)| n.clone()),
                ),
                TitleField::PartNumber => first(
                    mp("part_number"),
                    model_props
                        .as_ref()
                        .map_or(String::new(), |(_, n)| n.clone()),
                ),
                TitleField::Designer => first(dp("designer"), dp("author")),
                TitleField::CheckedBy => dp("checked_by"),
                TitleField::Date => dp("creation_date"),
                TitleField::Material => material.clone().unwrap_or_default(),
                TitleField::Company => dp("company"),
                TitleField::Revision => mp("revision"),
            }
        };
        fmt::format_prims(d, i, &get)
    }

    /// Base View: the preview follows the pointer; a click places it and
    /// Projected View continues from it.
    fn base_view_place(
        &mut self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        rect: Rect,
        response: &egui::Response,
        at: Option<[f64; 2]>,
    ) {
        let Some(dlg) = self.drw.dialog.clone() else {
            self.drw.tool = None;
            return;
        };
        let (Some(model), Some(at)) = (dlg.model, at) else {
            return;
        };
        let frame = dlg.orientation.frame();
        let Some(lines) = self.view_lines(ctx, model, &frame) else {
            return;
        };
        let mut scale = dlg.scale;
        if dlg.auto_scale
            && let Some(dims) = self.drawing().and_then(|d| d.sheet().map(|s| s.dims()))
        {
            scale = drw::fit_scale(lines.size(), dims, 0.35);
            if let Some(d) = self.drw.dialog.as_mut() {
                d.scale = scale;
            }
        }
        let v = ShownView {
            id: 0,
            identifier: String::new(),
            lo: at,
            hi: at,
            lines: Some(lines),
            position: at,
            scale,
            style: dlg.style,
            label: false,
        };
        draw_view_colored(painter, &self.drw.cam, rect, &v, PREVIEW);
        if response.clicked() {
            self.place_base_view(&dlg, at, scale);
        }
    }

    /// Places the dialog's base view at `at` and continues with Projected
    /// View from it.
    fn place_base_view(&mut self, dlg: &ViewDialog, at: [f64; 2], scale: f64) {
        let Some(model) = dlg.model else {
            return;
        };
        let id = self.edit_drawing("Base View", |d| {
            let id = d.add_base_view(model, dlg.orientation, snap(at), scale, dlg.style)?;
            if let Some(v) = d.sheet_mut().and_then(|s| s.view_mut(id)) {
                v.identifier = dlg.identifier.clone();
                v.label_visible = dlg.label_visible;
                v.display = dlg.display;
                v.recovery = dlg.recovery;
            }
            Some(id)
        });
        if let Some(Some(id)) = id {
            self.drw.dialog = None;
            self.drw.sel = Some(id);
            self.drw.tool = Some(DrwTool::Projected(Some(id)));
            self.file_status = Some(Ok(
                "Base View placed: click beside it for projected views, Esc or right-click ends"
                    .into(),
            ));
        } else {
            self.history.cancel();
        }
    }

    /// Projected View: pick the parent, then each click places a view.
    #[allow(clippy::too_many_arguments)]
    fn projected_place(
        &mut self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        rect: Rect,
        response: &egui::Response,
        at: Option<[f64; 2]>,
        parent: Option<u32>,
        views: &[ShownView],
    ) {
        let Some(parent) = parent else {
            if response.clicked()
                && let Some(id) = self.drw.hover
            {
                self.drw.tool = Some(DrwTool::Projected(Some(id)));
                self.drw.sel = Some(id);
            }
            return;
        };
        let Some(at) = at else {
            return;
        };
        let Some(pv) = views.iter().find(|v| v.id == parent) else {
            self.drw.tool = Some(DrwTool::Projected(None));
            return;
        };
        let Some((frame, model, projection)) = self.drawing().and_then(|d| {
            let s = d.sheet()?;
            Some((
                d.frame_of(s, parent)?,
                d.model_of(s, parent)?,
                d.standard.projection,
            ))
        }) else {
            return;
        };
        let Some(dir) = ProjectDir::from_offset(at[0] - pv.position[0], at[1] - pv.position[1])
        else {
            return;
        };
        // Not over the parent itself.
        if at[0] >= pv.lo[0] && at[0] <= pv.hi[0] && at[1] >= pv.lo[1] && at[1] <= pv.hi[1] {
            return;
        }
        let mut pos = at;
        match dir {
            ProjectDir::Left | ProjectDir::Right => pos[1] = pv.position[1],
            ProjectDir::Up | ProjectDir::Down => pos[0] = pv.position[0],
            ProjectDir::Iso(..) => {}
        }
        let f = frame.projected(dir, projection);
        let to = |p| self.drw.cam.to_screen(rect, p);
        painter.line_segment(
            [to(pv.position), to(pos)],
            Stroke::new(1.0, PREVIEW.gamma_multiply(0.6)),
        );
        if let Some(lines) = self.view_lines(ctx, model, &f) {
            let v = ShownView {
                id: 0,
                identifier: String::new(),
                lo: pos,
                hi: pos,
                lines: Some(lines),
                position: pos,
                scale: pv.scale,
                style: pv.style,
                label: false,
            };
            draw_view_colored(painter, &self.drw.cam, rect, &v, PREVIEW);
        }
        if response.clicked() {
            let placed =
                self.edit_drawing("Projected View", |d| d.add_projected_view(parent, snap(at)));
            if let Some(Some(id)) = placed {
                self.drw.sel = Some(id);
            } else {
                self.history.cancel();
            }
        }
    }

    /// No command: select, drag, double-click to edit, right-click menu.
    fn sheet_select(
        &mut self,
        rect: Rect,
        response: &egui::Response,
        at: Option<[f64; 2]>,
        views: &[ShownView],
    ) {
        let cam = self.drw.cam;
        if response.drag_started_by(egui::PointerButton::Primary) {
            let origin = response
                .ctx
                .input(|i| i.pointer.press_origin())
                .map(|p| cam.to_sheet(rect, p));
            let id = origin.and_then(|q| {
                views
                    .iter()
                    .rev()
                    .find(|v| {
                        q[0] >= v.lo[0] && q[0] <= v.hi[0] && q[1] >= v.lo[1] && q[1] <= v.hi[1]
                    })
                    .map(|v| (v.id, [v.position[0] - q[0], v.position[1] - q[1]]))
            });
            if let Some((id, off)) = id {
                self.drw.drag = Some((id, off));
                self.drw.sel = Some(id);
            }
        }
        if let (Some((id, off)), Some(q)) = (self.drw.drag, at)
            && response.dragged_by(egui::PointerButton::Primary)
        {
            let to = snap([q[0] + off[0], q[1] + off[1]]);
            let moved = self
                .drawing()
                .and_then(|d| d.sheet()?.view(id))
                .is_some_and(|v| (v.position[0] - to[0]).hypot(v.position[1] - to[1]) > 1e-6);
            if moved {
                self.edit_drawing("Move View", |d| d.move_view(id, to));
            }
        }
        if response.drag_stopped() && self.drw.drag.take().is_some() {
            self.history.break_coalescing();
        }
        if response.double_clicked() {
            if let Some(id) = self.drw.hover {
                self.open_edit_view(id);
            }
        } else if response.clicked() {
            self.drw.sel = self.drw.hover;
        }
        if response.secondary_clicked() {
            self.drw.sel = self.drw.hover;
        }
        let target = self.drw.sel;
        response.context_menu(|ui| {
            if let Some(id) = target {
                if ui.button("Edit View...").clicked() {
                    self.open_edit_view(id);
                    ui.close();
                }
                if ui.button("Projected View (PV)").clicked() {
                    self.drw.tool = Some(DrwTool::Projected(Some(id)));
                    ui.close();
                }
                if ui.button("Delete").clicked() {
                    self.delete_drawing_view(id);
                    ui.close();
                }
                ui.separator();
            }
            if ui.button("Base View (BV)...").clicked() {
                self.open_base_view();
                ui.close();
            }
            if ui.button("Edit Sheet...").clicked() {
                self.drw_apply(DrwCmd::EditSheet);
                ui.close();
            }
            if ui.button("New Sheet (NS)").clicked() {
                self.drw_apply(DrwCmd::NewSheet);
                ui.close();
            }
            if ui.button("Zoom All").clicked() {
                self.drw.cam.fit = true;
                ui.close();
            }
        });
    }

    /// Deletes view `id` and the views projected from it (one Undo step).
    fn delete_drawing_view(&mut self, id: u32) {
        if let Some(gone) = self.edit_drawing("Delete View", |d| d.delete_view(id)) {
            let n = gone.len();
            if n == 0 {
                self.history.cancel();
            } else {
                self.drw.sel = None;
                self.file_status = Some(Ok(if n > 1 {
                    format!("Delete View: {n} views removed (with the views projected from it)")
                } else {
                    "Delete View: the view was removed".into()
                }));
            }
        }
    }

    // ---- windows -------------------------------------------------------------------

    /// The Drawing View and Edit Sheet windows.
    pub(super) fn drawing_windows(&mut self, ctx: &egui::Context) {
        if !self.is_drawing() {
            return;
        }
        self.view_dialog_window(ctx);
        self.sheet_dialog_window(ctx);
    }

    fn view_dialog_window(&mut self, ctx: &egui::Context) {
        let Some(mut dlg) = self.drw.dialog.clone() else {
            return;
        };
        let models = self.drawing_models();
        let mut open = true;
        let mut ok = false;
        let mut cancel = false;
        let mut browse = false;
        let title = if dlg.edit.is_some() {
            "Drawing View (Edit)"
        } else {
            "Drawing View"
        };
        let win = egui::Window::new(title)
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .default_width(460.0);
        self.layout
            .place("drawing_view", ctx.content_rect(), false, win)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (t, name) in [
                        (DlgTab::Component, "Component"),
                        (DlgTab::Model, "Model"),
                        (DlgTab::Display, "Display Options"),
                        (DlgTab::Recovery, "Recovery Options"),
                    ] {
                        ui.selectable_value(&mut dlg.tab, t, name);
                    }
                });
                ui.separator();
                match dlg.tab {
                    DlgTab::Component => component_tab(ui, &mut dlg, &models, &mut browse),
                    DlgTab::Model => model_tab(ui),
                    DlgTab::Display => display_tab(ui, &mut dlg),
                    DlgTab::Recovery => recovery_tab(ui, &mut dlg),
                }
                ui.separator();
                ui.horizontal(|ui| {
                    let ready = dlg.model.is_some();
                    if ui.add_enabled(ready, egui::Button::new("OK")).clicked()
                        || (ready && super::keys::panel_ok_key(ui, false))
                    {
                        ok = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                    if dlg.edit.is_none() {
                        ui.weak("or click on the sheet to place it");
                    }
                });
            });
        if browse {
            self.drw.dialog = Some(dlg);
            self.open_place_dialog();
            return;
        }
        if cancel || !open {
            self.drw.dialog = None;
            if self.drw.tool == Some(DrwTool::Base) {
                self.drw.tool = None;
            }
            return;
        }
        if ok {
            match dlg.edit {
                Some(id) => self.apply_view_edit(id, &dlg),
                None => {
                    // OK without a click: the upper left third of the sheet.
                    let at = self
                        .drawing()
                        .and_then(|d| d.sheet().map(|s| s.dims()))
                        .map_or([100.0, 150.0], |(w, h)| [w * 0.3, h * 0.62]);
                    let scale = dlg.scale;
                    self.place_base_view(&dlg, at, scale);
                }
            }
            return;
        }
        self.drw.dialog = Some(dlg);
    }

    /// OK of Edit View.
    fn apply_view_edit(&mut self, id: u32, dlg: &ViewDialog) {
        let dlg = dlg.clone();
        self.edit_drawing("Edit View", |d| {
            let Some(v) = d.sheet_mut().and_then(|s| s.view_mut(id)) else {
                return;
            };
            if let ViewKind::Base { model, orientation } = &mut v.kind {
                if let Some(m) = dlg.model {
                    *model = m;
                }
                *orientation = dlg.orientation;
            }
            v.style = dlg.style;
            v.style_from_base = dlg.style_from_base && dlg.projected;
            v.scale = dlg.scale;
            v.scale_from_base = dlg.scale_from_base && dlg.projected;
            v.identifier = dlg.identifier.clone();
            v.label_visible = dlg.label_visible;
            v.display = dlg.display;
            v.recovery = dlg.recovery;
        });
        self.drw.dialog = None;
    }

    /// A part file linked by the Place browser for the Base View.
    pub(super) fn drw_linked(&mut self, r: DocRef) {
        if let Some(d) = self.drw.dialog.as_mut() {
            d.model = Some(r);
            d.auto_scale = d.edit.is_none();
        }
        if self.drw.dialog.as_ref().is_some_and(|d| d.edit.is_none()) {
            self.drw.tool = Some(DrwTool::Base);
        }
    }

    fn sheet_dialog_window(&mut self, ctx: &egui::Context) {
        let Some(mut dlg) = self.drw.sheet_dialog.clone() else {
            return;
        };
        let mut open = true;
        let mut ok = false;
        let mut cancel = false;
        let win = egui::Window::new("Edit Sheet")
            .open(&mut open)
            .resizable(false)
            .collapsible(false);
        self.layout
            .place("edit_sheet", ctx.content_rect(), false, win)
            .show(ctx, |ui| {
                egui::Grid::new("edit_sheet_grid")
                    .num_columns(2)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        ui.label("Name");
                        ui.text_edit_singleline(&mut dlg.name);
                        ui.end_row();
                        ui.label("Size");
                        let custom = matches!(dlg.size, SheetSize::Custom { .. });
                        egui::ComboBox::from_id_salt("sheet_size")
                            .selected_text(dlg.size.label())
                            .show_ui(ui, |ui| {
                                for s in SheetSize::NAMED {
                                    ui.selectable_value(&mut dlg.size, s, s.label());
                                }
                                let c = SheetSize::Custom {
                                    width: dlg.width,
                                    height: dlg.height,
                                };
                                if ui.selectable_label(custom, "Custom Size").clicked() {
                                    dlg.size = c;
                                }
                            });
                        ui.end_row();
                        // Height and width are editable only for a custom size.
                        let custom = matches!(dlg.size, SheetSize::Custom { .. });
                        if !custom {
                            (dlg.width, dlg.height) = dlg.size.landscape();
                        }
                        ui.label("Width");
                        ui.add_enabled(
                            custom,
                            ExprValue::new(&mut dlg.width)
                                .range(50.0..=5000.0)
                                .suffix(" mm"),
                        );
                        ui.end_row();
                        ui.label("Height");
                        ui.add_enabled(
                            custom,
                            ExprValue::new(&mut dlg.height)
                                .range(50.0..=5000.0)
                                .suffix(" mm"),
                        );
                        ui.end_row();
                        if custom {
                            dlg.size = SheetSize::Custom {
                                width: dlg.width,
                                height: dlg.height,
                            };
                        }
                        ui.label("Orientation");
                        ui.horizontal(|ui| {
                            ui.radio_value(
                                &mut dlg.orientation,
                                Orientation::Landscape,
                                "Landscape",
                            );
                            ui.radio_value(&mut dlg.orientation, Orientation::Portrait, "Portrait");
                        });
                        ui.end_row();
                        ui.label("Revision");
                        ui.text_edit_singleline(&mut dlg.revision);
                        ui.end_row();
                    });
                ui.add_space(4.0);
                ui.checkbox(&mut dlg.border, "Border");
                ui.checkbox(&mut dlg.title_block, "Title Block");
                ui.checkbox(&mut dlg.exclude_count, "Exclude from count");
                ui.checkbox(&mut dlg.exclude_print, "Exclude from printing");
                ui.add_enabled(false, egui::Label::new("Title block location: lower right"))
                    .on_disabled_hover_text("Other corners: planned (spec 4 §3.1)");
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() || super::keys::panel_ok_key(ui, false) {
                        ok = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        if cancel || !open {
            self.drw.sheet_dialog = None;
            return;
        }
        if ok {
            let d2 = dlg.clone();
            self.edit_drawing("Edit Sheet", |d| {
                if let Some(s) = d.sheets.get_mut(d2.index) {
                    s.name = d2.name.trim().to_string();
                    s.size = d2.size;
                    s.orientation = d2.orientation;
                    s.revision = d2.revision.clone();
                    s.exclude_from_count = d2.exclude_count;
                    s.exclude_from_printing = d2.exclude_print;
                    s.border = d2.border;
                    s.title_block = d2.title_block;
                }
            });
            self.drw.sheet_dialog = None;
            self.drw.cam.fit = true;
            return;
        }
        self.drw.sheet_dialog = Some(dlg);
    }

    // ---- browser and properties ------------------------------------------------------

    /// The browser tree of the active drawing.
    pub(super) fn drawing_tree(&mut self, ui: &mut egui::Ui, name: &str) {
        let Some(d) = self.drawing() else {
            return;
        };
        let active = d.active_sheet;
        let sheets: Vec<(String, String, Vec<TreeView>)> = d
            .sheets
            .iter()
            .map(|s| {
                let views = s
                    .views
                    .iter()
                    .map(|v| {
                        let model = d
                            .model_of(s, v.id)
                            .map_or_else(String::new, |m| self.model_name(m));
                        (v.id, v.identifier.clone(), v.parent(), model)
                    })
                    .collect();
                (s.name.clone(), s.size.label(), views)
            })
            .collect();
        let mut activate = None;
        let mut edit_sheet = None;
        let mut delete_sheet = None;
        let mut new_sheet = false;
        let mut select = None;
        let mut edit_view = None;
        let mut delete_view = None;
        ui.horizontal(|ui| {
            super::panels::tree_icon(ui, Icon::Drawing);
            ui.label(RichText::new(name).strong());
        });
        ui.indent("drw_tree", |ui| {
            egui::CollapsingHeader::new("Drawing Resources")
                .id_salt("drw_resources")
                .default_open(false)
                .show(ui, |ui| {
                    for r in [
                        "Sheet Formats",
                        "Borders",
                        "Title Blocks",
                        "Sketched Symbols",
                    ] {
                        ui.horizontal(|ui| {
                            super::panels::tree_icon(ui, Icon::Folder);
                            ui.weak(r).on_hover_text(
                                "Editing resources: planned (spec 4 §3.1, stage D5)",
                            );
                        });
                    }
                });
            for (i, (sname, size, views)) in sheets.iter().enumerate() {
                let label = RichText::new(format!("{sname} ({size})"));
                let label = if i == active { label.strong() } else { label };
                let header = egui::CollapsingHeader::new(label)
                    .id_salt(("drw_sheet", i))
                    .default_open(i == active)
                    .show(ui, |ui| {
                        // Base views, then the views projected from each.
                        let mut stack: Vec<(u32, usize)> = views
                            .iter()
                            .filter(|v| v.2.is_none())
                            .rev()
                            .map(|v| (v.0, 0))
                            .collect();
                        let mut seen = HashSet::new();
                        while let Some((id, depth)) = stack.pop() {
                            if !seen.insert(id) {
                                continue;
                            }
                            let Some(v) = views.iter().find(|v| v.0 == id) else {
                                continue;
                            };
                            ui.horizontal(|ui| {
                                ui.add_space(depth as f32 * 14.0);
                                let icon = if v.2.is_none() {
                                    Icon::BaseView
                                } else {
                                    Icon::ProjectedView
                                };
                                super::panels::tree_icon(ui, icon);
                                let text = format!("{}: {}", v.1, v.3);
                                let r = ui.selectable_label(
                                    i == active && self.drw.sel == Some(id),
                                    text,
                                );
                                if r.double_clicked() {
                                    edit_view = Some((i, id));
                                } else if r.clicked() {
                                    select = Some((i, id));
                                }
                                r.context_menu(|ui| {
                                    if ui.button("Edit View...").clicked() {
                                        edit_view = Some((i, id));
                                        ui.close();
                                    }
                                    if ui.button("Delete").clicked() {
                                        delete_view = Some((i, id));
                                        ui.close();
                                    }
                                });
                            });
                            for c in views.iter().filter(|c| c.2 == Some(id)).rev() {
                                stack.push((c.0, depth + 1));
                            }
                        }
                    });
                if header.header_response.clicked() && i != active {
                    activate = Some(i);
                }
                header.header_response.context_menu(|ui| {
                    if ui.button("Activate").clicked() {
                        activate = Some(i);
                        ui.close();
                    }
                    if ui.button("Edit Sheet...").clicked() {
                        edit_sheet = Some(i);
                        ui.close();
                    }
                    if ui
                        .add_enabled(sheets.len() > 1, egui::Button::new("Delete Sheet"))
                        .clicked()
                    {
                        delete_sheet = Some(i);
                        ui.close();
                    }
                    if ui.button("New Sheet").clicked() {
                        new_sheet = true;
                        ui.close();
                    }
                });
            }
        });
        let switch = |app: &mut Self, i: usize| {
            if let Some(d) = app.drawing_mut() {
                d.active_sheet = i.min(d.sheets.len().saturating_sub(1));
            }
            app.drw.sel = None;
            app.drw.tool = None;
        };
        if let Some(i) = activate {
            switch(self, i);
        }
        if let Some((i, id)) = select {
            switch(self, i);
            self.drw.sel = Some(id);
        }
        if let Some((i, id)) = edit_view {
            switch(self, i);
            self.drw.sel = Some(id);
            self.open_edit_view(id);
        }
        if let Some((i, id)) = delete_view {
            switch(self, i);
            self.delete_drawing_view(id);
        }
        if let Some(i) = edit_sheet {
            self.open_sheet_dialog(i);
        }
        if let Some(i) = delete_sheet {
            let deleted = self.edit_drawing("Delete Sheet", |d| d.delete_sheet(i));
            if deleted != Some(true) {
                self.history.cancel();
            }
            self.drw.sel = None;
        }
        if new_sheet {
            self.drw_apply(DrwCmd::NewSheet);
        }
    }

    /// Properties of the selected view, else of the active sheet.
    pub(super) fn drawing_properties(&mut self, ui: &mut egui::Ui) {
        let Some(d) = self.drawing() else {
            return;
        };
        let Some(sheet) = d.sheet() else {
            return;
        };
        let label_w = super::panels::label_width(ui.ctx());
        let view = self.drw.sel.and_then(|id| sheet.view(id)).cloned();
        let Some(v) = view else {
            let (w, h) = sheet.dims();
            let (n, m) = d.sheet_number(d.active_sheet);
            egui::Grid::new("drw_sheet_props")
                .num_columns(2)
                .min_col_width(label_w)
                .show(ui, |ui| {
                    super::panels::plabel(ui, "Sheet");
                    ui.label(&sheet.name);
                    ui.end_row();
                    super::panels::plabel(ui, "Size");
                    ui.label(format!("{} ({w:.0} × {h:.0} mm)", sheet.size.label()));
                    ui.end_row();
                    super::panels::plabel(ui, "Standard");
                    ui.label(format!(
                        "{} ({})",
                        d.standard.kind.label(),
                        match d.standard.projection {
                            drw::Projection::FirstAngle => "first angle",
                            drw::Projection::ThirdAngle => "third angle",
                        }
                    ));
                    ui.end_row();
                    super::panels::plabel(ui, "Number");
                    ui.label(if n == 0 {
                        "not counted".to_string()
                    } else {
                        format!("{n} of {m}")
                    });
                    ui.end_row();
                    super::panels::plabel(ui, "Views");
                    ui.label(sheet.views.len().to_string());
                    ui.end_row();
                });
            ui.add_space(6.0);
            if ui.button("Edit Sheet...").clicked() {
                self.drw_apply(DrwCmd::EditSheet);
            }
            return;
        };
        let kind = match v.kind {
            ViewKind::Base { .. } => "Base View".to_string(),
            ViewKind::Projected { dir, .. } => format!(
                "Projected View ({})",
                match dir {
                    ProjectDir::Up => "above",
                    ProjectDir::Down => "below",
                    ProjectDir::Left => "left",
                    ProjectDir::Right => "right",
                    ProjectDir::Iso(..) => "isometric",
                }
            ),
        };
        let model = d
            .model_of(sheet, v.id)
            .map_or_else(String::new, |m| self.model_name(m));
        let parent = v.parent();
        let mut nv = v.clone();
        egui::Grid::new("drw_view_props")
            .num_columns(2)
            .min_col_width(label_w)
            .show(ui, |ui| {
                super::panels::plabel(ui, "View");
                ui.text_edit_singleline(&mut nv.identifier);
                ui.end_row();
                super::panels::plabel(ui, "Type");
                ui.label(kind);
                ui.end_row();
                super::panels::plabel(ui, "Model");
                ui.label(model);
                ui.end_row();
                if let ViewKind::Base { orientation, .. } = &mut nv.kind {
                    super::panels::plabel(ui, "Orientation");
                    egui::ComboBox::from_id_salt("drw_orient")
                        .selected_text(orientation.label())
                        .show_ui(ui, |ui| {
                            for o in ViewOrientation::ALL {
                                ui.selectable_value(orientation, o, o.label());
                            }
                        });
                    ui.end_row();
                }
                // One narrow value per row, so the panel keeps its width.
                let from_scale = parent.is_some() && nv.scale_from_base;
                super::panels::plabel(ui, "Scale");
                ui.add_enabled_ui(!from_scale, |ui| {
                    egui::ComboBox::from_id_salt("drw_scale_prop")
                        .selected_text(drw::scale_text(nv.scale))
                        .show_ui(ui, |ui| {
                            for s in drw::SCALES {
                                ui.selectable_value(&mut nv.scale, s, drw::scale_text(s));
                            }
                        });
                });
                ui.end_row();
                super::panels::plabel(ui, "Scale value");
                ui.add_enabled(
                    !from_scale,
                    ExprValue::new(&mut nv.scale).range(0.001..=1000.0),
                );
                ui.end_row();
                if parent.is_some() {
                    super::panels::plabel(ui, "Scale from Base");
                    ui.checkbox(&mut nv.scale_from_base, "");
                    ui.end_row();
                }
                super::panels::plabel(ui, "Style");
                ui.add_enabled_ui(!(parent.is_some() && nv.style_from_base), |ui| {
                    egui::ComboBox::from_id_salt("drw_style_prop")
                        .selected_text(nv.style.label())
                        .show_ui(ui, |ui| {
                            for s in ViewStyle::ALL {
                                ui.selectable_value(&mut nv.style, s, s.label());
                            }
                        });
                });
                ui.end_row();
                if parent.is_some() {
                    super::panels::plabel(ui, "Style from Base");
                    ui.checkbox(&mut nv.style_from_base, "");
                    ui.end_row();
                }
                super::panels::plabel(ui, "Label visible");
                ui.checkbox(&mut nv.label_visible, "");
                ui.end_row();
                super::panels::plabel(ui, "Position X");
                ui.add(ExprValue::new(&mut nv.position[0]).suffix(" mm"));
                ui.end_row();
                super::panels::plabel(ui, "Position Y");
                ui.add(ExprValue::new(&mut nv.position[1]).suffix(" mm"));
                ui.end_row();
            });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.button("Edit View...").clicked() {
                self.open_edit_view(v.id);
            }
            if ui.button("Delete").clicked() {
                self.delete_drawing_view(v.id);
            }
        });
        if nv != v {
            let id = v.id;
            let moved = nv.position != v.position;
            let to = nv.position;
            self.edit_drawing("Edit View", move |d| {
                if let Some(s) = d.sheet_mut()
                    && let Some(x) = s.view_mut(id)
                {
                    nv.position = x.position;
                    *x = nv;
                }
                if moved {
                    d.move_view(id, to);
                }
            });
        }
    }
}

/// Sheet positions are kept to 0.1 mm (readable in Properties).
fn snap(p: [f64; 2]) -> [f64; 2] {
    p.map(|v| (v * 10.0).round() / 10.0)
}

// ---- dialog tabs ---------------------------------------------------------------------

/// A disabled check box with the reason in its tooltip.
fn planned_check(ui: &mut egui::Ui, text: &str, why: &str) {
    let mut v = false;
    ui.add_enabled(false, egui::Checkbox::new(&mut v, text))
        .on_disabled_hover_text(why);
}

fn scale_combo(ui: &mut egui::Ui, id: &str, scale: &mut f64) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(drw::scale_text(*scale))
        .width(70.0)
        .show_ui(ui, |ui| {
            for s in drw::SCALES {
                ui.selectable_value(scale, s, drw::scale_text(s));
            }
        });
    ui.add(ExprValue::new(scale).range(0.001..=1000.0))
        .on_hover_text("Any scale as a number (0.5 = 1 : 2)");
}

fn style_buttons(ui: &mut egui::Ui, style: &mut ViewStyle) {
    for s in ViewStyle::ALL {
        let r = ui.selectable_label(*style == s, s.label());
        if r.clicked() {
            *style = s;
        }
        if s == ViewStyle::Shaded {
            r.on_hover_text(
                "Shaded: drawn as Hidden Line Removed for now (shading planned, spec 4 §3.2)",
            );
        }
    }
}

fn component_tab(
    ui: &mut egui::Ui,
    d: &mut ViewDialog,
    models: &[(DocRef, String)],
    browse: &mut bool,
) {
    let why = "Planned (spec 4 §3.2)";
    ui.columns(2, |cols| {
        let ui = &mut cols[0];
        ui.label(RichText::new("File").strong());
        ui.horizontal(|ui| {
            let name = d
                .model
                .and_then(|m| models.iter().find(|x| x.0 == m))
                .map_or("(choose a part)", |x| x.1.as_str())
                .to_string();
            ui.add_enabled_ui(d.edit.is_none() || !d.projected, |ui| {
                egui::ComboBox::from_id_salt("drw_file")
                    .selected_text(name)
                    .width(170.0)
                    .show_ui(ui, |ui| {
                        for (r, n) in models {
                            if ui.selectable_label(d.model == Some(*r), n).clicked() {
                                d.model = Some(*r);
                                d.auto_scale = d.edit.is_none();
                            }
                        }
                    });
                if ui
                    .button("Browse...")
                    .on_hover_text(
                        "Link a part file (the drawing is saved first: the link is relative to it)",
                    )
                    .clicked()
                {
                    *browse = true;
                }
            });
        });
        ui.add_space(4.0);
        ui.label(RichText::new("Representation").strong());
        ui.add_enabled_ui(false, |ui| {
            ui.horizontal(|ui| {
                ui.label("Model State");
                egui::ComboBox::from_id_salt("drw_state")
                    .selected_text("[Primary]")
                    .show_ui(ui, |_| {});
            });
            ui.horizontal(|ui| {
                ui.label("Design View");
                egui::ComboBox::from_id_salt("drw_dv")
                    .selected_text("Default")
                    .show_ui(ui, |_| {});
            });
        })
        .response
        .on_disabled_hover_text("Model states and design views: planned (spec 4 §3.2)");
        planned_check(ui, "Camera View", why);
        planned_check(ui, "Include 3D Annotations", why);
        ui.add_space(4.0);
        ui.label(RichText::new("Orientation").strong());
        if d.projected {
            ui.weak("From the parent view");
        } else {
            egui::Grid::new("drw_orient_grid")
                .num_columns(2)
                .show(ui, |ui| {
                    for (k, o) in ViewOrientation::ALL.into_iter().enumerate() {
                        ui.radio_value(&mut d.orientation, o, o.label());
                        if k % 2 == 1 {
                            ui.end_row();
                        }
                    }
                });
        }
        let ui = &mut cols[1];
        ui.label(RichText::new("Style").strong());
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled_ui(!(d.projected && d.style_from_base), |ui| {
                style_buttons(ui, &mut d.style)
            });
        });
        if d.projected {
            ui.checkbox(&mut d.style_from_base, "Style from Base");
        }
        planned_check(
            ui,
            "Raster View",
            "Raster views: planned for large assemblies (spec 4 §3.2)",
        );
        ui.add_space(4.0);
        ui.label(RichText::new("Scale").strong());
        ui.horizontal(|ui| {
            ui.add_enabled_ui(!(d.projected && d.scale_from_base), |ui| {
                let before = d.scale;
                scale_combo(ui, "drw_scale_dlg", &mut d.scale);
                if d.scale != before {
                    d.auto_scale = false;
                }
            });
        });
        if d.projected {
            ui.checkbox(&mut d.scale_from_base, "Scale from Base");
        } else if d.auto_scale {
            ui.weak("Fitted to the sheet until you choose one");
        }
        ui.add_space(4.0);
        ui.label(RichText::new("Label").strong());
        ui.horizontal(|ui| {
            ui.label("View Identifier");
            ui.add(egui::TextEdit::singleline(&mut d.identifier).desired_width(90.0));
        });
        ui.checkbox(&mut d.label_visible, "Show label");
        ui.add_space(4.0);
        planned_check(ui, "Hide small parts", "Assemblies: planned (spec 4 §3.2)");
    });
}

fn model_tab(ui: &mut egui::Ui) {
    ui.label("Options of iParts / iAssemblies, reference data and weldments.");
    ui.add_enabled_ui(false, |ui| {
        ui.horizontal(|ui| {
            ui.label("Member");
            egui::ComboBox::from_id_salt("drw_member")
                .selected_text("-")
                .show_ui(ui, |_| {});
        });
        ui.label(RichText::new("Reference Data").strong());
        ui.horizontal(|ui| {
            ui.label("Hidden Line Calculation");
            egui::ComboBox::from_id_salt("drw_refhlr")
                .selected_text("All Bodies")
                .show_ui(ui, |_| {});
        });
        ui.horizontal(|ui| {
            ui.label("Margin");
            ui.label("0 mm");
        });
        ui.label(RichText::new("Weldment").strong());
        let mut v = false;
        ui.checkbox(&mut v, "Model Welding Symbols");
        ui.checkbox(&mut v, "Weld Annotations");
    })
    .response
    .on_disabled_hover_text("Planned: iParts, reference data and weldments (spec 4 §3.2)");
}

fn display_tab(ui: &mut egui::Ui, d: &mut ViewDialog) {
    let why = "Planned (spec 4 §3.2)";
    ui.columns(2, |cols| {
        let ui = &mut cols[0];
        ui.label(RichText::new("Standard Parts").strong());
        ui.add_enabled_ui(false, |ui| {
            ui.horizontal(|ui| {
                ui.label("Hidden Lines");
                egui::ComboBox::from_id_salt("drw_std_hidden")
                    .selected_text("Obey Browser")
                    .show_ui(ui, |_| {});
            });
            ui.horizontal(|ui| {
                ui.label("Section");
                egui::ComboBox::from_id_salt("drw_std_section")
                    .selected_text("Obey Browser")
                    .show_ui(ui, |_| {});
            });
        })
        .response
        .on_disabled_hover_text("Assemblies: planned (spec 4 §3.2)");
        planned_check(
            ui,
            "Cut Inheritance",
            "With section and break views (stage D2)",
        );
        planned_check(
            ui,
            "Thread Feature",
            "Cosmetic threads in views: planned (stage D3)",
        );
        let mut te = d.display.tangent_edges;
        ui.add_enabled(false, egui::Checkbox::new(&mut te, "Tangent Edges"))
            .on_disabled_hover_text("Tangent edges are always shown for now (stage D1 follow-up)");
        planned_check(ui, "Foreshortened", why);
        planned_check(ui, "Interference Edges", "Assemblies: planned (stage D4)");
        let ui = &mut cols[1];
        ui.add_enabled(
            d.projected,
            egui::Checkbox::new(&mut d.display.align_to_base, "Align to Base"),
        )
        .on_disabled_hover_text("Projected views only");
        planned_check(
            ui,
            "Definition in Base View",
            "With section and detail views (stage D2)",
        );
        let mut ob = true;
        ui.add_enabled(false, egui::Checkbox::new(&mut ob, "Orientation from Base"))
            .on_disabled_hover_text("Projected views always take the parent's orientation for now");
        planned_check(ui, "Hatching", "Section views (stage D2)");
        ui.add_space(4.0);
        ui.add_enabled_ui(false, |ui| {
            ui.horizontal(|ui| {
                let mut c = d.display.center_justified;
                ui.radio_value(&mut c, true, "Centered");
                ui.radio_value(&mut c, false, "Fixed");
            });
        })
        .response
        .on_disabled_hover_text("Views stay centred on their geometry for now");
    });
}

fn recovery_tab(ui: &mut egui::Ui, _d: &mut ViewDialog) {
    planned_check(
        ui,
        "Include Surface Bodies",
        "Surface bodies in views: planned (spec 4 §3.2)",
    );
    planned_check(ui, "Include Mesh Bodies", "Planned (spec 4 §3.2)");
    planned_check(
        ui,
        "All Model Dimensions",
        "Retrieve model dimensions: stage D3",
    );
    planned_check(
        ui,
        "Use Work Features",
        "Work features as reference lines: planned",
    );
}

// ---- painting -------------------------------------------------------------------------

fn dashed_rect(p: &egui::Painter, a: Pos2, b: Pos2, c: Color32) {
    let r = Rect::from_two_pos(a, b);
    let pts = [
        r.left_top(),
        r.right_top(),
        r.right_bottom(),
        r.left_bottom(),
        r.left_top(),
    ];
    p.extend(Shape::dashed_line(&pts, Stroke::new(1.2, c), 6.0, 4.0));
}

fn draw_view(p: &egui::Painter, cam: &SheetCam, rect: Rect, v: &ShownView) {
    draw_view_colored(p, cam, rect, v, INK);
}

/// A view's lines: visible solid (0.5 mm), hidden dashed (0.35 mm) when the
/// style shows them.
fn draw_view_colored(p: &egui::Painter, cam: &SheetCam, rect: Rect, v: &ShownView, ink: Color32) {
    let Some(lines) = &v.lines else {
        return;
    };
    let c = lines.center();
    let map = |q: &[f64; 2]| {
        cam.to_screen(
            rect,
            [
                v.position[0] + (q[0] - c[0]) * v.scale,
                v.position[1] + (q[1] - c[1]) * v.scale,
            ],
        )
    };
    let w = ((0.5 * cam.zoom) as f32).clamp(1.0, 4.0);
    let hw = ((0.35 * cam.zoom) as f32).clamp(0.8, 3.0);
    if v.style == ViewStyle::HiddenShown {
        let hidden = if ink == INK {
            HIDDEN_INK
        } else {
            ink.gamma_multiply(0.7)
        };
        let dash = ((3.0 * cam.zoom) as f32).clamp(3.0, 40.0);
        let gap = ((1.5 * cam.zoom) as f32).clamp(2.0, 20.0);
        for l in &lines.hidden {
            let pts: Vec<Pos2> = l.iter().map(map).collect();
            p.extend(Shape::dashed_line(&pts, Stroke::new(hw, hidden), dash, gap));
        }
    }
    for l in &lines.visible {
        let pts: Vec<Pos2> = l.iter().map(map).collect();
        p.add(Shape::line(pts, Stroke::new(w, ink)));
    }
}
