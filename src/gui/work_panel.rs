//! Properties panel of the work feature commands
//! (`docs/spec/2.10-work-features.md`): the command dialog (picked inputs,
//! the method they make, its values and options, OK / Cancel) and the
//! properties of an existing work plane, axis, point or UCS.

use eframe::egui::{self, RichText};

use super::icons::Icon;
use super::panels::{Fields, field_row, plabel, psection, warning_box};
use super::work::{WorkDraft, WorkKind, WpInput};
use super::{CadCraftApp, Mode, PickPurpose};
use crate::part::work::WorkResult;
use crate::part::{
    Feature, LineRef, PlaneRef, PointRef, UcsDef, WorkAxisDef as A, WorkDef, WorkFeature,
    WorkPlaneDef as D, WorkPointDef as P,
};

fn icon(kind: WorkKind) -> Icon {
    match kind {
        WorkKind::Plane => Icon::WorkPlane,
        WorkKind::Axis => Icon::WorkAxis,
        WorkKind::Point => Icon::WorkPoint,
        WorkKind::Ucs => Icon::Ucs,
    }
}

/// The rows of a definition's inputs (label, text) for Properties.
fn input_rows(def: &WorkDef) -> Vec<(&'static str, String)> {
    let pl = |r: &PlaneRef| WpInput::Plane(r.clone()).describe();
    let pt = |r: &PointRef| r.describe();
    let ln = |r: &LineRef| r.describe();
    let three = |labels: [&'static str; 3], v: [String; 3]| labels.into_iter().zip(v).collect();
    match def {
        WorkDef::Plane(d) => match d {
            D::Offset { base, .. } => vec![("Plane", pl(base))],
            D::ThroughPoint { base, point } => vec![("Plane", pl(base)), ("Point", pt(point))],
            D::Midplane { a, b } => vec![("Plane 1", pl(a)), ("Plane 2", pl(b))],
            D::Angle { base, axis, .. } => vec![("Plane", pl(base)), ("Edge", ln(axis))],
            D::ThreePoints { points } => three(
                ["Point 1", "Point 2", "Point 3"],
                points.clone().map(|p| pt(&p)),
            ),
            D::TwoLines { a, b } => vec![("Edge 1", ln(a)), ("Edge 2", ln(b))],
            D::NormalToAxis { axis, point } => vec![("Axis", ln(axis)), ("Point", pt(point))],
            D::Fixed { .. } => Vec::new(),
            D::TorusMidplane { face } => vec![("Torus", face.clone())],
            D::TangentEdge { face, line, .. } => vec![("Face", face.clone()), ("Edge", ln(line))],
            D::TangentPoint { face, point, .. } => {
                vec![("Face", face.clone()), ("Point", pt(point))]
            }
            D::TangentParallel { face, base, .. } => {
                vec![("Face", face.clone()), ("Plane", pl(base))]
            }
            D::NormalToCurve { curve, point } => {
                vec![("Curve", curve.describe()), ("Point", pt(point))]
            }
        },
        WorkDef::Axis(d) => match d {
            A::OnLine { line } => vec![("Line", ln(line))],
            A::ParallelThroughPoint { line, point } => {
                vec![("Line", ln(line)), ("Point", pt(point))]
            }
            A::TwoPoints { a, b } => vec![("Point 1", pt(a)), ("Point 2", pt(b))],
            A::TwoPlanes { a, b } => vec![("Plane 1", pl(a)), ("Plane 2", pl(b))],
            A::NormalToPlane { plane, point } => vec![("Plane", pl(plane)), ("Point", pt(point))],
            A::CircularEdge { edge } => vec![("Edge", edge.clone())],
            A::Revolved { feature } => vec![("Feature", feature.clone())],
            A::LineOnPlane { line, plane } => vec![("Line", ln(line)), ("Plane", pl(plane))],
            A::Fixed { .. } => Vec::new(),
            A::SketchLine { sketch, .. } => vec![("Line", format!("Line of {sketch}"))],
            A::RevolvedFace { face } => vec![("Face", face.clone())],
            A::SketchCircle { sketch, .. } => vec![("Curve", format!("Curve of {sketch}"))],
        },
        WorkDef::Point(d) => match d {
            P::OnPoint { point } => vec![("Point", pt(point))],
            P::ThreePlanes { planes } => three(
                ["Plane 1", "Plane 2", "Plane 3"],
                planes.clone().map(|p| pl(&p)),
            ),
            P::TwoLines { a, b } => vec![("Line 1", ln(a)), ("Line 2", ln(b))],
            P::PlaneLine { plane, line } => vec![("Plane", pl(plane)), ("Line", ln(line))],
            P::LoopCenter { edge, .. } => vec![("Edge", edge.clone())],
            P::Fixed { .. } => Vec::new(),
            P::SketchPoint { sketch, .. } => vec![("Point", format!("Point of {sketch}"))],
            P::SurfaceLine { face, line, .. } => vec![("Face", face.clone()), ("Line", ln(line))],
            P::TorusCenter { face } => vec![("Torus", face.clone())],
            P::SphereCenter { face } => vec![("Sphere", face.clone())],
        },
        WorkDef::Ucs(UcsDef::ThreePoints { points }) => three(
            ["Center", "X direction", "Y direction"],
            points.clone().map(|p| pt(&p)),
        ),
        WorkDef::Ucs(UcsDef::Offsets { .. }) => vec![("Placed", "Relative to the Origin".into())],
    }
}

/// Label of a numeric field.
fn field_label(field: &str) -> &str {
    match field {
        "distance" => "Distance",
        "angle" => "Angle",
        "x" => "X",
        "y" => "Y",
        "z" => "Z",
        "rx" => "Angle X",
        "ry" => "Angle Y",
        "rz" => "Angle Z",
        other => other,
    }
}

impl CadCraftApp {
    /// The Work Plane / Axis / Point / UCS command dialog: picked inputs,
    /// the method they make, its values and options, OK / Cancel.
    pub(super) fn work_dialog(&mut self, ui: &mut egui::Ui) {
        let Some(mut d) = self.work_draft.clone() else {
            return;
        };
        let title = match d.edit {
            Some(_) => format!("{} (Redefine)", d.kind.title()),
            None => d.kind.title().to_owned(),
        };
        self.panel_header(ui, icon(d.kind), &title);
        let mut remove = None;
        psection(ui, "wp_input", "Input", |ui| {
            for (k, p) in d.picks.iter().enumerate() {
                plabel(ui, &format!("{}", k + 1));
                ui.horizontal(|ui| {
                    ui.label(p.describe()).on_hover_text(p.describe());
                    if ui.small_button("x").on_hover_text("Remove").clicked() {
                        remove = Some(k);
                    }
                });
                ui.end_row();
            }
            plabel(ui, "Origin");
            ui.horizontal(|ui| {
                for (k, n) in ["X", "Y", "Z"].iter().enumerate() {
                    if ui
                        .small_button(*n)
                        .on_hover_text(format!("Use the {n} axis of the Origin"))
                        .clicked()
                    {
                        d.pick(WpInput::Line(LineRef::Origin(k)));
                    }
                }
                if ui
                    .small_button("Center")
                    .on_hover_text("Use the Center Point of the Origin")
                    .clicked()
                {
                    d.pick(WpInput::Point(PointRef::PartOrigin));
                }
            });
            ui.end_row();
        });
        if let Some(k) = remove {
            d.picks.remove(k);
        }
        psection(ui, "wp_method", "Method", |ui| {
            if d.kind == WorkKind::Point {
                plabel(ui, "Grounded");
                let was = d.grounded;
                ui.checkbox(&mut d.grounded, "")
                    .on_hover_text("Fixed in space at X, Y, Z (no longer follows the model)");
                // Start from the picked point.
                if d.grounded && !was {
                    let mut probe = d.clone();
                    probe.grounded = false;
                    if let Some(Ok(WorkResult::Point(_, p))) = self.preview_draft(&probe) {
                        d.xyz = [p.x, p.y, p.z];
                    }
                }
                ui.end_row();
            }
            plabel(ui, "Method");
            match d.def() {
                Ok(def) => ui.label(RichText::new(def.method()).strong()),
                Err(e) => ui.label(RichText::new(super::work::tr(e)).weak()),
            };
            ui.end_row();
            let def = d.def();
            match &def {
                Ok(WorkDef::Plane(D::Offset { .. })) => {
                    plabel(ui, "Distance");
                    ui.add(
                        super::valuefield::ExprValue::new(&mut d.distance)
                            .speed(0.5)
                            .suffix(" mm"),
                    );
                    ui.end_row();
                }
                Ok(WorkDef::Plane(D::Angle { .. })) => {
                    plabel(ui, "Angle");
                    let mut deg = d.angle.to_degrees();
                    if ui
                        .add(
                            super::valuefield::ExprValue::new(&mut deg)
                                .speed(1.0)
                                .suffix(" °"),
                        )
                        .changed()
                    {
                        d.angle = deg.to_radians();
                    }
                    ui.end_row();
                }
                Ok(WorkDef::Point(P::Fixed { .. })) => {
                    for (k, n) in ["X", "Y", "Z"].iter().enumerate() {
                        plabel(ui, n);
                        ui.add(
                            super::valuefield::ExprValue::new(&mut d.xyz[k])
                                .speed(0.5)
                                .suffix(" mm"),
                        );
                        ui.end_row();
                    }
                }
                Ok(WorkDef::Point(P::SurfaceLine { .. })) => {
                    plabel(ui, "Solution");
                    ui.horizontal(|ui| {
                        ui.label(format!("{}", d.index + 1));
                        if ui
                            .small_button("Next")
                            .on_hover_text("The next point where the line meets the face")
                            .clicked()
                        {
                            d.index += 1;
                            let mut probe = d.clone();
                            if !matches!(self.preview_draft(&probe), Some(Ok(_))) {
                                probe.index = 0;
                                d.index = 0;
                            }
                        }
                    });
                    ui.end_row();
                }
                Ok(WorkDef::Ucs(UcsDef::Offsets { .. })) => {
                    for (k, n) in ["X", "Y", "Z"].iter().enumerate() {
                        plabel(ui, n);
                        ui.add(
                            super::valuefield::ExprValue::new(&mut d.ucs[k])
                                .speed(0.5)
                                .suffix(" mm"),
                        );
                        ui.end_row();
                    }
                    for (k, n) in ["Angle X", "Angle Y", "Angle Z"].iter().enumerate() {
                        plabel(ui, n);
                        let mut deg = d.ucs[3 + k].to_degrees();
                        if ui
                            .add(
                                super::valuefield::ExprValue::new(&mut deg)
                                    .speed(1.0)
                                    .suffix(" °"),
                            )
                            .changed()
                        {
                            d.ucs[3 + k] = deg.to_radians();
                        }
                        ui.end_row();
                    }
                }
                _ => {}
            }
            if let Ok(def) = &def {
                if WorkDraft::has_other(def) {
                    plabel(ui, "Other side");
                    ui.checkbox(&mut d.other, "")
                        .on_hover_text("The second of the two tangent planes");
                    ui.end_row();
                }
                if matches!(def, WorkDef::Plane(_)) {
                    plabel(ui, "Flip Normal");
                    ui.checkbox(&mut d.flip, "")
                        .on_hover_text("Reverse the plane's normal (its X stays)");
                    ui.end_row();
                }
            }
        });
        // Incomplete picks only grey the method out; a complete one that
        // cannot be built is a warning.
        let built = self.preview_draft(&d);
        if let Some(Err(e)) = &built {
            warning_box(ui, e);
        }
        let ok = matches!(built, Some(Ok(_)));
        self.work_draft = Some(d);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(ok, egui::Button::new("OK").min_size(egui::vec2(70.0, 22.0)))
                .clicked()
            {
                self.commit_work();
            }
            if ui
                .add(egui::Button::new("Cancel").min_size(egui::vec2(70.0, 22.0)))
                .clicked()
            {
                self.work_draft = None;
            }
        });
    }

    /// Properties of a work feature: method, inputs, values (expressions),
    /// options, visibility.
    pub(super) fn work_properties(&mut self, ui: &mut egui::Ui, k: usize, w: WorkFeature) {
        self.panel_header(ui, icon(WorkKind::of(&w.def)), &w.name);
        if let Some(h) = self.part().feature_health.get(&w.id) {
            warning_box(ui, h);
        }
        psection(ui, &format!("wp_props_{k}"), "Input", |ui| {
            plabel(ui, "Method");
            ui.label(w.def.method());
            ui.end_row();
            for (l, v) in input_rows(&w.def) {
                plabel(ui, l);
                ui.label(&v).on_hover_text(&v);
                ui.end_row();
            }
        });
        let table = self.part().parameter_table();
        let mut fx = Fields {
            table: &table,
            key: format!("feature{k}"),
            commits: Vec::new(),
            bodies: Vec::new(),
        };
        let f = Feature::Work(w.clone());
        let fields = crate::part::params::feature_field_names(&f);
        if !fields.is_empty() {
            psection(ui, &format!("wp_value_{k}"), "Value", |ui| {
                for field in fields {
                    field_row(ui, &mut fx, field_label(field), &f, field);
                }
            });
        }
        let commits = std::mem::take(&mut fx.commits);
        drop(fx);
        for (field, text) in commits {
            self.checkpoint("Edit Feature");
            match self.part_mut().set_feature_field(k, field, &text) {
                Ok(()) => {
                    if let Err(e) = self.part_mut().regenerate() {
                        self.file_status = Some(Err(e.to_string()));
                    }
                    self.scene_dirty = true;
                }
                Err(e) => {
                    self.history.cancel();
                    self.file_status = Some(Err(format!("Edit Feature: {e}")));
                }
            }
        }
        // Options: the other solution, Flip Normal.
        let mut def = w.def.clone();
        let other = match &mut def {
            WorkDef::Plane(
                D::TangentEdge { other, .. }
                | D::TangentPoint { other, .. }
                | D::TangentParallel { other, .. },
            ) => Some(other),
            _ => None,
        };
        let mut changed = None;
        if let Some(other) = other
            && ui
                .checkbox(other, "Other side")
                .on_hover_text("The second of the two tangent planes")
                .changed()
        {
            changed = Some(def.clone());
        }
        let mut flip = w.flip;
        if matches!(w.def, WorkDef::Plane(_))
            && ui
                .checkbox(&mut flip, "Flip Normal")
                .on_hover_text("Reverse the plane's normal (its X stays)")
                .changed()
        {
            self.checkpoint("Flip Normal");
            if let Some(Feature::Work(x)) = self.part_mut().features.get_mut(k) {
                x.flip = flip;
            }
            if let Err(e) = self.part_mut().regenerate() {
                self.file_status = Some(Err(e.to_string()));
            }
            self.scene_dirty = true;
        }
        if let Some(def) = changed {
            self.checkpoint("Edit Feature");
            let mut nw = w.clone();
            nw.def = def;
            if let Err(e) = self.part_mut().replace_feature(k, Feature::Work(nw)) {
                self.history.cancel();
                self.file_status = Some(Err(format!("Edit Feature: {e}")));
            }
            self.scene_dirty = true;
        }
        let mut visible = w.visible;
        if ui.checkbox(&mut visible, "Visible").changed() {
            self.checkpoint("Visibility");
            if let Some(Feature::Work(x)) = self.part_mut().features.get_mut(k) {
                x.visible = visible;
            }
        }
        ui.horizontal(|ui| {
            let sketch_plane = match &w.def {
                WorkDef::Plane(_) => Some(w.id.clone()),
                WorkDef::Ucs(_) => Some(format!("{}:XY", w.id)),
                _ => None,
            };
            if let Some(id) = sketch_plane
                && ui.button("New Sketch").clicked()
            {
                self.mode = Mode::PickPlane(PickPurpose::NewSketch);
                self.plane_picked(PlaneRef::Work(id));
            }
            if ui
                .button("Redefine")
                .on_hover_text("Pick the inputs again (same name)")
                .clicked()
            {
                self.redefine_work(k);
            }
            if ui.button("Delete").clicked() {
                self.delete_feature(k);
            }
        });
    }
}
