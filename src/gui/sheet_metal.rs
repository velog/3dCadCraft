//! The Sheet Metal environment (`docs/spec/5.6-sheet-metal.md`, stage S1):
//! the Sheet Metal tab of a sheet metal part — Sheet Metal Defaults (the
//! rule), **Face** (a plate from a sketch), **Flange** (pick sheet edges;
//! height, angle, bend radius, Bend Position, Flip), **Flat Pattern** (the
//! unfolded part in the view, its extents and bends in Properties) — and
//! Convert to Sheet Metal on the 3D Model tab.

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Stroke};

use super::CadCraftApp;
use super::valuefield::ExprValue;
use crate::kernel::Point3;
use crate::part::sheet_metal::{FlangePosition, SheetEdge, SheetFlangeFeature, SheetMetalDefaults};

/// An open sheet metal command.
#[derive(Clone, Debug)]
pub(super) enum SmDraft {
    /// Face: the sketch (index) and Flip.
    Face { sketch: Option<usize>, flip: bool },
    /// Flange: the definition so far (its body is the first edge's sheet).
    Flange { def: SheetFlangeFeature },
}

/// The environment's state.
#[derive(Default)]
pub(super) struct SheetUi {
    pub draft: Option<SmDraft>,
    /// Sheet Metal Defaults being edited.
    pub defaults: Option<SheetMetalDefaults>,
    /// The view shows the flat pattern.
    pub flat: bool,
    /// The sheet edge under the cursor (Flange): its polyline (screen).
    hover: Option<Vec<Pos2>>,
}

impl CadCraftApp {
    /// Whether the active part is a sheet metal part.
    pub(super) fn is_sheet_metal(&self) -> bool {
        !self.is_assembly() && self.part().sheet_metal.is_some()
    }

    /// The Sheet Metal commands.
    pub(super) fn sm_apply(&mut self, a: super::ribbon::Action) {
        use super::ribbon::Action;
        match a {
            Action::SmConvert => {
                if self.part().sheet_metal.is_none() {
                    self.checkpoint("Convert to Sheet Metal");
                    self.part_mut().make_sheet_metal(None);
                    self.file_status = Some(Ok(
                        "Converted to sheet metal: the Sheet Metal tab has Face, Flange and Flat Pattern".into(),
                    ));
                }
                self.tab = super::ribbon::Tab::SheetMetal;
            }
            Action::SmDefaults => {
                self.sm.defaults = Some(crate::part::sheet_metal::rule_of(self.part()));
            }
            Action::SmFace => {
                let sketch = self.sm_default_sketch();
                self.sm.draft = Some(SmDraft::Face {
                    sketch,
                    flip: false,
                });
            }
            Action::SmFlange => {
                if self.part().sheet_models.is_empty() {
                    self.file_status = Some(Err("Flange: make a Face first".into()));
                    return;
                }
                self.sm.draft = Some(SmDraft::Flange {
                    def: SheetFlangeFeature::new("", Vec::new(), 20.0),
                });
            }
            Action::SmFlatPattern => {
                if self.part().sheet_models.is_empty() {
                    self.file_status =
                        Some(Err("Flat Pattern: there is no sheet yet (Face)".into()));
                    return;
                }
                self.sm.flat = !self.sm.flat;
                self.scene_dirty = true;
            }
            _ => {}
        }
    }

    /// The sketch a new Face starts with: the newest one with a closed
    /// profile that no feature uses yet.
    fn sm_default_sketch(&self) -> Option<usize> {
        let p = self.part();
        (0..p.sketches.len()).rev().find(|&i| {
            p.consumers(&p.sketches[i].id).is_empty() && !p.sketches[i].sketch.regions().is_empty()
        })
    }

    /// The command's panel in Properties.
    pub(super) fn sm_panel(&mut self, ui: &mut egui::Ui) {
        let Some(mut d) = self.sm.draft.clone() else {
            return;
        };
        let mut ok = false;
        let mut cancel = false;
        match &mut d {
            SmDraft::Face { sketch, flip } => {
                self.panel_header(ui, super::icons::Icon::SheetMetal, "Face");
                let names: Vec<String> = self
                    .part()
                    .sketches
                    .iter()
                    .map(|s| s.name.clone())
                    .collect();
                ui.label("Profile: the sketch's closed profile");
                egui::ComboBox::from_id_salt("sm_face_sketch")
                    .selected_text(
                        sketch
                            .and_then(|i| names.get(i).cloned())
                            .unwrap_or_else(|| "(pick a sketch)".into()),
                    )
                    .show_ui(ui, |ui| {
                        for (i, n) in names.iter().enumerate() {
                            ui.selectable_value(sketch, Some(i), n);
                        }
                    });
                let t = crate::part::sheet_metal::rule_of(self.part()).thickness;
                ui.label(format!("Thickness {t:.3} mm (Sheet Metal Defaults)"));
                ui.checkbox(flip, "Flip (the thickness on the other side)");
            }
            SmDraft::Flange { def } => {
                self.panel_header(ui, super::icons::Icon::SheetMetal, "Flange");
                ui.label(format!(
                    "Edges: {} (click sheet edges; again removes one)",
                    def.edges.len()
                ));
                if !def.edges.is_empty() && ui.small_button("Clear").clicked() {
                    def.edges.clear();
                }
                egui::Grid::new("sm_flange")
                    .num_columns(2)
                    .min_col_width(super::panels::label_width(ui.ctx()))
                    .show(ui, |ui| {
                        super::panels::plabel(ui, "Height");
                        ui.add(ExprValue::new(&mut def.height).suffix(" mm"));
                        ui.end_row();
                        super::panels::plabel(ui, "Angle");
                        let mut a = def.angle.to_degrees();
                        ui.add(ExprValue::new(&mut a).suffix("°").range(0.0..=180.0));
                        def.angle = a.to_radians();
                        ui.end_row();
                        super::panels::plabel(ui, "Bend Radius");
                        let rule = crate::part::sheet_metal::rule_of(self.part())
                            .values()
                            .bend_radius;
                        let mut own = def.radius.is_some();
                        ui.horizontal(|ui| {
                            if ui
                                .checkbox(&mut own, "")
                                .on_hover_text("Off: the rule's radius")
                                .changed()
                            {
                                def.radius = own.then_some(rule);
                            }
                            match def.radius.as_mut() {
                                Some(r) => {
                                    ui.add(ExprValue::new(r).suffix(" mm"));
                                }
                                None => {
                                    ui.weak(format!("{rule:.3} mm (rule)"));
                                }
                            }
                        });
                        ui.end_row();
                        super::panels::plabel(ui, "Bend Position");
                        egui::ComboBox::from_id_salt("sm_pos")
                            .selected_text(def.position.label())
                            .show_ui(ui, |ui| {
                                for p in [
                                    FlangePosition::Inside,
                                    FlangePosition::Outside,
                                    FlangePosition::Middle,
                                ] {
                                    ui.selectable_value(&mut def.position, p, p.label());
                                }
                            });
                        ui.end_row();
                    });
                ui.checkbox(&mut def.flip, "Flip direction");
            }
        }
        ui.separator();
        ui.horizontal(|ui| {
            let ready = match &d {
                SmDraft::Face { sketch, .. } => sketch.is_some(),
                SmDraft::Flange { def } => !def.edges.is_empty(),
            };
            let r = ui.add_enabled(ready, egui::Button::new("OK"));
            if r.clicked() || super::keys::panel_ok_key(ui, ready) {
                ok = true;
            }
            if ui.button("Cancel").clicked() {
                cancel = true;
            }
        });
        if cancel {
            self.sm.draft = None;
            self.sm.hover = None;
            return;
        }
        self.sm.draft = Some(d.clone());
        if ok {
            self.sm_commit(d);
        }
    }

    fn sm_commit(&mut self, d: SmDraft) {
        self.checkpoint(match d {
            SmDraft::Face { .. } => "Face",
            SmDraft::Flange { .. } => "Flange",
        });
        let r = match d {
            SmDraft::Face {
                sketch: Some(i),
                flip,
            } => {
                let profiles = match self.part().sketches.get(i) {
                    Some(ps) => crate::part::extrude::default_profiles(ps, &ps.sketch.regions()),
                    None => Vec::new(),
                };
                self.part_mut()
                    .add_sheet_face(i, profiles, flip)
                    .map(|_| ())
            }
            SmDraft::Face { sketch: None, .. } => Ok(()),
            SmDraft::Flange { def } => self.part_mut().add_sheet_flange(def).map(|_| ()),
        };
        match r {
            Ok(()) => {
                self.sm.draft = None;
                self.sm.hover = None;
                self.scene_dirty = true;
            }
            Err(e) => {
                self.history.cancel();
                self.file_status = Some(Err(e.to_string()));
            }
        }
    }

    /// Flange: the sheet edge under the cursor (highlighted), a click adds it
    /// (or removes it). Returns `true` when it took the click.
    pub(super) fn sm_view(
        &mut self,
        painter: &egui::Painter,
        rect: Rect,
        pointer: Option<Pos2>,
        clicked: bool,
    ) -> bool {
        if !matches!(self.sm.draft, Some(SmDraft::Flange { .. })) {
            return false;
        }
        self.sm.hover = None;
        let mut took = false;
        if let Some(p) = pointer
            && let Some(e) = self.edge_under(p, rect)
            && let Some((o, dir)) = self.camera.ray(p, rect)
        {
            let opts = crate::kernel::TessellationOptions::default();
            if let Ok(pts) = self.part().body.sample_edge(e, &opts) {
                // The point of the edge nearest the eye ray.
                let at = pts
                    .windows(2)
                    .map(|w| super::closest_on_segment_pub(&w[0], &w[1], &o, &dir))
                    .min_by(|a, b| {
                        let da = (a - o).cross(&dir).norm();
                        let db = (b - o).cross(&dir).norm();
                        da.total_cmp(&db)
                    });
                self.sm.hover = Some(
                    pts.iter()
                        .filter_map(|q| self.camera.project(q, rect))
                        .collect(),
                );
                if clicked && let Some(at) = at {
                    took = true;
                    self.sm_pick_edge(at);
                }
            }
        }
        if let Some(line) = &self.sm.hover {
            painter.add(egui::Shape::line(
                line.clone(),
                Stroke::new(3.0, Color32::from_rgb(110, 220, 120)),
            ));
        }
        if let Some(SmDraft::Flange { def }) = &self.sm.draft {
            for e in &def.edges {
                if let Some(s) = self.camera.project(&Point3::from(e.hint), rect) {
                    painter.circle_filled(s, 5.0, Color32::from_rgb(80, 160, 255));
                }
            }
        }
        took
    }

    fn sm_pick_edge(&mut self, at: Point3) {
        let Some((body, edge)) = self.part().sheet_edge_at(at) else {
            self.file_status = Some(Err("Flange: that is not an edge of a sheet".into()));
            return;
        };
        let Some(SmDraft::Flange { def }) = self.sm.draft.as_mut() else {
            return;
        };
        if def.body.is_empty() {
            def.body = body.clone();
        } else if def.body != body {
            self.file_status = Some(Err("Flange: the edges must be on one sheet".into()));
            return;
        }
        match def
            .edges
            .iter()
            .position(|e: &SheetEdge| e.name == edge.name)
        {
            Some(k) => {
                def.edges.remove(k);
            }
            None => def.edges.push(edge),
        }
    }

    /// The status-bar prompt.
    pub(super) fn sm_prompt(&self) -> Option<String> {
        Some(match self.sm.draft.as_ref()? {
            SmDraft::Face { .. } => "Face: choose the sketch with the profile, then OK".into(),
            SmDraft::Flange { def } if def.edges.is_empty() => {
                "Flange: click the sheet edges to bend (on the top or bottom face)".into()
            }
            SmDraft::Flange { .. } => {
                "Flange: more edges, or set the height and angle, then OK".into()
            }
        })
    }

    /// Sheet Metal Defaults window.
    pub(super) fn sm_defaults_window(&mut self, ctx: &egui::Context) {
        let Some(mut r) = self.sm.defaults.clone() else {
            return;
        };
        let mut open = true;
        let mut apply = false;
        let win = egui::Window::new("Sheet Metal Defaults")
            .open(&mut open)
            .resizable(false)
            .collapsible(false);
        self.layout
            .place("sm_defaults", ctx.content_rect(), false, win)
            .show(ctx, |ui| {
                egui::Grid::new("sm_rule")
                    .num_columns(2)
                    .min_col_width(super::panels::label_width(ui.ctx()))
                    .show(ui, |ui| {
                        super::panels::plabel(ui, "Thickness");
                        ui.add(ExprValue::new(&mut r.thickness).suffix(" mm"));
                        ui.end_row();
                        super::panels::plabel(ui, "K-factor");
                        ui.add(ExprValue::new(&mut r.k_factor).range(0.0..=1.0));
                        ui.end_row();
                        let t = r.thickness;
                        for (label, slot, def) in [
                            ("Bend Radius", &mut r.bend_radius, t),
                            ("Relief Width", &mut r.relief_width, t),
                            ("Relief Depth", &mut r.relief_depth, t * 0.5),
                            ("Corner Relief", &mut r.corner_relief, t * 4.0),
                            ("Gap", &mut r.gap, t),
                        ] {
                            super::panels::plabel(ui, label);
                            ui.horizontal(|ui| {
                                let mut own = slot.is_some();
                                if ui
                                    .checkbox(&mut own, "")
                                    .on_hover_text("Off: follows the thickness")
                                    .changed()
                                {
                                    *slot = own.then_some(def);
                                }
                                match slot.as_mut() {
                                    Some(v) => {
                                        ui.add(ExprValue::new(v).suffix(" mm"));
                                    }
                                    None => {
                                        ui.weak(format!("{def:.3} mm (from Thickness)"));
                                    }
                                }
                            });
                            ui.end_row();
                        }
                    });
                ui.label(RichText::new("Unfold rule: linear, BA = angle × (R + K × T)").weak());
                if let Err(e) = r.validate() {
                    ui.colored_label(Color32::from_rgb(235, 80, 70), e);
                }
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(r.validate().is_ok(), egui::Button::new("OK"))
                        .clicked()
                    {
                        apply = true;
                    }
                    if ui.button("Cancel").clicked() {
                        self.sm.defaults = None;
                    }
                });
            });
        if apply {
            self.checkpoint("Sheet Metal Defaults");
            self.part_mut().sheet_metal = Some(r.clone());
            if let Err(e) = self.part_mut().regenerate() {
                self.file_status = Some(Err(e.to_string()));
            }
            self.scene_dirty = true;
            self.sm.defaults = None;
            return;
        }
        if !open {
            self.sm.defaults = None;
        } else if self.sm.defaults.is_some() {
            self.sm.defaults = Some(r);
        }
    }

    /// The flat patterns for the view (when Flat Pattern is on): a part of
    /// their solids.
    pub(super) fn sm_flat_part(&self) -> Option<crate::part::PartDocument> {
        if !self.sm.flat || !self.is_sheet_metal() {
            return None;
        }
        let flats = self.part().flat_patterns();
        if flats.is_empty() {
            return None;
        }
        let solids: Vec<_> = flats.into_iter().map(|f| f.solid).collect();
        let body = crate::kernel::exact::to_brep(&solids).ok()?;
        Some(crate::part::PartDocument {
            body,
            material: self.part().material.clone(),
            ..Default::default()
        })
    }

    /// Flat Pattern facts in Properties.
    pub(super) fn sm_flat_info(&self, ui: &mut egui::Ui) {
        for f in self.part().flat_patterns() {
            ui.label(RichText::new(format!("Flat Pattern of {}", f.body)).strong());
            ui.label(format!(
                "Extents {:.2} × {:.2} mm",
                f.size.0.max(f.size.1),
                f.size.0.min(f.size.1)
            ));
            ui.label(format!("{} bend(s)", f.bends.len()));
            for (k, b) in f.bends.iter().enumerate() {
                ui.weak(format!(
                    "Bend {}: {:.1}° {} R {:.2}, allowance {:.3} mm",
                    k + 1,
                    b.angle_deg,
                    if b.up { "up" } else { "down" },
                    b.radius,
                    b.allowance
                ));
            }
        }
    }
}
