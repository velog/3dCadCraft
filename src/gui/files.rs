//! Files: New / Open / Save / Save As / Export through an in-app file
//! dialog (folders, files of the right kind, a name field), the recent
//! files list, unsaved-changes prompts before New / Open / closing, and
//! opening a project dropped on the window (`docs/spec/0.12-files.md`).

use std::path::{Path, PathBuf};

use eframe::egui;

use super::CadCraftApp;
use crate::document::{DocumentData, FILE_EXTENSION, Project};
use crate::part::PartDocument;
use crate::part::export::ExportFormat;

/// What the file dialog is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DialogPurpose {
    Open,
    SaveAs,
    /// Save as an unpacked folder (git-friendly).
    SaveFolder,
    Export(ExportFormat),
}

impl DialogPurpose {
    fn title(self) -> String {
        match self {
            DialogPurpose::Open => "Open".into(),
            DialogPurpose::SaveAs => "Save As".into(),
            DialogPurpose::SaveFolder => "Save as Folder".into(),
            DialogPurpose::Export(f) => format!("Export {}", f.extension().to_uppercase()),
        }
    }

    /// Extensions listed and given to a typed name.
    fn extensions(self) -> &'static [&'static str] {
        match self {
            DialogPurpose::Open | DialogPurpose::SaveAs => &[FILE_EXTENSION],
            DialogPurpose::SaveFolder => &[],
            DialogPurpose::Export(ExportFormat::Stl) => &["stl"],
            DialogPurpose::Export(ExportFormat::Step) => &["step", "stp"],
            DialogPurpose::Export(ExportFormat::Glb) => &["glb"],
        }
    }

    fn saves(self) -> bool {
        self != DialogPurpose::Open
    }
}

/// The open file dialog.
#[derive(Clone, Debug)]
pub(super) struct FileDialog {
    pub purpose: DialogPurpose,
    /// The folder shown.
    pub dir: PathBuf,
    /// Its text field (editable; Enter goes there).
    pub dir_text: String,
    /// The file name (Save / Export) or the picked file (Open).
    pub name: String,
    /// The folder's entries: (name, is folder), folders first.
    pub entries: Vec<(String, bool)>,
    /// A problem to show (missing folder, file exists…).
    pub note: Option<String>,
    /// The name was confirmed once over an existing file.
    pub replace_ok: bool,
    /// Give the name field the keyboard focus (once, on opening).
    pub focus: bool,
}

impl FileDialog {
    pub(super) fn new(purpose: DialogPurpose, dir: PathBuf, name: String) -> Self {
        let mut d = Self {
            purpose,
            dir_text: dir.display().to_string(),
            dir,
            name,
            entries: Vec::new(),
            note: None,
            replace_ok: false,
            focus: true,
        };
        d.refresh();
        d
    }

    /// Re-reads the folder.
    pub(super) fn refresh(&mut self) {
        self.dir_text = self.dir.display().to_string();
        self.entries = list_dir(&self.dir, self.purpose.extensions());
        self.note = None;
    }

    fn go(&mut self, dir: PathBuf) {
        if dir.is_dir() {
            self.dir = dir;
            self.refresh();
        } else {
            self.note = Some(format!("No folder {}", dir.display()));
        }
    }

    /// The path the dialog would use for its name (extension added when
    /// missing; a bare `.` or `..` is no file).
    pub(super) fn target(&self) -> Option<PathBuf> {
        target_path(&self.dir, &self.name, self.purpose.extensions())
    }
}

/// Folders and files with one of `exts` (any file when empty and the list
/// is for folders only, files are left out), sorted: folders first, by
/// name, hidden entries skipped.
pub(super) fn list_dir(dir: &Path, exts: &[&str]) -> Vec<(String, bool)> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut v: Vec<(String, bool)> = rd
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_owned();
            if name.starts_with('.') {
                return None;
            }
            let path = e.path();
            // A packed project is a file; an unpacked one a folder with a
            // manifest: listed as a project too.
            let is_dir = path.is_dir();
            let ext_ok = Path::new(&name)
                .extension()
                .and_then(|x| x.to_str())
                .is_some_and(|x| exts.iter().any(|e| x.eq_ignore_ascii_case(e)));
            if is_dir && !(ext_ok && path.join("manifest.json").is_file()) {
                return Some((name, true));
            }
            (ext_ok).then_some((name, false))
        })
        .collect();
    v.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then(a.0.to_lowercase().cmp(&b.0.to_lowercase()))
    });
    v
}

/// `dir/name` with the first of `exts` added when the name has none of them.
pub(super) fn target_path(dir: &Path, name: &str, exts: &[&str]) -> Option<PathBuf> {
    let name = name.trim();
    if name.is_empty() || name == "." || name == ".." {
        return None;
    }
    let p = Path::new(name);
    let base = if p.is_absolute() {
        p.to_path_buf()
    } else {
        dir.join(p)
    };
    let has = base
        .extension()
        .and_then(|x| x.to_str())
        .is_some_and(|x| exts.iter().any(|e| x.eq_ignore_ascii_case(e)));
    Some(match exts.first() {
        Some(e) if !has => {
            let mut s = base.into_os_string();
            s.push(format!(".{e}"));
            PathBuf::from(s)
        }
        _ => base,
    })
}

/// What waits for "save changes?" to be answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Pending {
    New,
    Open,
    Close,
}

/// The user's home folder (`HOME`, or `USERPROFILE` on Windows).
pub(super) fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// `settings.json`: `%APPDATA%\3dcadcraft` on Windows, else
/// `~/.config/3dcadcraft`.
pub(super) fn settings_path() -> Option<PathBuf> {
    if cfg!(windows)
        && let Some(app) = std::env::var_os("APPDATA")
    {
        return Some(Path::new(&app).join("3dcadcraft").join("settings.json"));
    }
    Some(
        home_dir()?
            .join(".config")
            .join("3dcadcraft")
            .join("settings.json"),
    )
}

/// The recent files (newest first, at most 10, existing ones only).
pub(super) fn recent_files() -> Vec<PathBuf> {
    settings_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| {
            v.get("recent_files")?.as_array().map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(PathBuf::from))
                    .filter(|p| p.exists())
                    .collect()
            })
        })
        .unwrap_or_default()
}

/// Puts `path` first in the recent files.
fn remember_recent(path: &Path) {
    let Some(p) = settings_path() else { return };
    let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut list: Vec<PathBuf> = recent_files().into_iter().filter(|x| *x != abs).collect();
    list.insert(0, abs);
    list.truncate(10);
    let mut v = std::fs::read_to_string(&p)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    v["recent_files"] = serde_json::Value::Array(
        list.iter()
            .map(|x| serde_json::Value::String(x.display().to_string()))
            .collect(),
    );
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string_pretty(&v) {
        let _ = std::fs::write(&p, text);
    }
}

/// Where dialogs start: the current file's folder, else ~/Documents, else ~.
fn start_dir(current: &str) -> PathBuf {
    let cur = Path::new(current.trim());
    if let Some(parent) = cur
        .parent()
        .filter(|p| !p.as_os_str().is_empty() && p.is_dir())
    {
        return parent.to_path_buf();
    }
    let home = home_dir().unwrap_or_else(|| PathBuf::from("."));
    let docs = home.join("Documents");
    if docs.is_dir() { docs } else { home }
}

impl CadCraftApp {
    /// A file window (dialog or "save changes?") is open: the rest of the
    /// app takes no keys and no clicks in the view.
    pub(super) fn file_window_open(&self) -> bool {
        self.file_dialog.is_some() || self.unsaved_prompt.is_some()
    }

    /// A cheap signature of the whole project (for "unsaved changes").
    fn project_signature(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        serde_json::to_vec(&self.project)
            .unwrap_or_default()
            .hash(&mut h);
        h.finish()
    }

    /// The project as it is now counts as saved.
    pub(super) fn mark_saved(&mut self) {
        self.saved_sig = self.project_signature();
        self.dirty_cache = (f64::NEG_INFINITY, false);
    }

    /// Whether the project changed since it was last saved or opened
    /// (recomputed at most once a second; `force` recomputes now).
    pub(super) fn is_dirty(&mut self, now: f64, force: bool) -> bool {
        if force || now - self.dirty_cache.0 > 1.0 {
            let d = self.project_signature() != self.saved_sig;
            self.dirty_cache = (now, d);
        }
        self.dirty_cache.1
    }

    /// The document's display name: the file name, or the project name.
    pub(super) fn document_title(&self) -> String {
        Path::new(self.file_path.trim())
            .file_name()
            .and_then(|s| s.to_str())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("{} (not saved yet)", self.project.name))
    }

    /// File → Save (Ctrl+S): to the current file, or Save As when there is none.
    pub(super) fn save(&mut self) {
        if self.file_path.trim().is_empty() {
            self.open_dialog(DialogPurpose::SaveAs);
        } else {
            self.save_project(false);
        }
    }

    /// Opens the file dialog for `purpose`.
    pub(super) fn open_dialog(&mut self, purpose: DialogPurpose) {
        let stem = Path::new(self.file_path.trim())
            .file_stem()
            .and_then(|s| s.to_str())
            .map(str::to_owned)
            .unwrap_or_else(|| self.project.name.clone());
        let name = match purpose {
            DialogPurpose::Open => String::new(),
            DialogPurpose::SaveAs => format!("{stem}.{FILE_EXTENSION}"),
            DialogPurpose::SaveFolder => format!("{stem}.{FILE_EXTENSION}"),
            DialogPurpose::Export(f) => format!("{stem}.{}", f.extension()),
        };
        self.file_dialog = Some(FileDialog::new(purpose, start_dir(&self.file_path), name));
    }

    /// New / Open / Close: asks to save unsaved changes first.
    pub(super) fn request(&mut self, what: Pending, now: f64) {
        if self.is_dirty(now, true) {
            self.unsaved_prompt = Some(what);
        } else {
            self.run_pending(what);
        }
    }

    fn run_pending(&mut self, what: Pending) {
        match what {
            Pending::New => self.new_project(),
            Pending::Open => self.open_dialog(DialogPurpose::Open),
            Pending::Close => {
                self.close_ok = true;
            }
        }
    }

    /// File → New: an empty part, not saved yet.
    pub(super) fn new_project(&mut self) {
        let mut project = Project::new("Untitled");
        let id = project.add_document("Part1", DocumentData::Part(PartDocument::default()));
        self.project = project;
        self.part_id = id;
        self.file_path.clear();
        self.history.clear();
        self.editing = None;
        self.selection = super::Selection::Body;
        self.after_restore();
        self.fit_view();
        self.mark_saved();
        self.file_status = Some(Ok("New project".into()));
    }

    /// Opens `path` (from the dialog, the recent list or a drop).
    pub(super) fn open_path(&mut self, path: &Path) {
        let before = std::mem::replace(&mut self.file_path, path.display().to_string());
        self.open_project();
        if matches!(self.file_status, Some(Ok(_))) {
            remember_recent(path);
            self.mark_saved();
        } else {
            self.file_path = before;
        }
    }

    /// After a successful save to the current path.
    pub(super) fn saved_ok(&mut self) {
        remember_recent(Path::new(self.file_path.trim()));
        self.mark_saved();
        if let Some(next) = self.after_save.take() {
            self.run_pending(next);
        }
    }

    /// The file dialog, the "save changes?" prompt, dropped files and the
    /// close request. Called once per frame.
    pub(super) fn files_ui(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        // "*" in the title bar and next to the name (once a second).
        let dirty = self.is_dirty(now, false);
        let title = format!(
            "{}{} - 3DCadCraft",
            self.document_title(),
            if dirty { " *" } else { "" }
        );
        if title != self.window_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }
        // Closing the window with unsaved changes: ask first.
        if ctx.input(|i| i.viewport().close_requested()) && !self.close_ok {
            if self.is_dirty(now, true) {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.unsaved_prompt = Some(Pending::Close);
            } else {
                self.close_ok = true;
            }
        }
        if self.close_ok && self.unsaved_prompt.is_none() && self.after_save.is_none() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        // A project dropped on the window opens (after the prompt).
        let dropped: Option<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .find(|p| p.extension().and_then(|x| x.to_str()) == Some(FILE_EXTENSION))
        });
        if let Some(p) = dropped {
            if self.is_dirty(now, true) {
                self.dropped = Some(p);
                self.unsaved_prompt = Some(Pending::Open);
            } else {
                self.open_path(&p);
            }
        }
        self.unsaved_window(ctx);
        self.dialog_window(ctx);
    }

    fn unsaved_window(&mut self, ctx: &egui::Context) {
        let Some(what) = self.unsaved_prompt else {
            return;
        };
        let title = self.document_title();
        let mut answer = None;
        egui::Window::new("Unsaved changes")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label(format!("Save changes to {title}?"));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        answer = Some(0);
                    }
                    if ui.button("Don't Save").clicked() {
                        answer = Some(1);
                    }
                    if ui.button("Cancel").clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        answer = Some(2);
                    }
                });
            });
        match answer {
            Some(0) => {
                self.unsaved_prompt = None;
                self.after_save = Some(what);
                if self.file_path.trim().is_empty() {
                    self.open_dialog(DialogPurpose::SaveAs);
                } else {
                    self.save_project(false);
                }
            }
            Some(1) => {
                self.unsaved_prompt = None;
                self.after_unsaved(what);
            }
            Some(_) => {
                self.unsaved_prompt = None;
                self.dropped = None;
            }
            None => {}
        }
    }

    /// The prompt was answered with "Don't Save" (or saving finished).
    fn after_unsaved(&mut self, what: Pending) {
        if what == Pending::Open
            && let Some(p) = self.dropped.take()
        {
            self.open_path(&p);
            return;
        }
        self.run_pending(what);
    }

    fn dialog_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.file_dialog.take() else {
            return;
        };
        let mut done: Option<PathBuf> = None;
        let mut cancel = false;
        let mut open = true;
        egui::Window::new(d.purpose.title())
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_size(egui::vec2(520.0, 420.0))
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("Up").on_hover_text("Parent folder").clicked()
                        && let Some(p) = d.dir.parent()
                    {
                        let p = p.to_path_buf();
                        d.go(p);
                    }
                    if ui.button("Home").clicked()
                        && let Some(h) = home_dir()
                    {
                        d.go(h);
                    }
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut d.dir_text).desired_width(f32::INFINITY),
                    );
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        let p = PathBuf::from(d.dir_text.trim());
                        d.go(p);
                    }
                });
                ui.separator();
                let mut enter: Option<String> = None;
                let mut chosen: Option<String> = None;
                egui::ScrollArea::vertical()
                    .max_height(260.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if d.entries.is_empty() {
                            ui.weak("(no folders or matching files here)");
                        }
                        for (name, is_dir) in d.entries.clone() {
                            let label = if is_dir {
                                format!("[ ] {name}/")
                            } else {
                                name.clone()
                            };
                            let selected = !is_dir && d.name == name;
                            let r = ui.selectable_label(selected, label);
                            if is_dir {
                                if r.clicked() {
                                    enter = Some(name);
                                }
                            } else if r.double_clicked() {
                                chosen = Some(name);
                            } else if r.clicked() {
                                d.name = name;
                                d.replace_ok = false;
                            }
                        }
                    });
                if let Some(n) = enter {
                    let p = d.dir.join(n);
                    d.go(p);
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(if d.purpose.saves() {
                        "File name"
                    } else {
                        "File"
                    });
                    let r = ui
                        .add(egui::TextEdit::singleline(&mut d.name).desired_width(f32::INFINITY));
                    if std::mem::take(&mut d.focus) {
                        r.request_focus();
                    }
                    if r.changed() {
                        d.replace_ok = false;
                    }
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        chosen = Some(d.name.clone());
                    }
                });
                if let Some(n) = &d.note {
                    ui.colored_label(egui::Color32::from_rgb(220, 120, 40), n);
                }
                ui.horizontal(|ui| {
                    let ok = if d.purpose.saves() { "Save" } else { "Open" };
                    if ui.button(ok).clicked() {
                        chosen = Some(d.name.clone());
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                    let recent = recent_files();
                    if d.purpose == DialogPurpose::Open && !recent.is_empty() {
                        ui.menu_button("Recent", |ui| {
                            for p in recent {
                                if ui.button(p.display().to_string()).clicked() {
                                    d.name = p.display().to_string();
                                    chosen = Some(d.name.clone());
                                    ui.close();
                                }
                            }
                        });
                    }
                });
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    cancel = true;
                }
                if let Some(n) = chosen {
                    d.name = n;
                    match d.target() {
                        None => d.note = Some("Type a file name".into()),
                        Some(p) if d.purpose == DialogPurpose::Open => {
                            if p.exists() {
                                done = Some(p);
                            } else {
                                d.note = Some(format!("No file {}", p.display()));
                            }
                        }
                        Some(p) => {
                            if p.exists() && !d.replace_ok {
                                d.note = Some(format!(
                                    "{} exists: click {} again to replace it",
                                    p.file_name().and_then(|s| s.to_str()).unwrap_or_default(),
                                    if d.purpose.saves() { "Save" } else { "Open" }
                                ));
                                d.replace_ok = true;
                            } else {
                                done = Some(p);
                            }
                        }
                    }
                }
            });
        if !open || cancel {
            // A Save As asked for by "save changes?" was given up: so is
            // what waited for it.
            self.after_save = None;
            self.close_ok = false;
            return;
        }
        let Some(path) = done else {
            self.file_dialog = Some(d);
            return;
        };
        match d.purpose {
            DialogPurpose::Open => self.open_path(&path),
            DialogPurpose::SaveAs => {
                self.file_path = path.display().to_string();
                self.save_project(false);
            }
            DialogPurpose::SaveFolder => {
                self.file_path = path.display().to_string();
                self.save_project(true);
            }
            DialogPurpose::Export(f) => {
                let name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Part")
                    .to_owned();
                self.file_status = Some(
                    crate::part::export::export(self.part(), &name, &path)
                        .map(|_| format!("Exported {}", path.display()))
                        .map_err(|e| format!("Export {}: {e}", f.extension().to_uppercase())),
                );
            }
        }
    }

    /// The File menu.
    pub(super) fn file_menu(&mut self, ui: &mut egui::Ui) {
        let now = ui.input(|i| i.time);
        if ui.button("New            Ctrl+N").clicked() {
            self.request(Pending::New, now);
            ui.close();
        }
        if ui.button("Open...        Ctrl+O").clicked() {
            self.request(Pending::Open, now);
            ui.close();
        }
        let recent = recent_files();
        ui.add_enabled_ui(!recent.is_empty(), |ui| {
            ui.menu_button("Open Recent", |ui| {
                for p in recent {
                    if ui.button(p.display().to_string()).clicked() {
                        if self.is_dirty(now, true) {
                            self.dropped = Some(p);
                            self.unsaved_prompt = Some(Pending::Open);
                        } else {
                            self.open_path(&p);
                        }
                        ui.close();
                    }
                }
            });
        });
        ui.separator();
        if ui.button("Save           Ctrl+S").clicked() {
            self.save();
            ui.close();
        }
        if ui.button("Save As...     Ctrl+Shift+S").clicked() {
            self.open_dialog(DialogPurpose::SaveAs);
            ui.close();
        }
        if ui
            .button("Save as Folder...")
            .on_hover_text("Unpacked directory, git-friendly")
            .clicked()
        {
            self.open_dialog(DialogPurpose::SaveFolder);
            ui.close();
        }
        ui.separator();
        ui.menu_button("Export", |ui| {
            for (f, tip) in [
                (ExportFormat::Stl, "Triangles in mm, for 3D printing"),
                (ExportFormat::Step, "Exact B-rep for other CAD programs"),
                (ExportFormat::Glb, "Binary glTF for viewers and the web"),
            ] {
                let label = format!("{}...", f.extension().to_uppercase());
                if ui.button(label).on_hover_text(tip).clicked() {
                    self.open_dialog(DialogPurpose::Export(f));
                    ui.close();
                }
            }
        });
        ui.separator();
        ui.weak(match self.file_path.trim() {
            "" => "Not saved yet".to_owned(),
            p => p.to_owned(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_adds_the_extension() {
        let d = Path::new("/tmp");
        assert_eq!(
            target_path(d, "part", &["3dcadcraft"]),
            Some(PathBuf::from("/tmp/part.3dcadcraft"))
        );
        assert_eq!(
            target_path(d, "part.3DCADCRAFT", &["3dcadcraft"]),
            Some(PathBuf::from("/tmp/part.3DCADCRAFT"))
        );
        assert_eq!(
            target_path(d, "a/b.step", &["step", "stp"]),
            Some(PathBuf::from("/tmp/a/b.step"))
        );
        assert_eq!(
            target_path(d, "x.stp", &["step", "stp"]),
            Some(PathBuf::from("/tmp/x.stp"))
        );
        assert_eq!(
            target_path(d, "/abs/x", &["stl"]),
            Some(PathBuf::from("/abs/x.stl"))
        );
        assert_eq!(target_path(d, "  ", &["stl"]), None);
    }

    #[test]
    fn listing_shows_folders_then_matching_files() {
        let root = std::env::temp_dir().join(format!("cadcraft_list_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::create_dir_all(root.join(".hidden")).unwrap();
        std::fs::write(root.join("b.3dcadcraft"), b"x").unwrap();
        std::fs::write(root.join("a.3dcadcraft"), b"x").unwrap();
        std::fs::write(root.join("notes.txt"), b"x").unwrap();
        let v = list_dir(&root, &["3dcadcraft"]);
        assert_eq!(
            v,
            vec![
                ("sub".to_owned(), true),
                ("a.3dcadcraft".to_owned(), false),
                ("b.3dcadcraft".to_owned(), false)
            ]
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
