//! Design projects in the GUI (`docs/spec/0.6a-file-types-projects.md` §3.2):
//! the active project (Home's and Create New File's project list), New
//! Project, Browse, switching only with no changed documents open, and the
//! Projects editor. Also the templates side of New (`docs/spec/0.6c-templates.md`):
//! the templates folder in force, a new file from a template and Save Copy As
//! Template.
//!
//! The list and the active project are user settings (`settings.json`
//! `projects`), never document data.

use std::path::{Path, PathBuf};

use eframe::egui::{self, RichText};

use super::CadCraftApp;
use super::files::DialogPurpose;
use crate::document::design_project::{
    ActiveProject, DesignProject, NamedPath, ProjectList, create_project,
};
use crate::document::templates::{self, Folder, NewDocument};
use crate::document::{DocumentKind, FileType, Project};

/// The Projects editor's state.
#[derive(Clone, Debug, Default)]
pub(super) struct Editor {
    /// The project whose settings are shown (`None` = Default).
    selected: Option<PathBuf>,
    /// Its settings as edited (read from the file when it was selected).
    draft: Option<DesignProject>,
    /// The draft differs from the file.
    changed: bool,
    /// Why the selected file cannot be read or saved.
    error: Option<String>,
}

/// Projects and templates state of the app.
#[derive(Debug, Default)]
pub(super) struct ProjectsUi {
    loaded: bool,
    /// Known projects and the active one.
    pub list: ProjectList,
    active: Option<ActiveProject>,
    /// The last refusal or warning (shown on Home and in the dialogs).
    pub message: Option<String>,
    /// The Projects editor, when open.
    pub editor: Option<Editor>,
    /// The templates tree last read: (folder, tree or why not).
    tree: Option<(PathBuf, Result<Folder, String>)>,
    /// Create New File: the folder shown and the template selected.
    pub new_folder: Option<PathBuf>,
    pub new_selected: Option<PathBuf>,
    /// The details pane of the selected template: (file, lines).
    pub new_details: Option<(PathBuf, Vec<String>)>,
}

/// The projects list of `settings.json`.
fn read_list() -> ProjectList {
    super::home::read_settings()
        .get("projects")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

fn write_list(list: &ProjectList) {
    let mut v = super::home::read_settings();
    if let Ok(l) = serde_json::to_value(list) {
        v["projects"] = l;
    }
    super::home::write_settings(&v);
}

/// The kind's description in Create New File.
pub(super) fn kind_about(k: DocumentKind) -> &'static str {
    match k {
        DocumentKind::Part => {
            "Part: solids, surfaces and sheet metal built from sketches and features"
        }
        DocumentKind::Assembly => {
            "Assembly: parts and subassemblies placed and constrained together"
        }
        DocumentKind::Drawing => {
            "Drawing: sheets with views, dimensions and tables of parts and assemblies"
        }
    }
}

impl CadCraftApp {
    /// Reads the projects list once and opens the active project.
    pub(super) fn projects_load(&mut self) {
        if self.projects.loaded {
            return;
        }
        self.projects.loaded = true;
        self.projects.list = read_list();
        let docs = super::files::documents_dir();
        let (a, warn) = ActiveProject::from_list(&self.projects.list, &docs);
        if let Some(w) = warn {
            log::warn!("project: {w}; Default is active");
            self.projects.message = Some(format!("Project: {w}; Default is active"));
        }
        self.projects.active = Some(a);
    }

    /// The project in force.
    pub(super) fn active_project(&mut self) -> &ActiveProject {
        self.projects_load();
        if self.projects.active.is_none() {
            self.projects.active = Some(ActiveProject::default_in(&super::files::documents_dir()));
        }
        // Filled just above.
        self.projects
            .active
            .get_or_insert_with(|| ActiveProject::default_in(&super::files::documents_dir()))
    }

    /// The active project's workspace (New / Open / Save As start there);
    /// Default's own folder is created on first use.
    pub(super) fn workspace_dir(&mut self) -> Option<PathBuf> {
        let a = self.active_project().clone();
        let ws = a.workspace();
        if !ws.is_dir() && a.is_default() {
            let _ = std::fs::create_dir_all(&ws);
        }
        ws.is_dir().then_some(ws)
    }

    /// The templates folder in force (spec 0.6c §3.1); the default one gets
    /// the built-in templates when it has none.
    pub(super) fn templates_dir(&mut self) -> Option<PathBuf> {
        let a = self.active_project().clone();
        let d = templates::dir_for(&a.project, a.base(), None)?;
        if Some(&d) == templates::default_dir().as_ref()
            && let Err(e) = templates::ensure_builtin(&d)
        {
            log::warn!("templates: {e}");
        }
        Some(d)
    }

    /// The templates tree of the folder in force (read again on `refresh`).
    pub(super) fn templates_tree(
        &mut self,
        refresh: bool,
    ) -> Option<(PathBuf, Result<Folder, String>)> {
        let dir = self.templates_dir()?;
        let stale = refresh || self.projects.tree.as_ref().is_none_or(|(d, _)| *d != dir);
        if stale {
            let t = templates::list(&dir).map_err(|e| e.to_string());
            self.projects.tree = Some((dir, t));
        }
        self.projects.tree.clone()
    }

    /// The names of open documents with unsaved changes.
    fn changed_documents(&mut self) -> Vec<String> {
        let mut v = Vec::new();
        if self.is_dirty(f64::INFINITY, true) {
            v.push(self.document_title());
        }
        for t in self.tabs.iter().flatten() {
            if t.dirty {
                v.push(t.title.clone());
            }
        }
        v
    }

    /// Makes `to` the active project (`None` = Default). Refused while a
    /// document has unsaved changes; unchanged documents are closed (spec
    /// 0.6a §3.2). Returns whether it switched.
    pub(super) fn switch_project(&mut self, to: Option<PathBuf>) -> bool {
        self.projects_load();
        if self.projects.list.active == to {
            return true;
        }
        let project = match &to {
            None => ActiveProject::default_in(&super::files::documents_dir()),
            Some(f) => match DesignProject::load(f) {
                Ok(project) => ActiveProject {
                    project,
                    file: Some(f.clone()),
                },
                Err(e) => {
                    self.projects.message = Some(format!("Project: {e}"));
                    return false;
                }
            },
        };
        let changed = self.changed_documents();
        if !changed.is_empty() {
            self.projects.message = Some(format!(
                "Project: save or close {} first (a project changes only with no changed documents open)",
                changed.join(", ")
            ));
            return false;
        }
        // Every document closes; the last close leaves an empty Untitled.
        while self.tab_count() > 1 {
            self.close_active_tab();
        }
        if !self.active_is_blank() {
            self.close_active_tab();
        }
        let ws = project.workspace();
        let name = project.name().to_owned();
        self.projects.list.activate(to.as_deref());
        write_list(&self.projects.list);
        self.projects.active = Some(project);
        self.projects.tree = None;
        self.projects.new_folder = None;
        self.projects.new_selected = None;
        self.home.forget_list();
        let note = if ws.is_dir() {
            String::new()
        } else {
            match std::fs::create_dir_all(&ws) {
                Ok(()) => format!(" (workspace {} created)", ws.display()),
                Err(e) => format!(" (workspace {}: {e})", ws.display()),
            }
        };
        self.projects.message = None;
        self.file_status = Some(Ok(format!("Project {name} is active{note}")));
        true
    }

    /// A `.ccproj` chosen in Browse... (`new` = New Project's Save dialog).
    pub(super) fn project_file_picked(&mut self, path: &Path, new: bool) {
        self.projects_load();
        let path = if new {
            let folder = path.parent().unwrap_or(Path::new("."));
            let name = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            match create_project(folder, &name) {
                Ok(p) => p,
                Err(e) => {
                    self.file_status = Some(Err(format!("New Project: {e}")));
                    return;
                }
            }
        } else {
            path.to_path_buf()
        };
        if let Err(e) = DesignProject::load(&path) {
            self.file_status = Some(Err(format!("Open Project: {e}")));
            return;
        }
        self.projects.list.add(&path);
        write_list(&self.projects.list);
        if let Some(ed) = self.projects.editor.as_mut() {
            ed.selected = Some(path.clone());
            ed.draft = None;
        }
        if !self.switch_project(Some(path.clone()))
            && let Some(m) = self.projects.message.clone()
        {
            self.file_status = Some(Err(format!(
                "{m}; {} was added to the list",
                path.display()
            )));
        }
    }

    /// The active project's drop-down (Home, Create New File).
    pub(super) fn project_combo(&mut self, ui: &mut egui::Ui, id: &str) {
        self.projects_load();
        let active = self.active_project().clone();
        let mut pick: Option<Option<PathBuf>> = None;
        let mut browse = None;
        let tip = match &active.file {
            None => format!(
                "Default (built in), workspace {}",
                active.workspace().display()
            ),
            Some(f) => format!("Single user project {}", f.display()),
        };
        egui::ComboBox::from_id_salt(id)
            .width(200.0)
            .selected_text(active.name().to_owned())
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(active.is_default(), "Default")
                    .on_hover_text("Built-in project: workspace Documents\\3DCadCraft")
                    .clicked()
                {
                    pick = Some(None);
                }
                for f in self.projects.list.known.clone() {
                    let exists = f.is_file();
                    let name = f
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let text = if exists {
                        RichText::new(name)
                    } else {
                        RichText::new(format!("{name} (not found)")).weak()
                    };
                    let on = active.file.as_ref() == Some(&f);
                    let r = ui.add_enabled(exists, egui::Button::selectable(on, text));
                    if r.on_hover_text(f.display().to_string()).clicked() {
                        pick = Some(Some(f.clone()));
                    }
                }
                ui.separator();
                if ui
                    .button("New Project...")
                    .on_hover_text("A new project file (.ccproj); its folder is the workspace")
                    .clicked()
                {
                    browse = Some(DialogPurpose::NewProject);
                }
                if ui
                    .button("Browse...")
                    .on_hover_text("Add a project file (.ccproj) to the list and make it active")
                    .clicked()
                {
                    browse = Some(DialogPurpose::OpenProject);
                }
            })
            .response
            .on_hover_text(tip);
        if let Some(to) = pick {
            self.switch_project(to);
        }
        if let Some(p) = browse {
            self.open_dialog(p);
        }
    }

    /// A new file from the template at `path` (`None`: the standard template
    /// of `kind`) in a tab of its own (spec 0.6c §3.3).
    pub(super) fn new_from_template(&mut self, path: Option<&Path>, kind: DocumentKind) {
        let made = match path {
            Some(p) => templates::instantiate(p),
            None => {
                let dir = self.templates_dir();
                templates::standard_document(dir.as_deref(), kind)
            }
        };
        match made {
            Ok(n) => self.new_file_from(n, path),
            Err(e) => self.file_status = Some(Err(format!("New: {e}"))),
        }
    }

    /// The standard template's document of `kind` (Manage › Documents' New
    /// in this file), or the built-in one when it cannot be read.
    pub(super) fn standard_new_document(&mut self, kind: DocumentKind) -> NewDocument {
        let dir = self.templates_dir();
        templates::standard_document(dir.as_deref(), kind).unwrap_or_else(|e| {
            log::warn!("standard template: {e}");
            NewDocument {
                data: templates::builtin(kind),
                properties: crate::document::iprops::IProperties::new_document(),
            }
        })
    }

    /// A new, unsaved design file holding the document `n`.
    fn new_file_from(&mut self, n: NewDocument, from: Option<&Path>) {
        let kind = n.data.kind();
        self.add_blank_tab();
        let mut project = Project::new("Untitled");
        let id = project.add_new_document(templates::document_base_name(kind), n.data);
        if let Some(d) = project.document_mut(id) {
            d.properties = n.properties;
        }
        // A template's part has no display triangles: build it once.
        project.rebuild_unmeshed_parts();
        self.project = project;
        self.part_id = id;
        self.reset_asm_ui();
        self.cons = Default::default();
        self.file_path.clear();
        self.history.clear();
        self.editing = None;
        self.selection = super::Selection::Body;
        match kind {
            DocumentKind::Drawing => {
                self.drw.reset();
                self.tab = super::ribbon::Tab::PlaceViews;
            }
            DocumentKind::Assembly => self.tab = super::ribbon::Tab::Assemble,
            DocumentKind::Part => self.tab = super::ribbon::Tab::Model,
        }
        self.after_restore();
        self.fit_view();
        self.mark_saved();
        let from = from
            .and_then(Path::file_name)
            .map(|f| format!(" from {}", f.to_string_lossy()))
            .unwrap_or_default();
        let next = match kind {
            DocumentKind::Part => "Sketch > Start 2D Sketch (S) begins the model",
            DocumentKind::Assembly => "Assemble > Place (P) picks a part file to place",
            DocumentKind::Drawing => "Place Views > Base View (BV) picks a model file",
        };
        self.file_status = Some(Ok(format!(
            "New {}{from}: {next}",
            FileType::for_kind(kind).name().to_lowercase()
        )));
    }

    /// File › Save Copy As Template: the Save dialog in the templates folder.
    pub(super) fn start_save_template(&mut self) {
        match self.project.document(self.part_id).map(|d| d.data.kind()) {
            Some(k) => self.open_dialog(DialogPurpose::SaveTemplate(k)),
            None => {
                self.file_status = Some(Err("Save Copy As Template: no document is active".into()))
            }
        }
    }

    /// Writes the active document as a template to `path`.
    pub(super) fn save_template_to(&mut self, path: &Path) {
        self.file_status = Some(
            templates::save_as_template(&self.project, self.part_id, path)
                .map(|()| format!("Template saved: {} (File > New lists it)", path.display()))
                .map_err(|e| format!("Save Copy As Template: {e}")),
        );
        self.projects.tree = None;
    }

    /// Home's `...` → Settings, File › Projects..., Create New File's
    /// Projects...: the Projects editor.
    pub(super) fn open_projects_editor(&mut self) {
        self.projects_load();
        self.projects.editor = Some(Editor {
            selected: self.projects.list.active.clone(),
            ..Editor::default()
        });
    }

    /// The Projects editor window: the projects on top, the selected one's
    /// settings below (spec 0.6a §3.2).
    pub(super) fn projects_editor_window(&mut self, ctx: &egui::Context) {
        let Some(mut ed) = self.projects.editor.take() else {
            return;
        };
        let mut open = true;
        let mut close = false;
        let mut activate: Option<Option<PathBuf>> = None;
        let mut browse = None;
        let mut remove = None;
        let mut saved: Option<(PathBuf, DesignProject)> = None;
        let docs = super::files::documents_dir();
        // The selected project's settings.
        if ed.draft.is_none() {
            match &ed.selected {
                None => {
                    ed.draft = Some(DesignProject::default_project(&docs));
                    ed.error = None;
                }
                Some(f) => match DesignProject::load(f) {
                    Ok(p) => {
                        ed.draft = Some(p);
                        ed.error = None;
                    }
                    Err(e) => ed.error = Some(e.to_string()),
                },
            }
            ed.changed = false;
        }
        let active = self.active_project().file.clone();
        let known = self.projects.list.known.clone();
        let win = egui::Window::new("Projects")
            .open(&mut open)
            .resizable(true)
            .collapsible(false)
            .default_size([620.0, 520.0])
            .max_width(900.0);
        self.layout
            .place("projects", ctx.content_rect(), true, win)
            .show(ctx, |ui| {
                egui::Panel::bottom(egui::Id::new("projects_buttons")).show(ui, |ui| {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if ui
                            .button("New")
                            .on_hover_text("A new project file (.ccproj)")
                            .clicked()
                        {
                            browse = Some(DialogPurpose::NewProject);
                        }
                        if ui
                            .button("Browse...")
                            .on_hover_text("Add a project file to the list")
                            .clicked()
                        {
                            browse = Some(DialogPurpose::OpenProject);
                        }
                        let can_remove = ed.selected.is_some();
                        if ui
                            .add_enabled(can_remove, egui::Button::new("Remove"))
                            .on_hover_text("Take the project off the list (its file stays)")
                            .clicked()
                        {
                            remove = ed.selected.clone();
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Done").clicked() {
                                close = true;
                            }
                            let can_save = ed.changed && ed.selected.is_some();
                            if ui
                                .add_enabled(can_save, egui::Button::new("Save"))
                                .on_hover_text("Write the settings into the project file")
                                .clicked()
                                && let (Some(f), Some(d)) = (&ed.selected, &ed.draft)
                            {
                                match d.save(f) {
                                    Ok(()) => {
                                        ed.changed = false;
                                        ed.error = None;
                                        saved = Some((f.clone(), d.clone()));
                                    }
                                    Err(e) => ed.error = Some(e.to_string()),
                                }
                            }
                        });
                    });
                });
                // The list of projects.
                ui.label(RichText::new("Projects").strong());
                egui::Grid::new("projects_list")
                    .num_columns(3)
                    .striped(true)
                    .show(ui, |ui| {
                        ui.weak("");
                        ui.weak("Project Name");
                        ui.weak("Project Location");
                        ui.end_row();
                        let mut row = |ui: &mut egui::Ui,
                                       f: Option<&PathBuf>,
                                       name: String,
                                       location: String,
                                       exists: bool| {
                            let is_active = active.as_ref() == f;
                            ui.label(if is_active { "✔" } else { "" });
                            let sel = ed.selected.as_ref() == f;
                            let text = if exists {
                                RichText::new(name)
                            } else {
                                RichText::new(format!("{name} (not found)")).weak()
                            };
                            let r = ui.selectable_label(sel, text).on_hover_text(
                                "Click shows its settings; double-click makes it the active project",
                            );
                            if r.clicked() && !sel {
                                ed.selected = f.cloned();
                                ed.draft = None;
                            }
                            if r.double_clicked() && exists {
                                activate = Some(f.cloned());
                            }
                            ui.add(egui::Label::new(RichText::new(location).weak()).truncate());
                            ui.end_row();
                        };
                        row(
                            ui,
                            None,
                            "Default".into(),
                            "(built in)".into(),
                            true,
                        );
                        for f in &known {
                            let name = f
                                .file_stem()
                                .map(|s| s.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            let loc = f
                                .parent()
                                .map(|p| p.display().to_string())
                                .unwrap_or_default();
                            row(ui, Some(f), name, loc, f.is_file());
                        }
                    });
                ui.horizontal(|ui| {
                    let sel_active = ed.selected == active;
                    if ui
                        .add_enabled(!sel_active, egui::Button::new("Make Active"))
                        .on_hover_text("Only with no changed documents open; open ones are closed")
                        .clicked()
                    {
                        activate = Some(ed.selected.clone());
                    }
                    if let Some(m) = &self.projects.message {
                        ui.label(RichText::new(m).color(super::theme::tokens().warning));
                    }
                });
                ui.separator();
                // The selected project's settings.
                if let Some(e) = &ed.error {
                    ui.label(RichText::new(e).color(super::theme::tokens().error));
                }
                let base = ed
                    .selected
                    .as_deref()
                    .and_then(Path::parent)
                    .map(Path::to_path_buf);
                let editable = ed.selected.is_some();
                if let Some(d) = ed.draft.as_mut() {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.add_enabled_ui(editable, |ui| {
                            if project_settings(ui, d, base.as_deref(), ed.selected.as_deref()) {
                                ed.changed = true;
                            }
                        });
                        if !editable {
                            ui.weak("The Default project is built in: its settings cannot be changed.");
                        }
                    });
                }
            });
        // The active project's new settings take effect at once.
        if let Some((f, d)) = saved
            && let Some(a) = self.projects.active.as_mut()
            && a.file.as_ref() == Some(&f)
        {
            a.project = d;
            self.projects.tree = None;
            self.projects.new_folder = None;
            self.projects.new_details = None;
        }
        if let Some(p) = browse {
            self.projects.editor = Some(ed);
            self.open_dialog(p);
            return;
        }
        if let Some(f) = remove {
            self.projects.list.remove(&f);
            write_list(&self.projects.list);
            if active.as_ref() == Some(&f) {
                self.switch_project(None);
            }
            ed.selected = self.projects.list.active.clone();
            ed.draft = None;
        }
        if let Some(to) = activate
            && self.switch_project(to.clone())
        {
            ed.selected = to;
            ed.draft = None;
        }
        if open && !close {
            self.projects.editor = Some(ed);
        } else {
            self.projects.message = None;
        }
    }
}

/// The settings tree of one project; true when something changed.
fn project_settings(
    ui: &mut egui::Ui,
    d: &mut DesignProject,
    base: Option<&Path>,
    file: Option<&Path>,
) -> bool {
    let mut changed = false;
    let abs = |p: &Path| match base {
        Some(b) => DesignProject::resolve(b, p),
        None => p.to_path_buf(),
    };
    egui::CollapsingHeader::new("Project")
        .default_open(true)
        .show(ui, |ui| {
            egui::Grid::new("project_main")
                .num_columns(2)
                .show(ui, |ui| {
                    ui.label("Type");
                    ui.label("Single User")
                        .on_hover_text("Vault projects are not planned yet");
                    ui.end_row();
                    ui.label("Name");
                    changed |= ui.text_edit_singleline(&mut d.name).changed();
                    ui.end_row();
                    ui.label("Location");
                    ui.add(
                        egui::Label::new(
                            RichText::new(
                                file.and_then(Path::parent)
                                    .map(|p| p.display().to_string())
                                    .unwrap_or_else(|| "(built in)".into()),
                            )
                            .weak(),
                        )
                        .truncate(),
                    );
                    ui.end_row();
                    ui.label("Owner");
                    changed |= ui.text_edit_singleline(&mut d.owner).changed();
                    ui.end_row();
                });
        });
    egui::CollapsingHeader::new("Workspace")
        .default_open(true)
        .show(ui, |ui| {
            let mut ws = d.workspace.display().to_string();
            ui.horizontal(|ui| {
                if ui
                    .add(egui::TextEdit::singleline(&mut ws).desired_width(320.0))
                    .on_hover_text(
                        "Where files are edited and saved; relative to the project file's folder",
                    )
                    .changed()
                {
                    d.workspace = PathBuf::from(&ws);
                    changed = true;
                }
                let at = abs(&d.workspace);
                if !at.is_dir() {
                    ui.label(RichText::new("missing").color(super::theme::tokens().warning))
                        .on_hover_text(format!("{} does not exist yet", at.display()));
                }
            });
        });
    for (title, list, what) in [
        (
            "Libraries",
            &mut d.libraries,
            "read-only folders of standard parts, searched after the workspace",
        ),
        (
            "Frequently Used Subfolders",
            &mut d.shortcuts,
            "shortcuts of the file dialogs",
        ),
    ] {
        egui::CollapsingHeader::new(title)
            .default_open(true)
            .show(ui, |ui| {
                changed |= named_paths(ui, title, list, what);
            });
    }
    egui::CollapsingHeader::new("Folder Options")
        .default_open(true)
        .show(ui, |ui| {
            egui::Grid::new("project_folders").num_columns(2).show(ui, |ui| {
                for (label, slot, tip) in [
                    ("Templates", &mut d.templates, "Create New File lists the templates of this folder; empty = the default templates folder"),
                    ("Design Data (styles)", &mut d.styles, "styles folder; empty = the default"),
                    ("Component Library Files", &mut d.content_center, "Component Library folder; empty = the default"),
                ] {
                    ui.label(label);
                    let mut text = slot
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default();
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut text)
                                .hint_text("[Default]")
                                .desired_width(320.0),
                        )
                        .on_hover_text(tip)
                        .changed()
                    {
                        *slot = (!text.trim().is_empty()).then(|| PathBuf::from(text.trim()));
                        changed = true;
                    }
                    ui.end_row();
                }
            });
        });
    egui::CollapsingHeader::new("Options")
        .default_open(true)
        .show(ui, |ui| {
            egui::Grid::new("project_options").num_columns(2).show(ui, |ui| {
                ui.label("Old Versions To Keep On Save");
                let mut n = d.old_versions as f64;
                if ui
                    .add(super::valuefield::ExprValue::new(&mut n).range(-1.0..=100.0))
                    .on_hover_text("Copies kept in an OldVersions folder on save; -1 keeps all (not applied yet: one .bak is kept)")
                    .changed()
                {
                    d.old_versions = n.round() as i32;
                    changed = true;
                }
                ui.end_row();
                ui.label("Using Unique File Names");
                changed |= ui
                    .checkbox(&mut d.unique_names, "")
                    .on_hover_text("A reference not at its stored place is looked up by name in the whole project (not applied yet)")
                    .changed();
                ui.end_row();
            });
        });
    changed
}

/// An editable list of named folders.
fn named_paths(ui: &mut egui::Ui, id: &str, list: &mut Vec<NamedPath>, what: &str) -> bool {
    let mut changed = false;
    let mut drop = None;
    egui::Grid::new(("named_paths", id))
        .num_columns(3)
        .show(ui, |ui| {
            for (k, np) in list.iter_mut().enumerate() {
                changed |= ui
                    .add(egui::TextEdit::singleline(&mut np.name).desired_width(120.0))
                    .changed();
                let mut p = np.path.display().to_string();
                if ui
                    .add(egui::TextEdit::singleline(&mut p).desired_width(260.0))
                    .changed()
                {
                    np.path = PathBuf::from(p);
                    changed = true;
                }
                if super::theme::glyph(ui, super::theme::Glyph::Close)
                    .on_hover_text("Remove")
                    .clicked()
                {
                    drop = Some(k);
                }
                ui.end_row();
            }
        });
    if let Some(k) = drop {
        list.remove(k);
        changed = true;
    }
    if ui
        .button("Add")
        .on_hover_text(format!("Add a folder: {what}"))
        .clicked()
    {
        list.push(NamedPath {
            name: format!("{id} {}", list.len() + 1),
            path: PathBuf::from("."),
        });
        changed = true;
    }
    changed
}
