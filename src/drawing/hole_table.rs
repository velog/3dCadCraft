//! Hole tables (`docs/spec/4.7-drawing-commands.md`, Hole Table rows): the
//! holes of a view — all of them, picked ones, or those of one Hole feature
//! — with a tag per hole (a letter per kind of hole, a number per hole: A1,
//! A2, B1), its position from the view's origin indicator and its note.
//! Nothing of the holes is stored but their keys for a picked selection:
//! rows and tags follow the view's current holes (`ViewEdge::hole`).

use serde::{Deserialize, Serialize};

use super::annot::{Placement, line};
use super::hlr::ViewLines;
use super::parts::{Column, PartsList, Table, TableRow, item_text, table_prims_gap};
use super::sheet::{Anchor, Prim};
use super::style::{AnnotationLook, DimensionStyle};

/// Every column a hole table can show (key, heading), in Inventor's order.
pub const COLUMNS: [(&str, &str); 4] = [
    ("hole", "HOLE"),
    ("xdim", "XDIM"),
    ("ydim", "YDIM"),
    ("description", "DESCRIPTION"),
];

/// A column with Inventor's default heading and width (20, 15, 15, 50 mm).
pub fn column(key: &str) -> Column {
    let width = match key {
        "hole" => 20.0,
        "xdim" | "ydim" => 15.0,
        _ => 50.0,
    };
    Column {
        key: key.into(),
        title: COLUMNS
            .iter()
            .find(|(k, _)| *k == key)
            .map_or_else(|| key.to_uppercase(), |(_, t)| (*t).to_owned()),
        width,
    }
}

/// Inventor's default columns.
pub fn default_columns() -> Vec<Column> {
    COLUMNS.iter().map(|(k, _)| column(k)).collect()
}

/// Row gap of Inventor's Hole Table style (mm): rows 6.34 mm for 3.5 mm
/// text.
pub const ROW_GAP: f64 = 0.86;

/// Which holes a table lists.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum HoleScope {
    /// Every hole of the view (Hole Table - View).
    View,
    /// The picked holes (Hole Table - Selection), by key.
    Holes(Vec<String>),
    /// The holes of one Hole feature (Hole Table - Selected Feature).
    Feature(String),
}

fn default_title() -> String {
    "HOLE TABLE".into()
}

fn yes() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

/// A hole table on the sheet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HoleTable {
    /// The view.
    pub view: u32,
    /// The origin indicator (view plane, model mm).
    pub origin: [f64; 2],
    /// Upper left corner (sheet mm).
    pub at: [f64; 2],
    /// Title row text (empty: no title row).
    #[serde(default = "default_title")]
    pub title: String,
    /// Columns in order (keys from [`COLUMNS`]).
    #[serde(default = "default_columns")]
    pub columns: Vec<Column>,
    /// Which holes.
    pub scope: HoleScope,
    /// Tags beside the holes in the view.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub tags: bool,
}

/// One hole of a view.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewHole {
    /// `HoleInstance::key`.
    pub key: String,
    /// Centre (view plane).
    pub centre: [f64; 2],
    /// Radius of its smallest visible circle (view plane).
    pub radius: f64,
    /// Its note on one line.
    pub note: String,
}

/// The holes seen face-on in a view, one per hole, in feature order.
pub fn view_holes(lines: &ViewLines) -> Vec<ViewHole> {
    let mut out: Vec<ViewHole> = Vec::new();
    let mut at: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for e in lines.edges.iter().filter(|e| e.visible) {
        let (Some(key), Some((c, r))) = (&e.hole, e.circle) else {
            continue;
        };
        match at.get(key.as_str()) {
            Some(&k) => {
                let h = &mut out[k];
                if r < h.radius {
                    h.radius = r;
                    h.centre = c;
                }
            }
            None => {
                at.insert(key, out.len());
                out.push(ViewHole {
                    key: key.clone(),
                    centre: c,
                    radius: r,
                    note: e.note.clone().unwrap_or_default().replace('\n', " "),
                });
            }
        }
    }
    out.sort_by_cached_key(|h| natural(&h.key));
    out
}

/// A key compared with its numbers as numbers (`Hole2:1` before `Hole10:1`).
fn natural(s: &str) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    let mut text = String::new();
    let mut num = String::new();
    for ch in s.chars() {
        if ch.is_ascii_digit() {
            num.push(ch);
        } else {
            if !num.is_empty() {
                out.push((std::mem::take(&mut text), num.parse().unwrap_or(0)));
                num.clear();
            }
            text.push(ch);
        }
    }
    out.push((text, num.parse().unwrap_or(0)));
    out
}

/// A listed hole with its tag.
#[derive(Clone, Debug, PartialEq)]
pub struct TaggedHole {
    /// The hole.
    pub hole: ViewHole,
    /// `A1`, `B2`.
    pub tag: String,
    /// Distance from the origin along x (model mm).
    pub x: f64,
    /// Distance from the origin along y (model mm).
    pub y: f64,
}

impl HoleTable {
    /// A table of view `view` with Inventor's defaults.
    pub fn new(view: u32, origin: [f64; 2], at: [f64; 2], scope: HoleScope) -> Self {
        Self {
            view,
            origin,
            at,
            title: default_title(),
            columns: default_columns(),
            scope,
            tags: true,
        }
    }

    /// Total width (mm).
    pub fn width(&self) -> f64 {
        self.columns.iter().map(|c| c.width).sum()
    }

    /// The listed holes of `lines` with their tags: holes of one note are
    /// one kind (a letter, in feature order); within a kind they are
    /// numbered by position, Y first, then X (Inventor's Arrange by
    /// Position with Group Hole Types).
    pub fn tagged(&self, lines: &ViewLines) -> Vec<TaggedHole> {
        let holes: Vec<ViewHole> = view_holes(lines)
            .into_iter()
            .filter(|h| match &self.scope {
                HoleScope::View => true,
                HoleScope::Holes(keys) => keys.contains(&h.key),
                HoleScope::Feature(f) => feature_of(&h.key) == f.as_str(),
            })
            .collect();
        let mut kinds: Vec<String> = Vec::new();
        for h in &holes {
            if !kinds.contains(&h.note) {
                kinds.push(h.note.clone());
            }
        }
        let mut out = Vec::new();
        for (k, kind) in kinds.iter().enumerate() {
            let mut group: Vec<&ViewHole> = holes.iter().filter(|h| h.note == *kind).collect();
            group.sort_by(|a, b| {
                let ya = (a.centre[1] - b.centre[1]).abs() < 1e-6;
                if ya {
                    a.centre[0].total_cmp(&b.centre[0])
                } else {
                    a.centre[1].total_cmp(&b.centre[1])
                }
            });
            let letter = item_text(k + 1, 1, true, false);
            for (n, h) in group.into_iter().enumerate() {
                out.push(TaggedHole {
                    hole: h.clone(),
                    tag: format!("{letter}{}", n + 1),
                    x: h.centre[0] - self.origin[0],
                    y: h.centre[1] - self.origin[1],
                });
            }
        }
        out
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

/// The feature of a hole key (`Hole1:2#3` → `Hole1`; an assembly view's
/// `o3/Hole1:2` → `o3/Hole1`).
pub fn feature_of(key: &str) -> &str {
    key.rsplit_once(':').map_or(key, |(f, _)| f)
}

/// A position as the dimension style writes it, trailing zeros kept
/// (Inventor: `15,00`, `0,00`).
fn num(v: f64, d: &DimensionStyle) -> String {
    let mut t = format!("{:.*}", usize::from(d.precision), v);
    if t.starts_with('-') && t.trim_start_matches(['-', '0', '.']).is_empty() {
        t.remove(0);
    }
    if d.decimal_comma {
        t = t.replace('.', ",");
    }
    t
}

/// The table's rows over `lines`.
pub fn rows(ht: &HoleTable, lines: &ViewLines, d: &DimensionStyle) -> Table {
    let rows = ht
        .tagged(lines)
        .into_iter()
        .map(|t| {
            let cells = ht
                .columns
                .iter()
                .map(|c| match c.key.as_str() {
                    "hole" => t.tag.clone(),
                    "xdim" => num(t.x, d),
                    "ydim" => num(t.y, d),
                    "description" => t.hole.note.clone(),
                    _ => String::new(),
                })
                .collect::<Vec<_>>();
            TableRow {
                key: t.hole.key.clone(),
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

/// The table, its tags beside the holes and the origin indicator (`pl`
/// places the view). A column too narrow for its longest text (a
/// counterbore's description) widens to fit it.
pub fn prims(ht: &HoleTable, lines: &ViewLines, pl: Placement, look: &AnnotationLook) -> Vec<Prim> {
    let t = rows(ht, lines, &look.dim);
    let h = look.leader_text.height;
    let mut fit = ht.clone();
    for (k, c) in fit.columns.iter_mut().enumerate() {
        let need = t
            .rows
            .iter()
            .map(|r| r.cells.get(k).map_or(0, |s| s.chars().count()))
            .chain(std::iter::once(c.title.chars().count()))
            .max()
            .unwrap_or(0) as f64
            * h
            * 0.72
            + 1.5;
        c.width = c.width.max(need);
    }
    let mut out = table_prims_gap(&fit.grid(), &t, look, ROW_GAP);
    out.extend(origin_prims(pl.to_sheet(ht.origin)));
    if ht.tags {
        out.extend(tag_prims(ht, lines, pl, look));
    }
    out
}

/// The origin indicator: a small circle and two short axes along x and y.
pub fn origin_prims(o: [f64; 2]) -> Vec<Prim> {
    let r = 1.0;
    let len = 6.0;
    let mut out: Vec<Prim> = (0..12)
        .map(|k| {
            let (a, b) = (
                std::f64::consts::TAU * k as f64 / 12.0,
                std::f64::consts::TAU * (k + 1) as f64 / 12.0,
            );
            line(
                [o[0] + r * a.cos(), o[1] + r * a.sin()],
                [o[0] + r * b.cos(), o[1] + r * b.sin()],
            )
        })
        .collect();
    out.push(line([o[0] + r, o[1]], [o[0] + len, o[1]]));
    out.push(line([o[0], o[1] + r], [o[0], o[1] + len]));
    for (tip, d) in [
        ([o[0] + len, o[1]], [1.0, 0.0]),
        ([o[0], o[1] + len], [0.0, 1.0]),
    ] {
        let back = [tip[0] - d[0] * 1.5, tip[1] - d[1] * 1.5];
        let n = [-d[1] * 0.5, d[0] * 0.5];
        out.push(Prim::Fill {
            pts: vec![
                tip,
                [back[0] + n[0], back[1] + n[1]],
                [back[0] - n[0], back[1] - n[1]],
            ],
        });
    }
    out
}

/// The tags: each hole's tag at its rim, up and right at 45° (Inventor's
/// place for hole tags).
pub fn tag_prims(
    ht: &HoleTable,
    lines: &ViewLines,
    pl: Placement,
    look: &AnnotationLook,
) -> Vec<Prim> {
    let h = look.leader_text.height;
    let k = std::f64::consts::FRAC_1_SQRT_2;
    ht.tagged(lines)
        .into_iter()
        .map(|t| {
            let c = pl.to_sheet(t.hole.centre);
            let r = t.hole.radius * pl.scale;
            Prim::Text {
                at: [c[0] + r * k, c[1] + r * k],
                height: h,
                text: t.tag,
                anchor: Anchor::Left,
                angle: 0.0,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drawing::hlr::ViewEdge;

    fn circle(key: &str, c: [f64; 2], r: f64, note: &str) -> ViewEdge {
        ViewEdge {
            name: Some(format!("{key}:rim")),
            a: [c[0] + r, c[1]],
            b: [c[0] + r, c[1]],
            circle: Some((c, r)),
            full_circle: true,
            straight: false,
            visible: true,
            note: Some(note.into()),
            hole: Some(key.into()),
        }
    }

    fn lines() -> ViewLines {
        ViewLines {
            edges: vec![
                // Inventor's case: two Ø6.6 THRU, a Ø10 blind, an M8.
                circle("Hole1:2", [85.0, 15.0], 3.3, "Ø6.6 THRU"),
                circle("Hole1:1", [15.0, 15.0], 3.3, "Ø6.6 THRU"),
                circle("Hole2:1", [50.0, 30.0], 5.0, "Ø10 ↧6"),
                circle("Hole10:1", [30.0, 30.0], 4.0, "M8x1.25 - 6H"),
                // A counterbore's two circles count once (the bore's place).
                circle("Hole3:1", [15.0, 45.0], 5.5, "Ø6.6 THRU\n⌴ Ø11 ↧6.4"),
                circle("Hole3:1", [15.0, 45.0], 3.3, "Ø6.6 THRU\n⌴ Ø11 ↧6.4"),
            ],
            min: [0.0, 0.0],
            max: [100.0, 60.0],
            ..Default::default()
        }
    }

    #[test]
    fn tags_by_kind_and_position() {
        let ht = HoleTable::new(1, [0.0, 0.0], [300.0, 200.0], HoleScope::View);
        let t = ht.tagged(&lines());
        let tags: Vec<(&str, &str)> = t
            .iter()
            .map(|t| (t.tag.as_str(), t.hole.key.as_str()))
            .collect();
        assert_eq!(
            tags,
            [
                ("A1", "Hole1:1"),
                ("A2", "Hole1:2"),
                ("B1", "Hole2:1"),
                ("C1", "Hole3:1"),
                ("D1", "Hole10:1"),
            ]
        );
        let cb = t.iter().find(|t| t.tag == "C1").unwrap();
        assert!((cb.hole.radius - 3.3).abs() < 1e-12);
        assert_eq!(cb.hole.note, "Ø6.6 THRU ⌴ Ø11 ↧6.4");
        // Selection and feature scopes.
        let sel = HoleTable {
            scope: HoleScope::Holes(vec!["Hole1:2".into()]),
            ..ht.clone()
        };
        assert_eq!(sel.tagged(&lines()).len(), 1);
        let feat = HoleTable {
            scope: HoleScope::Feature("Hole1".into()),
            ..ht.clone()
        };
        assert_eq!(feat.tagged(&lines()).len(), 2);
        assert_eq!(feature_of("o3/Hole1:2#4"), "o3/Hole1");
    }

    #[test]
    fn rows_measure_from_the_origin() {
        let ht = HoleTable::new(1, [0.0, 0.0], [300.0, 200.0], HoleScope::View);
        let d = DimensionStyle {
            precision: 2,
            decimal_comma: true,
            ..Default::default()
        };
        let t = rows(&ht, &lines(), &d);
        assert_eq!(t.rows[0].cells, ["A1", "15,00", "15,00", "Ø6.6 THRU"]);
        assert_eq!(t.rows[1].cells[1], "85,00");
        let moved = HoleTable {
            origin: [15.0, 15.0],
            ..ht
        };
        assert_eq!(
            rows(&moved, &lines(), &d).rows[0].cells[1..3],
            ["0,00", "0,00"]
        );
    }

    #[test]
    fn layout_and_tags_match_inventor() {
        // 4 holes: title, headings, 4 rows of 6.343 mm (Inventor: 5 rows
        // 44.40 mm); 100 mm wide; the upper left corner at `at`; tags at the
        // rim, 45°.
        let ht = HoleTable::new(
            1,
            [0.0, 0.0],
            [270.0, 260.0],
            HoleScope::Holes(vec![
                "Hole1:1".into(),
                "Hole1:2".into(),
                "Hole2:1".into(),
                "Hole10:1".into(),
            ]),
        );
        let pl = Placement {
            breaks: Default::default(),
            position: [150.0, 150.0],
            scale: 1.0,
            center: [50.0, 30.0],
        };
        let look = AnnotationLook::default();
        let mut no_tags = ht.clone();
        no_tags.tags = false;
        let p = prims(&no_tags, &lines(), pl, &look);
        let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
        for q in &p {
            if let Prim::Line { a, b, .. } = q {
                // The origin indicator is elsewhere on the sheet.
                if a[0] < 260.0 {
                    continue;
                }
                for v in [a, b] {
                    for k in 0..2 {
                        lo[k] = lo[k].min(v[k]);
                        hi[k] = hi[k].max(v[k]);
                    }
                }
            }
        }
        assert!((hi[0] - lo[0] - 100.0).abs() < 1e-6, "{lo:?} {hi:?}");
        assert!(
            (hi[1] - lo[1] - 6.0 * 44.40 / 7.0).abs() < 0.02,
            "{lo:?} {hi:?}"
        );
        // A long description widens its column.
        let mut all = ht.clone();
        all.scope = HoleScope::View;
        all.tags = false;
        let wide = prims(&all, &lines(), pl, &look);
        let right = wide
            .iter()
            .filter_map(|q| match q {
                Prim::Line { a, b, .. } if a[0] >= 260.0 => Some(a[0].max(b[0])),
                _ => None,
            })
            .fold(f64::MIN, f64::max);
        assert!(right > 370.0 + 1e-6, "{right}");
        assert!((lo[0] - 270.0).abs() < 1e-6 && (hi[1] - 260.0).abs() < 1e-6);
        let tags = tag_prims(&ht, &lines(), pl, &look);
        let Prim::Text { at, text, .. } = &tags[0] else {
            panic!("a text")
        };
        assert_eq!(text, "A1");
        // Hole A1 at (15, 15) → sheet (115, 135); rim 3.3 at 45°.
        let k = 3.3 * std::f64::consts::FRAC_1_SQRT_2;
        assert!((at[0] - (115.0 + k)).abs() < 1e-9 && (at[1] - (135.0 + k)).abs() < 1e-9);
    }

    #[test]
    fn saves_and_loads() {
        let ht = HoleTable::new(
            2,
            [1.0, 2.0],
            [3.0, 4.0],
            HoleScope::Feature("Hole1".into()),
        );
        let back: HoleTable = serde_json::from_str(&serde_json::to_string(&ht).unwrap()).unwrap();
        assert_eq!(back, ht);
    }
}
