//! Sheet metal rules (spec 5.6 §3.1a): named sets of Sheet Metal Defaults —
//! a material at a thickness with its bend radius, K-factor, reliefs and
//! gap — kept in a table the user edits in a spreadsheet. The built-in
//! table is `data/sheet_metal/rules.csv`; a user table with the same columns
//! in the settings folder (`sheet_metal/rules.csv`) replaces rules by name
//! and adds new ones, like the material library. A part keeps a copy of
//! its rule's values (it regenerates the same without the table) and the
//! rule's name.
//!
//! The table is read the way spreadsheets write CSV: columns found by their
//! header (any order, unknown ones ignored), cells split by `,`, `;` or a tab
//! (the header line decides), a decimal comma when the cells are split by
//! `;` or a tab (Excel in locales such as Bulgarian), quoted cells, a UTF-8
//! byte order mark. A size may be an expression of the thickness (`T*1.5`).

use std::path::{Path, PathBuf};

use crate::kernel::Real;
use crate::part::sheet_metal::{CornerRelief, SheetMetalDefaults};

const BUILTIN: &str = include_str!("../data/sheet_metal/rules.csv");

/// The user's rules file in [`user_dir`].
pub const RULES_FILE: &str = "rules.csv";

/// A named sheet metal rule.
#[derive(Clone, Debug, PartialEq)]
pub struct SheetRule {
    /// Unique name (`Steel DC01 1.5 mm`).
    pub name: String,
    /// The material's name (the material library's).
    pub material: String,
    /// Thickness (mm).
    pub thickness: Real,
    /// Inner bend radius; `None`: the thickness.
    pub bend_radius: Option<Real>,
    /// K-factor of the linear unfold rule.
    pub k_factor: Real,
    /// Bend relief width; `None`: the thickness.
    pub relief_width: Option<Real>,
    /// Bend relief depth; `None`: half the thickness.
    pub relief_depth: Option<Real>,
    /// Corner relief size; `None`: four times the thickness.
    pub corner_relief: Option<Real>,
    /// Corner relief shape.
    pub corner_shape: CornerRelief,
    /// Gap between flanges at a corner; `None`: the thickness.
    pub gap: Option<Real>,
    /// Free text.
    pub note: String,
}

impl SheetRule {
    /// The part's Sheet Metal Defaults for this rule (its name kept).
    pub fn defaults(&self) -> SheetMetalDefaults {
        SheetMetalDefaults {
            thickness: self.thickness,
            bend_radius: self.bend_radius,
            k_factor: self.k_factor,
            relief_width: self.relief_width,
            relief_depth: self.relief_depth,
            corner_relief: self.corner_relief,
            gap: self.gap,
            corner_shape: self.corner_shape,
            rule: Some(self.name.clone()),
        }
    }

    /// Whether `d` holds this rule's values (its name aside).
    pub fn matches(&self, d: &SheetMetalDefaults) -> bool {
        SheetMetalDefaults {
            rule: d.rule.clone(),
            ..self.defaults()
        } == *d
    }
}

/// The rules: the built-in table with the user's merged in.
#[derive(Clone, Debug, Default)]
pub struct RuleLibrary {
    /// Every rule, in table order.
    pub rules: Vec<SheetRule>,
}

impl RuleLibrary {
    /// The built-in rules.
    pub fn builtin() -> Self {
        // The built-in table is tested to parse without errors.
        Self {
            rules: parse_rules(BUILTIN).0,
        }
    }

    /// The built-in rules with the user's table from `dir` merged in (same
    /// name replaces, new names are added); the problems found (unreadable
    /// file, bad lines). A missing file is no problem.
    pub fn with_user(dir: Option<&Path>) -> (Self, Vec<String>) {
        let mut lib = Self::builtin();
        let mut problems = Vec::new();
        if let Some(dir) = dir {
            let p = dir.join(RULES_FILE);
            match std::fs::read(&p) {
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    let (rules, errs) = parse_rules(&text);
                    problems.extend(errs.into_iter().map(|e| format!("{RULES_FILE} {e}")));
                    for r in rules {
                        lib.put(r);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => problems.push(format!("{}: {e}", p.display())),
            }
        }
        (lib, problems)
    }

    /// Adds or replaces (by name) a rule.
    pub fn put(&mut self, r: SheetRule) {
        match self.rules.iter_mut().find(|x| x.name == r.name) {
            Some(x) => *x = r,
            None => self.rules.push(r),
        }
    }

    /// A rule by name.
    pub fn rule(&self, name: &str) -> Option<&SheetRule> {
        self.rules.iter().find(|r| r.name == name)
    }

    /// The materials the rules use, in table order.
    pub fn materials(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for r in &self.rules {
            if !out.contains(&r.material.as_str()) {
                out.push(&r.material);
            }
        }
        out
    }
}

/// The user's rules folder: `sheet_metal` next to the material library's
/// (`%APPDATA%\3dcadcraft\sheet_metal` on Windows).
pub fn user_dir() -> Option<PathBuf> {
    Some(crate::materials::user_dir()?.parent()?.join("sheet_metal"))
}

/// The cells of one line split by `sep`, quotes (`"a;b"`, `""`) honoured.
fn split_cells(line: &str, sep: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            c if c == sep && !quoted => out.push(std::mem::take(&mut cur).trim().to_owned()),
            c => cur.push(c),
        }
    }
    out.push(cur.trim().to_owned());
    out
}

/// A size cell: empty, a number (a decimal comma when `comma`), or an
/// expression of the thickness `t` (`T`, `Thickness`).
fn size(cell: &str, t: Real, comma: bool) -> Result<Option<Real>, String> {
    let text = cell.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let text = if comma {
        text.replace(',', ".")
    } else {
        text.to_owned()
    };
    if let Ok(v) = text.parse::<Real>() {
        return Ok(Some(v));
    }
    let vars = |n: &str| (n == "T" || n.eq_ignore_ascii_case("thickness")).then_some(t);
    crate::part::expr::eval_simple(&text, crate::part::expr::Unit::Mm, &vars)
        .map(|(v, _)| Some(v))
        .map_err(|e| format!("{cell}: {e}"))
}

fn corner_shape(cell: &str) -> Result<CornerRelief, String> {
    match cell.trim().to_ascii_lowercase().as_str() {
        "" | "round" => Ok(CornerRelief::Round),
        "square" => Ok(CornerRelief::Square),
        "tear" => Ok(CornerRelief::Tear),
        "trim" | "trim to bend" | "none" => Ok(CornerRelief::Trim),
        other => Err(format!("corner_shape {other}: Round, Square, Tear or Trim")),
    }
}

/// The cell separator a table's header line uses.
fn separator(header: &str) -> char {
    if header.contains(';') {
        ';'
    } else if header.contains('\t') {
        '\t'
    } else {
        ','
    }
}

/// Parses a rules table. Bad lines are skipped and reported (`line N: reason`).
pub fn parse_rules(text: &str) -> (Vec<SheetRule>, Vec<String>) {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut out: Vec<SheetRule> = Vec::new();
    let mut errors = Vec::new();
    // The header: the first line that is no comment (a spreadsheet may pad
    // comment lines with empty cells).
    let mut header: Option<(char, Vec<String>)> = None;
    for (i, line) in text.lines().enumerate() {
        let n = i + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("\"#") {
            continue;
        }
        let Some((sep, cols)) = &header else {
            let sep = separator(trimmed);
            let cols: Vec<String> = split_cells(trimmed, sep)
                .into_iter()
                .map(|c| c.to_ascii_lowercase())
                .collect();
            if !cols.iter().any(|c| c == "name") || !cols.iter().any(|c| c == "thickness") {
                errors.push(format!(
                    "line {n}: the header must name the columns (at least name and thickness)"
                ));
                return (out, errors);
            }
            header = Some((sep, cols));
            continue;
        };
        let cells = split_cells(trimmed, *sep);
        if cells.iter().all(|c| c.is_empty()) {
            continue;
        }
        let comma = *sep != ',';
        let cell = |name: &str| -> &str {
            cols.iter()
                .position(|c| c == name)
                .and_then(|k| cells.get(k))
                .map_or("", String::as_str)
        };
        let parsed = (|| -> Result<SheetRule, String> {
            let name = cell("name");
            if name.is_empty() {
                return Err("no name".into());
            }
            let t = size(cell("thickness"), 0.0, comma)?.ok_or("no thickness")?;
            if !(t.is_finite() && t > 0.0) {
                return Err(format!("the thickness must be positive: {t}"));
            }
            let k = size(cell("k_factor"), t, comma)?.unwrap_or(0.44);
            if !(0.0..=1.0).contains(&k) {
                return Err(format!("the K-factor must be between 0 and 1: {k}"));
            }
            let positive = |what: &str| -> Result<Option<Real>, String> {
                match size(cell(what), t, comma)? {
                    Some(v) if !(v.is_finite() && v > 0.0) => {
                        Err(format!("{what} must be positive: {v}"))
                    }
                    v => Ok(v),
                }
            };
            Ok(SheetRule {
                name: name.to_owned(),
                material: cell("material").to_owned(),
                thickness: t,
                bend_radius: positive("bend_radius")?,
                k_factor: k,
                relief_width: positive("relief_width")?,
                relief_depth: positive("relief_depth")?,
                corner_relief: positive("corner_relief")?,
                corner_shape: corner_shape(cell("corner_shape"))?,
                gap: positive("gap")?,
                note: cell("note").to_owned(),
            })
        })();
        match parsed {
            Ok(r) => match out.iter_mut().find(|x| x.name == r.name) {
                Some(x) => *x = r,
                None => out.push(r),
            },
            Err(e) => errors.push(format!("line {n}: {e}")),
        }
    }
    (out, errors)
}

/// The built-in table written for a spreadsheet whose cells are split by
/// `sep` (`;` or a tab: numbers with a decimal comma), comments and
/// expressions kept.
pub fn builtin_table(sep: char) -> String {
    const NUMBERS: [&str; 7] = [
        "thickness",
        "bend_radius",
        "k_factor",
        "relief_width",
        "relief_depth",
        "corner_relief",
        "gap",
    ];
    let comma = sep != ',';
    let mut numeric: Vec<bool> = Vec::new();
    let mut out = String::new();
    for line in BUILTIN.lines() {
        if line.trim_start().starts_with('#') || line.trim().is_empty() {
            out.push_str(line);
        } else {
            let cells = split_cells(line, ',');
            if numeric.is_empty() {
                // The header: which columns hold numbers.
                numeric = cells
                    .iter()
                    .map(|c| NUMBERS.contains(&c.to_ascii_lowercase().as_str()))
                    .collect();
            }
            let cells: Vec<String> = cells
                .into_iter()
                .enumerate()
                .map(|(k, c)| {
                    let c = if comma && numeric.get(k).copied().unwrap_or(false) {
                        c.replace('.', ",")
                    } else {
                        c
                    };
                    if c.contains(sep) || c.contains('"') {
                        format!("\"{}\"", c.replace('"', "\"\""))
                    } else {
                        c
                    }
                })
                .collect();
            out.push_str(&cells.join(&sep.to_string()));
        }
        out.push('\n');
    }
    out
}

/// The list separator spreadsheets use on this system: Windows' regional
/// setting (`;` where the decimal mark is a comma), `,` elsewhere.
pub fn system_separator() -> char {
    if cfg!(windows)
        && let Ok(o) = std::process::Command::new("reg")
            .args(["query", r"HKCU\Control Panel\International", "/v", "sList"])
            .output()
    {
        let text = String::from_utf8_lossy(&o.stdout);
        if let Some(v) = text
            .lines()
            .find(|l| l.contains("sList"))
            .and_then(|l| l.split_whitespace().last())
            && let Some(c) = v.chars().next()
        {
            return c;
        }
    }
    ','
}

/// Creates the user's rules file (the built-in table, for this system's
/// spreadsheet) unless it is there; returns its path.
pub fn ensure_user_file() -> Result<PathBuf, String> {
    let dir = user_dir().ok_or("no settings folder")?;
    let path = dir.join(RULES_FILE);
    if !path.exists() {
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        // A byte order mark so that Excel reads the file as UTF-8.
        let text = format!("\u{feff}{}", builtin_table(system_separator()));
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(path)
}

/// Opens `path` in the program the system uses for it (a spreadsheet for `.csv`).
pub fn open_in_default_app(path: &Path) -> Result<(), String> {
    let r = if cfg!(windows) {
        std::process::Command::new("cmd")
            .args(["/C", "start", ""])
            .arg(path)
            .spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(path).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(path).spawn()
    };
    r.map(|_| ())
        .map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_builtin_table_parses() {
        let (rules, errs) = parse_rules(BUILTIN);
        assert!(errs.is_empty(), "{errs:?}");
        assert!(rules.len() >= 50, "{}", rules.len());
        assert!(rules.iter().any(|r| r.name == "Default"));
        // Every rule's material is in the material library.
        for r in &rules {
            assert!(
                crate::materials::builtin().material(&r.material).is_some(),
                "{}: no material {}",
                r.name,
                r.material
            );
            assert!(r.defaults().validate().is_ok(), "{}", r.name);
        }
    }

    #[test]
    fn expressions_of_the_thickness() {
        let lib = RuleLibrary::builtin();
        let r = lib.rule("Steel S235 8 mm").unwrap();
        assert_eq!(r.bend_radius, Some(12.0));
        assert_eq!(r.relief_width, None);
    }

    #[test]
    fn a_spreadsheet_with_semicolons_and_decimal_commas() {
        let text = "\u{feff}# made in Excel;;;\r\nName;Thickness;K_Factor;Bend_Radius;Material\r\n\
                    My Steel 1,5;1,5;0,42;T*2;Steel DC01\r\n\"Odd; name\";2;;;\r\n;;;;\r\n";
        let (rules, errs) = parse_rules(text);
        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].name, "My Steel 1,5");
        assert_eq!(rules[0].thickness, 1.5);
        assert_eq!(rules[0].k_factor, 0.42);
        assert_eq!(rules[0].bend_radius, Some(3.0));
        assert_eq!(rules[1].name, "Odd; name");
        assert_eq!(rules[1].k_factor, 0.44);
    }

    #[test]
    fn the_builtin_table_round_trips_through_a_semicolon_spreadsheet() {
        let a = parse_rules(BUILTIN).0;
        let (b, errs) = parse_rules(&builtin_table(';'));
        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!(a, b);
    }

    #[test]
    fn bad_lines_are_reported_and_skipped() {
        let text =
            "name,thickness,k_factor,corner_shape\nA,2,1.5,\nB,-1,,\nC,2,,Hexagon\nD,3,,Square\n";
        let (rules, errs) = parse_rules(text);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].corner_shape, CornerRelief::Square);
        assert_eq!(errs.len(), 3, "{errs:?}");
        assert!(errs[0].starts_with("line 2"));
    }

    #[test]
    fn the_user_table_replaces_and_adds() {
        let dir = std::env::temp_dir().join(format!("cc3d_rules_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(RULES_FILE),
            "name;material;thickness;k_factor\nDefault;Generic;3;0,4\nMine;Steel S235;7;\n",
        )
        .unwrap();
        let (lib, errs) = RuleLibrary::with_user(Some(&dir));
        std::fs::remove_dir_all(&dir).ok();
        assert!(errs.is_empty(), "{errs:?}");
        let d = lib.rule("Default").unwrap();
        assert_eq!((d.thickness, d.k_factor), (3.0, 0.4));
        assert!(lib.rule("Mine").is_some());
        assert_eq!(lib.rules.len(), RuleLibrary::builtin().rules.len() + 1);
    }
}
