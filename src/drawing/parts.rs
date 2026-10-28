//! Parts lists and balloons (spec 4.7 §3.5, `docs/spec/4.4-parts-list.md`):
//! the table of an assembly view's BOM and the item balloons that point at
//! its components.
//!
//! Nothing of the BOM is stored: rows come from `assembly::bom` every time
//! (through a callback, so this module stays GUI-free). The parts list
//! stores only what the user decided — columns, sort, cell overrides by
//! part number, hidden rows, custom rows, wrapping — so a changed assembly
//! updates the table and keeps every override. Balloons read their item
//! numbers through the parts list of the same model (its sort, renumbering
//! and Item overrides), else through the BOM's own order.
//!
//! Layout follows Inventor's ISO parts lists and balloons: title
//! "PARTS LIST" at the top, rows top-down,
//! columns ITEM 35, QTY 25, PART NUMBER 55, DESCRIPTION 55 mm, 3.5 mm text
//! with 0.9 mm row gaps (rows 6.42 mm); balloons 5.25 mm across, stretched
//! to their text, on a filled-arrow leader.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use super::annot::{self, add, mul, norm, sub};
use super::hlr::ViewLines;
use super::sheet::{Anchor, Prim, Weight};
use super::style::AnnotationLook;
use super::{DrawingDocument, Sheet};
use crate::assembly::bom::{BomRow, BomView};
use crate::document::DocRef;

/// The BOM view a parts list or balloon reads (Inventor's BOM Settings).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BomKind {
    /// First level of the structured BOM.
    #[default]
    Structured,
    /// Parts only, quantities summed.
    PartsOnly,
}

impl BomKind {
    /// The assembly BOM view.
    pub fn view(self) -> BomView {
        match self {
            BomKind::Structured => BomView::Structured,
            BomKind::PartsOnly => BomView::PartsOnly,
        }
    }

    /// Display name.
    pub fn label(self) -> &'static str {
        match self {
            BomKind::Structured => "Structured",
            BomKind::PartsOnly => "Parts Only",
        }
    }
}

/// One column of a parts list.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Column {
    /// What it shows: `item`, `qty`, `unit`, `part_number`, `structure`,
    /// `description`, `stock_number`, `material`, a standard iProperty key
    /// (`iprops::STANDARD`) or `custom:<name>` (a custom iProperty).
    pub key: String,
    /// Heading text.
    pub title: String,
    /// Width on paper (mm).
    pub width: f64,
}

impl Column {
    /// A column with its default heading and width.
    pub fn new(key: &str) -> Self {
        let width = match key {
            "item" => 35.0,
            "qty" | "unit" => 25.0,
            _ => 55.0,
        };
        Self {
            key: key.into(),
            title: column_title(key),
            width,
        }
    }
}

/// The default heading of a column key.
pub fn column_title(key: &str) -> String {
    match key {
        "item" => "ITEM".into(),
        "qty" => "QTY".into(),
        "unit" => "UNIT QTY".into(),
        "part_number" => "PART NUMBER".into(),
        "structure" => "BOM STRUCTURE".into(),
        "material" => "MATERIAL".into(),
        _ => match key.strip_prefix("custom:") {
            Some(n) => n.to_uppercase(),
            None => crate::document::iprops::STANDARD
                .iter()
                .find(|(k, ..)| *k == key)
                .map_or_else(|| key.to_uppercase(), |(_, _, l)| l.to_uppercase()),
        },
    }
}

/// Inventor's default columns.
pub fn default_columns() -> Vec<Column> {
    ["item", "qty", "part_number", "description"]
        .iter()
        .map(|k| Column::new(k))
        .collect()
}

/// Every column a parts list of `rows` can show: the BOM's own, every
/// standard iProperty, then the custom iProperties used.
pub fn available_columns(rows: &[BomRow]) -> Vec<(String, String)> {
    let mut keys: Vec<String> = [
        "item",
        "qty",
        "unit",
        "part_number",
        "structure",
        "description",
        "stock_number",
        "material",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    for (k, ..) in crate::document::iprops::STANDARD {
        if !keys.iter().any(|x| x == k) {
            keys.push((*k).to_owned());
        }
    }
    for c in crate::assembly::bom::custom_columns(rows) {
        keys.push(format!("custom:{c}"));
    }
    keys.into_iter()
        .map(|k| {
            let t = column_title(&k);
            (k, t)
        })
        .collect()
}

/// Sort order of a parts list.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SortBy {
    /// The column key.
    pub column: String,
    /// Ascending (else descending).
    #[serde(default = "yes")]
    pub ascending: bool,
    /// Item numbers follow the new order (Inventor's Renumber Items).
    #[serde(default)]
    pub renumber: bool,
}

/// A cell typed over the BOM's value; kept by part number, so it survives
/// a changed or reordered assembly (shown coloured in the editor).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CellOverride {
    /// The row's part number.
    pub row: String,
    /// The column key.
    pub column: String,
    /// The text shown.
    pub text: String,
}

/// A row the user added (not from the BOM), cells by column key.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CustomRow {
    /// Cell texts by column key.
    #[serde(default)]
    pub cells: BTreeMap<String, String>,
}

/// Table Wrapping.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Wrap {
    /// New sections go to the left (else to the right).
    #[serde(default = "yes")]
    pub left: bool,
    /// Enable Automatic Wrap: a new section after this many rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_rows: Option<u32>,
    /// Number of Sections (rows shared evenly), when not automatic.
    #[serde(default = "one_u32")]
    pub sections: u32,
}

impl Default for Wrap {
    fn default() -> Self {
        Self {
            left: true,
            max_rows: None,
            sections: 1,
        }
    }
}

/// A parts list on a sheet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PartsList {
    /// The view whose model is the source.
    pub view: u32,
    /// The table's upper right corner (sheet mm; Inventor's Position).
    pub at: [f64; 2],
    /// BOM view.
    #[serde(default)]
    pub bom: BomKind,
    /// Part Number Row Merge.
    #[serde(default = "yes")]
    pub merge: bool,
    /// Minimum Digits of item numbers (1–6).
    #[serde(default = "one_u8")]
    pub min_digits: u8,
    /// Alphabetic item numbers (A, B, … AA).
    #[serde(default)]
    pub alpha: bool,
    /// Lower case letters.
    #[serde(default)]
    pub lower: bool,
    /// Title row text (empty: no title row).
    #[serde(default = "default_title")]
    pub title: String,
    /// Columns, left to right.
    #[serde(default = "default_columns")]
    pub columns: Vec<Column>,
    /// Sort.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort: Option<SortBy>,
    /// Cell overrides.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overrides: Vec<CellOverride>,
    /// Hidden rows by part number (they keep their item numbers).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden: Vec<String>,
    /// Rows added by the user, after the BOM's.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_rows: Vec<CustomRow>,
    /// Table Wrapping.
    #[serde(default)]
    pub wrap: Wrap,
}

fn yes() -> bool {
    true
}

fn one_u8() -> u8 {
    1
}

fn one_u32() -> u32 {
    1
}

fn default_title() -> String {
    "PARTS LIST".into()
}

impl PartsList {
    /// A parts list of view `view` with Inventor's defaults.
    pub fn new(view: u32, at: [f64; 2]) -> Self {
        Self {
            view,
            at,
            bom: BomKind::Structured,
            merge: true,
            min_digits: 1,
            alpha: false,
            lower: false,
            title: default_title(),
            columns: default_columns(),
            sort: None,
            overrides: Vec::new(),
            hidden: Vec::new(),
            custom_rows: Vec::new(),
            wrap: Wrap::default(),
        }
    }

    /// The override of a cell, if any.
    pub fn override_of(&self, row: &str, column: &str) -> Option<&str> {
        self.overrides
            .iter()
            .find(|o| o.row == row && o.column == column)
            .map(|o| o.text.as_str())
    }

    /// Sets (or with `None` removes) a cell override.
    pub fn set_override(&mut self, row: &str, column: &str, text: Option<String>) {
        self.overrides
            .retain(|o| !(o.row == row && o.column == column));
        if let Some(text) = text {
            self.overrides.push(CellOverride {
                row: row.into(),
                column: column.into(),
                text,
            });
        }
    }

    /// The total width of one section (mm).
    pub fn width(&self) -> f64 {
        self.columns.iter().map(|c| c.width).sum()
    }
}

/// An item number as text: `n` from 1, zero-padded to `digits`, or letters.
pub fn item_text(n: usize, digits: u8, alpha: bool, lower: bool) -> String {
    if alpha {
        let mut s = Vec::new();
        let mut k = n;
        while k > 0 {
            let r = (k - 1) % 26;
            s.push(if lower { b'a' } else { b'A' } + r as u8);
            k = (k - 1) / 26;
        }
        s.reverse();
        String::from_utf8(s).unwrap_or_default()
    } else {
        format!("{n:0width$}", width = usize::from(digits.clamp(1, 6)))
    }
}

/// The BOM value of a row's cell.
pub fn bom_cell(r: &BomRow, key: &str) -> String {
    match key {
        "item" => r.item.clone(),
        "qty" => r.qty.to_string(),
        "unit" => r.unit.clone(),
        "part_number" => r.part_number.clone(),
        "structure" => r.structure.label().into(),
        "description" => r.description.clone(),
        "stock_number" => r.stock_number.clone(),
        "material" => r.material.clone(),
        _ => match key.strip_prefix("custom:") {
            Some(n) => r.custom.get(n).cloned().unwrap_or_default(),
            None => r.props.get(key).cloned().unwrap_or_default(),
        },
    }
}

/// One row of a computed table.
#[derive(Clone, Debug, PartialEq)]
pub struct TableRow {
    /// The row's key: its part number (custom rows: `custom:<k>`).
    pub key: String,
    /// Cell texts, one per column.
    pub cells: Vec<String>,
    /// The cells that show an override.
    pub overridden: Vec<bool>,
    /// A row added by the user.
    pub custom: bool,
    /// Hidden (not drawn).
    pub hidden: bool,
    /// The occurrences it counts.
    pub occurrences: Vec<u32>,
    /// A component of it is missing.
    pub missing: bool,
}

/// A parts list's rows, and the item number of each occurrence.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Table {
    /// Rows in display order (hidden ones included, flagged).
    pub rows: Vec<TableRow>,
    /// Item and quantity texts by occurrence id.
    pub items: HashMap<u32, (String, String)>,
}

/// The table of parts list `pl` over the BOM `rows`.
pub fn table(pl: &PartsList, rows: &[BomRow]) -> Table {
    let mut rows: Vec<BomRow> = rows.to_vec();
    if let Some(s) = &pl.sort {
        let key = |r: &BomRow| bom_cell(r, &s.column);
        rows.sort_by(|a, b| {
            let (x, y) = (key(a), key(b));
            let o = match (x.parse::<f64>(), y.parse::<f64>()) {
                (Ok(p), Ok(q)) => p.total_cmp(&q),
                _ => x.to_lowercase().cmp(&y.to_lowercase()),
            };
            if s.ascending { o } else { o.reverse() }
        });
        if s.renumber {
            for (k, r) in rows.iter_mut().enumerate() {
                r.item = (k + 1).to_string();
            }
        }
    }
    let mut out = Table::default();
    for r in &rows {
        let n: usize = r.item.parse().unwrap_or(0);
        let mut row_item = if n > 0 {
            item_text(n, pl.min_digits, pl.alpha, pl.lower)
        } else {
            r.item.clone()
        };
        if let Some(o) = pl.override_of(&r.part_number, "item") {
            row_item = o.to_owned();
        }
        let qty = pl
            .override_of(&r.part_number, "qty")
            .map_or_else(|| r.qty.to_string(), str::to_owned);
        let mut cells = Vec::new();
        let mut overridden = Vec::new();
        for c in &pl.columns {
            let o = pl.override_of(&r.part_number, &c.key);
            overridden.push(o.is_some());
            cells.push(match (c.key.as_str(), o) {
                (_, Some(t)) => t.to_owned(),
                ("item", None) => row_item.clone(),
                (k, None) => bom_cell(r, k),
            });
        }
        for o in &r.occurrences {
            // A subassembly's own row comes before the rows inside it.
            out.items
                .entry(*o)
                .or_insert_with(|| (row_item.clone(), qty.clone()));
        }
        out.rows.push(TableRow {
            key: r.part_number.clone(),
            cells,
            overridden,
            custom: false,
            hidden: pl.hidden.contains(&r.part_number),
            occurrences: r.occurrences.clone(),
            missing: r.missing,
        });
    }
    for (k, c) in pl.custom_rows.iter().enumerate() {
        let key = format!("custom:{k}");
        out.rows.push(TableRow {
            cells: pl
                .columns
                .iter()
                .map(|col| c.cells.get(&col.key).cloned().unwrap_or_default())
                .collect(),
            overridden: vec![false; pl.columns.len()],
            custom: true,
            hidden: pl.hidden.contains(&key),
            key,
            occurrences: Vec::new(),
            missing: false,
        });
    }
    out
}

/// Row height of a table with text `h` high (Inventor ISO: 6.42 mm for
/// 3.5 mm text and 0.9 mm row gaps).
pub fn row_height(h: f64) -> f64 {
    row_height_gap(h, 0.9)
}

/// Row height for text `h` high with `gap` above and below it (Inventor's
/// general Table style: 0.889 mm).
pub fn row_height_gap(h: f64, gap: f64) -> f64 {
    1.321 * h + 2.0 * gap
}

/// The rows of each section (indices into the shown rows).
fn sections(pl: &PartsList, n: usize) -> Vec<std::ops::Range<usize>> {
    let per = match pl.wrap.max_rows {
        Some(m) if m > 0 => m as usize,
        _ => n.div_ceil(pl.wrap.sections.max(1) as usize).max(1),
    };
    (0..n.max(1))
        .step_by(per)
        .map(|s| s..(s + per).min(n))
        .collect()
}

/// The table's outline on the sheet: lower left and upper right corners.
pub fn bounds(pl: &PartsList, t: &Table, h: f64) -> ([f64; 2], [f64; 2]) {
    let shown = t.rows.iter().filter(|r| !r.hidden).count();
    let secs = sections(pl, shown);
    let longest = secs.iter().map(|s| s.len()).max().unwrap_or(0);
    let rh = row_height(h);
    let title = usize::from(!pl.title.is_empty());
    let height = (longest + 1 + title) as f64 * rh;
    let w = pl.width() * secs.len().max(1) as f64;
    let (x0, x1) = if pl.wrap.left {
        (pl.at[0] - w, pl.at[0])
    } else {
        (pl.at[0] - pl.width(), pl.at[0] - pl.width() + w)
    };
    ([x0, pl.at[1] - height], [x1, pl.at[1]])
}

/// The table drawn: title row, heading row and the shown rows, in sections.
pub fn table_prims(pl: &PartsList, t: &Table, look: &AnnotationLook) -> Vec<Prim> {
    table_prims_gap(pl, t, look, 0.9)
}

/// [`table_prims`] with row gap `gap` (mm).
pub fn table_prims_gap(pl: &PartsList, t: &Table, look: &AnnotationLook, gap: f64) -> Vec<Prim> {
    let h = look.leader_text.height;
    let rh = row_height_gap(h, gap);
    let width = pl.width();
    let shown: Vec<&TableRow> = t.rows.iter().filter(|r| !r.hidden).collect();
    let mut out = Vec::new();
    let text = |out: &mut Vec<Prim>, x0: f64, w: f64, y: f64, s: &str, center: bool| {
        if s.is_empty() {
            return;
        }
        let at = if center {
            [x0 + w * 0.5, y + (rh - h) * 0.5]
        } else {
            [x0 + 1.0, y + (rh - h) * 0.5]
        };
        out.push(Prim::Text {
            at,
            height: h,
            text: s.to_owned(),
            anchor: if center { Anchor::Center } else { Anchor::Left },
            angle: 0.0,
        });
    };
    for (si, range) in sections(pl, shown.len()).into_iter().enumerate() {
        let right = if pl.wrap.left {
            pl.at[0] - si as f64 * width
        } else {
            pl.at[0] + si as f64 * width
        };
        let left = right - width;
        let mut y = pl.at[1];
        let line = |out: &mut Vec<Prim>, a: [f64; 2], b: [f64; 2], weight: Weight| {
            out.push(Prim::Line { a, b, weight });
        };
        if !pl.title.is_empty() {
            y -= rh;
            text(&mut out, left, width, y, &pl.title, true);
            line(&mut out, [left, y], [right, y], Weight::Thin);
        }
        let table_top = y;
        y -= rh;
        let mut x = left;
        for c in &pl.columns {
            text(&mut out, x, c.width, y, &c.title, true);
            x += c.width;
        }
        line(&mut out, [left, y], [right, y], Weight::Medium);
        for r in &shown[range] {
            y -= rh;
            let mut x = left;
            for (c, s) in pl.columns.iter().zip(&r.cells) {
                // Bend, general and revision tables centre every value
                // (Inventor's Table styles).
                let center = matches!(c.key.as_str(), "item" | "qty" | "unit")
                    || ["bend_", "table:", "rev:"]
                        .iter()
                        .any(|p| c.key.starts_with(p));
                text(&mut out, x, c.width, y, s, center);
                x += c.width;
            }
            line(&mut out, [left, y], [right, y], Weight::Thin);
        }
        // Column lines and the outline.
        let mut x = left;
        for c in &pl.columns[..pl.columns.len().saturating_sub(1)] {
            x += c.width;
            line(&mut out, [x, table_top], [x, y], Weight::Thin);
        }
        for (a, b) in [
            ([left, y], [right, y]),
            ([right, y], [right, pl.at[1]]),
            ([right, pl.at[1]], [left, pl.at[1]]),
            ([left, pl.at[1]], [left, y]),
        ] {
            line(&mut out, a, b, Weight::Medium);
        }
    }
    out
}

/// Balloon shapes (Inventor's Balloon Shape list).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BalloonShape {
    /// Circular - 1 Entry.
    #[default]
    Circle,
    /// Circular - 2 Entries: item above quantity.
    CircleTwo,
    /// Hexagon.
    Hexagon,
    /// Linear: the text on the leader's shoulder.
    Linear,
}

impl BalloonShape {
    /// Every shape, in Inventor's order.
    pub const ALL: [BalloonShape; 4] = [
        BalloonShape::Circle,
        BalloonShape::CircleTwo,
        BalloonShape::Hexagon,
        BalloonShape::Linear,
    ];

    /// Display name.
    pub fn label(self) -> &'static str {
        match self {
            BalloonShape::Circle => "Circular - 1 Entry",
            BalloonShape::CircleTwo => "Circular - 2 Entries",
            BalloonShape::Hexagon => "Hexagon",
            BalloonShape::Linear => "Linear",
        }
    }
}

/// An item balloon: a leader from a component's edge to a balloon with the
/// component's item number.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Balloon {
    /// The view.
    pub view: u32,
    /// The edge the leader starts on (`o<occurrence>/<edge>`).
    pub edge: String,
    /// Where on the edge: 0–1 from its start to its end (a circle: the
    /// angle as a fraction of a turn).
    #[serde(default = "half")]
    pub along: f64,
    /// The balloon's centre relative to the view's position (sheet mm), so
    /// it moves with the view.
    pub at: [f64; 2],
    /// Shape.
    #[serde(default)]
    pub shape: BalloonShape,
    /// The BOM view the item number comes from.
    #[serde(default)]
    pub bom: BomKind,
    /// Text typed over the item number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

fn half() -> f64 {
    0.5
}

/// Inventor ISO balloon diameter for 3.5 mm text.
pub const BALLOON_DIAMETER: f64 = 5.25;

/// The point of `edge` at `along` (view plane).
pub fn edge_point(e: &super::hlr::ViewEdge, along: f64) -> [f64; 2] {
    match e.circle {
        Some((c, r)) if !e.straight && e.full_circle => {
            let t = std::f64::consts::TAU * along;
            [c[0] + r * t.cos(), c[1] + r * t.sin()]
        }
        _ => add(e.a, mul(sub(e.b, e.a), along.clamp(0.0, 1.0))),
    }
}

/// Where `q` (view plane) lies along edge `e` (inverse of [`edge_point`]).
pub fn edge_along(e: &super::hlr::ViewEdge, q: [f64; 2]) -> f64 {
    match e.circle {
        Some((c, _)) if !e.straight && e.full_circle => {
            let a = (q[1] - c[1]).atan2(q[0] - c[0]);
            a.rem_euclid(std::f64::consts::TAU) / std::f64::consts::TAU
        }
        _ => {
            let d = sub(e.b, e.a);
            let l2 = d[0] * d[0] + d[1] * d[1];
            if l2 < 1e-18 {
                0.5
            } else {
                (((q[0] - e.a[0]) * d[0] + (q[1] - e.a[1]) * d[1]) / l2).clamp(0.0, 1.0)
            }
        }
    }
}

/// A balloon drawn: `tip` on the edge, `c` the centre (sheet mm), its
/// texts (item, quantity).
pub fn balloon_prims(
    shape: BalloonShape,
    tip: [f64; 2],
    c: [f64; 2],
    item: &str,
    qty: &str,
    look: &AnnotationLook,
) -> Vec<Prim> {
    let h = look.leader_text.height;
    let w = annot::text_width(item, h) * look.leader_text.width_scale;
    let base = BALLOON_DIAMETER * h / 3.5 * 0.5;
    let mut out = Vec::new();
    let weight = look.dim_weight;
    let r = match shape {
        BalloonShape::CircleTwo => {
            let w2 = annot::text_width(qty, h) * look.leader_text.width_scale;
            (w.max(w2) * 0.5 + 0.8).max(h * 1.2).max(base)
        }
        _ => (w * 0.5 + 0.6).max(base),
    };
    let d = sub(c, tip);
    let len = norm(d);
    let dir = if len < 1e-9 {
        [1.0, 0.0]
    } else {
        mul(d, 1.0 / len)
    };
    let rim = match shape {
        BalloonShape::Linear => {
            // The leader ends on the near end of the underline.
            let sx = if dir[0] >= 0.0 { -1.0 } else { 1.0 };
            [c[0] + sx * (w * 0.5 + 0.6), c[1] - h * 0.5 - 0.6]
        }
        _ => sub(c, mul(dir, r)),
    };
    out.push(annot::line(tip, rim));
    let l = &look.leader;
    annot::terminator(
        &mut out,
        tip,
        mul(dir, -1.0),
        l.terminator,
        l.arrow_size,
        l.arrow_height,
    );
    let ring = |n: usize, rr: f64, phase: f64| -> Vec<[f64; 2]> {
        (0..=n)
            .map(|k| {
                let a = phase + std::f64::consts::TAU * k as f64 / n as f64;
                [c[0] + rr * a.cos(), c[1] + rr * a.sin()]
            })
            .collect()
    };
    let text = |out: &mut Vec<Prim>, s: &str, y: f64| {
        out.push(Prim::Text {
            at: [c[0], y],
            height: h,
            text: s.to_owned(),
            anchor: Anchor::Center,
            angle: 0.0,
        });
    };
    match shape {
        BalloonShape::Circle => {
            out.push(Prim::Path {
                pts: ring(48, r, 0.0),
                width: weight,
                dashed: false,
            });
            text(&mut out, item, c[1] - h * 0.5);
        }
        BalloonShape::Hexagon => {
            // Flat top and bottom, the circle inscribed.
            out.push(Prim::Path {
                pts: ring(6, r / (std::f64::consts::PI / 6.0).cos(), 0.0),
                width: weight,
                dashed: false,
            });
            text(&mut out, item, c[1] - h * 0.5);
        }
        BalloonShape::CircleTwo => {
            out.push(Prim::Path {
                pts: ring(48, r, 0.0),
                width: weight,
                dashed: false,
            });
            out.push(Prim::Path {
                pts: vec![[c[0] - r, c[1]], [c[0] + r, c[1]]],
                width: weight,
                dashed: false,
            });
            let gap = (r - h) * 0.5;
            text(&mut out, item, c[1] + gap.max(0.3));
            text(&mut out, qty, c[1] - gap.max(0.3) - h);
        }
        BalloonShape::Linear => {
            let x = w * 0.5 + 0.6;
            out.push(Prim::Path {
                pts: vec![
                    [c[0] - x, c[1] - h * 0.5 - 0.6],
                    [c[0] + x, c[1] - h * 0.5 - 0.6],
                ],
                width: weight,
                dashed: false,
            });
            text(&mut out, item, c[1] - h * 0.5);
        }
    }
    out
}

/// The BOM rows of a model (callback of [`Tables::build`]).
pub type BomRows<'a> = &'a dyn Fn(DocRef, BomKind, bool) -> Option<Vec<BomRow>>;

/// What parts lists and balloons of a sheet read, computed once per frame:
/// the models of the views, the BOM rows of each model, and the item
/// numbers (through the drawing's parts lists).
#[derive(Clone, Debug, Default)]
pub struct Tables {
    /// The model of each view of the sheet.
    pub view_model: HashMap<u32, DocRef>,
    /// BOM rows by (model, BOM view, merge).
    pub rows: HashMap<(DocRef, BomKind, bool), Vec<BomRow>>,
    /// Item and quantity by (model, BOM view) and occurrence.
    pub items: HashMap<(DocRef, BomKind), HashMap<u32, (String, String)>>,
}

impl Tables {
    /// The tables of sheet `i` of `doc`.
    pub fn build(doc: &DrawingDocument, i: usize, bom: BomRows) -> Self {
        let mut t = Tables::default();
        let Some(sheet) = doc.sheets.get(i) else {
            return t;
        };
        for v in &sheet.views {
            if let Some(m) = doc.model_of(sheet, v.id) {
                t.view_model.insert(v.id, m);
            }
        }
        let rows_of = |t: &mut Tables, m: DocRef, k: BomKind, merge: bool| {
            if !t.rows.contains_key(&(m, k, merge))
                && let Some(r) = bom(m, k, merge)
            {
                t.rows.insert((m, k, merge), r);
            }
        };
        // The parts lists of this sheet first (their items win), then those
        // of the other sheets.
        let order = std::iter::once(i).chain((0..doc.sheets.len()).filter(|&k| k != i));
        for si in order {
            let s: &Sheet = &doc.sheets[si];
            for a in &s.annotations {
                let super::AnnotationKind::PartsList(pl) = &a.kind else {
                    continue;
                };
                let Some(m) = doc.model_of(s, pl.view) else {
                    continue;
                };
                rows_of(&mut t, m, pl.bom, pl.merge);
                if si != i && t.items.contains_key(&(m, pl.bom)) {
                    continue;
                }
                if let Some(r) = t.rows.get(&(m, pl.bom, pl.merge)) {
                    let tab = table(pl, r);
                    t.items.entry((m, pl.bom)).or_insert(tab.items);
                }
            }
        }
        // Balloons without a parts list: the BOM's own numbers.
        for a in &sheet.annotations {
            let super::AnnotationKind::Balloon(b) = &a.kind else {
                continue;
            };
            let Some(&m) = t.view_model.get(&b.view) else {
                continue;
            };
            if t.items.contains_key(&(m, b.bom)) {
                continue;
            }
            rows_of(&mut t, m, b.bom, true);
            if let Some(r) = t.rows.get(&(m, b.bom, true)) {
                let tab = table(&PartsList::new(b.view, [0.0; 2]), r);
                t.items.insert((m, b.bom), tab.items);
            }
        }
        t
    }

    /// The table of parts list `pl`, when its view's BOM is known.
    pub fn table_of(&self, pl: &PartsList) -> Option<Table> {
        let m = self.view_model.get(&pl.view)?;
        self.rows.get(&(*m, pl.bom, pl.merge)).map(|r| table(pl, r))
    }

    /// The item and quantity of occurrence `occ` in view `view`'s model.
    pub fn item(&self, view: u32, kind: BomKind, occ: u32) -> Option<&(String, String)> {
        let m = self.view_model.get(&view)?;
        self.items.get(&(*m, kind))?.get(&occ)
    }
}

/// A balloon resolved: its prims, or why not.
pub fn resolve_balloon(
    b: &Balloon,
    pl: annot::Placement,
    lines: &ViewLines,
    tables: &Tables,
    look: &AnnotationLook,
) -> Result<Vec<Prim>, annot::Orphan> {
    let missing = || annot::Orphan::Missing(b.edge.clone());
    let e = lines.edge(&b.edge).ok_or_else(missing)?;
    let occ = super::hlr::occurrence_of(&b.edge).ok_or_else(missing)?;
    let (item, qty) = tables.item(b.view, b.bom, occ).ok_or_else(missing)?;
    let item = b.text.as_deref().unwrap_or(item);
    let tip = pl.to_sheet(edge_point(e, b.along));
    let c = add(pl.position, b.at);
    Ok(balloon_prims(b.shape, tip, c, item, qty, look))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::iprops::BomStructure;

    fn row(item: &str, pn: &str, qty: u32, occ: &[u32]) -> BomRow {
        BomRow {
            item: item.into(),
            level: 0,
            part_number: pn.into(),
            structure: BomStructure::Normal,
            qty,
            unit: "Each".into(),
            stock_number: String::new(),
            description: format!("{pn} desc"),
            material: "Steel".into(),
            components: Vec::new(),
            occurrences: occ.to_vec(),
            custom: BTreeMap::from([("Grade".to_owned(), "8.8".to_owned())]),
            props: BTreeMap::from([("vendor".to_owned(), "ACME".to_owned())]),
            missing: false,
        }
    }

    fn rows() -> Vec<BomRow> {
        vec![
            row("1", "plate", 1, &[1]),
            row("2", "bolt", 3, &[2, 3, 4]),
            row("3", "nut", 2, &[5, 6]),
        ]
    }

    /// Item numbers: digits, letters, sort with and without renumbering,
    /// an Item override that balloons follow.
    #[test]
    fn numbering_sort_and_overrides() {
        assert_eq!(item_text(7, 3, false, false), "007");
        assert_eq!(item_text(1, 1, true, false), "A");
        assert_eq!(item_text(27, 1, true, true), "aa");
        let mut pl = PartsList::new(1, [400.0, 280.0]);
        let t = table(&pl, &rows());
        assert_eq!(t.rows[1].cells, vec!["2", "3", "bolt", "bolt desc"]);
        assert_eq!(t.items[&3], ("2".to_owned(), "3".to_owned()));
        pl.sort = Some(SortBy {
            column: "part_number".into(),
            ascending: true,
            renumber: false,
        });
        let t = table(&pl, &rows());
        let order: Vec<&str> = t.rows.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(order, ["bolt", "nut", "plate"]);
        assert_eq!(t.rows[0].cells[0], "2");
        if let Some(s) = pl.sort.as_mut() {
            s.renumber = true;
        }
        let t = table(&pl, &rows());
        assert_eq!(t.rows[0].cells[0], "1");
        assert_eq!(t.items[&1].0, "3");
        pl.set_override("nut", "item", Some("10".into()));
        pl.set_override("plate", "description", Some("Base plate".into()));
        let t = table(&pl, &rows());
        assert_eq!(t.items[&5].0, "10");
        let plate = t.rows.iter().find(|r| r.key == "plate").unwrap();
        assert_eq!(plate.cells[3], "Base plate");
        assert_eq!(plate.overridden, vec![false, false, false, true]);
        pl.set_override("plate", "description", None);
        assert_eq!(pl.overrides.len(), 1);
    }

    /// Any iProperty or custom property as a column; hidden and custom rows.
    #[test]
    fn columns_hidden_and_custom_rows() {
        let mut pl = PartsList::new(1, [400.0, 280.0]);
        pl.columns.push(Column::new("vendor"));
        pl.columns.push(Column::new("custom:Grade"));
        assert_eq!(pl.columns[5].title, "GRADE");
        pl.hidden.push("nut".into());
        pl.custom_rows.push(CustomRow {
            cells: BTreeMap::from([("description".to_owned(), "Grease".to_owned())]),
        });
        let t = table(&pl, &rows());
        assert_eq!(t.rows[0].cells[4], "ACME");
        assert_eq!(t.rows[0].cells[5], "8.8");
        assert!(t.rows[2].hidden);
        assert!(t.rows[3].custom && t.rows[3].cells[3] == "Grease");
        let cols = available_columns(&rows());
        assert!(cols.iter().any(|(k, _)| k == "custom:Grade"));
        assert!(cols.iter().any(|(k, _)| k == "vendor"));
    }

    /// Inventor's ISO layout: 170 mm wide, title + heading + rows of 6.42
    /// mm from the upper right corner; wrapping puts sections side by side.
    #[test]
    fn layout_matches_inventor_iso() {
        let look = AnnotationLook::default();
        let mut pl = PartsList::new(1, [405.0, 287.0]);
        let t = table(&pl, &rows()[..2]);
        let (lo, hi) = bounds(&pl, &t, look.leader_text.height);
        assert!((pl.width() - 170.0).abs() < 1e-9);
        assert!((hi[0] - 405.0).abs() < 1e-9 && (hi[1] - 287.0).abs() < 1e-9);
        // 25.69 mm for title, heading and two rows.
        assert!((hi[1] - lo[1] - 25.693).abs() < 0.01, "{}", hi[1] - lo[1]);
        let prims = table_prims(&pl, &t, &look);
        let texts: Vec<&str> = prims
            .iter()
            .filter_map(|p| match p {
                Prim::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            texts[..6],
            [
                "PARTS LIST",
                "ITEM",
                "QTY",
                "PART NUMBER",
                "DESCRIPTION",
                "1"
            ]
        );
        pl.wrap.max_rows = Some(2);
        let t = table(&pl, &rows());
        let (lo, _) = bounds(&pl, &t, look.leader_text.height);
        assert!((lo[0] - (405.0 - 340.0)).abs() < 1e-9);
    }

    /// A balloon's leader ends on its rim and the arrow sits at the edge.
    #[test]
    fn balloon_geometry() {
        let look = AnnotationLook::default();
        let p = balloon_prims(
            BalloonShape::Circle,
            [0.0, 0.0],
            [20.0, 0.0],
            "2",
            "3",
            &look,
        );
        let Prim::Line { a, b, .. } = &p[0] else {
            panic!("leader first")
        };
        assert_eq!(*a, [0.0, 0.0]);
        assert!((b[0] - (20.0 - BALLOON_DIAMETER / 2.0)).abs() < 1e-9);
        assert!(
            p.iter()
                .any(|q| matches!(q, Prim::Text { text, .. } if text == "2"))
        );
        let two = balloon_prims(
            BalloonShape::CircleTwo,
            [0.0, 0.0],
            [0.0, 20.0],
            "2",
            "3",
            &look,
        );
        assert!(
            two.iter()
                .any(|q| matches!(q, Prim::Text { text, .. } if text == "3"))
        );
    }
}
