//! File › Pack and Go (spec `docs/spec/0.6d-pack-and-go.md`): the window over
//! `document::pack`. Walking the links, searching for referencing files and
//! copying run on a worker thread; the window only shows their results.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};

use eframe::egui::{self, RichText};

use super::CadCraftApp;
use super::files::DialogPurpose;
use crate::document::design_project::ActiveProject;
use crate::document::pack::{self, Found, Layout, PackOptions, Plan, Scope};

/// What a worker sends back.
enum Done {
    Collected(Vec<Found>),
    Referencing(Vec<PathBuf>),
    Packed(Result<usize, String>, PathBuf),
}

/// Where Search Now looks.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SearchIn {
    Project,
    Folder,
}

/// The open Pack and Go window.
pub(super) struct PackUi {
    source: String,
    destination: String,
    options: PackOptions,
    project: ActiveProject,
    search_in: SearchIn,
    search_folder: String,
    subfolders: bool,
    /// Referencing files found, with their check boxes.
    referencing: Vec<(PathBuf, bool)>,
    searched: bool,
    found: Vec<Found>,
    plan: Option<Plan>,
    job: Option<Receiver<Done>>,
    /// The file has changes the package would not hold.
    unsaved: bool,
    result: Option<Result<String, String>>,
}

impl PackUi {
    fn scope(&self) -> Scope<'_> {
        Scope {
            project: &self.project.project,
            base: self.project.base(),
        }
    }

    /// The files whose links are walked.
    fn roots(&self) -> Vec<PathBuf> {
        let mut roots = vec![PathBuf::from(self.source.trim())];
        roots.extend(
            self.referencing
                .iter()
                .filter(|(_, on)| *on)
                .map(|(p, _)| p.clone()),
        );
        roots
    }

    /// Walks the links again on a worker thread.
    fn collect(&mut self) {
        self.plan = None;
        self.found.clear();
        if self.source.trim().is_empty() {
            return;
        }
        let (roots, active) = (self.roots(), self.project.clone());
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let scope = Scope {
                project: &active.project,
                base: active.base(),
            };
            let _ = tx.send(Done::Collected(pack::collect(&roots, scope)));
        });
        self.job = Some(rx);
    }

    fn replan(&mut self) {
        self.plan =
            (!self.found.is_empty()).then(|| pack::plan(&self.found, self.scope(), &self.options));
    }

    /// Search Now.
    fn search(&mut self) {
        let dirs = match self.search_in {
            SearchIn::Project => match self.project.base() {
                Some(b) => self.project.project.search_dirs(b),
                None => vec![self.project.workspace()],
            },
            SearchIn::Folder => vec![PathBuf::from(self.search_folder.trim())],
        };
        let deep = self.search_in == SearchIn::Project || self.subfolders;
        let (found, active) = (self.found.clone(), self.project.clone());
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let scope = Scope {
                project: &active.project,
                base: active.base(),
            };
            let _ = tx.send(Done::Referencing(pack::referencing(
                &found, &dirs, deep, scope,
            )));
        });
        self.job = Some(rx);
    }

    /// Start: what stops it, else the copy on a worker thread.
    fn start(&mut self) {
        let Some(plan) = self.plan.clone() else {
            return;
        };
        let dest = PathBuf::from(self.destination.trim());
        if dest.as_os_str().is_empty() {
            self.result = Some(Err("Pack and Go: choose a destination folder".into()));
            return;
        }
        if let Err(e) = pack::check_destination(&self.found, self.scope(), &dest) {
            self.result = Some(Err(format!("Pack and Go: {e}")));
            return;
        }
        if let Some(c) = pack::conflicts(&plan, &dest).first() {
            self.result = Some(Err(format!(
                "Pack and Go: {} already exists; nothing was copied",
                c.display()
            )));
            return;
        }
        let found = self.found.clone();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let r = pack::run(&plan, &found, &dest);
            let _ = tx.send(Done::Packed(r, dest));
        });
        self.result = None;
        self.job = Some(rx);
    }

    /// Takes a finished worker's result; a status text for the app when the
    /// copy ended.
    fn poll(&mut self) -> Option<Result<String, String>> {
        let done = self.job.as_ref()?.try_recv().ok()?;
        self.job = None;
        match done {
            Done::Collected(found) => {
                self.found = found;
                self.replan();
                None
            }
            Done::Referencing(list) => {
                self.searched = true;
                for p in list {
                    if !self.referencing.iter().any(|(q, _)| *q == p) {
                        self.referencing.push((p, false));
                    }
                }
                None
            }
            Done::Packed(r, dest) => {
                let msg = r
                    .map(|n| format!("Pack and Go: {n} files to {}", dest.display()))
                    .map_err(|e| format!("Pack and Go: {e}"));
                self.result = Some(msg.clone());
                Some(msg)
            }
        }
    }
}

/// `n` bytes for people.
fn size_text(n: u64) -> String {
    match n {
        0..=1023 => format!("{n} B"),
        1024..=1_048_575 => format!("{:.1} KB", n as f64 / 1024.0),
        _ => format!("{:.1} MB", n as f64 / 1_048_576.0),
    }
}

impl CadCraftApp {
    /// File › Pack and Go: the window for the active file.
    pub(super) fn open_pack_and_go(&mut self) {
        self.projects_load();
        let project = self.active_project().clone();
        let source = self.file_path.trim().to_string();
        let now = 0.0;
        let unsaved = !source.is_empty() && self.is_dirty(now, true);
        let mut ui = PackUi {
            source,
            destination: String::new(),
            options: PackOptions::default(),
            project,
            search_in: SearchIn::Project,
            search_folder: String::new(),
            subfolders: true,
            referencing: Vec::new(),
            searched: false,
            found: Vec::new(),
            plan: None,
            job: None,
            unsaved,
            result: None,
        };
        ui.collect();
        self.pack = Some(ui);
    }

    pub(super) fn pack_source_picked(&mut self, path: &Path) {
        if let Some(p) = &mut self.pack {
            p.source = path.display().to_string();
            p.unsaved = false;
            p.referencing.clear();
            p.searched = false;
            p.collect();
        }
    }

    pub(super) fn pack_destination_picked(&mut self, path: &Path) {
        if let Some(p) = &mut self.pack {
            p.destination = path.display().to_string();
        }
    }

    pub(super) fn pack_window(&mut self, ctx: &egui::Context) {
        let Some(mut p) = self.pack.take() else {
            return;
        };
        if let Some(msg) = p.poll() {
            self.file_status = Some(msg);
        }
        if p.job.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        let mut open = true;
        let mut close = false;
        let mut dialog = None;
        let win = egui::Window::new("Pack and Go")
            .open(&mut open)
            .resizable(true)
            .collapsible(false)
            .default_size([680.0, 620.0])
            .max_width(1000.0);
        self.layout
            .place("pack_and_go", ctx.content_rect(), true, win)
            .show(ctx, |ui| {
                egui::Panel::bottom(egui::Id::new("pack_buttons")).show(ui, |ui| {
                    pack_footer(ui, &mut p, &mut close);
                });
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| pack_body(ui, &mut p, &mut dialog));
            });
        if let Some(purpose) = dialog {
            self.open_dialog(purpose);
        }
        if open && !close {
            self.pack = Some(p);
        }
    }
}

/// Total Files, Disk Space Required, the result and the buttons.
fn pack_footer(ui: &mut egui::Ui, p: &mut PackUi, close: &mut bool) {
    let k = super::theme::tokens();
    ui.add_space(4.0);
    if let Some(r) = &p.result {
        let (text, color) = match r {
            Ok(t) => (t, k.ok),
            Err(t) => (t, k.error),
        };
        ui.add(egui::Label::new(RichText::new(text).color(color)).wrap());
    }
    ui.horizontal(|ui| {
        let (n, bytes) = p.plan.as_ref().map_or((0, 0), Plan::totals);
        ui.label(format!("Total Files: {n}"));
        ui.separator();
        ui.label(format!("Disk Space Required: {}", size_text(bytes)));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Done").clicked() {
                *close = true;
            }
            let can = p.job.is_none() && n > 0 && !p.destination.trim().is_empty();
            if ui
                .add_enabled(can, super::theme::ok_button("Start"))
                .on_hover_text("Copy the files; the originals are not changed")
                .clicked()
            {
                p.start();
            }
            if p.job.is_some() {
                ui.add(egui::Spinner::new());
            }
        });
    });
}

/// Every option, in Inventor's order.
fn pack_body(ui: &mut egui::Ui, p: &mut PackUi, dialog: &mut Option<DialogPurpose>) {
    let k = super::theme::tokens();
    egui::Grid::new("pack_paths").num_columns(3).show(ui, |ui| {
        ui.label("Source File");
        let r = ui.add_sized([460.0, 20.0], egui::TextEdit::singleline(&mut p.source));
        if r.lost_focus() && r.changed() {
            p.collect();
        }
        if ui.button("Browse...").clicked() {
            *dialog = Some(DialogPurpose::PackSource);
        }
        ui.end_row();
        ui.label("Destination Folder");
        ui.add_sized(
            [460.0, 20.0],
            egui::TextEdit::singleline(&mut p.destination),
        )
        .on_hover_text("Created when missing");
        if ui.button("Browse...").clicked() {
            *dialog = Some(DialogPurpose::PackDestination);
        }
        ui.end_row();
        ui.label("Project File");
        let name = match &p.project.file {
            Some(f) => f.display().to_string(),
            None => "Default".into(),
        };
        ui.add_sized(
            [460.0, 20.0],
            egui::Label::new(RichText::new(name).weak()).truncate(),
        );
        ui.end_row();
    });
    if p.source.trim().is_empty() {
        ui.colored_label(
            k.warning,
            "Save the file first, or choose one with Browse...",
        );
    } else if p.unsaved {
        ui.colored_label(
            k.warning,
            "The file has unsaved changes: the package holds the saved version",
        );
    }
    ui.separator();
    let before = p.options.clone();
    ui.horizontal(|ui| {
        ui.radio_value(&mut p.options.layout, Layout::KeepHierarchy, "Keep Folder Hierarchy")
            .on_hover_text("The project's folders are rebuilt under the destination; library files go to Libraries");
        ui.radio_value(&mut p.options.layout, Layout::SinglePath, "Copy to Single Path")
            .on_hover_text("Every file into the destination folder itself");
    });
    ui.horizontal(|ui| {
        let mut model_only = true;
        ui.radio_value(&mut model_only, true, "Model Files Only");
        ui.add_enabled_ui(false, |ui| {
            ui.radio_value(&mut model_only, false, "Include Linked Files")
                .on_disabled_hover_text(
                    "Spec 0.6d §3.1: the source files of Reference Model imports are not packed yet",
                );
        });
    });
    ui.horizontal(|ui| {
        ui.checkbox(&mut p.options.skip_libraries, "Skip Libraries")
            .on_hover_text(
                "Library files stay where they are; links point at them with absolute paths",
            );
        ui.checkbox(&mut p.options.skip_templates, "Skip Templates")
            .on_hover_text("Unchecked: the project's templates folder is copied too");
    });
    if p.options != before {
        p.replan();
    }
    ui.separator();
    ui.label(RichText::new("Search for referencing files").strong());
    ui.horizontal(|ui| {
        ui.radio_value(
            &mut p.search_in,
            SearchIn::Project,
            "Search project file locations",
        );
        ui.radio_value(&mut p.search_in, SearchIn::Folder, "Search folder");
    });
    if p.search_in == SearchIn::Folder {
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut p.search_folder).desired_width(400.0));
            ui.checkbox(&mut p.subfolders, "Include Subfolders");
        });
    }
    let can_search = p.job.is_none() && !p.found.is_empty();
    if ui
        .add_enabled(can_search, egui::Button::new("Search Now"))
        .on_hover_text("Finds drawings and assemblies that use these files")
        .clicked()
    {
        p.search();
    }
    let mut toggled = false;
    for (f, on) in &mut p.referencing {
        toggled |= ui.checkbox(on, f.display().to_string()).changed();
    }
    if p.searched && p.referencing.is_empty() {
        ui.weak("No other file uses these files");
    }
    if toggled {
        p.collect();
    }
    ui.separator();
    ui.label(RichText::new("Files Found").strong());
    let Some(plan) = &p.plan else {
        if p.job.is_some() {
            ui.weak("Reading the links...");
        }
        return;
    };
    egui::Grid::new("pack_files")
        .num_columns(3)
        .striped(true)
        .show(ui, |ui| {
            ui.weak("File");
            ui.weak("Goes to");
            ui.weak("Size");
            ui.end_row();
            for f in &plan.files {
                let name = f.source.file_name().unwrap_or_default().to_string_lossy();
                ui.label(name.as_ref())
                    .on_hover_text(f.source.display().to_string());
                match &f.dest {
                    Some(d) => ui.label(d.display().to_string()),
                    None => ui.weak("stays (library)"),
                };
                ui.label(size_text(f.size));
                ui.end_row();
            }
            ui.label(plan.project.0.display().to_string());
            ui.weak("new project");
            ui.label("");
            ui.end_row();
        });
    for problem in &plan.problems {
        ui.add(egui::Label::new(RichText::new(problem).color(k.warning)).wrap());
    }
}
