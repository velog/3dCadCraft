//! Drawing resources in the GUI (spec 4.13): the sheet's border and title
//! block rows in the browser and Edit Drawing Border Parameters.

use eframe::egui;

use super::CadCraftApp;
use super::icons::Icon;
use super::valuefield::ExprValue;
use crate::drawing::resources::{Delimit, LabelFrom, ZoneLabel, ZoneParams};

/// What a border / title block row of the browser asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FormatAction {
    /// Edit Instance of the border (its parameters on this sheet).
    EditBorder,
    /// Delete the border from the sheet.
    DeleteBorder,
    /// Delete the title block from the sheet.
    DeleteTitle,
}

/// Edit Drawing Border Parameters of one sheet.
#[derive(Clone, Debug)]
pub(super) struct BorderDialog {
    /// Sheet index.
    pub sheet: usize,
    pub params: ZoneParams,
    /// Margins top, bottom, left, right.
    margins: [f64; 4],
}

/// The sheet's border and title block under it in the browser.
pub(super) fn sheet_format_rows(
    ui: &mut egui::Ui,
    [border, title]: [bool; 2],
    title_name: &str,
) -> Option<FormatAction> {
    let mut action = None;
    if border {
        ui.horizontal(|ui| {
            super::panels::tree_icon(ui, Icon::Drawing);
            let r = ui.selectable_label(false, "Default Border");
            if r.double_clicked() {
                action = Some(FormatAction::EditBorder);
            }
            r.context_menu(|ui| {
                if ui.button("Edit Instance...").clicked() {
                    action = Some(FormatAction::EditBorder);
                    ui.close();
                }
                ui.add_enabled(false, egui::Button::new("Edit Field Text"))
                    .on_disabled_hover_text("Planned: spec 4.13 §3.5 (prompted entries)");
                if ui.button("Delete").clicked() {
                    action = Some(FormatAction::DeleteBorder);
                    ui.close();
                }
            });
        });
    }
    if title {
        ui.horizontal(|ui| {
            super::panels::tree_icon(ui, Icon::Drawing);
            ui.selectable_label(false, title_name).context_menu(|ui| {
                ui.add_enabled(false, egui::Button::new("Edit Definition"))
                    .on_disabled_hover_text("Planned: spec 4.13 §3.2");
                ui.add_enabled(false, egui::Button::new("Edit Field Text"))
                    .on_disabled_hover_text("Planned: spec 4.13 §3.5 (prompted entries)");
                if ui.button("Delete").clicked() {
                    action = Some(FormatAction::DeleteTitle);
                    ui.close();
                }
            });
        });
    }
    action
}

impl CadCraftApp {
    /// Runs a border / title block row's action on sheet `i`.
    pub(super) fn format_action(&mut self, i: usize, a: FormatAction) {
        match a {
            FormatAction::EditBorder => {
                let Some(s) = self.drawing().and_then(|d| d.sheets.get(i)) else {
                    return;
                };
                let params = crate::drawing::sheet::border_params(s);
                let margins = crate::drawing::sheet::margins(s);
                self.drw.border_dialog = Some(BorderDialog {
                    sheet: i,
                    margins,
                    params,
                });
            }
            FormatAction::DeleteBorder => {
                self.edit_drawing("Delete Border", |d| {
                    if let Some(s) = d.sheets.get_mut(i) {
                        s.border = false;
                    }
                });
            }
            FormatAction::DeleteTitle => {
                self.edit_drawing("Delete Title Block", |d| {
                    if let Some(s) = d.sheets.get_mut(i) {
                        s.title_block = false;
                    }
                });
            }
        }
    }

    /// Edit Drawing Border Parameters (spec 4.13 §3.3, Edit Instance).
    pub(super) fn border_dialog_window(&mut self, ctx: &egui::Context) {
        let Some(mut dlg) = self.drw.border_dialog.clone() else {
            return;
        };
        let standard = self
            .drawing()
            .and_then(|d| d.sheets.get(dlg.sheet))
            .map(crate::drawing::sheet::margin);
        let Some(standard) = standard else {
            self.drw.border_dialog = None;
            return;
        };
        let mut open = true;
        let (mut ok, mut cancel) = (false, false);
        let win = egui::Window::new("Edit Drawing Border Parameters")
            .open(&mut open)
            .resizable(false)
            .collapsible(false);
        self.layout
            .place("border_params", ctx.content_rect(), false, win)
            .show(ctx, |ui| {
                border_form(ui, &mut dlg, standard);
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.add(super::theme::ok_button("OK")).clicked()
                        || super::keys::panel_ok_key(ui, false)
                    {
                        ok = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                    if ui.button("Defaults").clicked() {
                        dlg.params = ZoneParams::default();
                        dlg.margins = [standard; 4];
                    }
                });
            });
        if cancel || !open {
            self.drw.border_dialog = None;
            return;
        }
        if ok {
            let mut p = dlg.params.clone();
            p.margins = (dlg.margins != [standard; 4]).then_some(dlg.margins);
            let p = p.clamped();
            let own = (p != ZoneParams::default()).then_some(p);
            let i = dlg.sheet;
            self.edit_drawing("Edit Border", |d| {
                if let Some(s) = d.sheets.get_mut(i) {
                    s.border_params = own;
                }
            });
            self.drw.border_dialog = None;
            return;
        }
        self.drw.border_dialog = Some(dlg);
    }
}

/// The rows of the border parameters (Inventor's Default Drawing Border
/// Parameters).
fn border_form(ui: &mut egui::Ui, dlg: &mut BorderDialog, standard: f64) {
    let p = &mut dlg.params;
    ui.horizontal(|ui| {
        ui.radio_value(&mut p.zones, true, "Zone Border");
        ui.radio_value(&mut p.zones, false, "Border");
    });
    ui.add_enabled_ui(p.zones, |ui| {
        egui::Grid::new("border_zones")
            .num_columns(2)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                ui.strong("Horizontal Zones");
                ui.end_row();
                ui.label("Number of Zones");
                ui.add(ExprValue::new(&mut p.horizontal).range(1..=100));
                ui.end_row();
                ui.label("Label");
                label_combo(ui, "h_label", &mut p.h_label);
                ui.end_row();
                ui.strong("Vertical Zones");
                ui.end_row();
                ui.label("Number of Zones");
                ui.add(ExprValue::new(&mut p.vertical).range(1..=100));
                ui.end_row();
                ui.label("Label");
                label_combo(ui, "v_label", &mut p.v_label);
                ui.end_row();
                ui.label("Label Zones From");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut p.from, LabelFrom::BottomRight, "Bottom/Right");
                    ui.radio_value(&mut p.from, LabelFrom::TopLeft, "Top/Left");
                });
                ui.end_row();
                ui.label("Delimit Zones By");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut p.delimit, Delimit::Line, "Line");
                    ui.radio_value(&mut p.delimit, Delimit::Arrowhead, "Arrowhead");
                });
                ui.end_row();
            });
    });
    ui.checkbox(&mut p.center_marks, "Center Marks");
    ui.add_enabled(false, egui::Label::new("Text Style: Label Text (ISO)"))
        .on_disabled_hover_text("Planned: spec 4.13 §3.3 (text style and layers)");
    ui.add_space(4.0);
    ui.strong("Sheet Margins");
    egui::Grid::new("border_margins")
        .num_columns(4)
        .spacing([12.0, 6.0])
        .show(ui, |ui| {
            for (k, name) in ["Top", "Bottom", "Left", "Right"].iter().enumerate() {
                ui.label(*name);
                ui.add(
                    ExprValue::new(&mut dlg.margins[k])
                        .range(0.0..=200.0)
                        .suffix(" mm"),
                );
                if k % 2 == 1 {
                    ui.end_row();
                }
            }
        });
    ui.weak(format!("Standard margin: {standard} mm"));
}

fn label_combo(ui: &mut egui::Ui, id: &str, v: &mut ZoneLabel) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(v.label())
        .show_ui(ui, |ui| {
            for l in ZoneLabel::ALL {
                ui.selectable_value(v, l, l.label());
            }
        });
}
