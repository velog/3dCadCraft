//! **Presets** of feature commands (`docs/spec/0.11-presets.md`): named
//! sets of a command's settings (Extrude distance and extent, Fillet
//! radius, Shell thickness…) that fill its panel in one click.
//!
//! Presets are **user settings, never document data**: each command keeps
//! one JSON file (`<Command>.json`) in the settings folder
//! (`%APPDATA%\3dcadcraft\presets` on Windows, `~/.config/3dcadcraft/presets`
//! elsewhere). A feature only records the *name* of the preset it was made
//! with ([`PresetData::preset_name`]).
//!
//! What a preset holds is the command's feature definition serialized to
//! JSON with only the setting keys kept ([`PresetData::KEYS`]): the inputs
//! (profiles, edges, faces, axes, bodies) are left out. Numeric fields with
//! an equation keep the typed expression text (`params`), re-evaluated in
//! the document the preset is applied to ([`apply_in`]).
//!
//! **Adding presets to a command** (Hole, Pattern…): implement
//! [`PresetData`] for its feature type (keys + [`PresetData::strip_inputs`]
//! for inputs hidden inside a setting), add its built-ins to [`builtins`]
//! and its starting values to [`factory`]; the GUI side is four hooks in
//! `gui/presets.rs`.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::part::{
    AxisRef, BlendFeature, BlendKind, ChamferMethod, DraftFeature, Extent, ExtentDirection,
    ExtrudeFeature, Feature, Operation, PartDocument, RevolveExtent, RevolveFeature,
    ShellDirection, ShellFeature,
};

/// The settings of a preset: a JSON object of setting keys.
pub type Settings = Map<String, Value>;

/// The dropdown entry for the factory values.
pub const NO_PRESET: &str = "No Preset";
/// The dropdown entry for the values used last.
pub const LAST_USED: &str = "Last Used";

/// The key of the field equations inside [`Settings`] (field → expression).
const PARAMS: &str = "params";

/// A feature definition that presets can be taken from and applied to.
pub trait PresetData: Clone + Serialize + DeserializeOwned {
    /// Top-level keys of the serialized definition that are settings
    /// (everything else is an input or an identity and is never stored).
    const KEYS: &'static [&'static str];

    /// Removes inputs that hide inside a setting (e.g. an extent `To` a
    /// face) or equations of fields that are inputs.
    fn strip_inputs(_settings: &mut Settings) {}

    /// The preset the feature was made with.
    fn preset_name(&self) -> Option<&str>;

    /// Records the preset the feature is made with.
    fn set_preset_name(&mut self, name: Option<String>);

    /// The definition as a feature (for evaluating field equations).
    fn to_feature(&self) -> Feature;

    /// The definition back from a feature of its kind.
    fn from_feature(f: Feature) -> Option<Self>;

    /// The settings of this definition, inputs left out.
    fn capture(&self) -> Settings {
        let Ok(Value::Object(all)) = serde_json::to_value(self) else {
            return Settings::new();
        };
        let mut s = Settings::new();
        for k in Self::KEYS {
            // A key left out by `skip_serializing_if` holds its default:
            // stored as `null`, which applies as "the default".
            s.insert((*k).to_owned(), all.get(*k).cloned().unwrap_or(Value::Null));
        }
        if let Some(Value::Object(p)) = all.get(PARAMS) {
            let exprs: Settings = p
                .iter()
                .filter_map(|(field, v)| {
                    let e = v.get("expr")?.as_str()?;
                    Some((field.clone(), Value::String(e.to_owned())))
                })
                .collect();
            if !exprs.is_empty() {
                s.insert(PARAMS.into(), Value::Object(exprs));
            }
        }
        Self::strip_inputs(&mut s);
        s
    }

    /// Sets the stored settings (only [`Self::KEYS`]; unknown keys are
    /// ignored). Fields without a stored equation lose theirs; parameter
    /// names stay. On error (a value that does not fit) nothing changes.
    fn apply(&mut self, settings: &Settings) -> Result<(), String> {
        let mut all = match serde_json::to_value(&*self) {
            Ok(Value::Object(m)) => m,
            Ok(_) => return Err("the definition is not an object".into()),
            Err(e) => return Err(e.to_string()),
        };
        for (k, v) in settings {
            if k != PARAMS && Self::KEYS.contains(&k.as_str()) {
                if v.is_null() {
                    all.remove(k);
                } else {
                    all.insert(k.clone(), v.clone());
                }
            }
        }
        let exprs = match settings.get(PARAMS) {
            Some(Value::Object(m)) => m.clone(),
            _ => Map::new(),
        };
        let params = all
            .entry(PARAMS)
            .or_insert_with(|| Value::Object(Map::new()));
        if let Value::Object(p) = params {
            for v in p.values_mut() {
                if let Value::Object(e) = v {
                    e.remove("expr");
                }
            }
            for (field, e) in exprs {
                if !e.is_string() {
                    continue;
                }
                let entry = p.entry(field).or_insert_with(|| {
                    let mut o = Map::new();
                    o.insert("name".into(), Value::String(String::new()));
                    Value::Object(o)
                });
                if let Value::Object(o) = entry {
                    o.insert("expr".into(), e);
                }
            }
        }
        let next: Self = serde_json::from_value(Value::Object(all))
            .map_err(|e| format!("the preset does not fit this command ({e})"))?;
        *self = next;
        Ok(())
    }

    /// Whether the definition's settings differ from `settings` (the
    /// asterisk of a modified preset).
    fn differs_from(&self, settings: &Settings) -> bool {
        let mut c = self.clone();
        c.apply(settings).is_err() || c.capture() != self.capture()
    }
}

/// Applies `settings` to `def` in document `part`: the stored values, then
/// the stored equations evaluated against the document's parameters. An
/// equation that cannot be evaluated there (unknown parameter) is dropped
/// and its stored value kept; those come back as warnings. On error nothing
/// changes.
pub fn apply_in<T: PresetData>(
    part: &PartDocument,
    def: &mut T,
    settings: &Settings,
) -> Result<Vec<String>, String> {
    let exprs: Vec<(String, String)> = match settings.get(PARAMS) {
        Some(Value::Object(m)) => m
            .iter()
            .filter_map(|(f, e)| Some((f.clone(), e.as_str()?.to_owned())))
            .collect(),
        _ => Vec::new(),
    };
    let mut trial = def.clone();
    trial.apply(settings)?;
    let mut warnings = Vec::new();
    let mut good = Vec::new();
    for (field, expr) in exprs {
        let mut f = trial.to_feature();
        match part.apply_draft_field(&mut f, &field, &expr) {
            Ok(()) => good.push((field, expr)),
            Err(e) => warnings.push(format!("{field} = {expr}: {e}; the stored value is used")),
        }
    }
    if !warnings.is_empty() {
        // Again without the equations that failed.
        let mut s = settings.clone();
        if let Some(Value::Object(p)) = s.get_mut(PARAMS) {
            p.retain(|f, _| good.iter().any(|(g, _)| g == f));
        }
        trial = def.clone();
        trial.apply(&s)?;
    }
    for (field, expr) in good {
        let mut f = trial.to_feature();
        if part.apply_draft_field(&mut f, &field, &expr).is_ok()
            && let Some(t) = T::from_feature(f)
        {
            trial = t;
        }
    }
    *def = trial;
    Ok(warnings)
}

impl PresetData for ExtrudeFeature {
    const KEYS: &'static [&'static str] = &[
        "extent",
        "distance",
        "distance_b",
        "direction",
        "taper",
        "taper_b",
        "operation",
        "output",
    ];

    /// `To` a face / vertex is an input (Distance, To Next and Through All
    /// are settings).
    fn strip_inputs(s: &mut Settings) {
        if s.get("extent").is_some_and(Value::is_object) {
            s.remove("extent");
        }
    }

    fn preset_name(&self) -> Option<&str> {
        self.preset.as_deref()
    }

    fn set_preset_name(&mut self, name: Option<String>) {
        self.preset = name;
    }

    fn to_feature(&self) -> Feature {
        Feature::Extrude(self.clone())
    }

    fn from_feature(f: Feature) -> Option<Self> {
        match f {
            Feature::Extrude(e) => Some(e),
            _ => None,
        }
    }
}

impl PresetData for RevolveFeature {
    const KEYS: &'static [&'static str] = &[
        "extent",
        "angle",
        "angle_b",
        "direction",
        "operation",
        "output",
    ];

    fn preset_name(&self) -> Option<&str> {
        self.preset.as_deref()
    }

    fn set_preset_name(&mut self, name: Option<String>) {
        self.preset = name;
    }

    fn to_feature(&self) -> Feature {
        Feature::Revolve(self.clone())
    }

    fn from_feature(f: Feature) -> Option<Self> {
        match f {
            Feature::Revolve(r) => Some(r),
            _ => None,
        }
    }
}

impl PresetData for BlendFeature {
    const KEYS: &'static [&'static str] = &["kind", "fillet_type", "smooth", "cone_corners"];

    /// The radii of further edge sets and the partial chamfer belong to
    /// their inputs.
    fn strip_inputs(s: &mut Settings) {
        if let Some(Value::Object(p)) = s.get_mut(PARAMS) {
            p.retain(|f, _| !matches!(f.as_str(), "to_start" | "partial_length"));
            p.retain(|f, _| f == "radius" || !f.starts_with("radius"));
        }
    }

    fn preset_name(&self) -> Option<&str> {
        self.preset.as_deref()
    }

    fn set_preset_name(&mut self, name: Option<String>) {
        self.preset = name;
    }

    fn to_feature(&self) -> Feature {
        Feature::Blend(self.clone())
    }

    fn from_feature(f: Feature) -> Option<Self> {
        match f {
            Feature::Blend(b) => Some(b),
            _ => None,
        }
    }
}

impl PresetData for ShellFeature {
    const KEYS: &'static [&'static str] = &["thickness", "direction", "chain"];

    fn preset_name(&self) -> Option<&str> {
        self.preset.as_deref()
    }

    fn set_preset_name(&mut self, name: Option<String>) {
        self.preset = name;
    }

    fn to_feature(&self) -> Feature {
        Feature::Shell(self.clone())
    }

    fn from_feature(f: Feature) -> Option<Self> {
        match f {
            Feature::Shell(s) => Some(s),
            _ => None,
        }
    }
}

impl PresetData for DraftFeature {
    const KEYS: &'static [&'static str] = &["draft_type", "angle", "flip", "chain"];

    fn preset_name(&self) -> Option<&str> {
        self.preset.as_deref()
    }

    fn set_preset_name(&mut self, name: Option<String>) {
        self.preset = name;
    }

    fn to_feature(&self) -> Feature {
        Feature::Draft(self.clone())
    }

    fn from_feature(f: Feature) -> Option<Self> {
        match f {
            Feature::Draft(d) => Some(d),
            _ => None,
        }
    }
}

/// The commands with presets (Inventor's names).
pub const COMMANDS: &[&str] = &[
    "Extrude",
    "Revolve",
    "Fillet",
    "Chamfer",
    "Shell",
    "Face Draft",
];

fn without(mut s: Settings, key: &str) -> Settings {
    s.remove(key);
    s
}

/// The values a command starts with without a preset (**No Preset**). For
/// Extrude / Revolve the Boolean operation is left out: it stays the
/// predicted one (Join with a body, New Solid without).
pub fn factory(command: &str) -> Option<Settings> {
    Some(match command {
        "Extrude" => without(ExtrudeFeature::new(Vec::new(), 10.0).capture(), "operation"),
        "Revolve" => without(
            RevolveFeature::new(Vec::new(), AxisRef::Origin(0)).capture(),
            "operation",
        ),
        "Fillet" => BlendFeature::fillet(Vec::new(), 2.0).capture(),
        "Chamfer" => BlendFeature::chamfer(Vec::new(), 1.0).capture(),
        "Shell" => ShellFeature::new(2.0).capture(),
        "Face Draft" => DraftFeature::new(1f64.to_radians()).capture(),
        _ => return None,
    })
}

/// The built-in (read-only, metric) presets of a command.
pub fn builtins(command: &str) -> Vec<(String, Settings)> {
    use std::f64::consts::PI;
    let named = |n: &str, s: Settings| (n.to_owned(), s);
    match command {
        "Extrude" => {
            let plain = ExtrudeFeature::new(Vec::new(), 10.0);
            let mut cut = plain.clone();
            cut.operation = Operation::Cut;
            cut.extent = Extent::ThroughAll;
            let mut sym = ExtrudeFeature::new(Vec::new(), 20.0);
            sym.direction = ExtentDirection::Symmetric;
            vec![
                named("Extrude 10 mm", without(plain.capture(), "operation")),
                named("Cut Through All", cut.capture()),
                named("Symmetric 20 mm", without(sym.capture(), "operation")),
            ]
        }
        "Revolve" => {
            let full = RevolveFeature::new(Vec::new(), AxisRef::Origin(0));
            let mut half = full.clone();
            half.extent = RevolveExtent::Angle;
            half.angle = PI;
            half.direction = ExtentDirection::Symmetric;
            vec![
                named("Revolve 360°", without(full.capture(), "operation")),
                named(
                    "Revolve 180° Symmetric",
                    without(half.capture(), "operation"),
                ),
            ]
        }
        "Fillet" => [1.0, 2.0, 5.0]
            .iter()
            .map(|r| {
                named(
                    &format!("Fillet {r} mm"),
                    BlendFeature::fillet(Vec::new(), *r).capture(),
                )
            })
            .collect(),
        "Chamfer" => {
            let mut angled = BlendFeature::chamfer(Vec::new(), 0.5);
            angled.kind = BlendKind::Chamfer {
                method: ChamferMethod::DistanceAngle,
                distance: 0.5,
                distance2: 0.5,
                angle: PI / 4.0,
                flip: false,
            };
            vec![
                named("Chamfer 0.5 x 45°", angled.capture()),
                named(
                    "Chamfer 1 mm",
                    BlendFeature::chamfer(Vec::new(), 1.0).capture(),
                ),
            ]
        }
        "Shell" => {
            let mut out = ShellFeature::new(1.0);
            out.direction = ShellDirection::Outside;
            vec![
                named("Shell 2 mm Inside", ShellFeature::new(2.0).capture()),
                named("Shell 1 mm Outside", out.capture()),
            ]
        }
        "Face Draft" => [1.0f64, 3.0]
            .iter()
            .map(|a| {
                named(
                    &format!("Draft {a}°"),
                    DraftFeature::new(a.to_radians()).capture(),
                )
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// A saved preset.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    /// Its name (unique per command).
    pub name: String,
    /// The settings.
    pub settings: Settings,
    /// Tick of creation (for Sort by Date created).
    #[serde(default)]
    pub created: u64,
    /// Tick of the last change (Last modified).
    #[serde(default)]
    pub modified: u64,
    /// Tick of the last use (Recently Used).
    #[serde(default)]
    pub used: u64,
    /// Built-in (read-only, never written to the file).
    #[serde(skip)]
    pub builtin: bool,
}

/// What a new command starts with (Inventor's **New <Command> Default**).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum NewDefault {
    /// The values used last.
    #[default]
    LastUsed,
    /// The preset chosen last.
    UseCurrent,
    /// The factory values.
    NoPreset,
}

/// Order of the presets in the dropdown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortOrder {
    /// Most recently used first.
    #[default]
    RecentlyUsed,
    /// Oldest first.
    DateCreated,
    /// Numbers first, then letters (case-insensitive, numbers by value).
    Alphabetical,
    /// Most recently changed first.
    LastModified,
}

/// The dropdown's current entry.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Active {
    /// Factory values.
    #[default]
    NoPreset,
    /// The values used last.
    LastUsed,
    /// A preset by name (user or built-in).
    Named(String),
}

fn yes() -> bool {
    true
}

/// The presets of one command (the content of its file).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CommandPresets {
    /// The command (Inventor's name).
    #[serde(default)]
    pub command: String,
    /// The user's presets.
    #[serde(default)]
    pub presets: Vec<Preset>,
    /// The values of the last OK / Apply (Last Used).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used: Option<Settings>,
    /// The preset chosen last (Use Current Preset).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<String>,
    /// What a new command starts with.
    #[serde(default)]
    pub new_default: NewDefault,
    /// Dropdown order.
    #[serde(default)]
    pub sort: SortOrder,
    /// Show the built-in presets.
    #[serde(default = "yes")]
    pub show_builtins: bool,
    /// Event counter behind `created` / `modified` / `used`.
    #[serde(default)]
    pub clock: u64,
}

impl CommandPresets {
    /// No presets yet for `command`.
    pub fn new(command: &str) -> Self {
        Self {
            command: command.to_owned(),
            presets: Vec::new(),
            last_used: None,
            current: None,
            new_default: NewDefault::LastUsed,
            sort: SortOrder::RecentlyUsed,
            show_builtins: true,
            clock: 0,
        }
    }

    fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    /// The built-in presets.
    pub fn builtin(&self) -> Vec<Preset> {
        builtins(&self.command)
            .into_iter()
            .map(|(name, settings)| Preset {
                name,
                settings,
                created: 0,
                modified: 0,
                used: 0,
                builtin: true,
            })
            .collect()
    }

    /// A preset by name: the user's first, then the built-ins.
    pub fn find(&self, name: &str) -> Option<Preset> {
        self.presets
            .iter()
            .find(|p| p.name == name)
            .cloned()
            .or_else(|| self.builtin().into_iter().find(|p| p.name == name))
    }

    /// Whether `name` is a built-in preset (and not a user one).
    pub fn is_builtin(&self, name: &str) -> bool {
        self.find(name).is_some_and(|p| p.builtin)
    }

    /// The dropdown's presets: the user's in the chosen order, then the
    /// built-ins (if shown).
    pub fn list(&self) -> Vec<Preset> {
        let mut v = self.presets.clone();
        match self.sort {
            SortOrder::RecentlyUsed => v.sort_by_key(|p| std::cmp::Reverse(p.used)),
            SortOrder::DateCreated => v.sort_by_key(|p| p.created),
            SortOrder::Alphabetical => v.sort_by(|a, b| natural_cmp(&a.name, &b.name)),
            SortOrder::LastModified => v.sort_by_key(|p| std::cmp::Reverse(p.modified)),
        }
        if self.show_builtins {
            v.extend(self.builtin());
        }
        v
    }

    /// The name offered for a new preset: `<Command> 1`, `2`…
    pub fn default_name(&self) -> String {
        (1..)
            .map(|k| format!("{} {k}", self.command))
            .find(|n| self.find(n).is_none())
            .unwrap_or_default()
    }

    /// A valid new name (trimmed), `except` being the preset renamed.
    fn check_name(&self, name: &str, except: Option<&str>) -> Result<String, String> {
        let n = name.trim();
        if n.is_empty() {
            return Err("a preset needs a name".into());
        }
        if n == NO_PRESET || n == LAST_USED {
            return Err(format!("'{n}' is reserved"));
        }
        if Some(n) != except && self.find(n).is_some() {
            return Err(format!("a preset named '{n}' already exists"));
        }
        Ok(n.to_owned())
    }

    fn user_mut(&mut self, name: &str) -> Result<&mut Preset, String> {
        if self.is_builtin(name) {
            return Err(format!(
                "'{name}' is a built-in preset and cannot be changed"
            ));
        }
        self.presets
            .iter_mut()
            .find(|p| p.name == name)
            .ok_or_else(|| format!("no preset named '{name}'"))
    }

    /// **Create new preset**: `settings` under a new name; it becomes the
    /// current one. Returns the name used.
    pub fn create(&mut self, name: &str, settings: Settings) -> Result<String, String> {
        let name = self.check_name(name, None)?;
        let t = self.tick();
        self.presets.push(Preset {
            name: name.clone(),
            settings,
            created: t,
            modified: t,
            used: t,
            builtin: false,
        });
        self.current = Some(name.clone());
        Ok(name)
    }

    /// **Save Current**: overwrites user preset `name`.
    pub fn save_current(&mut self, name: &str, settings: Settings) -> Result<(), String> {
        let t = self.tick();
        let p = self.user_mut(name)?;
        p.settings = settings;
        p.modified = t;
        Ok(())
    }

    /// **Rename Current**. Returns the new name.
    pub fn rename(&mut self, old: &str, new: &str) -> Result<String, String> {
        self.user_mut(old)?;
        let new = self.check_name(new, Some(old))?;
        let t = self.tick();
        let p = self.user_mut(old)?;
        p.name = new.clone();
        p.modified = t;
        if self.current.as_deref() == Some(old) {
            self.current = Some(new.clone());
        }
        Ok(new)
    }

    /// **Delete Current**.
    pub fn delete(&mut self, name: &str) -> Result<(), String> {
        self.user_mut(name)?;
        self.presets.retain(|p| p.name != name);
        if self.current.as_deref() == Some(name) {
            self.current = None;
        }
        Ok(())
    }

    /// Preset `name` was chosen / used: it becomes the current one.
    pub fn mark_used(&mut self, name: &str) {
        let t = self.tick();
        if let Some(p) = self.presets.iter_mut().find(|p| p.name == name) {
            p.used = t;
        }
        if self.find(name).is_some() {
            self.current = Some(name.to_owned());
        }
    }

    /// OK / Apply of a new feature with `settings` (Last Used).
    pub fn record_last_used(&mut self, settings: Settings) {
        self.last_used = Some(settings);
    }

    /// The settings of a dropdown entry (`None`: no values, e.g. Last
    /// Used before the first use).
    pub fn settings_of(&self, active: &Active) -> Option<Settings> {
        match active {
            Active::NoPreset => factory(&self.command),
            Active::LastUsed => self.last_used.clone(),
            Active::Named(n) => self.find(n).map(|p| p.settings),
        }
    }

    /// What a new command starts with: the dropdown entry and the values to
    /// apply (`None`: keep the command's own defaults).
    pub fn start(&self) -> (Active, Option<Settings>) {
        match self.new_default {
            NewDefault::LastUsed => match &self.last_used {
                Some(s) => (Active::LastUsed, Some(s.clone())),
                None => (Active::NoPreset, None),
            },
            NewDefault::UseCurrent => match self.current.as_deref().and_then(|n| self.find(n)) {
                Some(p) => (Active::Named(p.name), Some(p.settings)),
                None => (Active::NoPreset, None),
            },
            NewDefault::NoPreset => (Active::NoPreset, None),
        }
    }
}

/// Compares names "naturally": digit runs by value, the rest
/// case-insensitively; digits sort before letters.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    fn chunks(s: &str) -> Vec<(bool, String)> {
        let mut v: Vec<(bool, String)> = Vec::new();
        for c in s.chars() {
            let d = c.is_ascii_digit();
            match v.last_mut() {
                Some((kind, text)) if *kind == d => text.push(c),
                _ => v.push((d, c.to_string())),
            }
        }
        v
    }
    let (ca, cb) = (chunks(a), chunks(b));
    for (x, y) in ca.iter().zip(&cb) {
        let o = match (x.0, y.0) {
            (true, true) => {
                let (tx, ty) = (x.1.trim_start_matches('0'), y.1.trim_start_matches('0'));
                tx.len().cmp(&ty.len()).then_with(|| tx.cmp(ty))
            }
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (false, false) => x.1.to_lowercase().cmp(&y.1.to_lowercase()),
        };
        if o != Ordering::Equal {
            return o;
        }
    }
    ca.len().cmp(&cb.len()).then_with(|| a.cmp(b))
}

/// The user's settings folder: `%APPDATA%\3dcadcraft` on Windows, else
/// `~/.config/3dcadcraft` (`settings.json` lives there too).
pub fn config_dir() -> Option<PathBuf> {
    if cfg!(windows)
        && let Some(app) = std::env::var_os("APPDATA").filter(|s| !s.is_empty())
    {
        return Some(Path::new(&app).join("3dcadcraft"));
    }
    let (first, second) = if cfg!(windows) {
        ("USERPROFILE", "HOME")
    } else {
        ("HOME", "USERPROFILE")
    };
    let home = std::env::var_os(first)
        .or_else(|| std::env::var_os(second))
        .filter(|s| !s.is_empty())?;
    Some(PathBuf::from(home).join(".config").join("3dcadcraft"))
}

/// The presets folder: `CADCRAFT3D_PRESETS_DIR` if set, else
/// `<config_dir>/presets`.
pub fn user_dir() -> Option<PathBuf> {
    std::env::var_os("CADCRAFT3D_PRESETS_DIR")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| Some(config_dir()?.join("presets")))
}

/// The file name of a command's presets (`Face Draft` → `FaceDraft.json`).
pub fn file_name(command: &str) -> String {
    let stem: String = command
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    format!("{stem}.json")
}

/// One command's presets as loaded from its file.
#[derive(Clone, Debug)]
pub struct Loaded {
    /// The presets.
    pub data: CommandPresets,
    /// The file is read-only: changes stay in this session.
    pub read_only: bool,
    /// The file could not be read: it is moved to `.bak` at the next save.
    damaged: bool,
    /// A warning not shown yet.
    warning: Option<String>,
}

/// Loads `<dir>/<Command>.json`; a missing file is an empty set, a damaged
/// or read-only one gives a warning (never an error).
pub fn load_file(path: &Path, command: &str) -> Loaded {
    let mut l = Loaded {
        data: CommandPresets::new(command),
        read_only: false,
        damaged: false,
        warning: None,
    };
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return l,
        Err(e) => {
            l.damaged = true;
            l.warning = Some(format!("{} cannot be read ({e})", path.display()));
            return l;
        }
    };
    match serde_json::from_str::<CommandPresets>(&text) {
        Ok(mut d) => {
            d.command = command.to_owned();
            // A user preset never shadows a built-in one.
            let builtin: Vec<String> = builtins(command).into_iter().map(|(n, _)| n).collect();
            d.presets.retain(|p| !builtin.contains(&p.name));
            l.data = d;
        }
        Err(e) => {
            l.damaged = true;
            l.warning = Some(format!(
                "{} is damaged ({e}); it is kept as .bak at the next save",
                path.display()
            ));
        }
    }
    if std::fs::metadata(path).is_ok_and(|m| m.permissions().readonly()) {
        l.read_only = true;
        let w = format!("{} is read-only; changes are not saved", path.display());
        l.warning = Some(match l.warning.take() {
            Some(old) => format!("{old}; {w}"),
            None => w,
        });
    }
    l
}

/// Writes `data` to `path` atomically (temporary sibling, then rename).
pub fn write_file(path: &Path, data: &CommandPresets) -> Result<(), String> {
    let text = serde_json::to_string_pretty(data).map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let name = path
        .file_name()
        .map_or_else(|| "presets".into(), |n| n.to_string_lossy().into_owned());
    let tmp = path.with_file_name(format!(".{name}.tmp-{}", uuid::Uuid::new_v4().simple()));
    std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {e}", path.display())
    })
}

/// The presets of every command, loaded on first use.
#[derive(Debug, Default)]
pub struct PresetStore {
    /// The presets folder (`None`: in memory only).
    dir: Option<PathBuf>,
    loaded: HashMap<String, Loaded>,
}

impl PresetStore {
    /// A store in `dir` (`None`: nothing is read or written).
    pub fn new(dir: Option<PathBuf>) -> Self {
        Self {
            dir,
            loaded: HashMap::new(),
        }
    }

    /// The user's store ([`user_dir`]).
    pub fn user() -> Self {
        Self::new(user_dir())
    }

    /// The file of `command`.
    pub fn path(&self, command: &str) -> Option<PathBuf> {
        Some(self.dir.as_ref()?.join(file_name(command)))
    }

    /// The command's presets (loaded from its file the first time).
    pub fn get(&mut self, command: &str) -> &mut Loaded {
        let path = self.path(command);
        self.loaded
            .entry(command.to_owned())
            .or_insert_with(|| match path {
                Some(p) => load_file(&p, command),
                None => Loaded {
                    data: CommandPresets::new(command),
                    read_only: false,
                    damaged: false,
                    warning: None,
                },
            })
    }

    /// The command's presets, read-only.
    pub fn data(&mut self, command: &str) -> &CommandPresets {
        &self.get(command).data
    }

    /// The command's presets for a change (save with [`Self::save`]).
    pub fn data_mut(&mut self, command: &str) -> &mut CommandPresets {
        &mut self.get(command).data
    }

    /// A warning from loading, once.
    pub fn take_warning(&mut self, command: &str) -> Option<String> {
        self.get(command).warning.take()
    }

    /// Writes the command's presets to its file. A read-only file is
    /// never written; a damaged one is moved to `.bak` first.
    pub fn save(&mut self, command: &str) -> Result<(), String> {
        let Some(path) = self.path(command) else {
            return Ok(());
        };
        let l = self.get(command);
        if l.read_only {
            return Err(format!(
                "{} is read-only; changes are not saved",
                path.display()
            ));
        }
        if l.damaged && path.exists() {
            let mut bak = path.clone().into_os_string();
            bak.push(".bak");
            std::fs::rename(&path, &bak).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        write_file(&path, &l.data)?;
        l.damaged = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::part::blend::EdgeRef;
    use crate::part::shell::FaceRef;
    use crate::part::{DraftType, ProfileRef, PullRef, TargetRef};
    use crate::sketch::Point2;

    fn profile() -> ProfileRef {
        ProfileRef {
            sources: Vec::new(),
            seed: Point2::new(1.0, 2.0),
        }
    }

    fn edge() -> EdgeRef {
        EdgeRef {
            faces: ["Box1:Top".into(), "Box1:Front".into()],
            hint: [1.0, 2.0, 3.0],
        }
    }

    fn face() -> FaceRef {
        FaceRef {
            face: "Box1:Top".into(),
            hint: [0.0; 3],
        }
    }

    #[test]
    fn strip_inputs_leaves_out_the_inputs() {
        // Extrude: profiles, sketch, To face, from, solids out.
        let mut e = ExtrudeFeature::new(vec![profile()], 12.0);
        e.id = "Extrusion1".into();
        e.sketch = "Sketch1".into();
        e.extent = Extent::To(TargetRef::Face("Box1:Top".into()));
        e.from = Some(TargetRef::Face("Box1:Bottom".into()));
        e.solids = vec!["Box1".into()];
        e.operation = Operation::Cut;
        let s = e.capture();
        for k in [
            "profiles", "sketch", "extent", "from", "solids", "id", "name",
        ] {
            assert!(!s.contains_key(k), "extrude kept {k}");
        }
        assert_eq!(s["distance"], 12.0);
        assert_eq!(s["operation"], "Cut");
        e.extent = Extent::ThroughAll;
        assert_eq!(e.capture()["extent"], "ThroughAll");
        // Revolve: profiles and axis out.
        let mut r = RevolveFeature::new(vec![profile()], AxisRef::Origin(2));
        r.solids = vec!["Box1".into()];
        let s = r.capture();
        for k in ["profiles", "axis", "sketch", "solids"] {
            assert!(!s.contains_key(k), "revolve kept {k}");
        }
        assert!(s.contains_key("angle"));
        // Fillet: edges, sets, variable radii, full round out.
        let mut f = BlendFeature::fillet(vec![edge()], 3.0);
        f.sets.push(crate::part::blend::FilletSet {
            edges: vec![edge()],
            radius: 4.0,
        });
        f.full_round = Some("Box1:Top".into());
        let s = f.capture();
        for k in ["edges", "sets", "variable", "full_round", "partial"] {
            assert!(!s.contains_key(k), "fillet kept {k}");
        }
        assert_eq!(s["kind"]["Fillet"]["radius"], 3.0);
        // Chamfer: edges out.
        let s = BlendFeature::chamfer(vec![edge()], 1.0).capture();
        assert!(!s.contains_key("edges") && s.contains_key("kind"));
        // Shell: removed faces and solids out.
        let mut sh = ShellFeature::new(2.0);
        sh.remove = vec![face()];
        sh.solids = vec!["Box1".into()];
        let s = sh.capture();
        assert!(!s.contains_key("remove") && !s.contains_key("solids"));
        assert_eq!(s["thickness"], 2.0);
        // Face Draft: pull, plane, faces out.
        let mut d = DraftFeature::new(0.1);
        d.pull = Some(PullRef::Plane(crate::part::PlaneRef::Face(
            "Box1:Top".into(),
        )));
        d.plane = Some(crate::part::PlaneRef::Face("Box1:Top".into()));
        d.faces = vec![face()];
        let s = d.capture();
        for k in ["pull", "plane", "faces"] {
            assert!(!s.contains_key(k), "draft kept {k}");
        }
        assert_eq!(s["angle"], 0.1);
    }

    #[test]
    fn applying_changes_exactly_the_stored_settings() {
        let mut src = ExtrudeFeature::new(Vec::new(), 25.0);
        src.direction = ExtentDirection::Symmetric;
        src.taper = 0.05;
        src.operation = Operation::Cut;
        let s = src.capture();
        let mut e = ExtrudeFeature::new(vec![profile()], 5.0);
        e.id = "Extrusion3".into();
        e.sketch = "Sketch2".into();
        e.solids = vec!["Box1".into()];
        let mut expected = e.clone();
        e.apply(&s).unwrap();
        expected.distance = 25.0;
        expected.distance_b = 25.0;
        expected.direction = ExtentDirection::Symmetric;
        expected.taper = 0.05;
        expected.operation = Operation::Cut;
        assert_eq!(e, expected);
        // Shell: inputs kept, settings taken.
        let mut src = ShellFeature::new(4.0);
        src.direction = ShellDirection::Both;
        src.chain = false;
        let mut sh = ShellFeature::new(2.0);
        sh.remove = vec![face()];
        sh.apply(&src.capture()).unwrap();
        assert_eq!(sh.remove, vec![face()]);
        assert_eq!(
            (sh.thickness, sh.direction, sh.chain),
            (4.0, ShellDirection::Both, false)
        );
        // Draft and blend.
        let mut src = DraftFeature::new(0.2);
        src.draft_type = DraftType::FixedPlane;
        let mut d = DraftFeature::new(0.01);
        d.faces = vec![face()];
        d.apply(&src.capture()).unwrap();
        assert_eq!(
            (d.angle, d.draft_type, d.faces.len()),
            (0.2, DraftType::FixedPlane, 1)
        );
        let mut b = BlendFeature::fillet(vec![edge()], 2.0);
        b.apply(&BlendFeature::fillet(Vec::new(), 7.0).capture())
            .unwrap();
        assert_eq!(b.kind, BlendKind::Fillet { radius: 7.0 });
        assert_eq!(b.edges, vec![edge()]);
        // Settings left out of the JSON (defaults) are reset too.
        let mut v = BlendFeature::fillet(Vec::new(), 2.0);
        v.fillet_type = crate::part::FilletType::Chord;
        v.cone_corners = true;
        v.apply(&factory("Fillet").unwrap()).unwrap();
        assert_eq!(v.fillet_type, crate::part::FilletType::Constant);
        assert!(!v.cone_corners);
        // An extent To a face is an input: a preset keeps it.
        let mut e = ExtrudeFeature::new(Vec::new(), 5.0);
        e.extent = Extent::To(TargetRef::Face("Box1:Top".into()));
        e.apply(&ExtrudeFeature::new(Vec::new(), 9.0).capture())
            .unwrap();
        assert_eq!(e.extent, Extent::Distance);
        let mut e = ExtrudeFeature::new(Vec::new(), 5.0);
        e.extent = Extent::To(TargetRef::Face("Box1:Top".into()));
        let mut src = ExtrudeFeature::new(Vec::new(), 9.0);
        src.extent = Extent::To(TargetRef::Face("Other".into()));
        e.apply(&src.capture()).unwrap();
        assert_eq!(e.extent, Extent::To(TargetRef::Face("Box1:Top".into())));
    }

    #[test]
    fn a_preset_that_does_not_fit_leaves_the_definition_unchanged() {
        let mut s = Settings::new();
        s.insert("distance".into(), Value::String("ten".into()));
        let mut e = ExtrudeFeature::new(Vec::new(), 5.0);
        let before = e.clone();
        assert!(e.apply(&s).is_err());
        assert_eq!(e, before);
        // Unknown keys (a newer build) are ignored.
        let mut s = Settings::new();
        s.insert("future_option".into(), Value::Bool(true));
        s.insert("distance".into(), 8.0.into());
        e.apply(&s).unwrap();
        assert_eq!(e.distance, 8.0);
    }

    #[test]
    fn equations_are_stored_as_text_and_evaluated_in_the_document() {
        let mut part = PartDocument::default();
        let w = part.add_user_parameter(crate::part::expr::Unit::Mm);
        part.rename_parameter(&w, "wall").unwrap();
        part.set_parameter_expr("wall", "3").unwrap();
        let mut src = Feature::Shell(ShellFeature::new(2.0));
        part.apply_draft_field(&mut src, "thickness", "wall * 2")
            .unwrap();
        let Some(src) = ShellFeature::from_feature(src) else {
            panic!("not a shell")
        };
        let s = src.capture();
        assert_eq!(s["params"]["thickness"], "wall * 2");
        assert!(
            !s["params"].to_string().contains("\"name\""),
            "parameter names are document data"
        );
        // Another document with wall = 5: re-evaluated.
        let mut other = PartDocument::default();
        let w = other.add_user_parameter(crate::part::expr::Unit::Mm);
        other.rename_parameter(&w, "wall").unwrap();
        other.set_parameter_expr("wall", "5").unwrap();
        let mut sh = ShellFeature::new(1.0);
        let warnings = apply_in(&other, &mut sh, &s).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(sh.thickness, 10.0);
        assert_eq!(sh.params.0["thickness"].expr.as_deref(), Some("wall * 2"));
        // A document without `wall`: the stored value, no equation.
        let mut sh = ShellFeature::new(1.0);
        let warnings = apply_in(&PartDocument::default(), &mut sh, &s).unwrap();
        assert_eq!(warnings.len(), 1);
        assert_eq!(sh.thickness, 6.0);
        assert!(
            sh.params
                .0
                .get("thickness")
                .is_none_or(|p| p.expr.is_none())
        );
        // A plain preset drops an equation the definition had.
        let mut sh2 = sh.clone();
        sh2.params.0.insert(
            "thickness".into(),
            crate::part::params::FeatureParam {
                name: "d4".into(),
                expr: Some("wall".into()),
            },
        );
        sh2.apply(&ShellFeature::new(3.0).capture()).unwrap();
        assert_eq!(sh2.params.0["thickness"].name, "d4");
        assert_eq!(sh2.params.0["thickness"].expr, None);
    }

    #[test]
    fn modified_detection() {
        let s = ShellFeature::new(2.0).capture();
        let mut sh = ShellFeature::new(2.0);
        sh.remove = vec![face()];
        assert!(!sh.differs_from(&s), "inputs do not count");
        sh.thickness = 2.5;
        assert!(sh.differs_from(&s));
    }

    #[test]
    fn create_rename_delete() {
        let mut p = CommandPresets::new("Fillet");
        assert_eq!(p.default_name(), "Fillet 1");
        let s = BlendFeature::fillet(Vec::new(), 3.0).capture();
        assert_eq!(p.create(" Mine ", s.clone()).unwrap(), "Mine");
        assert_eq!(p.current.as_deref(), Some("Mine"));
        assert!(p.create("Mine", s.clone()).is_err(), "duplicate");
        assert!(p.create("", s.clone()).is_err(), "empty");
        assert!(p.create(NO_PRESET, s.clone()).is_err(), "reserved");
        assert!(p.create("Fillet 2 mm", s.clone()).is_err(), "built-in name");
        assert_eq!(p.rename("Mine", "Ours").unwrap(), "Ours");
        assert_eq!(p.current.as_deref(), Some("Ours"));
        assert!(p.find("Mine").is_none());
        p.create("Other", s.clone()).unwrap();
        assert!(p.rename("Ours", "Other").is_err());
        let s5 = BlendFeature::fillet(Vec::new(), 5.0).capture();
        p.save_current("Ours", s5.clone()).unwrap();
        assert_eq!(p.find("Ours").unwrap().settings, s5);
        p.delete("Other").unwrap();
        assert!(p.find("Other").is_none());
        assert!(p.delete("Other").is_err());
    }

    #[test]
    fn builtins_are_read_only() {
        for c in COMMANDS {
            let mut p = CommandPresets::new(c);
            let b = p.builtin();
            assert!(b.len() >= 2, "{c} has built-ins");
            let n = b[0].name.clone();
            assert!(p.is_builtin(&n));
            assert!(p.save_current(&n, Settings::new()).is_err());
            assert!(p.rename(&n, "x").is_err());
            assert!(p.delete(&n).is_err());
            assert_eq!(p.find(&n).unwrap().settings, b[0].settings);
            assert!(factory(c).is_some());
        }
        // They fit their command.
        let mut b = BlendFeature::chamfer(Vec::new(), 1.0);
        let s = CommandPresets::new("Chamfer")
            .find("Chamfer 0.5 x 45°")
            .unwrap()
            .settings;
        b.apply(&s).unwrap();
        assert!(matches!(
            b.kind,
            BlendKind::Chamfer {
                method: ChamferMethod::DistanceAngle,
                distance: 0.5,
                ..
            }
        ));
        let mut sh = ShellFeature::new(5.0);
        sh.apply(
            &CommandPresets::new("Shell")
                .find("Shell 2 mm Inside")
                .unwrap()
                .settings,
        )
        .unwrap();
        assert_eq!((sh.thickness, sh.direction), (2.0, ShellDirection::Inside));
        // Hidden built-ins are not listed but can still be found.
        let mut p = CommandPresets::new("Shell");
        p.show_builtins = false;
        assert!(p.list().is_empty());
    }

    #[test]
    fn start_follows_the_new_default() {
        let mut p = CommandPresets::new("Shell");
        // Never used: No Preset.
        assert_eq!(p.start(), (Active::NoPreset, None));
        let s3 = ShellFeature::new(3.0).capture();
        p.record_last_used(s3.clone());
        assert_eq!(p.start(), (Active::LastUsed, Some(s3.clone())));
        // Use Current Preset.
        p.new_default = NewDefault::UseCurrent;
        assert_eq!(p.start(), (Active::NoPreset, None), "nothing chosen yet");
        p.mark_used("Shell 1 mm Outside");
        let (a, s) = p.start();
        assert_eq!(a, Active::Named("Shell 1 mm Outside".into()));
        assert_eq!(s.unwrap()["direction"], "Outside");
        // No Preset.
        p.new_default = NewDefault::NoPreset;
        assert_eq!(p.start(), (Active::NoPreset, None));
        assert_eq!(p.settings_of(&Active::NoPreset), factory("Shell"));
    }

    #[test]
    fn sort_orders() {
        let mut p = CommandPresets::new("Shell");
        p.show_builtins = false;
        let s = Settings::new();
        for n in ["b10", "B2", "a", "10"] {
            p.create(n, s.clone()).unwrap();
        }
        p.mark_used("B2");
        p.save_current("a", s.clone()).unwrap();
        let names = |p: &CommandPresets| p.list().into_iter().map(|x| x.name).collect::<Vec<_>>();
        p.sort = SortOrder::RecentlyUsed;
        assert_eq!(names(&p)[0], "B2");
        p.sort = SortOrder::DateCreated;
        assert_eq!(names(&p), ["b10", "B2", "a", "10"]);
        p.sort = SortOrder::Alphabetical;
        assert_eq!(names(&p), ["10", "a", "B2", "b10"]);
        p.sort = SortOrder::LastModified;
        assert_eq!(names(&p)[0], "a");
    }

    #[test]
    fn storage_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = PresetStore::new(Some(dir.path().to_path_buf()));
        let d = store.data_mut("Face Draft");
        d.create("Steep", DraftFeature::new(0.1).capture()).unwrap();
        d.record_last_used(DraftFeature::new(0.2).capture());
        d.new_default = NewDefault::UseCurrent;
        d.sort = SortOrder::Alphabetical;
        store.save("Face Draft").unwrap();
        assert!(dir.path().join("FaceDraft.json").exists());
        // No temporary files left.
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        let mut again = PresetStore::new(Some(dir.path().to_path_buf()));
        assert_eq!(again.take_warning("Face Draft"), None);
        assert_eq!(again.data("Face Draft"), store.data("Face Draft"));
        // Built-ins are never written.
        let text = std::fs::read_to_string(dir.path().join("FaceDraft.json")).unwrap();
        assert!(!text.contains("Draft 3°"));
        // In memory only.
        let mut mem = PresetStore::new(None);
        mem.data_mut("Shell").create("x", Settings::new()).unwrap();
        mem.save("Shell").unwrap();
    }

    #[test]
    fn a_damaged_file_gives_a_warning_and_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Shell.json");
        std::fs::write(&path, "{ not json").unwrap();
        let mut store = PresetStore::new(Some(dir.path().to_path_buf()));
        assert!(store.data("Shell").presets.is_empty());
        let w = store.take_warning("Shell").unwrap();
        assert!(w.contains("damaged"), "{w}");
        assert_eq!(store.take_warning("Shell"), None, "warned once");
        store
            .data_mut("Shell")
            .create("x", ShellFeature::new(1.0).capture())
            .unwrap();
        store.save("Shell").unwrap();
        let bak = dir.path().join("Shell.json.bak");
        assert_eq!(std::fs::read_to_string(bak).unwrap(), "{ not json");
        let mut again = PresetStore::new(Some(dir.path().to_path_buf()));
        assert!(again.data("Shell").find("x").is_some());
    }

    #[test]
    fn a_read_only_file_is_used_but_never_written() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Fillet.json");
        let mut d = CommandPresets::new("Fillet");
        d.create("R3", BlendFeature::fillet(Vec::new(), 3.0).capture())
            .unwrap();
        write_file(&path, &d).unwrap();
        let mut perm = std::fs::metadata(&path).unwrap().permissions();
        perm.set_readonly(true);
        std::fs::set_permissions(&path, perm.clone()).unwrap();
        let mut store = PresetStore::new(Some(dir.path().to_path_buf()));
        assert!(store.data("Fillet").find("R3").is_some());
        assert!(store.take_warning("Fillet").unwrap().contains("read-only"));
        store
            .data_mut("Fillet")
            .create("R4", Settings::new())
            .unwrap();
        assert!(store.save("Fillet").is_err());
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("R4"));
        #[allow(clippy::permissions_set_readonly_false)]
        perm.set_readonly(false);
        std::fs::set_permissions(&path, perm).unwrap();
    }

    #[test]
    fn a_user_preset_never_shadows_a_built_in() {
        let dir = tempfile::tempdir().unwrap();
        let mut d = CommandPresets::new("Fillet");
        d.presets.push(Preset {
            name: "Fillet 1 mm".into(),
            settings: Settings::new(),
            created: 1,
            modified: 1,
            used: 1,
            builtin: false,
        });
        write_file(&dir.path().join("Fillet.json"), &d).unwrap();
        let mut store = PresetStore::new(Some(dir.path().to_path_buf()));
        assert!(store.data("Fillet").is_builtin("Fillet 1 mm"));
    }

    #[test]
    fn features_round_trip_with_and_without_the_preset_name() {
        let mut sh = ShellFeature::new(2.0);
        let plain = serde_json::to_string(&Feature::Shell(sh.clone())).unwrap();
        assert!(!plain.contains("preset"), "no key without a preset");
        sh.set_preset_name(Some("Thin".into()));
        let named = serde_json::to_string(&Feature::Shell(sh.clone())).unwrap();
        assert!(named.contains("\"preset\":\"Thin\""));
        let back: Feature = serde_json::from_str(&named).unwrap();
        assert_eq!(back, Feature::Shell(sh));
        let back: Feature = serde_json::from_str(&plain).unwrap();
        let Feature::Shell(b) = back else {
            panic!("not a shell")
        };
        assert_eq!(b.preset, None);
        for f in [
            Feature::Extrude(ExtrudeFeature::new(Vec::new(), 1.0)),
            Feature::Revolve(RevolveFeature::new(Vec::new(), AxisRef::Origin(0))),
            Feature::Blend(BlendFeature::fillet(Vec::new(), 1.0)),
            Feature::Draft(DraftFeature::new(0.1)),
        ] {
            let text = serde_json::to_string(&f).unwrap();
            assert!(!text.contains("preset"));
            assert_eq!(serde_json::from_str::<Feature>(&text).unwrap(), f);
        }
    }

    #[test]
    fn natural_order() {
        assert_eq!(natural_cmp("Fillet 2", "Fillet 10"), Ordering::Less);
        assert_eq!(natural_cmp("10", "a"), Ordering::Less);
        assert_eq!(natural_cmp("abc", "ABD"), Ordering::Less);
    }
}
