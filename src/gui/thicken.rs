//! **Thicken/Offset** (3D Model › Modify, `docs/spec/2.14-thicken-offset.md`):
//! the panel with Inventor's two tabs, selectors on the shared flow (faces or
//! quilts, the solid), the preview of the new edges, OK / Apply / Cancel and
//! Edit Feature. The geometry is `part::body_op` + `part/exact.rs::body_op`.

use eframe::egui::{self, RichText};

use super::CadCraftApp;
use super::command_ui::{RowEvents, selector_row};
use super::icons::Icon;
use super::panels::{plabel, psection, warning_box};
use super::select::SelItem;
use super::valuefield::ExprValue;
use crate::command::flow::{Flow, PickKind, Selector, colors};
use crate::kernel::Point3;
use crate::part::boolean::Operation;
use crate::part::{
    BodyOp, BodyOpFeature, Feature, ThickenDirection, ThickenOp, ThickenOutput, ThickenSelect,
};

/// The open Thicken/Offset command.
#[derive(Clone, Debug)]
pub(super) struct ThickenDraft {
    pub def: ThickenOp,
    /// Edit Feature of this feature index.
    pub edit: Option<usize>,
    /// The active selector (`faces`, `quilts`, `solids`).
    pub input: &'static str,
    /// The tab shown (0 Thicken/Offset, 1 More).
    pub tab: usize,
}

const FACES: &[PickKind] = &[PickKind::Face, PickKind::PlanarFace];
const QUILTS: &[PickKind] = &[PickKind::Surface];
const SOLIDS: &[PickKind] = &[PickKind::Body];

impl ThickenDraft {
    pub(super) fn new() -> Self {
        Self {
            def: ThickenOp::new(),
            edit: None,
            input: "faces",
            tab: 0,
        }
    }

    fn first_key(&self) -> &'static str {
        match self.def.select {
            ThickenSelect::Face => "faces",
            ThickenSelect::Quilt => "quilts",
        }
    }

    /// The selectors: Faces (or Quilts), then Solids.
    pub(super) fn flow(&self) -> Flow {
        let first = match self.def.select {
            ThickenSelect::Face => Selector {
                key: "faces",
                label: "Faces",
                prompt: "Select the faces to thicken or offset",
                accepts: FACES,
                single: false,
                required: true,
                count: self.def.faces.len(),
                color: colors::FACES,
            },
            ThickenSelect::Quilt => Selector {
                key: "quilts",
                label: "Quilts",
                prompt: "Select the surface bodies to thicken",
                accepts: QUILTS,
                single: false,
                required: true,
                count: self.def.quilts.len(),
                color: colors::FACES,
            },
        };
        let solids = Selector {
            key: "solids",
            label: "Solids",
            prompt: "Select the solid the result is combined with (multi-body parts)",
            accepts: SOLIDS,
            single: true,
            required: false,
            count: self.def.solids.len(),
            color: colors::PLANE,
        };
        let key = if self.input == "solids" {
            "solids"
        } else {
            self.first_key()
        };
        Flow::new(vec![first, solids], Some(key))
    }
}

impl CadCraftApp {
    /// **Thicken/Offset**: faces selected before are its faces (rule 17).
    pub(super) fn start_thicken(&mut self) {
        let pre = self.sel.clone();
        self.finish_sketch();
        self.mode = super::Mode::Normal;
        self.cancel_extrude();
        self.work_draft = None;
        self.blend_draft = None;
        self.face_sel = None;
        self.edge_sel = None;
        let mut d = ThickenDraft::new();
        for x in pre {
            self.thicken_take(&mut d, &x, None);
        }
        self.sel.clear();
        self.thicken_draft = Some(d);
    }

    /// Edit Feature of Thicken feature `k`.
    pub(super) fn thicken_edit(&mut self, k: usize) {
        let Some(Feature::BodyOp(BodyOpFeature {
            op: BodyOp::Thicken(t),
            ..
        })) = self.part().features.get(k).cloned()
        else {
            return;
        };
        self.roll_back_to(k);
        let mut d = ThickenDraft::new();
        d.input = match t.select {
            ThickenSelect::Face => "faces",
            ThickenSelect::Quilt => "quilts",
        };
        d.def = t;
        d.edit = Some(k);
        self.thicken_draft = Some(d);
    }

    /// Adds (or with a second pick removes) item `x` to the active selector;
    /// `at`: the click point on a face.
    fn thicken_take(&self, d: &mut ThickenDraft, x: &SelItem, at: Option<Point3>) {
        let body = &self.part().body;
        match (d.input, x) {
            ("faces", SelItem::Face(n)) => {
                let Some(f) = body.find_face(n) else {
                    return;
                };
                let Some(r) = crate::part::shell::face_ref(body, f, at) else {
                    return;
                };
                if let Some(i) = d.def.faces.iter().position(|q| q.face == r.face) {
                    d.def.faces.remove(i);
                    return;
                }
                d.def.faces.push(r);
                // Automatic Face Chain: the faces tangent to it come along.
                if d.def.automatic_face_chain {
                    for g in crate::part::select::tangent_faces(body, f) {
                        if let Some(q) = crate::part::shell::face_ref(body, g, None)
                            && !d.def.faces.iter().any(|x| x.face == q.face)
                        {
                            d.def.faces.push(q);
                        }
                    }
                }
            }
            ("quilts", SelItem::Body(n)) | ("quilts", SelItem::Face(n))
                if self
                    .body_name_of(x)
                    .is_some_and(|b| crate::part::select::body_is_surface(body, &b)) =>
            {
                let b = self.body_name_of(x).unwrap_or_else(|| n.clone());
                match d.def.quilts.iter().position(|q| *q == b) {
                    Some(i) => {
                        d.def.quilts.remove(i);
                    }
                    None => d.def.quilts.push(b),
                }
            }
            ("solids", _) => {
                if let Some(b) = self.body_name_of(x)
                    && !crate::part::select::body_is_surface(body, &b)
                {
                    d.def.solids = if d.def.solids.contains(&b) {
                        Vec::new()
                    } else {
                        vec![b]
                    };
                }
            }
            _ => {}
        }
    }

    /// The body an item belongs to.
    fn body_name_of(&self, x: &SelItem) -> Option<String> {
        let body = &self.part().body;
        match x {
            SelItem::Body(n) => Some(n.clone()),
            SelItem::Face(n) => body
                .find_face(n)
                .and_then(|f| crate::part::select::body_of_face(body, f)),
            _ => None,
        }
    }

    /// A click in the view while Thicken/Offset is open.
    pub(super) fn thicken_click(&mut self, p: egui::Pos2, rect: egui::Rect) {
        let Some(mut d) = self.thicken_draft.clone() else {
            return;
        };
        let edge = None;
        let face = match self.plane_under(p, rect, false) {
            Some(crate::part::PlaneRef::Face(n)) => Some(n),
            _ => None,
        };
        // The point on the face (Split pieces are told apart by it).
        let at = self.camera.ray(p, rect).and_then(|(o, dir)| {
            let dir = dir.try_normalize(1e-12)?;
            let (_, t) = self.picker().pick(&o, &dir)?;
            Some(o + dir * t)
        });
        // Quilts: the surface body whose face is under the cursor (the
        // shared pickers resolve a face to a solid body only).
        let quilt = (d.input == "quilts")
            .then(|| {
                let (o, dir) = self.camera.ray(p, rect)?;
                let (f, _) = self.picker().pick(&o, &dir.try_normalize(1e-12)?)?;
                let body = &self.part().body;
                crate::part::select::body_of_face(body, f)
                    .filter(|n| crate::part::select::body_is_surface(body, n))
            })
            .flatten()
            .map(SelItem::Body);
        let item = quilt.or_else(|| {
            self.sel_item_at(p, rect, edge, face.as_deref())
                .or_else(|| face.clone().map(SelItem::Face))
        });
        if let Some(x) = item {
            self.thicken_take(&mut d, &x, at);
            // The first selector done: on to the solid of a multi-body part.
            let solids = self.part().body.solids().filter(|(_, s)| !s.open).count();
            if d.input != "solids"
                && solids > 1
                && d.def.solids.is_empty()
                && d.def.faces.len() == 1
            {
                d.input = "solids";
            }
        }
        self.thicken_draft = Some(d);
    }

    /// A body row picked in the browser (Quilts or Solids selector).
    pub(super) fn thicken_pick_body(&mut self, name: String) {
        if let Some(mut d) = self.thicken_draft.clone() {
            self.thicken_take(&mut d, &SelItem::Body(name), None);
            self.thicken_draft = Some(d);
        }
    }

    /// Makes selector `key` active.
    pub(super) fn thicken_activate(&mut self, key: &str) {
        if let Some(d) = self.thicken_draft.as_mut() {
            d.input = match key {
                "solids" => "solids",
                "quilts" => "quilts",
                _ => "faces",
            };
        }
    }

    /// The new geometry's edges (worker thread), or why it fails.
    pub(super) fn thicken_preview(&self) -> Result<Vec<Vec<Point3>>, String> {
        let Some(d) = &self.thicken_draft else {
            return Err(String::new());
        };
        if d.def.validate().is_err() {
            return Err(String::new());
        }
        let key = format!("thicken|{:?}|{:?}", d.edit, d.def);
        if let Some(r) = self.preview_ready(&key) {
            return r;
        }
        let mut trial = self.part().clone();
        let (def, edit) = (d.def.clone(), d.edit);
        self.preview_async(key, move || {
            let f = BodyOpFeature::new(BodyOp::Thicken(def));
            let built = match edit {
                Some(k) => trial.replace_feature(k, Feature::BodyOp(f)),
                None => trial.add_body_op(f),
            };
            built
                .map(|id| {
                    let own = format!("{id}:");
                    super::combine_split::face_edges(&trial, |n| n.starts_with(&own))
                })
                .map_err(|e| e.to_string())
        })
    }

    /// OK (`keep_open` false) or Apply.
    pub(super) fn commit_thicken(&mut self, keep_open: bool) {
        let Some(d) = self.thicken_draft.clone() else {
            return;
        };
        let cmd = if d.edit.is_some() {
            "Edit Feature"
        } else {
            "Thicken/Offset"
        };
        self.checkpoint(cmd);
        let f = BodyOpFeature::new(BodyOp::Thicken(d.def.clone()));
        let built = match d.edit {
            Some(k) => self.part_mut().replace_feature(k, Feature::BodyOp(f)),
            None => self.part_mut().add_body_op(f),
        };
        if built.is_err()
            && let Some(k) = d.edit
        {
            self.part_mut().rollback = Some(k);
            let _ = self.part_mut().regenerate();
        }
        match built {
            Ok(_) => {
                let k = d.edit.unwrap_or(self.part().last_inserted());
                self.file_status = None;
                self.scene_dirty = true;
                if keep_open && d.edit.is_none() {
                    let mut next = ThickenDraft::new();
                    next.def = ThickenOp {
                        faces: Vec::new(),
                        quilts: Vec::new(),
                        solids: Vec::new(),
                        ..d.def
                    };
                    self.thicken_draft = Some(next);
                } else {
                    self.thicken_draft = None;
                    self.end_feature_edit();
                    self.selection = super::Selection::Feature(k);
                }
            }
            Err(e) => {
                self.history.cancel();
                self.file_status = Some(Err(format!("{cmd}: {e}")));
            }
        }
    }

    /// Closes the command (Cancel / Esc).
    pub(super) fn cancel_thicken(&mut self) {
        if self.thicken_draft.take().is_some() {
            self.end_feature_edit();
        }
    }

    /// The status-bar prompt.
    pub(super) fn thicken_prompt(&self) -> Option<String> {
        let d = self.thicken_draft.as_ref()?;
        let f = d.flow();
        let p = f.current().map_or("", |s| s.prompt);
        let ok = if d.def.validate().is_ok() {
            ", or OK (Enter)"
        } else {
            ""
        };
        Some(format!("Thicken/Offset: {p}{ok}"))
    }

    /// The panel (Properties).
    pub(super) fn thicken_panel(&mut self, ui: &mut egui::Ui) {
        let Some(mut d) = self.thicken_draft.clone() else {
            return;
        };
        self.panel_header(ui, Icon::Thicken, "Thicken/Offset");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut d.tab, 0, "Thicken/Offset");
            ui.selectable_value(&mut d.tab, 1, "More");
        });
        ui.separator();
        let mut ev = RowEvents::default();
        if d.tab == 0 {
            psection(ui, "thicken_input", "Input Geometry", |ui| {
                plabel(ui, "Select");
                ui.horizontal(|ui| {
                    for (v, l) in [
                        (ThickenSelect::Face, "Face"),
                        (ThickenSelect::Quilt, "Quilt"),
                    ] {
                        if ui.radio(d.def.select == v, l).clicked() && d.def.select != v {
                            d.def.select = v;
                            d.def.faces.clear();
                            d.def.quilts.clear();
                            d.input = d.first_key();
                        }
                    }
                });
                ui.end_row();
                let flow = d.flow();
                for (i, s) in flow.selectors.iter().enumerate() {
                    let text = match s.key {
                        "solids" => d.def.solids.first().cloned().unwrap_or_default(),
                        "quilts" if d.def.quilts.len() == 1 => d.def.quilts[0].clone(),
                        _ if s.count > 0 => format!("{} selected", s.count),
                        _ => String::new(),
                    };
                    selector_row(ui, s, flow.active == Some(i), &text, &mut ev);
                    ui.end_row();
                }
            });
            psection(ui, "thicken_behavior", "Behavior", |ui| {
                plabel(ui, "Output");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut d.def.output, ThickenOutput::Solid, "Solid");
                    ui.add_enabled_ui(d.def.select == ThickenSelect::Face, |ui| {
                        ui.radio_value(&mut d.def.output, ThickenOutput::Surface, "Surface")
                            .on_disabled_hover_text("An offset surface is made from faces");
                    });
                });
                ui.end_row();
                plabel(ui, "Operation");
                ui.add_enabled_ui(d.def.output == ThickenOutput::Solid, |ui| {
                    ui.vertical(|ui| {
                        for (op, l) in [
                            (Operation::Join, "Join"),
                            (Operation::Cut, "Cut"),
                            (Operation::Intersect, "Intersect"),
                            (Operation::NewSolid, "New Solid"),
                        ] {
                            ui.radio_value(&mut d.def.operation, op, l);
                        }
                    });
                });
                ui.end_row();
                plabel(ui, "Distance");
                ui.add(ExprValue::new(&mut d.def.distance).suffix(" mm"));
                ui.end_row();
                plabel(ui, "Direction");
                ui.vertical(|ui| {
                    ui.radio_value(
                        &mut d.def.direction,
                        ThickenDirection::Direction1,
                        "Direction 1",
                    )
                    .on_hover_text("Along the faces' outward normals");
                    ui.radio_value(
                        &mut d.def.direction,
                        ThickenDirection::Direction2,
                        "Direction 2",
                    )
                    .on_hover_text("Against them (into the solid)");
                    ui.add_enabled_ui(d.def.output == ThickenOutput::Solid, |ui| {
                        ui.radio_value(
                            &mut d.def.direction,
                            ThickenDirection::Symmetric,
                            "Symmetric",
                        )
                        .on_disabled_hover_text("An offset surface goes one way");
                    });
                });
                ui.end_row();
                plabel(ui, "");
                ui.checkbox(&mut d.def.automatic_face_chain, "Automatic Face Chain")
                    .on_hover_text("A picked face brings the faces tangent to it");
                ui.end_row();
                plabel(ui, "");
                ui.add_enabled(
                    false,
                    egui::Checkbox::new(&mut d.def.vertical_surfaces, "Create Vertical Surfaces"),
                )
                .on_disabled_hover_text("Spec 2.14 row Create Vertical Surfaces: not built yet");
                ui.end_row();
            });
        } else {
            psection(ui, "thicken_more", "More", |ui| {
                plabel(ui, "");
                ui.checkbox(&mut d.def.automatic_blending, "Automatic Blending")
                    .on_hover_text(
                        "Smooth neighbouring faces stay smooth (the kernel offsets them together)",
                    );
                ui.end_row();
                plabel(ui, "");
                let mut approx = true;
                ui.add_enabled(
                    false,
                    egui::Checkbox::new(&mut approx, "Allow Approximation"),
                )
                .on_disabled_hover_text(
                    "Free-form faces are always fitted within the tolerance below (spec 2.14)",
                );
                ui.end_row();
                plabel(ui, "Approximation");
                ui.add_enabled_ui(false, |ui| {
                    ui.vertical(|ui| {
                        let mut a = 0usize;
                        for (i, l) in ["Mean", "Never Too Thin", "Never Too Thick"]
                            .iter()
                            .enumerate()
                        {
                            ui.radio_value(&mut a, i, *l);
                        }
                    });
                })
                .response
                .on_disabled_hover_text("Spec 2.14: the kernel fits the mean offset");
                ui.end_row();
                plabel(ui, "Tolerance");
                ui.vertical(|ui| {
                    let mut specify = d.def.tolerance.is_some();
                    ui.radio_value(&mut specify, false, "Optimized")
                        .on_hover_text(
                            "The finest of 0.01 … 1 mm that fits, at most 2 % of the distance",
                        );
                    ui.radio_value(&mut specify, true, "Specify Tolerance")
                        .on_hover_text("How far a fitted free-form face may stray (mm)");
                    match (specify, d.def.tolerance) {
                        (false, Some(_)) => d.def.tolerance = None,
                        (true, None) => d.def.tolerance = Some(0.1),
                        _ => {}
                    }
                });
                ui.end_row();
                if let Some(tol) = d.def.tolerance.as_mut() {
                    plabel(ui, "");
                    ui.add(ExprValue::new(tol).suffix(" mm"));
                    ui.end_row();
                }
            });
        }
        if let Some(k) = ev.activate {
            d.input = match k {
                "solids" => "solids",
                "quilts" => "quilts",
                _ => "faces",
            };
        }
        if let Some(k) = ev.clear {
            match k {
                "solids" => d.def.solids.clear(),
                "quilts" => d.def.quilts.clear(),
                _ => d.def.faces.clear(),
            }
        }
        let editing = d.edit.is_some();
        self.thicken_draft = Some(d.clone());
        // The preview and the buttons.
        let ready = d.def.validate().is_ok();
        let preview = self.thicken_preview();
        match &preview {
            Err(e) if e == super::PREVIEW_PENDING => {
                ui.label(e);
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(50));
            }
            Err(e) if ready && !e.is_empty() => warning_box(ui, e),
            Err(_) if !ready => {
                if let Err(e) = d.def.validate() {
                    ui.label(RichText::new(e).weak());
                }
            }
            _ => {}
        }
        let ok =
            ready && (preview.is_ok() || matches!(&preview, Err(e) if e == super::PREVIEW_PENDING));
        ui.horizontal(|ui| {
            if ui
                .add_enabled(ok, egui::Button::new("OK").min_size(egui::vec2(60.0, 22.0)))
                .on_hover_text("Enter, Ctrl+Enter")
                .clicked()
                || super::keys::panel_ok_key(ui, ok)
            {
                self.commit_thicken(false);
            }
            if !editing
                && ui
                    .add_enabled(
                        ok,
                        egui::Button::new("Apply").min_size(egui::vec2(60.0, 22.0)),
                    )
                    .on_hover_text("Create it and keep the command open")
                    .clicked()
            {
                self.commit_thicken(true);
            }
            if ui
                .add(egui::Button::new("Cancel").min_size(egui::vec2(60.0, 22.0)))
                .clicked()
            {
                self.cancel_thicken();
            }
        });
    }

    /// Properties of a Thicken feature (selected in the browser).
    pub(super) fn thicken_properties(&mut self, ui: &mut egui::Ui, k: usize) {
        let Some(f) = self.part().features.get(k).cloned() else {
            return;
        };
        let Feature::BodyOp(b) = &f else {
            return;
        };
        self.panel_header(ui, Icon::Thicken, f.name());
        if let Some(h) = self.part().feature_health.get(f.id()) {
            warning_box(ui, h);
        }
        ui.label(b.describe());
        if ui.button("Edit Feature").clicked() {
            self.thicken_edit(k);
        }
    }
}
