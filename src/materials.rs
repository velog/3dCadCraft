//! # Material and appearance libraries
//!
//! A [`Material`] carries the physical data a part needs (density for the
//! mass, optional strength values) and the name of its default look; an
//! [`Appearance`] is only how something looks (sRGB colour, opacity, and
//! roughness/metalness for a later realistic renderer). See
//! `docs/spec/2.15-materials.md`.
//!
//! The built-in libraries are plain CSV files (`data/materials/materials.csv`,
//! `data/appearances/appearances.csv`) compiled in with `include_str!`, so
//! they are easy to edit. A user library with the same columns in
//! [`user_dir`] overrides entries by name and adds new ones.
//!
//! Parts keep a *document copy* of every definition they use
//! (`part::appearance::Looks`), so a file looks and weighs the same on a
//! machine without the library.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::kernel::Real;

/// Name of the default material (new parts).
pub const GENERIC: &str = "Generic";
/// Name of the default appearance.
pub const DEFAULT_APPEARANCE: &str = "Default";

/// File names inside a library folder.
pub const MATERIALS_FILE: &str = "materials.csv";
/// See [`MATERIALS_FILE`].
pub const APPEARANCES_FILE: &str = "appearances.csv";

const BUILTIN_MATERIALS: &str = include_str!("../data/materials/materials.csv");
const BUILTIN_APPEARANCES: &str = include_str!("../data/appearances/appearances.csv");

/// A physical material.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Material {
    /// Unique name (`Steel S235`).
    pub name: String,
    /// Group in the browser (`Steel`, `Plastic`…).
    #[serde(default)]
    pub category: String,
    /// Density (g/cm³).
    pub density: Real,
    /// Yield strength (MPa), when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub yield_strength: Option<Real>,
    /// Young's modulus (GPa), when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub youngs_modulus: Option<Real>,
    /// Name of the material's own appearance.
    #[serde(default = "default_appearance_name")]
    pub appearance: String,
}

fn default_appearance_name() -> String {
    DEFAULT_APPEARANCE.to_owned()
}

/// A look: colour and opacity (roughness and metalness for later).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Appearance {
    /// Unique name (`ABS - Red`).
    pub name: String,
    /// Group in the browser.
    #[serde(default)]
    pub category: String,
    /// sRGB colour.
    pub color: [u8; 3],
    /// 1 = opaque, 0 = invisible.
    #[serde(default = "opaque")]
    pub opacity: Real,
    /// 0 = mirror, 1 = matte (not rendered yet).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roughness: Option<Real>,
    /// 0 = dielectric, 1 = metal (not rendered yet).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metalness: Option<Real>,
}

fn opaque() -> Real {
    1.0
}

impl Appearance {
    /// A custom look (Adjust's colour editor).
    pub fn custom(name: impl Into<String>, color: [u8; 3], opacity: Real) -> Self {
        Self {
            name: name.into(),
            category: "Custom".into(),
            color,
            opacity: opacity.clamp(0.0, 1.0),
            roughness: None,
            metalness: None,
        }
    }

    /// The fallback look when nothing else resolves: the colour the
    /// viewport always used for bodies.
    pub fn fallback() -> Self {
        Self {
            name: DEFAULT_APPEARANCE.into(),
            category: "Default".into(),
            color: [172, 196, 224],
            opacity: 1.0,
            roughness: Some(0.6),
            metalness: Some(0.1),
        }
    }

    /// The colour in linear RGB (0–1), as shaders and glTF expect it.
    pub fn linear(&self) -> [f32; 3] {
        self.color.map(srgb_to_linear)
    }

    /// The colour as the viewport shader takes it: the shader writes
    /// `c^(1/2.2)` into a gamma-space target, so this is `(srgb/255)^2.2`
    /// (the Default look gives the viewport's old body colour).
    pub fn shader_rgb(&self) -> [f32; 3] {
        self.color.map(|c| (c as f32 / 255.0).powf(2.2))
    }

    /// Whether it needs blending.
    pub fn is_transparent(&self) -> bool {
        self.opacity < 0.999
    }
}

/// sRGB byte → linear 0–1 (the exact sRGB curve).
pub fn srgb_to_linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Linear 0–1 → sRGB byte.
pub fn linear_to_srgb(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0).round() as u8
}

/// Materials and appearances, in display order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Library {
    /// Materials (unique names).
    pub materials: Vec<Material>,
    /// Appearances (unique names).
    pub appearances: Vec<Appearance>,
}

impl Library {
    /// The libraries compiled into the program.
    pub fn builtin() -> Self {
        // The built-in files are tested to parse without errors.
        let (materials, _) = parse_materials(BUILTIN_MATERIALS);
        let (appearances, _) = parse_appearances(BUILTIN_APPEARANCES);
        Self {
            materials,
            appearances,
        }
    }

    /// The built-in library with the user's library from `dir` merged in
    /// (same name replaces, new names are added). Returns the problems
    /// found (unreadable file, bad lines); missing files are not problems.
    pub fn with_user(dir: Option<&Path>) -> (Self, Vec<String>) {
        let mut lib = Self::builtin();
        let mut problems = Vec::new();
        if let Some(dir) = dir {
            let mut read = |file: &str| -> Option<String> {
                let p = dir.join(file);
                match std::fs::read_to_string(&p) {
                    Ok(s) => Some(s),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                    Err(e) => {
                        problems.push(format!("Materials: {}: {e}", p.display()));
                        None
                    }
                }
            };
            let mats = read(MATERIALS_FILE);
            let looks = read(APPEARANCES_FILE);
            if let Some(text) = mats {
                let (list, errs) = parse_materials(&text);
                problems.extend(
                    errs.into_iter()
                        .map(|e| format!("Materials: {MATERIALS_FILE} {e}")),
                );
                for m in list {
                    lib.put_material(m);
                }
            }
            if let Some(text) = looks {
                let (list, errs) = parse_appearances(&text);
                problems.extend(
                    errs.into_iter()
                        .map(|e| format!("Materials: {APPEARANCES_FILE} {e}")),
                );
                for a in list {
                    lib.put_appearance(a);
                }
            }
        }
        (lib, problems)
    }

    /// Adds or replaces (by name) a material.
    pub fn put_material(&mut self, m: Material) {
        match self.materials.iter_mut().find(|x| x.name == m.name) {
            Some(x) => *x = m,
            None => self.materials.push(m),
        }
    }

    /// Adds or replaces (by name) an appearance.
    pub fn put_appearance(&mut self, a: Appearance) {
        match self.appearances.iter_mut().find(|x| x.name == a.name) {
            Some(x) => *x = a,
            None => self.appearances.push(a),
        }
    }

    /// A material by name.
    pub fn material(&self, name: &str) -> Option<&Material> {
        self.materials.iter().find(|m| m.name == name)
    }

    /// An appearance by name.
    pub fn appearance(&self, name: &str) -> Option<&Appearance> {
        self.appearances.iter().find(|a| a.name == name)
    }

    /// The Generic material (always present: built in, else synthesised).
    pub fn generic() -> Material {
        Material {
            name: GENERIC.into(),
            category: "Generic".into(),
            density: 1.0,
            yield_strength: None,
            youngs_modulus: None,
            appearance: DEFAULT_APPEARANCE.into(),
        }
    }
}

/// The built-in library, parsed once.
pub fn builtin() -> &'static Library {
    static LIB: std::sync::OnceLock<Library> = std::sync::OnceLock::new();
    LIB.get_or_init(Library::builtin)
}

/// The user's library folder: `%APPDATA%\3dcadcraft\materials` on Windows,
/// else `~/.config/3dcadcraft/materials` (next to `settings.json`).
pub fn user_dir() -> Option<PathBuf> {
    if cfg!(windows)
        && let Some(app) = std::env::var_os("APPDATA")
    {
        return Some(Path::new(&app).join("3dcadcraft").join("materials"));
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(
        Path::new(&home)
            .join(".config")
            .join("3dcadcraft")
            .join("materials"),
    )
}

/// The data lines of a CSV text: (line number, cells), skipping blank
/// lines, `#` comments and the header (a first line starting with `name`).
fn rows(text: &str) -> impl Iterator<Item = (usize, Vec<&str>)> {
    let mut header_seen = false;
    text.lines().enumerate().filter_map(move |(i, line)| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        let cells: Vec<&str> = line.split(',').map(str::trim).collect();
        if !header_seen {
            header_seen = true;
            if cells
                .first()
                .is_some_and(|c| c.eq_ignore_ascii_case("name"))
            {
                return None;
            }
        }
        Some((i + 1, cells))
    })
}

fn number(cell: Option<&&str>, what: &str) -> Result<Option<Real>, String> {
    match cell.copied().filter(|c| !c.is_empty()) {
        None => Ok(None),
        Some(c) => c
            .parse::<Real>()
            .ok()
            .filter(|v| v.is_finite())
            .map(Some)
            .ok_or_else(|| format!("{what} is not a number: {c}")),
    }
}

/// Parses a materials CSV (`name,category,density,yield_mpa,youngs_gpa,
/// appearance`). Bad lines are skipped and reported (`line N: reason`).
pub fn parse_materials(text: &str) -> (Vec<Material>, Vec<String>) {
    let mut out: Vec<Material> = Vec::new();
    let mut errors = Vec::new();
    for (n, c) in rows(text) {
        let parsed = (|| -> Result<Material, String> {
            let name = c
                .first()
                .copied()
                .filter(|s| !s.is_empty())
                .ok_or("no name")?;
            let density = number(c.get(2), "density")?.ok_or("no density")?;
            if density <= 0.0 {
                return Err(format!("density must be greater than 0: {density}"));
            }
            Ok(Material {
                name: name.to_owned(),
                category: c.get(1).copied().unwrap_or_default().to_owned(),
                density,
                yield_strength: number(c.get(3), "yield strength")?,
                youngs_modulus: number(c.get(4), "Young's modulus")?,
                appearance: c
                    .get(5)
                    .copied()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(DEFAULT_APPEARANCE)
                    .to_owned(),
            })
        })();
        match parsed {
            Ok(m) => match out.iter_mut().find(|x| x.name == m.name) {
                Some(x) => *x = m,
                None => out.push(m),
            },
            Err(e) => errors.push(format!("line {n}: {e}")),
        }
    }
    (out, errors)
}

/// Parses an appearances CSV (`name,category,r,g,b,opacity,roughness,
/// metalness`).
pub fn parse_appearances(text: &str) -> (Vec<Appearance>, Vec<String>) {
    let mut out: Vec<Appearance> = Vec::new();
    let mut errors = Vec::new();
    for (n, c) in rows(text) {
        let parsed = (|| -> Result<Appearance, String> {
            let name = c
                .first()
                .copied()
                .filter(|s| !s.is_empty())
                .ok_or("no name")?;
            let mut color = [0u8; 3];
            for (k, ch) in ["red", "green", "blue"].iter().enumerate() {
                let cell = c.get(2 + k).copied().unwrap_or_default();
                color[k] = cell
                    .parse::<u8>()
                    .map_err(|_| format!("{ch} must be 0-255: {cell}"))?;
            }
            let unit = |v: Option<Real>, what: &str| -> Result<Option<Real>, String> {
                match v {
                    Some(x) if !(0.0..=1.0).contains(&x) => Err(format!("{what} must be 0-1: {x}")),
                    other => Ok(other),
                }
            };
            Ok(Appearance {
                name: name.to_owned(),
                category: c.get(1).copied().unwrap_or_default().to_owned(),
                color,
                opacity: unit(number(c.get(5), "opacity")?, "opacity")?.unwrap_or(1.0),
                roughness: unit(number(c.get(6), "roughness")?, "roughness")?,
                metalness: unit(number(c.get(7), "metalness")?, "metalness")?,
            })
        })();
        match parsed {
            Ok(a) => match out.iter_mut().find(|x| x.name == a.name) {
                Some(x) => *x = a,
                None => out.push(a),
            },
            Err(e) => errors.push(format!("line {n}: {e}")),
        }
    }
    (out, errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The built-in files parse without a single error, every material's
    /// appearance exists, and the requested materials are there.
    #[test]
    fn builtin_library_is_complete() {
        let (mats, e1) = parse_materials(BUILTIN_MATERIALS);
        let (looks, e2) = parse_appearances(BUILTIN_APPEARANCES);
        assert!(e1.is_empty() && e2.is_empty(), "{e1:?} {e2:?}");
        assert!(mats.len() >= 24, "{}", mats.len());
        for m in &mats {
            assert!(
                looks.iter().any(|a| a.name == m.appearance),
                "{}: no appearance {}",
                m.name,
                m.appearance
            );
        }
        let lib = Library::builtin();
        for name in [
            "Generic",
            "Steel S235",
            "Steel S355",
            "Steel AISI 1018",
            "Steel AISI 4140",
            "Stainless Steel 304",
            "Stainless Steel 316",
            "Aluminium 6061-T6",
            "Aluminium 6082-T6",
            "Aluminium 7075-T6",
            "Brass CuZn37",
            "Copper Cu-ETP",
            "Cast Iron EN-GJL-250",
            "Titanium Grade 5 (Ti-6Al-4V)",
            "ABS",
            "PLA",
            "PETG",
            "Nylon PA12",
            "POM (Acetal)",
            "Polycarbonate (PC)",
            "Rubber (Natural)",
            "Glass (Soda-Lime)",
            "Wood (Oak)",
            "Wood (Pine)",
        ] {
            assert!(lib.material(name).is_some(), "{name}");
        }
        assert_eq!(lib.material("Steel S235").map(|m| m.density), Some(7.85));
        assert_eq!(lib.material(GENERIC), Some(&Library::generic()));
        // The fallback look is the library's Default.
        assert_eq!(
            lib.appearance(DEFAULT_APPEARANCE).map(|a| a.color),
            Some(Appearance::fallback().color)
        );
        assert!(
            lib.appearance("Glass - Clear")
                .is_some_and(Appearance::is_transparent)
        );
    }

    #[test]
    fn bad_lines_are_reported_and_skipped() {
        let text = "name,category,density\n\
                    # comment\n\
                    Good,X,2.5,,,Default\n\
                    NoDensity,X,\n\
                    Neg,X,-1\n\
                    Text,X,abc\n\
                    ,X,1\n";
        let (m, e) = parse_materials(text);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].name, "Good");
        assert_eq!(e.len(), 4, "{e:?}");
        assert!(e[0].starts_with("line 4:"), "{e:?}");
        let (a, e) = parse_appearances("Red,P,255,0,0\nBad,P,300,0,0\nFog,P,1,2,3,1.5\n");
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].opacity, 1.0);
        assert_eq!(e.len(), 2, "{e:?}");
    }

    /// A user library replaces built-in entries by name and adds new ones;
    /// missing files are fine, bad lines are reported.
    #[test]
    fn user_library_overrides_builtin() {
        let dir = tempfile::tempdir().unwrap();
        let (lib, problems) = Library::with_user(Some(dir.path()));
        assert!(problems.is_empty());
        assert_eq!(lib, Library::builtin());
        std::fs::write(
            dir.path().join(MATERIALS_FILE),
            "name,category,density,yield_mpa,youngs_gpa,appearance\n\
             Steel S235,Steel,7.80,240,210,My Blue\n\
             Unobtainium,Exotic,0.1,,,Default\n\
             Broken,X,zero\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join(APPEARANCES_FILE),
            "My Blue,Mine,10,20,200,0.8\n",
        )
        .unwrap();
        let (lib, problems) = Library::with_user(Some(dir.path()));
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("line 4"), "{problems:?}");
        let s = lib.material("Steel S235").unwrap();
        assert_eq!((s.density, s.appearance.as_str()), (7.80, "My Blue"));
        assert!(lib.material("Unobtainium").is_some());
        assert_eq!(lib.appearance("My Blue").map(|a| a.opacity), Some(0.8));
        assert_eq!(lib.materials.len(), Library::builtin().materials.len() + 1);
    }

    #[test]
    fn srgb_round_trip() {
        for c in [0u8, 1, 10, 100, 172, 255] {
            assert_eq!(linear_to_srgb(srgb_to_linear(c)), c);
        }
    }
}
