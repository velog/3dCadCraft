//! Revision tables, revision tags and revision clouds
//! (`docs/spec/4.7-drawing-commands.md`, Revision rows). Inventor's Revision
//! Table (ISO) style: title `REVISION HISTORY`, columns ZONE / REV /
//! DESCRIPTION / DATE / APPROVED of 30 mm, rows 6.40 mm, numeric index from
//! 1 with Auto-Index on, the table placed by its upper left corner and one
//! revision row at once. Alpha indexing skips I, O, Q, S, X and Z. The tag of
//! the ISO style is a triangle round the revision (the active row's when it
//! is placed).

use serde::{Deserialize, Serialize};

use super::annot::line;
use super::parts::Column;
use super::sheet::{Anchor, Prim};
use super::style::AnnotationLook;

/// Row gap of Inventor's Revision Table style (mm).
pub const ROW_GAP: f64 = 0.889;

/// Letters an alpha index skips (Inventor's default exclusions).
pub const SKIPPED: [char; 6] = ['I', 'O', 'Q', 'S', 'X', 'Z'];

/// Every column (key, heading), in Inventor's order.
pub const COLUMNS: [(&str, &str); 5] = [
    ("zone", "ZONE"),
    ("rev", "REV"),
    ("description", "DESCRIPTION"),
    ("date", "DATE"),
    ("approved", "APPROVED"),
];

/// Where the revisions count.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RevScope {
    /// One table for the whole drawing (writes the drawing's Revision
    /// Number iProperty).
    #[default]
    Drawing,
    /// A table of the active sheet (writes the sheet's revision).
    Sheet,
}

fn yes() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

fn default_title() -> String {
    "REVISION HISTORY".into()
}

fn default_start() -> String {
    "1".into()
}

/// A column of a revision table.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RevColumn {
    /// `zone`, `rev`, `description`, `date`, `approved`.
    pub key: String,
    /// Heading.
    pub title: String,
    /// Width on paper (mm).
    pub width: f64,
}

/// Inventor's default columns.
pub fn default_columns() -> Vec<RevColumn> {
    COLUMNS
        .iter()
        .map(|(k, t)| RevColumn {
            key: (*k).into(),
            title: (*t).into(),
            width: 30.0,
        })
        .collect()
}

/// One row of a revision table.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RevRow {
    /// Zone of the change on the sheet.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub zone: String,
    /// The revision (`1`, `B`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub rev: String,
    /// What changed.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// When.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub date: String,
    /// Who approved it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub approved: String,
    /// A row inserted by Insert Row: not a revision (no index, never the
    /// active row).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub inserted: bool,
}

impl RevRow {
    /// The cell of column `key`.
    pub fn cell(&self, key: &str) -> &str {
        match key {
            "zone" => &self.zone,
            "rev" => &self.rev,
            "description" => &self.description,
            "date" => &self.date,
            "approved" => &self.approved,
            _ => "",
        }
    }

    /// The cell of column `key`, to edit.
    pub fn cell_mut(&mut self, key: &str) -> Option<&mut String> {
        Some(match key {
            "zone" => &mut self.zone,
            "rev" => &mut self.rev,
            "description" => &mut self.description,
            "date" => &mut self.date,
            "approved" => &mut self.approved,
            _ => return None,
        })
    }
}

/// A revision table on the sheet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RevisionTable {
    /// Upper left corner (sheet mm).
    pub at: [f64; 2],
    /// Entire drawing or the active sheet.
    #[serde(default)]
    pub scope: RevScope,
    /// Numeric index (else alpha).
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub numeric: bool,
    /// The first revision.
    #[serde(default = "default_start")]
    pub start: String,
    /// New revision rows get the next index (else an empty REV).
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub auto_index: bool,
    /// Editing the active row's REV writes the Revision Number property.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub update_property: bool,
    /// Delete Rule Auto: keep only the last `n` revision rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep: Option<u32>,
    /// Title row text (empty: no title row).
    #[serde(default = "default_title")]
    pub title: String,
    /// Columns in order.
    #[serde(default = "default_columns")]
    pub columns: Vec<RevColumn>,
    /// Rows, oldest first.
    #[serde(default)]
    pub rows: Vec<RevRow>,
}

/// The revision table settings a new table starts with.
impl Default for RevisionTable {
    fn default() -> Self {
        Self {
            at: [0.0, 0.0],
            scope: RevScope::Drawing,
            numeric: true,
            start: default_start(),
            auto_index: true,
            update_property: true,
            keep: None,
            title: default_title(),
            columns: default_columns(),
            rows: Vec::new(),
        }
    }
}

/// The index after `rev`: a number plus one, or the next letter without
/// the skipped ones (`Y` → `AA`, `H` → `J`). `None` when it is neither.
pub fn next_index(rev: &str, numeric: bool) -> Option<String> {
    let rev = rev.trim();
    if numeric {
        return rev.parse::<u64>().ok().map(|n| (n + 1).to_string());
    }
    if rev.is_empty() || !rev.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let lower = rev.chars().all(|c| c.is_ascii_lowercase());
    let letters: Vec<char> = ('A'..='Z').filter(|c| !SKIPPED.contains(c)).collect();
    let mut digits: Vec<usize> = Vec::new();
    for c in rev.to_ascii_uppercase().chars() {
        digits.push(letters.iter().position(|&l| l == c)?);
    }
    let mut k = digits.len();
    loop {
        if k == 0 {
            digits.insert(0, 0);
            break;
        }
        k -= 1;
        if digits[k] + 1 < letters.len() {
            digits[k] += 1;
            break;
        }
        digits[k] = 0;
    }
    let s: String = digits.iter().map(|&d| letters[d]).collect();
    Some(if lower { s.to_lowercase() } else { s })
}

impl RevisionTable {
    /// A table at `at` with these settings and its first revision row.
    pub fn place(mut self, at: [f64; 2], date: &str) -> Self {
        self.at = at;
        if self.rows.is_empty() {
            self.add_revision(date);
        }
        self
    }

    /// The active row: the last revision row.
    pub fn active(&self) -> Option<usize> {
        self.rows.iter().rposition(|r| !r.inserted)
    }

    /// The active revision ("" without revision rows).
    pub fn active_rev(&self) -> &str {
        self.active().map_or("", |k| self.rows[k].rev.as_str())
    }

    /// The REV of the next revision row.
    pub fn next_rev(&self) -> String {
        if !self.auto_index {
            return String::new();
        }
        match self.active() {
            None => self.start.clone(),
            Some(k) => {
                next_index(&self.rows[k].rev, self.numeric).unwrap_or_else(|| self.start.clone())
            }
        }
    }

    /// Add Revision Row: a row with the next index and `date`, at the end;
    /// the Delete Rule then drops the oldest revision rows.
    pub fn add_revision(&mut self, date: &str) {
        let rev = self.next_rev();
        self.rows.push(RevRow {
            rev,
            date: date.to_owned(),
            ..Default::default()
        });
        self.apply_delete_rule();
    }

    /// Delete Rule Auto: keeps the last `keep` revision rows (inserted rows
    /// stay).
    pub fn apply_delete_rule(&mut self) {
        let Some(n) = self.keep.map(|n| n.max(1) as usize) else {
            return;
        };
        let mut revs = self.rows.iter().filter(|r| !r.inserted).count();
        self.rows.retain(|r| {
            if r.inserted || revs <= n {
                true
            } else {
                revs -= 1;
                false
            }
        });
    }

    /// Total width (mm).
    pub fn width(&self) -> f64 {
        self.columns.iter().map(|c| c.width).sum()
    }
}

/// The table's lines and texts.
pub fn prims(t: &RevisionTable, look: &AnnotationLook) -> Vec<Prim> {
    let columns: Vec<Column> = t
        .columns
        .iter()
        .map(|c| Column {
            key: format!("rev:{}", c.key),
            title: c.title.clone(),
            width: c.width,
        })
        .collect();
    let rows: Vec<Vec<String>> = t
        .rows
        .iter()
        .map(|r| {
            t.columns
                .iter()
                .map(|c| r.cell(&c.key).to_owned())
                .collect()
        })
        .collect();
    super::parts::table_prims_gap(
        &super::table::grid(t.at, &t.title, &columns),
        &super::table::text_table(&rows, columns.len()),
        look,
        ROW_GAP,
    )
}

/// Revision tag shapes (Inventor's list; ISO uses the triangle).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TagShape {
    /// Circular.
    Circle,
    /// Hexagon.
    Hexagon,
    /// Square.
    Square,
    /// Triangular.
    #[default]
    Triangle,
}

impl TagShape {
    /// Every shape, in Inventor's order.
    pub const ALL: [TagShape; 4] = [
        TagShape::Circle,
        TagShape::Hexagon,
        TagShape::Square,
        TagShape::Triangle,
    ];

    /// Display name.
    pub fn label(self) -> &'static str {
        match self {
            TagShape::Circle => "Circular",
            TagShape::Hexagon => "Hexagon",
            TagShape::Square => "Square",
            TagShape::Triangle => "Triangular",
        }
    }
}

fn is_default_shape(s: &TagShape) -> bool {
    *s == TagShape::default()
}

/// A revision tag: the revision in a shape, on the sheet or on a view
/// (then it moves with the view), with an optional leader.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RevisionTag {
    /// The view it belongs to (`at` and `tip` then relative to the view's
    /// position).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<u32>,
    /// The tag's centre (sheet mm, or relative to the view).
    pub at: [f64; 2],
    /// The leader's arrow on an edge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tip: Option<[f64; 2]>,
    /// The revision it shows.
    pub rev: String,
    /// Its shape.
    #[serde(default, skip_serializing_if = "is_default_shape")]
    pub shape: TagShape,
}

/// The tag's outline round its text (sheet mm): a polygon of corners.
fn tag_outline(c: [f64; 2], text: &str, shape: TagShape, h: f64) -> Vec<[f64; 2]> {
    let w = super::annot::text_width(text, h).max(h * 0.6);
    let poly = |n: usize, r: f64, start: f64| -> Vec<[f64; 2]> {
        (0..n)
            .map(|k| {
                let a = start + std::f64::consts::TAU * k as f64 / n as f64;
                [c[0] + r * a.cos(), c[1] + r * a.sin()]
            })
            .collect()
    };
    match shape {
        TagShape::Circle => poly(36, (w * 0.5).max(h * 0.5) + h * 0.5, 0.0),
        TagShape::Hexagon => poly(6, (w * 0.5).max(h * 0.5) + h * 0.6, 0.0),
        TagShape::Square => {
            let s = (w * 0.5).max(h * 0.5) + h * 0.4;
            vec![
                [c[0] - s, c[1] - s],
                [c[0] + s, c[1] - s],
                [c[0] + s, c[1] + s],
                [c[0] - s, c[1] + s],
            ]
        }
        TagShape::Triangle => {
            // Equilateral, the text in its lower part: the incircle holds
            // the text, the centre is the incircle's.
            let r = (w * 0.5).max(h * 0.5) + h * 0.35;
            poly(3, 2.0 * r, std::f64::consts::FRAC_PI_2)
        }
    }
}

/// The tag's lines and text; `origin` is the view's position (or zero).
pub fn tag_prims(t: &RevisionTag, origin: [f64; 2], look: &AnnotationLook) -> Vec<Prim> {
    let h = look.leader_text.height;
    let c = [origin[0] + t.at[0], origin[1] + t.at[1]];
    let outline = tag_outline(c, &t.rev, t.shape, h);
    let mut out: Vec<Prim> = (0..outline.len())
        .map(|k| line(outline[k], outline[(k + 1) % outline.len()]))
        .collect();
    out.push(Prim::Text {
        at: [c[0], c[1] - h * 0.5],
        height: h,
        text: t.rev.clone(),
        anchor: Anchor::Center,
        angle: 0.0,
    });
    if let Some(tip) = t.tip {
        let tip = [origin[0] + tip[0], origin[1] + tip[1]];
        // The leader starts where it leaves the outline.
        let start = edge_point(&outline, c, tip);
        out.push(line(start, tip));
        let d = [tip[0] - start[0], tip[1] - start[1]];
        let len = d[0].hypot(d[1]);
        if len > 1e-9 {
            let l = &look.leader;
            super::annot::terminator(
                &mut out,
                tip,
                [d[0] / len, d[1] / len],
                l.terminator,
                l.arrow_size,
                l.arrow_height,
            );
        }
    }
    out
}

/// Where the ray from `c` towards `p` leaves the polygon.
fn edge_point(poly: &[[f64; 2]], c: [f64; 2], p: [f64; 2]) -> [f64; 2] {
    let d = [p[0] - c[0], p[1] - c[1]];
    let mut best = 1.0_f64;
    for k in 0..poly.len() {
        let (a, b) = (poly[k], poly[(k + 1) % poly.len()]);
        let e = [b[0] - a[0], b[1] - a[1]];
        let den = d[0] * e[1] - d[1] * e[0];
        if den.abs() < 1e-12 {
            continue;
        }
        let w = [a[0] - c[0], a[1] - c[1]];
        let t = (w[0] * e[1] - w[1] * e[0]) / den;
        let s = (w[0] * d[1] - w[1] * d[0]) / den;
        if t > 0.0 && (0.0..=1.0).contains(&s) {
            best = best.min(t);
        }
    }
    [c[0] + d[0] * best, c[1] + d[1] * best]
}

fn default_arc() -> f64 {
    8.0
}

/// A revision cloud round a change: arcs bulging outwards along a closed
/// outline.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RevisionCloud {
    /// The view it belongs to (`pts` then relative to the view's position).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<u32>,
    /// Corners of the outline (sheet mm, or relative to the view).
    pub pts: Vec<[f64; 2]>,
    /// Arc length (chord of one arc, mm).
    #[serde(default = "default_arc")]
    pub arc: f64,
}

/// The cloud's arcs; `origin` is the view's position (or zero).
pub fn cloud_prims(c: &RevisionCloud, origin: [f64; 2], width: f64) -> Vec<Prim> {
    let pts: Vec<[f64; 2]> = c
        .pts
        .iter()
        .map(|p| [origin[0] + p[0], origin[1] + p[1]])
        .collect();
    if pts.len() < 2 {
        return Vec::new();
    }
    // Arcs bulge to the outside: away from the enclosed area.
    let area: f64 = (0..pts.len())
        .map(|k| {
            let (a, b) = (pts[k], pts[(k + 1) % pts.len()]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum();
    let out_sign = if area >= 0.0 { 1.0 } else { -1.0 };
    let closed = pts.len() > 2;
    let n_edges = if closed { pts.len() } else { 1 };
    let mut path = Vec::new();
    for k in 0..n_edges {
        let (a, b) = (pts[k], pts[(k + 1) % pts.len()]);
        let d = [b[0] - a[0], b[1] - a[1]];
        let len = d[0].hypot(d[1]);
        if len < 1e-9 {
            continue;
        }
        let n = (len / c.arc.max(0.5)).ceil().max(1.0) as usize;
        // Right-hand normal of a counter-clockwise outline points out.
        let nrm = [d[1] / len * out_sign, -d[0] / len * out_sign];
        for i in 0..n {
            let p0 = [
                a[0] + d[0] * i as f64 / n as f64,
                a[1] + d[1] * i as f64 / n as f64,
            ];
            let chord = len / n as f64;
            // An arc of 140° over the chord.
            let half = 70f64.to_radians();
            let r = chord * 0.5 / half.sin();
            let mid = [
                p0[0] + d[0] / len * chord * 0.5,
                p0[1] + d[1] / len * chord * 0.5,
            ];
            let centre = [
                mid[0] - nrm[0] * r * half.cos(),
                mid[1] - nrm[1] * r * half.cos(),
            ];
            let a0 = (p0[1] - centre[1]).atan2(p0[0] - centre[0]);
            let steps = 10;
            for s in 0..=steps {
                if s == 0 && !path.is_empty() {
                    continue;
                }
                // From p0 over the outside to the next point.
                let t = a0 + out_sign * 2.0 * half * s as f64 / steps as f64;
                path.push([centre[0] + r * t.cos(), centre[1] + r * t.sin()]);
            }
        }
    }
    vec![Prim::Path {
        pts: path,
        width,
        dashed: false,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indices_count_and_skip_letters() {
        assert_eq!(next_index("1", true).as_deref(), Some("2"));
        assert_eq!(next_index("9", true).as_deref(), Some("10"));
        assert_eq!(next_index("A", false).as_deref(), Some("B"));
        assert_eq!(next_index("H", false).as_deref(), Some("J"));
        assert_eq!(next_index("R", false).as_deref(), Some("T"));
        assert_eq!(next_index("Y", false).as_deref(), Some("AA"));
        assert_eq!(next_index("AY", false).as_deref(), Some("BA"));
        assert_eq!(next_index("b", false).as_deref(), Some("c"));
        assert_eq!(next_index("I", false), None);
        assert_eq!(next_index("x1", true), None);
    }

    #[test]
    fn a_placed_table_has_one_revision_and_rows_follow() {
        let t = RevisionTable::default().place([400.0, 280.0], "2026-10-28");
        assert_eq!(t.rows.len(), 1);
        assert_eq!(t.active_rev(), "1");
        assert!((t.width() - 150.0).abs() < 1e-9);
        let mut t = t;
        t.add_revision("d");
        t.rows.push(RevRow {
            description: "note".into(),
            inserted: true,
            ..Default::default()
        });
        t.add_revision("d");
        assert_eq!(t.active_rev(), "3");
        assert_eq!(t.active(), Some(3));
        let alpha = RevisionTable {
            numeric: false,
            start: "A".into(),
            ..Default::default()
        }
        .place([0.0, 0.0], "");
        assert_eq!(alpha.next_rev(), "B");
        let manual = RevisionTable {
            auto_index: false,
            ..Default::default()
        }
        .place([0.0, 0.0], "");
        assert_eq!(manual.active_rev(), "");
    }

    #[test]
    fn the_delete_rule_keeps_the_last_rows() {
        let mut t = RevisionTable {
            keep: Some(2),
            ..Default::default()
        };
        for _ in 0..5 {
            t.add_revision("");
        }
        let revs: Vec<&str> = t.rows.iter().map(|r| r.rev.as_str()).collect();
        assert_eq!(revs, ["4", "5"]);
    }

    #[test]
    fn the_table_matches_inventors_size() {
        let t = RevisionTable::default().place([400.0, 280.0], "x");
        let look = AnnotationLook::default();
        let p = prims(&t, &look);
        let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
        for q in &p {
            if let Prim::Line { a, b, .. } = q {
                for v in [a, b] {
                    lo = [lo[0].min(v[0]), lo[1].min(v[1])];
                    hi = [hi[0].max(v[0]), hi[1].max(v[1])];
                }
            }
        }
        // Inventor: 150.2 × 19.21 mm from (400, 280) down and right.
        assert!((lo[0] - 400.0).abs() < 1e-9 && (hi[1] - 280.0).abs() < 1e-9);
        assert!((hi[0] - lo[0] - 150.0).abs() < 0.3);
        assert!((hi[1] - lo[1] - 19.21).abs() < 0.05, "{}", hi[1] - lo[1]);
        let texts: Vec<&str> = p
            .iter()
            .filter_map(|q| match q {
                Prim::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            texts,
            [
                "REVISION HISTORY",
                "ZONE",
                "REV",
                "DESCRIPTION",
                "DATE",
                "APPROVED",
                "1",
                "x"
            ]
        );
    }

    #[test]
    fn a_tag_with_a_leader_ends_on_the_tip() {
        let look = AnnotationLook::default();
        let tag = RevisionTag {
            view: Some(1),
            at: [10.0, 10.0],
            tip: Some([30.0, 10.0]),
            rev: "2".into(),
            shape: TagShape::Triangle,
        };
        let p = tag_prims(&tag, [100.0, 100.0], &look);
        assert!(p.iter().any(|q| matches!(q, Prim::Text { text, at, .. }
            if text == "2" && (at[0] - 110.0).abs() < 1e-9)));
        assert!(p.iter().any(|q| matches!(q, Prim::Line { b, .. }
            if (b[0] - 130.0).abs() < 1e-9 && (b[1] - 110.0).abs() < 1e-9)));
        // The arrow at the tip.
        assert!(
            p.iter()
                .any(|q| matches!(q, Prim::Fill { pts } if pts[0] == [130.0, 110.0])),
            "{p:?}"
        );
        for s in TagShape::ALL {
            let o = tag_outline([0.0, 0.0], "12", s, 3.5);
            // The text fits inside: its corners are within the outline's box.
            let xmax = o.iter().map(|p| p[0]).fold(f64::MIN, f64::max);
            assert!(
                xmax > super::super::annot::text_width("12", 3.5) * 0.5,
                "{s:?}"
            );
        }
    }

    #[test]
    fn a_cloud_bulges_outwards_on_every_edge() {
        let square = RevisionCloud {
            view: None,
            pts: vec![[0.0, 0.0], [40.0, 0.0], [40.0, 40.0], [0.0, 40.0]],
            arc: 8.0,
        };
        for pts in [
            square.pts.clone(),
            square.pts.iter().rev().copied().collect(),
        ] {
            let c = RevisionCloud {
                pts,
                ..square.clone()
            };
            let p = cloud_prims(&c, [0.0, 0.0], 0.35);
            let Some(Prim::Path { pts, .. }) = p.first() else {
                panic!("no path");
            };
            // 4 edges × 5 arcs × 10 steps + 1.
            assert_eq!(pts.len(), 201);
            // Nothing inside the square beyond the corners' rounding.
            assert!(
                pts.iter()
                    .all(|q| !(q[0] > 0.5 && q[0] < 39.5 && q[1] > 0.5 && q[1] < 39.5)),
                "an arc bulges inwards"
            );
            assert!(pts.iter().any(|q| q[1] < -1.0));
        }
    }
}
