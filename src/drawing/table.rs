//! General tables (`docs/spec/4.7-drawing-commands.md`, Table row): a grid
//! of texts typed by the user or read from a CSV file, with a title row and
//! column headings, placed by its upper left corner. Inventor's Table (ISO)
//! style: title `TABLE`, columns 25.4 mm, rows 6.40 mm for 3.5 mm text, every
//! cell centred. A table read from a file keeps the file's name but not a
//! link: its cells are the table's own (rule 8: the drawing opens the same
//! without the file).

use serde::{Deserialize, Serialize};

use super::parts::{Column, PartsList, Table, TableRow, table_prims_gap};
use super::sheet::Prim;
use super::style::AnnotationLook;

/// Row gap of Inventor's Table style (mm).
pub const ROW_GAP: f64 = 0.889;

/// Inventor's default column width (mm).
pub const COLUMN_WIDTH: f64 = 25.4;

/// Inventor's default size of an empty table: columns, data rows.
pub const DEFAULT_SIZE: (usize, usize) = (4, 10);

fn default_title() -> String {
    "TABLE".into()
}

/// A general table on the sheet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GeneralTable {
    /// Upper left corner (sheet mm).
    pub at: [f64; 2],
    /// Title row text (empty: no title row).
    #[serde(default = "default_title")]
    pub title: String,
    /// Column headings and widths.
    pub columns: Vec<TableColumn>,
    /// Data rows, one text per column.
    #[serde(default)]
    pub rows: Vec<Vec<String>>,
    /// The CSV file the cells were read from (shown in the editor; no link).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
}

/// A column of a general table.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TableColumn {
    /// Heading.
    pub title: String,
    /// Width on paper (mm).
    pub width: f64,
}

impl TableColumn {
    /// A column with Inventor's default width.
    pub fn new(title: &str) -> Self {
        Self {
            title: title.into(),
            width: COLUMN_WIDTH,
        }
    }
}

/// Spreadsheet-like column names: A, B, … Z, AA.
pub fn column_name(k: usize) -> String {
    super::parts::item_text(k + 1, 1, true, false)
}

impl GeneralTable {
    /// An empty table of `columns` × `rows` at `at` (headings A, B, C…).
    pub fn empty(at: [f64; 2], columns: usize, rows: usize) -> Self {
        let columns: Vec<TableColumn> = (0..columns.max(1))
            .map(|k| TableColumn::new(&column_name(k)))
            .collect();
        let n = columns.len();
        Self {
            at,
            title: default_title(),
            columns,
            rows: vec![vec![String::new(); n]; rows],
            source: String::new(),
        }
    }

    /// A table from CSV text: the first row gives the headings when
    /// `headers` (else headings stay empty). Like Inventor, reading stops
    /// at the first blank row and the columns end at the first empty
    /// heading / cell of the first row.
    pub fn from_csv(at: [f64; 2], text: &str, headers: bool) -> Result<Self, String> {
        let mut lines: Vec<Vec<String>> = Vec::new();
        for line in text.lines() {
            if line.trim().is_empty() {
                break;
            }
            lines.push(split_csv(line));
        }
        let Some(first) = lines.first() else {
            return Err("the file has no rows".into());
        };
        let n = first.iter().take_while(|c| !c.trim().is_empty()).count();
        if n == 0 {
            return Err("the first row has no values".into());
        }
        let titles: Vec<String> = if headers {
            first[..n].iter().map(|s| s.trim().to_owned()).collect()
        } else {
            vec![String::new(); n]
        };
        let body = if headers { &lines[1..] } else { &lines[..] };
        let rows = body
            .iter()
            .map(|r| {
                (0..n)
                    .map(|k| r.get(k).map_or(String::new(), |s| s.trim().to_owned()))
                    .collect()
            })
            .collect();
        Ok(Self {
            at,
            title: default_title(),
            columns: titles
                .iter()
                .map(|t| TableColumn {
                    title: t.clone(),
                    width: COLUMN_WIDTH,
                })
                .collect(),
            rows,
            source: String::new(),
        })
    }

    /// Total width (mm).
    pub fn width(&self) -> f64 {
        self.columns.iter().map(|c| c.width).sum()
    }

    /// Inserts an empty row at `k` (clamped).
    pub fn insert_row(&mut self, k: usize) {
        let n = self.columns.len();
        self.rows
            .insert(k.min(self.rows.len()), vec![String::new(); n]);
    }

    /// Inserts a column at `k` (clamped) with an empty cell in every row.
    pub fn insert_column(&mut self, k: usize) {
        let k = k.min(self.columns.len());
        self.columns
            .insert(k, TableColumn::new(&column_name(self.columns.len())));
        for r in &mut self.rows {
            r.insert(k.min(r.len()), String::new());
        }
    }

    /// Removes column `k`; the last column stays.
    pub fn remove_column(&mut self, k: usize) {
        if self.columns.len() <= 1 || k >= self.columns.len() {
            return;
        }
        self.columns.remove(k);
        for r in &mut self.rows {
            if k < r.len() {
                r.remove(k);
            }
        }
    }
}

/// One CSV line split into cells (quotes and doubled quotes; `,` or `;`
/// when the line has no comma — spreadsheets of decimal-comma locales).
pub fn split_csv(line: &str) -> Vec<String> {
    let sep = if line.contains(',') { ',' } else { ';' };
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
            c if c == sep && !quoted => out.push(std::mem::take(&mut cur)),
            c => cur.push(c),
        }
    }
    out.push(cur);
    out
}

/// The parts-list shape a grid of `columns` is drawn with: anchored at its
/// upper right corner, keys `table:<k>` (every cell centred).
pub(super) fn grid(at: [f64; 2], title: &str, columns: &[Column]) -> PartsList {
    let width: f64 = columns.iter().map(|c| c.width).sum();
    let mut pl = PartsList::new(0, [at[0] + width, at[1]]);
    pl.title = title.to_owned();
    pl.columns = columns.to_vec();
    pl
}

/// Rows of texts as a parts-list table.
pub(super) fn text_table(rows: &[Vec<String>], n: usize) -> Table {
    Table {
        rows: rows
            .iter()
            .enumerate()
            .map(|(k, r)| {
                let cells: Vec<String> = (0..n)
                    .map(|i| r.get(i).cloned().unwrap_or_default())
                    .collect();
                TableRow {
                    key: k.to_string(),
                    overridden: vec![false; n],
                    cells,
                    custom: true,
                    hidden: false,
                    occurrences: Vec::new(),
                    missing: false,
                }
            })
            .collect(),
        items: Default::default(),
    }
}

/// The table's lines and texts.
pub fn prims(t: &GeneralTable, look: &AnnotationLook) -> Vec<Prim> {
    let columns: Vec<Column> = t
        .columns
        .iter()
        .enumerate()
        .map(|(k, c)| Column {
            key: format!("table:{k}"),
            title: c.title.clone(),
            width: c.width,
        })
        .collect();
    table_prims_gap(
        &grid(t.at, &t.title, &columns),
        &text_table(&t.rows, columns.len()),
        look,
        ROW_GAP,
    )
}

/// Height of a grid with `rows` data rows (mm).
pub fn height(rows: usize, title: bool, look: &AnnotationLook) -> f64 {
    let rh = super::parts::row_height_gap(look.leader_text.height, ROW_GAP);
    (rows + 1 + usize::from(title)) as f64 * rh
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_table_has_inventors_size() {
        let t = GeneralTable::empty([20.0, 250.0], 4, 10);
        assert_eq!(t.columns.len(), 4);
        assert_eq!(t.rows.len(), 10);
        assert_eq!(t.title, "TABLE");
        assert!((t.width() - 101.6).abs() < 1e-9);
        assert_eq!(t.columns[3].title, "D");
        // Inventor: 12 rows (title, heading, 10) are 76.82 mm.
        let look = AnnotationLook::default();
        assert!((height(10, true, &look) - 76.82).abs() < 0.05);
    }

    #[test]
    fn csv_reads_headers_and_stops_at_a_blank_row() {
        let csv = "NAME,SIZE,QTY\nBolt,M8x20,4\n\"Nut, hex\",M8,4\n\nWasher,8,8\n";
        let t = GeneralTable::from_csv([0.0, 0.0], csv, true).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            t.columns
                .iter()
                .map(|c| c.title.as_str())
                .collect::<Vec<_>>(),
            ["NAME", "SIZE", "QTY"]
        );
        assert_eq!(t.rows.len(), 2);
        assert_eq!(t.rows[1][0], "Nut, hex");
        let raw = GeneralTable::from_csv([0.0, 0.0], csv, false).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(raw.rows.len(), 3);
        assert_eq!(raw.columns[0].title, "");
        assert!(GeneralTable::from_csv([0.0, 0.0], "\n", true).is_err());
        assert_eq!(split_csv("1,5;2"), ["1", "5;2"]);
        assert_eq!(split_csv("1,5;2,0"), ["1", "5;2", "0"]);
        assert_eq!(split_csv("a;b"), ["a", "b"]);
    }

    #[test]
    fn rows_and_columns_are_inserted_and_removed() {
        let mut t = GeneralTable::empty([0.0, 0.0], 2, 1);
        t.insert_column(1);
        assert_eq!(t.columns.len(), 3);
        assert_eq!(t.rows[0].len(), 3);
        t.insert_row(0);
        assert_eq!(t.rows.len(), 2);
        t.remove_column(0);
        t.remove_column(0);
        t.remove_column(0);
        assert_eq!(t.columns.len(), 1);
        assert_eq!(t.rows[0].len(), 1);
    }

    #[test]
    fn the_table_is_drawn_from_its_upper_left_corner() {
        let mut t = GeneralTable::empty([20.0, 250.0], 2, 1);
        t.rows[0] = vec!["x".into(), "y".into()];
        let look = AnnotationLook::default();
        let p = prims(&t, &look);
        let texts: Vec<&str> = p
            .iter()
            .filter_map(|q| match q {
                Prim::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, ["TABLE", "A", "B", "x", "y"]);
        let xs = p.iter().filter_map(|q| match q {
            Prim::Line { a, b, .. } => Some(a[0].min(b[0])),
            _ => None,
        });
        assert!((xs.fold(f64::MAX, f64::min) - 20.0).abs() < 1e-9);
    }
}
