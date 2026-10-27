//! Bill of Materials (`docs/spec/3.6-bom.md`): the rows of an assembly's
//! BOM in the Model Data, Structured and Parts Only views, from its
//! occurrences and each component's iProperties and BOM Structure; CSV
//! export. GUI-free: the components are reached through a callback.

use std::collections::BTreeMap;

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
    /// Its iProperties.
    pub props: IProperties,
    /// Its material's name.
    pub material: Option<String>,
    /// The part (file) could not be found.
    pub missing: bool,
    /// A sheet metal part's flat pattern extents: length, width (mm) and
    /// area (mm²) — the columns `Flat Pattern Length / Width / Area`.
    pub flat: Option<(f64, f64, f64)>,
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
    /// Item number (empty in Model Data).
    pub item: String,
    /// Part Number (the file name when not set).
    pub part_number: String,
    /// BOM Structure (the occurrence's override first).
    pub structure: BomStructure,
    /// Quantity.
    pub qty: u32,
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
    /// Custom iProperties by name (text).
    pub custom: BTreeMap<String, String>,
    /// The component's standard iProperties by key (`iprops::STANDARD`),
    /// for parts list columns.
    pub props: BTreeMap<String, String>,
    /// A component of it was not found.
    pub missing: bool,
}

/// The rows of `asm` in `view`. `merge` puts components of one part number
/// in one row (Part Number Row Merge); `info` describes a component.
pub fn rows(
    asm: &AssemblyDocument,
    view: BomView,
    merge: bool,
    info: &dyn Fn(DocRef) -> ComponentInfo,
) -> Vec<BomRow> {
    let mut out: Vec<BomRow> = Vec::new();
    // Suppressed components are left out (Inventor).
    for o in asm.occurrences.iter().filter(|o| !o.suppressed) {
        let ci = info(o.component);
        let structure = o.bom.unwrap_or(ci.props.bom_structure);
        // Reference and Phantom parts are not in a reportable BOM (no
        // sub-assemblies yet, so a phantom has no children to promote).
        if view != BomView::ModelData
            && matches!(structure, BomStructure::Reference | BomStructure::Phantom)
        {
            continue;
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
            r.occurrences.push(o.id);
            if !r.components.contains(&o.component) {
                r.components.push(o.component);
            }
            continue;
        }
        out.push(BomRow {
            item: String::new(),
            part_number,
            structure,
            qty: 1,
            unit: "Each".into(),
            stock_number: ci.props.get("stock_number").to_owned(),
            description: ci.props.get("description").to_owned(),
            material: ci.material.clone().unwrap_or_default(),
            components: vec![o.component],
            occurrences: vec![o.id],
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
    if view != BomView::ModelData {
        for (k, r) in out.iter_mut().enumerate() {
            r.item = (k + 1).to_string();
        }
    }
    out
}

/// The custom iProperty names used by `rows` (optional columns).
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
}
