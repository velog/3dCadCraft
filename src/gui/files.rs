//! Files: New / Open / Save / Save As / Export through an in-app file
//! dialog (folders, files of the right kind, a name field), the recent
//! files list, unsaved-changes prompts before New / Open / closing, and
//! opening a project dropped on the window (`docs/spec/0.6-files.md`).

use std::path::{Path, PathBuf};

use eframe::egui;

use super::CadCraftApp;
use crate::document::format::{FilePreview, read_preview};
use crate::document::{DocumentData, DocumentKind, FILE_EXTENSION, Project};
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
    /// Bill of Materials → Export (CSV).
    ExportBom,
    /// Sheet Metal → Export Flat Pattern (DXF).
    ExportDxf,
    /// Drawing → File › Export › PDF.
    ExportPdf,
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
            DialogPurpose::ExportBom => "Export Bill of Materials".into(),
            DialogPurpose::ExportDxf => "Export Flat Pattern".into(),
            DialogPurpose::ExportPdf => "Export PDF".into(),
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
            DialogPurpose::ExportParams
            | DialogPurpose::ImportParams
            | DialogPurpose::ExportBom => &["csv"],
            DialogPurpose::ExportDxf => &["dxf"],
            DialogPurpose::ExportPdf => &["pdf"],
        }
    }

    /// The "Files of type" line.
    fn type_text(self) -> String {
        let kind = match self {
            DialogPurpose::Open | DialogPurpose::SaveAs => "3DCadCraft project",
            DialogPurpose::SaveFolder => return "3DCadCraft project folder".into(),
            DialogPurpose::Export(ExportFormat::Stl) => "STL",
            DialogPurpose::Export(ExportFormat::Step) => "STEP",
            DialogPurpose::Export(ExportFormat::Glb) => "glTF binary",
            DialogPurpose::Diagnostics => "Zip archive",
            DialogPurpose::ExportParams
            | DialogPurpose::ImportParams
            | DialogPurpose::ExportBom => "CSV",
            DialogPurpose::ExportDxf => "DXF",
            DialogPurpose::ExportPdf => "PDF",
        };
        let pats: Vec<String> = self.extensions().iter().map(|e| format!("*.{e}")).collect();
        format!("{kind} ({})", pats.join(", "))
    }

    fn saves(self) -> bool {
        !matches!(self, DialogPurpose::Open | DialogPurpose::ImportParams)
    }
}

/// A row of the file dialog's list.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Entry {
    /// The name shown; in the Recent place the file's full path.
    pub name: String,
    pub is_dir: bool,
    pub modified: Option<std::time::SystemTime>,
    /// Files only.
    pub size: Option<u64>,
}

/// The column the list is sorted by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SortBy {
    Name,
    Modified,
    Size,
}

/// What the dialog shows: a place of the navigation pane, or a folder.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Location {
    Recent,
    /// The drive list (Windows "This PC").
    Drives,
    Dir(PathBuf),
}

/// A place of the navigation pane.
#[derive(Clone, Debug)]
pub(super) struct PlaceLink {
    pub label: &'static str,
    pub icon: super::icons::Icon,
    pub to: Location,
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
    /// The folder's entries, folders first, in `sort` order.
    pub entries: Vec<Entry>,
    /// A problem to show (missing folder, file exists…).
    pub note: Option<String>,
    /// The name was confirmed once over an existing file.
    pub replace_ok: bool,
    /// Give the name field the keyboard focus (once, on opening).
    pub focus: bool,
    /// Places left with Back / Forward.
    pub back: Vec<Location>,
    pub forward: Vec<Location>,
    pub sort: SortBy,
    pub descending: bool,
    /// The highlighted row (its name).
    pub selected: Option<String>,
    /// The Recent place is shown instead of a folder.
    pub recent: bool,
    pub places: Vec<PlaceLink>,
    /// The list's scroll offset and visible height (last frame).
    pub view: (f32, f32),
    /// The last click / arrow key was in the list (Enter opens its folder).
    pub list_active: bool,
    /// The preview pane of the selected file.
    pub preview: PreviewSlot,
}

/// The preview pane's file, read on a worker thread.
#[derive(Clone, Default)]
pub(super) struct PreviewSlot {
    /// The file shown and its time (a new time reads it again).
    key: Option<(PathBuf, Option<std::time::SystemTime>)>,
    /// Filled by the worker.
    result: std::sync::Arc<std::sync::Mutex<Option<Result<FilePreview, String>>>>,
    texture: Option<egui::TextureHandle>,
}

impl std::fmt::Debug for PreviewSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreviewSlot")
            .field("key", &self.key)
            .field("texture", &self.texture.is_some())
            .finish()
    }
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
            back: Vec::new(),
            forward: Vec::new(),
            sort: SortBy::Name,
            descending: false,
            selected: None,
            recent: false,
            places: places(""),
            view: (0.0, 0.0),
            list_active: false,
            preview: PreviewSlot::default(),
        };
        d.refresh();
        d
    }

    /// Adds the current file's folder to the places.
    pub(super) fn with_file(mut self, current: &str) -> Self {
        self.places = places(current);
        self
    }

    /// The drive list is shown (Windows: Up from a drive's root).
    pub(super) fn at_drives(&self) -> bool {
        !self.recent && self.dir.as_os_str().is_empty()
    }

    pub(super) fn location(&self) -> Location {
        if self.recent {
            Location::Recent
        } else if self.at_drives() {
            Location::Drives
        } else {
            Location::Dir(self.dir.clone())
        }
    }

    /// Re-reads the folder (or the recent files).
    pub(super) fn refresh(&mut self) {
        self.note = None;
        if self.recent {
            self.dir_text.clear();
            // Newest first: not sorted.
            self.entries = recent_files().iter().map(|p| path_entry(p)).collect();
        } else if self.at_drives() {
            self.dir_text.clear();
            self.entries = drives()
                .into_iter()
                .map(|d| Entry {
                    name: d,
                    is_dir: true,
                    modified: None,
                    size: None,
                })
                .collect();
        } else {
            self.dir_text = self.dir.display().to_string();
            match list_entries(&self.dir, self.purpose.extensions()) {
                Ok(v) => {
                    self.entries = v;
                    sort_entries(&mut self.entries, self.sort, self.descending);
                }
                Err(e) => {
                    self.entries.clear();
                    self.note = Some(format!("Cannot read {}: {e}", self.dir.display()));
                }
            }
        }
        if let Some(s) = &self.selected
            && !self.entries.iter().any(|e| &e.name == s)
        {
            self.selected = None;
        }
    }

    /// Shows `to` without touching Back / Forward; false when it is no folder.
    fn show(&mut self, to: Location) -> bool {
        let dir = match to {
            Location::Recent => {
                self.recent = true;
                self.selected = None;
                self.refresh();
                return true;
            }
            Location::Drives => PathBuf::new(),
            Location::Dir(d) => drive_root(d),
        };
        if dir.is_dir() || (cfg!(windows) && dir.as_os_str().is_empty()) {
            self.recent = false;
            self.dir = dir;
            self.selected = None;
            self.refresh();
            true
        } else {
            self.note = Some(format!("No folder {}", dir.display()));
            false
        }
    }

    /// Goes to `to`, remembering where it came from for Back.
    pub(super) fn go_to(&mut self, to: Location) {
        let from = self.location();
        if self.show(to) && self.location() != from {
            self.back.push(from);
            self.forward.clear();
        }
    }

    fn go(&mut self, dir: PathBuf) {
        self.go_to(Location::Dir(dir));
    }

    pub(super) fn go_back(&mut self) {
        let from = self.location();
        if let Some(to) = self.back.pop()
            && self.show(to)
        {
            self.forward.push(from);
        }
    }

    pub(super) fn go_forward(&mut self) {
        let from = self.location();
        if let Some(to) = self.forward.pop()
            && self.show(to)
        {
            self.back.push(from);
        }
    }

    fn can_go_up(&self) -> bool {
        !self.recent && (self.dir.parent().is_some() || (cfg!(windows) && !self.at_drives()))
    }

    /// Up: the parent folder; from a drive's root, the drive list (Windows).
    fn up(&mut self) {
        if self.recent {
            return;
        }
        match self.dir.parent() {
            Some(p) => {
                let p = p.to_path_buf();
                self.go(p);
            }
            None if cfg!(windows) && !self.at_drives() => self.go_to(Location::Drives),
            None => {}
        }
    }

    /// Enter / Open on `name`: a folder (name or path) is entered and
    /// `None` returned; anything else is the file to use.
    pub(super) fn activate(&mut self, name: &str) -> Option<String> {
        let t = name.trim();
        if !t.is_empty() && !self.recent {
            let p = Path::new(t);
            let p = drive_root(if p.is_absolute() {
                p.to_path_buf()
            } else {
                self.dir.join(p)
            });
            // An unpacked project is a folder with a manifest: a file here.
            if p.is_dir() && !p.join("manifest.json").is_file() {
                self.go(p);
                self.name.clear();
                self.focus = true;
                return None;
            }
        }
        Some(t.to_owned())
    }

    /// Up / Down: the selection moves by `step` rows (a file also fills the name).
    fn move_selection(&mut self, step: i32) {
        if self.entries.is_empty() {
            return;
        }
        let at = self
            .selected
            .as_ref()
            .and_then(|s| self.entries.iter().position(|e| &e.name == s));
        let last = self.entries.len() as i32 - 1;
        let k = match at {
            Some(k) => (k as i32 + step).clamp(0, last),
            None if step < 0 => last,
            None => 0,
        } as usize;
        let e = &self.entries[k];
        self.selected = Some(e.name.clone());
        if !e.is_dir {
            self.name = e.name.clone();
            self.replace_ok = false;
        }
    }

    /// Sorts by `by`; the same column again reverses the order.
    fn sort_by(&mut self, by: SortBy) {
        if self.sort == by {
            self.descending = !self.descending;
        } else {
            self.sort = by;
            self.descending = false;
        }
        if !self.recent {
            sort_entries(&mut self.entries, self.sort, self.descending);
        }
    }

    /// The selected file (not a folder) and its time.
    fn selected_file(&self) -> Option<(PathBuf, Option<std::time::SystemTime>)> {
        let s = self.selected.as_ref()?;
        let e = self.entries.iter().find(|e| &e.name == s && !e.is_dir)?;
        let path = if self.recent {
            PathBuf::from(&e.name)
        } else {
            self.dir.join(&e.name)
        };
        Some((path, e.modified))
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

/// The navigation pane: Recent, the current file's folder, Desktop,
/// Documents, Downloads, Home and (Windows) This PC; missing folders are left out.
fn places(current: &str) -> Vec<PlaceLink> {
    use super::icons::Icon;
    let home = home_dir();
    let onedrive = cfg!(windows)
        .then(|| std::env::var_os("OneDrive"))
        .flatten()
        .map(PathBuf::from);
    let known = |sub: &str| {
        home.iter()
            .chain(onedrive.iter())
            .map(|h| h.join(sub))
            .find(|d| d.is_dir())
    };
    let mut v = vec![PlaceLink {
        label: "Recent",
        icon: Icon::Undo,
        to: Location::Recent,
    }];
    if let Some(p) = Path::new(current.trim())
        .parent()
        .filter(|p| !p.as_os_str().is_empty() && p.is_dir())
    {
        v.push(PlaceLink {
            label: "Project Folder",
            icon: Icon::Part,
            to: Location::Dir(p.to_path_buf()),
        });
    }
    for label in ["Desktop", "Documents", "Downloads"] {
        if let Some(d) = known(label) {
            v.push(PlaceLink {
                label,
                icon: Icon::Folder,
                to: Location::Dir(d),
            });
        }
    }
    if let Some(h) = home.clone() {
        v.push(PlaceLink {
            label: "Home",
            icon: Icon::Home,
            to: Location::Dir(h),
        });
    }
    if cfg!(windows) {
        v.push(PlaceLink {
            label: "This PC",
            icon: Icon::Ortho,
            to: Location::Drives,
        });
    }
    v
}

/// A recent file as a row (name = its full path).
fn path_entry(p: &Path) -> Entry {
    let meta = std::fs::metadata(p).ok();
    Entry {
        name: p.display().to_string(),
        is_dir: false,
        modified: meta.as_ref().and_then(|m| m.modified().ok()),
        size: meta.filter(std::fs::Metadata::is_file).map(|m| m.len()),
    }
}

/// Folders above files; within them by `by` (then by name).
pub(super) fn sort_entries(v: &mut [Entry], by: SortBy, descending: bool) {
    v.sort_by_cached_key(|e| e.name.to_lowercase());
    // Stable: equal keys keep the name order.
    v.sort_by(|a, b| {
        let o = match by {
            SortBy::Name => std::cmp::Ordering::Equal,
            SortBy::Modified => a.modified.cmp(&b.modified),
            SortBy::Size => a.size.cmp(&b.size),
        };
        b.is_dir.cmp(&a.is_dir).then(o)
    });
    if descending {
        let dirs = v.iter().take_while(|e| e.is_dir).count();
        v[..dirs].reverse();
        v[dirs..].reverse();
    }
}

/// `12 KB`, `3.4 MB` (KB rounded up, as Explorer shows them).
pub(super) fn size_text(n: u64) -> String {
    const K: u64 = 1024;
    if n < 1000 * K {
        format!("{} KB", n.div_ceil(K))
    } else if n < 1000 * K * K {
        format!("{:.1} MB", n as f64 / (K * K) as f64)
    } else {
        format!("{:.1} GB", n as f64 / (K * K * K) as f64)
    }
}

/// Local date and time, `2026-10-26 14:05`.
pub(super) fn date_text(t: std::time::SystemTime) -> String {
    chrono::DateTime::<chrono::Local>::from(t)
        .format("%Y-%m-%d %H:%M")
        .to_string()
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
    Ok(list_entries(dir, exts)?
        .into_iter()
        .map(|e| (e.name, e.is_dir))
        .collect())
}

/// [`list_dir`] with dates and sizes. Type and metadata come with the
/// directory listing (no extra file-system call per entry on Windows);
/// links are followed.
pub(super) fn list_entries(dir: &Path, exts: &[&str]) -> std::io::Result<Vec<Entry>> {
    let rd = std::fs::read_dir(dir)?;
    let mut v: Vec<Entry> = rd
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_owned();
            if is_hidden(&e, &name) {
                return None;
            }
            let meta = match e.file_type() {
                Ok(t) if t.is_symlink() => std::fs::metadata(e.path()).ok(),
                _ => e.metadata().ok(),
            };
            let is_dir = meta.as_ref().is_some_and(std::fs::Metadata::is_dir);
            let modified = meta.as_ref().and_then(|m| m.modified().ok());
            let ext_ok = Path::new(&name)
                .extension()
                .and_then(|x| x.to_str())
                .is_some_and(|x| exts.iter().any(|e| x.eq_ignore_ascii_case(e)));
            // A packed project is a file; an unpacked one a folder with a
            // manifest: listed as a project too.
            if is_dir && !(ext_ok && e.path().join("manifest.json").is_file()) {
                return Some(Entry {
                    name,
                    is_dir: true,
                    modified,
                    size: None,
                });
            }
            ext_ok.then(|| Entry {
                name,
                is_dir: false,
                modified,
                size: meta.filter(std::fs::Metadata::is_file).map(|m| m.len()),
            })
        })
        .collect();
    sort_entries(&mut v, SortBy::Name, false);
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
            DialogPurpose::ExportBom => format!("{stem}-bom.csv"),
            DialogPurpose::ExportDxf => format!("{stem}-flat.dxf"),
            DialogPurpose::ExportPdf => format!("{stem}.pdf"),
            DialogPurpose::ImportParams => String::new(),
        };
        self.file_dialog = Some(
            FileDialog::new(purpose, start_dir(&self.file_path), name).with_file(&self.file_path),
        );
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
        let screen = ctx.content_rect();
        let size = egui::vec2(980.0, 540.0).min(screen.size() * 0.9);
        let wants_keys = ctx.egui_wants_keyboard_input();
        self.layout
            .place(
                "file_window",
                screen,
                true,
                egui::Window::new(d.purpose.title())
                    .open(&mut open)
                    .collapsible(false)
                    .resizable(true)
                    .min_size(egui::vec2(640.0, 360.0))
                    .default_size(size)
                    // A centre pivot needs the size, which the first frame does not know.
                    .pivot(egui::Align2::LEFT_TOP)
                    .default_pos(screen.center() - size / 2.0),
            )
            .show(ctx, |ui| {
                let mut chosen: Option<String> = None;
                let mut enter: Option<String> = None;
                let mut go_place: Option<Location> = None;
                let mut path_enter = false;
                let (step, k_enter, k_back, k_fwd, k_parent, k_esc) = ui.input(|i| {
                    let alt = i.modifiers.alt;
                    let step = if alt {
                        0
                    } else if i.key_pressed(egui::Key::ArrowDown) {
                        1
                    } else if i.key_pressed(egui::Key::ArrowUp) {
                        -1
                    } else {
                        0
                    };
                    (
                        step,
                        i.key_pressed(egui::Key::Enter),
                        alt && i.key_pressed(egui::Key::ArrowLeft),
                        alt && i.key_pressed(egui::Key::ArrowRight),
                        (alt && i.key_pressed(egui::Key::ArrowUp))
                            || (!wants_keys && i.key_pressed(egui::Key::Backspace)),
                        i.key_pressed(egui::Key::Escape),
                    )
                });
                ui.horizontal(|ui| {
                    let back = ui.add_enabled(!d.back.is_empty(), egui::Button::new("Back"));
                    if back.on_hover_text("Back (Alt+Left)").clicked() || k_back {
                        d.go_back();
                    }
                    let fwd = ui.add_enabled(!d.forward.is_empty(), egui::Button::new("Forward"));
                    if fwd.on_hover_text("Forward (Alt+Right)").clicked() || k_fwd {
                        d.go_forward();
                    }
                    let up = ui.add_enabled(d.can_go_up(), egui::Button::new("Up"));
                    if up
                        .on_hover_text("Parent folder (Alt+Up, Backspace)")
                        .clicked()
                        || k_parent
                    {
                        d.up();
                    }
                    if ui
                        .button("Refresh")
                        .on_hover_text("Read the folder again")
                        .clicked()
                    {
                        d.refresh();
                    }
                    let hint = if d.recent {
                        "Recent files"
                    } else {
                        "Drives - type a folder"
                    };
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut d.dir_text)
                            .hint_text(hint)
                            .desired_width(f32::INFINITY),
                    );
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        path_enter = true;
                        let p = PathBuf::from(d.dir_text.trim());
                        d.go(p);
                    }
                });
                ui.separator();
                let row_h = 22.0;
                // File name, Files of type, the note and the buttons first, at the
                // bottom: the list takes what is left (sizing it from an estimate
                // made the window grow every frame).
                egui::Panel::bottom(egui::Id::new("file_dialog_bottom"))
                    .resizable(false)
                    .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(0, 6)))
                    .show(ui, |ui| {
                        let label_w = 90.0;
                        let label = |ui: &mut egui::Ui, text: &str| {
                            ui.allocate_ui_with_layout(
                                egui::vec2(label_w, row_h),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| ui.label(text),
                            );
                        };
                        ui.horizontal(|ui| {
                            label(ui, "File name");
                            let r = ui.add(
                                egui::TextEdit::singleline(&mut d.name)
                                    .desired_width(f32::INFINITY),
                            );
                            if std::mem::take(&mut d.focus) {
                                r.request_focus();
                            }
                            if r.changed() {
                                d.replace_ok = false;
                                d.list_active = false;
                            }
                        });
                        ui.horizontal(|ui| {
                            label(ui, "Files of type");
                            ui.weak(d.purpose.type_text());
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.button("Cancel").clicked() {
                                        cancel = true;
                                    }
                                    let ok = if d.purpose.saves() { "Save" } else { "Open" };
                                    if ui.button(ok).clicked() {
                                        chosen = d.activate(&d.name.clone());
                                    }
                                },
                            );
                        });
                        if let Some(n) = &d.note {
                            ui.colored_label(egui::Color32::from_rgb(220, 120, 40), n);
                        }
                    });
                let list_h = ui.available_height().max(120.0);
                ui.horizontal_top(|ui| {
                    let w = 140.0;
                    ui.allocate_ui_with_layout(
                        egui::vec2(w, list_h),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_min_size(egui::vec2(w, list_h));
                            ui.spacing_mut().item_spacing.y = 0.0;
                            let here = d.location();
                            for pl in &d.places {
                                let r =
                                    icon_row(ui, w, row_h, Some(pl.icon), pl.label, here == pl.to);
                                if r.clicked() {
                                    go_place = Some(pl.to.clone());
                                }
                            }
                        },
                    );
                    ui.separator();
                    // The preview pane keeps its width; the list takes the rest.
                    let pane_w = 240.0;
                    let list_w =
                        (ui.available_width() - pane_w - 3.0 * ui.spacing().item_spacing.x)
                            .max(200.0);
                    ui.allocate_ui_with_layout(
                        egui::vec2(list_w, list_h),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_width(list_w);
                            let (e, c) = file_list(ui, &mut d, list_h, row_h, step);
                            enter = e;
                            chosen = chosen.take().or(c);
                        },
                    );
                    ui.separator();
                    preview_pane(ui, &mut d, pane_w, list_h);
                });
                if let Some(to) = go_place {
                    d.go_to(to);
                }
                if k_esc {
                    cancel = true;
                }
                if k_enter && !path_enter && chosen.is_none() && enter.is_none() {
                    let dir = d
                        .selected
                        .as_ref()
                        .filter(|_| d.list_active)
                        .filter(|s| d.entries.iter().any(|e| &e.name == *s && e.is_dir))
                        .cloned();
                    match dir {
                        Some(n) => enter = Some(n),
                        None => chosen = d.activate(&d.name.clone()),
                    }
                }
                if let Some(n) = enter {
                    let p = d.dir.join(n);
                    d.go(p);
                    d.focus = true;
                }
                if let Some(n) = chosen {
                    d.name = n;
                    d.focus = true;
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
            DialogPurpose::ExportPdf => self.write_drawing_pdf(&path),
            DialogPurpose::ExportDxf => {
                let text = self.sm.dxf.take().unwrap_or_default();
                self.file_status = Some(
                    std::fs::write(&path, text)
                        .map(|()| format!("Flat pattern exported to {}", path.display()))
                        .map_err(|e| format!("Export Flat Pattern: {e}")),
                );
            }
            DialogPurpose::ExportBom => {
                let text = self.bom_export.take().unwrap_or_default();
                self.file_status = Some(
                    std::fs::write(&path, text)
                        .map(|()| format!("Bill of Materials exported to {}", path.display()))
                        .map_err(|e| format!("Export Bill of Materials: {e}")),
                );
            }
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
        if ui.button("New...         Ctrl+N").clicked() {
            self.open_new_file();
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
            if self.is_drawing() {
                if ui
                    .button("PDF...")
                    .on_hover_text("The drawing's sheets as a vector PDF")
                    .clicked()
                {
                    self.open_pdf_export();
                    ui.close();
                }
                return;
            }
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

/// The preview pane: the selected file's preview picture and properties
/// (`docs/spec/0.6-files.md` §3.0a), read on a worker thread once per file.
fn preview_pane(ui: &mut egui::Ui, d: &mut FileDialog, w: f32, h: f32) {
    let want = d.selected_file();
    if want != d.preview.key {
        d.preview = PreviewSlot {
            key: want.clone(),
            ..PreviewSlot::default()
        };
        if let Some((path, _)) = want {
            let slot = d.preview.result.clone();
            let ctx = ui.ctx().clone();
            let spawned = std::thread::Builder::new()
                .name("file-preview".into())
                .spawn(move || {
                    let r = read_preview(&path).map_err(|e| e.to_string());
                    *crate::diagnostics::lock(&slot) = Some(r);
                    ctx.request_repaint();
                });
            if let Err(e) = spawned {
                *crate::diagnostics::lock(&d.preview.result) = Some(Err(e.to_string()));
            }
        }
    }
    let result = crate::diagnostics::lock(&d.preview.result).clone();
    let entry = d
        .selected
        .as_ref()
        .and_then(|s| d.entries.iter().find(|e| &e.name == s && !e.is_dir))
        .cloned();
    ui.allocate_ui_with_layout(
        egui::vec2(w, h),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.set_min_size(egui::vec2(w, h));
            ui.set_max_width(w);
            let side = (w - 4.0).min(h * 0.6).max(64.0);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
            ui.painter()
                .rect_filled(rect, 4.0, ui.visuals().extreme_bg_color);
            let note = |ui: &mut egui::Ui, text: &str| {
                ui.put(
                    rect.shrink(12.0),
                    egui::Label::new(egui::RichText::new(text).weak()).wrap(),
                );
            };
            match (&d.preview.key, &result) {
                (None, _) => note(ui, "Select a file to see its preview"),
                (Some(_), None) => note(ui, "Reading..."),
                (Some(_), Some(Err(e))) => note(ui, e),
                (Some((path, _)), Some(Ok(v))) => {
                    if d.preview.texture.is_none()
                        && let Some((iw, ih, rgba)) = v
                            .thumbnail
                            .as_deref()
                            .and_then(crate::part::thumbnail::decode_png)
                    {
                        let img = egui::ColorImage::from_rgba_unmultiplied(
                            [iw as usize, ih as usize],
                            &rgba,
                        );
                        d.preview.texture = Some(ui.ctx().load_texture(
                            format!("file-preview:{}", path.display()),
                            img,
                            egui::TextureOptions::LINEAR,
                        ));
                    }
                    match &d.preview.texture {
                        Some(t) => {
                            ui.painter().image(
                                t.id(),
                                rect.shrink(4.0),
                                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                                egui::Color32::WHITE,
                            );
                        }
                        None => note(
                            ui,
                            "No preview picture: saved by an older version (save it again to add one)",
                        ),
                    }
                }
            }
            ui.add_space(6.0);
            let Some(Ok(v)) = &result else {
                return;
            };
            if d.preview.key.is_none() {
                return;
            }
            let kind = |k: DocumentKind| match k {
                DocumentKind::Part => "Part",
                DocumentKind::Assembly => "Assembly",
                DocumentKind::Drawing => "Drawing",
            };
            let mut rows: Vec<(&str, String)> = Vec::new();
            if let Some((name, k)) = &v.document {
                rows.push(("Document", format!("{name} ({})", kind(*k))));
            }
            for (key, label) in [
                ("part_number", "Part Number"),
                ("description", "Description"),
                ("title", "Title"),
                ("author", "Author"),
                ("revision", "Revision"),
            ] {
                let val = v.properties.get(key);
                if !val.is_empty() {
                    rows.push((label, val.to_owned()));
                }
            }
            rows.push((
                "Design State",
                v.properties.design_state.label().to_owned(),
            ));
            let parts = v
                .documents
                .iter()
                .filter(|d| d.1 == DocumentKind::Part)
                .count();
            let asms = v.documents.len() - parts;
            rows.push((
                "Documents",
                match asms {
                    0 => format!("{parts} part(s)"),
                    _ => format!("{parts} part(s), {asms} assembly(ies)"),
                },
            ));
            if let Some(e) = &entry {
                if let Some(t) = e.modified {
                    rows.push(("Modified", date_text(t)));
                }
                if let Some(n) = e.size {
                    rows.push(("Size", size_text(n)));
                }
            }
            rows.push(("Saved by", v.generator.clone()));
            egui::Grid::new("file_preview_props")
                .num_columns(2)
                .spacing(egui::vec2(8.0, 2.0))
                .show(ui, |ui| {
                    let value_w = (w - 100.0).max(60.0);
                    for (label, val) in rows {
                        ui.weak(label);
                        ui.allocate_ui_with_layout(
                            egui::vec2(value_w, 16.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.add(egui::Label::new(val.as_str()).truncate())
                                    .on_hover_text(&val);
                            },
                        );
                        ui.end_row();
                    }
                });
        },
    );
}

impl CadCraftApp {
    /// The preview picture saved with the project (Application Options ›
    /// Save): the active part, or every visible occurrence of the active
    /// assembly.
    pub(super) fn thumbnail_png(&self) -> Option<Vec<u8>> {
        use super::layout::ThumbnailSource;
        use crate::part::thumbnail;
        let view = match self.thumbnail_source {
            ThumbnailSource::Off => return None,
            ThumbnailSource::Iso => thumbnail::iso_view(),
            ThumbnailSource::ActiveView => self.camera.frame().0,
        };
        let started = std::time::Instant::now();
        let mut scene = thumbnail::Scene::default();
        if let Some(a) = self.assembly() {
            for o in a.occurrences.iter().filter(|o| o.visible) {
                self.with_part(o.component, |part| scene.add_part(part, Some(&o.transform)));
            }
        } else if let Some(d) = self.drawing() {
            // A drawing shows the model of its first view (the sheet itself
            // is not drawn yet).
            let model = d
                .sheets
                .iter()
                .find_map(|s| s.views.first().and_then(|v| d.model_of(s, v.id)));
            if let Some(m) = model {
                self.with_part(m, |part| scene.add_part(part, None));
            }
        } else {
            scene.add_part(self.part(), None);
        }
        let png = thumbnail::png(&scene, view, thumbnail::SIZE);
        log::info!(
            "thumbnail: {} ms, {} bytes",
            started.elapsed().as_millis(),
            png.as_ref().map_or(0, Vec::len)
        );
        png
    }
}

/// A row of the places pane: icon and label, highlighted when `on`.
fn icon_row(
    ui: &mut egui::Ui,
    w: f32,
    h: f32,
    icon: Option<super::icons::Icon>,
    text: &str,
    on: bool,
) -> egui::Response {
    let (rect, r) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click());
    let vis = ui.visuals();
    let painter = ui.painter();
    if on {
        painter.rect_filled(rect, 2.0, vis.selection.bg_fill);
    } else if r.hovered() {
        painter.rect_filled(rect, 2.0, vis.widgets.hovered.weak_bg_fill);
    }
    if let Some(i) = icon {
        let ir = egui::Rect::from_min_size(
            rect.min + egui::vec2(4.0, (h - 16.0) / 2.0),
            egui::vec2(16.0, 16.0),
        );
        super::icons::paint(painter, ir, i, &super::icons::Palette::ENABLED);
    }
    let color = if on {
        vis.selection.stroke.color
    } else {
        vis.text_color()
    };
    painter.text(
        egui::pos2(rect.min.x + 26.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        egui::TextStyle::Body.resolve(ui.style()),
        color,
    );
    r
}

/// The file dialog's list: a header that sorts, then only the visible rows.
/// Returns (folder to enter, file chosen) from a double-click.
fn file_list(
    ui: &mut egui::Ui,
    d: &mut FileDialog,
    height: f32,
    row_h: f32,
    step: i32,
) -> (Option<String>, Option<String>) {
    use super::icons::{Icon, Palette};
    ui.spacing_mut().item_spacing.y = 0.0;
    let w = ui.available_width();
    let (date_w, size_w) = (130.0, 80.0);
    let name_w = (w - date_w - size_w).max(120.0);
    let font = egui::TextStyle::Body.resolve(ui.style());
    let weak = ui.visuals().weak_text_color();
    let text = ui.visuals().text_color();
    // Header.
    let (head, _) = ui.allocate_exact_size(egui::vec2(w, row_h), egui::Sense::hover());
    let mut x = head.min.x;
    for (label, by, cw) in [
        ("Name", SortBy::Name, name_w),
        ("Date modified", SortBy::Modified, date_w),
        ("Size", SortBy::Size, size_w),
    ] {
        let rect = egui::Rect::from_min_size(egui::pos2(x, head.min.y), egui::vec2(cw, row_h));
        x += cw;
        let r = ui.interact(
            rect,
            ui.id().with(("file_head", label)),
            egui::Sense::click(),
        );
        let painter = ui.painter();
        if r.hovered() && !d.recent {
            painter.rect_filled(rect, 2.0, ui.visuals().widgets.hovered.weak_bg_fill);
        }
        let g = painter.text(
            egui::pos2(rect.min.x + 6.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            font.clone(),
            text,
        );
        if d.sort == by && !d.recent {
            // A small triangle: up = ascending.
            let c = egui::pos2(g.max.x + 8.0, rect.center().y);
            let s = if d.descending { 3.0 } else { -3.0 };
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(c.x - 4.0, c.y - s),
                    egui::pos2(c.x + 4.0, c.y - s),
                    egui::pos2(c.x, c.y + s),
                ],
                weak,
                egui::Stroke::NONE,
            ));
        }
        if r.clicked() {
            d.sort_by(by);
        }
    }
    ui.painter().hline(
        head.x_range(),
        head.max.y,
        ui.visuals().widgets.noninteractive.bg_stroke,
    );
    let mut scroll_to = None;
    if step != 0 {
        d.move_selection(step);
        d.list_active = true;
        scroll_to = d
            .selected
            .as_ref()
            .and_then(|s| d.entries.iter().position(|e| &e.name == s));
    }
    let view_h = (height - row_h - 1.0).max(row_h);
    let mut area = egui::ScrollArea::vertical()
        .id_salt("file_list")
        .max_height(view_h)
        .min_scrolled_height(view_h)
        .auto_shrink([false, false]);
    if let Some(k) = scroll_to {
        let (off, view) = d.view;
        let top = k as f32 * row_h;
        if top < off {
            area = area.vertical_scroll_offset(top);
        } else if top + row_h > off + view {
            area = area.vertical_scroll_offset(top + row_h - view);
        }
    }
    if d.entries.is_empty() {
        ui.add_space(6.0);
        ui.weak(if d.recent {
            "(no recent files)"
        } else {
            "(no folders or matching files here)"
        });
        return (None, None);
    }
    let (mut click, mut double) = (None, None);
    let out = area.show_rows(ui, row_h, d.entries.len(), |ui, range| {
        for k in range {
            let e = &d.entries[k];
            let (rect, r) = ui.allocate_exact_size(egui::vec2(w, row_h), egui::Sense::click());
            if r.double_clicked() {
                double = Some(k);
            } else if r.clicked() {
                click = Some(k);
            }
            let vis = ui.visuals();
            let painter = ui.painter();
            let sel = d.selected.as_deref() == Some(e.name.as_str());
            if sel {
                painter.rect_filled(rect, 2.0, vis.selection.bg_fill);
            } else if r.hovered() {
                painter.rect_filled(rect, 2.0, vis.widgets.hovered.weak_bg_fill);
            }
            let project = Path::new(&e.name)
                .extension()
                .and_then(|x| x.to_str())
                .is_some_and(|x| x.eq_ignore_ascii_case(FILE_EXTENSION));
            let icon = if e.is_dir && !project {
                Some(Icon::Folder)
            } else if project {
                Some(Icon::Part)
            } else {
                None
            };
            if let Some(i) = icon {
                let ir = egui::Rect::from_min_size(
                    rect.min + egui::vec2(4.0, (row_h - 16.0) / 2.0),
                    egui::vec2(16.0, 16.0),
                );
                super::icons::paint(painter, ir, i, &Palette::ENABLED);
            }
            let (label, folder) = if d.recent {
                let p = Path::new(&e.name);
                (
                    p.file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or(&e.name)
                        .to_owned(),
                    p.parent().map(|f| f.display().to_string()),
                )
            } else if d.at_drives() {
                (format!("{}\\", e.name), None)
            } else {
                (e.name.clone(), None)
            };
            let color = if sel {
                vis.selection.stroke.color
            } else {
                text
            };
            let cy = rect.center().y;
            let name_rect = egui::Rect::from_min_max(
                egui::pos2(rect.min.x + 26.0, rect.min.y),
                egui::pos2(rect.min.x + name_w - 6.0, rect.max.y),
            );
            let clip = painter.with_clip_rect(name_rect.intersect(painter.clip_rect()));
            let g = clip.text(
                egui::pos2(name_rect.min.x, cy),
                egui::Align2::LEFT_CENTER,
                label,
                font.clone(),
                color,
            );
            if let Some(f) = folder {
                clip.text(
                    egui::pos2(g.max.x + 10.0, cy),
                    egui::Align2::LEFT_CENTER,
                    f,
                    font.clone(),
                    weak,
                );
            }
            if let Some(t) = e.modified {
                painter.text(
                    egui::pos2(rect.min.x + name_w + 6.0, cy),
                    egui::Align2::LEFT_CENTER,
                    date_text(t),
                    font.clone(),
                    weak,
                );
            }
            if let Some(n) = e.size {
                painter.text(
                    egui::pos2(rect.max.x - 8.0, cy),
                    egui::Align2::RIGHT_CENTER,
                    size_text(n),
                    font.clone(),
                    weak,
                );
            }
        }
    });
    d.view = (out.state.offset.y, out.inner_rect.height());
    let (mut enter, mut chosen) = (None, None);
    if let Some(k) = click {
        let e = &d.entries[k];
        d.selected = Some(e.name.clone());
        d.list_active = true;
        if !e.is_dir {
            d.name = e.name.clone();
            d.replace_ok = false;
        }
    }
    if let Some(k) = double {
        let e = d.entries[k].clone();
        d.selected = Some(e.name.clone());
        if e.is_dir {
            enter = Some(e.name);
        } else {
            chosen = Some(e.name);
        }
    }
    (enter, chosen)
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
    fn dialog_history_typed_folders_and_sorting() {
        let root = std::env::temp_dir().join(format!("cadcraft_dialog_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("sub").join("deep")).unwrap();
        std::fs::write(root.join("big.3dcadcraft"), vec![0u8; 5000]).unwrap();
        std::fs::write(root.join("a.3dcadcraft"), b"x").unwrap();
        let mut d = FileDialog::new(DialogPurpose::Open, root.clone(), String::new());
        let names = |d: &FileDialog| d.entries.iter().map(|e| e.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&d), ["sub", "a.3dcadcraft", "big.3dcadcraft"]);
        assert_eq!(d.entries[2].size, Some(5000));
        assert!(d.entries[0].size.is_none() && d.entries[0].modified.is_some());
        // Size, then reversed: folders stay on top.
        d.sort_by(SortBy::Size);
        d.sort_by(SortBy::Size);
        assert_eq!(names(&d), ["sub", "big.3dcadcraft", "a.3dcadcraft"]);
        // A typed folder path is entered, not opened; a file name is the file.
        assert_eq!(d.activate("sub/deep"), None);
        assert_eq!(d.dir, root.join("sub/deep"));
        assert!(d.name.is_empty());
        d.go_back();
        assert_eq!(d.dir, root);
        d.go_forward();
        assert_eq!(d.dir, root.join("sub/deep"));
        d.go_back();
        assert_eq!(d.activate(" a "), Some("a".into()));
        // Arrow keys walk the rows and fill the name with files.
        d.sort_by(SortBy::Name);
        d.move_selection(1);
        d.move_selection(1);
        assert_eq!(d.selected.as_deref(), Some("a.3dcadcraft"));
        assert_eq!(d.name, "a.3dcadcraft");
        d.move_selection(-5);
        assert_eq!(d.selected.as_deref(), Some("sub"));
        // Recent and back to the folder.
        d.go_to(Location::Recent);
        assert!(d.recent && !d.can_go_up());
        d.go_back();
        assert_eq!(d.location(), Location::Dir(root.clone()));
        assert!(d.places.iter().any(|p| p.to == Location::Recent));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn sizes_read_like_explorer() {
        assert_eq!(size_text(0), "0 KB");
        assert_eq!(size_text(1), "1 KB");
        assert_eq!(size_text(15_000), "15 KB");
        assert_eq!(size_text(3 * 1024 * 1024), "3.0 MB");
        assert_eq!(
            DialogPurpose::Export(ExportFormat::Step).type_text(),
            "STEP (*.step, *.stp)"
        );
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
        assert!(d.entries.iter().all(|e| e.is_dir));
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
