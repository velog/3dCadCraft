//! **Part Family** (`docs/spec/5.7-part-families.md`): one part with a table,
//! each row a member (a size or a variant). GUI-free.
//!
//! A column holds numbers with a unit or texts; its cells are expressions over
//! the row's other columns (units checked by [`super::expr`]) and it may have
//! one formula for all rows. A column drives its [`Target`]: a parameter, an
//! Property, a feature's suppression, the material or the file name. A member
//! is the part with its row applied and regenerated; features are named by id
//! and parameters by name, never by index (rules 2, 3).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::expr::{self, Unit, Value};
use super::{PartDocument, PartError};
use crate::document::iprops::IProperties;

/// What a column drives.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "name")]
pub enum Target {
    /// Nothing: a value for other columns or for the eye.
    #[default]
    None,
    /// A model or user parameter, by name.
    Parameter(String),
    /// A standard property (`document::iprops::STANDARD`), by key.
    Property(String),
    /// A feature by id: a true cell suppresses it.
    Suppress(String),
    /// The material's name.
    Material,
    /// The member's file name (without extension).
    FileName,
}

/// One column.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Column {
    /// Name other columns use in their expressions.
    pub name: String,
    /// Heading shown (`Nominal Length`); empty = the name.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub caption: String,
    /// Unit; `Unit::Text` = texts.
    pub unit: Unit,
    /// Position among the keys that pick a member (from 1); 0 = no key.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub key: u32,
    /// What it drives.
    #[serde(default, skip_serializing_if = "is_none")]
    pub target: Target,
    /// One expression for every row; the cells are then not used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formula: Option<String>,
}

fn is_zero(v: &u32) -> bool {
    *v == 0
}

fn is_none(t: &Target) -> bool {
    *t == Target::None
}

impl Column {
    /// A column of `unit` driving `target`.
    pub fn new(name: &str, unit: Unit, target: Target) -> Self {
        Self {
            name: name.to_owned(),
            caption: String::new(),
            unit,
            key: 0,
            target,
            formula: None,
        }
    }

    /// The heading.
    pub fn heading(&self) -> &str {
        if self.caption.is_empty() {
            &self.name
        } else {
            &self.caption
        }
    }

    fn is_text(&self) -> bool {
        self.unit == Unit::Text
    }
}

/// The table of a part family.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FamilyTable {
    /// Stable identity: a member's file identity is derived from it.
    pub id: String,
    /// The columns.
    pub columns: Vec<Column>,
    /// One text per column, per member.
    pub rows: Vec<Vec<String>>,
    /// The member placed when nothing is chosen.
    #[serde(default)]
    pub default_row: usize,
}

/// A problem [`FamilyTable::verify`] found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    /// The row, when it concerns one.
    pub row: Option<usize>,
    /// The column, when it concerns one.
    pub column: Option<usize>,
    /// What is wrong.
    pub message: String,
}

/// A member made by [`FamilyTable::member`].
#[derive(Clone, Debug)]
pub struct Member {
    /// The part with the row applied; it carries no table.
    pub part: PartDocument,
    /// The template's properties with the row's on top.
    pub properties: IProperties,
    /// File name without extension.
    pub file_name: String,
}

/// Text of a number in a designation: at most 6 decimals, no trailing zeros.
fn plain(v: f64) -> String {
    let s = format!("{v:.6}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

impl FamilyTable {
    /// The columns that pick a member, in key order.
    pub fn keys(&self) -> Vec<usize> {
        let mut k: Vec<usize> = (0..self.columns.len())
            .filter(|&i| self.columns[i].key > 0)
            .collect();
        k.sort_by_key(|&i| self.columns[i].key);
        k
    }

    /// Every column's value in `row`. Columns may refer to each other in any
    /// order; a circular reference or an unknown name is an error.
    pub fn values(&self, row: usize) -> Result<BTreeMap<String, Value>, String> {
        let cells = self
            .rows
            .get(row)
            .ok_or_else(|| format!("no member {}", row + 1))?;
        let mut done: BTreeMap<String, Value> = BTreeMap::new();
        let mut left: Vec<usize> = (0..self.columns.len()).collect();
        while !left.is_empty() {
            let before = left.len();
            let mut last_error = String::new();
            let mut waiting = Vec::new();
            for &i in &left {
                let c = &self.columns[i];
                let cell = cells.get(i).map_or("", String::as_str);
                match self.cell_value(c, cell, &done) {
                    Ok(Some(v)) => {
                        done.insert(c.name.clone(), v);
                    }
                    Ok(None) => waiting.push(i),
                    Err(e) => last_error = format!("{}: {e}", c.heading()),
                }
                if !last_error.is_empty() {
                    return Err(last_error);
                }
            }
            left = waiting;
            if left.len() == before {
                let names: Vec<&str> = left.iter().map(|&i| self.columns[i].heading()).collect();
                return Err(format!(
                    "{}: a circular reference or an unknown name",
                    names.join(", ")
                ));
            }
        }
        Ok(done)
    }

    /// `None`: it refers to a column not evaluated yet.
    fn cell_value(
        &self,
        c: &Column,
        cell: &str,
        done: &BTreeMap<String, Value>,
    ) -> Result<Option<Value>, String> {
        let text = match &c.formula {
            Some(f) => f.as_str(),
            None if c.is_text() => match cell.strip_prefix('=') {
                Some(e) => e,
                None => return Ok(Some(Value::Text(cell.to_owned()))),
            },
            None => cell,
        };
        let is_column = |n: &str| self.columns.iter().any(|o| o.name == n);
        if expr::names_in(text)
            .iter()
            .any(|n| is_column(n) && !done.contains_key(n))
        {
            return Ok(None);
        }
        let vars = |n: &str| done.get(n).cloned();
        expr::eval_value(text, c.unit, &vars)
            .map(|(v, _)| Some(v))
            .map_err(|e| {
                if text.trim().is_empty() {
                    "the cell is empty".to_owned()
                } else {
                    e
                }
            })
    }

    /// The text of a value in its column's unit (`30`, `M8`).
    fn shown(c: &Column, v: &Value) -> String {
        match v {
            Value::Text(t) => t.clone(),
            Value::Bool(b) => b.to_string(),
            Value::Num(x, _) => plain(c.unit.to_display(*x)),
        }
    }

    /// What `row` shows in each key column.
    pub fn key_texts(&self, row: usize) -> Result<Vec<String>, String> {
        let vals = self.values(row)?;
        Ok(self
            .keys()
            .into_iter()
            .map(|i| {
                let c = &self.columns[i];
                vals.get(&c.name)
                    .map(|v| Self::shown(c, v))
                    .unwrap_or_default()
            })
            .collect())
    }

    /// The member's name: its file name column, else its keys.
    pub fn member_name(&self, row: usize) -> Result<String, String> {
        let vals = self.values(row)?;
        if let Some(c) = self.columns.iter().find(|c| c.target == Target::FileName)
            && let Some(v) = vals.get(&c.name)
        {
            return Ok(Self::shown(c, v));
        }
        Ok(self.key_texts(row)?.join(" x "))
    }

    /// The values the next key column offers once `picked` has chosen the
    /// keys before it, in table order without repeats.
    pub fn choices(&self, picked: &[String]) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for r in 0..self.rows.len() {
            let Ok(k) = self.key_texts(r) else { continue };
            if k.len() > picked.len()
                && k[..picked.len()] == *picked
                && !out.contains(&k[picked.len()])
            {
                out.push(k[picked.len()].clone());
            }
        }
        out
    }

    /// The row whose keys are `picked`.
    pub fn find(&self, picked: &[String]) -> Option<usize> {
        (0..self.rows.len()).find(|&r| self.key_texts(r).is_ok_and(|k| k == picked))
    }

    /// Everything wrong with the table for `part`: bad columns, targets the
    /// part does not have, rows that cannot be evaluated, members that
    /// cannot be told apart. Nothing is built.
    pub fn verify(&self, part: &PartDocument) -> Vec<Issue> {
        let mut out = Vec::new();
        let mut add = |row, column, message: String| {
            out.push(Issue {
                row,
                column,
                message,
            });
        };
        if self.keys().is_empty() {
            add(None, None, "at least one column must be a key".into());
        }
        if self.rows.is_empty() {
            add(None, None, "the table has no members".into());
        }
        let params = part.parameter_table();
        for (i, c) in self.columns.iter().enumerate() {
            if !expr::valid_name(&c.name) {
                add(None, Some(i), format!("'{}' is not a valid name", c.name));
            }
            if self.columns[..i].iter().any(|o| o.name == c.name) {
                add(None, Some(i), format!("'{}' appears twice", c.name));
            }
            match &c.target {
                Target::Parameter(p) if !params.iter().any(|r| r.name == *p) => {
                    add(None, Some(i), format!("the part has no parameter '{p}'"));
                }
                Target::Suppress(f) if !part.features.iter().any(|x| x.id() == f) => {
                    add(None, Some(i), format!("the part has no feature '{f}'"));
                }
                _ => {}
            }
        }
        let mut seen_keys: BTreeMap<Vec<String>, usize> = BTreeMap::new();
        let mut seen_names: BTreeMap<String, usize> = BTreeMap::new();
        for r in 0..self.rows.len() {
            if self.rows[r].len() != self.columns.len() {
                add(
                    Some(r),
                    None,
                    format!(
                        "{} cells for {} columns",
                        self.rows[r].len(),
                        self.columns.len()
                    ),
                );
                continue;
            }
            if let Err(e) = self.values(r) {
                add(Some(r), None, e);
                continue;
            }
            if let Ok(k) = self.key_texts(r)
                && let Some(first) = seen_keys.insert(k, r)
            {
                add(
                    Some(r),
                    None,
                    format!("the same keys as member {}", first + 1),
                );
            }
            if let Ok(n) = self.member_name(r)
                && let Some(first) = seen_names.insert(n.clone(), r)
            {
                add(
                    Some(r),
                    None,
                    format!("the file name '{n}' is also member {}'s", first + 1),
                );
            }
        }
        out
    }

    /// Applies `row` to `part` and regenerates; on an error the part is as it
    /// was (rule 5). Returns the row's properties.
    pub fn apply(
        &self,
        part: &mut PartDocument,
        row: usize,
    ) -> Result<Vec<(String, String)>, PartError> {
        let before = part.clone();
        let result = self.apply_inner(part, row);
        if result.is_err() {
            *part = before;
        }
        result
    }

    fn apply_inner(
        &self,
        part: &mut PartDocument,
        row: usize,
    ) -> Result<Vec<(String, String)>, PartError> {
        let bad = |e: String| PartError::Feature(e);
        let vals = self.values(row).map_err(bad)?;
        let mut props = Vec::new();
        for c in &self.columns {
            let Some(v) = vals.get(&c.name) else { continue };
            match &c.target {
                Target::None | Target::FileName => {}
                Target::Parameter(p) => match v {
                    Value::Num(x, _) => {
                        // In internal units (a plain number stores no equation).
                        let row_unit = part
                            .parameter(p)
                            .ok_or_else(|| bad(format!("the part has no parameter '{p}'")))?
                            .def();
                        let text = plain(row_unit.to_display(*x));
                        part.set_parameter_expr(p, &text)?;
                    }
                    other => return Err(bad(format!("{}: {other:?} is no number", c.heading()))),
                },
                Target::Property(key) => props.push((key.clone(), Self::shown(c, v))),
                Target::Suppress(id) => {
                    let k = part
                        .features
                        .iter()
                        .position(|f| f.id() == id)
                        .ok_or_else(|| bad(format!("the part has no feature '{id}'")))?;
                    let on = matches!(v, Value::Bool(true))
                        || matches!(v, Value::Num(x, _) if *x != 0.0);
                    if on {
                        part.suppress_feature(k)?;
                    } else if part.suppressed.contains(id) {
                        part.unsuppress_feature(k)?;
                    }
                }
                Target::Material => {
                    part.set_material(Some(&Self::shown(c, v)), crate::materials::with_user())
                        .map_err(bad)?;
                }
            }
        }
        part.regenerate()?;
        if let Some((id, why)) = part.feature_health.iter().next() {
            return Err(bad(format!("{id}: {why}")));
        }
        Ok(props)
    }

    /// The member of `row`: `template` is not changed.
    pub fn member(
        &self,
        template: &PartDocument,
        properties: &IProperties,
        row: usize,
    ) -> Result<Member, PartError> {
        let mut part = template.clone();
        part.family = None;
        let set = self.apply(&mut part, row)?;
        let mut props = properties.clone();
        for (k, v) in set {
            props.set(&k, &v);
        }
        let file_name = self.member_name(row).map_err(PartError::Feature)?;
        Ok(Member {
            part,
            properties: props,
            file_name,
        })
    }

    /// The rows as CSV, headed by the column names (the spreadsheet route).
    pub fn to_csv(&self) -> String {
        let quote = |s: &str| {
            if s.contains([',', '"', '\n']) {
                format!("\"{}\"", s.replace('"', "\"\""))
            } else {
                s.to_owned()
            }
        };
        let mut out = String::new();
        let head: Vec<String> = self.columns.iter().map(|c| quote(&c.name)).collect();
        out.push_str(&head.join(","));
        out.push('\n');
        for r in &self.rows {
            let cells: Vec<String> = r.iter().map(|c| quote(c)).collect();
            out.push_str(&cells.join(","));
            out.push('\n');
        }
        out
    }

    /// Replaces the rows from CSV written by [`Self::to_csv`] or a spreadsheet:
    /// columns are matched by name, so their order may differ. A column
    /// missing from the text or a name the table does not have is an error,
    /// and the table is left as it was.
    pub fn read_csv(&mut self, text: &str) -> Result<usize, String> {
        let mut lines = parse_csv(text).into_iter();
        let head = lines.next().ok_or("the text is empty")?;
        let mut order = Vec::with_capacity(self.columns.len());
        for c in &self.columns {
            let at = head
                .iter()
                .position(|h| h.trim() == c.name)
                .ok_or_else(|| format!("no column '{}' in the text", c.name))?;
            order.push(at);
        }
        if let Some(extra) = head
            .iter()
            .find(|h| !self.columns.iter().any(|c| c.name == h.trim()))
        {
            return Err(format!("the table has no column '{}'", extra.trim()));
        }
        let mut rows = Vec::new();
        for (n, line) in lines.enumerate() {
            if line.iter().all(|c| c.trim().is_empty()) {
                continue;
            }
            if line.len() != head.len() {
                return Err(format!(
                    "line {}: {} cells, the heading has {}",
                    n + 2,
                    line.len(),
                    head.len()
                ));
            }
            rows.push(order.iter().map(|&i| line[i].clone()).collect());
        }
        let count = rows.len();
        self.rows = rows;
        self.default_row = self.default_row.min(count.saturating_sub(1));
        Ok(count)
    }
}

/// Lines of cells; quotes may hold commas, doubled quotes and line ends.
fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    let mut line = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match (ch, quoted) {
            ('"', true) if chars.peek() == Some(&'"') => {
                chars.next();
                cell.push('"');
            }
            ('"', _) => quoted = !quoted,
            (',', false) => line.push(std::mem::take(&mut cell)),
            ('\r', false) => {}
            ('\n', false) => {
                line.push(std::mem::take(&mut cell));
                out.push(std::mem::take(&mut line));
            }
            (c, _) => cell.push(c),
        }
    }
    if !cell.is_empty() || !line.is_empty() {
        line.push(cell);
        out.push(line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::part::frame::{Loop, draw_loop};
    use crate::part::physical::physical_properties;
    use crate::part::{
        ExtrudeFeature, Operation, OriginPlane, ProfileRef, SketchOptions, SketchPlacement,
    };
    use crate::sketch::Point2;

    fn volume(p: &PartDocument) -> f64 {
        physical_properties(p, Default::default())
            .unwrap()
            .total
            .volume
    }

    /// A block with parameters `length`, `width`, `height` and, on top, a
    /// feature `Extrusion1` of a 2 × 2 × 5 peg.
    fn template() -> PartDocument {
        let mut part = PartDocument::with_box("Box1", [10.0, 20.0, 30.0]).unwrap();
        let opts = SketchOptions {
            auto_project_origin: false,
            auto_reference_edges: false,
            ..Default::default()
        };
        let i = part
            .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts)
            .unwrap();
        let (a, b) = (-5.0, -3.0);
        draw_loop(
            &mut part.sketches[i].sketch,
            Loop::Path(vec![
                (Point2::new(a, a), None),
                (Point2::new(b, a), None),
                (Point2::new(b, b), None),
                (Point2::new(a, b), None),
            ]),
        )
        .unwrap();
        let regions = part.sketches[i].sketch.regions();
        let r = &regions[0];
        let mut e = ExtrudeFeature::new(vec![ProfileRef::of(r, r.interior_point())], 5.0);
        e.operation = Operation::NewSolid;
        part.add_extrude(i, e).unwrap();
        part.rename_parameter("d0", "length").unwrap();
        part.rename_parameter("d1", "width").unwrap();
        part.rename_parameter("d2", "height").unwrap();
        part
    }

    fn table() -> FamilyTable {
        let mut len = Column::new("L", Unit::Mm, Target::Parameter("length".into()));
        len.key = 1;
        let mut wid = Column::new("W", Unit::Mm, Target::Parameter("width".into()));
        wid.key = 2;
        let height = Column::new("H", Unit::Mm, Target::Parameter("height".into()));
        let mut pn = Column::new("PN", Unit::Text, Target::Property("part_number".into()));
        pn.formula = Some("\"Block \" + L + \" x \" + W".into());
        let mut file = Column::new("FILE", Unit::Text, Target::FileName);
        file.formula = Some("\"B\" + L + \"-\" + W".into());
        FamilyTable {
            id: "t".into(),
            columns: vec![len, wid, height, pn, file],
            rows: vec![
                vec![
                    "10".into(),
                    "20".into(),
                    "30".into(),
                    String::new(),
                    String::new(),
                ],
                vec![
                    "10".into(),
                    "40".into(),
                    "L * 2".into(),
                    String::new(),
                    String::new(),
                ],
                vec![
                    "25".into(),
                    "40".into(),
                    "0.03 m".into(),
                    String::new(),
                    String::new(),
                ],
            ],
            default_row: 0,
        }
    }

    fn body_volume_of(p: &PartDocument, row: &FamilyTable, r: usize) -> f64 {
        let m = row.member(p, &IProperties::default(), r).unwrap();
        volume(&m.part)
    }

    #[test]
    fn a_table_of_the_right_shape_verifies() {
        let issues = table().verify(&template());
        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn cells_are_expressions_with_units_over_other_columns() {
        let t = table();
        let v = t.values(1).unwrap();
        assert!((v["H"].number() - 20.0).abs() < 1e-9, "{:?}", v["H"]);
        assert!((t.values(2).unwrap()["H"].number() - 30.0).abs() < 1e-9);
        assert_eq!(v["PN"], Value::Text("Block 10 x 40".into()));
        assert_eq!(t.member_name(1).unwrap(), "B10-40");
    }

    #[test]
    fn keys_narrow_the_choice_in_order() {
        let t = table();
        assert_eq!(t.choices(&[]), ["10", "25"]);
        assert_eq!(t.choices(&["10".to_owned()]), ["20", "40"]);
        assert_eq!(t.choices(&["25".to_owned()]), ["40"]);
        assert_eq!(t.find(&["10".to_owned(), "40".to_owned()]), Some(1));
        assert_eq!(t.find(&["25".to_owned(), "20".to_owned()]), None);
    }

    #[test]
    fn a_member_applies_its_row_and_leaves_the_template() {
        let part = template();
        let v0 = volume(&part);
        let t = table();
        let peg = 2.0 * 2.0 * 5.0;
        let m = t.member(&part, &IProperties::default(), 1).unwrap();
        assert!(
            (volume(&m.part) - (10.0 * 40.0 * 20.0 + peg)).abs() < 1e-6,
            "{}",
            volume(&m.part)
        );
        assert_eq!(m.properties.get("part_number"), "Block 10 x 40");
        assert_eq!(m.file_name, "B10-40");
        assert!(m.part.family.is_none(), "a member holds no table");
        assert!((volume(&part) - v0).abs() < 1e-9, "the template changed");
    }

    #[test]
    fn a_suppress_column_takes_a_feature_out_for_one_member() {
        let part = template();
        let mut t = table();
        t.columns.push(Column::new(
            "NoPeg",
            Unit::Bool,
            Target::Suppress("Extrusion1".into()),
        ));
        t.rows[0].push("true".into());
        t.rows[1].push("false".into());
        t.rows[2].push("L > 20".into());
        assert!(t.verify(&part).is_empty(), "{:?}", t.verify(&part));
        let peg = 2.0 * 2.0 * 5.0;
        assert!((body_volume_of(&part, &t, 0) - 6000.0).abs() < 1e-6);
        assert!((body_volume_of(&part, &t, 1) - (8000.0 + peg)).abs() < 1e-6);
        assert!((body_volume_of(&part, &t, 2) - 30000.0).abs() < 1e-6);
    }

    #[test]
    fn verify_names_what_is_wrong() {
        let part = template();
        let message = |t: &FamilyTable| {
            t.verify(&part)
                .into_iter()
                .map(|i| i.message)
                .collect::<Vec<_>>()
                .join("; ")
        };
        let mut t = table();
        t.columns[0].target = Target::Parameter("nope".into());
        assert!(
            message(&t).contains("no parameter 'nope'"),
            "{}",
            message(&t)
        );
        let mut t = table();
        t.rows[2][1] = "20".into();
        t.rows[2][0] = "10".into();
        assert!(
            message(&t).contains("the same keys as member 1"),
            "{}",
            message(&t)
        );
        let mut t = table();
        t.rows[0][2] = "H * 2".into();
        assert!(message(&t).contains("circular"), "{}", message(&t));
        let mut t = table();
        t.rows[0][2] = "5 deg".into();
        assert!(message(&t).contains("H:"), "{}", message(&t));
        let mut t = table();
        for c in &mut t.columns {
            c.key = 0;
        }
        assert!(message(&t).contains("must be a key"), "{}", message(&t));
    }

    #[test]
    fn a_failing_member_leaves_the_part_unchanged() {
        let mut part = template();
        let mut t = table();
        // L is applied before the bad column is reached.
        t.rows[0][0] = "77".into();
        t.columns[2].target = Target::Parameter("nope".into());
        let names = part.parameter("length").unwrap().value;
        assert!(t.apply(&mut part, 0).is_err());
        assert!((part.parameter("length").unwrap().value - names).abs() < 1e-12);
    }

    #[test]
    fn the_table_goes_through_csv_in_any_column_order() {
        let t = table();
        let mut back = t.clone();
        assert_eq!(back.read_csv(&t.to_csv()).unwrap(), 3);
        assert_eq!(back, t);
        let mut other = t.clone();
        let text = "W,L,H,PN,FILE\r\n1,2,\"3, 4\",,\n";
        assert_eq!(other.read_csv(text).unwrap(), 1);
        assert_eq!(other.rows[0][..3], ["2", "1", "3, 4"]);
        let before = t.clone();
        let mut wrong = t.clone();
        assert!(wrong.read_csv("L,W\n1,2\n").is_err());
        assert_eq!(wrong, before);
    }

    #[test]
    fn the_table_saves_with_the_part() {
        let mut part = template();
        part.family = Some(table());
        let json = serde_json::to_string(&part).unwrap();
        let back: PartDocument = serde_json::from_str(&json).unwrap();
        assert_eq!(back.family, part.family);
        // A part without a table writes nothing new (rule 6).
        let plain = template();
        assert!(
            !serde_json::to_string(&plain)
                .unwrap()
                .contains("\"family\"")
        );
    }
}
