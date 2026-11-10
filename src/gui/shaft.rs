//! The Shaft generator's panel (spec 6.2-design-calculators §3.1–3.2) on top of
//! the planned panel: the section list and its editor on the Design page, the
//! material, supports, loads and fatigue factors on the Calculation page, the
//! curves on the Graphs page; OK builds the shaft part (`design::shaft::part`)
//! and places it in the assembly.

use eframe::egui::{self, RichText};

use super::CadCraftApp;
use super::asm_planned::AsmPlanned;
use super::panels::plabel;
use super::part_planned::{PartDialog, Planned};
use super::valuefield::ExprValue;
use crate::assembly::relation::{ASSEMBLY, Geom};
use crate::design::shaft::{EdgeFeature, RingGroove, Section, SectionShape, ShaftDesign};
use crate::design::shaft_calc::{
    Load, Material, ReducedStress, Reliability, ShaftLoads, ShaftResults, Support, SupportKind,
    calculate,
};
use crate::document::{DocRef, DocumentData};
use crate::kernel::{Point3, Transform3, Vector3};

/// The panel's own state (`PartDialog::shaft`).
#[derive(Clone, Debug)]
pub struct ShaftPanel {
    /// The sections.
    pub design: ShaftDesign,
    /// The section being edited.
    pub selected: usize,
    /// The calculation's inputs.
    pub loads: ShaftLoads,
    /// The material preset (index into `Material::presets`; past the end: typed).
    pub preset: usize,
    /// The last calculation (for the inputs it was made with).
    pub results: Option<(ShaftLoads, ShaftDesign, Result<ShaftResults, String>)>,
}

impl Default for ShaftPanel {
    fn default() -> ShaftPanel {
        let design = ShaftDesign::starter();
        let len = design.length();
        ShaftPanel {
            design,
            selected: 0,
            loads: ShaftLoads {
                supports: vec![
                    Support {
                        z: 20.0,
                        kind: SupportKind::Fixed,
                    },
                    Support {
                        z: len - 30.0,
                        kind: SupportKind::Free,
                    },
                ],
                loads: vec![Load::Radial {
                    z: 130.0,
                    force: 1000.0,
                    angle: 0.0,
                }],
                ..ShaftLoads::default()
            },
            preset: 0,
            results: None,
        }
    }
}

/// Whether `d` is the Shaft panel.
pub(super) fn is_shaft(d: &PartDialog) -> bool {
    d.kind == Planned::Asm(AsmPlanned::Shaft)
}

/// A small picture of a section kind, beside its row (rule 23).
fn section_icon(ui: &mut egui::Ui, shape: &SectionShape) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 14.0), egui::Sense::hover());
    let p = ui.painter();
    let ink = ui.visuals().text_color();
    let stroke = egui::Stroke::new(1.2, ink);
    let c = rect.center();
    match shape {
        SectionShape::Cylinder { .. } => {
            p.rect_stroke(
                egui::Rect::from_center_size(c, egui::vec2(14.0, 8.0)),
                0.0,
                stroke,
                egui::StrokeKind::Middle,
            );
        }
        SectionShape::Cone { .. } => {
            let pts = vec![
                c + egui::vec2(-7.0, -5.0),
                c + egui::vec2(7.0, -2.5),
                c + egui::vec2(7.0, 2.5),
                c + egui::vec2(-7.0, 5.0),
            ];
            p.add(egui::Shape::closed_line(pts, stroke));
        }
        SectionShape::Polygon { .. } => {
            let pts = (0..6)
                .map(|k| {
                    let a = std::f32::consts::TAU * k as f32 / 6.0;
                    c + egui::vec2(6.0 * a.cos(), 6.0 * a.sin())
                })
                .collect();
            p.add(egui::Shape::closed_line(pts, stroke));
        }
    }
}

/// A number field with its unit label after it.
fn field(ui: &mut egui::Ui, v: &mut f64, unit: &str) -> bool {
    let changed = ui
        .add(ExprValue::new(v).suffix(if unit == "mm" { " mm" } else { "" }))
        .changed();
    if unit != "mm" && !unit.is_empty() {
        ui.label(RichText::new(unit).weak());
    }
    changed
}

/// The Sections row of the Design page: insert buttons, the list, the editor.
pub(super) fn sections_ui(d: &mut PartDialog, ui: &mut egui::Ui) {
    let p = &mut d.shaft;
    plabel(ui, "Sections");
    ui.vertical(|ui| {
        ui.horizontal_wrapped(|ui| {
            let at = (p.selected + 1).min(p.design.sections.len());
            let last = p
                .design
                .sections
                .get(p.selected)
                .map_or(40.0, |s| s.diameters().1);
            if ui.button("Insert Cylinder").clicked() {
                p.design.sections.insert(at, Section::cylinder(last, 40.0));
                p.selected = at;
            }
            if ui.button("Insert Cone").clicked() {
                p.design
                    .sections
                    .insert(at, Section::cone(last, last * 0.8, 30.0));
                p.selected = at;
            }
            if ui.button("Insert Polygon").clicked() {
                p.design
                    .sections
                    .insert(at, Section::polygon(6, last, 20.0));
                p.selected = at;
            }
            for t in [
                "Bore on the left",
                "Bore on the right",
                "Split Selected Section",
            ] {
                ui.add_enabled(false, egui::Button::new(t))
                    .on_disabled_hover_text("Spec 6.2-design-calculators §3.1: comes later");
            }
        });
        let mut z = 0.0;
        let mut act: Option<(usize, i32)> = None;
        for (k, s) in p.design.sections.iter().enumerate() {
            ui.horizontal(|ui| {
                section_icon(ui, &s.shape);
                let mut text = s.label();
                if let Some(l) = s.keyway {
                    text.push_str(&format!(", keyway {l:.0}"));
                }
                if !s.grooves.is_empty() {
                    text.push_str(&format!(", {} groove(s)", s.grooves.len()));
                }
                if ui
                    .selectable_label(p.selected == k, format!("{}. {text}", k + 1))
                    .on_hover_text(format!("from z = {z:.1} mm"))
                    .clicked()
                {
                    p.selected = k;
                }
                if ui.small_button("Up").clicked() && k > 0 {
                    act = Some((k, -1));
                }
                if ui.small_button("Down").clicked() {
                    act = Some((k, 1));
                }
                if ui.small_button("Delete").clicked() {
                    act = Some((k, 0));
                }
            });
            z += s.length;
        }
        if let Some((k, dir)) = act {
            let n = p.design.sections.len();
            match dir {
                0 => {
                    p.design.sections.remove(k);
                    p.selected = p.selected.min(p.design.sections.len().saturating_sub(1));
                }
                -1 => {
                    p.design.sections.swap(k, k - 1);
                    p.selected = k - 1;
                }
                _ if k + 1 < n => {
                    p.design.sections.swap(k, k + 1);
                    p.selected = k + 1;
                }
                _ => {}
            }
        }
        ui.label(RichText::new(format!("Shaft length {:.2} mm", p.design.length())).weak());
        let sel = p.selected;
        let Some(s) = p.design.sections.get_mut(sel) else {
            return;
        };
        ui.separator();
        ui.label(RichText::new(format!("Section {}", sel + 1)).strong());
        egui::Grid::new("shaft_section")
            .num_columns(2)
            .show(ui, |ui| {
                match &mut s.shape {
                    SectionShape::Cylinder { d } => {
                        ui.label("Main Diameter (D)");
                        field(ui, d, "mm");
                        ui.end_row();
                    }
                    SectionShape::Cone { d1, d2 } => {
                        ui.label("First Diameter (D1)");
                        field(ui, d1, "mm");
                        ui.end_row();
                        ui.label("Second Diameter (D2)");
                        field(ui, d2, "mm");
                        ui.end_row();
                    }
                    SectionShape::Polygon { sides, d } => {
                        ui.label("Number of Edges (N)");
                        let mut n = f64::from(*sides);
                        if ui.add(ExprValue::new(&mut n).range(3.0..=64.0)).changed() {
                            *sides = n.round() as u32;
                        }
                        ui.end_row();
                        ui.label("Diameter Circumscribed");
                        field(ui, d, "mm");
                        ui.end_row();
                    }
                }
                ui.label("Length (L)");
                field(ui, &mut s.length, "mm");
                ui.end_row();
                edge_row(ui, "Start edge", &mut s.start, ("shaft_start", sel));
                edge_row(ui, "End edge", &mut s.end, ("shaft_end", sel));
                let cylinder = matches!(s.shape, SectionShape::Cylinder { .. });
                ui.label("Keyway Groove");
                ui.horizontal(|ui| {
                let mut on = s.keyway.is_some();
                if ui.checkbox(&mut on, "").changed() {
                    s.keyway = on.then_some((s.length * 0.8).floor());
                }
                if let Some(l) = s.keyway.as_mut() {
                    ui.label("length");
                    field(ui, l, "mm");
                    if let Ok(k) = crate::design::key::size_for(s.diameters().0) {
                        ui.label(
                            RichText::new(format!("DIN 6885 A {}×{}", k.b, k.h)).weak(),
                        );
                    }
                }
            })
            .response
            .on_hover_text(
                "Sets the keyway's notch factor in the calculation (RP-1123 Table V); the groove \
                 in the model comes later",
            );
                ui.end_row();
                ui.label("Retaining Rings");
                ui.vertical(|ui| {
                    let mut gone = None;
                    for (g, gr) in s.grooves.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label("at");
                            field(ui, &mut gr.at, "mm");
                            ui.label("m");
                            field(ui, &mut gr.width, "mm");
                            ui.label("t");
                            field(ui, &mut gr.depth, "mm");
                            if ui.small_button("Delete").clicked() {
                                gone = Some(g);
                            }
                        });
                    }
                    if let Some(g) = gone {
                        s.grooves.remove(g);
                    }
                    let r =
                        ui.add_enabled(cylinder, egui::Button::new("Add Retaining Ring groove"));
                    if r.clicked() {
                        let d = s.diameters().0;
                        s.grooves.push(RingGroove {
                            at: s.length / 2.0,
                            width: (d / 25.0).clamp(0.8, 4.2),
                            depth: (d / 40.0).clamp(0.3, 3.0),
                        });
                    }
                    r.on_disabled_hover_text("Grooves go into cylinders");
                });
                ui.end_row();
            });
        if let Err(e) = p.design.validate() {
            ui.colored_label(super::theme::tokens().error, format!("Shaft: {e}"));
        }
    });
    ui.end_row();
}

fn edge_row(ui: &mut egui::Ui, label: &str, e: &mut EdgeFeature, id: (&str, usize)) {
    ui.label(label);
    ui.horizontal(|ui| {
        let names = ["None", "Chamfer", "Fillet"];
        let mut k = match e {
            EdgeFeature::None => 0,
            EdgeFeature::Chamfer(_) => 1,
            EdgeFeature::Fillet(_) => 2,
        };
        let size = match e {
            EdgeFeature::Chamfer(v) | EdgeFeature::Fillet(v) => *v,
            EdgeFeature::None => 1.0,
        };
        let before = k;
        egui::ComboBox::from_id_salt(id)
            .selected_text(names[k])
            .show_ui(ui, |ui| {
                for (i, n) in names.iter().enumerate() {
                    ui.selectable_value(&mut k, i, *n);
                }
            });
        let mut size = size;
        if k > 0 {
            field(ui, &mut size, "mm");
        }
        if k != before || k > 0 {
            *e = match k {
                1 => EdgeFeature::Chamfer(size),
                2 => EdgeFeature::Fillet(size),
                _ => EdgeFeature::None,
            };
        }
    });
    ui.end_row();
}

/// The Calculation page: material, properties, supports, loads, fatigue, results.
pub(super) fn calculation_ui(d: &mut PartDialog, ui: &mut egui::Ui) {
    let p = &mut d.shaft;
    let presets = Material::presets();
    plabel(ui, "Material");
    ui.vertical(|ui| {
        let name = presets
            .get(p.preset)
            .map_or("User material".to_owned(), |m| m.name.clone());
        egui::ComboBox::from_id_salt("shaft_material")
            .selected_text(name)
            .width(240.0)
            .show_ui(ui, |ui| {
                for (i, m) in presets.iter().enumerate() {
                    if ui
                        .selectable_label(p.preset == i, m.name.as_str())
                        .clicked()
                    {
                        p.preset = i;
                        p.loads.material = m.clone();
                    }
                }
            });
        let m = &mut p.loads.material;
        let mut typed = false;
        egui::Grid::new("shaft_material_grid").show(ui, |ui| {
            for (label, v) in [
                ("Ultimate Tensile Strength", &mut m.ultimate),
                ("Yield Strength", &mut m.yield_strength),
                ("Fatigue Limit (polished)", &mut m.fatigue_limit),
                ("Modulus of Elasticity", &mut m.e),
                ("Modulus of Rigidity", &mut m.g),
            ] {
                ui.label(label);
                ui.horizontal(|ui| typed |= field(ui, v, "MPa"));
                ui.end_row();
            }
            ui.label("Density");
            ui.horizontal(|ui| typed |= field(ui, &mut m.density, "kg/m³"));
            ui.end_row();
            ui.label("");
            typed |= ui
                .checkbox(&mut m.hardened, "Hardened above 200 BHN")
                .changed();
            ui.end_row();
        });
        if typed {
            p.preset = presets.len();
        }
    });
    ui.end_row();
    plabel(ui, "Calculation Properties");
    ui.vertical(|ui| {
        ui.checkbox(&mut p.loads.use_density, "Use density");
        ui.horizontal(|ui| {
            ui.label("Number of shaft divisions");
            let mut n = p.loads.divisions as f64;
            if ui
                .add(ExprValue::new(&mut n).range(10.0..=5000.0))
                .changed()
            {
                p.loads.divisions = n.round() as usize;
            }
        });
        ui.add_enabled(
            false,
            egui::Checkbox::new(&mut false, "Use shear displacement ratio"),
        )
        .on_disabled_hover_text("Spec 6.2-design-calculators §3.2: shear deflection comes later");
        ui.horizontal(|ui| {
            ui.label("Mode of reduced stress");
            ui.radio_value(&mut p.loads.reduced, ReducedStress::Hmh, "HMH");
            ui.radio_value(&mut p.loads.reduced, ReducedStress::Tresca, "Tresca-Guest");
        });
    });
    ui.end_row();
    let len = p.design.length();
    plabel(ui, "Supports");
    ui.vertical(|ui| {
        for (k, s) in p.loads.supports.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt(("shaft_support", k))
                    .selected_text(match s.kind {
                        SupportKind::Fixed => "Fixed",
                        SupportKind::Free => "Free",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut s.kind, SupportKind::Fixed, "Fixed");
                        ui.selectable_value(&mut s.kind, SupportKind::Free, "Free");
                    });
                ui.label("at z");
                field(ui, &mut s.z, "mm");
            });
        }
        ui.horizontal(|ui| {
            let two = p.loads.supports.len() >= 2;
            if ui
                .add_enabled(!two, egui::Button::new("Add Free Support"))
                .on_disabled_hover_text("The calculation takes two supports (more come later)")
                .clicked()
            {
                p.loads.supports.push(Support {
                    z: len,
                    kind: SupportKind::Free,
                });
            }
            if ui.button("Remove Last").clicked() {
                p.loads.supports.pop();
            }
        });
    });
    ui.end_row();
    plabel(ui, "Loads");
    ui.vertical(|ui| {
        let mut gone = None;
        for (k, l) in p.loads.loads.iter_mut().enumerate() {
            ui.horizontal_wrapped(|ui| {
                match l {
                    Load::Radial { z, force, angle } => {
                        ui.label("Force at z");
                        field(ui, z, "mm");
                        field(ui, force, "N");
                        angle_field(ui, angle);
                    }
                    Load::Axial { z, force } => {
                        ui.label("Axial Force at z");
                        field(ui, z, "mm");
                        field(ui, force, "N");
                    }
                    Load::Continuous { z0, z1, q, angle } => {
                        ui.label("Continuous Load from");
                        field(ui, z0, "mm");
                        ui.label("to");
                        field(ui, z1, "mm");
                        field(ui, q, "N/mm");
                        angle_field(ui, angle);
                    }
                    Load::Torque { z, torque } => {
                        ui.label("Torque at z");
                        field(ui, z, "mm");
                        let mut nm = *torque / 1000.0;
                        if field(ui, &mut nm, "N·m") {
                            *torque = nm * 1000.0;
                        }
                    }
                }
                if ui.small_button("Delete").clicked() {
                    gone = Some(k);
                }
            });
        }
        if let Some(k) = gone {
            p.loads.loads.remove(k);
        }
        ui.horizontal_wrapped(|ui| {
            let mid = len / 2.0;
            if ui.button("Add Force").clicked() {
                p.loads.loads.push(Load::Radial {
                    z: mid,
                    force: 1000.0,
                    angle: 0.0,
                });
            }
            if ui.button("Add Axial Force").clicked() {
                p.loads.loads.push(Load::Axial {
                    z: len,
                    force: 500.0,
                });
            }
            if ui.button("Add Continuous Load").clicked() {
                p.loads.loads.push(Load::Continuous {
                    z0: len * 0.4,
                    z1: len * 0.6,
                    q: 10.0,
                    angle: 0.0,
                });
            }
            if ui.button("Add Torque").clicked() {
                // A pair: in at the start, out at the end (they must sum to zero).
                p.loads.loads.push(Load::Torque {
                    z: 0.0,
                    torque: 100e3,
                });
                p.loads.loads.push(Load::Torque {
                    z: len,
                    torque: -100e3,
                });
            }
            ui.add_enabled(false, egui::Button::new("Add Bending Moment"))
                .on_disabled_hover_text("Spec 6.2-design-calculators §3.2: comes later");
        });
    });
    ui.end_row();
    let f = &mut p.loads.fatigue;
    plabel(ui, "Fatigue (NASA RP-1123)");
    egui::Grid::new("shaft_fatigue").show(ui, |ui| {
        ui.label("Surface factor ka");
        field(ui, &mut f.surface, "");
        ui.end_row();
        ui.label("Reliability kc");
        egui::ComboBox::from_id_salt("shaft_reliability")
            .selected_text(f.reliability.label())
            .show_ui(ui, |ui| {
                for r in Reliability::ALL {
                    ui.selectable_value(&mut f.reliability, r, r.label());
                }
            });
        ui.end_row();
        ui.label("Temperature factor kd");
        field(ui, &mut f.temperature, "");
        ui.end_row();
        ui.label("Other factors kf·kg·kh·ki");
        field(ui, &mut f.other, "");
        ui.end_row();
        ui.label("Stress concentration Kt");
        field(ui, &mut f.kt_bending, "");
        ui.end_row();
        ui.label("Notch sensitivity q");
        field(ui, &mut f.notch_sensitivity, "");
        ui.end_row();
        ui.label("Required safety factor");
        field(ui, &mut f.safety, "");
        ui.end_row();
    });
    ui.end_row();
    plabel(ui, "");
    if ui.button("Calculate").clicked() {
        let r = calculate(&p.design, &p.loads);
        p.results = Some((p.loads.clone(), p.design.clone(), r));
    }
    ui.end_row();
    results_ui(p, ui);
}

fn angle_field(ui: &mut egui::Ui, a: &mut f64) {
    let mut deg = a.to_degrees();
    if ui.add(ExprValue::new(&mut deg).suffix("°")).changed() {
        *a = deg.to_radians();
    }
}

/// The Results rows under Calculate.
fn results_ui(p: &ShaftPanel, ui: &mut egui::Ui) {
    let Some((loads, design, r)) = &p.results else {
        return;
    };
    let stale = loads != &p.loads || design != &p.design;
    plabel(ui, "Results");
    ui.vertical(|ui| {
        if stale {
            ui.label(RichText::new("The inputs changed: Calculate again").weak());
        }
        match r {
            Err(e) => {
                ui.colored_label(super::theme::tokens().error, format!("Shaft: {e}"));
            }
            Ok(r) => {
                let t = super::theme::tokens();
                let (fs, at) = r.min_safety();
                let need = loads.fatigue.safety;
                let rows = [
                    ("Length", format!("{:.2} mm", design.length())),
                    ("Mass", format!("{:.3} kg", r.mass)),
                    (
                        "Maximum Bending Stress",
                        format!("{:.2} MPa", ShaftResults::max_abs(&r.bending_stress)),
                    ),
                    (
                        "Maximum Torsional Stress",
                        format!("{:.2} MPa", ShaftResults::max_abs(&r.torsion_stress)),
                    ),
                    (
                        "Maximum Reduced Stress",
                        format!("{:.2} MPa", ShaftResults::max_abs(&r.reduced_stress)),
                    ),
                    (
                        "Maximum Deflection",
                        format!("{:.4} mm", ShaftResults::max_abs(&r.deflection)),
                    ),
                    ("Angle of Twist", format!("{:.4}°", r.twist)),
                    (
                        "Critical Speed (eq. 33)",
                        r.critical_rpm.map_or("—".into(), |v| format!("{v:.0} rpm")),
                    ),
                ];
                egui::Grid::new("shaft_results").show(ui, |ui| {
                    for (a, b) in rows {
                        ui.label(a);
                        ui.label(b);
                        ui.end_row();
                    }
                    ui.label("Fatigue Safety (eq. 17)");
                    let c = if fs >= need { t.ok } else { t.error };
                    ui.colored_label(c, format!("{fs:.2} at z = {at:.1} mm (required {need:.2})"));
                    ui.end_row();
                    for (k, re) in r.reactions.iter().enumerate() {
                        ui.label(format!("Support {} reaction", k + 1));
                        ui.label(format!(
                            "Y {:.1} N, X {:.1} N, axial {:.1} N",
                            re[0], re[1], re[2]
                        ));
                        ui.end_row();
                    }
                });
            }
        }
    });
    ui.end_row();
}

/// The Graphs page: the curves of the last calculation.
pub(super) fn graphs_ui(d: &mut PartDialog, ui: &mut egui::Ui) {
    plabel(ui, "Graphs");
    ui.vertical(|ui| {
        let Some((_, _, Ok(r))) = &d.shaft.results else {
            ui.label(RichText::new("Calculate on the Calculation page first").weak());
            return;
        };
        for (title, v, unit) in [
            ("Shear Force", &r.shear, "N"),
            ("Bending Moment", &r.moment, "N·mm"),
            ("Deflection", &r.deflection, "mm"),
            ("Reduced Stress", &r.reduced_stress, "MPa"),
            ("Diameter", &r.d, "mm"),
        ] {
            plot(ui, title, &r.z, v, unit);
        }
    });
    ui.end_row();
}

fn plot(ui: &mut egui::Ui, title: &str, z: &[f64], v: &[f64], unit: &str) {
    let max = ShaftResults::max_abs(v).max(1e-12);
    ui.label(format!("{title} (max {max:.4} {unit})"));
    let (rect, _) = ui.allocate_exact_size(egui::vec2(260.0, 60.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    let t = super::theme::tokens();
    p.rect_stroke(
        rect,
        2.0,
        egui::Stroke::new(1.0, ui.visuals().weak_text_color()),
        egui::StrokeKind::Inside,
    );
    let len = z.last().copied().unwrap_or(1.0).max(1e-9);
    let pts: Vec<egui::Pos2> = z
        .iter()
        .zip(v)
        .map(|(&z, &y)| {
            egui::pos2(
                rect.left() + (z / len) as f32 * rect.width(),
                rect.bottom() - (y.abs() / max) as f32 * (rect.height() - 4.0) - 2.0,
            )
        })
        .collect();
    p.add(egui::Shape::line(pts, egui::Stroke::new(1.5, t.accent)));
}

impl CadCraftApp {
    /// The Shaft panel's OK: the part, its document and its occurrence (one
    /// Undo step). Returns whether the panel can close.
    pub(super) fn shaft_ok(&mut self, d: &PartDialog) -> bool {
        let design = &d.shaft.design;
        let part = match crate::design::shaft::part(design) {
            Ok(p) => p,
            Err(e) => {
                self.file_status = Some(Err(format!("Shaft: {e}")));
                return false;
            }
        };
        let place = match self.shaft_placement(d) {
            Ok(t) => t,
            Err(e) => {
                self.file_status = Some(Err(format!("Shaft: {e}")));
                return false;
            }
        };
        let mut trial = self.project.clone();
        let asm = self.part_id;
        let Some(DocumentData::Assembly(_)) = trial.document(asm).map(|x| &x.data) else {
            self.file_status = Some(Err("Shaft: no assembly is open".into()));
            return false;
        };
        let name = trial.free_name("Shaft");
        let mut part = part;
        if let Err(e) = part.regenerate() {
            self.file_status = Some(Err(format!("Shaft: {e}")));
            return false;
        }
        if let Some((_, msg)) = part.feature_health.iter().next() {
            self.file_status = Some(Err(format!("Shaft: {msg}")));
            return false;
        }
        let id = trial.add_document(name.clone(), DocumentData::Part(part));
        let Some(DocumentData::Assembly(a)) = trial.document_mut(asm).map(|x| &mut x.data) else {
            return false;
        };
        let first = a.occurrences.is_empty();
        let occ = a.place(DocRef::local(id), &name, place, first);
        self.history.break_coalescing();
        self.checkpoint("Shaft");
        self.project = trial;
        self.history.break_coalescing();
        self.asm_solve(None);
        self.asm.sel = vec![occ];
        self.asm.sub.clear();
        self.scene_dirty = true;
        self.file_status = Some(Ok(format!(
            "Shaft: {name} placed ({} sections, {:.1} mm)",
            design.sections.len(),
            design.length()
        )));
        true
    }

    /// Where the shaft goes: its start on the Start plane, along the Axis
    /// (or the plane's normal), reversed by Flip; the origin without picks.
    fn shaft_placement(&self, d: &PartDialog) -> Result<Transform3, String> {
        let a = self.assembly().ok_or("no assembly")?;
        let geom = |key: &str| -> Result<Option<Geom>, String> {
            let Some(r) = d.refs.get(key).and_then(|v| v.first()) else {
                return Ok(None);
            };
            let g = self.asm_local_in(a, r)?;
            Ok(Some(if r.occurrence() == ASSEMBLY {
                g
            } else {
                let o = a
                    .find(r.occurrence())
                    .ok_or("the component no longer exists")?;
                g.moved(&o.transform)
            }))
        };
        let axis = match geom("axis")? {
            Some(Geom::Axis { p, d } | Geom::Cylinder { p, d, .. }) => Some((p, d)),
            Some(Geom::Circle { c, n, .. }) => Some((c, n)),
            Some(_) => return Err("the axis must be a cylindrical face, an axis or an edge".into()),
            None => None,
        };
        let plane = match geom("start")? {
            Some(Geom::Plane { p, n }) => Some((p, n)),
            Some(_) => return Err("the start must be a planar face or a plane".into()),
            None => None,
        };
        let (o, dir): (Point3, Vector3) = match (axis, plane) {
            (Some((p, dv)), Some((q, n))) => {
                let den = dv.dot(&n);
                if den.abs() < 1e-9 {
                    return Err("the axis lies in the start plane".into());
                }
                let t = (q - p).dot(&n) / den;
                let dir = if den < 0.0 { -dv } else { dv };
                (p + dv * t, dir)
            }
            (Some((p, dv)), None) => (p, dv),
            (None, Some((q, n))) => (q, n),
            (None, None) => (Point3::origin(), Vector3::z()),
        };
        let dir = if d.check(0, "Flip") { -dir } else { dir };
        Ok(crate::assembly::create::frame_on(o, dir))
    }
}
