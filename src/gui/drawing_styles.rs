//! The drawing's Style and Standard Editor (`docs/spec/4.6-drawing-styles.md`):
//! the standard and every style type on the left, the selected style's
//! settings on the right (Inventor's tabs for dimension styles). Options
//! without logic yet are shown disabled with their status. Every change is
//! one Undo step and redraws the sheet.

use eframe::egui::{self, Color32, RichText};

use super::CadCraftApp;
use super::valuefield::ExprValue;
use crate::drawing::style::{
    DrawingStyles, LineType, PLANNED_TYPES, StyleType, Terminator, TextOrientation,
};
use crate::drawing::{Projection, StandardKind};

/// What the editor shows.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct StylesEditor {
    /// The selected style type (`None`: the standard).
    pub kind: Option<StyleType>,
    /// The selected style's name.
    pub name: String,
    /// The dimension style's tab.
    tab: DimTab,
    /// Rename in progress.
    rename: Option<String>,
    /// Last refusal (delete in use …).
    error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DimTab {
    Units,
    AlternateUnits,
    Display,
    Text,
    Tolerance,
    Options,
    NotesLeaders,
}

impl StylesEditor {
    /// The editor opened on the standard.
    pub(super) fn new() -> Self {
        Self {
            kind: None,
            name: String::new(),
            tab: DimTab::Units,
            rename: None,
            error: None,
        }
    }
}

/// A disabled widget with the reason.
fn planned(ui: &mut egui::Ui, text: &str, why: &str) {
    ui.add_enabled(false, egui::Label::new(text))
        .on_disabled_hover_text(why);
}

fn color_edit(ui: &mut egui::Ui, c: &mut [u8; 3]) {
    let mut col = Color32::from_rgb(c[0], c[1], c[2]);
    if ui.color_edit_button_srgba(&mut col).changed() {
        *c = [col.r(), col.g(), col.b()];
    }
}

fn mm(ui: &mut egui::Ui, label: &str, v: &mut f64, range: std::ops::RangeInclusive<f64>) {
    ui.label(label);
    ui.add(ExprValue::new(v).range(range).suffix(" mm"));
    ui.end_row();
}

fn combo_names(ui: &mut egui::Ui, id: &str, value: &mut String, names: &[String]) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(value.clone())
        .width(180.0)
        .show_ui(ui, |ui| {
            for n in names {
                ui.selectable_value(value, n.clone(), n);
            }
        });
}

impl CadCraftApp {
    /// Manage › Styles Editor in a drawing.
    pub(super) fn open_drawing_styles(&mut self) {
        self.drw.styles_editor = Some(StylesEditor::new());
    }

    /// The editor window.
    pub(super) fn drawing_styles_window(&mut self, ctx: &egui::Context) {
        let Some(mut ed) = self.drw.styles_editor.clone() else {
            return;
        };
        let Some(doc) = self.drawing().cloned() else {
            return;
        };
        let doc_name = self
            .project
            .document(self.part_id)
            .map_or_else(String::new, |d| d.name.clone());
        let mut st = doc.styles.clone();
        let mut standard = doc.standard;
        let mut open = true;
        let mut done = false;
        let win = egui::Window::new(format!("Style and Standard Editor [{doc_name}]"))
            .open(&mut open)
            .resizable(true)
            .collapsible(false)
            .default_size([820.0, 560.0]);
        self.layout
            .place("drawing_styles", ctx.content_rect(), true, win)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .button("New...")
                        .on_hover_text("A copy of the selected style")
                        .clicked()
                        && let Some(k) = ed.kind
                        && let Some(n) = st.duplicate(k, &ed.name)
                    {
                        ed.name = n;
                    }
                    if ui.button("Rename").clicked() && ed.kind.is_some() {
                        ed.rename = Some(ed.name.clone());
                    }
                    if ui.button("Delete").clicked()
                        && let Some(k) = ed.kind
                    {
                        match st.delete(k, &ed.name) {
                            Ok(()) => ed.name = st.names(k).first().cloned().unwrap_or_default(),
                            Err(e) => ed.error = Some(format!("Delete: {e}")),
                        }
                    }
                    ui.add_enabled(false, egui::Button::new("Purge"))
                        .on_disabled_hover_text("Planned (spec 4.6 §3.1)");
                    ui.add_enabled(false, egui::Button::new("Import"))
                        .on_disabled_hover_text("Style library on disk: planned (spec 4.6 §3.1)");
                    ui.separator();
                    ui.weak("Changes apply at once (Undo takes them back).");
                });
                if let Some(e) = &ed.error {
                    ui.colored_label(Color32::from_rgb(220, 70, 60), e);
                }
                if let (Some(k), Some(r)) = (ed.kind, ed.rename.as_mut()) {
                    let mut commit = false;
                    ui.horizontal(|ui| {
                        ui.label("New name");
                        let resp = ui.text_edit_singleline(r);
                        commit = ui.button("OK").clicked()
                            || (resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                    });
                    if commit {
                        let new = r.clone();
                        match st.rename(k, &ed.name, &new) {
                            Ok(()) => {
                                ed.name = new.trim().to_string();
                                ed.rename = None;
                            }
                            Err(e) => ed.error = Some(format!("Rename: {e}")),
                        }
                    }
                }
                ui.separator();
                ui.horizontal_top(|ui| {
                    // Left: the standard and the style types.
                    ui.vertical(|ui| {
                        ui.set_width(230.0);
                        egui::ScrollArea::vertical()
                            .id_salt("styles_tree")
                            .max_height(470.0)
                            .show(ui, |ui| {
                                let std_name = format!("Standard: {}", standard.kind.label());
                                if ui
                                    .selectable_label(
                                        ed.kind.is_none(),
                                        RichText::new(std_name).strong(),
                                    )
                                    .clicked()
                                {
                                    ed.kind = None;
                                    ed.rename = None;
                                }
                                for t in StyleType::ALL {
                                    egui::CollapsingHeader::new(t.label())
                                        .id_salt(("style_type", t.label()))
                                        .default_open(ed.kind == Some(t))
                                        .show(ui, |ui| {
                                            for n in st.names(t) {
                                                let used = st.in_use(t, &n);
                                                let text = if used {
                                                    RichText::new(&n).strong()
                                                } else {
                                                    RichText::new(&n)
                                                };
                                                let on = ed.kind == Some(t) && ed.name == n;
                                                let r = ui.selectable_label(on, text);
                                                if r.clicked() {
                                                    ed.kind = Some(t);
                                                    ed.name = n.clone();
                                                    ed.rename = None;
                                                    ed.error = None;
                                                }
                                                if used {
                                                    r.on_hover_text("In use (bold)");
                                                }
                                            }
                                        });
                                }
                                ui.separator();
                                for (name, why) in PLANNED_TYPES {
                                    ui.add_enabled(false, egui::Label::new(name))
                                        .on_disabled_hover_text(format!("Planned: {why}"));
                                }
                            });
                    });
                    ui.separator();
                    // Right: the selection's settings.
                    ui.vertical(|ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("styles_pane")
                            .max_height(470.0)
                            .show(ui, |ui| match ed.kind {
                                None => standard_pane(ui, &mut standard, &mut st),
                                Some(t) => style_pane(ui, t, &ed.name.clone(), &mut st, &mut ed),
                            });
                    });
                });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Done").clicked() {
                        done = true;
                    }
                });
            });
        if st != doc.styles || standard != doc.standard {
            self.edit_drawing("Edit Styles", |d| {
                d.styles = st;
                d.standard = standard;
            });
        }
        self.drw.styles_editor = (open && !done).then_some(ed);
    }
}

/// The Standard: projection type, units, decimal marker, which family's
/// defaults the styles came from.
fn standard_pane(
    ui: &mut egui::Ui,
    standard: &mut crate::drawing::DrawingStandard,
    st: &mut DrawingStyles,
) {
    ui.label(RichText::new("Standard").heading());
    egui::Grid::new("std_grid")
        .num_columns(2)
        .spacing([14.0, 6.0])
        .show(ui, |ui| {
            ui.label("Family");
            ui.label(standard.kind.label());
            ui.end_row();
            ui.label("Units");
            ui.label("mm");
            ui.end_row();
            ui.label("Decimal Marker");
            let mut comma = st.dimension.iter().all(|d| d.decimal_comma);
            if ui.checkbox(&mut comma, "comma (,)").changed() {
                for d in &mut st.dimension {
                    d.decimal_comma = comma;
                }
            }
            ui.end_row();
            ui.label("Projection type");
            ui.horizontal(|ui| {
                ui.radio_value(
                    &mut standard.projection,
                    Projection::FirstAngle,
                    "First Angle",
                );
                ui.radio_value(
                    &mut standard.projection,
                    Projection::ThirdAngle,
                    "Third Angle",
                );
            });
            ui.end_row();
        });
    ui.separator();
    ui.label(RichText::new("Reset the styles to a family's defaults").strong());
    ui.horizontal(|ui| {
        for k in StandardKind::ALL {
            if ui
                .button(k.label())
                .on_hover_text(
                    "Replaces every style with this family's defaults (Undo takes it back)",
                )
                .clicked()
            {
                *st = DrawingStyles::of(k);
                *standard = crate::drawing::DrawingStandard {
                    kind: k,
                    projection: crate::drawing::DrawingStandard::of(k).projection,
                };
            }
        }
    });
    ui.separator();
    for (name, why) in [
        (
            "Preset Values",
            "line weights, text heights, scales, hatch angles lists",
        ),
        (
            "Character Exclude",
            "the excluded letters are fixed: I O Q S X Z",
        ),
        ("Global Line Scale", "scales every line type"),
        (
            "View Preferences: view labels, cut edges, thread display, front view plane",
            "fixed labels for now",
        ),
        ("Available Styles", "every style is available"),
        (
            "Material Hatch Pattern Defaults",
            "one hatch style for every material",
        ),
    ] {
        planned(ui, name, &format!("Planned: {why} (spec 4.6 §3.2)"));
    }
}

/// The selected style's settings.
fn style_pane(
    ui: &mut egui::Ui,
    t: StyleType,
    name: &str,
    st: &mut DrawingStyles,
    ed: &mut StylesEditor,
) {
    ui.label(RichText::new(format!("{} — {name}", t.label())).heading());
    let text_names: Vec<String> = st.text.iter().map(|s| s.name.clone()).collect();
    let leader_names: Vec<String> = st.leader.iter().map(|s| s.name.clone()).collect();
    match t {
        StyleType::Dimension => {
            let Some(d) = st.dimension.iter_mut().find(|s| s.name == name) else {
                return;
            };
            ui.horizontal(|ui| {
                for (tab, label) in [
                    (DimTab::Units, "Units"),
                    (DimTab::AlternateUnits, "Alternate Units"),
                    (DimTab::Display, "Display"),
                    (DimTab::Text, "Text"),
                    (DimTab::Tolerance, "Tolerance"),
                    (DimTab::Options, "Options"),
                    (DimTab::NotesLeaders, "Notes and Leaders"),
                ] {
                    ui.selectable_value(&mut ed.tab, tab, label);
                }
            });
            ui.separator();
            match ed.tab {
                DimTab::Units => {
                    egui::Grid::new("dim_units")
                        .num_columns(2)
                        .spacing([14.0, 6.0])
                        .show(ui, |ui| {
                            ui.label("Units");
                            ui.label("mm");
                            ui.end_row();
                            ui.label("Decimal Marker");
                            ui.horizontal(|ui| {
                                ui.radio_value(&mut d.decimal_comma, false, "Period (.)");
                                ui.radio_value(&mut d.decimal_comma, true, "Comma (,)");
                            });
                            ui.end_row();
                            ui.label("Linear Format");
                            ui.label("Decimal");
                            ui.end_row();
                            ui.label("Precision");
                            let mut p = usize::from(d.precision);
                            ui.add(ExprValue::new(&mut p).range(0..=6));
                            d.precision = p.min(6) as u8;
                            ui.end_row();
                            ui.label("Trailing Zeros");
                            ui.checkbox(&mut d.trailing_zeros, "keep up to the precision");
                            ui.end_row();
                            ui.label("Angular Precision");
                            ui.add_enabled(
                                false,
                                ExprValue::new(&mut usize::from(d.angular_precision)).range(0..=6),
                            )
                            .on_disabled_hover_text("With angular dimensions (spec 4.6 §3.5)");
                            ui.end_row();
                        });
                    planned(
                        ui,
                        "Unit String, Included Zeros, Angular Format",
                        "Planned (spec 4.6 §3.5)",
                    );
                }
                DimTab::AlternateUnits => {
                    planned(
                        ui,
                        "Show Alternate Units (units, format, precision, dual format)",
                        "Planned (spec 4.6 §3.5)",
                    );
                }
                DimTab::Display => {
                    egui::Grid::new("dim_display")
                        .num_columns(2)
                        .spacing([14.0, 6.0])
                        .show(ui, |ui| {
                            ui.label("Terminator");
                            egui::ComboBox::from_id_salt("dim_term")
                                .selected_text(d.terminator.label())
                                .show_ui(ui, |ui| {
                                    for k in Terminator::ALL {
                                        ui.selectable_value(&mut d.terminator, k, k.label());
                                    }
                                });
                            ui.end_row();
                            mm(ui, "Size (X)", &mut d.arrow_size, 0.1..=20.0);
                            mm(ui, "Height (Y)", &mut d.arrow_height, 0.1..=20.0);
                            mm(ui, "Extension (A)", &mut d.extension, 0.0..=20.0);
                            mm(ui, "Origin Offset (B)", &mut d.origin_offset, 0.0..=20.0);
                            mm(ui, "Gap (C)", &mut d.gap, 0.0..=20.0);
                            ui.label("Spacing (D)");
                            ui.add_enabled(false, ExprValue::new(&mut d.spacing).suffix(" mm"))
                                .on_disabled_hover_text(
                                    "Used by baseline / chain dimensions (planned)",
                                );
                            ui.end_row();
                            ui.label("Part Offset (E)");
                            ui.add_enabled(false, ExprValue::new(&mut d.part_offset).suffix(" mm"))
                                .on_disabled_hover_text("Placement aid (planned)");
                            ui.end_row();
                        });
                    planned(
                        ui,
                        "Line Type / Weight / Color: from the Dimension layer (Layers)",
                        "Per-style line overrides are planned",
                    );
                    planned(
                        ui,
                        "Dimension Line Visibility, Break Symbol",
                        "Planned (spec 4.6 §3.5)",
                    );
                }
                DimTab::Text => {
                    egui::Grid::new("dim_text")
                        .num_columns(2)
                        .spacing([14.0, 6.0])
                        .show(ui, |ui| {
                            ui.label("Primary Text Style");
                            combo_names(ui, "dim_text_style", &mut d.text_style, &text_names);
                            ui.end_row();
                            ui.label("Linear Orientation");
                            ui.horizontal(|ui| {
                                ui.radio_value(
                                    &mut d.orientation,
                                    TextOrientation::Aligned,
                                    "Aligned",
                                );
                                ui.radio_value(
                                    &mut d.orientation,
                                    TextOrientation::Horizontal,
                                    "Horizontal",
                                );
                            });
                            ui.end_row();
                            ui.label("Prefix");
                            ui.text_edit_singleline(&mut d.prefix);
                            ui.end_row();
                            ui.label("Suffix");
                            ui.text_edit_singleline(&mut d.suffix);
                            ui.end_row();
                        });
                    planned(
                        ui,
                        "Tolerance text style, diameter / radial / angle orientation, text position",
                        "Planned (spec 4.6 §3.5)",
                    );
                }
                DimTab::Tolerance => {
                    planned(
                        ui,
                        "Method, Upper, Lower, Zero Tolerance, Basic box, precisions",
                        "With dimension tolerances (4.3)",
                    );
                }
                DimTab::Options => {
                    ui.checkbox(&mut d.diameter_symbol, "Diameter symbol (Ø) on circles");
                    planned(
                        ui,
                        "Arrowhead placement, hide extension lines, radius / diameter leaders, arc and ordinate options",
                        "Planned (spec 4.6 §3.5)",
                    );
                }
                DimTab::NotesLeaders => {
                    egui::Grid::new("dim_notes")
                        .num_columns(2)
                        .spacing([14.0, 6.0])
                        .show(ui, |ui| {
                            ui.label("Leader Style");
                            combo_names(ui, "dim_leader", &mut d.leader_style, &leader_names);
                            ui.end_row();
                        });
                    planned(
                        ui,
                        "Hole / thread / chamfer / bend / punch note formats",
                        "Notes use fixed formats for now (Ø, THRU, CBORE, CSK)",
                    );
                }
            }
        }
        StyleType::Text => {
            let Some(s) = st.text.iter_mut().find(|s| s.name == name) else {
                return;
            };
            egui::Grid::new("text_style")
                .num_columns(2)
                .spacing([14.0, 6.0])
                .show(ui, |ui| {
                    ui.label("Font");
                    ui.add_enabled(false, egui::Label::new(&s.font))
                        .on_disabled_hover_text(
                            "One font until the drawing font is chosen (owner's decision)",
                        );
                    ui.end_row();
                    mm(ui, "Text Height", &mut s.height, 0.5..=50.0);
                    ui.label("Width Scale");
                    ui.add(ExprValue::new(&mut s.width_scale).range(0.2..=5.0));
                    ui.end_row();
                    ui.label("Line Spacing");
                    ui.add(ExprValue::new(&mut s.line_spacing).range(0.8..=5.0));
                    ui.end_row();
                    ui.label("Color");
                    color_edit(ui, &mut s.color);
                    ui.end_row();
                    ui.label("Bold / Italic");
                    ui.horizontal(|ui| {
                        ui.add_enabled(false, egui::Checkbox::new(&mut s.bold, "Bold"));
                        ui.add_enabled(false, egui::Checkbox::new(&mut s.italic, "Italic"));
                    });
                    ui.end_row();
                });
            planned(
                ui,
                "Justification, rotation, colour on paper",
                "Planned (spec 4.6 §3.6)",
            );
        }
        StyleType::Leader => {
            let Some(s) = st.leader.iter_mut().find(|s| s.name == name) else {
                return;
            };
            egui::Grid::new("leader_style")
                .num_columns(2)
                .spacing([14.0, 6.0])
                .show(ui, |ui| {
                    ui.label("Terminator");
                    egui::ComboBox::from_id_salt("leader_term")
                        .selected_text(s.terminator.label())
                        .show_ui(ui, |ui| {
                            for k in Terminator::ALL {
                                ui.selectable_value(&mut s.terminator, k, k.label());
                            }
                        });
                    ui.end_row();
                    mm(ui, "Size (X)", &mut s.arrow_size, 0.1..=20.0);
                    mm(ui, "Height (Y)", &mut s.arrow_height, 0.1..=20.0);
                    ui.label("Text Style");
                    combo_names(ui, "leader_text", &mut s.text_style, &text_names);
                    ui.end_row();
                });
            planned(
                ui,
                "Extension line offset / overshoot, All-Around symbol, attached text position",
                "Planned (spec 4.6 §3.7)",
            );
        }
        StyleType::CenterMark => {
            let Some(s) = st.center_mark.iter_mut().find(|s| s.name == name) else {
                return;
            };
            egui::Grid::new("cm_style")
                .num_columns(2)
                .spacing([14.0, 6.0])
                .show(ui, |ui| {
                    mm(ui, "Mark", &mut s.mark, 0.0..=30.0);
                    mm(ui, "Gap", &mut s.gap, 0.0..=30.0);
                    mm(ui, "Overshoot", &mut s.overshoot, 0.0..=30.0);
                });
            planned(
                ui,
                "Extension (minimum), Default Radius",
                "Planned (spec 4.6 §3.8)",
            );
        }
        StyleType::Hatch => {
            let Some(s) = st.hatch.iter_mut().find(|s| s.name == name) else {
                return;
            };
            egui::Grid::new("hatch_style")
                .num_columns(2)
                .spacing([14.0, 6.0])
                .show(ui, |ui| {
                    ui.label("Pattern");
                    ui.add_enabled(false, egui::Label::new("Lines (ANSI 31)"))
                        .on_disabled_hover_text("Named patterns are planned (spec 4.6 §3.9)");
                    ui.end_row();
                    ui.label("Angle");
                    ui.add(
                        ExprValue::new(&mut s.angle)
                            .range(-180.0..=180.0)
                            .suffix("°"),
                    );
                    ui.end_row();
                    mm(ui, "Spacing", &mut s.spacing, 0.3..=50.0);
                    ui.label("Double");
                    ui.checkbox(&mut s.double, "cross-hatched");
                    ui.end_row();
                });
            planned(
                ui,
                "Shift, line weight / colour override",
                "From the Hatch layer for now",
            );
        }
        StyleType::ViewAnnotation => {
            let Some(s) = st.view_annotation.iter_mut().find(|s| s.name == name) else {
                return;
            };
            egui::Grid::new("va_style")
                .num_columns(2)
                .spacing([14.0, 6.0])
                .show(ui, |ui| {
                    mm(ui, "View label height", &mut s.label_height, 1.0..=20.0);
                    mm(
                        ui,
                        "Identifier letters",
                        &mut s.identifier_height,
                        1.0..=30.0,
                    );
                });
            planned(
                ui,
                "Label text style, section line / arrow settings",
                "Planned (spec 4.6 §3.10)",
            );
        }
        StyleType::Layers => {
            egui::Grid::new("layers")
                .num_columns(7)
                .striped(true)
                .spacing([10.0, 4.0])
                .show(ui, |ui| {
                    for h in [
                        "Layer Name",
                        "On",
                        "Color",
                        "Line Type",
                        "Line Weight",
                        "Plot",
                        "",
                    ] {
                        ui.label(RichText::new(h).strong());
                    }
                    ui.end_row();
                    for (k, l) in st.layers.iter_mut().enumerate() {
                        let sel = l.name == name;
                        ui.label(if sel {
                            RichText::new(&l.name).strong()
                        } else {
                            RichText::new(&l.name)
                        });
                        ui.checkbox(&mut l.on, "");
                        color_edit(ui, &mut l.color);
                        egui::ComboBox::from_id_salt(("layer_lt", k))
                            .selected_text(l.line_type.label())
                            .width(100.0)
                            .show_ui(ui, |ui| {
                                for t in LineType::ALL {
                                    ui.selectable_value(&mut l.line_type, t, t.label());
                                }
                            });
                        ui.add(
                            ExprValue::new(&mut l.weight)
                                .range(0.05..=3.0)
                                .suffix(" mm"),
                        );
                        ui.add_enabled(false, egui::Checkbox::new(&mut l.plot, ""))
                            .on_disabled_hover_text("Plot: planned (every layer prints)");
                        ui.label("");
                        ui.end_row();
                    }
                });
            planned(
                ui,
                "Line types of layers draw from the object (hidden dashed, centre chain) for now; colours on screen and paper are planned",
                "spec 4.6 §3.4",
            );
        }
        StyleType::ObjectDefaults => {
            let names = |t: StyleType| st.names(t);
            let (dims, cms, hatches, texts, leaders, vas, layers) = (
                names(StyleType::Dimension),
                names(StyleType::CenterMark),
                names(StyleType::Hatch),
                names(StyleType::Text),
                names(StyleType::Leader),
                names(StyleType::ViewAnnotation),
                names(StyleType::Layers),
            );
            let d = &mut st.defaults;
            egui::Grid::new("obj_defaults")
                .num_columns(2)
                .striped(true)
                .spacing([14.0, 6.0])
                .show(ui, |ui| {
                    ui.label(RichText::new("Object Type").strong());
                    ui.label(RichText::new("Object Style / Layer").strong());
                    ui.end_row();
                    ui.label("Dimensions");
                    combo_names(ui, "od_dim", &mut d.dimension, &dims);
                    ui.end_row();
                    ui.label("Center Mark, Centerline");
                    combo_names(ui, "od_cm", &mut d.center_mark, &cms);
                    ui.end_row();
                    ui.label("Hatch");
                    combo_names(ui, "od_hatch", &mut d.hatch, &hatches);
                    ui.end_row();
                    ui.label("General Note (Text)");
                    combo_names(ui, "od_text", &mut d.text, &texts);
                    ui.end_row();
                    ui.label("Leader Text, Hole Note");
                    combo_names(ui, "od_leader", &mut d.leader, &leaders);
                    ui.end_row();
                    ui.label("View Label, Definitions");
                    combo_names(ui, "od_va", &mut d.view_annotation, &vas);
                    ui.end_row();
                    for (label, layer, id) in [
                        ("Visible edges — layer", &mut d.visible_layer, "od_l_vis"),
                        ("Hidden edges — layer", &mut d.hidden_layer, "od_l_hid"),
                        ("Centre lines — layer", &mut d.center_layer, "od_l_cen"),
                        (
                            "Dimensions, notes — layer",
                            &mut d.dimension_layer,
                            "od_l_dim",
                        ),
                        ("Hatch — layer", &mut d.hatch_layer, "od_l_hat"),
                        (
                            "Border, title block — layer",
                            &mut d.border_layer,
                            "od_l_bor",
                        ),
                    ] {
                        ui.label(label);
                        combo_names(ui, id, layer, &layers);
                        ui.end_row();
                    }
                });
            planned(
                ui,
                "Filter (Dimension / Leader / Model-View / Text objects)",
                "Planned (spec 4.6 §3.3)",
            );
        }
    }
}
