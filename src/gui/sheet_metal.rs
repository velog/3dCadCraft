//! The Sheet Metal environment (`docs/spec/5.6-sheet-metal.md`, stage S1):
//! the Sheet Metal tab of a sheet metal part — Sheet Metal Defaults (the
//! rule), **Face** (a plate from a sketch), **Flange** (pick sheet edges;
//! height, angle, bend radius, Bend Position, Flip), **Flat Pattern** (the
//! unfolded part in the view, its extents and bends in Properties) — and
//! Convert to Sheet Metal (a solid: fixed face + bend edges). Flange's Width
//! Extents have drag arrows at the flange's ends (`sm_handles`); Flange and
//! Convert show their result as a preview (`sm_preview`).

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Stroke};

use super::CadCraftApp;
use super::valuefield::ExprValue;
use crate::kernel::Point3;
use crate::part::sheet_metal::{
    CornerRelief, FlangePosition, FlangeWidth, FoldLocation, SheetConvertFeature, SheetEdge,
    SheetFlangeFeature, SheetMetalDefaults, SheetOp, SheetOpFeature,
};

/// An open sheet metal command.
#[derive(Clone, Debug)]
pub(super) enum SmDraft {
    /// Face: the sketch (index) and Flip.
    Face { sketch: Option<usize>, flip: bool },
    /// Flange: the definition so far (its body is the first edge's sheet).
    Flange { def: SheetFlangeFeature },
    /// Convert to Sheet Metal: the fixed face, then the bend edges.
    Convert { def: SheetConvertFeature },
    /// Hem, Fold, Contour Flange or Cut.
    Op { def: SheetOpFeature },
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
    /// The flat pattern's DXF waiting for the Export dialog's path.
    pub dxf: Option<String>,
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
                // A solid that is not a sheet yet: pick its fixed face and
                // bends (SolidWorks / Solid Edge); else only the rule.
                let solid = self
                    .part()
                    .exact_bodies
                    .0
                    .iter()
                    .filter_map(|s| s.name.clone())
                    .find(|n| !self.part().sheet_models.contains_key(n));
                if let Some(body) = solid
                    && self.part().exact
                {
                    self.sm.flat = false;
                    self.sm.draft = Some(SmDraft::Convert {
                        def: SheetConvertFeature {
                            id: String::new(),
                            name: String::new(),
                            body,
                            face: crate::part::shell::FaceRef {
                                face: String::new(),
                                hint: [0.0; 3],
                            },
                            bends: Vec::new(),
                        },
                    });
                    self.tab = super::ribbon::Tab::SheetMetal;
                    return;
                }
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
                // Edges are picked on the folded part.
                if self.sm.flat {
                    self.sm.flat = false;
                    self.scene_dirty = true;
                }
                self.sm.draft = Some(SmDraft::Flange {
                    def: SheetFlangeFeature::new("", Vec::new(), 20.0),
                });
            }
            Action::SmHem => {
                if self.part().sheet_models.is_empty() {
                    self.file_status = Some(Err("Hem: make a Face first".into()));
                    return;
                }
                self.sm.flat = false;
                self.sm.draft = Some(SmDraft::Op {
                    def: SheetOpFeature::new(SheetOp::Hem {
                        body: String::new(),
                        edges: Vec::new(),
                        length: crate::part::sheet_metal::rule_of(self.part()).thickness * 4.0,
                        gap: None,
                        flip: false,
                    }),
                });
            }
            Action::SmFold | Action::SmContour | Action::SmCut => {
                let needs_sheet = !matches!(a, Action::SmContour);
                if needs_sheet && self.part().sheet_models.is_empty() {
                    self.file_status = Some(Err("Sheet metal: make a Face first".into()));
                    return;
                }
                let sketch = self
                    .sm_default_sketch_any()
                    .map(|i| self.part().sketches[i].id.clone())
                    .unwrap_or_default();
                let op = match a {
                    Action::SmFold => SheetOp::Fold {
                        body: String::new(),
                        sketch,
                        line: String::new(),
                        angle: std::f64::consts::FRAC_PI_2,
                        radius: None,
                        location: FoldLocation::Centerline,
                        flip: false,
                        flip_side: false,
                    },
                    Action::SmContour => SheetOp::Contour {
                        sketch,
                        width: 50.0,
                        flip: false,
                    },
                    _ => SheetOp::Cut {
                        sketch,
                        profiles: Vec::new(),
                    },
                };
                self.sm.flat = false;
                self.sm.draft = Some(SmDraft::Op {
                    def: SheetOpFeature::new(op),
                });
            }
            Action::SmUnfold | Action::SmRefold => {
                let unfold = matches!(a, Action::SmUnfold);
                let what = if unfold { "Unfold" } else { "Refold" };
                let mut names: Vec<String> = self.part().sheet_models.keys().cloned().collect();
                names.sort();
                let Some(body) = names
                    .into_iter()
                    .find(|b| self.part().sheet_is_flat(b) != unfold)
                else {
                    self.file_status = Some(Err(format!(
                        "{what}: {}",
                        if unfold {
                            "there is no folded sheet"
                        } else {
                            "no sheet is unfolded"
                        }
                    )));
                    return;
                };
                self.checkpoint(what);
                let op = if unfold {
                    SheetOp::Unfold { body }
                } else {
                    SheetOp::Refold { body }
                };
                match self.part_mut().add_sheet_op(SheetOpFeature::new(op)) {
                    Ok(_) => {
                        self.sm.flat = false;
                        self.scene_dirty = true;
                    }
                    Err(e) => {
                        self.history.cancel();
                        self.file_status = Some(Err(format!("{what}: {e}")));
                    }
                }
            }
            Action::SmExportFlat => {
                let mut names: Vec<String> = self.part().sheet_models.keys().cloned().collect();
                names.sort();
                let Some(body) = names.first() else {
                    self.file_status = Some(Err(
                        "Export Flat Pattern: there is no sheet yet (Face)".into(),
                    ));
                    return;
                };
                match self.part().flat_pattern_dxf(body) {
                    Ok(text) => {
                        self.sm.dxf = Some(text);
                        self.open_dialog(super::files::DialogPurpose::ExportDxf);
                    }
                    Err(e) => self.file_status = Some(Err(format!("Export Flat Pattern: {e}"))),
                }
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

    /// The newest sketch no feature uses yet (any geometry).
    fn sm_default_sketch_any(&self) -> Option<usize> {
        let p = self.part();
        (0..p.sketches.len())
            .rev()
            .find(|&i| p.consumers(&p.sketches[i].id).is_empty())
            .or_else(|| p.sketches.len().checked_sub(1))
    }

    /// The sketch chooser of an Op panel.
    fn sm_sketch_combo(&self, ui: &mut egui::Ui, salt: &str, sketch: &mut String) {
        let p = self.part();
        let shown = p
            .sketches
            .iter()
            .find(|s| s.id == *sketch)
            .map(|s| s.name.clone())
            .unwrap_or_else(|| "(pick a sketch)".into());
        egui::ComboBox::from_id_salt(salt)
            .selected_text(shown)
            .show_ui(ui, |ui| {
                for s in &p.sketches {
                    ui.selectable_value(sketch, s.id.clone(), &s.name);
                }
            });
    }

    /// The Op panel (Hem / Fold / Contour Flange / Cut).
    fn sm_op_panel(&mut self, ui: &mut egui::Ui, def: &mut SheetOpFeature) {
        let title = match def.op {
            SheetOp::Hem { .. } => "Hem",
            SheetOp::Fold { .. } => "Fold",
            SheetOp::Contour { .. } => "Contour Flange",
            SheetOp::Cut { .. } => "Cut",
            SheetOp::Unfold { .. } => "Unfold",
            SheetOp::Refold { .. } => "Refold",
            SheetOp::CornerSeam { .. } => "Corner Seam",
        };
        self.panel_header(ui, super::icons::Icon::SheetMetal, title);
        let rule = crate::part::sheet_metal::rule_of(self.part()).values();
        let grid = |ui: &mut egui::Ui, id: &str, add: &mut dyn FnMut(&mut egui::Ui)| {
            egui::Grid::new(id)
                .num_columns(2)
                .min_col_width(super::panels::label_width(ui.ctx()))
                .show(ui, |ui| add(ui));
        };
        match &mut def.op {
            SheetOp::Hem {
                edges,
                length,
                gap,
                flip,
                ..
            } => {
                ui.label(format!(
                    "Edges: {} (click sheet edges; again removes one)",
                    edges.len()
                ));
                if !edges.is_empty() && ui.small_button("Clear").clicked() {
                    edges.clear();
                }
                grid(ui, "sm_hem", &mut |ui| {
                    super::panels::plabel(ui, "Length");
                    ui.add(ExprValue::new(length).suffix(" mm"));
                    ui.end_row();
                    super::panels::plabel(ui, "Gap");
                    ui.horizontal(|ui| {
                        let mut own = gap.is_some();
                        if ui
                            .checkbox(&mut own, "")
                            .on_hover_text("Off: half the thickness")
                            .changed()
                        {
                            *gap = own.then_some(rule.thickness * 0.5);
                        }
                        match gap.as_mut() {
                            Some(g) => {
                                ui.add(ExprValue::new(g).suffix(" mm"));
                            }
                            None => {
                                ui.weak(format!(
                                    "{:.3} mm (Thickness × 0.5)",
                                    rule.thickness * 0.5
                                ));
                            }
                        }
                    });
                    ui.end_row();
                });
                ui.checkbox(flip, "Flip (to the other face)");
                ui.weak("Type: Single (Teardrop, Rolled and Double are planned)");
            }
            SheetOp::Fold {
                body,
                sketch,
                line,
                angle,
                radius,
                location,
                flip,
                flip_side,
            } => {
                ui.label("Bend Line: a sketch line on a face of the sheet");
                self.sm_sketch_combo(ui, "sm_fold_sketch", sketch);
                let p = self.part();
                let lines: Vec<(String, crate::sketch::Point2, crate::sketch::Point2)> = p
                    .sketches
                    .iter()
                    .find(|s| s.id == *sketch)
                    .map(|ps| {
                        ps.sketch
                            .entities()
                            .filter_map(|(id, e)| match e {
                                crate::sketch::Entity::Line { start, end } => Some((
                                    crate::part::extrude::entity_key(id),
                                    ps.sketch.point(*start)?.position,
                                    ps.sketch.point(*end)?.position,
                                )),
                                _ => None,
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                if !lines.iter().any(|l| l.0 == *line) {
                    *line = lines.first().map(|l| l.0.clone()).unwrap_or_default();
                }
                egui::ComboBox::from_id_salt("sm_fold_line")
                    .selected_text(if line.is_empty() {
                        "(no line)".to_owned()
                    } else {
                        format!("Line {line}")
                    })
                    .show_ui(ui, |ui| {
                        for (k, (key, a, b)) in lines.iter().enumerate() {
                            let len = (b - a).norm();
                            ui.selectable_value(
                                line,
                                key.clone(),
                                format!("Line {} ({len:.1} mm)", k + 1),
                            );
                        }
                    });
                // The sheet the line lies on.
                if let Some((_, a, b)) = lines.iter().find(|l| l.0 == *line)
                    && let Some(ps) = p.sketches.iter().find(|s| s.id == *sketch)
                {
                    let m = ps
                        .sketch
                        .plane
                        .point_at((a.x + b.x) * 0.5, (a.y + b.y) * 0.5);
                    if let Some(found) = p.sheet_body_at(m) {
                        *body = found;
                    }
                }
                grid(ui, "sm_fold", &mut |ui| {
                    super::panels::plabel(ui, "Angle");
                    let mut deg = angle.to_degrees();
                    ui.add(ExprValue::new(&mut deg).suffix("°").range(0.0..=180.0));
                    *angle = deg.to_radians();
                    ui.end_row();
                    super::panels::plabel(ui, "Bend Radius");
                    ui.horizontal(|ui| {
                        let mut own = radius.is_some();
                        if ui
                            .checkbox(&mut own, "")
                            .on_hover_text("Off: the rule's radius")
                            .changed()
                        {
                            *radius = own.then_some(rule.bend_radius);
                        }
                        match radius.as_mut() {
                            Some(r) => {
                                ui.add(ExprValue::new(r).suffix(" mm"));
                            }
                            None => {
                                ui.weak(format!("{:.3} mm (rule)", rule.bend_radius));
                            }
                        }
                    });
                    ui.end_row();
                    super::panels::plabel(ui, "Fold Location");
                    egui::ComboBox::from_id_salt("sm_fold_loc")
                        .selected_text(location.label())
                        .show_ui(ui, |ui| {
                            for l in FoldLocation::ALL {
                                ui.selectable_value(location, l, l.label());
                            }
                        });
                    ui.end_row();
                });
                ui.checkbox(flip_side, "Flip Side (the other side moves)");
                ui.checkbox(flip, "Flip Direction");
            }
            SheetOp::Contour {
                sketch,
                width,
                flip,
            } => {
                ui.label("Profile: an open chain of sketch lines (each corner becomes a bend)");
                self.sm_sketch_combo(ui, "sm_contour_sketch", sketch);
                grid(ui, "sm_contour", &mut |ui| {
                    super::panels::plabel(ui, "Distance");
                    ui.add(ExprValue::new(width).suffix(" mm"));
                    ui.end_row();
                });
                ui.checkbox(flip, "Flip (material on the other side of the chain)");
                let t = rule.thickness;
                ui.weak(format!(
                    "Thickness {t:.3} mm, bend radius {:.3} mm (Sheet Metal Defaults)",
                    rule.bend_radius
                ));
            }
            SheetOp::Cut { sketch, profiles } => {
                ui.label("Profile: the closed profiles of a sketch on a face of the sheet");
                self.sm_sketch_combo(ui, "sm_cut_sketch", sketch);
                *profiles = self
                    .part()
                    .sketches
                    .iter()
                    .find(|s| s.id == *sketch)
                    .map(|ps| crate::part::extrude::default_profiles(ps, &ps.sketch.regions()))
                    .unwrap_or_default();
                ui.label(format!("{} profile(s), through the sheet", profiles.len()));
                ui.weak("Cut Across Bend: Unfold first, cut, then Refold");
            }
            SheetOp::Unfold { .. } | SheetOp::Refold { .. } | SheetOp::CornerSeam { .. } => {}
        }
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
                ui.separator();
                ui.label(RichText::new("Width Extents").strong());
                egui::ComboBox::from_id_salt("sm_width")
                    .selected_text(def.extents.label())
                    .show_ui(ui, |ui| {
                        for w in FlangeWidth::ALL {
                            ui.selectable_value(&mut def.extents, w, w.label());
                        }
                    });
                egui::Grid::new("sm_width_grid")
                    .num_columns(2)
                    .min_col_width(super::panels::label_width(ui.ctx()))
                    .show(ui, |ui| match def.extents {
                        FlangeWidth::Edge => {}
                        FlangeWidth::Width => {
                            super::panels::plabel(ui, "Width");
                            ui.add(ExprValue::new(&mut def.width).suffix(" mm"));
                            ui.end_row();
                            super::panels::plabel(ui, "Centered");
                            ui.checkbox(&mut def.centered, "");
                            ui.end_row();
                            if !def.centered {
                                super::panels::plabel(ui, "Offset");
                                ui.add(ExprValue::new(&mut def.offset1).suffix(" mm"));
                                ui.end_row();
                            }
                        }
                        FlangeWidth::Offset => {
                            super::panels::plabel(ui, "Offset 1");
                            ui.add(ExprValue::new(&mut def.offset1).suffix(" mm"));
                            ui.end_row();
                            super::panels::plabel(ui, "Offset 2");
                            ui.add(ExprValue::new(&mut def.offset2).suffix(" mm"));
                            ui.end_row();
                        }
                    });
                ui.weak("Drag the arrows at the flange's ends to make it narrower");
                if def.extents == FlangeWidth::Width && def.width <= 0.0 {
                    // A first width: half the first edge.
                    if let Some(e) = def.edges.first()
                        && let Some((a, b)) = self.part().sheet_edge_ends(&def.body, e)
                    {
                        def.width = ((b - a).norm() * 0.5).round();
                    }
                }
            }
            SmDraft::Op { def } => self.sm_op_panel(ui, def),
            SmDraft::Convert { def } => {
                self.panel_header(ui, super::icons::Icon::SheetMetal, "Convert to Sheet Metal");
                ui.label(format!("Solid: {}", def.body));
                ui.horizontal(|ui| {
                    if def.face.face.is_empty() {
                        ui.label("Base Face: click the face that stays put");
                    } else {
                        ui.label(format!("Base Face: {}", def.face.face));
                        if ui.small_button("x").on_hover_text("Pick again").clicked() {
                            def.face.face.clear();
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(format!("Bends: {} edge(s)", def.bends.len()));
                    if !def.bends.is_empty() && ui.small_button("Clear").clicked() {
                        def.bends.clear();
                    }
                });
                let t = crate::part::sheet_metal::rule_of(self.part()).thickness;
                ui.weak(format!(
                    "Thickness {t:.3} mm (Sheet Metal Defaults); a shelled solid keeps its walls. Edges not picked are cut open."
                ));
            }
        }
        ui.separator();
        ui.horizontal(|ui| {
            let ready = match &d {
                SmDraft::Face { sketch, .. } => sketch.is_some(),
                SmDraft::Flange { def } => !def.edges.is_empty(),
                SmDraft::Convert { def } => !def.face.face.is_empty() && !def.bends.is_empty(),
                SmDraft::Op { def } => op_ready(def),
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
        self.checkpoint(match &d {
            SmDraft::Face { .. } => "Face",
            SmDraft::Flange { .. } => "Flange",
            SmDraft::Convert { .. } => "Convert to Sheet Metal",
            SmDraft::Op { def } => match def.op {
                SheetOp::Hem { .. } => "Hem",
                SheetOp::Fold { .. } => "Fold",
                SheetOp::Contour { .. } => "Contour Flange",
                _ => "Cut",
            },
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
            SmDraft::Convert { def } => self.part_mut().add_sheet_convert(def).map(|_| ()),
            SmDraft::Op { def } => self.part_mut().add_sheet_op(def).map(|_| ()),
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
        if let Some(SmDraft::Convert { .. }) = &self.sm.draft {
            return self.sm_convert_view(painter, rect, pointer, clicked);
        }
        let picks_edges = matches!(self.sm.draft, Some(SmDraft::Flange { .. }))
            || matches!(
                &self.sm.draft,
                Some(SmDraft::Op { def }) if matches!(def.op, SheetOp::Hem { .. })
            );
        if !picks_edges {
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
        let picked: Vec<[f64; 3]> = match &self.sm.draft {
            Some(SmDraft::Flange { def }) => def.edges.iter().map(|e| e.hint).collect(),
            Some(SmDraft::Op { def }) => match &def.op {
                SheetOp::Hem { edges, .. } => edges.iter().map(|e| e.hint).collect(),
                _ => Vec::new(),
            },
            _ => Vec::new(),
        };
        {
            for e in &picked {
                if let Some(s) = self.camera.project(&Point3::from(*e), rect) {
                    painter.circle_filled(s, 5.0, Color32::from_rgb(80, 160, 255));
                }
            }
        }
        took
    }

    /// Convert: a click on a face sets the base face (while it is empty),
    /// then clicks on edges add / remove bends.
    fn sm_convert_view(
        &mut self,
        painter: &egui::Painter,
        rect: Rect,
        pointer: Option<Pos2>,
        clicked: bool,
    ) -> bool {
        let Some(SmDraft::Convert { def }) = &self.sm.draft else {
            return false;
        };
        let want_face = def.face.face.is_empty();
        let mut took = false;
        self.sm.hover = None;
        if let Some(p) = pointer
            && let Some((o, dir)) = self.camera.ray(p, rect)
        {
            if want_face {
                if let Some((f, t)) = self.face_hit(&o, &dir)
                    && let Some(name) = self.part().body.face(f).and_then(|x| x.name.clone())
                {
                    let opts = crate::kernel::TessellationOptions::default();
                    let body = &self.part().body;
                    let lines: Vec<Vec<Pos2>> = crate::part::select::edges_of_face(body, f)
                        .into_iter()
                        .filter_map(|e| body.sample_edge(e, &opts).ok())
                        .map(|pts| {
                            pts.iter()
                                .filter_map(|q| self.camera.project(q, rect))
                                .collect()
                        })
                        .collect();
                    for l in lines {
                        painter.add(egui::Shape::line(
                            l,
                            Stroke::new(3.0, Color32::from_rgb(110, 220, 120)),
                        ));
                    }
                    if clicked {
                        let hit = o + dir.normalize() * t;
                        if let Some(SmDraft::Convert { def }) = self.sm.draft.as_mut() {
                            def.face = crate::part::shell::FaceRef {
                                face: name,
                                hint: [hit.x, hit.y, hit.z],
                            };
                        }
                        took = true;
                    }
                }
            } else if let Some(e) = self.edge_under(p, rect)
                && let Some(r) = crate::part::blend::edge_ref(&self.part().body, e)
            {
                let opts = crate::kernel::TessellationOptions::default();
                if let Ok(pts) = self.part().body.sample_edge(e, &opts) {
                    let line: Vec<Pos2> = pts
                        .iter()
                        .filter_map(|q| self.camera.project(q, rect))
                        .collect();
                    painter.add(egui::Shape::line(
                        line,
                        Stroke::new(3.0, Color32::from_rgb(110, 220, 120)),
                    ));
                }
                if clicked && let Some(SmDraft::Convert { def }) = self.sm.draft.as_mut() {
                    match def.bends.iter().position(|b| b.faces == r.faces) {
                        Some(k) => {
                            def.bends.remove(k);
                        }
                        None => def.bends.push(r),
                    }
                    took = true;
                }
            }
        }
        if let Some(SmDraft::Convert { def }) = &self.sm.draft {
            for b in &def.bends {
                if let Some(s) = self.camera.project(&Point3::from(b.hint), rect) {
                    painter.circle_filled(s, 5.0, Color32::from_rgb(80, 160, 255));
                }
            }
            if !def.face.face.is_empty()
                && let Some(s) = self.camera.project(&Point3::from(def.face.hint), rect)
            {
                painter.circle_filled(s, 6.0, Color32::from_rgb(255, 170, 40));
            }
        }
        took
    }

    /// Flange's Width Extents arrows: at both ends of its first edge, along
    /// the edge; dragging one makes the flange Offset-wide.
    pub(super) fn sm_handles(&self) -> Vec<super::manip::Handle> {
        use super::manip::{DragGeom, Handle};
        let Some(SmDraft::Flange { def }) = &self.sm.draft else {
            return Vec::new();
        };
        let Some(e) = def.edges.first() else {
            return Vec::new();
        };
        let Some((a, b)) = self.part().sheet_edge_ends(&def.body, e) else {
            return Vec::new();
        };
        let len = (b - a).norm();
        if len < 1e-6 {
            return Vec::new();
        }
        let d = (b - a) / len;
        let (o1, o2) = def.spans(len).unwrap_or((def.offset1, def.offset2));
        let reach = (len * 0.08).clamp(3.0, 20.0);
        let handle =
            |field: &'static str, origin: Point3, dir: crate::kernel::Vector3, off: f64| {
                let grip = origin + dir * off;
                Handle {
                    field,
                    path: vec![grip - dir * reach, grip],
                    grip,
                    geom: DragGeom::Linear {
                        origin,
                        dir,
                        scale: 1.0,
                    },
                    arrow: Some(-dir),
                    angle: false,
                    signed: false,
                }
            };
        vec![handle("offset1", a, d, o1), handle("offset2", b, -d, o2)]
    }

    /// A Width Extents arrow was dragged to `v` (mm from its end).
    pub(super) fn sm_manip(&mut self, field: &str, v: f64) {
        let ends = match &self.sm.draft {
            Some(SmDraft::Flange { def }) => def
                .edges
                .first()
                .and_then(|e| self.part().sheet_edge_ends(&def.body, e)),
            _ => None,
        };
        let Some((a, b)) = ends else {
            return;
        };
        let len = (b - a).norm();
        let Some(SmDraft::Flange { def }) = self.sm.draft.as_mut() else {
            return;
        };
        let (o1, o2) = def.spans(len).unwrap_or((0.0, 0.0));
        // Dragged ends: the flange is Offset-wide from now on.
        def.extents = FlangeWidth::Offset;
        (def.offset1, def.offset2) = (o1, o2);
        let room = |other: f64| (len - other - 1.0).max(0.0);
        match field {
            "offset1" => def.offset1 = v.clamp(0.0, room(def.offset2)),
            "offset2" => def.offset2 = v.clamp(0.0, room(def.offset1)),
            _ => {}
        }
        for f in ["offset1", "offset2"] {
            if let Some(p) = def.params.0.get_mut(f) {
                p.expr = None;
            }
        }
    }

    /// The result of the open Flange / Convert as edge polylines (worker
    /// thread), or why it fails.
    pub(super) fn sm_preview(&self) -> Result<Vec<Vec<Point3>>, String> {
        let (key, feature) = match &self.sm.draft {
            Some(SmDraft::Flange { def }) if !def.edges.is_empty() => (
                format!("sm|{def:?}"),
                crate::part::Feature::SheetFlange(def.clone()),
            ),
            Some(SmDraft::Op { def }) if op_ready(def) => (
                format!("sm|{def:?}"),
                crate::part::Feature::SheetOp(def.clone()),
            ),
            Some(SmDraft::Convert { def })
                if !def.face.face.is_empty() && !def.bends.is_empty() =>
            {
                (
                    format!("sm|{def:?}"),
                    crate::part::Feature::SheetConvert(def.clone()),
                )
            }
            _ => return Err(String::new()),
        };
        if let Some(r) = self.preview_ready(&key) {
            return r;
        }
        let mut trial = self.part().clone();
        self.preview_async(key, move || {
            let built = match feature {
                crate::part::Feature::SheetFlange(d) => trial.add_sheet_flange(d),
                crate::part::Feature::SheetConvert(d) => trial.add_sheet_convert(d),
                crate::part::Feature::SheetOp(d) => trial.add_sheet_op(d),
                _ => return Err(String::new()),
            };
            built
                .map(|_| super::combine_split::face_edges(&trial, |_| true))
                .map_err(|e| e.to_string())
        })
    }

    fn sm_pick_edge(&mut self, at: Point3) {
        let Some((body, edge)) = self.part().sheet_edge_at(at) else {
            self.file_status = Some(Err("Flange: that is not an edge of a sheet".into()));
            return;
        };
        let (sheet, edges) = match self.sm.draft.as_mut() {
            Some(SmDraft::Flange { def }) => (&mut def.body, &mut def.edges),
            Some(SmDraft::Op { def }) => match &mut def.op {
                SheetOp::Hem { body, edges, .. } => (body, edges),
                _ => return,
            },
            _ => return,
        };
        if sheet.is_empty() {
            *sheet = body.clone();
        } else if *sheet != body {
            self.file_status = Some(Err("Sheet metal: the edges must be on one sheet".into()));
            return;
        }
        match edges.iter().position(|e: &SheetEdge| e.name == edge.name) {
            Some(k) => {
                edges.remove(k);
            }
            None => edges.push(edge),
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
                "Flange: more edges, drag the end arrows for a narrower flange, then OK".into()
            }
            SmDraft::Op { def } => match &def.op {
                SheetOp::Hem { edges, .. } if edges.is_empty() => {
                    "Hem: click the sheet edges to hem".into()
                }
                SheetOp::Hem { .. } => "Hem: more edges, or set the length and gap, then OK".into(),
                SheetOp::Fold { .. } => {
                    "Fold: choose the sketch and its line on a face of the sheet, then OK".into()
                }
                SheetOp::Contour { .. } => {
                    "Contour Flange: choose the sketch with an open chain of lines, then OK".into()
                }
                _ => "Cut: choose the sketch with the closed profiles, then OK".into(),
            },
            SmDraft::Convert { def } if def.face.face.is_empty() => {
                "Convert to Sheet Metal: click the face that stays put (the base face)".into()
            }
            SmDraft::Convert { .. } => {
                "Convert to Sheet Metal: click the edges that become bends, then OK".into()
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
                ui.horizontal(|ui| {
                    ui.label("Corner Relief Shape");
                    egui::ComboBox::from_id_salt("sm_corner")
                        .selected_text(r.corner_shape.label())
                        .show_ui(ui, |ui| {
                            for c in CornerRelief::ALL {
                                ui.selectable_value(&mut r.corner_shape, c, c.label());
                            }
                        });
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

/// Whether an Op draft has its inputs.
fn op_ready(def: &SheetOpFeature) -> bool {
    match &def.op {
        SheetOp::Hem { edges, .. } => !edges.is_empty(),
        SheetOp::Fold {
            body, sketch, line, ..
        } => !body.is_empty() && !sketch.is_empty() && !line.is_empty(),
        SheetOp::Contour { sketch, .. } => !sketch.is_empty(),
        SheetOp::Cut { sketch, profiles } => !sketch.is_empty() && !profiles.is_empty(),
        SheetOp::Unfold { .. } | SheetOp::Refold { .. } => true,
        SheetOp::CornerSeam { a, b, .. } => !a.is_empty() && !b.is_empty(),
    }
}
