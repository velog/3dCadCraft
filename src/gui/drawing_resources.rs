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
    /// Edit Definition of the sheet's own title block.
    EditTitle,
    /// Edit Field Text: its prompted entries' values on this sheet.
    EditFields,
}

/// What a Drawing Resources row asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum TitleAction {
    /// Define New Title Block.
    Define,
    /// Insert title block `Some(name)` (`None`: the standard's) on the
    /// active sheet.
    Insert(Option<String>),
    /// Edit title block `name`.
    Edit(String),
    /// Delete title block `name`.
    Delete(String),
}

/// A sheet's border and title block rows in the browser.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct SheetFormat {
    pub border: bool,
    /// The title block's name, whether it is the user's, whether it has
    /// prompted entries.
    pub title: Option<(String, bool, bool)>,
}

impl SheetFormat {
    /// The rows of `sheet` of `d`.
    pub fn of(d: &crate::drawing::DrawingDocument, sheet: &crate::drawing::Sheet) -> Self {
        let title =
            sheet
                .title_block
                .then(|| match crate::drawing::resources::title_of(d, sheet) {
                    Some(def) => (def.name.clone(), true, !def.prompts().is_empty()),
                    None => (d.standard.kind.label().to_string(), false, false),
                });
        Self {
            border: sheet.border,
            title,
        }
    }
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

/// The name asked when a new title block is saved.
#[derive(Clone, Debug)]
pub(super) struct TitleSave {
    pub name: String,
    pub def: crate::drawing::resources::Definition,
    focused: bool,
}

/// Edit Property Fields: the prompted entries of the title block on a sheet.
#[derive(Clone, Debug)]
pub(super) struct PromptDialog {
    /// Sheet index.
    pub sheet: usize,
    /// The title block.
    pub name: String,
    /// Entry title and value, in the definition's order.
    pub values: Vec<(String, String)>,
}

/// The sheet's border and title block under it in the browser.
pub(super) fn sheet_format_rows(ui: &mut egui::Ui, f: &SheetFormat) -> Option<FormatAction> {
    let mut action = None;
    if f.border {
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
                    .on_disabled_hover_text("The default border has no prompted entries");
                if ui.button("Delete").clicked() {
                    action = Some(FormatAction::DeleteBorder);
                    ui.close();
                }
            });
        });
    }
    if let Some((name, user, prompts)) = &f.title {
        ui.horizontal(|ui| {
            super::panels::tree_icon(ui, Icon::Drawing);
            let r = ui.selectable_label(false, name.as_str());
            if r.double_clicked() && *prompts {
                action = Some(FormatAction::EditFields);
            }
            r.context_menu(|ui| {
                if ui
                    .add_enabled(*user, egui::Button::new("Edit Definition..."))
                    .on_disabled_hover_text("The standard's title block: define your own (DTB)")
                    .clicked()
                {
                    action = Some(FormatAction::EditTitle);
                    ui.close();
                }
                if ui
                    .add_enabled(*prompts, egui::Button::new("Edit Field Text..."))
                    .on_disabled_hover_text("This title block has no prompted entries")
                    .clicked()
                {
                    action = Some(FormatAction::EditFields);
                    ui.close();
                }
                if ui.button("Delete").clicked() {
                    action = Some(FormatAction::DeleteTitle);
                    ui.close();
                }
            });
        });
    }
    action
}

/// One title block row of Drawing Resources (`user`: the user's block of
/// that name; `None`: the standard's).
fn title_row(
    ui: &mut egui::Ui,
    label: &str,
    user: Option<&String>,
    action: &mut Option<TitleAction>,
) {
    ui.horizontal(|ui| {
        super::panels::tree_icon(ui, Icon::Drawing);
        let tip = if user.is_some() {
            "Double-click: put it on the active sheet"
        } else {
            "Built in: the standard's title block. Double-click: put it on the active sheet"
        };
        let r = ui.selectable_label(false, label).on_hover_text(tip);
        if r.double_clicked() {
            *action = Some(TitleAction::Insert(user.cloned()));
        }
        r.context_menu(|ui| {
            if ui.button("Insert").clicked() {
                *action = Some(TitleAction::Insert(user.cloned()));
                ui.close();
            }
            match user {
                Some(n) => {
                    if ui.button("Edit...").clicked() {
                        *action = Some(TitleAction::Edit(n.clone()));
                        ui.close();
                    }
                    if ui.button("Delete").clicked() {
                        *action = Some(TitleAction::Delete(n.clone()));
                        ui.close();
                    }
                }
                None => {
                    ui.add_enabled(false, egui::Button::new("Edit..."))
                        .on_disabled_hover_text("Built in: define your own title block (DTB)");
                }
            }
            for item in ["Copy", "Rename"] {
                ui.add_enabled(false, egui::Button::new(item))
                    .on_disabled_hover_text("Planned: spec 4.13 §3.1");
            }
        });
    });
}

/// Drawing Resources in the browser: the folders, the built-in border and
/// title block, the user's title blocks (`blocks`).
pub(super) fn resources_tree(
    ui: &mut egui::Ui,
    title_name: &str,
    blocks: &[String],
) -> Option<TitleAction> {
    let mut action = None;
    let off = |ui: &mut egui::Ui, label: &str, row: &str| {
        ui.add_enabled(false, egui::Button::new(label))
            .on_disabled_hover_text(format!("Planned: spec 4.13 {row}"));
    };
    let header = |ui: &mut egui::Ui, name: &str, body: &mut dyn FnMut(&mut egui::Ui)| {
        egui::CollapsingHeader::new(name)
            .id_salt(("drw_res", name))
            .default_open(true)
            .show(ui, |ui| body(ui))
            .header_response
    };
    header(ui, "Sheet Formats", &mut |ui| {
        ui.weak("(none)");
    })
    .context_menu(|ui| off(ui, "Create New Sheet Format", "§3.1 (templates: spec 4)"));
    header(ui, "Borders", &mut |ui| {
        ui.horizontal(|ui| {
            super::panels::tree_icon(ui, Icon::Drawing);
            ui.selectable_label(false, "Default Border")
                .on_hover_text(
                    "Built in: zones, margins and centre marks; each sheet's Edit Instance sets them",
                )
                .context_menu(|ui| off(ui, "Edit", "§3.3"));
        });
    })
    .context_menu(|ui| {
        off(ui, "Define New Border", "§3.3");
        off(ui, "Define New Zone Border", "§3.3");
    });
    header(ui, "Title Blocks", &mut |ui| {
        title_row(ui, title_name, None, &mut action);
        for b in blocks {
            title_row(ui, b, Some(b), &mut action);
        }
    })
    .context_menu(|ui| {
        if ui.button("Define New Title Block").clicked() {
            action = Some(TitleAction::Define);
            ui.close();
        }
        off(ui, "Sort by Name", "§3.1");
    });
    header(ui, "Sketched Symbols", &mut |ui| {
        ui.weak("(none)");
    })
    .context_menu(|ui| off(ui, "Define New Symbol", "§3.4"));
    action
}

/// The Properties a sheet's fields read (spec 4.13 §3.2): the drawing's,
/// and those and the material of the model of the sheet's first view.
#[derive(Clone, Debug, Default)]
pub(super) struct FieldProps {
    drawing: Option<crate::document::iprops::IProperties>,
    model: Option<crate::document::iprops::IProperties>,
    /// The model document's name (its Part Number when none is set, as
    /// Inventor fills it from the file name).
    model_name: String,
    material: Option<String>,
}

impl FieldProps {
    /// The value of `f` (`None`: no such value, drawn as `#?`).
    pub fn get(&self, f: &crate::drawing::fields::Field) -> Option<String> {
        use crate::drawing::fields::{Field, property};
        match f {
            Field::Drawing(k) => property(self.drawing.as_ref()?, k),
            Field::Model(k) => {
                let v = property(self.model.as_ref()?, k)?;
                Some(if v.is_empty() && k == "part_number" {
                    self.model_name.clone()
                } else {
                    v
                })
            }
            Field::Material => self.material.clone(),
            Field::Sheet(_) => None,
        }
    }
}

impl CadCraftApp {
    /// The Properties for the fields of sheet `i` of `d`.
    pub(super) fn field_props(&self, d: &crate::drawing::DrawingDocument, i: usize) -> FieldProps {
        let model = d
            .sheets
            .get(i)
            .and_then(|s| s.views.first().and_then(|v| d.model_of(s, v.id)));
        let drawing = self
            .project
            .document(self.part_id)
            .map(|x| x.properties.clone());
        let model_doc = model.and_then(|m| match m.library {
            None => self
                .project
                .document(m.document)
                .map(|x| (x.properties.clone(), x.name.clone())),
            Some(_) => None,
        });
        let (model_props, model_name) = match model_doc {
            Some((p, n)) => (Some(p), n),
            None => (None, String::new()),
        };
        let material = model
            .and_then(|m| self.with_part(m, |p| p.material.clone()))
            .flatten();
        FieldProps {
            drawing,
            model: model_props,
            model_name,
            material,
        }
    }

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
            FormatAction::EditTitle => {
                let name = self
                    .drawing()
                    .and_then(|d| d.sheets.get(i))
                    .and_then(|s| s.title_def.as_ref())
                    .map(|p| p.name.clone());
                if let Some(n) = name {
                    self.edit_title_definition(&n);
                }
            }
            FormatAction::EditFields => self.open_prompt_dialog(i),
        }
    }

    /// A Drawing Resources row's action.
    pub(super) fn res_action(&mut self, a: TitleAction) {
        let i = self.drawing().map_or(0, |d| d.active_sheet);
        match a {
            TitleAction::Define => self.define_title_block(),
            TitleAction::Insert(None) => {
                self.edit_drawing("Insert Title Block", |d| {
                    d.set_title_block(i, None, Vec::new())
                });
            }
            TitleAction::Insert(Some(n)) => {
                let prompts = self
                    .drawing()
                    .and_then(|d| d.resources.title_blocks.iter().find(|b| b.name == n))
                    .map(|b| b.prompts())
                    .unwrap_or_default();
                if prompts.is_empty() {
                    self.edit_drawing("Insert Title Block", |d| {
                        d.set_title_block(i, Some(&n), Vec::new())
                    });
                } else {
                    // The prompted entries are asked first (Edit Property
                    // Fields); OK puts the block on the sheet.
                    self.drw.prompt_dialog = Some(PromptDialog {
                        sheet: i,
                        name: n,
                        values: prompts.into_iter().map(|t| (t, String::new())).collect(),
                    });
                }
            }
            TitleAction::Edit(n) => self.edit_title_definition(&n),
            TitleAction::Delete(n) => {
                let r = self.edit_drawing("Delete Title Block", |d| d.delete_title_block(&n));
                if let Some(Err(e)) = r {
                    self.history.cancel();
                    self.file_status = Some(Err(e));
                }
            }
        }
    }

    /// Define Title Block (DTB): a sheet sketch whose Finish saves a title
    /// block definition.
    pub(super) fn define_title_block(&mut self) {
        self.open_view_sketch(
            super::drawing_sketch::SHEET,
            super::drawing_sketch::SketchFor::TitleBlock,
            Default::default(),
        );
        self.drw.sel = None;
        self.file_status = Some(Ok(
            "Define Title Block: sketch the block with lines and texts (fields: Insert Field); Finish Sketch saves it"
                .into(),
        ));
    }

    /// Edit of title block `name`: its sketch opens at the active sheet's
    /// frame corner; Finish Sketch changes every sheet using it.
    pub(super) fn edit_title_definition(&mut self, name: &str) {
        let Some((mut def, corner)) = self.drawing().and_then(|d| {
            let def = d
                .resources
                .title_blocks
                .iter()
                .find(|b| b.name == name)?
                .clone();
            let sheet = d.sheet()?;
            let (lo, hi) = crate::drawing::sheet::frame(sheet);
            Some((def, [hi[0], lo[1]]))
        }) else {
            return;
        };
        def.translate(corner);
        self.open_view_sketch(
            super::drawing_sketch::SHEET,
            super::drawing_sketch::SketchFor::TitleBlock,
            def.sketch,
        );
        if let Some(s) = self.drw.sketch.as_mut() {
            s.texts = def.texts;
            s.resource = Some(name.to_string());
        }
        self.drw.sel = None;
    }

    /// Finish Sketch of a title block definition: a new one asks for its
    /// name, an edited one changes every sheet using it.
    pub(super) fn finish_title_sketch(
        &mut self,
        sketch: crate::drawing::view_sketch::ViewSketch,
        texts: Vec<crate::drawing::view_sketch::DrawingText>,
        resource: Option<String>,
    ) {
        let def = crate::drawing::resources::Definition {
            name: resource.clone().unwrap_or_default(),
            sketch,
            texts,
        };
        if def.extent().is_none() {
            self.file_status = Some(Err(
                "Define Title Block: the title block is empty (nothing saved)".into(),
            ));
            return;
        }
        let def = def.normalised();
        match resource {
            Some(n) => {
                let (sk, tx) = (def.sketch, def.texts);
                if self.edit_drawing("Edit Title Block", |d| d.update_title_block(&n, sk, tx))
                    != Some(true)
                {
                    self.history.cancel();
                }
                self.history.break_coalescing();
            }
            None => {
                let name = self
                    .drawing()
                    .map(|d| d.next_title_block_name())
                    .unwrap_or_default();
                self.drw.title_save = Some(TitleSave {
                    name,
                    def,
                    focused: false,
                });
            }
        }
    }

    /// Edit Property Fields of the title block on sheet `i`.
    pub(super) fn open_prompt_dialog(&mut self, i: usize) {
        let Some((name, values)) = self.drawing().and_then(|d| {
            let s = d.sheets.get(i)?;
            let def = crate::drawing::resources::title_of(d, s)?;
            let placed = s.title_def.as_ref()?;
            let values = def
                .prompts()
                .into_iter()
                .map(|t| {
                    let v = placed.prompt(&t).unwrap_or_default();
                    (t, v)
                })
                .collect();
            Some((def.name.clone(), values))
        }) else {
            return;
        };
        self.drw.prompt_dialog = Some(PromptDialog {
            sheet: i,
            name,
            values,
        });
    }

    /// The Save Title Block name window and Edit Property Fields.
    pub(super) fn resource_windows(&mut self, ctx: &egui::Context) {
        if let Some(mut t) = self.drw.title_save.clone() {
            let mut open = true;
            let (mut ok, mut cancel) = (false, false);
            let win = egui::Window::new("Save Title Block")
                .open(&mut open)
                .resizable(false)
                .collapsible(false);
            self.layout
                .place("title_save", ctx.content_rect(), false, win)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Name");
                        let r = ui.text_edit_singleline(&mut t.name);
                        if !t.focused {
                            r.request_focus();
                            t.focused = true;
                        }
                    });
                    ui.weak(
                        "Double-click it in Drawing Resources › Title Blocks to put it on a sheet",
                    );
                    ui.separator();
                    ui.horizontal(|ui| {
                        let can = !t.name.trim().is_empty();
                        if ui
                            .add_enabled(can, super::theme::ok_button("Save"))
                            .clicked()
                            || (can && super::keys::panel_ok_key(ui, false))
                        {
                            ok = true;
                        }
                        if ui.button("Cancel").clicked() {
                            cancel = true;
                        }
                    });
                });
            if cancel || !open {
                self.drw.title_save = None;
                self.file_status = Some(Ok("Define Title Block: not saved".into()));
            } else if ok {
                let def = crate::drawing::resources::Definition {
                    name: t.name.trim().to_string(),
                    ..t.def.clone()
                };
                let name = def.name.clone();
                match self.edit_drawing("Define Title Block", |d| d.add_title_block(def)) {
                    Some(Err(e)) => {
                        self.history.cancel();
                        self.file_status = Some(Err(e));
                        self.drw.title_save = Some(t);
                    }
                    _ => {
                        self.drw.title_save = None;
                        self.file_status = Some(Ok(format!(
                            "Define Title Block: {name} saved in Drawing Resources"
                        )));
                    }
                }
            } else {
                self.drw.title_save = Some(t);
            }
        }
        if let Some(mut p) = self.drw.prompt_dialog.clone() {
            let mut open = true;
            let (mut ok, mut cancel) = (false, false);
            let win = egui::Window::new("Edit Property Fields")
                .open(&mut open)
                .resizable(false)
                .collapsible(false);
            self.layout
                .place("prompt_fields", ctx.content_rect(), false, win)
                .show(ctx, |ui| {
                    ui.label(format!("Prompted entries of {}", p.name));
                    egui::Grid::new("prompt_fields_grid")
                        .num_columns(2)
                        .striped(true)
                        .spacing([12.0, 6.0])
                        .show(ui, |ui| {
                            ui.strong("Property Field");
                            ui.strong("Value");
                            ui.end_row();
                            for (title, value) in &mut p.values {
                                ui.label(title.as_str());
                                ui.text_edit_singleline(value);
                                ui.end_row();
                            }
                        });
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
                    });
                });
            if cancel || !open {
                self.drw.prompt_dialog = None;
            } else if ok {
                let values: Vec<(String, String)> = p
                    .values
                    .iter()
                    .filter(|(_, v)| !v.is_empty())
                    .cloned()
                    .collect();
                let (i, name) = (p.sheet, p.name.clone());
                self.edit_drawing("Edit Field Text", |d| {
                    d.set_title_block(i, Some(&name), values)
                });
                self.drw.prompt_dialog = None;
            } else {
                self.drw.prompt_dialog = Some(p);
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
