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
    /// Edit Definition of the sheet's own border.
    EditBorderDef,
    /// Edit Field Text of the sheet's own border.
    EditBorderFields,
}

/// What a Drawing Resources row asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ResAction {
    /// Define New Title Block.
    Define,
    /// Insert title block `Some(name)` (`None`: the standard's) on the
    /// active sheet.
    Insert(Option<String>),
    /// Edit title block `name`.
    Edit(String),
    /// Delete title block `name`.
    Delete(String),
    /// Define New Border (`true`: Define New Zone Border).
    DefineBorder(bool),
    /// Insert border `Some(name)` (`None`: the default border) on the
    /// active sheet.
    InsertBorder(Option<String>),
    /// Edit border `name`.
    EditBorder(String),
    /// Delete border `name`.
    DeleteBorder(String),
}

/// One border / title block row under a sheet.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct FormatRow {
    pub name: String,
    /// The user's definition (not the built-in one).
    pub user: bool,
    /// It has prompted entries.
    pub prompts: bool,
    /// A border with zones (Edit Instance sets them).
    pub zones: bool,
}

/// A sheet's border and title block rows in the browser.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct SheetFormat {
    pub border: Option<FormatRow>,
    pub title: Option<FormatRow>,
}

impl SheetFormat {
    /// The rows of `sheet` of `d`.
    pub fn of(d: &crate::drawing::DrawingDocument, sheet: &crate::drawing::Sheet) -> Self {
        use crate::drawing::resources::{DEFAULT_BORDER, border_of, title_of};
        let title = sheet.title_block.then(|| match title_of(d, sheet) {
            Some(def) => FormatRow {
                name: def.name.clone(),
                user: true,
                prompts: !def.prompts().is_empty(),
                zones: false,
            },
            None => FormatRow {
                name: d.standard.kind.label().to_string(),
                user: false,
                prompts: false,
                zones: false,
            },
        });
        let border = sheet.border.then(|| match border_of(d, sheet) {
            Some(b) => FormatRow {
                name: b.def.name.clone(),
                user: true,
                prompts: !b.def.prompts().is_empty(),
                zones: b.zones.is_some(),
            },
            None => FormatRow {
                name: DEFAULT_BORDER.to_string(),
                user: false,
                prompts: false,
                zones: true,
            },
        });
        Self { border, title }
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
    /// Insert of this zone border: OK puts it on the sheet with these
    /// parameters (`None`: Edit Instance).
    pub insert: Option<String>,
}

/// The name asked when a new title block is saved.
#[derive(Clone, Debug)]
pub(super) struct TitleSave {
    pub name: String,
    pub def: crate::drawing::resources::Definition,
    /// A border (`Some(zone border)`), else a title block.
    pub border: Option<bool>,
    focused: bool,
}

/// Edit Property Fields: the prompted entries of the title block on a sheet.
#[derive(Clone, Debug)]
pub(super) struct PromptDialog {
    /// Sheet index.
    pub sheet: usize,
    /// The title block or border.
    pub name: String,
    /// The border's entries (else the title block's).
    pub border: bool,
    /// Entry title and value, in the definition's order.
    pub values: Vec<(String, String)>,
}

/// The sheet's border and title block under it in the browser.
pub(super) fn sheet_format_rows(ui: &mut egui::Ui, f: &SheetFormat) -> Option<FormatAction> {
    let mut action = None;
    if let Some(b) = &f.border {
        ui.horizontal(|ui| {
            super::panels::tree_icon(ui, Icon::Drawing);
            let r = ui.selectable_label(false, b.name.as_str());
            if r.double_clicked() {
                if b.zones {
                    action = Some(FormatAction::EditBorder);
                } else if b.prompts {
                    action = Some(FormatAction::EditBorderFields);
                }
            }
            r.context_menu(|ui| {
                if ui
                    .add_enabled(b.zones, egui::Button::new("Edit Instance..."))
                    .on_disabled_hover_text("A border without zones: Edit Definition...")
                    .clicked()
                {
                    action = Some(FormatAction::EditBorder);
                    ui.close();
                }
                if ui
                    .add_enabled(b.user, egui::Button::new("Edit Definition..."))
                    .on_disabled_hover_text("The default border: define your own (DBR)")
                    .clicked()
                {
                    action = Some(FormatAction::EditBorderDef);
                    ui.close();
                }
                if ui
                    .add_enabled(b.prompts, egui::Button::new("Edit Field Text..."))
                    .on_disabled_hover_text("This border has no prompted entries")
                    .clicked()
                {
                    action = Some(FormatAction::EditBorderFields);
                    ui.close();
                }
                if ui.button("Delete").clicked() {
                    action = Some(FormatAction::DeleteBorder);
                    ui.close();
                }
            });
        });
    }
    if let Some(FormatRow {
        name,
        user,
        prompts,
        ..
    }) = &f.title
    {
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
    action: &mut Option<ResAction>,
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
            *action = Some(ResAction::Insert(user.cloned()));
        }
        r.context_menu(|ui| {
            if ui.button("Insert").clicked() {
                *action = Some(ResAction::Insert(user.cloned()));
                ui.close();
            }
            match user {
                Some(n) => {
                    if ui.button("Edit...").clicked() {
                        *action = Some(ResAction::Edit(n.clone()));
                        ui.close();
                    }
                    if ui.button("Delete").clicked() {
                        *action = Some(ResAction::Delete(n.clone()));
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

/// One border row of Drawing Resources (`user`: the user's border of that
/// name; `None`: the default border).
fn border_row(
    ui: &mut egui::Ui,
    label: &str,
    user: Option<&String>,
    action: &mut Option<ResAction>,
) {
    ui.horizontal(|ui| {
        super::panels::tree_icon(ui, Icon::Drawing);
        let tip = if user.is_some() {
            "Double-click: put it on the active sheet"
        } else {
            "Built in: zones, margins and centre marks; each sheet's Edit Instance sets them. Double-click: put it on the active sheet"
        };
        let r = ui.selectable_label(false, label).on_hover_text(tip);
        if r.double_clicked() {
            *action = Some(ResAction::InsertBorder(user.cloned()));
        }
        r.context_menu(|ui| {
            if ui.button("Insert").clicked() {
                *action = Some(ResAction::InsertBorder(user.cloned()));
                ui.close();
            }
            match user {
                Some(n) => {
                    if ui.button("Edit...").clicked() {
                        *action = Some(ResAction::EditBorder(n.clone()));
                        ui.close();
                    }
                    if ui.button("Delete").clicked() {
                        *action = Some(ResAction::DeleteBorder(n.clone()));
                        ui.close();
                    }
                }
                None => {
                    ui.add_enabled(false, egui::Button::new("Edit..."))
                        .on_disabled_hover_text("Built in: define your own border (DBR)");
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
/// title block, the user's title blocks (`blocks`) and borders (`borders`).
pub(super) fn resources_tree(
    ui: &mut egui::Ui,
    title_name: &str,
    blocks: &[String],
    borders: &[String],
) -> Option<ResAction> {
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
        border_row(
            ui,
            crate::drawing::resources::DEFAULT_BORDER,
            None,
            &mut action,
        );
        for b in borders {
            border_row(ui, b, Some(b), &mut action);
        }
    })
    .context_menu(|ui| {
        if ui.button("Define New Border").clicked() {
            action = Some(ResAction::DefineBorder(false));
            ui.close();
        }
        if ui.button("Define New Zone Border").clicked() {
            action = Some(ResAction::DefineBorder(true));
            ui.close();
        }
    });
    header(ui, "Title Blocks", &mut |ui| {
        title_row(ui, title_name, None, &mut action);
        for b in blocks {
            title_row(ui, b, Some(b), &mut action);
        }
    })
    .context_menu(|ui| {
        if ui.button("Define New Title Block").clicked() {
            action = Some(ResAction::Define);
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
                    insert: None,
                });
            }
            FormatAction::EditBorderDef => {
                let name = self
                    .drawing()
                    .and_then(|d| d.sheets.get(i))
                    .and_then(|s| s.border_def.as_ref())
                    .map(|p| p.name.clone());
                if let Some(n) = name {
                    self.edit_border_definition(&n);
                }
            }
            FormatAction::EditBorderFields => self.open_prompt_dialog(i, true),
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
            FormatAction::EditFields => self.open_prompt_dialog(i, false),
        }
    }

    /// A Drawing Resources row's action.
    pub(super) fn res_action(&mut self, a: ResAction) {
        let i = self.drawing().map_or(0, |d| d.active_sheet);
        match a {
            ResAction::Define => self.define_title_block(),
            ResAction::Insert(None) => {
                self.edit_drawing("Insert Title Block", |d| {
                    d.set_title_block(i, None, Vec::new())
                });
            }
            ResAction::Insert(Some(n)) => {
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
                        border: false,
                        values: prompts.into_iter().map(|t| (t, String::new())).collect(),
                    });
                }
            }
            ResAction::Edit(n) => self.edit_title_definition(&n),
            ResAction::Delete(n) => {
                let r = self.edit_drawing("Delete Title Block", |d| d.delete_title_block(&n));
                if let Some(Err(e)) = r {
                    self.history.cancel();
                    self.file_status = Some(Err(e));
                }
            }
            ResAction::DefineBorder(zones) => self.define_border(zones),
            ResAction::InsertBorder(None) => {
                self.edit_drawing("Insert Border", |d| d.set_border(i, None, Vec::new(), None));
            }
            ResAction::InsertBorder(Some(n)) => {
                let Some((zones, prompts)) = self.drawing().and_then(|d| {
                    let b = d.resources.borders.iter().find(|b| b.def.name == n)?;
                    Some((b.zones.clone(), b.def.prompts()))
                }) else {
                    return;
                };
                if let Some(params) = zones {
                    // A zone border: Edit Drawing Border Parameters first
                    // (Inventor's Insert with parameters).
                    let margins = params.margins.unwrap_or_else(|| {
                        self.drawing()
                            .and_then(|d| d.sheets.get(i))
                            .map_or([10.0; 4], |s| [crate::drawing::sheet::margin(s); 4])
                    });
                    self.drw.border_dialog = Some(BorderDialog {
                        sheet: i,
                        params,
                        margins,
                        insert: Some(n),
                    });
                } else if prompts.is_empty() {
                    self.edit_drawing("Insert Border", |d| {
                        d.set_border(i, Some(&n), Vec::new(), None)
                    });
                } else {
                    self.drw.prompt_dialog = Some(PromptDialog {
                        sheet: i,
                        name: n,
                        border: true,
                        values: prompts.into_iter().map(|t| (t, String::new())).collect(),
                    });
                }
            }
            ResAction::EditBorder(n) => self.edit_border_definition(&n),
            ResAction::DeleteBorder(n) => {
                let r = self.edit_drawing("Delete Border", |d| d.delete_border(&n));
                if let Some(Err(e)) = r {
                    self.history.cancel();
                    self.file_status = Some(Err(e));
                }
            }
        }
    }

    /// Define Border (DBR) / Define New Zone Border: a sheet sketch whose
    /// Finish saves a border definition (sheet mm as drawn).
    pub(super) fn define_border(&mut self, zones: bool) {
        self.open_view_sketch(
            super::drawing_sketch::SHEET,
            super::drawing_sketch::SketchFor::Border,
            Default::default(),
        );
        if let Some(s) = self.drw.sketch.as_mut() {
            s.zones = zones;
        }
        self.drw.sel = None;
        self.file_status = Some(Ok(
            "Define Border: sketch the border with lines and texts; select points › right-click › Connection Point marks where the title block attaches; Finish Sketch saves it"
                .into(),
        ));
    }

    /// Edit of border `name`: its sketch opens where it was drawn; Finish
    /// Sketch changes every sheet using it.
    pub(super) fn edit_border_definition(&mut self, name: &str) {
        let Some(b) = self.drawing().and_then(|d| {
            d.resources
                .borders
                .iter()
                .find(|b| b.def.name == name)
                .cloned()
        }) else {
            return;
        };
        self.open_view_sketch(
            super::drawing_sketch::SHEET,
            super::drawing_sketch::SketchFor::Border,
            b.def.sketch,
        );
        if let Some(s) = self.drw.sketch.as_mut() {
            s.texts = b.def.texts;
            s.resource = Some(name.to_string());
            s.zones = b.zones.is_some();
            s.mark_connections(&b.def.connections);
        }
        self.drw.sel = None;
    }

    /// Finish Sketch of a border definition: a new one asks for its name,
    /// an edited one changes every sheet using it.
    pub(super) fn finish_border_sketch(
        &mut self,
        def: crate::drawing::resources::Definition,
        resource: Option<String>,
        zones: bool,
    ) {
        if def.extent().is_none() {
            self.file_status = Some(Err(
                "Define Border: the border is empty (nothing saved)".into()
            ));
            return;
        }
        match resource {
            Some(n) => {
                if self.edit_drawing("Edit Border", |d| d.update_border(&n, def)) != Some(true) {
                    self.history.cancel();
                }
                self.history.break_coalescing();
            }
            None => {
                let name = self
                    .drawing()
                    .map(|d| d.next_border_name())
                    .unwrap_or_default();
                self.drw.title_save = Some(TitleSave {
                    name,
                    def,
                    border: Some(zones),
                    focused: false,
                });
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
        let def = crate::drawing::resources::Definition::new(
            resource.clone().unwrap_or_default(),
            sketch,
            texts,
        );
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
                    border: None,
                    focused: false,
                });
            }
        }
    }

    /// Edit Property Fields of the title block (`border`: the border) on
    /// sheet `i`.
    pub(super) fn open_prompt_dialog(&mut self, i: usize, border: bool) {
        let Some((name, values)) = self.drawing().and_then(|d| {
            let s = d.sheets.get(i)?;
            let (def, placed) = if border {
                (
                    &crate::drawing::resources::border_of(d, s)?.def,
                    s.border_def.as_ref()?,
                )
            } else {
                (
                    crate::drawing::resources::title_of(d, s)?,
                    s.title_def.as_ref()?,
                )
            };
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
            border,
            values,
        });
    }

    /// The Save Title Block name window and Edit Property Fields.
    pub(super) fn resource_windows(&mut self, ctx: &egui::Context) {
        if let Some(mut t) = self.drw.title_save.clone() {
            let mut open = true;
            let (mut ok, mut cancel) = (false, false);
            let (title, what, folder) = match t.border {
                Some(_) => ("Save Border", "Define Border", "Borders"),
                None => ("Save Title Block", "Define Title Block", "Title Blocks"),
            };
            let win = egui::Window::new(title)
                .id(egui::Id::new("title_save"))
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
                    if let Some(z) = t.border.as_mut() {
                        ui.checkbox(z, "Zone Border").on_hover_text(
                            "The zones, margins and centre marks are drawn under the sketch; Insert asks their parameters",
                        );
                    }
                    ui.weak(format!(
                        "Double-click it in Drawing Resources › {folder} to put it on a sheet"
                    ));
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
                self.file_status = Some(Ok(format!("{what}: not saved")));
            } else if ok {
                let def = crate::drawing::resources::Definition {
                    name: t.name.trim().to_string(),
                    ..t.def.clone()
                };
                let name = def.name.clone();
                let border = t.border;
                match self.edit_drawing(what, |d| match border {
                    Some(z) => d.add_border(crate::drawing::resources::BorderDef {
                        def,
                        zones: z.then(ZoneParams::default),
                    }),
                    None => d.add_title_block(def),
                }) {
                    Some(Err(e)) => {
                        self.history.cancel();
                        self.file_status = Some(Err(e));
                        self.drw.title_save = Some(t);
                    }
                    _ => {
                        self.drw.title_save = None;
                        self.file_status =
                            Some(Ok(format!("{what}: {name} saved in Drawing Resources")));
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
                let (i, name, border) = (p.sheet, p.name.clone(), p.border);
                self.edit_drawing("Edit Field Text", |d| {
                    if border {
                        // The sheet keeps its zone parameters.
                        let params = d.sheets.get(i).and_then(|s| s.border_params.clone());
                        d.set_border(i, Some(&name), values, params)
                    } else {
                        d.set_title_block(i, Some(&name), values)
                    }
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
            let i = dlg.sheet;
            self.drw.border_dialog = None;
            if let Some(n) = dlg.insert {
                self.edit_drawing("Insert Border", |d| {
                    d.set_border(i, Some(&n), Vec::new(), Some(p))
                });
                let prompts = self.drawing().is_some_and(|d| {
                    d.resources
                        .borders
                        .iter()
                        .any(|b| b.def.name == n && !b.def.prompts().is_empty())
                });
                if prompts {
                    self.open_prompt_dialog(i, true);
                }
                return;
            }
            let own = (p != ZoneParams::default()).then_some(p);
            self.edit_drawing("Edit Border", |d| {
                if let Some(s) = d.sheets.get_mut(i) {
                    s.border_params = own;
                }
            });
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
