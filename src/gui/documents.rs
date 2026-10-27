//! Manage › Documents (`docs/spec/0.6b-multi-document-files.md`): the
//! documents of the open design file — New Part / Sheet Metal / Assembly /
//! Drawing *in this file*, Duplicate, Rename, Delete, the Documents window
//! and the storage commands (Save to Separate File, Embed, Pack, Unpack;
//! shown disabled until D3). Every change is one Undo step of the file.

use eframe::egui::{self, Color32, RichText};

use super::CadCraftApp;
use crate::document::documents::base_name;
use crate::document::{
    AssemblyDocument, DocumentData, DocumentId, DocumentKind, Project, ProjectResult,
};
use crate::part::PartDocument;

/// A command of the Documents panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocCmd {
    /// New Part in this file.
    NewPart,
    /// New Sheet Metal part in this file.
    NewSheetMetal,
    /// New Assembly in this file.
    NewAssembly,
    /// New Drawing in this file.
    NewDrawing,
    /// New Presentation in this file (presentations are still to come).
    NewPresentation,
    /// A copy of the document in this file.
    Duplicate,
    /// Rename the document.
    Rename,
    /// Delete the document (refused while something uses it).
    Delete,
    /// The Documents window.
    Window,
    /// Save to Separate File.
    Extract,
    /// Embed a linked document.
    Embed,
    /// Embed every linked document.
    Pack,
    /// Every internal document to its own file.
    Unpack,
}

impl DocCmd {
    /// The command's name (status messages).
    fn name(self) -> &'static str {
        match self {
            DocCmd::NewPart => "New Part",
            DocCmd::NewSheetMetal => "New Sheet Metal",
            DocCmd::NewAssembly => "New Assembly",
            DocCmd::NewDrawing => "New Drawing",
            DocCmd::NewPresentation => "New Presentation",
            DocCmd::Duplicate => "Duplicate Document",
            DocCmd::Rename => "Rename Document",
            DocCmd::Delete => "Delete Document",
            DocCmd::Window => "Documents",
            DocCmd::Extract => "Save to Separate File",
            DocCmd::Embed => "Embed",
            DocCmd::Pack => "Embed All",
            DocCmd::Unpack => "Separate All",
        }
    }

    /// Waits for a later step (spec 0.6b §6: D2–D3, presentations).
    pub(super) fn planned(self) -> bool {
        matches!(
            self,
            DocCmd::NewPresentation
                | DocCmd::Extract
                | DocCmd::Embed
                | DocCmd::Pack
                | DocCmd::Unpack
        )
    }
}

/// State of the Documents window and the Rename dialog.
#[derive(Debug, Default)]
pub struct DocsUi {
    /// The Documents window is open.
    pub window: bool,
    /// The row picked in the Documents window.
    selected: Option<DocumentId>,
    /// Rename Document: the document, the typed name, focus requested.
    rename: Option<(DocumentId, String, bool)>,
}

/// The kind's icon in lists.
pub(super) fn kind_icon(kind: DocumentKind) -> super::icons::Icon {
    use super::icons::Icon;
    match kind {
        DocumentKind::Part => Icon::Part,
        DocumentKind::Assembly => Icon::Assembly,
        DocumentKind::Drawing => Icon::Drawing,
    }
}

impl CadCraftApp {
    /// The document a Documents command acts on: the one picked in the
    /// Documents window or the browser, else the active one.
    fn doc_target(&self) -> DocumentId {
        match self.selection {
            super::Selection::Document(id) if self.project.document(id).is_some() => id,
            _ => self
                .docs
                .selected
                .filter(|_| self.docs.window)
                .unwrap_or(self.part_id),
        }
    }

    /// Runs a Documents command on the default target.
    pub(super) fn docs_apply(&mut self, c: DocCmd) {
        let target = self.doc_target();
        self.docs_apply_to(c, target);
    }

    /// Runs a Documents command on document `id` (browser rows, the window).
    pub(super) fn docs_apply_to(&mut self, c: DocCmd, id: DocumentId) {
        if c.planned() {
            self.file_status = Some(Err(format!(
                "{}: not yet (spec 0.6b, Manage › Documents)",
                c.name()
            )));
            return;
        }
        match c {
            DocCmd::NewPart => self.docs_new(c, DocumentData::Part(PartDocument::default())),
            DocCmd::NewSheetMetal => {
                let mut part = PartDocument::default();
                part.make_sheet_metal(None);
                self.docs_new(c, DocumentData::Part(part));
                self.tab = super::ribbon::Tab::SheetMetal;
            }
            DocCmd::NewAssembly => {
                self.docs_new(c, DocumentData::Assembly(AssemblyDocument::default()));
            }
            DocCmd::NewDrawing => self.new_drawing(),
            DocCmd::Duplicate => {
                if let Some(new) = self.docs_edit(c, |p| p.duplicate_document(id)) {
                    let name = self.doc_name(new);
                    self.selection = super::Selection::Document(new);
                    self.docs.selected = Some(new);
                    self.file_status = Some(Ok(format!("{}: {name}", c.name())));
                }
            }
            DocCmd::Rename => {
                let name = self.doc_name(id);
                self.docs.rename = Some((id, name, false));
            }
            DocCmd::Delete => self.docs_delete(id),
            DocCmd::Window => self.docs.window = !self.docs.window,
            _ => {}
        }
    }

    /// The name of document `id` (empty when it is gone).
    fn doc_name(&self, id: DocumentId) -> String {
        self.project
            .document(id)
            .map(|d| d.name.clone())
            .unwrap_or_default()
    }

    /// A change of the file's document list: tried on a copy, one Undo
    /// step when it works, `<Command>: <reason>` when it does not.
    fn docs_edit<T>(
        &mut self,
        c: DocCmd,
        f: impl FnOnce(&mut Project) -> ProjectResult<T>,
    ) -> Option<T> {
        let mut copy = self.project.clone();
        match f(&mut copy) {
            Ok(v) => {
                self.checkpoint(c.name());
                self.project = copy;
                self.scene_dirty = true;
                Some(v)
            }
            Err(e) => {
                self.file_status = Some(Err(format!("{}: {e}", c.name())));
                None
            }
        }
    }

    /// New Part / Sheet Metal / Assembly in this file: added with the next
    /// free name of its kind and made the active document.
    fn docs_new(&mut self, c: DocCmd, data: DocumentData) {
        let base = base_name(data.kind());
        if let Some(id) = self.docs_edit(c, |p| Ok(p.add_new_document(base, data))) {
            self.asm.came_from = None;
            self.activate_document(id);
            let name = self.doc_name(id);
            self.file_status = Some(Ok(format!(
                "{}: {name} in this file (Manage › Documents lists them)",
                c.name()
            )));
        }
    }

    /// Delete Document: refused while another document uses it (the
    /// message names them) and for the file's last document.
    fn docs_delete(&mut self, id: DocumentId) {
        let users: Vec<String> = self
            .project
            .users_of(id)
            .into_iter()
            .map(|u| self.doc_name(u))
            .collect();
        if !users.is_empty() {
            self.file_status = Some(Err(format!(
                "{}: used by {}",
                DocCmd::Delete.name(),
                users.join(", ")
            )));
            return;
        }
        let name = self.doc_name(id);
        // The active document gives way to another one first.
        let other = self
            .project
            .documents()
            .iter()
            .map(|d| d.id)
            .find(|&d| d != id);
        if id == self.part_id
            && self.project.documents().len() > 1
            && let Some(o) = other
        {
            self.activate_document(o);
        }
        if self
            .docs_edit(DocCmd::Delete, |p| p.delete_document(id))
            .is_some()
        {
            if self.docs.selected == Some(id) {
                self.docs.selected = None;
            }
            if self.selection == super::Selection::Document(id) {
                self.selection = super::Selection::Body;
            }
            self.after_restore();
            self.file_status = Some(Ok(format!("{}: {name}", DocCmd::Delete.name())));
        }
    }

    /// The Documents window and the Rename Document dialog.
    pub(super) fn documents_windows(&mut self, ctx: &egui::Context) {
        self.rename_document_window(ctx);
        if !self.docs.window {
            return;
        }
        let mut open = true;
        let mut ask: Option<(DocCmd, DocumentId)> = None;
        let mut activate = None;
        let rows: Vec<(DocumentId, String, DocumentKind, Vec<String>)> = self
            .project
            .documents()
            .iter()
            .map(|d| {
                let users = self
                    .project
                    .users_of(d.id)
                    .into_iter()
                    .map(|u| self.doc_name(u))
                    .collect();
                (d.id, d.name.clone(), d.data.kind(), users)
            })
            .collect();
        let links: Vec<(String, String)> = self
            .project
            .libraries()
            .iter()
            .map(|l| (l.name.clone(), l.path.clone()))
            .collect();
        let active = self.part_id;
        let file = if self.file_path.trim().is_empty() {
            "this file (not saved yet)".to_owned()
        } else {
            "in file".to_owned()
        };
        self.layout
            .place(
                "documents",
                ctx.content_rect(),
                true,
                egui::Window::new("Documents")
                    .open(&mut open)
                    .resizable(true)
                    .collapsible(false)
                    .default_size([560.0, 320.0]),
            )
            .show(ctx, |ui| {
                ui.label(
                    RichText::new("Documents stored in this design file and the files it links")
                        .weak(),
                );
                ui.separator();
                egui::Grid::new("documents_grid")
                    .striped(true)
                    .num_columns(4)
                    .show(ui, |ui| {
                        for h in ["Name", "Kind", "Storage", "Used by"] {
                            ui.label(RichText::new(h).strong());
                        }
                        ui.end_row();
                        for (id, name, kind, users) in &rows {
                            ui.horizontal(|ui| {
                                super::panels::tree_icon(ui, kind_icon(*kind));
                                let text = if *id == active {
                                    RichText::new(name).strong()
                                } else {
                                    RichText::new(name)
                                };
                                let r = ui
                                    .selectable_label(self.docs.selected == Some(*id), text)
                                    .on_hover_text("Double-click to open");
                                if r.double_clicked() {
                                    activate = Some(*id);
                                } else if r.clicked() {
                                    self.docs.selected = Some(*id);
                                }
                                r.context_menu(|ui| {
                                    doc_menu(ui, *id, &mut ask, &mut activate);
                                });
                            });
                            ui.label(format!("{kind:?}"));
                            ui.label(&file);
                            ui.label(if users.is_empty() {
                                "-".to_owned()
                            } else {
                                users.join(", ")
                            });
                            ui.end_row();
                        }
                        for (name, path) in &links {
                            ui.horizontal(|ui| {
                                super::panels::tree_icon(ui, super::icons::Icon::DocLink);
                                ui.label(name);
                            });
                            ui.label("Linked file");
                            ui.label(path);
                            ui.label("");
                            ui.end_row();
                        }
                    });
                ui.separator();
                let sel = self
                    .docs
                    .selected
                    .filter(|s| rows.iter().any(|r| r.0 == *s));
                ui.horizontal_wrapped(|ui| {
                    for (c, label) in [
                        (DocCmd::NewPart, "New Part"),
                        (DocCmd::NewAssembly, "New Assembly"),
                        (DocCmd::NewDrawing, "New Drawing"),
                    ] {
                        if ui.button(label).clicked() {
                            ask = Some((c, active));
                        }
                    }
                    ui.separator();
                    if ui
                        .add_enabled(sel.is_some(), egui::Button::new("Open"))
                        .clicked()
                    {
                        activate = sel;
                    }
                    for c in [DocCmd::Rename, DocCmd::Duplicate, DocCmd::Delete] {
                        if ui
                            .add_enabled(sel.is_some(), egui::Button::new(c.name()))
                            .clicked()
                            && let Some(s) = sel
                        {
                            ask = Some((c, s));
                        }
                    }
                    ui.separator();
                    for c in [DocCmd::Extract, DocCmd::Embed, DocCmd::Pack, DocCmd::Unpack] {
                        planned(ui.add_enabled(false, egui::Button::new(c.name())), c);
                    }
                });
            });
        if !open {
            self.docs.window = false;
        }
        if let Some(id) = activate {
            self.asm.came_from = None;
            self.activate_document(id);
        }
        if let Some((c, id)) = ask {
            self.docs_apply_to(c, id);
        }
    }

    /// Rename Document: a name field, OK / Cancel; Enter commits.
    fn rename_document_window(&mut self, ctx: &egui::Context) {
        let Some((id, mut text, focused)) = self.docs.rename.take() else {
            return;
        };
        let mut keep = true;
        let mut commit = false;
        let taken = self.project.name_taken(&text, Some(id));
        egui::Window::new("Rename Document")
            .collapsible(false)
            .resizable(false)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(240.0));
                if !focused {
                    r.request_focus();
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    commit = true;
                }
                if taken {
                    ui.label(
                        RichText::new("This name already exists in this file")
                            .color(Color32::from_rgb(230, 170, 60)),
                    );
                }
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(!taken && !text.trim().is_empty(), egui::Button::new("OK"))
                        .clicked()
                    {
                        commit = true;
                    }
                    if ui.button("Cancel").clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        keep = false;
                    }
                });
            });
        if commit {
            let name = text.clone();
            if self
                .docs_edit(DocCmd::Rename, |p| p.rename_document(id, &name))
                .is_some()
            {
                self.file_status = Some(Ok(format!("Rename Document: {name}")));
            }
            return;
        }
        if keep {
            self.docs.rename = Some((id, text, true));
        }
    }
}

/// The right-click menu of a document row (browser, Documents window).
pub(super) fn doc_menu(
    ui: &mut egui::Ui,
    id: DocumentId,
    ask: &mut Option<(DocCmd, DocumentId)>,
    open: &mut Option<DocumentId>,
) {
    if ui.button("Open").clicked() {
        *open = Some(id);
        ui.close();
    }
    for c in [DocCmd::Rename, DocCmd::Duplicate, DocCmd::Delete] {
        if ui.button(c.name()).clicked() {
            *ask = Some((c, id));
            ui.close();
        }
    }
    ui.separator();
    planned(
        ui.add_enabled(false, egui::Button::new(DocCmd::Extract.name())),
        DocCmd::Extract,
    );
}

/// The menu of the file's top node: new documents in this file.
pub(super) fn new_in_file_menu(ui: &mut egui::Ui, ask: &mut Option<DocCmd>) {
    for (c, label) in [
        (DocCmd::NewPart, "New Part in This File"),
        (DocCmd::NewSheetMetal, "New Sheet Metal in This File"),
        (DocCmd::NewAssembly, "New Assembly in This File"),
        (DocCmd::NewDrawing, "New Drawing in This File"),
    ] {
        if ui.button(label).clicked() {
            *ask = Some(c);
            ui.close();
        }
    }
    planned(
        ui.add_enabled(false, egui::Button::new("New Presentation in This File")),
        DocCmd::NewPresentation,
    );
    ui.separator();
    if ui.button("Documents...").clicked() {
        *ask = Some(DocCmd::Window);
        ui.close();
    }
}

/// A disabled control's tooltip: the spec it waits for.
fn planned(r: egui::Response, c: DocCmd) -> egui::Response {
    r.on_disabled_hover_text(format!(
        "{} — not yet (spec 0.6b-multi-document-files.md)",
        c.name()
    ))
}
