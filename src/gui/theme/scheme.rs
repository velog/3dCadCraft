//! In-canvas Color Schemes (spec 0.18): the colours of the graphics window, by
//! name, built-in or the user's own (one JSON file each in `color_schemes/` of
//! the settings folder). Independent of the UI Theme.

use eframe::egui::Color32;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

/// The name of the default scheme.
pub const DEFAULT_SCHEME: &str = "3DCadCraft Light";

/// Every named colour of the graphics window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanvasColors {
    /// The background; Background › 1 Color uses only the top.
    pub background_top: Color32,
    pub background_bottom: Color32,
    pub gradient: bool,
    pub ground_grid: Color32,
    pub sketch_grid: Color32,
    pub model_edges: Color32,
    pub selected_face: Color32,
    pub hover_face: Color32,
    /// Sketch curves by state.
    pub sketch_open: Color32,
    pub sketch_full: Color32,
    pub sketch_redundant: Color32,
    pub sketch_failed: Color32,
    pub reference: Color32,
    pub highlight: Color32,
    pub dimension: Color32,
    /// Closed regions: of a sketch, offered as a profile, chosen.
    pub region_fill: Color32,
    pub profile_fill: Color32,
    pub profile_chosen: Color32,
}

impl CanvasColors {
    /// The default: light and neutral, so the model and the sketch stand out.
    pub const fn light() -> CanvasColors {
        CanvasColors {
            background_top: Color32::from_rgb(240, 243, 247),
            background_bottom: Color32::from_rgb(208, 214, 223),
            gradient: true,
            ground_grid: Color32::from_rgb(194, 196, 201),
            sketch_grid: Color32::from_rgba_premultiplied(21, 26, 33, 60),
            model_edges: Color32::from_rgb(43, 43, 59),
            selected_face: Color32::from_rgb(135, 186, 250),
            hover_face: Color32::from_rgb(168, 233, 178),
            sketch_open: Color32::from_rgb(30, 90, 220),
            sketch_full: Color32::from_rgb(20, 120, 40),
            sketch_redundant: Color32::from_rgb(165, 85, 0),
            sketch_failed: Color32::from_rgb(210, 30, 30),
            reference: Color32::from_rgb(160, 105, 0),
            highlight: Color32::from_rgb(255, 140, 0),
            dimension: Color32::from_rgb(40, 70, 150),
            region_fill: Color32::from_rgba_unmultiplied_const(60, 130, 220, 28),
            profile_fill: Color32::from_rgba_unmultiplied_const(90, 190, 90, 50),
            profile_chosen: Color32::from_rgba_premultiplied(30, 110, 30, 120),
        }
    }

    /// A dark view: for work in a dim room.
    pub const fn graphite() -> CanvasColors {
        CanvasColors {
            background_top: Color32::from_rgb(62, 67, 77),
            background_bottom: Color32::from_rgb(38, 42, 49),
            gradient: true,
            ground_grid: Color32::from_rgb(92, 98, 108),
            sketch_grid: Color32::from_rgba_premultiplied(44, 48, 54, 60),
            model_edges: Color32::from_rgb(18, 18, 24),
            selected_face: Color32::from_rgb(120, 175, 250),
            hover_face: Color32::from_rgb(150, 225, 165),
            sketch_open: Color32::from_rgb(120, 175, 255),
            sketch_full: Color32::from_rgb(110, 215, 135),
            sketch_redundant: Color32::from_rgb(245, 175, 80),
            sketch_failed: Color32::from_rgb(250, 115, 105),
            reference: Color32::from_rgb(235, 190, 90),
            highlight: Color32::from_rgb(255, 170, 60),
            dimension: Color32::from_rgb(190, 210, 245),
            region_fill: Color32::from_rgba_unmultiplied_const(90, 150, 240, 40),
            profile_fill: Color32::from_rgba_unmultiplied_const(90, 190, 90, 50),
            profile_chosen: Color32::from_rgba_premultiplied(40, 120, 40, 120),
        }
    }
}

/// One line of the Color Scheme editor.
pub struct Element {
    /// The key in a scheme's file: never rename one.
    pub key: &'static str,
    pub label: &'static str,
    pub group: &'static str,
    /// Has a Translucency slider.
    pub translucent: bool,
    pub get: fn(&CanvasColors) -> Color32,
    pub set: fn(&mut CanvasColors, Color32),
}

macro_rules! element {
    ($field:ident, $label:literal, $group:literal, $translucent:literal) => {
        Element {
            key: stringify!($field),
            label: $label,
            group: $group,
            translucent: $translucent,
            get: |c| c.$field,
            set: |c, v| c.$field = v,
        }
    };
}

/// The Graphical Elements list, in the editor's order.
pub const ELEMENTS: &[Element] = &[
    element!(background_top, "Background, top", "Background", false),
    element!(background_bottom, "Background, bottom", "Background", false),
    element!(ground_grid, "Ground grid", "Background", false),
    element!(model_edges, "Edges", "Model", false),
    element!(selected_face, "Selected", "Model", false),
    element!(hover_face, "Prehighlight", "Model", false),
    element!(sketch_open, "Needs dimensions", "Sketch", false),
    element!(sketch_full, "Fully constrained", "Sketch", false),
    element!(sketch_redundant, "Over-constrained", "Sketch", false),
    element!(sketch_failed, "Not solved", "Sketch", false),
    element!(reference, "Projected geometry", "Sketch", false),
    element!(highlight, "Under the cursor", "Sketch", false),
    element!(dimension, "Dimensions", "Sketch", false),
    element!(sketch_grid, "Sketch grid", "Sketch", true),
    element!(region_fill, "Closed region", "Profiles", true),
    element!(profile_fill, "Offered profile", "Profiles", true),
    element!(profile_chosen, "Chosen profile", "Profiles", true),
];

/// A scheme: built-in or the user's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanvasScheme {
    pub name: String,
    pub builtin: bool,
    pub colors: CanvasColors,
}

/// The schemes that ship with the program.
pub fn builtin_schemes() -> Vec<CanvasScheme> {
    [
        (DEFAULT_SCHEME, CanvasColors::light()),
        ("3DCadCraft Graphite", CanvasColors::graphite()),
    ]
    .map(|(name, colors)| CanvasScheme {
        name: name.to_owned(),
        builtin: true,
        colors,
    })
    .into()
}

/// `#rrggbb`, or `#rrggbbaa` when see-through.
fn to_hex(c: Color32) -> String {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    if a == 255 {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
    }
}

fn from_hex(text: &str) -> Option<Color32> {
    let h = text.strip_prefix('#')?;
    if !(h.len() == 6 || h.len() == 8) || !h.is_ascii() {
        return None;
    }
    let at = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    let a = if h.len() == 8 { at(6)? } else { 255 };
    Some(Color32::from_rgba_unmultiplied(at(0)?, at(2)?, at(4)?, a))
}

impl CanvasScheme {
    /// The scheme's file.
    pub fn to_json(&self) -> Value {
        let colors: Map<String, Value> = ELEMENTS
            .iter()
            .map(|e| (e.key.to_owned(), json!(to_hex((e.get)(&self.colors)))))
            .collect();
        json!({ "name": self.name, "colors": colors })
    }

    /// A scheme from its file, with the keys it could not read. A missing or bad
    /// colour keeps the default, the others load (rule 5). `None`: no name.
    pub fn from_json(v: &Value) -> Option<(CanvasScheme, Vec<String>)> {
        let name = v.get("name")?.as_str()?.trim();
        if name.is_empty() {
            return None;
        }
        let mut colors = CanvasColors::light();
        let mut bad = Vec::new();
        let given = v.get("colors").and_then(Value::as_object);
        for e in ELEMENTS {
            match given.and_then(|m| m.get(e.key)) {
                Some(Value::String(t)) => match from_hex(t) {
                    Some(c) => (e.set)(&mut colors, c),
                    None => bad.push(e.key.to_owned()),
                },
                Some(_) => bad.push(e.key.to_owned()),
                None => {}
            }
        }
        Some((
            CanvasScheme {
                name: name.to_owned(),
                builtin: false,
                colors,
            },
            bad,
        ))
    }
}

/// The folder of the user's schemes. Tests never touch the user's settings.
pub fn schemes_dir() -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    crate::presets::config_dir().map(|d| d.join("color_schemes"))
}

/// A file name for a scheme's name: anything else becomes `_`.
fn file_name(name: &str) -> String {
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == ' ' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{}.json", safe.trim())
}

/// The user's schemes in `dir`, sorted by name; a file that cannot be read is
/// left out and logged.
pub fn load_schemes(dir: &Path) -> Vec<CanvasScheme> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return out;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let parsed = std::fs::read_to_string(&p)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| CanvasScheme::from_json(&v));
        match parsed {
            Some((scheme, bad)) => {
                if !bad.is_empty() {
                    log::warn!(
                        "colour scheme {}: default kept for {}",
                        p.display(),
                        bad.join(", ")
                    );
                }
                out.push(scheme);
            }
            None => log::warn!("colour scheme {}: not read", p.display()),
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Writes a scheme's file (through a temporary file, so a failure leaves the old).
pub fn save_scheme(dir: &Path, s: &CanvasScheme) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(file_name(&s.name));
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(&s.to_json()).map_err(std::io::Error::other)?;
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, &path)
}

/// Deletes a scheme's file; none there is fine.
pub fn remove_scheme(dir: &Path, name: &str) -> std::io::Result<()> {
    match std::fs::remove_file(dir.join(file_name(name))) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// A name not yet in `taken`: `base`, `base 2`, `base 3`…
pub fn unique_name(base: &str, taken: &[String]) -> String {
    let same = |n: &str| taken.iter().any(|t| t.eq_ignore_ascii_case(n));
    if !same(base) {
        return base.to_owned();
    }
    (2..)
        .map(|k| format!("{base} {k}"))
        .find(|n| !same(n))
        .unwrap_or_else(|| base.to_owned())
}

static CANVAS: RwLock<CanvasColors> = RwLock::new(CanvasColors::light());

/// The In-canvas colours in use.
pub fn canvas() -> CanvasColors {
    CANVAS.read().map_or_else(|p| *p.into_inner(), |c| *c)
}

/// Sets the In-canvas colours in use; the caller rebuilds the scene.
pub fn set_canvas(c: CanvasColors) {
    match CANVAS.write() {
        Ok(mut g) => *g = c,
        Err(p) => *p.into_inner() = c,
    }
}

/// A colour for the 3D scene's geometry (linear; the shader gamma-encodes it).
pub fn scene_rgb(c: Color32) -> [f32; 3] {
    let f = |v: u8| (f32::from(v) / 255.0).powf(2.2);
    [f(c.r()), f(c.g()), f(c.b())]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(c: Color32) -> f32 {
        let f = |v: u8| {
            let s = f32::from(v) / 255.0;
            if s <= 0.03928 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
    }

    fn contrast(a: Color32, b: Color32) -> f32 {
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cadcraft-schemes-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn every_builtin_scheme_keeps_sketch_states_readable_on_its_background() {
        for s in builtin_schemes() {
            let c = s.colors;
            for bg in [c.background_top, c.background_bottom] {
                for (name, fg) in [
                    ("open", c.sketch_open),
                    ("full", c.sketch_full),
                    ("redundant", c.sketch_redundant),
                    ("failed", c.sketch_failed),
                    ("reference", c.reference),
                    ("dimension", c.dimension),
                ] {
                    let k = contrast(fg, bg);
                    assert!(k >= 3.0, "{}: {name} {k:.2}", s.name);
                }
            }
        }
    }

    #[test]
    fn see_through_colours_are_valid() {
        for s in builtin_schemes() {
            for e in ELEMENTS {
                let [r, g, b, a] = (e.get)(&s.colors).to_array();
                assert!(r <= a && g <= a && b <= a, "{}: {}", s.name, e.key);
            }
        }
    }

    #[test]
    fn sketch_states_can_be_told_apart() {
        for s in builtin_schemes() {
            let c = s.colors;
            let states = [
                c.sketch_open,
                c.sketch_full,
                c.sketch_redundant,
                c.sketch_failed,
            ];
            for (i, a) in states.iter().enumerate() {
                for b in &states[i + 1..] {
                    assert_ne!(a, b, "{}", s.name);
                }
            }
        }
    }

    #[test]
    fn element_keys_are_unique_and_cover_get_and_set() {
        for (i, e) in ELEMENTS.iter().enumerate() {
            assert!(
                ELEMENTS[i + 1..].iter().all(|o| o.key != e.key),
                "{}",
                e.key
            );
            let mut c = CanvasColors::light();
            let v = Color32::from_rgb(1, 2, 3);
            (e.set)(&mut c, v);
            assert_eq!((e.get)(&c), v, "{}", e.key);
        }
    }

    #[test]
    fn a_scheme_comes_back_from_its_file() {
        let s = CanvasScheme {
            name: "Mine".into(),
            builtin: false,
            colors: CanvasColors::graphite(),
        };
        let (back, bad) =
            CanvasScheme::from_json(&s.to_json()).unwrap_or_else(|| panic!("no name"));
        assert!(bad.is_empty(), "{bad:?}");
        assert_eq!(back.name, "Mine");
        assert!(!back.builtin);
        for e in ELEMENTS {
            let (a, b) = ((e.get)(&s.colors), (e.get)(&back.colors));
            // A see-through colour may move one level through (un)multiplying.
            for (x, y) in a.to_array().iter().zip(b.to_array()) {
                assert!(x.abs_diff(y) <= 2, "{}: {a:?} {b:?}", e.key);
            }
        }
    }

    #[test]
    fn a_bad_colour_keeps_the_default_and_the_rest_load() {
        let v = json!({
            "name": "Half",
            "colors": { "dimension": "#010203", "reference": "not a colour", "highlight": 7 }
        });
        let (s, bad) = CanvasScheme::from_json(&v).unwrap_or_else(|| panic!("no name"));
        assert_eq!(s.colors.dimension, Color32::from_rgb(1, 2, 3));
        assert_eq!(s.colors.reference, CanvasColors::light().reference);
        assert_eq!(bad, ["reference", "highlight"]);
    }

    #[test]
    fn a_file_without_a_name_is_no_scheme() {
        assert!(CanvasScheme::from_json(&json!({ "colors": {} })).is_none());
        assert!(CanvasScheme::from_json(&json!({ "name": "  " })).is_none());
    }

    #[test]
    fn hex_refuses_what_is_not_a_colour() {
        for t in ["", "#", "#12345", "123456", "#gggggg", "#ééé"] {
            assert_eq!(from_hex(t), None, "{t}");
        }
        assert_eq!(from_hex("#0a0B0c"), Some(Color32::from_rgb(10, 11, 12)));
    }

    #[test]
    fn schemes_are_saved_listed_and_removed() {
        let dir = temp_dir("files");
        assert!(load_schemes(&dir).is_empty());
        let s = |n: &str| CanvasScheme {
            name: n.into(),
            builtin: false,
            colors: CanvasColors::light(),
        };
        save_scheme(&dir, &s("Zed")).unwrap_or_else(|e| panic!("{e}"));
        save_scheme(&dir, &s("Alpha / 2")).unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(dir.join("broken.json"), "{ not json").unwrap_or_else(|e| panic!("{e}"));
        let names: Vec<String> = load_schemes(&dir).into_iter().map(|x| x.name).collect();
        assert_eq!(names, ["Alpha / 2", "Zed"]);
        remove_scheme(&dir, "Zed").unwrap_or_else(|e| panic!("{e}"));
        remove_scheme(&dir, "Zed").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(load_schemes(&dir).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_new_name_does_not_repeat() {
        let taken = ["Mine".to_owned(), "mine 2".to_owned()];
        assert_eq!(unique_name("Other", &taken), "Other");
        assert_eq!(unique_name("Mine", &taken), "Mine 3");
    }

    #[test]
    fn the_scene_colours_of_the_default_scheme_stay_as_they_were() {
        let close = |c: Color32, want: [f32; 3]| {
            for (a, b) in scene_rgb(c).iter().zip(want) {
                assert!((a - b).abs() < 0.02, "{c:?}: {a} vs {b}");
            }
        };
        let c = CanvasColors::light();
        close(c.selected_face, [0.25, 0.5, 0.95]);
        close(c.hover_face, [0.4, 0.82, 0.45]);
        close(c.ground_grid, [0.55, 0.56, 0.6]);
        close(c.model_edges, [0.02, 0.02, 0.04]);
    }
}
