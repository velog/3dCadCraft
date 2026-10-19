//! Selection in the idle 3D view (`docs/spec/0.9-selection.md`): Inventor's
//! selection modes (Select Faces and Edges / Features / Bodies / Sketch
//! Features, Quick Access Toolbar), multiple selection with Ctrl/Shift,
//! window / crossing boxes, Select Tangencies (double click), Select All,
//! Invert Selection and Previous Selection.

use eframe::egui::{self, Color32};

use super::{CadCraftApp, overlay};
use crate::kernel::{EdgeId, FaceId};
use crate::part::select as psel;

/// What a click in the idle view selects (Inventor: the Select drop-down
/// of the Quick Access Toolbar, part environment).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum SelectMode {
    /// Faces and edges (the default).
    #[default]
    FacesEdges,
    /// Whole features: a click on a face selects the feature that made it.
    Features,
    /// Whole bodies (solids and surfaces).
    Bodies,
    /// Sketches: a click on a visible sketch's curve selects the sketch.
    SketchFeatures,
}

impl SelectMode {
    pub(super) const ALL: [SelectMode; 4] = [
        SelectMode::FacesEdges,
        SelectMode::Features,
        SelectMode::Bodies,
        SelectMode::SketchFeatures,
    ];

    pub(super) fn label(self) -> &'static str {
        match self {
            SelectMode::FacesEdges => "Select Faces and Edges",
            SelectMode::Features => "Select Features",
            SelectMode::Bodies => "Select Bodies",
            SelectMode::SketchFeatures => "Select Sketch Features",
        }
    }
}

/// One selected object. Faces, features and bodies by persistent name;
/// edges by id (the set is cleared when the body is rebuilt).
#[derive(Clone, Debug, PartialEq)]
pub(super) enum SelItem {
    Face(String),
    Edge(EdgeId),
    Feature(String),
    Body(String),
    Sketch(usize),
}

impl CadCraftApp {
    /// The object of the current mode under the cursor, from the edge or
    /// face found there (or a sketch curve in Sketch Features mode).
    pub(super) fn sel_item_at(
        &self,
        p: egui::Pos2,
        rect: egui::Rect,
        edge: Option<EdgeId>,
        face: Option<&str>,
    ) -> Option<SelItem> {
        let body = &self.part().body;
        let face_id = face.and_then(|n| body.find_face(n));
        let edge_face = || edge.and_then(|e| self.picker().faces_of(e).into_iter().next());
        match self.select_mode {
            SelectMode::FacesEdges => edge
                .map(SelItem::Edge)
                .or_else(|| face.map(|f| SelItem::Face(f.to_owned()))),
            SelectMode::Features => {
                if let Some(id) = self.work_feature_under(p, rect) {
                    return Some(SelItem::Feature(id));
                }
                let f = face_id.or_else(edge_face)?;
                let name = body.face(f)?.name.clone()?;
                psel::feature_id_of_face(&name).map(|id| SelItem::Feature(id.to_owned()))
            }
            SelectMode::Bodies => {
                let f = face_id.or_else(edge_face)?;
                psel::body_of_face(body, f).map(SelItem::Body)
            }
            SelectMode::SketchFeatures => self.sketch_under(p, rect).map(SelItem::Sketch),
        }
    }

    /// A visible sketch with a curve under the cursor.
    fn sketch_under(&self, p: egui::Pos2, rect: egui::Rect) -> Option<usize> {
        self.part().sketches.iter().enumerate().find_map(|(i, ps)| {
            (ps.visible
                && overlay::pick_entity(&self.camera, rect, &ps.sketch, p, overlay::CURVE_SNAP_PX)
                    .is_some())
            .then_some(i)
        })
    }

    /// Faces that light up for an item.
    pub(super) fn item_faces(&self, item: &SelItem) -> Vec<FaceId> {
        let body = &self.part().body;
        match item {
            SelItem::Face(n) => body.find_face(n).into_iter().collect(),
            SelItem::Feature(id) => psel::faces_of_feature(body, id),
            SelItem::Body(n) => psel::faces_of_body(body, n),
            SelItem::Edge(_) | SelItem::Sketch(_) => Vec::new(),
        }
    }

    /// Click in the idle view: no modifier replaces the selection, Shift
    /// adds, Ctrl toggles; a click on nothing clears it (without Ctrl/Shift).
    pub(super) fn sel_click(&mut self, item: Option<SelItem>, add: bool, toggle: bool) {
        if !add && !toggle {
            self.remember_selection();
            self.sel.clear();
        }
        let Some(item) = item else {
            return;
        };
        match self.sel.iter().position(|x| *x == item) {
            Some(k) if toggle => {
                self.sel.remove(k);
            }
            Some(_) => {}
            None => self.sel.push(item),
        }
    }

    /// Keeps the current set for Previous Selection.
    fn remember_selection(&mut self) {
        if !self.sel.is_empty() {
            self.prev_sel = self.sel.clone();
        }
    }

    /// Everything of the current mode.
    fn all_items(&self) -> Vec<SelItem> {
        let part = self.part();
        let body = &part.body;
        match self.select_mode {
            SelectMode::FacesEdges => body
                .faces()
                .filter_map(|(_, f)| f.name.clone().map(SelItem::Face))
                .collect(),
            SelectMode::Features => part
                .features
                .iter()
                .map(|f| SelItem::Feature(f.id().to_owned()))
                .collect(),
            SelectMode::Bodies => body
                .solids()
                .filter_map(|(_, s)| s.name.clone().map(SelItem::Body))
                .collect(),
            SelectMode::SketchFeatures => (0..part.sketches.len())
                .filter(|&i| part.sketches[i].visible)
                .map(SelItem::Sketch)
                .collect(),
        }
    }

    /// Ctrl+A (Inventor: Select All of the active mode).
    pub(super) fn select_all(&mut self) {
        self.remember_selection();
        self.sel = self.all_items();
    }

    /// Invert Selection: everything of the current mode that is not selected.
    pub(super) fn invert_selection(&mut self) {
        self.remember_selection();
        let old = std::mem::take(&mut self.sel);
        self.sel = self
            .all_items()
            .into_iter()
            .filter(|x| !old.contains(x))
            .collect();
    }

    /// Previous Selection: the set before the last change (one is kept).
    pub(super) fn previous_selection(&mut self) {
        let prev = std::mem::take(&mut self.prev_sel);
        self.prev_sel = std::mem::replace(&mut self.sel, prev);
    }

    /// Select Tangencies of an edge or a face (double click): adds the
    /// tangent chain of edges or the tangent faces.
    pub(super) fn select_tangencies(&mut self, item: &SelItem) {
        let body = &self.part().body;
        let more: Vec<SelItem> = match item {
            SelItem::Edge(e) => psel::tangent_edges(body, *e)
                .into_iter()
                .map(SelItem::Edge)
                .collect(),
            SelItem::Face(n) => match body.find_face(n) {
                Some(f) => psel::tangent_faces(body, f)
                    .into_iter()
                    .filter_map(|f| body.face(f).and_then(|x| x.name.clone()))
                    .map(SelItem::Face)
                    .collect(),
                None => Vec::new(),
            },
            _ => Vec::new(),
        };
        for x in more {
            if !self.sel.contains(&x) {
                self.sel.push(x);
            }
        }
    }

    /// Objects of the current mode taken by a box from `a` to `b`: window
    /// (left → right) takes those fully inside, crossing (right → left) also
    /// those it touches.
    pub(super) fn box_items(&self, rect: egui::Rect, a: egui::Pos2, b: egui::Pos2) -> Vec<SelItem> {
        let r = egui::Rect::from_two_pos(a, b);
        let window = b.x >= a.x;
        let taken = |pts: &[egui::Pos2]| {
            !pts.is_empty()
                && if window {
                    pts.iter().all(|p| r.contains(*p))
                } else {
                    pts.iter().any(|p| r.contains(*p))
                }
        };
        let picker = self.picker();
        let face_pts = |faces: &[FaceId]| -> Vec<egui::Pos2> {
            picker
                .tris
                .iter()
                .filter(|(f, _)| faces.contains(f))
                .flat_map(|(_, t)| t.iter())
                .filter_map(|q| self.camera.project(q, rect))
                .collect()
        };
        let mut out = Vec::new();
        if self.select_mode == SelectMode::FacesEdges {
            for (e, pts) in self.edge_polylines().iter() {
                let s: Vec<_> = pts
                    .iter()
                    .filter_map(|q| self.camera.project(q, rect))
                    .collect();
                if taken(&s) {
                    out.push(SelItem::Edge(*e));
                }
            }
        }
        if self.select_mode == SelectMode::SketchFeatures {
            for (i, ps) in self.part().sketches.iter().enumerate() {
                if !ps.visible {
                    continue;
                }
                let sk = &ps.sketch;
                let s: Vec<_> = sk
                    .entities()
                    .flat_map(|(id, _)| overlay::polyline(sk, id))
                    .filter_map(|q| self.camera.project(&sk.plane.point_at(q.x, q.y), rect))
                    .collect();
                if taken(&s) {
                    out.push(SelItem::Sketch(i));
                }
            }
            return out;
        }
        for item in self.all_items() {
            if taken(&face_pts(&self.item_faces(&item))) {
                out.push(item);
            }
        }
        out
    }

    /// Applies a finished box: replaces, or adds with Shift, toggles with Ctrl.
    pub(super) fn sel_box(&mut self, items: Vec<SelItem>, add: bool, toggle: bool) {
        if !add && !toggle {
            self.remember_selection();
            self.sel.clear();
        }
        for x in items {
            match self.sel.iter().position(|y| *y == x) {
                Some(k) if toggle => {
                    self.sel.remove(k);
                }
                Some(_) => {}
                None => self.sel.push(x),
            }
        }
    }

    /// Draws the selected edges and sketches (blue) and the edge or sketch
    /// under the cursor (green); faces are coloured in the 3D scene
    /// (`build_scene`, with depth).
    pub(super) fn draw_selection(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        hover: Option<&SelItem>,
    ) {
        let polys = self.edge_polylines();
        // Edges are drawn on top of the view, without their hidden pieces.
        let line = |e: EdgeId, color: Color32| {
            if let Some((_, pts)) = polys.iter().find(|(id, _)| *id == e) {
                self.draw_visible_polyline(painter, rect, pts, egui::Stroke::new(4.0, color));
            }
        };
        let sketch = |i: usize, color: Color32| {
            if let Some(ps) = self.part().sketches.get(i) {
                let sk = &ps.sketch;
                for (id, _) in sk.entities() {
                    let s: Vec<_> = overlay::polyline(sk, id)
                        .iter()
                        .filter_map(|q| self.camera.project(&sk.plane.point_at(q.x, q.y), rect))
                        .collect();
                    painter.add(egui::Shape::line(s, egui::Stroke::new(3.0, color)));
                }
            }
        };
        let blue = Color32::from_rgb(40, 110, 230);
        let green = Color32::from_rgb(60, 190, 90);
        for x in &self.sel {
            match x {
                SelItem::Edge(e) => line(*e, blue),
                SelItem::Sketch(i) => sketch(*i, blue),
                _ => {}
            }
        }
        if self.prehighlight
            && let Some(h) = hover
            && !self.sel.contains(h)
        {
            match h {
                SelItem::Edge(e) => line(*e, green),
                SelItem::Sketch(i) => sketch(*i, green),
                _ => {}
            }
        }
    }

    /// Whether model point `q` is hidden behind a face (the eye ray through
    /// it hits a face clearly in front). Very large bodies are not tested.
    pub(super) fn point_hidden(&self, q: &crate::kernel::Point3, rect: egui::Rect) -> bool {
        let picker = self.picker();
        if picker.tris.len() >= 200_000 {
            return false;
        }
        let Some(s) = self.camera.project(q, rect) else {
            return true;
        };
        let Some((o, d)) = self.camera.ray(s, rect) else {
            return false;
        };
        let Some(d) = d.try_normalize(1e-12) else {
            return false;
        };
        let te = (q - o).dot(&d);
        picker
            .pick(&o, &d)
            .is_some_and(|(_, ft)| ft < te - 0.01 * te.abs().max(1.0))
    }

    /// Draws a model polyline on top of the view, its hidden pieces left
    /// out (each segment judged by its middle).
    pub(super) fn draw_visible_polyline(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        pts: &[crate::kernel::Point3],
        stroke: egui::Stroke,
    ) {
        for w in pts.windows(2) {
            if self.point_hidden(&(w[0] + (w[1] - w[0]) * 0.5), rect) {
                continue;
            }
            if let (Some(a), Some(b)) = (
                self.camera.project(&w[0], rect),
                self.camera.project(&w[1], rect),
            ) {
                painter.line_segment([a, b], stroke);
            }
        }
    }

    /// "3 faces, 1 edge" for the status bar.
    pub(super) fn selection_summary(&self) -> Option<String> {
        if self.sel.is_empty() {
            return None;
        }
        let count = |f: fn(&SelItem) -> bool| self.sel.iter().filter(|x| f(x)).count();
        let parts = [
            (count(|x| matches!(x, SelItem::Face(_))), "face", "faces"),
            (count(|x| matches!(x, SelItem::Edge(_))), "edge", "edges"),
            (
                count(|x| matches!(x, SelItem::Feature(_))),
                "feature",
                "features",
            ),
            (count(|x| matches!(x, SelItem::Body(_))), "body", "bodies"),
            (
                count(|x| matches!(x, SelItem::Sketch(_))),
                "sketch",
                "sketches",
            ),
        ];
        let text: Vec<String> = parts
            .iter()
            .filter(|(n, _, _)| *n > 0)
            .map(|(n, one, many)| format!("{n} {}", if *n == 1 { one } else { many }))
            .collect();
        Some(format!("Selected: {}", text.join(", ")))
    }

    /// The Select drop-down of the Quick Access Toolbar.
    pub(super) fn select_menu(&mut self, ui: &mut egui::Ui) {
        let label = format!(
            "Select: {}",
            self.select_mode.label().trim_start_matches("Select ")
        );
        ui.menu_button(label, |ui| {
            for m in SelectMode::ALL {
                if ui.radio(self.select_mode == m, m.label()).clicked() {
                    if self.select_mode != m {
                        self.remember_selection();
                        self.sel.clear();
                        self.face_sel = None;
                        self.edge_sel = None;
                    }
                    self.select_mode = m;
                    ui.close();
                }
            }
            ui.separator();
            ui.checkbox(&mut self.prehighlight, "Enable Prehighlight");
            ui.separator();
            if ui.button("Select All (Ctrl+A)").clicked() {
                self.select_all();
                ui.close();
            }
            if ui.button("Invert Selection").clicked() {
                self.invert_selection();
                ui.close();
            }
            if ui
                .add_enabled(
                    !self.prev_sel.is_empty(),
                    egui::Button::new("Previous Selection"),
                )
                .clicked()
            {
                self.previous_selection();
                ui.close();
            }
            let tangent = self
                .sel
                .last()
                .filter(|x| matches!(x, SelItem::Edge(_) | SelItem::Face(_)))
                .cloned();
            if ui
                .add_enabled(tangent.is_some(), egui::Button::new("Select Tangencies"))
                .on_hover_text("Also: double click a face or an edge")
                .clicked()
            {
                if let Some(t) = tangent {
                    self.select_tangencies(&t);
                }
                ui.close();
            }
        });
    }
}
