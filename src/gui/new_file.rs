//! File → New (Inventor's Create New File, `docs/spec/0.6c-templates.md`
//! §3.2): the templates of the active project's templates folder — a
//! navigation bar, the folder tree on the left, the folder's templates grouped
//! by kind in the middle, the selected template's details on the right, the
//! project choice and Create / Cancel below. Sheet metal is made in a Part
//! (its tab is in every part, owner 2026-10-27).

use std::path::{Path, PathBuf};

use eframe::egui::{self, RichText};

use super::CadCraftApp;
use super::icons::Icon;
use super::ribbon::{Size, icon_button};
use crate::document::templates::{Folder, Template};
use crate::document::{DocumentData, DocumentKind};

/// What the quick New entries make.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Part,
    Assembly,
    Drawing,
}

impl Kind {
    pub(super) fn doc_kind(self) -> DocumentKind {
        match self {
            Kind::Part => DocumentKind::Part,
            Kind::Assembly => DocumentKind::Assembly,
            Kind::Drawing => DocumentKind::Drawing,
        }
    }
}

fn kind_icon(k: DocumentKind) -> Icon {
    match k {
        DocumentKind::Part => Icon::Part,
        DocumentKind::Assembly => Icon::Assembly,
        DocumentKind::Drawing => Icon::Drawing,
    }
}

/// What the details pane says about a template (read once per selection).
fn template_details(t: &Template) -> Vec<String> {
    let mut v = vec![super::design_projects::kind_about(t.kind).to_owned()];
    match crate::document::templates::instantiate(&t.path) {
        Err(e) => v.push(format!("Cannot be used: {e}")),
        Ok(n) => {
            match &n.data {
                DocumentData::Drawing(d) => {
                    v.push(format!("Standard: {}", d.standard.kind.label()));
                    for s in &d.sheets {
                        let (w, h) = s.dims();
                        v.push(format!("{}: {w:.0} x {h:.0} mm", s.name));
                    }
                }
                DocumentData::Part(p) => {
                    if let Some(m) = &p.material {
                        v.push(format!("Material: {m}"));
                    }
                    if !p.user_params.is_empty() {
                        v.push(format!("{} user parameters", p.user_params.len()));
                    }
                    if !p.features.is_empty() {
                        v.push(format!("{} features", p.features.len()));
                    }
                }
                DocumentData::Assembly(a) => {
                    if !a.occurrences.is_empty() {
                        v.push(format!("{} components", a.occurrences.len()));
                    }
                }
            }
            let filled: Vec<String> = n
                .properties
                .standard
                .iter()
                .filter(|(k, _)| !matches!(k.as_str(), "author" | "designer" | "creation_date"))
                .map(|(k, val)| format!("{k}: {val}"))
                .collect();
            if !filled.is_empty() {
                v.push(format!("Properties: {}", filled.join(", ")));
            }
        }
    }
    if let Ok(m) = std::fs::metadata(&t.path).and_then(|m| m.modified()) {
        v.push(format!("Modified: {}", super::files::date_text(m)));
    }
    v
}

impl CadCraftApp {
    /// File → New / Ctrl+N: the chooser.
    pub(super) fn open_new_file(&mut self) {
        self.new_file = true;
        self.projects.new_details = None;
    }

    /// The Create New File window.
    pub(super) fn new_file_window(&mut self, ctx: &egui::Context) {
        if !self.new_file {
            return;
        }
        let tree = self.templates_tree(false);
        let mut open = true;
        let mut cancel = false;
        let mut create: Option<(PathBuf, DocumentKind)> = None;
        let mut refresh = false;
        let mut projects = false;
        let mut explore = None;
        let root = tree.as_ref().map(|(d, _)| d.clone());
        // The folder shown: the remembered one while it is in the tree.
        let folder_path = match (&tree, &self.projects.new_folder) {
            (Some((_, Ok(t))), Some(f)) if t.find(f).is_some() => f.clone(),
            (Some((d, _)), _) => d.clone(),
            _ => PathBuf::new(),
        };
        let win = egui::Window::new("Create New File")
            .open(&mut open)
            .resizable(true)
            .collapsible(false)
            .default_size([760.0, 470.0])
            .max_width(1000.0);
        self.layout
            .place("new_file", ctx.content_rect(), true, win)
            .show(ctx, |ui| {
                // Navigation bar.
                ui.horizontal(|ui| {
                    let up = root.as_ref().is_some_and(|r| folder_path != *r);
                    if ui
                        .add_enabled(up, egui::Button::new("Up"))
                        .on_hover_text("Up one folder")
                        .clicked()
                    {
                        self.projects.new_folder = folder_path.parent().map(Path::to_path_buf);
                    }
                    if ui
                        .add_enabled(up, egui::Button::new("Top"))
                        .on_hover_text("The top templates folder")
                        .clicked()
                    {
                        self.projects.new_folder = root.clone();
                    }
                    if ui
                        .button("Refresh")
                        .on_hover_text("Read the folder again")
                        .clicked()
                    {
                        refresh = true;
                    }
                    if ui
                        .button("Copy Path")
                        .on_hover_text("Copy the folder's path")
                        .clicked()
                    {
                        ui.ctx().copy_text(folder_path.display().to_string());
                    }
                    if ui
                        .button("Show in Explorer")
                        .on_hover_text("Open the folder in the file manager")
                        .clicked()
                    {
                        explore = Some(folder_path.clone());
                    }
                    ui.add(
                        egui::Label::new(RichText::new(folder_path.display().to_string()).weak())
                            .truncate(),
                    )
                    .on_hover_text(folder_path.display().to_string());
                });
                ui.separator();
                // Project and buttons at the bottom first: the rest takes what is left.
                egui::Panel::bottom(egui::Id::new("new_file_bottom"))
                    .resizable(false)
                    .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(0, 6)))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Project file:");
                            self.project_combo(ui, "new_file_project");
                            if ui
                                .button("Projects...")
                                .on_hover_text("The Projects editor")
                                .clicked()
                            {
                                projects = true;
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.button("Cancel").clicked() {
                                        cancel = true;
                                    }
                                    let sel = self.projects.new_selected.clone().and_then(|p| {
                                        tree.as_ref().and_then(|(_, t)| t.as_ref().ok()).and_then(
                                            |t| t.all().into_iter().find(|x| x.path == p).cloned(),
                                        )
                                    });
                                    if ui
                                        .add_enabled(
                                            sel.is_some(),
                                            super::theme::ok_button("Create"),
                                        )
                                        .on_hover_text("A new file from the selected template")
                                        .clicked()
                                        && let Some(t) = sel
                                    {
                                        create = Some((t.path, t.kind));
                                    }
                                },
                            );
                        });
                        if let Some(m) = &self.projects.message {
                            ui.label(RichText::new(m).color(super::theme::tokens().warning));
                        }
                    });
                match &tree {
                    None => {
                        ui.label("No templates folder: the built-in documents are used.");
                    }
                    Some((d, Err(e))) => {
                        ui.label(
                            RichText::new(format!("Templates folder {}: {e}", d.display()))
                                .color(super::theme::tokens().error),
                        );
                        ui.weak("New Part / Assembly / Drawing use the built-in documents.");
                    }
                    Some((_, Ok(t))) => {
                        let folder = t.find(&folder_path).unwrap_or(t).clone();
                        egui::Panel::left(egui::Id::new("new_file_tree"))
                            .resizable(true)
                            .default_size(170.0)
                            .show(ui, |ui| {
                                egui::ScrollArea::vertical().show(ui, |ui| {
                                    self.template_tree_node(ui, t, &folder_path, true);
                                });
                            });
                        egui::Panel::right(egui::Id::new("new_file_details"))
                            .resizable(true)
                            .default_size(210.0)
                            .show(ui, |ui| {
                                self.template_details_pane(ui, &folder);
                            });
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            if let Some(c) = self.template_list(ui, &folder) {
                                create = Some(c);
                            }
                        });
                    }
                }
            });
        if refresh {
            self.templates_tree(true);
            self.projects.new_details = None;
        }
        if projects {
            self.open_projects_editor();
        }
        if let Some(p) = explore {
            let _ = if cfg!(windows) {
                std::process::Command::new("explorer").arg(&p).spawn()
            } else {
                std::process::Command::new("xdg-open").arg(&p).spawn()
            };
        }
        if let Some((path, kind)) = create {
            self.new_file = false;
            self.home.shown = false;
            self.new_from_template(Some(&path), kind);
        } else if !open || cancel {
            self.new_file = false;
        }
    }

    /// One node of the folder tree (and its subfolders).
    fn template_tree_node(&mut self, ui: &mut egui::Ui, f: &Folder, shown: &Path, top: bool) {
        let name = if top {
            "Templates".to_owned()
        } else {
            f.name.clone()
        };
        let sel = f.path == shown;
        if f.folders.is_empty() {
            if ui.selectable_label(sel, name).clicked() {
                self.projects.new_folder = Some(f.path.clone());
            }
            return;
        }
        let id = ui.make_persistent_id(("tpl_folder", &f.path));
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true)
            .show_header(ui, |ui| {
                if ui.selectable_label(sel, name).clicked() {
                    self.projects.new_folder = Some(f.path.clone());
                }
            })
            .body(|ui| {
                for sub in &f.folders {
                    self.template_tree_node(ui, sub, shown, false);
                }
            });
    }

    /// The folder's templates grouped by kind; a double-click creates.
    fn template_list(&mut self, ui: &mut egui::Ui, f: &Folder) -> Option<(PathBuf, DocumentKind)> {
        let mut create = None;
        for (kind, title) in [
            (Some(DocumentKind::Part), "Part"),
            (Some(DocumentKind::Assembly), "Assembly"),
            (Some(DocumentKind::Drawing), "Drawing"),
            (None, "Presentation"),
        ] {
            ui.label(RichText::new(title).strong());
            let Some(kind) = kind else {
                ui.weak("Presentations come with their document kind (.ccpres, spec 0.6a)");
                continue;
            };
            let of: Vec<&Template> = f.templates.iter().filter(|t| t.kind == kind).collect();
            if of.is_empty() {
                ui.weak("No templates of this kind in this folder");
                ui.add_space(6.0);
                continue;
            }
            ui.horizontal_wrapped(|ui| {
                for t in of {
                    let on = self.projects.new_selected.as_ref() == Some(&t.path);
                    let r = ui
                        .allocate_ui_with_layout(
                            egui::vec2(96.0, 74.0),
                            egui::Layout::top_down(egui::Align::Center),
                            |ui| {
                                ui.set_width(96.0);
                                icon_button(
                                    ui,
                                    kind_icon(t.kind),
                                    &t.name,
                                    Size::Large,
                                    true,
                                    on,
                                    false,
                                )
                            },
                        )
                        .inner
                        .on_hover_text(t.path.display().to_string());
                    if r.clicked() {
                        self.projects.new_selected = Some(t.path.clone());
                    }
                    if r.double_clicked() {
                        create = Some((t.path.clone(), t.kind));
                    }
                }
            });
            ui.add_space(6.0);
        }
        for (p, why) in &f.unreadable {
            ui.label(
                RichText::new(
                    p.file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                )
                .weak()
                .strikethrough(),
            )
            .on_hover_text(why);
        }
        create
    }

    /// The selected template's details.
    fn template_details_pane(&mut self, ui: &mut egui::Ui, f: &Folder) {
        let found = self
            .projects
            .new_selected
            .clone()
            .and_then(|sel| f.templates.iter().find(|t| t.path == sel).cloned());
        let Some(t) = found else {
            ui.weak("Select a template");
            return;
        };
        let stale = self
            .projects
            .new_details
            .as_ref()
            .is_none_or(|(p, _)| *p != t.path);
        if stale {
            self.projects.new_details = Some((t.path.clone(), template_details(&t)));
        }
        ui.label(RichText::new(&t.name).strong());
        if let Some((_, lines)) = &self.projects.new_details {
            for l in lines {
                ui.label(l);
            }
        }
    }

    /// A new document of kind `k` in a tab of its own from the standard
    /// template (Home's New drop-down, Manage › Documents with separate files).
    pub(super) fn new_file_of(&mut self, k: Kind, _now: f64) {
        self.new_from_template(None, k.doc_kind());
    }
}
