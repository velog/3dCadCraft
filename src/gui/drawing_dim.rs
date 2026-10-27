//! Edit Dimension (double-click a drawing dimension;
//! `docs/spec/4.7-drawing-commands.md`): the Text, Precision and Tolerance
//! and Inspection Dimension tabs over one `DimFormat`, with the label as it
//! will be drawn; OK writes it (one Undo step).

use eframe::egui::{self, RichText};

use super::CadCraftApp;
use super::valuefield::ExprValue;
use crate::drawing::AnnotationKind;
use crate::drawing::dimtext::{DimFormat, Inspection, InspectionShape, Label, TolMethod, VALUE};
use crate::drawing::fits;

/// The Edit Dimension window.
#[derive(Clone, Debug)]
pub(super) struct DimDialog {
    /// The dimension.
    pub id: u32,
    pub fmt: DimFormat,
    tab: Tab,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    Text,
    Tolerance,
    Inspection,
}

/// Symbols the Text tab inserts at the end of the text (square,
/// counterbore, countersink and depth wait for glyphs in the drawing
/// fonts).
const SYMBOLS: [&str; 6] = ["Ø", "R", "±", "°", "SØ", "SR"];

impl CadCraftApp {
    /// Double-click on a dimension.
    pub(super) fn open_dim_edit(&mut self, id: u32) {
        let Some(fmt) = self.drawing().and_then(|d| {
            d.sheet()?
                .annotations
                .iter()
                .find(|a| a.id == id)
                .and_then(|a| match &a.kind {
                    AnnotationKind::Dimension(x) => Some(x.format.clone()),
                    _ => None,
                })
        }) else {
            return;
        };
        self.drw.dim_edit = Some(DimDialog {
            id,
            fmt,
            tab: Tab::Text,
        });
    }

    /// The measured value of dimension `id` (from this frame's shown
    /// annotations), for the preview and the fit deviations.
    fn dim_value(&self, id: u32) -> Option<f64> {
        self.drw
            .dim_values
            .iter()
            .find(|(a, _)| *a == id)
            .map(|x| x.1)
    }

    pub(super) fn dim_edit_window(&mut self, ctx: &egui::Context) {
        let Some(mut dlg) = self.drw.dim_edit.clone() else {
            return;
        };
        let value = self.dim_value(dlg.id);
        let look = self.drw.look.clone();
        let (mut ok, mut cancel, mut open) = (false, false, true);
        let win = egui::Window::new("Edit Dimension")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .default_width(560.0);
        self.layout
            .place("drawing_dim_edit", ctx.content_rect(), false, win)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (t, name) in [
                        (Tab::Text, "Text"),
                        (Tab::Tolerance, "Precision and Tolerance"),
                        (Tab::Inspection, "Inspection Dimension"),
                    ] {
                        ui.selectable_value(&mut dlg.tab, t, name);
                    }
                });
                ui.separator();
                match dlg.tab {
                    Tab::Text => text_tab(ui, &mut dlg.fmt),
                    Tab::Tolerance => tolerance_tab(ui, &mut dlg.fmt, value, &look.dim),
                    Tab::Inspection => inspection_tab(ui, &mut dlg.fmt),
                }
                ui.separator();
                // The label as it will be drawn.
                if let Some(v) = value {
                    let l = Label::build(v, "", &dlg.fmt, &look.dim, 3.5, 1.0);
                    ui.horizontal(|ui| {
                        ui.weak("Preview:");
                        ui.label(RichText::new(l.texts().join("  ")).monospace());
                    });
                }
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
            self.drw.dim_edit = None;
            return;
        }
        if ok {
            let (id, fmt) = (dlg.id, dlg.fmt.clone());
            self.edit_drawing("Edit Dimension", move |d| {
                if let Some(AnnotationKind::Dimension(x)) = d
                    .sheet_mut()
                    .and_then(|s| s.annotations.iter_mut().find(|a| a.id == id))
                    .map(|a| &mut a.kind)
                {
                    x.format = fmt;
                }
            });
            self.drw.dim_edit = None;
            return;
        }
        self.drw.dim_edit = Some(dlg);
    }
}

fn text_tab(ui: &mut egui::Ui, f: &mut DimFormat) {
    if f.text.is_empty() {
        f.text = VALUE.to_string();
    }
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("dim_text_style")
            .selected_text("Note Text (ISO)")
            .show_ui(ui, |_| {});
        ui.add_enabled_ui(false, |ui| {
            for j in ["Left", "Center", "Right"] {
                let _ = ui.selectable_label(j == "Center", j);
            }
        })
        .response
        .on_disabled_hover_text("Justification: planned (spec 4.7 Edit Dimension)");
    });
    ui.add(
        egui::TextEdit::multiline(&mut f.text)
            .desired_rows(3)
            .desired_width(f32::INFINITY),
    )
    .on_hover_text("<<>> stands for the measured value (with its tolerance)");
    ui.horizontal_wrapped(|ui| {
        ui.label("Insert:");
        for s in SYMBOLS {
            if ui.small_button(s).clicked() {
                f.text.push_str(s);
            }
        }
        if !f.text.contains(VALUE) && ui.small_button("<<>>").clicked() {
            f.text.push_str(VALUE);
        }
    });
    ui.checkbox(&mut f.hide_value, "Hide Dimension Value");
    let mut measured = false;
    ui.add_enabled(
        false,
        egui::Checkbox::new(&mut measured, "Enable Measured Value"),
    )
    .on_disabled_hover_text("Measured value overrides: planned (spec 4.7 Edit Dimension)");
    if f.text == VALUE {
        f.text.clear();
    }
}

fn precision_combo(ui: &mut egui::Ui, id: &str, v: &mut Option<u8>, style: u8) {
    let text = |p: Option<u8>| match p {
        None => format!("Style ({style})"),
        Some(k) => {
            if k == 0 {
                "0".into()
            } else {
                format!("0.{}", "0".repeat(usize::from(k)))
            }
        }
    };
    egui::ComboBox::from_id_salt(id)
        .selected_text(text(*v))
        .show_ui(ui, |ui| {
            ui.selectable_value(v, None, text(None));
            for k in 0..=6u8 {
                ui.selectable_value(v, Some(k), text(Some(k)));
            }
        });
}

fn tolerance_tab(
    ui: &mut egui::Ui,
    f: &mut DimFormat,
    value: Option<f64>,
    style: &crate::drawing::style::DimensionStyle,
) {
    ui.columns(2, |cols| {
        let ui = &mut cols[0];
        ui.label(RichText::new("Tolerance Method").strong());
        egui::ScrollArea::vertical()
            .max_height(300.0)
            .id_salt("dim_tol_methods")
            .show(ui, |ui| {
                for m in TolMethod::ALL {
                    ui.selectable_value(&mut f.tol, m, m.label());
                }
            });
        let ui = &mut cols[1];
        ui.label(RichText::new("Tolerance").strong());
        let vals = f.tol.uses_values();
        egui::Grid::new("dim_tol_vals")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("Upper");
                ui.add_enabled(vals, ExprValue::new(&mut f.upper).suffix(" mm"));
                ui.end_row();
                ui.label("Lower");
                ui.add_enabled(
                    vals && f.tol != TolMethod::Symmetric,
                    ExprValue::new(&mut f.lower).suffix(" mm"),
                );
                ui.end_row();
            });
        let fits_on = f.tol.uses_fits();
        ui.add_space(4.0);
        ui.label(RichText::new("Hole / Shaft").strong());
        egui::Grid::new("dim_tol_fits")
            .num_columns(2)
            .show(ui, |ui| {
                for (name, v, list) in [
                    ("Hole", &mut f.hole, &fits::HOLES[..]),
                    ("Shaft", &mut f.shaft, &fits::SHAFTS[..]),
                ] {
                    ui.label(name);
                    ui.add_enabled_ui(fits_on, |ui| {
                        egui::ComboBox::from_id_salt(("dim_fit", name))
                            .selected_text(if v.is_empty() { "(none)" } else { v.as_str() })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(v, String::new(), "(none)");
                                for c in list {
                                    ui.selectable_value(v, (*c).to_string(), *c);
                                }
                            });
                    });
                    ui.end_row();
                }
            });
        if let (Some(v), true) = (value, fits_on) {
            match f.deviations(v) {
                // `+ 0.0` turns −0 into 0 (no "-0.000").
                Some((u, l)) => ui.weak(format!(
                    "{:+.3} / {:+.3} mm at {}",
                    u + 0.0,
                    l + 0.0,
                    crate::drawing::annot::format_value(v, 3)
                )),
                None => ui.weak("no ISO 286 values for this size / fit"),
            };
        }
        ui.add_space(4.0);
        ui.label(RichText::new("Precision").strong());
        egui::Grid::new("dim_prec").num_columns(2).show(ui, |ui| {
            ui.label("Primary Unit");
            precision_combo(ui, "dim_prec_p", &mut f.precision, style.precision);
            ui.end_row();
            ui.label("Primary Tolerance");
            precision_combo(
                ui,
                "dim_prec_t",
                &mut f.tol_precision,
                f.precision.unwrap_or(style.precision),
            );
            ui.end_row();
            for name in ["Alternate Unit", "Alternate Tolerance"] {
                ui.label(name);
                ui.add_enabled_ui(false, |ui| {
                    egui::ComboBox::from_id_salt(name)
                        .selected_text("-")
                        .show_ui(ui, |_| {});
                })
                .response
                .on_disabled_hover_text("Alternate units: planned (spec 4.7 Edit Dimension)");
                ui.end_row();
            }
        });
        let mut model = false;
        ui.add_enabled(
            false,
            egui::Checkbox::new(&mut model, "Model Value (override)"),
        )
        .on_disabled_hover_text("Model tolerances: planned with Retrieve Model Annotations");
    });
}

fn inspection_tab(ui: &mut egui::Ui, f: &mut DimFormat) {
    let mut on = f.inspection.is_some();
    if ui.checkbox(&mut on, "Inspection Dimension").changed() {
        f.inspection = on.then(|| Inspection {
            rate: "100%".into(),
            ..Default::default()
        });
    }
    let Some(i) = &mut f.inspection else {
        return;
    };
    ui.horizontal(|ui| {
        ui.label("Shape");
        ui.radio_value(&mut i.shape, InspectionShape::None, "None");
        ui.radio_value(&mut i.shape, InspectionShape::Round, "Round ends");
        ui.radio_value(&mut i.shape, InspectionShape::Angled, "Angled ends");
    });
    egui::Grid::new("dim_insp").num_columns(2).show(ui, |ui| {
        ui.label("Label");
        ui.text_edit_singleline(&mut i.label);
        ui.end_row();
        ui.label("Inspection Rate");
        ui.text_edit_singleline(&mut i.rate);
        ui.end_row();
    });
}
