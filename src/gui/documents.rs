//! Manage › Documents (`docs/spec/0.6b-multi-document-files.md`): the
//! documents of the open design file — New Part / Assembly / Drawing *in
//! this file*, Duplicate, Rename, Delete, the Documents window
//! and the storage commands (Save to Separate File, Embed, Embed All,
//! Separate All; `document::storage`). Every change is one Undo step of
//! the file; files written on disk stay.

use eframe::egui::{self, Color32, RichText};

use super::CadCraftApp;
use std::path::{Path, PathBuf};

use crate::document::documents::base_name;
use crate::document::storage;
use crate::document::{
    AssemblyDocument, DocumentData, DocumentId, DocumentKind, LibraryId, Project, ProjectResult,
    Resolver,
};
use crate::part::PartDocument;

/// A command of the Documents panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocCmd {
    /// New Part in this file.
    NewPart,
    /// New Assembly in this file.
    NewAssembly,
    /// New Drawing in this file.
    NewDrawing,
    /// New Presentation in this file (presentations are still to come).
    NewPresentation,
    /// Copy every document of another file into this one.
    Insert,
    /// Link another file (Place and Base View offer its documents).
    Link,
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
            DocCmd::NewAssembly => "New Assembly",
            DocCmd::NewDrawing => "New Drawing",
            DocCmd::NewPresentation => "New Presentation",
            DocCmd::Insert => "Insert Document",
            DocCmd::Link => "Link File",
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

    /// Waits for a later step (presentations are still to come).
    pub(super) fn planned(self) -> bool {
        matches!(self, DocCmd::NewPresentation)
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
    /// The linked file picked in the Documents window (Embed).
    selected_link: Option<LibraryId>,
    /// Application Options › Save › New documents: each new document in a
    /// file of its own instead of the open design file (spec 0.6b §3.6).
    pub separate: bool,
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
        if self.docs.separate && self.docs_new_file(c) {
            return;
        }
        match c {
            DocCmd::NewPart => self.docs_new(c, DocumentData::Part(PartDocument::default())),
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
            DocCmd::Extract => self.docs_extract(id),
            DocCmd::Insert => self.open_dialog(super::files::DialogPurpose::InsertDocument),
            DocCmd::Link => {
                if self.saved_path(c).is_some() {
                    self.open_dialog(super::files::DialogPurpose::LinkFile);
                }
            }
            DocCmd::Embed => self.docs_embed(),
            DocCmd::Pack => self.docs_pack(),
            DocCmd::Unpack => self.docs_unpack(),
            DocCmd::NewPresentation => {}
        }
    }

    /// File → New Assembly / New Drawing: into the open design file, unless
    /// New documents is *As separate files* or the open file is an
    /// untouched blank one (then a new file, as before).
    pub(super) fn file_new(&mut self, c: DocCmd) {
        if self.docs.separate || self.active_is_blank() {
            self.docs_new_file(c);
        } else {
            self.docs_apply_to(c, self.part_id);
        }
    }

    /// A new document of `c`'s kind in a new file (tab); false when `c`
    /// is not a New command.
    fn docs_new_file(&mut self, c: DocCmd) -> bool {
        use super::new_file::Kind;
        let kind = match c {
            DocCmd::NewPart => Kind::Part,
            DocCmd::NewAssembly => Kind::Assembly,
            DocCmd::NewDrawing => {
                self.new_drawing_file();
                return true;
            }
            _ => return false,
        };
        self.new_file_of(kind, 0.0);
        true
    }

    /// A new file (tab) holding one empty drawing.
    fn new_drawing_file(&mut self) {
        self.add_blank_tab();
        let mut project = Project::new("Drawing1");
        let id = project.add_new_document(
            "Drawing",
            DocumentData::Drawing(crate::drawing::DrawingDocument::default()),
        );
        self.project = project;
        self.part_id = id;
        self.asm = Default::default();
        self.cons = Default::default();
        self.file_path.clear();
        self.history.clear();
        self.editing = None;
        self.selection = super::Selection::Body;
        self.drw.reset();
        self.tab = super::ribbon::Tab::PlaceViews;
        self.after_restore();
        self.fit_view();
        self.mark_saved();
        self.file_status = Some(Ok(
            "New drawing: Place Views > Base View (BV) picks a model file".into(),
        ));
    }

    /// The saved design file, or a message that it must be saved first
    /// (links are paths relative to it).
    fn saved_path(&mut self, c: DocCmd) -> Option<PathBuf> {
        let p = self.file_path.trim();
        if p.is_empty() {
            self.file_status = Some(Err(format!(
                "{}: save this file first (separate files are linked by a path relative to it)",
                c.name()
            )));
            return None;
        }
        Some(PathBuf::from(p))
    }

    /// Save to Separate File: the Save dialog (in the design file's folder,
    /// `<name>.<kind ext>` proposed), then `docs_extract_to` (spec 0.6b §3.5).
    fn docs_extract(&mut self, id: DocumentId) {
        let c = DocCmd::Extract;
        if self.saved_path(c).is_none() {
            return;
        }
        let Some(kind) = self.project.document(id).map(|d| d.data.kind()) else {
            return;
        };
        if self.project.documents().len() <= 1 {
            self.file_status = Some(Err(format!(
                "{}: a file keeps at least one document",
                c.name()
            )));
            return;
        }
        self.open_dialog(super::files::DialogPurpose::Extract(id, kind));
    }

    /// Writes document `id` to `target` and links it back.
    pub(super) fn docs_extract_to(&mut self, id: DocumentId, target: &Path) {
        let c = DocCmd::Extract;
        let Some(owner) = self.saved_path(c) else {
            return;
        };
        let Some(doc) = self.project.document(id).cloned() else {
            return;
        };
        let target = target.to_path_buf();
        // The dialog asked before an existing file got here: replace it
        // (storage refuses another kind and a file this one links).
        let replace = target.exists();
        // The active document gives way first: it leaves this file.
        let back = self.part_id;
        if id == self.part_id
            && let Some(o) = self
                .project
                .documents()
                .iter()
                .map(|d| d.id)
                .find(|&d| d != id)
        {
            self.activate_document(o);
        }
        let t = target.clone();
        if self
            .docs_edit(c, |p| storage::extract(p, &owner, id, &t, replace))
            .is_some()
        {
            self.after_restore();
            self.file_status = Some(Ok(format!(
                "{}: {} is now {} (linked; save this file to keep the link)",
                c.name(),
                doc.name,
                target.display()
            )));
        } else if self.project.document(back).is_some() && back != self.part_id {
            self.activate_document(back);
        }
    }

    /// Link File: `path` is linked; Place and Base View offer its documents.
    pub(super) fn docs_link_file(&mut self, path: &Path) {
        let c = DocCmd::Link;
        let Some(owner) = self.saved_path(c) else {
            return;
        };
        let p = path.to_path_buf();
        if self
            .docs_edit(c, |pr| {
                storage::link_file(pr, &owner, &mut Resolver::new(), &p)
            })
            .is_some()
        {
            self.docs.window = true;
            self.file_status = Some(Ok(format!(
                "{}: {} is linked (Place offers its parts; Embed copies them in)",
                c.name(),
                path.display()
            )));
        }
    }

    /// Insert Document: every document of `path` is copied into this file.
    pub(super) fn docs_insert_file(&mut self, path: &Path) {
        let c = DocCmd::Insert;
        let owner = {
            let p = self.file_path.trim();
            (!p.is_empty()).then(|| PathBuf::from(p))
        };
        let p = path.to_path_buf();
        if let Some(added) = self.docs_edit(c, |pr| {
            storage::insert_file(pr, owner.as_deref(), &mut Resolver::new(), &p)
        }) {
            let names: Vec<String> = added.iter().map(|&d| self.doc_name(d)).collect();
            self.docs.window = true;
            self.file_status = Some(Ok(format!("{}: {}", c.name(), names.join(", "))));
        }
    }

    /// Embed: the documents used from the linked file picked in the
    /// Documents window (or the only linked file) come into this file.
    fn docs_embed(&mut self) {
        let c = DocCmd::Embed;
        let links: Vec<LibraryId> = self.project.libraries().iter().map(|l| l.id).collect();
        let lib = self
            .docs
            .selected_link
            .filter(|l| links.contains(l))
            .or_else(|| (links.len() == 1).then(|| links[0]));
        let Some(lib) = lib else {
            self.docs.window = true;
            self.file_status = Some(Err(format!(
                "{}: pick a linked file in the Documents window",
                c.name()
            )));
            return;
        };
        let Some(owner) = self.saved_path(c) else {
            return;
        };
        let docs: Vec<DocumentId> = self
            .project
            .documents()
            .iter()
            .flat_map(|d| d.data.references())
            .filter(|r| r.library == Some(lib))
            .map(|r| r.document)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let n = docs.len();
        if n == 0 {
            self.file_status = Some(Ok(format!(
                "{}: nothing is used from that file yet (Place a part of it first)",
                c.name()
            )));
            return;
        }
        let done = self.docs_edit(c, |p| {
            let mut res = Resolver::new();
            for d in docs {
                storage::embed(p, &owner, &mut res, lib, d)?;
            }
            Ok(())
        });
        if done.is_some() {
            self.docs.selected_link = None;
            self.file_status = Some(Ok(format!(
                "{}: {n} document(s) copied into this file",
                c.name()
            )));
        }
    }

    /// Embed All (Pack): every linked document comes into this file.
    fn docs_pack(&mut self) {
        let c = DocCmd::Pack;
        if self.project.libraries().is_empty() {
            self.file_status = Some(Ok(format!("{}: this file links no other file", c.name())));
            return;
        }
        let Some(owner) = self.saved_path(c) else {
            return;
        };
        if let Some(n) = self.docs_edit(c, |p| storage::embed_all(p, &owner, &mut Resolver::new()))
        {
            self.file_status = Some(Ok(format!(
                "{}: {n} document(s) copied in; this file stands alone",
                c.name()
            )));
        }
    }

    /// Separate All (Unpack): every document but the active one to its own
    /// file next to this one.
    fn docs_unpack(&mut self) {
        let c = DocCmd::Unpack;
        let Some(owner) = self.saved_path(c) else {
            return;
        };
        let keep = self.part_id;
        if let Some(files) = self.docs_edit(c, |p| storage::separate_all(p, &owner, keep)) {
            self.after_restore();
            self.file_status = Some(Ok(format!(
                "{}: {} file(s) written next to this one and linked; save this file to keep the links",
                c.name(),
                files.len()
            )));
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

    /// New Part / Assembly in this file: added with the next
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
        let links: Vec<(LibraryId, String, String, usize)> = self
            .project
            .libraries()
            .iter()
            .map(|l| {
                let used = self
                    .project
                    .documents()
                    .iter()
                    .flat_map(|d| d.data.references())
                    .filter(|r| r.library == Some(l.id))
                    .map(|r| r.document)
                    .collect::<std::collections::HashSet<_>>()
                    .len();
                (l.id, l.name.clone(), l.path.clone(), used)
            })
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
                                    self.docs.selected_link = None;
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
                        for (lib, name, path, used) in &links {
                            ui.horizontal(|ui| {
                                super::panels::tree_icon(ui, super::icons::Icon::DocLink);
                                let on = self.docs.selected_link == Some(*lib);
                                let r = ui
                                    .selectable_label(on, name)
                                    .on_hover_text("A linked file: Embed copies what is used from it into this file");
                                if r.clicked() {
                                    self.docs.selected_link = Some(*lib);
                                    self.docs.selected = None;
                                }
                                r.context_menu(|ui| {
                                    if ui.button(DocCmd::Embed.name()).clicked() {
                                        self.docs.selected_link = Some(*lib);
                                        ask = Some((DocCmd::Embed, active));
                                        ui.close();
                                    }
                                });
                            });
                            ui.label("Linked file");
                            ui.label(path);
                            ui.label(format!("{used} document(s) used here"));
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
                    if ui
                        .add_enabled(sel.is_some(), egui::Button::new(DocCmd::Extract.name()))
                        .on_hover_text("Move the picked document into its own file next to this one, linked back")
                        .clicked()
                        && let Some(s) = sel
                    {
                        ask = Some((DocCmd::Extract, s));
                    }
                    if ui
                        .add_enabled(self.docs.selected_link.is_some(), egui::Button::new(DocCmd::Embed.name()))
                        .on_hover_text("Copy what is used from the picked linked file into this file")
                        .clicked()
                    {
                        ask = Some((DocCmd::Embed, active));
                    }
                    if ui
                        .add_enabled(!links.is_empty(), egui::Button::new(DocCmd::Pack.name()))
                        .on_hover_text("Every linked document into this file, so it stands alone")
                        .clicked()
                    {
                        ask = Some((DocCmd::Pack, active));
                    }
                    if ui
                        .add_enabled(rows.len() > 1, egui::Button::new(DocCmd::Unpack.name()))
                        .on_hover_text("Every document but the active one into its own file, linked back")
                        .clicked()
                    {
                        ask = Some((DocCmd::Unpack, active));
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

impl CadCraftApp {
    /// Document chips after the active file's tab: one per document of the
    /// file when it holds several (spec 0.6b §3.3); a click makes that
    /// document the active one, sharing the file's Undo and Save.
    pub(super) fn document_chips(&mut self, ui: &mut egui::Ui) {
        if self.project.documents().len() < 2 {
            return;
        }
        let docs: Vec<(DocumentId, String, DocumentKind)> = self
            .project
            .documents()
            .iter()
            .map(|d| (d.id, d.name.clone(), d.data.kind()))
            .collect();
        let mut open = None;
        let mut ask = None;
        for (id, name, kind) in docs {
            super::panels::tree_icon(ui, kind_icon(kind));
            let on = id == self.part_id;
            let text = if on {
                RichText::new(&name).strong()
            } else {
                RichText::new(&name)
            };
            let r = ui
                .selectable_label(on, text)
                .on_hover_text("A document of this file: click to work on it");
            if r.clicked() && !on {
                open = Some(id);
            }
            r.context_menu(|ui| doc_menu(ui, id, &mut ask, &mut open));
        }
        if let Some(id) = open {
            self.asm.came_from = None;
            self.activate_document(id);
        }
        if let Some((c, id)) = ask {
            self.docs_apply_to(c, id);
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
    if ui
        .button(DocCmd::Extract.name())
        .on_hover_text("Move it into its own file next to this one, linked back")
        .clicked()
    {
        *ask = Some((DocCmd::Extract, id));
        ui.close();
    }
}

/// The menu of the file's top node: new documents in this file.
pub(super) fn new_in_file_menu(ui: &mut egui::Ui, ask: &mut Option<DocCmd>) {
    for (c, label) in [
        (DocCmd::NewPart, "New Part in This File"),
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
