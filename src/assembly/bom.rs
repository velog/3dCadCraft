//! Bill of Materials (`docs/spec/3.6-bom.md`): the rows of an assembly's
//! BOM in the Model Data, Structured and Parts Only views, from its
//! occurrences and each component's Properties and BOM Structure; CSV
//! export. GUI-free: the components are reached through a callback.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::assembly::tree;
use crate::document::iprops::{BomStructure, IProperties};
use crate::document::{AssemblyDocument, DocRef};

/// The BOM's views (Inventor's tabs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BomView {
    /// Every occurrence, as modelled (Reference and Phantom ones too).
    ModelData,
    /// The reportable BOM (sub-assemblies with their children).
    Structured,
    /// Parts only, quantities summed over every level.
    PartsOnly,
}

/// What the BOM needs of a component.
#[derive(Clone, Debug, Default)]
pub struct ComponentInfo {
    /// File (or document) name: the default Part Number.
    pub file_name: String,
    /// Its Properties.
    pub props: IProperties,
    /// Its material's name.
    pub material: Option<String>,
    /// The part (file) could not be found.
    pub missing: bool,
    /// A sheet metal part's flat pattern extents: length, width (mm) and
    /// area (mm²) — the columns `Flat Pattern Length / Width / Area`.
    pub flat: Option<(f64, f64, f64)>,
    /// The component's own occurrences when it is an assembly (a
    /// subassembly).
    pub assembly: Option<AssemblyDocument>,
}

/// Item numbers one view of the BOM keeps, by part number.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BomNumbers {
    /// What Renumber gave them: plain numbers that stay when rows come and go.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub renumbered: BTreeMap<String, String>,
    /// Typed by the user (an override, shown as one).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub typed: BTreeMap<String, String>,
}

impl BomNumbers {
    fn is_empty(&self) -> bool {
        self.renumbered.is_empty() && self.typed.is_empty()
    }

    fn of(&self, part_number: &str) -> Option<&String> {
        self.typed
            .get(part_number)
            .or_else(|| self.renumbered.get(part_number))
    }
}

/// What the user changed in an assembly's BOM (spec 3.6): everything else in it is
/// computed. Rows are found by part number, as a parts list's overrides are.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BomEdits {
    /// Item numbers of the Structured view.
    #[serde(default, skip_serializing_if = "BomNumbers::is_empty")]
    pub structured: BomNumbers,
    /// Item numbers of the Parts Only view.
    #[serde(default, skip_serializing_if = "BomNumbers::is_empty")]
    pub parts_only: BomNumbers,
    /// Static quantities, typed instead of the calculated ones.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub quantity: BTreeMap<String, u32>,
}

impl BomEdits {
    /// Nothing changed by the user.
    pub fn is_empty(&self) -> bool {
        self.structured.is_empty() && self.parts_only.is_empty() && self.quantity.is_empty()
    }

    fn numbers(&mut self, view: BomView) -> &mut BomNumbers {
        match view {
            BomView::PartsOnly => &mut self.parts_only,
            _ => &mut self.structured,
        }
    }

    /// Item Renumber: `parts` (in the order shown) from `start` by `step`.
    pub fn renumber(&mut self, view: BomView, parts: &[String], start: u32, step: u32) {
        let n = self.numbers(view);
        let mut value = start;
        for p in parts {
            n.typed.remove(p);
            n.renumbered.insert(p.clone(), value.to_string());
            value = value.saturating_add(step);
        }
    }

    /// A typed item number; empty text takes the override away.
    pub fn set_item(&mut self, view: BomView, part_number: &str, text: &str) {
        let n = self.numbers(view);
        if text.trim().is_empty() {
            n.typed.remove(part_number);
        } else {
            n.typed
                .insert(part_number.to_owned(), text.trim().to_owned());
        }
    }

    /// Static Quantity (`Some`) or Calculate Quantity (`None`).
    pub fn set_quantity(&mut self, part_number: &str, qty: Option<u32>) {
        match qty {
            Some(q) => {
                self.quantity.insert(part_number.to_owned(), q);
            }
            None => {
                self.quantity.remove(part_number);
            }
        }
    }

    /// A row's part number changed: what was kept for it goes along.
    pub fn rename(&mut self, old: &str, new: &str) {
        if old == new {
            return;
        }
        fn carry<T>(m: &mut BTreeMap<String, T>, old: &str, new: &str) {
            if let Some(v) = m.remove(old) {
                m.insert(new.to_owned(), v);
            }
        }
        for n in [&mut self.structured, &mut self.parts_only] {
            carry(&mut n.renumbered, old, new);
            carry(&mut n.typed, old, new);
        }
        carry(&mut self.quantity, old, new);
    }
}

/// Item numbers of one level's rows: what is kept for them, and for the others the
/// next numbers nobody has (so with nothing kept: 1, 2, 3...).
fn number_rows(rows: &mut [BomRow], numbers: &BomNumbers, prefix: &str) {
    let taken: BTreeSet<String> = rows
        .iter()
        .filter_map(|r| numbers.of(&r.part_number).cloned())
        .collect();
    let mut next = 0u32;
    for r in rows {
        r.item_typed = numbers.typed.contains_key(&r.part_number);
        r.item = match numbers.of(&r.part_number) {
            Some(kept) => format!("{prefix}{kept}"),
            None => loop {
                next += 1;
                let text = next.to_string();
                if !taken.contains(&text) {
                    break format!("{prefix}{text}");
                }
            },
        };
    }
}

/// Static quantities in place of the calculated ones.
fn static_quantities(rows: &mut [BomRow], edits: &BomEdits) {
    for r in rows {
        if let Some(q) = edits.quantity.get(&r.part_number) {
            r.qty = *q;
            r.qty_static = true;
        }
    }
}

/// The item numbers shown more than once among `rows` (a warning, not a refusal).
pub fn duplicate_items(rows: &[BomRow]) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut twice = BTreeSet::new();
    for r in rows {
        if !seen.insert(r.item.clone()) {
            twice.insert(r.item.clone());
        }
    }
    twice
}

/// The BOM columns of a sheet metal part's flat pattern (Inventor's FLAT
/// PATTERN EXTENTS LENGTH / WIDTH / AREA).
pub const FLAT_COLUMNS: [&str; 3] = [
    "Flat Pattern Length",
    "Flat Pattern Width",
    "Flat Pattern Area",
];

/// One row.
#[derive(Clone, Debug, PartialEq)]
pub struct BomRow {
    /// Item number (empty in Model Data; `1.2` for the second row inside
    /// item 1).
    pub item: String,
    /// Nesting level in the Structured view (0 = the assembly's own).
    pub level: usize,

    /// Part Number (the file name when not set).
    pub part_number: String,
    /// BOM Structure (the occurrence's override first).
    pub structure: BomStructure,
    /// Quantity.
    pub qty: u32,
    /// The quantity was typed (Static), not calculated.
    pub qty_static: bool,
    /// The item number was typed (an override).
    pub item_typed: bool,
    /// Unit quantity ("Each").
    pub unit: String,
    /// Stock Number.
    pub stock_number: String,
    /// Description.
    pub description: String,
    /// Material.
    pub material: String,
    /// The components in the row (several when merged by part number).
    pub components: Vec<DocRef>,
    /// The occurrences counted (ids).
    pub occurrences: Vec<u32>,
    /// Custom Properties by name (text).
    pub custom: BTreeMap<String, String>,
    /// The component's standard Properties by key (`iprops::STANDARD`),
    /// for parts list columns.
    pub props: BTreeMap<String, String>,
    /// A component of it was not found.
    pub missing: bool,
}

/// The rows of `asm` in `view`, the Structured view at its first level.
/// `merge` puts components of one part number in one row (Part Number Row
/// Merge); `info` describes a component.
pub fn rows(
    asm: &AssemblyDocument,
    view: BomView,
    merge: bool,
    info: &dyn Fn(DocRef) -> ComponentInfo,
) -> Vec<BomRow> {
    rows_in(asm, view, merge, false, info)
}

/// The rows of `asm` in `view` (`docs/spec/3.1a-subassemblies.md`): Model
/// Data lists the first level; Structured lists each level's components
/// (`all_levels`: a Normal subassembly's rows follow it, numbered `1.1`,
/// quantities per subassembly); Parts Only lists the parts of every level
/// with their total quantities. A Phantom subassembly is not listed and
/// its components move up a level; a Purchased or Inseparable one is one
/// item; Reference components are left out.
pub fn rows_in(
    asm: &AssemblyDocument,
    view: BomView,
    merge: bool,
    all_levels: bool,
    info: &dyn Fn(DocRef) -> ComponentInfo,
) -> Vec<BomRow> {
    let mut out = Vec::new();
    let mut stack = Vec::new();
    match view {
        BomView::ModelData => gather(asm, view, merge, info, None, &mut stack, &mut out),
        BomView::Structured => {
            structured(asm, merge, all_levels, info, "", 0, &mut stack, &mut out);
        }
        BomView::PartsOnly => {
            gather(asm, view, merge, info, None, &mut stack, &mut out);
            number_rows(&mut out, &asm.bom.parts_only, "");
            static_quantities(&mut out, &asm.bom);
        }
    }
    out
}

/// One level of the Structured view, numbered after `prefix`, each Normal
/// subassembly followed by its own rows when `all`.
#[allow(clippy::too_many_arguments)]
fn structured(
    asm: &AssemblyDocument,
    merge: bool,
    all: bool,
    info: &dyn Fn(DocRef) -> ComponentInfo,
    prefix: &str,
    level: usize,
    stack: &mut Vec<DocRef>,
    out: &mut Vec<BomRow>,
) {
    let mut rows = Vec::new();
    gather(
        asm,
        BomView::Structured,
        merge,
        info,
        None,
        stack,
        &mut rows,
    );
    number_rows(&mut rows, &asm.bom.structured, prefix);
    static_quantities(&mut rows, &asm.bom);
    for mut r in rows {
        r.level = level;
        let sub = match (all, r.structure, r.components.first()) {
            (true, BomStructure::Normal, Some(&c)) if !stack.contains(&c) => {
                info(c).assembly.map(|a| (c, a))
            }
            _ => None,
        };
        let (item, top) = (format!("{}.", r.item), r.occurrences.clone());
        out.push(r);
        if let Some((c, a)) = sub {
            let from = out.len();
            stack.push(c);
            structured(&a, merge, all, info, &item, level + 1, stack, out);
            stack.pop();
            // Balloons and parts lists find nested rows by the top-level
            // occurrence they lie in.
            for r in &mut out[from..] {
                r.occurrences = top.clone();
            }
        }
    }
}

/// The rows of one level into `out`: a Phantom subassembly's components
/// (and in Parts Only a Normal one's) join this level; `top` is the
/// top-level occurrence the level lies in (`None`: this is the top).
fn gather(
    asm: &AssemblyDocument,
    view: BomView,
    merge: bool,
    info: &dyn Fn(DocRef) -> ComponentInfo,
    top: Option<u32>,
    stack: &mut Vec<DocRef>,
    out: &mut Vec<BomRow>,
) {
    if stack.len() > tree::MAX_DEPTH {
        return;
    }
    // Suppressed components are left out (Inventor).
    for o in asm.occurrences.iter().filter(|o| !o.suppressed) {
        let mut ci = info(o.component);
        let structure = o.bom.unwrap_or(ci.props.bom_structure);
        let owner = top.unwrap_or(o.id);
        if view != BomView::ModelData {
            if structure == BomStructure::Reference {
                continue;
            }
            let walk = match structure {
                BomStructure::Phantom => true,
                BomStructure::Normal => view == BomView::PartsOnly,
                _ => false,
            };
            match ci.assembly.take() {
                Some(sub) if walk => {
                    if !stack.contains(&o.component) {
                        stack.push(o.component);
                        gather(&sub, view, merge, info, Some(owner), stack, out);
                        stack.pop();
                    }
                    continue;
                }
                // A phantom part is not listed.
                None if structure == BomStructure::Phantom => continue,
                _ => {}
            }
        }
        let part_number = if ci.missing {
            format!("(missing) {}", ci.file_name)
        } else {
            ci.props.effective("part_number", &ci.file_name)
        };
        let same = |r: &BomRow| {
            if view == BomView::ModelData {
                false
            } else if merge {
                r.part_number == part_number
            } else {
                r.components.first() == Some(&o.component)
            }
        };
        if let Some(r) = out.iter_mut().find(|r| same(r)) {
            r.qty += 1;
            if !r.occurrences.contains(&owner) {
                r.occurrences.push(owner);
            }
            if !r.components.contains(&o.component) {
                r.components.push(o.component);
            }
            continue;
        }
        out.push(BomRow {
            item: String::new(),
            level: 0,
            part_number,
            structure,
            qty: 1,
            qty_static: false,
            item_typed: false,
            unit: unit_qty(&ci.props),
            stock_number: ci.props.get("stock_number").to_owned(),
            description: ci.props.get("description").to_owned(),
            material: ci.material.clone().unwrap_or_default(),
            components: vec![o.component],
            occurrences: vec![owner],
            custom: ci
                .props
                .custom
                .iter()
                .map(|c| (c.name.clone(), c.value.text()))
                .chain(ci.flat.iter().flat_map(|&(l, w, a)| {
                    [
                        (FLAT_COLUMNS[0].to_owned(), format!("{l:.2} mm")),
                        (FLAT_COLUMNS[1].to_owned(), format!("{w:.2} mm")),
                        (FLAT_COLUMNS[2].to_owned(), format!("{a:.2} mm^2")),
                    ]
                }))
                .collect(),
            props: ci.props.standard.clone(),
            missing: ci.missing,
        });
    }
}

/// The Unit QTY of a component: a frame member's length (`G_L`), else Each.
fn unit_qty(props: &crate::document::iprops::IProperties) -> String {
    use crate::document::iprops::PropValue;
    props
        .custom
        .iter()
        .find(|c| c.name == crate::part::frame::LENGTH_PROPERTY)
        .and_then(|c| match c.value {
            PropValue::Number(l) => Some(format!("{l:.3} mm")),
            _ => None,
        })
        .unwrap_or_else(|| "Each".into())
}

/// The custom Property names used by `rows` (optional columns).
pub fn custom_columns(rows: &[BomRow]) -> Vec<String> {
    let mut v: Vec<String> = rows.iter().flat_map(|r| r.custom.keys().cloned()).collect();
    v.sort();
    v.dedup();
    v
}

/// The rows as CSV (UTF-8, a header row; `extra` custom columns at the end).
pub fn to_csv(rows: &[BomRow], extra: &[String]) -> String {
    let cell = |s: &str| {
        if s.contains([',', '"', '\n']) {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s.to_owned()
        }
    };
    let mut head: Vec<String> = [
        "Item",
        "Part Number",
        "BOM Structure",
        "Unit QTY",
        "QTY",
        "Stock Number",
        "Description",
        "Material",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    head.extend(extra.iter().cloned());
    let mut out = head.iter().map(|h| cell(h)).collect::<Vec<_>>().join(",");
    out.push('\n');
    for r in rows {
        let mut v = vec![
            r.item.clone(),
            r.part_number.clone(),
            r.structure.label().to_owned(),
            r.unit.clone(),
            r.qty.to_string(),
            r.stock_number.clone(),
            r.description.clone(),
            r.material.clone(),
        ];
        v.extend(
            extra
                .iter()
                .map(|c| r.custom.get(c).cloned().unwrap_or_default()),
        );
        out.push_str(&v.iter().map(|s| cell(s)).collect::<Vec<_>>().join(","));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::DocumentId;
    use crate::document::iprops::PropValue;
    use crate::kernel::Transform3;

    #[test]
    fn a_frame_member_s_unit_quantity_is_its_length() {
        let mut p = crate::document::iprops::IProperties::default();
        assert_eq!(unit_qty(&p), "Each");
        p.custom.push(crate::document::iprops::CustomProperty {
            name: crate::part::frame::LENGTH_PROPERTY.into(),
            value: PropValue::Number(1250.0),
        });
        assert_eq!(unit_qty(&p), "1250.000 mm");
    }

    #[test]
    fn quantities_structures_merging_and_csv() {
        let (bolt, plate, nut) = (
            DocRef::local(DocumentId::new_random()),
            DocRef::local(DocumentId::new_random()),
            DocRef::local(DocumentId::new_random()),
        );
        let mut a = AssemblyDocument::default();
        a.place(plate, "plate", Transform3::identity(), true);
        for _ in 0..4 {
            a.place(bolt, "bolt", Transform3::identity(), false);
        }
        a.place(nut, "nut", Transform3::identity(), false);
        // One bolt is only a reference (instance override).
        a.occurrences[4].bom = Some(BomStructure::Reference);
        let info = |r: DocRef| {
            let mut props = IProperties::default();
            let name = if r == bolt {
                props.set("description", "Hex bolt, M8");
                props
                    .add_custom("Grade", PropValue::Text("8.8".into()))
                    .unwrap();
                "bolt"
            } else if r == plate {
                props.set("part_number", "PL-10");
                "plate"
            } else {
                props.bom_structure = BomStructure::Purchased;
                // The nut shares the bolt's part number (a kit).
                props.set("part_number", "bolt");
                "nut"
            };
            ComponentInfo {
                file_name: name.into(),
                props,
                material: Some("Steel".into()),
                missing: false,
                flat: None,
                assembly: None,
            }
        };
        let s = rows(&a, BomView::Structured, false, &info);
        let got: Vec<(&str, &str, u32)> = s
            .iter()
            .map(|r| (r.item.as_str(), r.part_number.as_str(), r.qty))
            .collect();
        assert_eq!(
            got,
            vec![("1", "PL-10", 1), ("2", "bolt", 3), ("3", "bolt", 1)]
        );
        assert_eq!(s[2].structure, BomStructure::Purchased);
        // Merged by part number: the bolts and the nut are one row.
        let m = rows(&a, BomView::PartsOnly, true, &info);
        assert_eq!(m.len(), 2);
        assert_eq!((m[1].qty, m[1].components.len()), (4, 2));
        // Model Data lists every occurrence, the reference one too.
        assert_eq!(rows(&a, BomView::ModelData, true, &info).len(), 6);
        let cols = custom_columns(&s);
        assert_eq!(cols, vec!["Grade".to_owned()]);
        let csv = to_csv(&s, &cols);
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(
            lines[0],
            "Item,Part Number,BOM Structure,Unit QTY,QTY,Stock Number,Description,Material,Grade"
        );
        assert_eq!(lines[2], "2,bolt,Normal,Each,3,,\"Hex bolt, M8\",Steel,8.8");
        // A suppressed bolt leaves the BOM (Model Data too).
        a.occurrences[1].suppressed = true;
        let s = rows(&a, BomView::Structured, false, &info);
        assert_eq!(s[1].qty, 2);
        assert_eq!(rows(&a, BomView::ModelData, true, &info).len(), 5);
    }

    /// A sheet metal part's flat pattern extents are BOM columns.
    #[test]
    fn flat_pattern_columns() {
        let plate = DocRef::local(DocumentId::new_random());
        let mut a = AssemblyDocument::default();
        a.place(plate, "plate", Transform3::identity(), true);
        let info = |_| ComponentInfo {
            file_name: "bracket".into(),
            flat: Some((120.0, 80.0, 9600.0)),
            ..Default::default()
        };
        let s = rows(&a, BomView::Structured, false, &info);
        let cols = custom_columns(&s);
        assert_eq!(cols.len(), 3);
        assert!(FLAT_COLUMNS.iter().all(|c| cols.iter().any(|x| x == c)));
        assert_eq!(s[0].custom[FLAT_COLUMNS[0]], "120.00 mm");
        assert_eq!(s[0].custom[FLAT_COLUMNS[2]], "9600.00 mm^2");
    }

    /// A subassembly "Bracket" (a plate and two posts) placed twice next to
    /// a base: Structured First / All Levels, Parts Only totals, and the
    /// Phantom / Purchased structures of the subassembly.
    #[test]
    fn subassembly_levels() {
        let (base, plate, post, sub) = (
            DocRef::local(DocumentId::new_random()),
            DocRef::local(DocumentId::new_random()),
            DocRef::local(DocumentId::new_random()),
            DocRef::local(DocumentId::new_random()),
        );
        let mut bracket = AssemblyDocument::default();
        bracket.place(plate, "plate", Transform3::identity(), true);
        bracket.place(post, "post", Transform3::identity(), false);
        bracket.place(post, "post", Transform3::identity(), false);
        let mut top = AssemblyDocument::default();
        top.place(base, "base", Transform3::identity(), true);
        top.place(sub, "bracket", Transform3::identity(), false);
        top.place(sub, "bracket", Transform3::identity(), false);
        let with = |structure: BomStructure| {
            let bracket = bracket.clone();
            move |r: DocRef| {
                let mut props = IProperties::default();
                let name = if r == base {
                    "base"
                } else if r == plate {
                    "plate"
                } else if r == post {
                    "post"
                } else {
                    props.bom_structure = structure;
                    "bracket"
                };
                ComponentInfo {
                    file_name: name.into(),
                    props,
                    assembly: (r == sub).then(|| bracket.clone()),
                    ..Default::default()
                }
            }
        };
        let info = with(BomStructure::Normal);
        let list = |rows: Vec<BomRow>| -> Vec<(String, String, u32)> {
            rows.into_iter()
                .map(|r| (r.item, r.part_number, r.qty))
                .collect()
        };
        let row = |i: &str, p: &str, q: u32| (i.to_owned(), p.to_owned(), q);
        let first = rows(&top, BomView::Structured, false, &info);
        assert_eq!(list(first), [row("1", "base", 1), row("2", "bracket", 2)]);
        let all = rows_in(&top, BomView::Structured, false, true, &info);
        assert_eq!(all[2].level, 1);
        // Nested rows point at the subassembly's occurrences (balloons).
        assert_eq!(all[2].occurrences, [2, 3]);
        assert_eq!(
            list(all),
            [
                row("1", "base", 1),
                row("2", "bracket", 2),
                row("2.1", "plate", 1),
                row("2.2", "post", 2),
            ]
        );
        // Parts Only: every level, quantities multiplied.
        assert_eq!(
            list(rows(&top, BomView::PartsOnly, false, &info)),
            [
                row("1", "base", 1),
                row("2", "plate", 2),
                row("3", "post", 4)
            ]
        );
        // Model Data: the first level as modelled.
        assert_eq!(rows(&top, BomView::ModelData, false, &info).len(), 3);
        // Phantom: not listed, its parts move up a level.
        let info = with(BomStructure::Phantom);
        assert_eq!(
            list(rows_in(&top, BomView::Structured, false, true, &info)),
            [
                row("1", "base", 1),
                row("2", "plate", 2),
                row("3", "post", 4)
            ]
        );
        // Purchased: one item, its parts listed nowhere.
        let info = with(BomStructure::Purchased);
        assert_eq!(
            list(rows_in(&top, BomView::Structured, false, true, &info)),
            [row("1", "base", 1), row("2", "bracket", 2)]
        );
        assert_eq!(
            list(rows(&top, BomView::PartsOnly, false, &info)),
            [row("1", "base", 1), row("2", "bracket", 2)]
        );
    }

    /// Renumber, typed item numbers and static quantities: kept with the assembly,
    /// found by part number, in the views they were made in.
    #[test]
    fn item_numbers_and_static_quantities_stay() {
        let (x, y, z) = (
            DocRef::local(DocumentId::new_random()),
            DocRef::local(DocumentId::new_random()),
            DocRef::local(DocumentId::new_random()),
        );
        let mut a = AssemblyDocument::default();
        a.place(x, "x", Transform3::identity(), true);
        a.place(y, "y", Transform3::identity(), false);
        a.place(y, "y", Transform3::identity(), false);
        a.place(z, "z", Transform3::identity(), false);
        let info = |r: DocRef| ComponentInfo {
            file_name: if r == x {
                "X"
            } else if r == y {
                "Y"
            } else {
                "Z"
            }
            .into(),
            ..Default::default()
        };
        let items = |a: &AssemblyDocument, v| -> Vec<(String, String, u32)> {
            rows(a, v, true, &info)
                .into_iter()
                .map(|r| (r.part_number, r.item, r.qty))
                .collect()
        };
        let row = |p: &str, i: &str, q: u32| (p.to_owned(), i.to_owned(), q);
        // Nothing kept: 1, 2, 3 in placement order.
        assert_eq!(
            items(&a, BomView::Structured),
            [row("X", "1", 1), row("Y", "2", 2), row("Z", "3", 1)]
        );
        // Renumber in the order shown, from 10 by 5.
        let order = ["Z".to_owned(), "X".to_owned(), "Y".to_owned()];
        a.bom.renumber(BomView::Structured, &order, 10, 5);
        assert_eq!(
            items(&a, BomView::Structured),
            [row("X", "15", 1), row("Y", "20", 2), row("Z", "10", 1)]
        );
        assert!(!rows(&a, BomView::Structured, true, &info)[0].item_typed);
        // Another view keeps its own numbers.
        assert_eq!(items(&a, BomView::PartsOnly)[0], row("X", "1", 1));
        // A row that leaves does not move the others; a new one takes a free number.
        a.occurrences[0].suppressed = true;
        assert_eq!(items(&a, BomView::Structured)[0], row("Y", "20", 2));
        a.occurrences[0].suppressed = false;
        a.bom.structured.renumbered.remove("X");
        assert_eq!(items(&a, BomView::Structured)[0], row("X", "1", 1));
        // A typed number wins over Renumber's and is told apart; empty removes it.
        a.bom.set_item(BomView::Structured, "Y", "A-7");
        let got = rows(&a, BomView::Structured, true, &info);
        assert_eq!((got[1].item.as_str(), got[1].item_typed), ("A-7", true));
        a.bom.set_item(BomView::Structured, "Y", " ");
        assert_eq!(items(&a, BomView::Structured)[1], row("Y", "20", 2));
        // Numbers typed twice are reported.
        a.bom.set_item(BomView::Structured, "X", "20");
        assert!(duplicate_items(&rows(&a, BomView::Structured, true, &info)).contains("20"));
        a.bom.set_item(BomView::Structured, "X", "");
        // Static quantity, in both views; Calculate discards it. Model Data stays as
        // modelled: one row per occurrence.
        a.bom.set_quantity("Y", Some(9));
        let got = rows(&a, BomView::Structured, true, &info);
        assert_eq!((got[1].qty, got[1].qty_static), (9, true));
        assert_eq!(items(&a, BomView::PartsOnly)[1].2, 9);
        assert!(
            rows(&a, BomView::ModelData, true, &info)
                .iter()
                .all(|r| r.qty == 1)
        );
        // A changed part number takes what was kept along.
        a.bom.rename("Y", "Y2");
        assert_eq!(a.bom.quantity.get("Y2"), Some(&9));
        assert_eq!(
            a.bom.structured.renumbered.get("Y2").map(String::as_str),
            Some("20")
        );
        a.bom.set_quantity("Y2", None);
        assert!(!a.bom.quantity.contains_key("Y2"));
        // The file round trip, and an untouched BOM writes nothing.
        let back: BomEdits = serde_json::from_str(&serde_json::to_string(&a.bom).unwrap()).unwrap();
        assert_eq!(back, a.bom);
        assert_eq!(serde_json::to_string(&BomEdits::default()).unwrap(), "{}");
    }

    /// A subassembly's own numbers show under its row, with its row's number before them.
    #[test]
    fn a_subassembly_keeps_its_own_numbers() {
        let (part, sub) = (
            DocRef::local(DocumentId::new_random()),
            DocRef::local(DocumentId::new_random()),
        );
        let mut inner = AssemblyDocument::default();
        inner.place(part, "p", Transform3::identity(), false);
        inner
            .bom
            .renumber(BomView::Structured, &["P".to_owned()], 5, 1);
        let mut top = AssemblyDocument::default();
        top.place(sub, "s", Transform3::identity(), false);
        top.bom.set_item(BomView::Structured, "S", "7");
        let info = |r: DocRef| ComponentInfo {
            file_name: if r == part { "P" } else { "S" }.into(),
            assembly: (r == sub).then(|| inner.clone()),
            ..Default::default()
        };
        let got = rows_in(&top, BomView::Structured, true, true, &info);
        assert_eq!(got[0].item, "7");
        assert_eq!(got[1].item, "7.5");
    }
}
