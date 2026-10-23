//! Panel and window layout (Inventor: View → Windows → User Interface,
//! Clean Screen, Full Screen, Reset UI Layout) and the user settings kept
//! in `settings.json` under the keys `ui` (this layout) and `options`
//! (Application Options). Spec: `docs/spec/0.13-ui-layout.md`.
//!
//! The file is shared with other modules (`recent_files`,
//! `hints_bulgarian`): a save rewrites only our two keys, atomically, and
//! half a second after the last change (dragging a panel edge writes once).
//! Unknown keys are ignored, missing ones take their defaults field by
//! field, and a damaged file gives a warning — never a crash.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use eframe::egui::{self, Key, Rect, RichText};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::sketch_tools::InferenceSettings;
use super::{CadCraftApp, MousePreset};
use crate::part::SketchOptions;

/// Delay between the last change and the save (seconds).
pub const SAVE_DELAY: f64 = 0.5;

/// Which side of the window a docked panel sits on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    #[default]
    Left,
    Right,
}

/// A remembered floating window: outer rectangle in points.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WinRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl WinRect {
    pub fn from_rect(r: Rect) -> Self {
        Self {
            x: r.min.x,
            y: r.min.y,
            w: r.width(),
            h: r.height(),
        }
    }
    pub fn rect(self) -> Rect {
        Rect::from_min_size(egui::pos2(self.x, self.y), egui::vec2(self.w, self.h))
    }
}

/// The layout the user chose (key `ui` of `settings.json`). The defaults
/// are the layout of a first run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiSettings {
    pub show_browser: bool,
    pub show_properties: bool,
    pub show_ribbon: bool,
    pub show_status_bar: bool,
    pub show_view_cube: bool,
    pub show_triad: bool,
    /// The ribbon shows its tabs only (Minimize to Tabs).
    pub ribbon_minimized: bool,
    pub browser_side: Side,
    pub properties_side: Side,
    /// Outer widths / height chosen by dragging (`None`: the default).
    pub browser_width: Option<f32>,
    pub properties_width: Option<f32>,
    pub status_height: Option<f32>,
    /// Floating windows by key (see [`LayoutUi::place`]).
    pub windows: BTreeMap<String, WinRect>,
    /// Inner size of the main window when not maximized (points).
    pub main_size: Option<[f32; 2]>,
    pub maximized: bool,
}

/// Default widths of the side panels (today's layout).
pub const BROWSER_W: f32 = 250.0;
pub const PROPERTIES_W: f32 = 300.0;
/// Default inner size of the main window.
pub const MAIN_SIZE: [f32; 2] = [1400.0, 900.0];

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            show_browser: true,
            show_properties: true,
            show_ribbon: true,
            show_status_bar: true,
            show_view_cube: true,
            show_triad: true,
            ribbon_minimized: false,
            browser_side: Side::Left,
            properties_side: Side::Right,
            browser_width: None,
            properties_width: None,
            status_height: None,
            windows: BTreeMap::new(),
            main_size: None,
            maximized: false,
        }
    }
}

/// A part of the interface the User Interface menu shows or hides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Element {
    Browser,
    Properties,
    Ribbon,
    StatusBar,
    ViewCube,
    Triad,
}

impl Element {
    pub const ALL: [Element; 6] = [
        Element::Browser,
        Element::Properties,
        Element::Ribbon,
        Element::StatusBar,
        Element::ViewCube,
        Element::Triad,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Element::Browser => "Browser",
            Element::Properties => "Properties",
            Element::Ribbon => "Ribbon",
            Element::StatusBar => "Status Bar",
            Element::ViewCube => "ViewCube",
            Element::Triad => "Axis Triad",
        }
    }

    /// Hidden by Clean Screen.
    pub fn cleaned(self) -> bool {
        matches!(
            self,
            Element::Browser | Element::Properties | Element::Ribbon | Element::StatusBar
        )
    }
}

impl UiSettings {
    pub fn shown(&self, e: Element) -> bool {
        match e {
            Element::Browser => self.show_browser,
            Element::Properties => self.show_properties,
            Element::Ribbon => self.show_ribbon,
            Element::StatusBar => self.show_status_bar,
            Element::ViewCube => self.show_view_cube,
            Element::Triad => self.show_triad,
        }
    }

    fn flag(&mut self, e: Element) -> &mut bool {
        match e {
            Element::Browser => &mut self.show_browser,
            Element::Properties => &mut self.show_properties,
            Element::Ribbon => &mut self.show_ribbon,
            Element::StatusBar => &mut self.show_status_bar,
            Element::ViewCube => &mut self.show_view_cube,
            Element::Triad => &mut self.show_triad,
        }
    }

    /// Reset UI Layout: everything back to the defaults except the main
    /// window's own size.
    pub fn reset(&mut self) {
        let (size, max) = (self.main_size, self.maximized);
        *self = Self::default();
        self.main_size = size;
        self.maximized = max;
    }
}

/// Application Options that persist (key `options`, ROADMAP U1).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppOptions {
    pub mouse: MousePreset,
    pub invert_wheel: bool,
    pub sketch: SketchOptions,
    pub inference: InferenceSettings,
    pub show_labels: bool,
    /// Cosmetic threads drawn as a texture on their faces.
    pub thread_texture: bool,
    /// Show command alias input dialog (spec 0.15 §5.1, G6).
    pub alias_input: bool,
    /// Parameters table: text size (1 = normal) and the widths of the
    /// Name, Equation and Comment columns.
    pub param_font: f32,
    pub param_cols: [f32; 3],
    /// ViewCube → Set Current View as Home: (yaw, pitch) in degrees.
    pub home_view: Option<(f64, f64)>,
    /// Select Priority, filters (those switched off), box mode, prehighlight
    /// (`docs/spec/0.9a-selection-filters.md`).
    pub selection: crate::command::selection::SelectSettings,
    /// View → Object Visibility.
    pub object_visibility: ObjectVisibility,
    /// Registry ids of the recently run commands, newest first (alias
    /// input and command search, `commands::remember`).
    pub recent_commands: Vec<String>,
}

impl Default for AppOptions {
    fn default() -> Self {
        Self {
            mouse: MousePreset::Inventor,
            invert_wheel: false,
            sketch: SketchOptions::default(),
            inference: InferenceSettings::default(),
            show_labels: true,
            thread_texture: true,
            alias_input: false,
            param_font: 1.0,
            param_cols: [80.0, 150.0, 120.0],
            home_view: None,
            selection: Default::default(),
            object_visibility: ObjectVisibility::default(),
            recent_commands: Vec::new(),
        }
    }
}

/// View → Object Visibility: which kinds of construction geometry are
/// drawn at all (Inventor's global switches; a feature hidden on its own
/// stays hidden).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ObjectVisibility {
    /// The origin planes XY / XZ / YZ (otherwise shown only while a command
    /// picks a plane).
    pub origin_planes: bool,
    /// User work planes.
    pub work_planes: bool,
    /// User work axes.
    pub work_axes: bool,
    /// User work points.
    pub work_points: bool,
    /// UCS triads.
    pub ucs: bool,
}

impl Default for ObjectVisibility {
    fn default() -> Self {
        Self {
            origin_planes: false,
            work_planes: true,
            work_axes: true,
            work_points: true,
            ucs: true,
        }
    }
}

/// One switch of [`ObjectVisibility`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisKind {
    OriginPlanes,
    WorkPlanes,
    WorkAxes,
    WorkPoints,
    Ucs,
}

impl ObjectVisibility {
    pub fn get(&self, k: VisKind) -> bool {
        match k {
            VisKind::OriginPlanes => self.origin_planes,
            VisKind::WorkPlanes => self.work_planes,
            VisKind::WorkAxes => self.work_axes,
            VisKind::WorkPoints => self.work_points,
            VisKind::Ucs => self.ucs,
        }
    }

    pub fn toggle(&mut self, k: VisKind) {
        let v = match k {
            VisKind::OriginPlanes => &mut self.origin_planes,
            VisKind::WorkPlanes => &mut self.work_planes,
            VisKind::WorkAxes => &mut self.work_axes,
            VisKind::WorkPoints => &mut self.work_points,
            VisKind::Ucs => &mut self.ucs,
        };
        *v = !*v;
    }
}

/// A command of the User Interface menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiCmd {
    Toggle(Element),
    /// View → Object Visibility switch.
    ObjVis(VisKind),
    MinimizeRibbon,
    CleanScreen,
    FullScreen,
    ResetLayout,
}

// ---- reading and writing settings.json ------------------------------------

/// `T` from `file`, field by field: keys `T` does not know are ignored,
/// missing or unreadable ones keep their defaults. Returns whether every
/// known key was read.
pub fn lenient<T: Default + Serialize + DeserializeOwned>(file: &Value) -> (T, bool) {
    let Ok(mut base) = serde_json::to_value(T::default()) else {
        return (T::default(), false);
    };
    let mut leaves = Vec::new();
    collect_leaves(&base, file, &mut Vec::new(), &mut leaves);
    let mut all = file.is_object();
    for (path, v) in leaves {
        let mut trial = base.clone();
        if let Some(slot) = pointer_mut(&mut trial, &path) {
            *slot = v;
        }
        if serde_json::from_value::<T>(trial.clone()).is_ok() {
            base = trial;
        } else {
            all = false;
        }
    }
    match serde_json::from_value(base) {
        Ok(t) => (t, all),
        Err(_) => (T::default(), false),
    }
}

/// The values of `file` at the places `base` has: nested objects with
/// fields are walked, anything else (numbers, lists, maps that start
/// empty) is one leaf.
fn collect_leaves(
    base: &Value,
    file: &Value,
    path: &mut Vec<String>,
    out: &mut Vec<(Vec<String>, Value)>,
) {
    match (base, file) {
        (Value::Object(b), Value::Object(f)) if !b.is_empty() => {
            for (k, fv) in f {
                if let Some(bv) = b.get(k) {
                    path.push(k.clone());
                    collect_leaves(bv, fv, path, out);
                    path.pop();
                }
            }
        }
        _ => out.push((path.clone(), file.clone())),
    }
}

fn pointer_mut<'a>(v: &'a mut Value, path: &[String]) -> Option<&'a mut Value> {
    let mut cur = v;
    for k in path {
        cur = cur.get_mut(k)?;
    }
    Some(cur)
}

/// What [`load_from`] found.
#[derive(Debug, Default)]
pub struct Loaded {
    pub ui: UiSettings,
    pub options: AppOptions,
    /// The file could not be parsed: it is kept as `.bak` at the next save.
    pub damaged: bool,
    pub warning: Option<String>,
}

/// Reads `ui` and `options` from `path`. A missing file gives the defaults
/// without a warning.
pub fn load_from(path: &Path) -> Loaded {
    let mut l = Loaded::default();
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return l,
        Err(e) => {
            l.warning = Some(format!(
                "Settings: {} cannot be read ({e}); defaults are used",
                path.display()
            ));
            return l;
        }
    };
    let v: Value = match serde_json::from_str(&text) {
        Ok(v @ Value::Object(_)) => v,
        Ok(_) => {
            l.damaged = true;
            l.warning = Some(format!(
                "Settings: {} is damaged (not an object); defaults are used",
                path.display()
            ));
            return l;
        }
        Err(e) => {
            l.damaged = true;
            l.warning = Some(format!(
                "Settings: {} is damaged ({e}); defaults are used",
                path.display()
            ));
            return l;
        }
    };
    let mut bad = Vec::new();
    if let Some(u) = v.get("ui") {
        let (ui, ok) = lenient::<UiSettings>(u);
        l.ui = ui;
        if !ok {
            bad.push("ui");
        }
    }
    if let Some(o) = v.get("options") {
        let (opts, ok) = lenient::<AppOptions>(o);
        l.options = opts;
        if !ok {
            bad.push("options");
        }
    }
    if !bad.is_empty() {
        l.warning = Some(format!(
            "Settings: some values in {} ({}) were not understood; their defaults are used",
            path.display(),
            bad.join(", ")
        ));
    }
    l
}

/// Writes `ui` and `options` into `path`, keeping the file's other keys.
/// A damaged file is first moved to `settings.json.bak`. Atomic: a
/// temporary sibling is renamed over the file.
pub fn write_to(
    path: &Path,
    ui: &UiSettings,
    options: &AppOptions,
    damaged: bool,
) -> Result<(), String> {
    let mut v = if damaged {
        let bak = path.with_extension("json.bak");
        let _ = std::fs::rename(path, &bak);
        serde_json::json!({})
    } else {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .filter(Value::is_object)
            .unwrap_or_else(|| serde_json::json!({}))
    };
    v["ui"] = serde_json::to_value(ui).map_err(|e| e.to_string())?;
    v["options"] = serde_json::to_value(options).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let tmp = path.with_file_name(format!(
        ".settings.json.tmp-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {e}", path.display())
    })
}

/// Size and maximized state for the main window at start-up.
pub fn startup_window() -> ([f32; 2], bool) {
    let l = super::files::settings_path()
        .map(|p| load_from(&p))
        .unwrap_or_default();
    let s = l.ui.main_size.unwrap_or(MAIN_SIZE);
    let ok = |v: f32, lo: f32, hi: f32| if v.is_finite() { v.clamp(lo, hi) } else { lo };
    (
        [ok(s[0], 640.0, 7680.0), ok(s[1], 400.0, 4320.0)],
        l.ui.maximized,
    )
}

/// `r` moved (and shrunk if needed) to lie inside `screen`.
pub fn clamp_rect(r: Rect, screen: Rect) -> Rect {
    const MIN: egui::Vec2 = egui::vec2(80.0, 40.0);
    let finite =
        r.min.x.is_finite() && r.min.y.is_finite() && r.max.x.is_finite() && r.max.y.is_finite();
    let size = if finite {
        r.size()
    } else {
        egui::vec2(300.0, 200.0)
    };
    let size = size.max(MIN).min(screen.size());
    let size = size.max(egui::Vec2::ZERO);
    if !finite {
        return Rect::from_center_size(screen.center(), size);
    }
    let x = r
        .min
        .x
        .clamp(screen.min.x, (screen.max.x - size.x).max(screen.min.x));
    let y = r
        .min
        .y
        .clamp(screen.min.y, (screen.max.y - size.y).max(screen.min.y));
    Rect::from_min_size(egui::pos2(x, y), size)
}

// ---- debounced saving ------------------------------------------------------

/// Saves a snapshot [`SAVE_DELAY`] seconds after it last changed.
#[derive(Debug, Default)]
pub struct Saver {
    /// What the file holds (as loaded or last written).
    saved: Option<(UiSettings, AppOptions)>,
    /// The latest snapshot and when it changed.
    seen: Option<(UiSettings, AppOptions, f64)>,
    /// Number of writes (for tests and the log).
    pub writes: usize,
}

impl Saver {
    fn with_saved(ui: UiSettings, options: AppOptions) -> Self {
        Self {
            saved: Some((ui, options)),
            ..Self::default()
        }
    }

    /// Looks at the current state; returns what to write now, or how long
    /// to wait before asking again.
    pub fn tick(
        &mut self,
        ui: &UiSettings,
        options: &AppOptions,
        now: f64,
    ) -> Result<Option<(UiSettings, AppOptions)>, f64> {
        let same_seen = self
            .seen
            .as_ref()
            .is_some_and(|(u, o, _)| u == ui && o == options);
        if !same_seen {
            self.seen = Some((ui.clone(), options.clone(), now));
        }
        let saved = self
            .saved
            .as_ref()
            .is_some_and(|(u, o)| u == ui && o == options);
        if saved {
            return Ok(None);
        }
        let since = self.seen.as_ref().map_or(now, |s| s.2);
        if now - since >= SAVE_DELAY {
            self.saved = Some((ui.clone(), options.clone()));
            self.writes += 1;
            Ok(Some((ui.clone(), options.clone())))
        } else {
            Err(SAVE_DELAY - (now - since))
        }
    }

    /// At exit: what is not saved yet.
    pub fn flush(
        &mut self,
        ui: &UiSettings,
        options: &AppOptions,
    ) -> Option<(UiSettings, AppOptions)> {
        let saved = self
            .saved
            .as_ref()
            .is_some_and(|(u, o)| u == ui && o == options);
        if saved {
            return None;
        }
        self.saved = Some((ui.clone(), options.clone()));
        self.writes += 1;
        Some((ui.clone(), options.clone()))
    }
}

// ---- live layout state -----------------------------------------------------

/// Floating windows that Escape closes when the pointer is over them.
const ESCAPE_CLOSES: &[&str] = &[
    "options",
    "parameters",
    "materials",
    "appearances",
    "adjust",
    "clear_looks",
    "styles",
    "physical",
    "measure",
    "offset",
];

/// The layout of this session: the persisted [`UiSettings`] plus what is
/// not persisted (Clean Screen, Full Screen, the ribbon shown for a moment).
#[derive(Debug)]
pub struct LayoutUi {
    pub s: UiSettings,
    /// Clean Screen: only the view and the Quick Access bar.
    pub clean: bool,
    pub full_screen: bool,
    /// The minimized ribbon shows its panels until a command is picked.
    pub ribbon_peek: bool,
    /// Bumped by Reset UI Layout: new egui ids forget dragged sizes.
    generation: u64,
    path: Option<PathBuf>,
    damaged: bool,
    /// A warning for the status bar, not shown yet.
    pub warning: Option<String>,
    saver: Saver,
    /// Windows placed this frame and in the last one.
    shown: Vec<&'static str>,
    prev_shown: Vec<&'static str>,
}

impl Default for LayoutUi {
    fn default() -> Self {
        Self {
            s: UiSettings::default(),
            clean: false,
            full_screen: false,
            ribbon_peek: false,
            generation: 0,
            path: None,
            damaged: false,
            warning: None,
            saver: Saver::with_saved(UiSettings::default(), AppOptions::default()),
            shown: Vec::new(),
            prev_shown: Vec::new(),
        }
    }
}

impl LayoutUi {
    /// The layout from `path` (`None`: in memory only), and the options.
    pub fn load(path: Option<PathBuf>) -> (Self, AppOptions) {
        let l = path.as_deref().map(load_from).unwrap_or_default();
        let me = Self {
            s: l.ui.clone(),
            damaged: l.damaged,
            warning: l.warning,
            // A damaged file is rewritten at the first change only.
            saver: Saver::with_saved(l.ui, l.options.clone()),
            path,
            ..Self::default()
        };
        (me, l.options)
    }

    /// Whether `e` is on screen now (Clean Screen hides some).
    pub fn visible(&self, e: Element) -> bool {
        self.s.shown(e) && !(self.clean && e.cleaned())
    }

    /// Flips `e`; ends Clean Screen first (the toggles are what it hides).
    pub fn toggle(&mut self, e: Element) {
        let was = self.visible(e);
        self.clean = false;
        *self.s.flag(e) = !was;
    }

    pub fn reset(&mut self) {
        self.s.reset();
        self.clean = false;
        self.ribbon_peek = false;
        self.generation += 1;
    }

    /// The egui id of a docked panel (changes with Reset UI Layout).
    pub fn panel_id(&self, name: &'static str) -> egui::Id {
        egui::Id::new((name, self.generation))
    }

    /// The egui id of floating window `key`.
    pub fn window_id(&self, key: &str) -> egui::Id {
        egui::Id::new(("cadcraft_window", key, self.generation))
    }

    /// Gives floating window `w` its fixed id and, on its first show in this
    /// session, the remembered rectangle kept inside `screen`. Call it last
    /// in the builder chain. `sized`: restore the size too (resizable windows).
    pub fn place<'o>(
        &mut self,
        key: &'static str,
        screen: Rect,
        sized: bool,
        w: egui::Window<'o>,
    ) -> egui::Window<'o> {
        if !self.shown.contains(&key) {
            self.shown.push(key);
        }
        let mut w = w.id(self.window_id(key)).constrain(true);
        if let Some(r) = self.s.windows.get(key) {
            let r = clamp_rect(r.rect(), screen);
            w = w.pivot(egui::Align2::LEFT_TOP).default_pos(r.min);
            if sized {
                w = w.default_size(r.size());
            }
        }
        w
    }

    /// The floating window Escape should close: the one under the pointer,
    /// when nothing has keyboard focus.
    pub fn escape_target(&self, ctx: &egui::Context) -> Option<&'static str> {
        if !ctx.input(|i| i.key_pressed(Key::Escape)) || ctx.egui_wants_keyboard_input() {
            return None;
        }
        let layer = ctx.layer_id_at(ctx.pointer_hover_pos()?)?;
        self.prev_shown
            .iter()
            .copied()
            .find(|k| ESCAPE_CLOSES.contains(k) && self.window_id(k) == layer.id)
    }

    /// Records the rectangles of the windows placed this frame.
    fn record_windows(&mut self, ctx: &egui::Context) {
        let shown = std::mem::take(&mut self.shown);
        self.prev_shown.clear();
        for key in shown {
            if let Some(r) = ctx.memory(|m| m.area_rect(self.window_id(key))) {
                let new = WinRect::from_rect(r);
                let old = self.s.windows.get(key).copied();
                let moved = old.is_none_or(|o| {
                    (o.x - new.x).abs() > 0.5
                        || (o.y - new.y).abs() > 0.5
                        || (o.w - new.w).abs() > 0.5
                        || (o.h - new.h).abs() > 0.5
                });
                if moved {
                    self.s.windows.insert(key.to_owned(), new);
                }
            }
            self.prev_shown.push(key);
        }
    }

    /// Notes a docked panel's size after it was drawn.
    fn note(slot: &mut Option<f32>, size: f32) {
        if size.is_finite() && slot.is_none_or(|w| (w - size).abs() > 0.5) {
            *slot = Some(size);
        }
    }

    /// Debounced save of the layout and `options`.
    fn tick(&mut self, options: &AppOptions, now: f64) -> Option<f64> {
        match self.saver.tick(&self.s, options, now) {
            Ok(Some((ui, o))) => {
                self.write(&ui, &o);
                None
            }
            Ok(None) => None,
            Err(wait) => Some(wait),
        }
    }

    fn write(&mut self, ui: &UiSettings, o: &AppOptions) {
        let Some(path) = self.path.clone() else {
            return;
        };
        match write_to(&path, ui, o, self.damaged) {
            Ok(()) => self.damaged = false,
            Err(e) => self.warning = Some(format!("Settings: cannot save ({e})")),
        }
    }

    /// Saves what is pending (at exit).
    pub fn flush(&mut self, options: &AppOptions) {
        if let Some((ui, o)) = self.saver.flush(&self.s, options) {
            self.write(&ui, &o);
        }
    }
}

// ---- the application side ---------------------------------------------------

impl CadCraftApp {
    /// The persisted Application Options as they are now.
    pub(super) fn options_snapshot(&self) -> AppOptions {
        AppOptions {
            mouse: self.mouse,
            invert_wheel: self.invert_wheel,
            sketch: self.sketch_opts.clone(),
            inference: self.tools.inference,
            show_labels: self.show_labels,
            thread_texture: self.thread_texture,
            alias_input: self.alias_input,
            param_font: self.param_font,
            param_cols: self.param_cols,
            home_view: self.home_view,
            selection: self.select,
            object_visibility: self.obj_vis,
            recent_commands: self.recent_commands.clone(),
        }
    }

    /// Takes the loaded Application Options.
    pub(super) fn apply_options(&mut self, o: AppOptions) {
        self.mouse = o.mouse;
        self.invert_wheel = o.invert_wheel;
        self.sketch_opts = o.sketch;
        self.tools.inference = o.inference;
        self.show_labels = o.show_labels;
        if self.thread_texture != o.thread_texture {
            self.thread_texture = o.thread_texture;
            self.scene_dirty = true;
        }
        self.alias_input = o.alias_input;
        self.param_font = o.param_font.clamp(0.7, 2.5);
        self.param_cols = o.param_cols.map(|w| w.clamp(40.0, 600.0));
        self.home_view = o.home_view;
        self.select = o.selection;
        self.obj_vis = o.object_visibility;
        self.recent_commands = o.recent_commands;
    }

    /// Start of a frame: Ctrl+0, F11 (outside a sketch, where F11 is Relax
    /// Mode), Escape over a floating window, the load warning.
    pub(super) fn layout_begin(&mut self, ctx: &egui::Context) {
        if let Some(w) = self.layout.warning.take() {
            self.file_status = Some(Err(w));
        }
        if let Some(key) = self.layout.escape_target(ctx) {
            self.close_floating(key);
            ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        }
        if ctx.egui_wants_keyboard_input() || self.file_window_open() {
            return;
        }
        let (zero, f11, cmd) = ctx.input(|i| {
            (
                i.key_pressed(Key::Num0),
                i.key_pressed(Key::F11),
                i.modifiers.command,
            )
        });
        if cmd && zero {
            self.ui_command(UiCmd::CleanScreen);
        }
        if f11 && self.editing.is_none() {
            self.ui_command(UiCmd::FullScreen);
        }
    }

    /// End of a frame: remember window rectangles and the main window
    /// size, save when due.
    pub(super) fn layout_end(&mut self, ctx: &egui::Context) {
        self.layout.record_windows(ctx);
        let (inner, maxed, full) = ctx.input(|i| {
            let v = i.viewport();
            (v.inner_rect, v.maximized, v.fullscreen)
        });
        if let Some(f) = full {
            self.layout.full_screen = f;
        }
        if !self.layout.full_screen {
            let maxed = maxed.unwrap_or(false);
            self.layout.s.maximized = maxed;
            if let Some(r) = inner.filter(|_| !maxed) {
                let size = [r.width(), r.height()];
                let changed =
                    self.layout.s.main_size.is_none_or(|s| {
                        (s[0] - size[0]).abs() > 0.5 || (s[1] - size[1]).abs() > 0.5
                    });
                if changed && size[0] > 0.0 && size[1] > 0.0 {
                    self.layout.s.main_size = Some(size);
                }
            }
        }
        let now = ctx.input(|i| i.time);
        let options = self.options_snapshot();
        if let Some(wait) = self.layout.tick(&options, now) {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait.max(0.05)));
        }
    }

    /// Closes floating window `key` (Escape).
    fn close_floating(&mut self, key: &str) {
        match key {
            "options" => self.show_options = false,
            "parameters" => self.show_params = false,
            "offset" => self.offset_edit = None,
            "measure" => self.measure.open = false,
            other => self.mat.close_window(other),
        }
    }

    pub(super) fn ui_command(&mut self, c: UiCmd) {
        match c {
            UiCmd::Toggle(e) => self.layout.toggle(e),
            UiCmd::MinimizeRibbon => {
                self.layout.s.ribbon_minimized = !self.layout.s.ribbon_minimized;
                self.layout.ribbon_peek = false;
            }
            UiCmd::CleanScreen => self.layout.clean = !self.layout.clean,
            UiCmd::FullScreen => {
                self.layout.full_screen = !self.layout.full_screen;
                self.full_screen_cmd = Some(self.layout.full_screen);
            }
            UiCmd::ResetLayout => self.layout.reset(),
            UiCmd::ObjVis(k) => self.obj_vis.toggle(k),
        }
    }

    /// The **View** menu of the Quick Access bar (always reachable, also
    /// with the ribbon hidden or in Clean Screen).
    pub(super) fn view_menu(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("User Interface").strong());
        let mut cmd = None;
        for e in Element::ALL {
            let mut on = self.layout.visible(e);
            if ui.checkbox(&mut on, e.label()).changed() {
                cmd = Some(UiCmd::Toggle(e));
            }
        }
        let mut min = self.layout.s.ribbon_minimized;
        if ui
            .checkbox(&mut min, "Minimize Ribbon")
            .on_hover_text(
                "Tabs only; a click on a tab shows its panels (double-click the active tab)",
            )
            .changed()
        {
            cmd = Some(UiCmd::MinimizeRibbon);
        }
        ui.separator();
        let mut clean = self.layout.clean;
        if ui
            .checkbox(&mut clean, "Clean Screen (Ctrl+0)")
            .on_hover_text("Only the view and this bar")
            .changed()
        {
            cmd = Some(UiCmd::CleanScreen);
        }
        let mut full = self.layout.full_screen;
        if ui
            .checkbox(&mut full, "Full Screen (F11)")
            .on_hover_text("F11 outside a sketch; in a sketch F11 is Relax Mode")
            .changed()
        {
            cmd = Some(UiCmd::FullScreen);
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Browser");
            ui.selectable_value(&mut self.layout.s.browser_side, Side::Left, "Left");
            ui.selectable_value(&mut self.layout.s.browser_side, Side::Right, "Right");
        });
        ui.horizontal(|ui| {
            ui.label("Properties");
            ui.selectable_value(&mut self.layout.s.properties_side, Side::Left, "Left");
            ui.selectable_value(&mut self.layout.s.properties_side, Side::Right, "Right");
        });
        ui.separator();
        if ui
            .button("Reset UI Layout")
            .on_hover_text("Panels, sizes and window positions back to the defaults")
            .clicked()
        {
            cmd = Some(UiCmd::ResetLayout);
        }
        if let Some(c) = cmd {
            self.ui_command(c);
        }
    }

    /// The ribbon, status bar, browser, Properties and the view, as the
    /// layout says.
    pub(super) fn docked_panels(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let lay = &self.layout;
        let (ribbon, status, browser, props) = (
            lay.visible(Element::Ribbon),
            lay.visible(Element::StatusBar),
            lay.visible(Element::Browser),
            lay.visible(Element::Properties),
        );
        if ribbon {
            let mut action = None;
            let state = super::ribbon::State {
                labels_shown: self.show_labels,
                editing: self.editing.is_some(),
                picking: matches!(self.mode, super::Mode::PickPlane(_)),
                tool: self.tools.tool,
                construction: self.tools.construction,
                show_dof: self.show_dof,
                relax: self.relax,
                ui_shown: Element::ALL.map(|e| self.layout.visible(e)),
                obj_vis: self.obj_vis,
                clean: self.layout.clean,
                full_screen: self.layout.full_screen,
                project: self.project_geometry,
                center_point: self.tools.center_point,
            };
            let mut fold = super::ribbon::Fold {
                minimized: self.layout.s.ribbon_minimized,
                peek: self.layout.ribbon_peek,
            };
            let r = egui::Panel::top(self.layout.panel_id("ribbon")).show(ui, |ui| {
                action = super::ribbon::show(ui, &mut self.tab, &state, &mut fold);
            });
            // A click elsewhere (not in a popup of the ribbon) folds a
            // minimized ribbon back.
            let press = ui.input(|i| {
                i.pointer
                    .any_pressed()
                    .then(|| i.pointer.interact_pos())
                    .flatten()
            });
            let outside = press.is_some_and(|p| {
                !r.response.rect.contains(p)
                    && ui
                        .ctx()
                        .layer_id_at(p)
                        .is_none_or(|l| l.order == egui::Order::Background)
            });
            if fold.peek && (action.is_some() || outside) {
                fold.peek = false;
            }
            self.layout.s.ribbon_minimized = fold.minimized;
            self.layout.ribbon_peek = fold.peek;
            if let Some(a) = action {
                self.apply(a);
            }
        }
        if status {
            let mut p = egui::Panel::bottom(self.layout.panel_id("status_bar")).resizable(true);
            if let Some(h) = self.layout.s.status_height {
                p = p.default_size(h);
            }
            let r = p.show(ui, |ui| self.status_bar(ui));
            LayoutUi::note(&mut self.layout.s.status_height, r.response.rect.height());
        }
        if browser {
            let w = self.layout.s.browser_width.unwrap_or(BROWSER_W);
            let p = match self.layout.s.browser_side {
                Side::Left => egui::Panel::left(self.layout.panel_id("model_browser")),
                Side::Right => egui::Panel::right(self.layout.panel_id("model_browser")),
            };
            let r = p
                .resizable(true)
                .default_size(w)
                .show(ui, |ui| self.browser(ui));
            LayoutUi::note(&mut self.layout.s.browser_width, r.response.rect.width());
        }
        if props {
            let w = self.layout.s.properties_width.unwrap_or(PROPERTIES_W);
            let p = match self.layout.s.properties_side {
                Side::Left => egui::Panel::left(self.layout.panel_id("properties")),
                Side::Right => egui::Panel::right(self.layout.panel_id("properties")),
            };
            let r = p.resizable(true).default_size(w).show(ui, |ui| {
                // Long names wrap or are cut instead of widening the panel
                // (which moved the view under the cursor mid-command).
                ui.set_max_width(ui.available_width());
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                // Long command panels scroll (Hole, Loft…: they outgrew the
                // window).
                egui::ScrollArea::vertical()
                    .id_salt("properties_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_max_width(ui.available_width());
                        self.properties(ui)
                    })
            });
            LayoutUi::note(&mut self.layout.s.properties_width, r.response.rect.width());
        }
        // Document tabs above the view (`doc_tabs`).
        egui::Panel::top(self.layout.panel_id("doc_tabs")).show(ui, |ui| self.tab_bar(ui));
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| self.viewport(ui, frame));
        self.shift_menu_ui(ui.ctx());
        if let Some(on) = self.full_screen_cmd.take() {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Fullscreen(on));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> tempfile::TempDir {
        match tempfile::tempdir() {
            Ok(d) => d,
            Err(e) => panic!("tempdir: {e}"),
        }
    }

    #[test]
    fn defaults_are_todays_layout() {
        let s = UiSettings::default();
        for e in Element::ALL {
            assert!(s.shown(e), "{e:?} visible on a first run");
        }
        assert!(!s.ribbon_minimized);
        assert_eq!(s.browser_side, Side::Left);
        assert_eq!(s.properties_side, Side::Right);
        assert_eq!(s.browser_width.unwrap_or(BROWSER_W), 250.0);
        assert_eq!(s.properties_width.unwrap_or(PROPERTIES_W), 300.0);
        assert!(s.status_height.is_none() && s.windows.is_empty());
        let o = AppOptions::default();
        assert_eq!(o.mouse, MousePreset::Inventor);
        assert!(!o.invert_wheel && o.show_labels && o.home_view.is_none());
        assert_eq!(o.sketch, SketchOptions::default());
        assert_eq!(o.inference, InferenceSettings::default());
        let lay = LayoutUi::default();
        assert!(!lay.clean && !lay.full_screen);
    }

    fn custom() -> (UiSettings, AppOptions) {
        let mut ui = UiSettings {
            show_browser: false,
            show_view_cube: false,
            ribbon_minimized: true,
            browser_side: Side::Right,
            properties_side: Side::Left,
            browser_width: Some(321.5),
            properties_width: Some(222.0),
            status_height: Some(40.0),
            main_size: Some([1200.0, 800.0]),
            maximized: true,
            ..UiSettings::default()
        };
        ui.windows.insert(
            "parameters".into(),
            WinRect {
                x: 10.0,
                y: 20.0,
                w: 700.0,
                h: 300.0,
            },
        );
        let mut o = AppOptions {
            mouse: MousePreset::Classic,
            invert_wheel: true,
            show_labels: false,
            home_view: Some((30.0, -20.0)),
            ..AppOptions::default()
        };
        o.sketch.show_grid = false;
        o.sketch.auto_project_origin = false;
        o.inference.persist = false;
        o.inference.midpoint = false;
        (ui, o)
    }

    #[test]
    fn serde_round_trip_and_options_persist() {
        let d = tmp();
        let p = d.path().join("settings.json");
        let (ui, o) = custom();
        assert!(write_to(&p, &ui, &o, false).is_ok());
        let l = load_from(&p);
        assert_eq!(l.ui, ui);
        assert_eq!(l.options, o);
        assert!(l.warning.is_none() && !l.damaged);
        // Load → the same values in the app snapshot type.
        let (lay, opts) = LayoutUi::load(Some(p));
        assert_eq!(lay.s, ui);
        assert_eq!(opts, o);
    }

    #[test]
    fn missing_file_gives_defaults_without_warning() {
        let d = tmp();
        let l = load_from(&d.path().join("nope.json"));
        assert_eq!(l.ui, UiSettings::default());
        assert_eq!(l.options, AppOptions::default());
        assert!(l.warning.is_none() && !l.damaged);
    }

    #[test]
    fn old_and_unknown_keys() {
        let d = tmp();
        let p = d.path().join("settings.json");
        // An old file: other modules' keys, part of `ui`, a key we do not
        // know, a wrongly typed value and a partial nested struct.
        let text = r#"{
            "recent_files": ["a.3dcadcraft"],
            "hints_bulgarian": true,
            "ui": { "show_browser": false, "future_key": 3, "browser_width": "wide" },
            "options": { "invert_wheel": true, "sketch": { "show_grid": false } }
        }"#;
        assert!(std::fs::write(&p, text).is_ok());
        let l = load_from(&p);
        assert!(!l.ui.show_browser);
        assert!(l.ui.browser_width.is_none(), "bad value keeps its default");
        assert!(l.ui.show_properties);
        assert!(l.options.invert_wheel);
        assert!(!l.options.sketch.show_grid);
        assert!(
            l.options.sketch.auto_project_origin,
            "missing field: default"
        );
        assert!(l.warning.is_some(), "the bad value is reported");
        assert!(!l.damaged);
        // Saving keeps the other modules' keys.
        assert!(write_to(&p, &l.ui, &l.options, false).is_ok());
        let v: Value = match std::fs::read_to_string(&p).map(|s| serde_json::from_str(&s)) {
            Ok(Ok(v)) => v,
            _ => panic!("settings unreadable"),
        };
        assert_eq!(v["hints_bulgarian"], Value::Bool(true));
        assert_eq!(v["recent_files"][0], "a.3dcadcraft");
        assert!(v["ui"].get("future_key").is_none());
    }

    #[test]
    fn corrupt_file_warns_and_is_kept_as_bak() {
        let d = tmp();
        let p = d.path().join("settings.json");
        assert!(std::fs::write(&p, "{ not json").is_ok());
        let l = load_from(&p);
        assert!(l.damaged);
        assert!(l.warning.as_deref().is_some_and(|w| w.contains("damaged")));
        assert_eq!(l.ui, UiSettings::default());
        assert!(write_to(&p, &l.ui, &l.options, true).is_ok());
        assert!(p.with_extension("json.bak").exists());
        assert!(!load_from(&p).damaged);
        // Not an object at all.
        assert!(std::fs::write(&p, "[1, 2]").is_ok());
        assert!(load_from(&p).damaged);
    }

    #[test]
    fn window_rects_are_clamped_to_the_screen() {
        let screen = Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1000.0, 700.0));
        let inside = Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(300.0, 200.0));
        assert_eq!(clamp_rect(inside, screen), inside);
        // Off to the right and below (a bigger monitor last time).
        let off = Rect::from_min_size(egui::pos2(1800.0, 900.0), egui::vec2(300.0, 200.0));
        let c = clamp_rect(off, screen);
        assert!(screen.contains_rect(c), "{c:?}");
        assert_eq!(c.size(), off.size());
        // Larger than the screen: shrunk to it.
        let big = Rect::from_min_size(egui::pos2(-50.0, -50.0), egui::vec2(3000.0, 2000.0));
        let c = clamp_rect(big, screen);
        assert_eq!(c, screen);
        // Nonsense values.
        let nan = Rect::from_min_size(egui::pos2(f32::NAN, 0.0), egui::vec2(10.0, 10.0));
        assert!(screen.contains_rect(clamp_rect(nan, screen)));
        // Tiny ones get a minimum size.
        let tiny = Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(1.0, 1.0));
        assert!(clamp_rect(tiny, screen).width() >= 80.0);
    }

    #[test]
    fn toggles_clean_screen_and_reset() {
        let mut lay = LayoutUi::default();
        lay.toggle(Element::Browser);
        assert!(!lay.visible(Element::Browser));
        lay.toggle(Element::ViewCube);
        lay.s.browser_width = Some(400.0);
        lay.s.ribbon_minimized = true;
        lay.s.properties_side = Side::Left;
        lay.s.main_size = Some([1000.0, 700.0]);
        lay.s.windows.insert(
            "options".into(),
            WinRect {
                x: 1.0,
                y: 2.0,
                w: 3.0,
                h: 4.0,
            },
        );
        // Clean Screen hides the panels, keeps the view's cube setting, and
        // gives back exactly what was there.
        let before = lay.s.clone();
        lay.clean = true;
        assert!(!lay.visible(Element::Ribbon) && !lay.visible(Element::StatusBar));
        assert!(!lay.visible(Element::Properties));
        assert!(lay.visible(Element::Triad));
        lay.clean = false;
        assert_eq!(lay.s, before);
        // A toggle in Clean Screen ends it and flips what was seen.
        lay.clean = true;
        lay.toggle(Element::Ribbon);
        assert!(!lay.clean && lay.visible(Element::Ribbon));
        let id = lay.panel_id("model_browser");
        let wid = lay.window_id("options");
        lay.reset();
        let expect = UiSettings {
            main_size: Some([1000.0, 700.0]),
            ..UiSettings::default()
        };
        assert_eq!(lay.s, expect, "Reset keeps only the main window size");
        assert_ne!(
            lay.panel_id("model_browser"),
            id,
            "dragged sizes are forgotten"
        );
        assert_ne!(lay.window_id("options"), wid);
    }

    #[test]
    fn debounced_save_writes_once() {
        let d = tmp();
        let p = d.path().join("settings.json");
        let (mut lay, opts) = LayoutUi::load(Some(p.clone()));
        // Nothing changed: nothing written.
        assert!(lay.tick(&opts, 0.0).is_none());
        assert!(!p.exists());
        // A drag: many changes in a row.
        for k in 0..20 {
            lay.s.browser_width = Some(250.0 + k as f32);
            let wait = lay.tick(&opts, 1.0 + k as f64 * 0.02);
            assert!(wait.is_some(), "still dragging, not written");
        }
        assert!(!p.exists());
        assert!(lay.tick(&opts, 1.38 + 0.2).is_some());
        assert!(lay.tick(&opts, 1.38 + SAVE_DELAY + 0.01).is_none());
        assert_eq!(lay.saver.writes, 1);
        assert!(lay.tick(&opts, 10.0).is_none());
        assert_eq!(lay.saver.writes, 1, "written once");
        assert_eq!(load_from(&p).ui.browser_width, Some(269.0));
        // An option change, then exit before the delay: flushed.
        let mut o2 = opts.clone();
        o2.invert_wheel = true;
        assert!(lay.tick(&o2, 11.0).is_some());
        lay.flush(&o2);
        assert_eq!(lay.saver.writes, 2);
        assert!(load_from(&p).options.invert_wheel);
        lay.flush(&o2);
        assert_eq!(lay.saver.writes, 2, "nothing pending");
    }
}
