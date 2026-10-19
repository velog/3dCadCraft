//! Fastener and thread standards for Hole and Thread (`docs/spec/2.3-hole.md`).
//!
//! The tables are plain CSV files in `data/standards/` (built into the
//! program with `include_str!`). A file of the same name in the user's
//! standards folder ([`user_dir`]: `%APPDATA%\3dcadcraft\standards` on
//! Windows, `~/.config/3dcadcraft/standards` elsewhere) replaces the built-in
//! one, so values can be corrected or extended without a rebuild.
//!
//! Format: `#` starts a comment line, the first other line is the header,
//! columns are found by their header name (extra columns are ignored), list
//! cells use `;`. Diameters in mm.
//!
//! Features copy the numbers they use into themselves: a part regenerates
//! the same on every machine whatever its tables say.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::kernel::Real;

/// Built-in files: (file name, contents).
const BUILTIN: [(&str, &str); 5] = [
    (
        THREADS_ISO,
        include_str!("../../data/standards/iso_metric_threads.csv"),
    ),
    (
        THREADS_UNIFIED,
        include_str!("../../data/standards/unified_threads.csv"),
    ),
    (
        CLEARANCE,
        include_str!("../../data/standards/clearance_holes.csv"),
    ),
    (
        COUNTERBORES,
        include_str!("../../data/standards/counterbores.csv"),
    ),
    (
        COUNTERSINKS,
        include_str!("../../data/standards/countersinks.csv"),
    ),
];

const THREADS_ISO: &str = "iso_metric_threads.csv";
const THREADS_UNIFIED: &str = "unified_threads.csv";
const CLEARANCE: &str = "clearance_holes.csv";
const COUNTERBORES: &str = "counterbores.csv";
const COUNTERSINKS: &str = "countersinks.csv";

/// Thread type names (Inventor's Thread Type list).
pub const ISO_METRIC: &str = "ISO Metric profile";
/// Unified inch threads (UNC / UNF).
pub const ANSI_UNIFIED: &str = "ANSI Unified Screw Threads";

/// One thread size and pitch.
#[derive(Clone, Debug, PartialEq)]
pub struct ThreadRow {
    /// Designation, e.g. `M8`, `M8x1`, `1/4-20 UNC`.
    pub designation: String,
    /// Series: `coarse`, `fine`, `UNC`, `UNF`.
    pub series: String,
    /// Nominal size, e.g. `M8`, `1/4`, `#10`.
    pub size: String,
    /// Major (nominal) diameter.
    pub major: Real,
    /// Pitch (mm).
    pub pitch: Real,
    /// Tap drill diameter.
    pub tap_drill: Real,
    /// Basic minor diameter of the internal thread.
    pub minor: Real,
    /// Tolerance classes of internal threads (first: the default).
    pub classes_internal: Vec<String>,
    /// Tolerance classes of external threads.
    pub classes_external: Vec<String>,
}

/// A thread type: one table.
#[derive(Clone, Debug, PartialEq)]
pub struct ThreadTable {
    /// Its name ([`ISO_METRIC`], [`ANSI_UNIFIED`]).
    pub name: String,
    /// Rows in file order.
    pub rows: Vec<ThreadRow>,
}

impl ThreadTable {
    /// The sizes, in order, without repeats.
    pub fn sizes(&self) -> Vec<&str> {
        let mut v: Vec<&str> = Vec::new();
        for r in &self.rows {
            if !v.contains(&r.size.as_str()) {
                v.push(&r.size);
            }
        }
        v
    }

    /// The rows (designations) of one size.
    pub fn of_size<'a>(&'a self, size: &'a str) -> impl Iterator<Item = &'a ThreadRow> + 'a {
        self.rows.iter().filter(move |r| r.size == size)
    }

    /// The row with this designation.
    pub fn row(&self, designation: &str) -> Option<&ThreadRow> {
        self.rows.iter().find(|r| r.designation == designation)
    }
}

/// Clearance hole diameters of one fastener size.
#[derive(Clone, Debug, PartialEq)]
pub struct ClearanceRow {
    /// Standard (`ISO 273`, `ASME B18.2.8`).
    pub standard: String,
    /// Fastener size (`M6`, `1/4`).
    pub size: String,
    /// Close fit.
    pub close: Real,
    /// Normal fit.
    pub normal: Real,
    /// Loose fit.
    pub loose: Real,
}

/// A seat (counterbore or countersink) for one fastener size.
#[derive(Clone, Debug, PartialEq)]
pub struct SeatRow {
    /// Standard family (`ISO`, `ANSI`).
    pub standard: String,
    /// Fastener type, e.g. `Socket Head Cap Screw ISO 4762`.
    pub fastener: String,
    /// Size.
    pub size: String,
    /// Hole (clearance) diameter.
    pub hole: Real,
    /// Seat diameter.
    pub diameter: Real,
    /// Counterbore depth, or countersink angle in degrees.
    pub value: Real,
}

/// Every table.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Standards {
    /// Thread types.
    pub threads: Vec<ThreadTable>,
    /// Clearance holes.
    pub clearance: Vec<ClearanceRow>,
    /// Counterbores (value = depth).
    pub counterbores: Vec<SeatRow>,
    /// Countersinks (value = angle in degrees).
    pub countersinks: Vec<SeatRow>,
}

/// The user's standards folder.
pub fn user_dir() -> Option<PathBuf> {
    if cfg!(windows)
        && let Some(app) = std::env::var_os("APPDATA")
    {
        return Some(Path::new(&app).join("3dcadcraft").join("standards"));
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(
        Path::new(&home)
            .join(".config")
            .join("3dcadcraft")
            .join("standards"),
    )
}

/// A parsed CSV file: header names and rows of cells.
struct Csv {
    header: Vec<String>,
    rows: Vec<(usize, Vec<String>)>,
}

fn parse_csv(text: &str) -> Csv {
    let mut header = Vec::new();
    let mut rows = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cells: Vec<String> = line.split(',').map(|c| c.trim().to_owned()).collect();
        if header.is_empty() {
            header = cells.iter().map(|c| c.to_ascii_lowercase()).collect();
        } else {
            rows.push((n + 1, cells));
        }
    }
    Csv { header, rows }
}

impl Csv {
    fn col(&self, name: &str) -> Result<usize, String> {
        self.header
            .iter()
            .position(|h| h == name)
            .ok_or_else(|| format!("missing column `{name}`"))
    }

    /// Each row as a cell getter; a bad row becomes a warning.
    fn each<T>(
        &self,
        file: &str,
        warnings: &mut Vec<String>,
        f: impl Fn(&dyn Fn(&str) -> Result<String, String>) -> Result<T, String>,
    ) -> Vec<T> {
        let mut out = Vec::new();
        for (line, cells) in &self.rows {
            let get = |name: &str| -> Result<String, String> {
                let i = self.col(name)?;
                cells
                    .get(i)
                    .cloned()
                    .filter(|c| !c.is_empty())
                    .ok_or_else(|| format!("empty `{name}`"))
            };
            match f(&get) {
                Ok(v) => out.push(v),
                Err(e) => warnings.push(format!("{file}:{line}: {e}")),
            }
        }
        out
    }
}

fn number(get: &dyn Fn(&str) -> Result<String, String>, name: &str) -> Result<Real, String> {
    let s = get(name)?;
    let v: Real = s
        .parse()
        .map_err(|_| format!("`{name}` is not a number: {s}"))?;
    if v.is_finite() && v > 0.0 {
        Ok(v)
    } else {
        Err(format!("`{name}` must be positive: {s}"))
    }
}

fn list(get: &dyn Fn(&str) -> Result<String, String>, name: &str) -> Vec<String> {
    get(name)
        .map(|s| {
            s.split(';')
                .map(|c| c.trim().to_owned())
                .filter(|c| !c.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn threads(name: &str, file: &str, text: &str, warnings: &mut Vec<String>) -> ThreadTable {
    let csv = parse_csv(text);
    let rows = csv.each(file, warnings, |get| {
        let row = ThreadRow {
            designation: get("designation")?,
            series: get("series").unwrap_or_default(),
            size: get("size")?,
            major: number(get, "major")?,
            pitch: number(get, "pitch")?,
            tap_drill: number(get, "tap_drill")?,
            minor: number(get, "minor")?,
            classes_internal: list(get, "classes_internal"),
            classes_external: list(get, "classes_external"),
        };
        if !(row.minor < row.major && row.tap_drill < row.major && row.pitch < row.major) {
            return Err(format!(
                "{}: the minor diameter, tap drill and pitch must be below the major diameter",
                row.designation
            ));
        }
        Ok(row)
    });
    ThreadTable {
        name: name.to_owned(),
        rows,
    }
}

fn seats(file: &str, text: &str, value: &str, warnings: &mut Vec<String>) -> Vec<SeatRow> {
    parse_csv(text).each(file, warnings, |get| {
        let row = SeatRow {
            standard: get("standard")?,
            fastener: get("fastener")?,
            size: get("size")?,
            hole: number(get, "hole")?,
            diameter: number(get, "diameter")?,
            value: number(get, value)?,
        };
        if row.diameter <= row.hole {
            return Err(format!(
                "{} {}: the seat must be wider than the hole",
                row.fastener, row.size
            ));
        }
        Ok(row)
    })
}

impl Standards {
    /// Parses the five tables from `text(file name)`; bad rows are skipped
    /// with a warning (`file:line: reason`).
    pub fn parse(text: &dyn Fn(&str) -> String) -> (Self, Vec<String>) {
        let mut w = Vec::new();
        let threads_iso = threads(ISO_METRIC, THREADS_ISO, &text(THREADS_ISO), &mut w);
        let threads_un = threads(
            ANSI_UNIFIED,
            THREADS_UNIFIED,
            &text(THREADS_UNIFIED),
            &mut w,
        );
        let clearance = parse_csv(&text(CLEARANCE)).each(CLEARANCE, &mut w, |get| {
            Ok(ClearanceRow {
                standard: get("standard")?,
                size: get("size")?,
                close: number(get, "close")?,
                normal: number(get, "normal")?,
                loose: number(get, "loose")?,
            })
        });
        let counterbores = seats(COUNTERBORES, &text(COUNTERBORES), "depth", &mut w);
        let countersinks = seats(COUNTERSINKS, &text(COUNTERSINKS), "angle", &mut w);
        (
            Self {
                threads: vec![threads_iso, threads_un],
                clearance,
                counterbores,
                countersinks,
            },
            w,
        )
    }

    /// The built-in tables.
    pub fn builtin() -> Self {
        Self::parse(&|f| builtin_text(f).to_owned()).0
    }

    /// The tables with the files in `dir` replacing built-in ones of the
    /// same name, and the warnings (unreadable files, bad rows).
    pub fn load(dir: Option<&Path>) -> (Self, Vec<String>) {
        let mut io = Vec::new();
        let read = |f: &str| -> String {
            dir.map(|d| d.join(f))
                .filter(|p| p.is_file())
                .and_then(|p| std::fs::read_to_string(p).ok())
                .unwrap_or_else(|| builtin_text(f).to_owned())
        };
        let (s, mut w) = Self::parse(&read);
        io.append(&mut w);
        (s, io)
    }

    /// The tables as loaded once per run (built-in + the user folder).
    pub fn global() -> &'static Standards {
        static ONCE: OnceLock<Standards> = OnceLock::new();
        ONCE.get_or_init(|| {
            let (s, warnings) = Self::load(user_dir().as_deref());
            for w in warnings {
                log::warn!("standards: {w}");
            }
            s
        })
    }

    /// The thread table named `name`.
    pub fn thread_table(&self, name: &str) -> Option<&ThreadTable> {
        self.threads.iter().find(|t| t.name == name)
    }

    /// The standards of the clearance table, in order.
    pub fn clearance_standards(&self) -> Vec<&str> {
        let mut v: Vec<&str> = Vec::new();
        for r in &self.clearance {
            if !v.contains(&r.standard.as_str()) {
                v.push(&r.standard);
            }
        }
        v
    }

    /// The clearance row of `size` in `standard`.
    pub fn clearance(&self, standard: &str, size: &str) -> Option<&ClearanceRow> {
        self.clearance
            .iter()
            .find(|r| r.standard == standard && r.size == size)
    }
}

impl ClearanceRow {
    /// The diameter of a fit.
    pub fn diameter(&self, fit: super::hole::FastenerFit) -> Real {
        match fit {
            super::hole::FastenerFit::Close => self.close,
            super::hole::FastenerFit::Normal => self.normal,
            super::hole::FastenerFit::Loose => self.loose,
        }
    }
}

fn builtin_text(file: &str) -> &'static str {
    BUILTIN
        .iter()
        .find(|(n, _)| *n == file)
        .map_or("", |(_, t)| t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_tables_parse_without_warnings() {
        let (s, w) = Standards::parse(&|f| builtin_text(f).to_owned());
        assert!(w.is_empty(), "{w:?}");
        let iso = s.thread_table(ISO_METRIC).unwrap();
        let m8 = iso.row("M8").unwrap();
        assert_eq!((m8.major, m8.pitch, m8.tap_drill), (8.0, 1.25, 6.75));
        // D1 = D − 1.082532·P (ISO 724).
        assert!((m8.minor - (8.0 - 1.082532 * 1.25)).abs() < 1e-3);
        assert_eq!(m8.classes_internal[0], "6H");
        assert!(iso.row("M8x1").is_some(), "fine pitch");
        assert_eq!(iso.of_size("M10").count(), 3, "M10, M10x1.25, M10x1");
        let un = s.thread_table(ANSI_UNIFIED).unwrap();
        let q = un.row("1/4-20 UNC").unwrap();
        assert!((q.major - 6.35).abs() < 1e-9);
        assert!((q.pitch - 1.27).abs() < 1e-9);
        // 75 % thread: 0.25 − 0.9743/20 in = 0.2013 in (a #7 drill).
        assert!(
            (q.tap_drill - 0.2013 * 25.4).abs() < 0.01,
            "{}",
            q.tap_drill
        );
        assert!(un.row("1/4-28 UNF").is_some());
        let m6 = s.clearance("ISO 273", "M6").unwrap();
        assert_eq!((m6.close, m6.normal, m6.loose), (6.4, 6.6, 7.0));
        assert!(s.clearance("ASME B18.2.8", "1/4").is_some());
        assert!(
            s.counterbores
                .iter()
                .any(|r| r.size == "M6" && r.diameter == 11.0)
        );
        assert!(
            s.countersinks
                .iter()
                .any(|r| r.size == "M6" && r.value == 90.0)
        );
        assert!(
            s.countersinks
                .iter()
                .any(|r| r.size == "1/4" && r.value == 82.0)
        );
    }

    #[test]
    fn bad_rows_are_skipped_with_a_warning() {
        let text = |f: &str| -> String {
            if f == THREADS_ISO {
                "# comment\ndesignation,size,major,pitch,tap_drill,minor\nM8,M8,8,1.25,6.75,6.647\nM9,M9,x,1,8,7.9\nM10,M10,10,1.5,11,8.4\n".into()
            } else {
                builtin_text(f).to_owned()
            }
        };
        let (s, w) = Standards::parse(&text);
        let iso = s.thread_table(ISO_METRIC).unwrap();
        assert_eq!(iso.rows.len(), 1);
        assert_eq!(w.len(), 2, "{w:?}");
        assert!(w[0].starts_with("iso_metric_threads.csv:4:"), "{w:?}");
        assert!(w[1].contains("below the major"), "{w:?}");
    }

    #[test]
    fn a_user_file_replaces_the_builtin_one() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(CLEARANCE),
            "standard,size,close,normal,loose\nMy shop,M6,6.3,6.5,6.9\n",
        )
        .unwrap();
        let (s, w) = Standards::load(Some(dir.path()));
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(s.clearance_standards(), ["My shop"]);
        // The other tables stay built in.
        assert!(s.thread_table(ISO_METRIC).unwrap().row("M8").is_some());
    }
}
