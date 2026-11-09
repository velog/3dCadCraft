//! Drawing resources in the GUI (spec 4.13): the sheet's border and title
//! block rows in the browser and Edit Drawing Border Parameters.

use eframe::egui;

use super::CadCraftApp;
use super::icons::Icon;
use super::valuefield::ExprValue;
use crate::drawing::resources::{Delimit, LabelFrom, ResourceKind, ZoneLabel, ZoneParams};

/// What a border / title block row of the browser asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum FormatAction {
    /// Replace what is on the sheet with another definition.
    Replace(ResAction),
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
    /// Define New Symbol.
    DefineSymbol,
    /// Sketch Symbols on definition `name`.
    InsertSymbol(String),
    /// Edit Definition of symbol `name`.
    EditSymbol(String),
    /// Delete symbol `name`.
    DeleteSymbol(String),
    /// Edit of the standard's title block: makes a copy of it first.
    EditStandardTitle,
    /// Edit of the default border: makes a copy of it first.
    EditDefaultBorder,
    /// Copy a definition.
    Copy(ResourceKind, String),
    /// Ask a definition's new name.
    Rename(ResourceKind, String),
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
    /// What can replace the title block: (name, `None` = the standard's).
    pub titles: Vec<(String, Option<String>)>,
    /// What can replace the border: (name, `None` = the default one).
    pub borders: Vec<(String, Option<String>)>,
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
        let titles = std::iter::once((d.standard.kind.label().to_string(), None))
            .chain(
                d.resources
                    .title_blocks
                    .iter()
                    .map(|b| (b.name.clone(), Some(b.name.clone()))),
            )
            .collect();
        let borders = std::iter::once((DEFAULT_BORDER.to_string(), None))
            .chain(
                d.resources
                    .borders
                    .iter()
                    .map(|b| (b.def.name.clone(), Some(b.def.name.clone()))),
            )
            .collect();
        Self {
            border,
            title,
            titles,
            borders,
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
    /// Insert of this zone border: OK puts it on the sheet with these
    /// parameters (`None`: Edit Instance).
    pub insert: Option<String>,
}

/// The name asked when a new title block is saved.
#[derive(Clone, Debug)]
pub(super) struct TitleSave {
    pub name: String,
    pub def: crate::drawing::resources::Definition,
    /// A border (`Some(zone border)`), else a title block or a symbol.
    pub border: Option<bool>,
    /// A sketched symbol.
    pub symbol: bool,
    focused: bool,
}

/// The Rename window of a definition.
#[derive(Clone, Debug)]
pub(super) struct ResRename {
    pub kind: ResourceKind,
    pub old: String,
    pub name: String,
    focused: bool,
}

/// Edit Property Fields' Property Type.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum PropertyType {
    /// Everything the definition reads.
    #[default]
    All,
    /// Only what is asked of the user.
    Prompted,
    /// Only what comes from Properties and the sheet.
    Properties,
}

impl PropertyType {
    const ALL: [Self; 3] = [Self::All, Self::Prompted, Self::Properties];

    fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Prompted => "Prompted Entries",
            Self::Properties => "Properties",
        }
    }
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
    /// Which rows are listed.
    pub filter: PropertyType,
}

/// Replace's list on a sheet's border or title block row.
fn replace_menu(
    ui: &mut egui::Ui,
    current: &str,
    options: &[(String, Option<String>)],
    border: bool,
    action: &mut Option<FormatAction>,
) {
    let others: Vec<_> = options.iter().filter(|(n, _)| n != current).collect();
    ui.add_enabled_ui(!others.is_empty(), |ui| {
        ui.menu_button("Replace", |ui| {
            for (label, def) in others {
                if ui.button(label.as_str()).clicked() {
                    *action = Some(FormatAction::Replace(if border {
                        ResAction::InsertBorder(def.clone())
                    } else {
                        ResAction::Insert(def.clone())
                    }));
                    ui.close();
                }
            }
        });
    });
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
                if ui.button("Edit Definition...").clicked() {
                    action = Some(FormatAction::EditBorderDef);
                    ui.close();
                }
                replace_menu(ui, &b.name, &f.borders, true, &mut action);
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
    if let Some(FormatRow { name, prompts, .. }) = &f.title {
        ui.horizontal(|ui| {
            super::panels::tree_icon(ui, Icon::Drawing);
            let r = ui.selectable_label(false, name.as_str());
            if r.double_clicked() && *prompts {
                action = Some(FormatAction::EditFields);
            }
            r.context_menu(|ui| {
                if ui.button("Edit Definition...").clicked() {
                    action = Some(FormatAction::EditTitle);
                    ui.close();
                }
                replace_menu(ui, name, &f.titles, false, &mut action);
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
                    if ui.button("Edit...").clicked() {
                        *action = Some(ResAction::EditStandardTitle);
                        ui.close();
                    }
                }
            }
            copy_rename(ui, ResourceKind::TitleBlock, user, action);
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
                    if ui.button("Edit...").clicked() {
                        *action = Some(ResAction::EditDefaultBorder);
                        ui.close();
                    }
                }
            }
            copy_rename(ui, ResourceKind::Border, user, action);
        });
    });
}

/// A symbol of Drawing Resources.
fn symbol_row(ui: &mut egui::Ui, name: &str, action: &mut Option<ResAction>) {
    ui.horizontal(|ui| {
        super::panels::tree_icon(ui, Icon::Drawing);
        let r = ui
            .selectable_label(false, name)
            .on_hover_text("Double-click: place it on the active sheet");
        if r.double_clicked() {
            *action = Some(ResAction::InsertSymbol(name.to_string()));
        }
        r.context_menu(|ui| {
            for (label, a) in [
                ("Insert", ResAction::InsertSymbol(name.to_string())),
                ("Edit Symbol...", ResAction::EditSymbol(name.to_string())),
                ("Delete", ResAction::DeleteSymbol(name.to_string())),
            ] {
                if ui.button(label).clicked() {
                    *action = Some(a);
                    ui.close();
                }
            }
            copy_rename(ui, ResourceKind::Symbol, Some(&name.to_string()), action);
        });
    });
}

/// Copy and Rename of a definition's menu (`None`: built in, greyed).
fn copy_rename(
    ui: &mut egui::Ui,
    kind: ResourceKind,
    user: Option<&String>,
    action: &mut Option<ResAction>,
) {
    let Some(n) = user else {
        for item in ["Copy", "Rename"] {
            ui.add_enabled(false, egui::Button::new(item))
                .on_disabled_hover_text("Built in: only your own definitions");
        }
        return;
    };
    if ui.button("Copy").clicked() {
        *action = Some(ResAction::Copy(kind, n.clone()));
        ui.close();
    }
    if ui.button("Rename").clicked() {
        *action = Some(ResAction::Rename(kind, n.clone()));
        ui.close();
    }
}

/// Drawing Resources in the browser: the folders, the built-in border and
/// title block, the user's title blocks (`blocks`), borders (`borders`) and
/// symbols (`symbols`).
pub(super) fn resources_tree(
    ui: &mut egui::Ui,
    title_name: &str,
    blocks: &[String],
    borders: &[String],
    symbols: &[String],
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
        if symbols.is_empty() {
            ui.weak("(none)");
        }
        for s in symbols {
            symbol_row(ui, s, &mut action);
        }
    })
    .context_menu(|ui| {
        if ui.button("Define New Symbol").clicked() {
            action = Some(ResAction::DefineSymbol);
            ui.close();
        }
    });
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
    /// Parameters and physical properties the texts read.
    values: Vec<(crate::drawing::fields::Field, String)>,
}

impl FieldProps {
    /// The value of `f` (`None`: no such value, drawn as `#?`).
    pub fn get(&self, f: &crate::drawing::fields::Field) -> Option<String> {
        use crate::drawing::fields::{Field, property};
        match f {
            Field::Drawing(k) => {
                let p = self.drawing.as_ref()?;
                let v = property(p, k)?;
                Some(if v.is_empty() && k == "designer" {
                    p.get("author").to_string()
                } else {
                    v
                })
            }
            Field::Model(k) => {
                let p = self.model.as_ref()?;
                let v = property(p, k)?;
                // The fallbacks of the built-in title block: a block made
                // from it reads the same.
                Some(match k.as_str() {
                    "part_number" if v.is_empty() => self.model_name.clone(),
                    "title" if v.is_empty() => {
                        let d = p.get("description").to_string();
                        if d.is_empty() {
                            self.model_name.clone()
                        } else {
                            d
                        }
                    }
                    _ => v,
                })
            }
            Field::Material => self.material.clone(),
            Field::Sheet(_) => None,
            Field::Parameter(_) | Field::Physical(_) => self
                .values
                .iter()
                .find(|(g, _)| g == f)
                .map(|(_, v)| v.clone()),
        }
    }
}

/// `base`, or `base (2)` … when that is taken.
fn free_name(base: &str, taken: impl Fn(&str) -> bool) -> String {
    std::iter::once(base.to_string())
        .chain((2..).map(|k| format!("{base} ({k})")))
        .find(|n| !taken(n))
        .unwrap_or_else(|| base.to_string())
}

impl CadCraftApp {
    /// The Properties for the fields of sheet `i` of `d`.
    pub(super) fn field_props(&self, d: &crate::drawing::DrawingDocument, i: usize) -> FieldProps {
        let model = d.sheets.get(i).and_then(|s| d.source_model(s));
        let drawing = self
            .project
            .document(self.part_id)
            .map(|x| x.properties.clone());
        let model_doc = model.and_then(|m| self.model_properties(m));
        let (model_props, model_name) = match model_doc {
            Some((p, n)) => (Some(p), n),
            None => (None, String::new()),
        };
        let material = model.map(|m| {
            self.with_part(m, |p| p.material.clone())
                .flatten()
                .unwrap_or_else(|| "Generic".to_string())
        });
        FieldProps {
            drawing,
            model: model_props,
            model_name,
            material,
            values: model.map(|m| self.model_values(d, m)).unwrap_or_default(),
        }
    }

    /// The Properties and name of model `r`: a document of this file or of
    /// a linked one (as edited from here), with its variant's own values.
    pub(super) fn model_properties(
        &self,
        r: crate::document::DocRef,
    ) -> Option<(crate::document::iprops::IProperties, String)> {
        let (mut props, name) = match r.library {
            None => {
                let d = self.project.document(r.document)?;
                (d.properties.clone(), d.name.clone())
            }
            Some(lib) => {
                let read = |p: &crate::document::Project| {
                    p.document(r.document)
                        .map(|d| (d.properties.clone(), d.name.clone()))
                };
                match self.project.linked_edits.get(&lib) {
                    Some(p) => read(p)?,
                    None => read(&*self.part_file_pub(&self.library_path(lib)?).ok()?)?,
                }
            }
        };
        if let Some(v) = r.variant
            && let Some(Some(own)) =
                self.with_part(r.base(), |p| p.variants.get(v).map(|x| x.info.clone()))
        {
            own.write(&mut props);
        }
        Some((props, name))
    }

    /// The parameter and physical fields `d`'s texts read, from model `m`.
    fn model_values(
        &self,
        d: &crate::drawing::DrawingDocument,
        m: crate::document::DocRef,
    ) -> Vec<(crate::drawing::fields::Field, String)> {
        use crate::drawing::fields::{Field, PhysicalField};
        let wanted = crate::drawing::resources::model_value_fields(d);
        if wanted.is_empty() {
            return Vec::new();
        }
        let table = self.with_part(m, |p| {
            p.parameter_table()
                .into_iter()
                .map(|r| {
                    let v = crate::part::params::format_in(r.value, r.def(), "");
                    (r.name, v)
                })
                .collect::<Vec<_>>()
        });
        // Mass and the like integrate the triangles: once per body.
        let mass = wanted
            .iter()
            .any(|f| matches!(f, Field::Physical(_)))
            .then(|| {
                let key = (m, self.with_part(m, |p| p.body_signature())?);
                let mut cache = self.drw.physical.borrow_mut();
                if let Some(v) = cache.get(&key) {
                    return Some(*v);
                }
                let v = self
                    .with_part(m, |p| {
                        crate::part::physical::physical_properties(p, Default::default())
                            .ok()
                            .map(|x| (x.total.mass, x.total.volume, x.total.area))
                    })
                    .flatten()?;
                cache.insert(key, v);
                Some(v)
            })
            .flatten();
        wanted
            .into_iter()
            .filter_map(|f| {
                let v = match &f {
                    Field::Parameter(n) => table
                        .as_ref()?
                        .iter()
                        .find(|(k, _)| k == n)
                        .map(|(_, v)| v.clone())?,
                    Field::Physical(p) => {
                        let (ma, vo, ar) = mass?;
                        match p {
                            PhysicalField::Mass => format!("{ma:.3}"),
                            PhysicalField::Volume => format!("{vo:.3}"),
                            PhysicalField::Area => format!("{ar:.3}"),
                        }
                    }
                    _ => return None,
                };
                Some((f, v))
            })
            .collect()
    }

    /// Properties of model `r`: its document, or (read only) the linked file.
    pub(super) fn open_model_iproperties(&mut self, r: crate::document::DocRef) {
        match r.library {
            None => self.open_iproperties(r.document),
            Some(lib) => {
                let linked = self
                    .library_path(lib)
                    .and_then(|p| Some((p.clone(), self.part_file_pub(&p).ok()?)));
                self.iprops = Some(super::iprops::IPropsWin::new(r.document, linked));
            }
        }
    }

    /// Edit of the standard's title block: a copy with live fields takes
    /// its place on the active sheet and opens.
    fn edit_standard_title(&mut self) {
        let Some((i, name)) = self.drawing().map(|d| {
            let base = format!("{} Copy", d.standard.kind.label());
            (
                d.active_sheet,
                free_name(&base, |n| {
                    d.resources.title_blocks.iter().any(|b| b.name == n)
                }),
            )
        }) else {
            return;
        };
        let done = self.edit_drawing("Edit Title Block", |d| {
            let Some(def) =
                crate::drawing::resources::standard_title_definition(d, i, name.clone())
            else {
                return false;
            };
            d.add_title_block(def).is_ok() && d.set_title_block(i, Some(&name), Vec::new())
        });
        if done == Some(true) {
            self.edit_title_definition(&name);
        } else {
            self.history.cancel();
        }
    }

    /// Edit of the default border: a copy of it takes its place on the
    /// active sheet and opens.
    fn edit_default_border(&mut self) {
        use crate::drawing::resources::DEFAULT_BORDER;
        let Some((i, name)) = self.drawing().map(|d| {
            let base = format!("{DEFAULT_BORDER} Copy");
            (
                d.active_sheet,
                free_name(&base, |n| {
                    d.resources.borders.iter().any(|b| b.def.name == n)
                }),
            )
        }) else {
            return;
        };
        let done = self.edit_drawing("Edit Border", |d| {
            let Some(b) = crate::drawing::resources::default_border_definition(d, i, name.clone())
            else {
                return false;
            };
            d.add_border(b).is_ok() && d.set_border(i, Some(&name), Vec::new(), None)
        });
        if done == Some(true) {
            self.edit_border_definition(&name);
        } else {
            self.history.cancel();
        }
    }

    /// Runs a border / title block row's action on sheet `i`.
    pub(super) fn format_action(&mut self, i: usize, a: FormatAction) {
        match a {
            FormatAction::Replace(r) => {
                self.activate_sheet(i);
                self.res_action(r);
            }
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
                match name {
                    Some(n) => self.edit_border_definition(&n),
                    None => {
                        self.activate_sheet(i);
                        self.edit_default_border();
                    }
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
                match name {
                    Some(n) => self.edit_title_definition(&n),
                    None => {
                        self.activate_sheet(i);
                        self.edit_standard_title();
                    }
                }
            }
            FormatAction::EditFields => self.open_prompt_dialog(i, false),
        }
    }

    /// What Insert `a` would replace on the active sheet: (the sheet, what
    /// is there, what comes). `None`: nothing or the same one.
    fn replaces(&self, a: &ResAction) -> Option<(String, String, String)> {
        let d = self.drawing()?;
        let sheet = d.sheet()?;
        let f = SheetFormat::of(d, sheet);
        let (old, new) = match a {
            ResAction::Insert(n) => (
                f.title?.name,
                n.clone()
                    .unwrap_or_else(|| d.standard.kind.label().to_string()),
            ),
            ResAction::InsertBorder(n) => (
                f.border?.name,
                n.clone()
                    .unwrap_or_else(|| crate::drawing::resources::DEFAULT_BORDER.to_string()),
            ),
            _ => return None,
        };
        (old != new).then(|| (sheet.name.clone(), old, new))
    }

    /// A Drawing Resources row's action; what would replace a sheet's border
    /// or title block asks first.
    pub(super) fn res_action(&mut self, a: ResAction) {
        if self.replaces(&a).is_some() {
            self.drw.replace_ask = Some(a);
        } else {
            self.res_run(a);
        }
    }

    /// Carries out a Drawing Resources row's action.
    fn res_run(&mut self, a: ResAction) {
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
                        filter: PropertyType::All,
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
                        filter: PropertyType::All,
                    });
                }
            }
            ResAction::EditBorder(n) => self.edit_border_definition(&n),
            ResAction::DefineSymbol => self.define_symbol(),
            ResAction::InsertSymbol(n) => self.open_symbols(Some(n)),
            ResAction::EditSymbol(n) => self.edit_symbol_definition(&n),
            ResAction::DeleteSymbol(n) => {
                let r = self.edit_drawing("Delete Symbol", |d| d.delete_symbol(&n));
                if let Some(Err(e)) = r {
                    self.history.cancel();
                    self.file_status = Some(Err(e));
                }
            }
            ResAction::DeleteBorder(n) => {
                let r = self.edit_drawing("Delete Border", |d| d.delete_border(&n));
                if let Some(Err(e)) = r {
                    self.history.cancel();
                    self.file_status = Some(Err(e));
                }
            }
            ResAction::Copy(kind, n) => {
                match self.edit_drawing("Copy", |d| d.copy_resource(kind, &n)) {
                    Some(Err(e)) => {
                        self.history.cancel();
                        self.file_status = Some(Err(e));
                    }
                    Some(Ok(copy)) => self.file_status = Some(Ok(format!("Copy: {copy}"))),
                    None => {}
                }
            }
            ResAction::EditStandardTitle => self.edit_standard_title(),
            ResAction::EditDefaultBorder => self.edit_default_border(),
            ResAction::Rename(kind, n) => {
                self.drw.res_rename = Some(ResRename {
                    kind,
                    name: n.clone(),
                    old: n,
                    focused: false,
                });
            }
        }
    }

    /// The question before a sheet's border or title block is replaced.
    fn replace_window(&mut self, ctx: &egui::Context) {
        let Some(a) = self.drw.replace_ask.clone() else {
            return;
        };
        let Some((sheet, old, new)) = self.replaces(&a) else {
            self.drw.replace_ask = None;
            return;
        };
        let title = match a {
            ResAction::Insert(_) => "Replace Title Block",
            _ => "Replace Border",
        };
        let mut open = true;
        let (mut yes, mut no) = (false, false);
        let win = egui::Window::new(title)
            .id(egui::Id::new("res_replace"))
            .open(&mut open)
            .resizable(false)
            .collapsible(false);
        self.layout
            .place("res_replace", ctx.content_rect(), false, win)
            .show(ctx, |ui| {
                ui.label(format!("Replace {old} on {sheet} with {new}?"));
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.add(super::theme::ok_button("Yes")).clicked()
                        || super::keys::panel_ok_key(ui, false)
                    {
                        yes = true;
                    }
                    if ui.button("No").clicked() {
                        no = true;
                    }
                });
            });
        if yes {
            self.drw.replace_ask = None;
            self.res_run(a);
        } else if no || !open {
            self.drw.replace_ask = None;
        }
    }

    /// The Rename window of a definition.
    fn rename_window(&mut self, ctx: &egui::Context) {
        let Some(mut r) = self.drw.res_rename.clone() else {
            return;
        };
        let mut open = true;
        let (mut ok, mut cancel) = (false, false);
        let win = egui::Window::new("Rename")
            .id(egui::Id::new("res_rename"))
            .open(&mut open)
            .resizable(false)
            .collapsible(false);
        self.layout
            .place("res_rename", ctx.content_rect(), false, win)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Name");
                    let f = ui.text_edit_singleline(&mut r.name);
                    if !r.focused {
                        f.request_focus();
                        r.focused = true;
                    }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    let can = !r.name.trim().is_empty();
                    if ui.add_enabled(can, super::theme::ok_button("OK")).clicked()
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
            self.drw.res_rename = None;
        } else if ok {
            let (kind, old, name) = (r.kind, r.old.clone(), r.name.clone());
            match self.edit_drawing("Rename", |d| d.rename_resource(kind, &old, &name)) {
                Some(Err(e)) => {
                    self.history.cancel();
                    self.file_status = Some(Err(e));
                    self.drw.res_rename = Some(r);
                }
                _ => self.drw.res_rename = None,
            }
        } else {
            self.drw.res_rename = Some(r);
        }
    }

    /// What the definition of an Edit Property Fields window reads from
    /// Properties and the sheet: (field, value).
    fn read_fields(&self, p: &PromptDialog) -> Vec<(String, String)> {
        use crate::drawing::fields::{Field, Piece, sheet_value};
        let Some(d) = self.drawing() else {
            return Vec::new();
        };
        let def = if p.border {
            d.resources
                .borders
                .iter()
                .map(|b| &b.def)
                .find(|b| b.name == p.name)
        } else {
            d.resources.title_blocks.iter().find(|b| b.name == p.name)
        };
        let Some(def) = def else {
            return Vec::new();
        };
        let props = self.field_props(d, p.sheet);
        let mut out: Vec<(String, String)> = Vec::new();
        for piece in def.texts.iter().flat_map(|t| &t.pieces) {
            let Piece::Field(f) = piece else {
                continue;
            };
            let name = f.name();
            if out.iter().any(|(n, _)| *n == name) {
                continue;
            }
            let value = match f {
                Field::Sheet(s) => sheet_value(d, p.sheet, *s),
                other => props.get(other),
            };
            out.push((name, value.unwrap_or_else(|| "#?".to_string())));
        }
        out
    }

    /// Define Sketched Symbol: a sheet sketch whose Finish saves a symbol.
    pub(super) fn define_symbol(&mut self) {
        self.open_view_sketch(
            super::drawing_sketch::SHEET,
            super::drawing_sketch::SketchFor::Symbol,
            Default::default(),
        );
        self.drw.sel = None;
        self.file_status = Some(Ok(
            "Define Sketched Symbol: sketch it; select a point › right-click › Insertion Point sets where it is held; Finish Sketch saves it"
                .into(),
        ));
    }

    /// Edit Definition of symbol `name`: its sketch opens where it was
    /// drawn; Finish Sketch changes every instance.
    pub(super) fn edit_symbol_definition(&mut self, name: &str) {
        let Some(def) = self
            .drawing()
            .and_then(|d| d.resources.symbols.iter().find(|s| s.name == name).cloned())
        else {
            return;
        };
        self.open_view_sketch(
            super::drawing_sketch::SHEET,
            super::drawing_sketch::SketchFor::Symbol,
            def.sketch,
        );
        if let Some(s) = self.drw.sketch.as_mut() {
            s.texts = def.texts;
            s.resource = Some(name.to_string());
            s.mark_connections(&def.connections);
            if let Some(q) = def.insertion {
                s.mark_insertion(q);
            }
        }
        self.drw.sel = None;
    }

    /// Finish Sketch of a symbol: a new one asks for its name, an edited
    /// one changes every instance.
    pub(super) fn finish_symbol_sketch(
        &mut self,
        def: crate::drawing::resources::Definition,
        resource: Option<String>,
    ) {
        if def.extent().is_none() {
            self.file_status = Some(Err(
                "Define Sketched Symbol: the symbol is empty (nothing saved)".into(),
            ));
            return;
        }
        match resource {
            Some(n) => {
                if self.edit_drawing("Edit Symbol", |d| d.update_symbol(&n, def)) != Some(true) {
                    self.history.cancel();
                }
                self.history.break_coalescing();
            }
            None => {
                let name = self
                    .drawing()
                    .map(|d| d.next_symbol_name())
                    .unwrap_or_default();
                self.drw.title_save = Some(TitleSave {
                    name,
                    def,
                    border: None,
                    symbol: true,
                    focused: false,
                });
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
                    symbol: false,
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
                    symbol: false,
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
            filter: PropertyType::All,
        });
    }

    /// The Save Title Block name window and Edit Property Fields.
    pub(super) fn resource_windows(&mut self, ctx: &egui::Context) {
        self.replace_window(ctx);
        self.rename_window(ctx);
        if let Some(mut t) = self.drw.title_save.clone() {
            let mut open = true;
            let (mut ok, mut cancel) = (false, false);
            let (title, what, folder) = match (t.symbol, t.border) {
                (true, _) => (
                    "Save Sketched Symbol",
                    "Define Sketched Symbol",
                    "Sketched Symbols",
                ),
                (_, Some(_)) => ("Save Border", "Define Border", "Borders"),
                _ => ("Save Title Block", "Define Title Block", "Title Blocks"),
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
                let symbol = t.symbol;
                match self.edit_drawing(what, |d| match border {
                    _ if symbol => d.add_symbol(def),
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
            let (mut ok, mut cancel, mut properties, mut model_props) =
                (false, false, false, false);
            let source = self
                .drawing()
                .and_then(|d| d.sheets.get(p.sheet).and_then(|s| d.source_model(s)));
            let source_name = source.map_or_else(
                || "(no view on this sheet)".to_string(),
                |m| {
                    self.model_properties(m)
                        .map_or_else(|| "(missing)".to_string(), |x| x.1)
                },
            );
            let read = self.read_fields(&p);
            let win = egui::Window::new("Edit Property Fields")
                .open(&mut open)
                .resizable(false)
                .collapsible(false);
            self.layout
                .place("prompt_fields", ctx.content_rect(), false, win)
                .show(ctx, |ui| {
                    ui.label(format!("Fields of {}", p.name));
                    ui.horizontal(|ui| {
                        ui.label("Property Type");
                        egui::ComboBox::from_id_salt("prompt_property_type")
                            .selected_text(p.filter.label())
                            .show_ui(ui, |ui| {
                                for f in PropertyType::ALL {
                                    ui.selectable_value(&mut p.filter, f, f.label());
                                }
                            });
                        properties = ui
                            .button("Drawing Properties...")
                            .on_hover_text("The drawing's Properties; the fields follow at once")
                            .clicked();
                    });
                    ui.horizontal(|ui| {
                        ui.label(format!("Model: {source_name}"));
                        model_props = ui
                            .add_enabled(source.is_some(), egui::Button::new("Model Properties..."))
                            .on_hover_text("The Properties of the model these fields read")
                            .clicked();
                    });
                    egui::Grid::new("prompt_fields_grid")
                        .num_columns(2)
                        .striped(true)
                        .spacing([12.0, 6.0])
                        .show(ui, |ui| {
                            ui.strong("Property Field");
                            ui.strong("Value");
                            ui.end_row();
                            if p.filter != PropertyType::Properties {
                                for (title, value) in &mut p.values {
                                    ui.label(title.as_str());
                                    ui.text_edit_singleline(value);
                                    ui.end_row();
                                }
                            }
                            if p.filter != PropertyType::Prompted {
                                for (title, value) in &read {
                                    ui.label(title.as_str());
                                    ui.add_enabled(false, egui::Label::new(value.as_str()));
                                    ui.end_row();
                                }
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
            if properties {
                self.open_iproperties(self.part_id);
            }
            if model_props && let Some(m) = source {
                self.open_model_iproperties(m);
            }
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
