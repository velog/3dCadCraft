//! Files: New / Open / Save / Save As / Export through an in-app file
//! dialog (folders, files of the right kind, a name field), the recent
//! files list, unsaved-changes prompts before New / Open / closing, and
//! opening a project dropped on the window (`docs/spec/0.6-files.md`).

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
    /// Help -> Save Diagnostics (a zip, `gui/diag.rs`).
    Diagnostics,
    /// Parameters → Export CSV / Import CSV.
    ExportParams,
    ImportParams,
}

impl DialogPurpose {
    fn title(self) -> String {
        match self {
            DialogPurpose::Open => "Open".into(),
            DialogPurpose::SaveAs => "Save As".into(),
            DialogPurpose::SaveFolder => "Save as Folder".into(),
            DialogPurpose::Export(f) => format!("Export {}", f.extension().to_uppercase()),
            DialogPurpose::Diagnostics => "Save Diagnostics".into(),
            DialogPurpose::ExportParams => "Export Parameters".into(),
            DialogPurpose::ImportParams => "Import Parameters".into(),
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
            DialogPurpose::Diagnostics => &["zip"],
            DialogPurpose::ExportParams | DialogPurpose::ImportParams => &["csv"],
        }
    }

    fn saves(self) -> bool {
        !matches!(self, DialogPurpose::Open | DialogPurpose::ImportParams)
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

    /// The drive list is shown (Windows: Up from a drive's root).
    pub(super) fn at_drives(&self) -> bool {
        self.dir.as_os_str().is_empty()
    }

    /// Re-reads the folder.
    pub(super) fn refresh(&mut self) {
        self.note = None;
        if self.at_drives() {
            self.dir_text.clear();
            self.entries = drives().into_iter().map(|d| (d, true)).collect();
            return;
        }
        self.dir_text = self.dir.display().to_string();
        match list_dir(&self.dir, self.purpose.extensions()) {
            Ok(v) => self.entries = v,
            Err(e) => {
                self.entries.clear();
                self.note = Some(format!("Cannot read {}: {e}", self.dir.display()));
            }
        }
    }

    fn go(&mut self, dir: PathBuf) {
        let dir = drive_root(dir);
        if dir.is_dir() || (cfg!(windows) && dir.as_os_str().is_empty()) {
            self.dir = dir;
            self.refresh();
        } else {
            self.note = Some(format!("No folder {}", dir.display()));
        }
    }

    /// Up: the parent folder; from a drive's root, the drive list (Windows).
    fn up(&mut self) {
        match self.dir.parent() {
            Some(p) => {
                let p = p.to_path_buf();
                self.go(p);
            }
            None if cfg!(windows) && !self.at_drives() => self.go(PathBuf::new()),
            None => {}
        }
    }

    /// The path the dialog would use for its name (extension added when
    /// missing; a bare `.` or `..` is no file). In the drive list only a
    /// full path names a file.
    pub(super) fn target(&self) -> Option<PathBuf> {
        if self.at_drives() && !Path::new(self.name.trim()).is_absolute() {
            return None;
        }
        target_path(&self.dir, &self.name, self.purpose.extensions())
    }
}

/// A bare drive (`C:`) means its root (`C:\`); `C:` alone is "the current
/// folder on C:", so joining a name to it would not give `C:\name`.
pub(super) fn drive_root(dir: PathBuf) -> PathBuf {
    match dir.to_str() {
        Some(s) if s.len() == 2 && s.ends_with(':') && s.as_bytes()[0].is_ascii_alphabetic() => {
            PathBuf::from(format!("{s}\\"))
        }
        _ => dir,
    }
}

/// The drives that exist (`C:`, `D:`…); empty outside Windows.
pub(super) fn drives() -> Vec<String> {
    if !cfg!(windows) {
        return Vec::new();
    }
    (b'A'..=b'Z')
        .map(|c| format!("{}:", c as char))
        .filter(|d| Path::new(&format!("{d}\\")).is_dir())
        .collect()
}

/// Hidden entries: a leading `.` everywhere, and on Windows the Hidden or
/// System attribute (`$Recycle.Bin`, `AppData`, `ntuser.dat`…).
fn is_hidden(e: &std::fs::DirEntry, name: &str) -> bool {
    if name.starts_with('.') {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const HIDDEN_OR_SYSTEM: u32 = 0x2 | 0x4;
        if e.metadata()
            .is_ok_and(|m| m.file_attributes() & HIDDEN_OR_SYSTEM != 0)
        {
            return true;
        }
    }
    #[cfg(not(windows))]
    let _ = e;
    false
}

/// Folders and files with one of `exts` (any file when empty and the list
/// is for folders only, files are left out), sorted: folders first, by
/// name, hidden entries skipped. An unreadable folder is an error.
pub(super) fn list_dir(dir: &Path, exts: &[&str]) -> std::io::Result<Vec<(String, bool)>> {
    let rd = std::fs::read_dir(dir)?;
    let mut v: Vec<(String, bool)> = rd
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_owned();
            if is_hidden(&e, &name) {
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
    Ok(v)
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
    /// Quit the application.
    Close,
    /// Close the active document tab.
    CloseTab,
    /// Assemble › Place, after the assembly was saved.
    Place,
}

/// The user's home folder: `USERPROFILE` on Windows (Git Bash / MSYS set
/// `HOME` to paths like `/c/Users/x` that Windows cannot open), else `HOME`.
pub(super) fn home_dir() -> Option<PathBuf> {
    let (first, second) = if cfg!(windows) {
        ("USERPROFILE", "HOME")
    } else {
        ("HOME", "USERPROFILE")
    };
    std::env::var_os(first)
        .or_else(|| std::env::var_os(second))
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

/// `settings.json`: `%APPDATA%\3dcadcraft` on Windows, else
/// `~/.config/3dcadcraft`.
pub(super) fn settings_path() -> Option<PathBuf> {
    Some(crate::presets::config_dir()?.join("settings.json"))
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
    let abs = std::fs::canonicalize(path)
        .map(crate::document::library::without_verbatim)
        .unwrap_or_else(|_| path.to_path_buf());
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

/// Where dialogs start: the current file's folder, else ~/Documents (on
/// Windows also a OneDrive-redirected Documents), else ~.
fn start_dir(current: &str) -> PathBuf {
    let cur = Path::new(current.trim());
    if let Some(parent) = cur
        .parent()
        .filter(|p| !p.as_os_str().is_empty() && p.is_dir())
    {
        return parent.to_path_buf();
    }
    let home = home_dir().unwrap_or_else(|| PathBuf::from("."));
    let onedrive = cfg!(windows)
        .then(|| std::env::var_os("OneDrive"))
        .flatten()
        .map(|d| Path::new(&d).join("Documents"));
    std::iter::once(home.join("Documents"))
        .chain(onedrive)
        .find(|d| d.is_dir())
        .unwrap_or(home)
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
            DialogPurpose::Diagnostics => format!(
                "3dcadcraft-diagnostics-{}.zip",
                crate::diagnostics::stamp(std::time::SystemTime::now())
            ),
            DialogPurpose::ExportParams => format!("{stem}-parameters.csv"),
            DialogPurpose::ImportParams => String::new(),
        };
        self.file_dialog = Some(FileDialog::new(purpose, start_dir(&self.file_path), name));
    }

    /// New / Open open a tab of their own; Close / Close Tab ask to save
    /// unsaved changes first.
    pub(super) fn request(&mut self, what: Pending, now: f64) {
        let closes = matches!(what, Pending::Close | Pending::CloseTab);
        if what == Pending::Close {
            self.quit_dropped.clear();
        }
        if closes && self.is_dirty(now, true) {
            self.unsaved_prompt = Some(what);
        } else {
            self.run_pending(what);
        }
    }

    fn run_pending(&mut self, what: Pending) {
        match what {
            Pending::New => {
                self.add_blank_tab();
                self.new_project();
            }
            Pending::Open => self.open_dialog(DialogPurpose::Open),
            Pending::CloseTab => self.close_active_tab(),
            Pending::Place => self.open_place_dialog(),
            Pending::Close => {
                // Every changed tab asks in turn.
                if let Some(k) = self.dirty_other_tab() {
                    self.switch_tab(k);
                    self.unsaved_prompt = Some(Pending::Close);
                } else {
                    self.close_ok = true;
                }
            }
        }
    }

    /// File → New: an empty part, not saved yet.
    pub(super) fn new_project(&mut self) {
        let mut project = Project::new("Untitled");
        let id = project.add_document("Part1", DocumentData::Part(PartDocument::default()));
        if let Some(d) = project.document_mut(id) {
            d.properties = crate::document::iprops::IProperties::new_document();
        }
        self.project = project;
        self.part_id = id;
        self.asm = Default::default();
        self.cons = Default::default();
        self.file_path.clear();
        self.history.clear();
        self.editing = None;
        self.selection = super::Selection::Body;
        self.after_restore();
        self.fit_view();
        self.mark_saved();
        self.file_status = Some(Ok("New project".into()));
    }

    /// Opens `path` (from the dialog, the recent list or a drop) in a tab of
    /// its own; a file already open comes forward.
    pub(super) fn open_path(&mut self, path: &Path) {
        if let Some(k) = self.tab_of_path(path) {
            self.switch_tab(k);
            self.file_status = Some(Ok(format!("'{}' is already open", self.document_title())));
            return;
        }
        let prev = self.active_tab;
        let n = self.tab_count();
        self.add_blank_tab();
        let added = self.tab_count() > n;
        self.open_path_here(path);
        if added && !matches!(self.file_status, Some(Ok(_))) {
            // It failed: the empty tab goes, the old document is back.
            let status = self.file_status.take();
            self.close_active_tab();
            self.switch_tab(prev);
            self.file_status = status;
        }
    }

    /// Opens `path` into the active tab.
    fn open_path_here(&mut self, path: &Path) {
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
            self.quit_dropped.clear();
            if self.is_dirty(now, true) {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.unsaved_prompt = Some(Pending::Close);
            } else if let Some(k) = self.dirty_other_tab() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.switch_tab(k);
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
                .find(|p| {
                    p.extension()
                        .and_then(|x| x.to_str())
                        .is_some_and(|x| x.eq_ignore_ascii_case(FILE_EXTENSION))
                })
        });
        if let Some(p) = dropped {
            // A tab of its own: nothing to save first.
            self.open_path(&p);
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
                if what == Pending::Close {
                    // Quit: this tab's changes are dropped, the next asks.
                    self.quit_dropped.push(self.active_tab);
                }
                self.after_unsaved(what);
            }
            Some(_) => {
                self.quit_dropped.clear();
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
        self.layout
            .place(
                "file_dialog",
                ctx.content_rect(),
                true,
                egui::Window::new(d.purpose.title())
                    .open(&mut open)
                    .collapsible(false)
                    .resizable(true)
                    .default_size(egui::vec2(520.0, 420.0))
                    .pivot(egui::Align2::CENTER_CENTER)
                    .default_pos(ctx.content_rect().center()),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let up = ui.add_enabled(
                        d.dir.parent().is_some() || (cfg!(windows) && !d.at_drives()),
                        egui::Button::new("Up"),
                    );
                    if up.on_hover_text("Parent folder").clicked() {
                        d.up();
                    }
                    if ui.button("Home").clicked()
                        && let Some(h) = home_dir()
                    {
                        d.go(h);
                    }
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut d.dir_text)
                            .hint_text("Drives - type a folder")
                            .desired_width(f32::INFINITY),
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
                            let label = if d.at_drives() {
                                format!("[ ] {name}\\")
                            } else if is_dir {
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
                        None if d.at_drives() && !d.name.trim().is_empty() => {
                            d.note = Some("Pick a drive or type a full path".into())
                        }
                        None => d.note = Some("Type a file name".into()),
                        Some(p) if !d.purpose.saves() => {
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
            DialogPurpose::Diagnostics => self.save_diagnostics(&path),
            DialogPurpose::ExportParams => {
                let text = self.part().parameters_csv();
                self.file_status = Some(
                    std::fs::write(&path, text)
                        .map(|()| format!("Parameters exported to {}", path.display()))
                        .map_err(|e| format!("Export Parameters: {e}")),
                );
            }
            DialogPurpose::ImportParams => {
                let r = std::fs::read_to_string(&path).map_err(|e| e.to_string());
                self.checkpoint("Import Parameters");
                let r = r.and_then(|t| {
                    self.part_mut()
                        .import_parameters_csv(&t)
                        .map_err(|e| e.to_string())
                });
                match r {
                    Ok(n) => {
                        let _ = self.part_mut().regenerate();
                        self.scene_dirty = true;
                        self.solve();
                        self.file_status = Some(Ok(format!("{n} parameters imported")));
                    }
                    Err(e) => {
                        self.history.cancel();
                        self.file_status = Some(Err(format!("Import Parameters: {e}")));
                    }
                }
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
        if ui
            .button("iProperties...")
            .on_hover_text("Title, part number, status and custom properties of this document")
            .clicked()
        {
            self.open_iproperties(self.part_id);
            ui.close();
        }
        if ui
            .button("New Assembly")
            .on_hover_text("An assembly in this project: place the project's parts in it")
            .clicked()
        {
            self.new_assembly();
            ui.close();
        }
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
        let v = list_dir(&root, &["3dcadcraft"]).unwrap();
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

    #[test]
    fn unreadable_folder_is_an_error() {
        let missing = std::env::temp_dir().join("cadcraft_no_such_folder_x");
        assert!(list_dir(&missing, &["stl"]).is_err());
    }

    #[test]
    fn bare_drive_means_its_root() {
        assert_eq!(drive_root(PathBuf::from("C:")), PathBuf::from("C:\\"));
        assert_eq!(drive_root(PathBuf::from("d:")), PathBuf::from("d:\\"));
        for p in ["C:\\", "C:\\x", "/home", "ab", ""] {
            assert_eq!(drive_root(PathBuf::from(p)), PathBuf::from(p));
        }
    }

    /// Windows: Up from `C:\` shows the drive list, a drive opens its root,
    /// and a name typed there is no file until a drive is picked.
    #[cfg(windows)]
    #[test]
    fn up_from_a_drive_root_lists_the_drives() {
        let sys = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        assert!(drives().contains(&sys.to_uppercase()), "{:?}", drives());
        let mut d = FileDialog::new(
            DialogPurpose::SaveAs,
            PathBuf::from(format!("{sys}\\")),
            "x".into(),
        );
        d.up();
        assert!(d.at_drives());
        assert!(d.entries.iter().all(|(_, dir)| *dir));
        assert_eq!(d.target(), None);
        d.name = format!("{sys}\\a\\x");
        assert_eq!(
            d.target(),
            Some(PathBuf::from(format!("{sys}\\a\\x.3dcadcraft")))
        );
        d.go(d.dir.join(&sys));
        assert_eq!(d.dir, PathBuf::from(format!("{sys}\\")));
        // Up from the drive list does nothing.
        d.go(PathBuf::new());
        d.up();
        assert!(d.at_drives());
    }

    /// Windows: folders with the Hidden or System attribute are not listed.
    #[cfg(windows)]
    #[test]
    fn hidden_attribute_hides_entries() {
        let root = std::env::temp_dir().join(format!("cadcraft_hidden_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("secret")).unwrap();
        std::fs::create_dir_all(root.join("shown")).unwrap();
        let ok = std::process::Command::new("attrib")
            .arg("+h")
            .arg(root.join("secret"))
            .status()
            .unwrap()
            .success();
        assert!(ok);
        let v = list_dir(&root, &["3dcadcraft"]).unwrap();
        assert_eq!(v, vec![("shown".to_owned(), true)]);
        let _ = std::process::Command::new("attrib")
            .arg("-h")
            .arg(root.join("secret"))
            .status();
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn home_is_a_native_path() {
        if let Some(h) = home_dir() {
            assert!(h.is_absolute(), "{}", h.display());
        }
    }
}
