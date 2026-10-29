//! Bend tables of flat pattern views (`docs/spec/4.8-sheet-metal-drawings.md`
//! §3.4): one row per bend of the view (ID, direction, angle, radius,
//! alternate-unit radius, K-factor) and a tag with the bend's ID on each bend
//! line. Nothing of the bends is stored: rows and tags follow the view's
//! current bends (`ViewLines::bends`, in the flat's bend order).

use serde::{Deserialize, Serialize};

use super::annot::Placement;
use super::hlr::ViewLines;
use super::parts::{Column, PartsList, Table, TableRow, item_text, table_prims_gap};
use super::sheet::{Anchor, Prim};
use super::style::{AnnotationLook, DimensionStyle};

/// Every column a bend table can show (key, heading), in Inventor's order.
pub const COLUMNS: [(&str, &str); 6] = [
    ("bend_id", "BEND ID"),
    ("bend_direction", "BEND DIRECTION"),
    ("bend_angle", "BEND ANGLE"),
    ("bend_radius", "BEND RADIUS"),
    ("bend_alt_radius", "BEND ALT RADIUS"),
    ("bend_kfactor", "BEND KFACTOR"),
];

/// The default heading of a bend column.
pub fn column_title(key: &str) -> String {
    COLUMNS
        .iter()
        .find(|(k, _)| *k == key)
        .map_or_else(|| key.to_uppercase(), |(_, t)| (*t).to_owned())
}

/// A bend column with its default heading and width: Inventor fits the
/// four default columns to their headings (25.4, 40.16, 29.70, 32.07 mm);
/// the other two get widths for their headings at the same rate.
pub fn column(key: &str) -> Column {
    let width = match key {
        "bend_id" => 25.4,
        "bend_direction" => 40.16,
        "bend_angle" => 29.7,
        "bend_radius" => 32.07,
        "bend_alt_radius" => 42.8,
        _ => 34.9,
    };
    Column {
        key: key.into(),
        title: column_title(key),
        width,
    }
}

/// Row gap of Inventor's Table style (mm).
pub const ROW_GAP: f64 = 0.889;

/// Inventor's default columns.
pub fn default_columns() -> Vec<Column> {
    COLUMNS[..4].iter().map(|(k, _)| column(k)).collect()
}

fn default_title() -> String {
    "TABLE".into()
}

fn yes() -> bool {
    true
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_true(b: &bool) -> bool {
    *b
}

/// A bend table on the sheet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BendTable {
    /// The flat pattern view.
    pub view: u32,
    /// Upper left corner (sheet mm).
    pub at: [f64; 2],
    /// Title row text (empty: no title row).
    #[serde(default = "default_title")]
    pub title: String,
    /// Columns in order (keys from [`COLUMNS`]).
    #[serde(default = "default_columns")]
    pub columns: Vec<Column>,
    /// Alphabetic IDs (A, B … AA) instead of numbers.
    #[serde(default, skip_serializing_if = "is_false")]
    pub alpha: bool,
    /// Text before each ID.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub prefix: String,
    /// Tags on the view's bend lines.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub tags: bool,
    /// Bends shown in the other direction (positions in the bend order).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reversed: Vec<usize>,
}

impl BendTable {
    /// A table of view `view` with Inventor's defaults, its upper left
    /// corner at `at`.
    pub fn new(view: u32, at: [f64; 2]) -> Self {
        Self {
            view,
            at,
            title: default_title(),
            columns: default_columns(),
            alpha: false,
            prefix: String::new(),
            tags: true,
            reversed: Vec::new(),
        }
    }

    /// The ID of the bend at position `k` (from 0) in the bend order.
    pub fn id(&self, k: usize) -> String {
        format!("{}{}", self.prefix, item_text(k + 1, 1, self.alpha, false))
    }

    /// Is bend `k` shown up (after Reverse Direction)?
    pub fn up(&self, k: usize, up: bool) -> bool {
        up != self.reversed.contains(&k)
    }

    /// Total width (mm).
    pub fn width(&self) -> f64 {
        self.columns.iter().map(|c| c.width).sum()
    }

    /// The parts-list shape the table is drawn with (anchored at its upper
    /// right corner).
    fn grid(&self) -> PartsList {
        let mut pl = PartsList::new(self.view, [self.at[0] + self.width(), self.at[1]]);
        pl.title = self.title.clone();
        pl.columns = self.columns.clone();
        pl
    }
}

/// A number in the dimension style's precision and decimal marker, without
/// trailing zeros (Inventor's bend table shows 90 and 2).
fn num(v: f64, precision: usize, d: &DimensionStyle) -> String {
    let mut t = format!("{v:.precision$}");
    if t.contains('.') {
        t = t.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    if d.decimal_comma {
        t = t.replace('.', ",");
    }
    t
}

/// The table's rows over `lines` (a flat pattern view's lines).
pub fn rows(bt: &BendTable, lines: &ViewLines, d: &DimensionStyle) -> Table {
    let p = usize::from(d.precision);
    let rows = lines
        .bends
        .iter()
        .enumerate()
        .map(|(k, b)| {
            let cells = bt
                .columns
                .iter()
                .map(|c| match c.key.as_str() {
                    "bend_id" => bt.id(k),
                    "bend_direction" => if bt.up(k, b.up) { "UP" } else { "DOWN" }.into(),
                    "bend_angle" => num(b.angle, p, d),
                    "bend_radius" => num(b.radius, p, d),
                    "bend_alt_radius" => format!("{} in", num(b.radius / 25.4, 3, d)),
                    "bend_kfactor" => num(b.kfactor, 3, d),
                    _ => String::new(),
                })
                .collect::<Vec<_>>();
            TableRow {
                key: k.to_string(),
                overridden: vec![false; cells.len()],
                cells,
                custom: false,
                hidden: false,
                occurrences: Vec::new(),
                missing: false,
            }
        })
        .collect();
    Table {
        rows,
        items: Default::default(),
    }
}

/// The table drawn, and the tags on the view's bend lines (`pl` places the
/// view).
pub fn prims(bt: &BendTable, lines: &ViewLines, pl: Placement, look: &AnnotationLook) -> Vec<Prim> {
    let t = rows(bt, lines, &look.dim);
    let mut out = table_prims_gap(&bt.grid(), &t, look, ROW_GAP);
    if bt.tags {
        out.extend(tag_prims(bt, lines, pl, look));
    }
    out
}

/// The tags: each bend's ID beside the middle of its centre line, past
/// the bend zone on the side away from a bend note (which sits above the
/// line), upright.
pub fn tag_prims(
    bt: &BendTable,
    lines: &ViewLines,
    pl: Placement,
    look: &AnnotationLook,
) -> Vec<Prim> {
    let h = look.leader_text.height;
    lines
        .bends
        .iter()
        .enumerate()
        .map(|(k, b)| {
            let a = pl.to_sheet(b.centre[0]);
            let c = pl.to_sheet(b.centre[1]);
            let mut d = [c[0] - a[0], c[1] - a[1]];
            // The same "up" side as the bend note's.
            if d[0] < -1e-9 || (d[0].abs() <= 1e-9 && d[1] < 0.0) {
                d = [-d[0], -d[1]];
            }
            let len = d[0].hypot(d[1]).max(1e-9);
            let n = [-d[1] / len, d[0] / len];
            let half = {
                let (p, q) = (pl.to_sheet(b.zone[0][0]), pl.to_sheet(b.zone[1][0]));
                (p[0] - q[0]).hypot(p[1] - q[1]) * 0.5
            };
            let mid = [(a[0] + c[0]) * 0.5, (a[1] + c[1]) * 0.5];
            let off = half + 1.0 + h * 0.6;
            let centre = [mid[0] - n[0] * off, mid[1] - n[1] * off];
            Prim::Text {
                at: [centre[0], centre[1] - h * 0.5],
                height: h,
                text: bt.id(k),
                anchor: Anchor::Center,
                angle: 0.0,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drawing::hlr::ViewBend;

    fn bend(up: bool, x: f64) -> ViewBend {
        ViewBend {
            centre: [[x, 0.0], [x, 50.0]],
            zone: [
                [[x - 1.0, 0.0], [x - 1.0, 50.0]],
                [[x + 1.0, 0.0], [x + 1.0, 50.0]],
            ],
            up,
            angle: 90.0,
            radius: 2.0,
            kfactor: 0.44,
        }
    }

    fn lines() -> ViewLines {
        ViewLines {
            bends: vec![bend(true, 0.0), bend(false, 40.0)],
            ..Default::default()
        }
    }

    #[test]
    fn rows_follow_the_bends() {
        let mut bt = BendTable::new(1, [400.0, 280.0]);
        let d = DimensionStyle::default();
        let t = rows(&bt, &lines(), &d);
        assert_eq!(t.rows.len(), 2);
        assert_eq!(t.rows[0].cells[0], "1");
        assert_eq!(t.rows[0].cells[1], "UP");
        assert_eq!(t.rows[1].cells[1], "DOWN");
        assert_eq!(t.rows[0].cells[2], "90");
        assert_eq!(t.rows[0].cells[3], "2");
        // Alphabetic with a prefix, a reversed bend, the K-factor column.
        bt.alpha = true;
        bt.prefix = "B".into();
        bt.reversed = vec![1];
        bt.columns.push(column("bend_kfactor"));
        let t = rows(&bt, &lines(), &d);
        assert_eq!(t.rows[1].cells[0], "BB");
        assert_eq!(t.rows[1].cells[1], "UP");
        assert!(t.rows[0].cells[4] == "0.44" || t.rows[0].cells[4] == "0,44");
    }

    #[test]
    fn tags_sit_on_each_bend() {
        let bt = BendTable::new(1, [400.0, 280.0]);
        let pl = Placement {
            breaks: Default::default(),
            position: [100.0, 100.0],
            scale: 1.0,
            center: [20.0, 25.0],
        };
        let tags = tag_prims(&bt, &lines(), pl, &AnnotationLook::default());
        assert_eq!(tags.len(), 2);
        let Prim::Text { text, at, .. } = &tags[1] else {
            panic!("a text")
        };
        assert_eq!(text, "2");
        // Bend 2's line is at x = 40 → sheet x = 120; the tag is right of
        // its zone (the note goes left), at the line's middle.
        assert!(at[0] > 121.0 && at[0] < 128.0);
        assert!((at[1] + 3.5 * 0.5 - 100.0).abs() < 1e-9);
    }

    #[test]
    fn layout_matches_inventor_defaults() {
        // One bend: title, headings and one row of 6.40 mm; four columns
        // 127.33 mm wide; the upper left corner at `at`.
        let bt = BendTable::new(1, [300.0, 280.0]);
        let one = ViewLines {
            bends: vec![bend(false, 0.0)],
            ..Default::default()
        };
        let pl = Placement {
            breaks: Default::default(),
            position: [100.0, 100.0],
            scale: 1.0,
            center: [0.0, 25.0],
        };
        let mut bt_no_tags = bt.clone();
        bt_no_tags.tags = false;
        let prims = prims(&bt_no_tags, &one, pl, &AnnotationLook::default());
        let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for p in &prims {
            if let Prim::Line { a, b, .. } = p {
                for q in [a, b] {
                    for k in 0..2 {
                        lo[k] = lo[k].min(q[k]);
                        hi[k] = hi[k].max(q[k]);
                    }
                }
            }
        }
        assert!((lo[0] - 300.0).abs() < 1e-9 && (hi[1] - 280.0).abs() < 1e-9);
        assert!((hi[0] - lo[0] - 127.33).abs() < 1e-6);
        assert!((hi[1] - lo[1] - 19.206).abs() < 0.01);
    }

    #[test]
    fn serde_keeps_defaults_short() {
        let bt = BendTable::new(3, [1.0, 2.0]);
        let j = serde_json::to_string(&bt).unwrap_or_default();
        assert!(!j.contains("tags") && !j.contains("alpha") && !j.contains("reversed"));
        let back: BendTable =
            serde_json::from_str(&j).unwrap_or_else(|_| BendTable::new(0, [0.0; 2]));
        assert_eq!(back, bt);
    }
}
