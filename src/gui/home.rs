//! Home (`docs/spec/0.17-home.md`): Inventor's start page — the active
//! design project, New / Open, the recent documents as cards or a list
//! (search, sort, filter, pins, selection) and learning links. It is a tab
//! of its own, not a document: no browser, Properties or Undo.
//!
//! Interface first (rule 15): parts whose logic is not there yet (projects
//! other than Default, the Projects editor, Open with options, column
//! configuration) are shown disabled with their spec row in the tooltip.
//! Home's own settings (view, sort, pins, Show on startup) live in
//! `settings.json` key `home`, beside `recent_files`.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};

use super::CadCraftApp;
use super::icons::{Icon, Palette};
use crate::document::FileType;

/// Cards or a table.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum View {
    #[default]
    Tiles,
    List,
}

/// What the recent list is sorted by.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Sort {
    #[default]
    LastOpened,
    Modified,
    Name,
    Location,
    Kind,
}

impl Sort {
    const ALL: [Sort; 5] = [
        Sort::LastOpened,
        Sort::Modified,
        Sort::Name,
        Sort::Location,
        Sort::Kind,
    ];

    fn label(self) -> &'static str {
        match self {
            Sort::LastOpened => "Last Opened",
            Sort::Modified => "Date Modified",
            Sort::Name => "Name",
            Sort::Location => "Location",
            Sort::Kind => "Kind",
        }
    }
}

/// Date filter of the recent list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Age {
    #[default]
    Any,
    Today,
    Week,
    Month,
    Older,
}

impl Age {
    const ALL: [Age; 5] = [Age::Any, Age::Today, Age::Week, Age::Month, Age::Older];

    fn label(self) -> &'static str {
        match self {
            Age::Any => "Any time",
            Age::Today => "Today",
            Age::Week => "This week",
            Age::Month => "This month",
            Age::Older => "Older",
        }
    }

    /// Does a file `secs` old pass?
    fn takes(self, secs: Option<u64>) -> bool {
        const DAY: u64 = 86_400;
        match (self, secs) {
            (Age::Any, _) => true,
            (_, None) => false,
            (Age::Today, Some(s)) => s < DAY,
            (Age::Week, Some(s)) => s < 7 * DAY,
            (Age::Month, Some(s)) => s < 31 * DAY,
            (Age::Older, Some(s)) => s >= 31 * DAY,
        }
    }
}

/// Persisted Home settings (`settings.json` → `home`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct HomeSettings {
    /// Application Options → General → Show Home on startup.
    pub show_on_start: bool,
    pub view: View,
    pub sort: Sort,
    pub descending: bool,
    /// Pinned recent files (stay on top, survive Remove Unpinned).
    pub pinned: Vec<PathBuf>,
}

impl Default for HomeSettings {
    fn default() -> Self {
        Self {
            show_on_start: true,
            view: View::Tiles,
            sort: Sort::LastOpened,
            descending: false,
            pinned: Vec::new(),
        }
    }
}

/// One recent file.
#[derive(Clone, Debug)]
struct Recent {
    path: PathBuf,
    /// Position in the recent list (0 = opened last).
    rank: usize,
    ty: Option<FileType>,
    modified: Option<SystemTime>,
    exists: bool,
    pinned: bool,
}

impl Recent {
    fn name(&self) -> String {
        self.path
            .file_stem()
            .map_or_else(String::new, |s| s.to_string_lossy().into_owned())
    }

    fn folder(&self) -> String {
        self.path
            .parent()
            .map_or_else(String::new, |p| p.display().to_string())
    }

    fn kind_label(&self) -> &'static str {
        self.ty.map_or("File", FileType::name)
    }

    fn icon(&self) -> Icon {
        match self.ty {
            Some(FileType::Assembly) => Icon::Assembly,
            Some(FileType::Drawing) => Icon::Drawing,
            Some(FileType::Package) => Icon::Folder,
            _ => Icon::Part,
        }
    }

    fn age_secs(&self, now: SystemTime) -> Option<u64> {
        self.modified
            .and_then(|m| now.duration_since(m).ok())
            .map(|d| d.as_secs())
    }
}

/// A decoded RGBA picture: width, height, pixels.
type Picture = (u32, u32, Vec<u8>);

/// A card's picture, read on a worker thread.
#[derive(Default)]
struct Thumb {
    /// Filled by the worker: the decoded RGBA picture, or none.
    result: Arc<Mutex<Option<Option<Picture>>>>,
    texture: Option<egui::TextureHandle>,
    done: bool,
}

/// Home's state.
#[derive(Default)]
pub(super) struct HomeUi {
    /// Home is the visible tab.
    pub shown: bool,
    /// The open documents when Home came forward (a change hides it).
    docs_sig: Option<(usize, usize, String)>,
    pub s: HomeSettings,
    loaded: bool,
    search: String,
    types: BTreeSet<&'static str>,
    age: Age,
    selected: BTreeSet<PathBuf>,
    /// The recent list as read last (`None` = read again).
    list: Option<Vec<Recent>>,
    thumbs: HashMap<PathBuf, Thumb>,
    message: Option<String>,
}

impl std::fmt::Debug for HomeUi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HomeUi")
            .field("shown", &self.shown)
            .field("s", &self.s)
            .finish()
    }
}

// ---- settings.json -----------------------------------------------------------

fn read_settings() -> serde_json::Value {
    super::files::settings_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}))
}

fn write_settings(v: &serde_json::Value) {
    let Some(p) = super::files::settings_path() else {
        return;
    };
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string_pretty(v) {
        let _ = std::fs::write(&p, text);
    }
}

/// Every recent file as stored (also missing ones), opened last first.
fn recent_paths(v: &serde_json::Value) -> Vec<PathBuf> {
    v.get("recent_files")
        .and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(PathBuf::from))
                .collect()
        })
        .unwrap_or_default()
}

/// The recent list without `drop`ped paths.
fn remove_recent(drop: &BTreeSet<PathBuf>) {
    let mut v = read_settings();
    let keep: Vec<serde_json::Value> = recent_paths(&v)
        .into_iter()
        .filter(|p| !drop.contains(p))
        .map(|p| serde_json::Value::String(p.display().to_string()))
        .collect();
    v["recent_files"] = serde_json::Value::Array(keep);
    write_settings(&v);
}

/// Sorts `v` by `sort`; pinned files stay on top.
fn sort_recent(v: &mut [Recent], sort: Sort, descending: bool) {
    v.sort_by(|a, b| {
        let by = match sort {
            Sort::LastOpened => a.rank.cmp(&b.rank),
            // Newest first is the natural order of dates.
            Sort::Modified => b.modified.cmp(&a.modified),
            Sort::Name => a.name().to_lowercase().cmp(&b.name().to_lowercase()),
            Sort::Location => a.folder().to_lowercase().cmp(&b.folder().to_lowercase()),
            Sort::Kind => a.kind_label().cmp(b.kind_label()),
        };
        let by = if descending { by.reverse() } else { by };
        b.pinned.cmp(&a.pinned).then(by)
    });
}

/// Does `r` match the search text (name, folder with the extension)?
fn matches(r: &Recent, search: &str) -> bool {
    let s = search.trim().to_lowercase();
    s.is_empty() || r.path.display().to_string().to_lowercase().contains(&s)
}

impl HomeUi {
    /// Reads the settings once.
    pub(super) fn load(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        if let Some(h) = read_settings().get("home") {
            self.s = serde_json::from_value(h.clone()).unwrap_or_default();
        }
    }

    fn save(&self) {
        let mut v = read_settings();
        if let Ok(h) = serde_json::to_value(&self.s) {
            v["home"] = h;
        }
        write_settings(&v);
    }

    /// The recent list, read from the settings when needed.
    fn recent(&mut self) -> Vec<Recent> {
        if self.list.is_none() {
            let v = read_settings();
            let list = recent_paths(&v)
                .into_iter()
                .enumerate()
                .map(|(rank, path)| {
                    let meta = std::fs::metadata(&path).ok();
                    Recent {
                        ty: FileType::from_path(&path),
                        modified: meta.as_ref().and_then(|m| m.modified().ok()),
                        exists: meta.is_some(),
                        pinned: self.s.pinned.contains(&path),
                        rank,
                        path,
                    }
                })
                .collect();
            self.list = Some(list);
        }
        self.list.clone().unwrap_or_default()
    }

    /// The thumbnail texture of `path` (started on first ask).
    fn thumb(&mut self, ctx: &egui::Context, path: &Path) -> Option<egui::TextureHandle> {
        let t = self.thumbs.entry(path.to_path_buf()).or_insert_with(|| {
            let t = Thumb::default();
            let slot = t.result.clone();
            let (p, c) = (path.to_path_buf(), ctx.clone());
            let _ = std::thread::Builder::new()
                .name("home-thumb".into())
                .spawn(move || {
                    let pic = crate::document::format::read_preview(&p)
                        .ok()
                        .and_then(|v| v.thumbnail)
                        .and_then(|png| crate::part::thumbnail::decode_png(&png));
                    *crate::diagnostics::lock(&slot) = Some(pic);
                    c.request_repaint();
                });
            t
        });
        if !t.done
            && let Some(r) = crate::diagnostics::lock(&t.result).take()
        {
            t.done = true;
            if let Some((w, h, rgba)) = r {
                let img = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
                t.texture = Some(ctx.load_texture(
                    format!("home:{}", path.display()),
                    img,
                    Default::default(),
                ));
            }
        }
        t.texture.clone()
    }
}

/// What a click on Home asks the app to do (applied after drawing).
enum Ask {
    Open(PathBuf),
    /// Open Selected: each in its own tab.
    OpenMany(Vec<PathBuf>),
    OpenDialog,
    New(super::new_file::Kind),
    NewDialog,
    Explore(PathBuf),
    Folder(PathBuf),
    Help,
}

/// A disabled control's tooltip: the spec row it waits for.
fn planned(r: egui::Response, what: &str) -> egui::Response {
    r.on_disabled_hover_text(format!("{what} — not yet (spec 0.17 / 0.6a)"))
}

impl CadCraftApp {
    /// Home comes forward (start, last document closed, its tab clicked).
    pub(super) fn show_home(&mut self) {
        self.home.load();
        self.leave_document();
        self.home.shown = true;
        self.home.list = None;
        self.home.docs_sig = Some(self.docs_sig());
    }

    /// The open documents, to notice New / Open done elsewhere.
    fn docs_sig(&self) -> (usize, usize, String) {
        (self.tabs.len(), self.active_tab, self.file_path.clone())
    }

    /// Hides Home when a document was opened or made meanwhile.
    pub(super) fn home_follow_documents(&mut self) {
        if self.home.shown && self.home.docs_sig.as_ref() != Some(&self.docs_sig()) {
            self.home.shown = false;
        }
    }

    /// The Home tab in the tab bar; a click brings Home forward.
    pub(super) fn home_tab(&mut self, ui: &mut egui::Ui) {
        let on = self.home.shown;
        let (rect, r) = ui.allocate_exact_size(egui::vec2(22.0, 20.0), egui::Sense::click());
        if on {
            ui.painter()
                .rect_filled(rect, 3.0, ui.visuals().selection.bg_fill);
        } else if r.hovered() {
            ui.painter()
                .rect_filled(rect, 3.0, ui.visuals().widgets.hovered.bg_fill);
        }
        super::icons::paint(
            ui.painter(),
            rect.shrink(2.0),
            Icon::Home,
            &Palette::ENABLED,
        );
        if r.on_hover_text("Home").clicked() && !on {
            self.show_home();
        }
        ui.separator();
    }

    /// The Home page (the central area while Home is shown).
    pub(super) fn home_page(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let mut ask = None;
        egui::Panel::left("home_nav")
            .resizable(false)
            .exact_size(230.0)
            .show(ui, |ui| self.home_nav(ui, &mut ask));
        egui::Panel::bottom("home_learn")
            .resizable(false)
            .show(ui, |ui| home_learn(ui, &mut ask));
        egui::CentralPanel::default().show(ui, |ui| self.home_recent(ui, &ctx, &mut ask));
        if let Some(a) = ask {
            self.home_apply(a, &ctx);
        }
    }

    fn home_apply(&mut self, a: Ask, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        match a {
            Ask::Open(p) => {
                self.home.shown = false;
                self.open_path(&p);
                self.home.list = None;
                if !matches!(self.file_status, Some(Ok(_))) {
                    self.show_home();
                }
            }
            Ask::OpenMany(v) => {
                self.home.shown = false;
                for p in v.iter().filter(|p| p.is_file()) {
                    self.open_path(p);
                }
                self.home.selected.clear();
                self.home.list = None;
            }
            Ask::OpenDialog => self.request(super::files::Pending::Open, now),
            Ask::NewDialog => self.open_new_file(),
            Ask::New(k) => {
                self.home.shown = false;
                self.new_file_of(k, now);
            }
            Ask::Folder(dir) => {
                self.request(super::files::Pending::Open, now);
                if let Some(d) = self.file_dialog.as_mut() {
                    d.go_to(super::files::Location::Dir(dir));
                }
            }
            Ask::Explore(p) => {
                let r = if cfg!(windows) {
                    std::process::Command::new("explorer")
                        .arg(format!("/select,{}", p.display()))
                        .spawn()
                } else {
                    std::process::Command::new("xdg-open")
                        .arg(p.parent().unwrap_or(&p))
                        .spawn()
                };
                if let Err(e) = r {
                    self.home.message = Some(format!("Show in Explorer: {e}"));
                }
            }
            Ask::Help => {
                self.home.shown = false;
                self.open_help();
            }
        }
    }

    /// Left: active project, New, Open, settings.
    fn home_nav(&mut self, ui: &mut egui::Ui, ask: &mut Option<Ask>) {
        ui.add_space(8.0);
        ui.label(RichText::new("Project").weak());
        egui::ComboBox::from_id_salt("home_project")
            .width(200.0)
            .selected_text("Default")
            .show_ui(ui, |ui| {
                let _ = ui
                    .selectable_label(true, "Default")
                    .on_hover_text("Built-in project: workspace Documents\\3DCadCraft");
                ui.separator();
                planned(
                    ui.add_enabled(false, egui::Button::new("New Project...")),
                    "New Project",
                );
                planned(
                    ui.add_enabled(false, egui::Button::new("Browse...")),
                    "Browse for a project file (.ccproj)",
                );
            })
            .response
            .on_hover_text("The active project: where files are found and saved");
        ui.add_space(12.0);
        let big = |ui: &mut egui::Ui, icon, text: &str| {
            super::ribbon::icon_button(
                ui,
                icon,
                text,
                super::ribbon::Size::Large,
                true,
                false,
                false,
            )
        };
        ui.horizontal(|ui| {
            if big(ui, Icon::Part, "New")
                .on_hover_text("Create New File (Ctrl+N)")
                .clicked()
            {
                *ask = Some(Ask::NewDialog);
            }
            ui.menu_button("v", |ui| {
                use super::new_file::Kind;
                for (k, label) in [
                    (Kind::Part, "Part"),
                    (Kind::SheetMetal, "Sheet Metal"),
                    (Kind::Assembly, "Assembly"),
                    (Kind::Drawing, "Drawing"),
                ] {
                    if ui.button(label).clicked() {
                        *ask = Some(Ask::New(k));
                        ui.close();
                    }
                }
                planned(
                    ui.add_enabled(false, egui::Button::new("Presentation")),
                    "Presentation (.ccpres)",
                );
                ui.separator();
                planned(
                    ui.add_enabled(false, egui::Button::new("Browse templates...")),
                    "Templates",
                );
            })
            .response
            .on_hover_text("New from a standard template");
            if big(ui, Icon::Folder, "Open")
                .on_hover_text("Open a file (Ctrl+O)")
                .clicked()
            {
                *ask = Some(Ask::OpenDialog);
            }
            ui.menu_button("...", |ui| {
                planned(
                    ui.add_enabled(false, egui::Button::new("Settings...")),
                    "Projects editor",
                );
            });
        });
        ui.add_space(16.0);
        let mut on = self.home.s.show_on_start;
        if ui
            .checkbox(&mut on, "Show Home on startup")
            .on_hover_text("Application Options → General")
            .changed()
        {
            self.home.s.show_on_start = on;
            self.home.save();
        }
        if let Some(m) = &self.home.message {
            ui.add_space(8.0);
            ui.label(RichText::new(m).color(Color32::from_rgb(230, 170, 60)));
        }
    }

    /// Centre: the recent documents.
    fn home_recent(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, ask: &mut Option<Ask>) {
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.heading("Recent");
            ui.add_space(12.0);
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.home.search)
                    .hint_text("Search")
                    .desired_width(180.0),
            );
            if r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.home.search.clear();
            }
            if !self.home.search.is_empty() && ui.small_button("x").on_hover_text("Clear").clicked()
            {
                self.home.search.clear();
            }
            let n = self.home.types.len() + usize::from(self.home.age != Age::Any);
            let label = if n > 0 {
                format!("Filter ({n})")
            } else {
                "Filter".to_owned()
            };
            ui.menu_button(label, |ui| {
                ui.label(RichText::new("Date modified").weak());
                for a in Age::ALL {
                    ui.radio_value(&mut self.home.age, a, a.label());
                }
                ui.separator();
                ui.label(RichText::new("File type").weak());
                for t in FileType::MODELS {
                    let mut on = self.home.types.contains(t.name());
                    if ui.checkbox(&mut on, t.filter()).changed() {
                        if on {
                            self.home.types.insert(t.name());
                        } else {
                            self.home.types.remove(t.name());
                        }
                    }
                }
                ui.separator();
                if ui.button("Clear Filters").clicked() {
                    self.home.types.clear();
                    self.home.age = Age::Any;
                }
            });
            ui.label("Sort");
            egui::ComboBox::from_id_salt("home_sort")
                .selected_text(self.home.s.sort.label())
                .show_ui(ui, |ui| {
                    for s in Sort::ALL {
                        changed |= ui
                            .selectable_value(&mut self.home.s.sort, s, s.label())
                            .changed();
                    }
                });
            let arrow = if self.home.s.descending { "v" } else { "^" };
            if ui
                .small_button(arrow)
                .on_hover_text("Ascending / descending")
                .clicked()
            {
                self.home.s.descending = !self.home.s.descending;
                changed = true;
            }
            ui.separator();
            changed |= ui
                .selectable_value(&mut self.home.s.view, View::Tiles, "Tiles")
                .changed();
            changed |= ui
                .selectable_value(&mut self.home.s.view, View::List, "List")
                .changed();
        });
        // Selection actions.
        let mut remove: BTreeSet<PathBuf> = BTreeSet::new();
        ui.horizontal(|ui| {
            let sel = !self.home.selected.is_empty();
            if sel {
                if ui.button("Open Selected").clicked() {
                    *ask = Some(Ask::OpenMany(self.home.selected.iter().cloned().collect()));
                }
                if ui.button("Remove Selected").clicked() {
                    remove.extend(self.home.selected.iter().cloned());
                }
            }
            if ui
                .button("Remove Unpinned")
                .on_hover_text("Clear the list except pinned files (the files stay on disk)")
                .clicked()
            {
                for r in self.home.recent() {
                    if !r.pinned {
                        remove.insert(r.path);
                    }
                }
            }
        });
        ui.separator();

        let now = SystemTime::now();
        let mut list: Vec<Recent> = self
            .home
            .recent()
            .into_iter()
            .filter(|r| matches(r, &self.home.search))
            .filter(|r| self.home.types.is_empty() || self.home.types.contains(r.kind_label()))
            .filter(|r| self.home.age.takes(r.age_secs(now)))
            .collect();
        sort_recent(&mut list, self.home.s.sort, self.home.s.descending);

        let mut pin: Option<PathBuf> = None;
        if self.home.recent().is_empty() {
            ui.add_space(30.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("No recent documents").heading().weak());
                ui.label("Make a new file or open one from disk.");
                ui.horizontal(|ui| {
                    if ui.button("New").clicked() {
                        *ask = Some(Ask::NewDialog);
                    }
                    if ui.button("Open").clicked() {
                        *ask = Some(Ask::OpenDialog);
                    }
                });
            });
        } else if list.is_empty() {
            ui.label(RichText::new("No file matches the search or filter.").weak());
        } else {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| match self.home.s.view {
                    View::Tiles => {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
                            for r in &list {
                                self.home_card(ui, ctx, r, ask, &mut pin, &mut remove);
                            }
                        });
                    }
                    View::List => self.home_table(ui, &list, ask, &mut pin, &mut remove),
                });
        }

        if let Some(p) = pin {
            if let Some(k) = self.home.s.pinned.iter().position(|x| *x == p) {
                self.home.s.pinned.remove(k);
            } else {
                self.home.s.pinned.push(p);
            }
            changed = true;
            self.home.list = None;
        }
        if !remove.is_empty() {
            remove_recent(&remove);
            self.home.s.pinned.retain(|p| !remove.contains(p));
            self.home.selected.retain(|p| !remove.contains(p));
            self.home.list = None;
            changed = true;
        }
        if changed {
            self.home.save();
        }
    }

    /// One card of the tile view.
    fn home_card(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        r: &Recent,
        ask: &mut Option<Ask>,
        pin: &mut Option<PathBuf>,
        remove: &mut BTreeSet<PathBuf>,
    ) {
        const W: f32 = 170.0;
        let tex = if r.exists {
            self.home.thumb(ctx, &r.path)
        } else {
            None
        };
        let frame = egui::Frame::group(ui.style()).inner_margin(6.0);
        let resp = frame
            .show(ui, |ui| {
                ui.set_width(W);
                ui.vertical(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(W, 120.0), egui::Sense::hover());
                    ui.painter()
                        .rect_filled(rect, 4.0, ui.visuals().extreme_bg_color);
                    match &tex {
                        Some(t) => {
                            let side = rect.height();
                            let img =
                                egui::Rect::from_center_size(rect.center(), egui::vec2(side, side));
                            ui.painter().image(
                                t.id(),
                                img,
                                egui::Rect::from_min_max(
                                    egui::pos2(0.0, 0.0),
                                    egui::pos2(1.0, 1.0),
                                ),
                                Color32::WHITE,
                            );
                        }
                        None => {
                            let ir =
                                egui::Rect::from_center_size(rect.center(), egui::vec2(48.0, 48.0));
                            let pal = if r.exists {
                                Palette::ENABLED
                            } else {
                                Palette::DISABLED
                            };
                            super::icons::paint(ui.painter(), ir, r.icon(), &pal);
                        }
                    }
                    ui.horizontal(|ui| {
                        let mut sel = self.home.selected.contains(&r.path);
                        if ui.checkbox(&mut sel, "").changed() {
                            if sel {
                                self.home.selected.insert(r.path.clone());
                            } else {
                                self.home.selected.remove(&r.path);
                            }
                        }
                        let mut name = RichText::new(r.name()).strong();
                        if !r.exists {
                            name = name.weak().strikethrough();
                        }
                        ui.add(egui::Label::new(name).truncate());
                    });
                    let sub = if r.exists {
                        format!("{} · {}", r.kind_label(), r.folder())
                    } else {
                        "not found".to_owned()
                    };
                    ui.add(egui::Label::new(RichText::new(sub).small().weak()).truncate());
                    ui.horizontal(|ui| {
                        let p = if r.pinned { "Unpin" } else { "Pin" };
                        if ui
                            .small_button(p)
                            .on_hover_text("Keep this file at the top of the list")
                            .clicked()
                        {
                            *pin = Some(r.path.clone());
                        }
                        ui.menu_button("...", |ui| card_menu(ui, r, ask, remove));
                    });
                });
            })
            .response
            .interact(egui::Sense::click());
        let resp = resp.on_hover_text(r.path.display().to_string());
        if resp.double_clicked() && r.exists {
            *ask = Some(Ask::Open(r.path.clone()));
        }
        resp.context_menu(|ui| card_menu(ui, r, ask, remove));
    }

    /// The list view.
    fn home_table(
        &mut self,
        ui: &mut egui::Ui,
        list: &[Recent],
        ask: &mut Option<Ask>,
        pin: &mut Option<PathBuf>,
        remove: &mut BTreeSet<PathBuf>,
    ) {
        let mut sort_by = None;
        egui::Grid::new("home_list")
            .striped(true)
            .num_columns(6)
            .spacing(egui::vec2(14.0, 4.0))
            .show(ui, |ui| {
                ui.label("");
                ui.label(RichText::new("Pinned").strong());
                for (s, h) in [
                    (Sort::Name, "Name"),
                    (Sort::Kind, "Kind"),
                    (Sort::Modified, "Date Modified"),
                    (Sort::Location, "Location"),
                ] {
                    if ui
                        .add(
                            egui::Label::new(RichText::new(h).strong()).sense(egui::Sense::click()),
                        )
                        .on_hover_text("Sort by this column")
                        .clicked()
                    {
                        sort_by = Some(s);
                    }
                }
                ui.end_row();
                for r in list {
                    let mut sel = self.home.selected.contains(&r.path);
                    if ui.checkbox(&mut sel, "").changed() {
                        if sel {
                            self.home.selected.insert(r.path.clone());
                        } else {
                            self.home.selected.remove(&r.path);
                        }
                    }
                    if ui
                        .selectable_label(r.pinned, if r.pinned { "pinned" } else { "-" })
                        .clicked()
                    {
                        *pin = Some(r.path.clone());
                    }
                    let mut name = RichText::new(r.name());
                    if !r.exists {
                        name = name.weak().strikethrough();
                    }
                    let n = ui.add(egui::Label::new(name).sense(egui::Sense::click()));
                    if n.double_clicked() && r.exists {
                        *ask = Some(Ask::Open(r.path.clone()));
                    }
                    n.context_menu(|ui| card_menu(ui, r, ask, remove));
                    ui.label(r.kind_label());
                    ui.label(r.modified.map_or_else(String::new, super::files::date_text));
                    if r.exists {
                        ui.label(shorten(&r.folder(), 70)).on_hover_text(r.folder());
                    } else {
                        ui.label("not found");
                    }
                    ui.end_row();
                }
            });
        if let Some(s) = sort_by {
            if self.home.s.sort == s {
                self.home.s.descending = !self.home.s.descending;
            } else {
                self.home.s.sort = s;
                self.home.s.descending = false;
            }
            self.home.save();
        }
    }
}

/// `s` cut in the middle to at most `n` characters (long folders).
fn shorten(s: &str, n: usize) -> String {
    let c: Vec<char> = s.chars().collect();
    if c.len() <= n {
        return s.to_owned();
    }
    let half = (n - 3) / 2;
    let head: String = c[..half].iter().collect();
    let tail: String = c[c.len() - half..].iter().collect();
    format!("{head}...{tail}")
}

/// A card's `...` / right-click menu.
fn card_menu(ui: &mut egui::Ui, r: &Recent, ask: &mut Option<Ask>, remove: &mut BTreeSet<PathBuf>) {
    if ui
        .add_enabled(r.exists, egui::Button::new("Open"))
        .clicked()
    {
        *ask = Some(Ask::Open(r.path.clone()));
        ui.close();
    }
    planned(
        ui.add_enabled(false, egui::Button::new("Open with Options...")),
        "Open with options (model states, design views)",
    );
    if ui.button("Remove from Recent").clicked() {
        remove.insert(r.path.clone());
        ui.close();
    }
    ui.separator();
    if let Some(dir) = r.path.parent().filter(|d| d.is_dir())
        && ui.button("Open Containing Folder").clicked()
    {
        *ask = Some(Ask::Folder(dir.to_path_buf()));
        ui.close();
    }
    if ui
        .add_enabled(r.exists, egui::Button::new("Show in Explorer"))
        .clicked()
    {
        *ask = Some(Ask::Explore(r.path.clone()));
        ui.close();
    }
    planned(
        ui.add_enabled(false, egui::Button::new("iProperties...")),
        "iProperties of a file that is not open",
    );
}

/// Bottom: learning and community links.
fn home_learn(ui: &mut egui::Ui, ask: &mut Option<Ask>) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Learn").strong());
        if ui.link("Help").clicked() {
            *ask = Some(Ask::Help);
        }
        ui.hyperlink_to(
            "What's New",
            "https://github.com/velog/3dCadCraft/blob/main/docs/ROADMAP.md",
        );
        ui.hyperlink_to(
            "Keyboard and mouse",
            "https://github.com/velog/3dCadCraft/blob/main/docs/REFERENCE.md",
        );
        ui.hyperlink_to("Project page", "https://github.com/velog/3dCadCraft");
        ui.hyperlink_to(
            "Report a problem",
            "https://github.com/velog/3dCadCraft/issues",
        );
    });
    ui.add_space(4.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(name: &str, rank: usize, pinned: bool, age: u64) -> Recent {
        Recent {
            path: PathBuf::from(format!("/d/{name}")),
            rank,
            ty: FileType::from_path(Path::new(name)),
            modified: SystemTime::UNIX_EPOCH.checked_add(std::time::Duration::from_secs(age)),
            exists: true,
            pinned,
        }
    }

    #[test]
    fn pinned_files_stay_on_top_in_every_order() {
        let mut v = vec![
            rec("b.ccpart", 0, false, 30),
            rec("a.ccasm", 1, false, 10),
            rec("c.ccdrw", 2, true, 20),
        ];
        for sort in Sort::ALL {
            for desc in [false, true] {
                sort_recent(&mut v, sort, desc);
                assert_eq!(v[0].name(), "c", "{sort:?} {desc}");
            }
        }
        sort_recent(&mut v, Sort::Name, false);
        assert_eq!(v[1].name(), "a");
        sort_recent(&mut v, Sort::LastOpened, false);
        assert_eq!(v[1].name(), "b");
        sort_recent(&mut v, Sort::Modified, false);
        assert_eq!(v[1].name(), "b", "newest first");
    }

    #[test]
    fn search_and_age_filter() {
        let r = rec("Bracket.ccpart", 0, false, 0);
        assert!(matches(&r, "brack"));
        assert!(matches(&r, ".CCPART"));
        assert!(!matches(&r, "shaft"));
        assert!(Age::Today.takes(Some(10)));
        assert!(!Age::Today.takes(Some(90_000)));
        assert!(Age::Older.takes(Some(40 * 86_400)));
        assert!(!Age::Week.takes(None));
        assert!(Age::Any.takes(None));
    }

    #[test]
    fn long_folders_are_cut_in_the_middle() {
        assert_eq!(shorten("C:/a", 70), "C:/a");
        let s = shorten(&"ab".repeat(25), 11);
        assert_eq!(s.chars().count(), 11);
        assert!(s.contains("..."));
    }

    #[test]
    fn settings_default_and_partial() {
        let s: HomeSettings = serde_json::from_str(r#"{"view":"List"}"#).unwrap();
        assert_eq!(s.view, View::List);
        assert!(s.show_on_start);
        assert_eq!(s.sort, Sort::LastOpened);
    }
}
