//! Assemble › Bill of Materials (`docs/spec/3.6-bom.md`): the Model Data,
//! Structured and Parts Only views as tabs, Part Number Row Merge, sorting
//! by a column, optional custom Property columns, editing the components'
//! Part Number / Stock Number / Description / BOM Structure (components of
//! this file; a linked part file is read only), the Reference override per
//! occurrence, Item Renumber, typed item numbers, Static Quantity and Export to CSV.

use eframe::egui::{self, RichText};

use super::CadCraftApp;
use crate::assembly::bom::{self, BomRow, BomView, ComponentInfo};
use crate::document::iprops::BomStructure;
use crate::document::{DocRef, DocumentData};

/// The open window.
pub(super) struct BomWin {
    view: BomView,
    merge: bool,
    /// Structured: the rows inside subassemblies too (All Levels).
    all_levels: bool,
    /// Sort by column (index) and direction.
    sort: Option<(usize, bool)>,
    /// Custom Property columns shown.
    extra: Vec<String>,
    /// Rows checked for Item Renumber (by part number).
    picked: std::collections::BTreeSet<String>,
    /// The Item Renumber row is open.
    renumber: Option<RenumberUi>,
    /// The item number being typed: part number and text (it applies when the field
    /// is left: an emptied field would turn back into a number under the fingers).
    typing: Option<(String, String)>,
    /// The Thumbnail column is shown.
    thumbnails: bool,
    /// Its pictures.
    thumbs: super::bom_thumbs::BomThumbs,
}

/// The side of a row's picture, in points.
const THUMB_SIDE: f32 = 36.0;

/// The Item Renumber row.
struct RenumberUi {
    /// Selected Rows, else All Rows.
    selected: bool,
    start: crate::kernel::Real,
    step: crate::kernel::Real,
}

/// What the window changes in the assembly's BOM.
enum BomOp {
    Item(String, String),
    Quantity(String, Option<u32>),
    Renumber(Vec<String>, u32, u32),
}

impl Default for BomWin {
    fn default() -> Self {
        Self {
            view: BomView::Structured,
            merge: true,
            all_levels: false,
            sort: None,
            extra: Vec::new(),
            picked: Default::default(),
            renumber: None,
            typing: None,
            thumbnails: true,
            thumbs: Default::default(),
        }
    }
}

const COLS: [&str; 8] = [
    "Item",
    "Part Number",
    "BOM Structure",
    "Unit QTY",
    "QTY",
    "Stock Number",
    "Description",
    "Material",
];

impl CadCraftApp {
    /// What the BOM reads of a component.
    pub(super) fn bom_info(&self, r: DocRef) -> ComponentInfo {
        let mut info = self.bom_info_of(r);
        // A chosen variant's Part Number, Description and BOM Structure.
        if let Some(v) = r.variant
            && let Some(Some(own)) =
                self.with_part(r.base(), |p| p.variants.get(v).map(|x| x.info.clone()))
        {
            own.write(&mut info.props);
        }
        info
    }

    fn bom_info_of(&self, r: DocRef) -> ComponentInfo {
        let material = self.with_part(r, |p| p.material.clone()).flatten();
        let flat = self
            .with_part(r, |p| p.flat_extents().map(|x| (x.length, x.width, x.area)))
            .flatten();
        match r.library {
            None => match self.project.document(r.document) {
                Some(d) => ComponentInfo {
                    file_name: d.name.clone(),
                    props: d.properties.clone(),
                    material,
                    missing: false,
                    flat,
                    assembly: crate::assembly::tree::assembly_of(&self.project, &self.linked(), r)
                        .cloned(),
                },
                None => ComponentInfo {
                    file_name: "?".into(),
                    missing: true,
                    ..Default::default()
                },
            },
            Some(lib) => {
                let path = self.library_path(lib);
                let name = path
                    .as_ref()
                    .and_then(|p| p.file_stem())
                    .and_then(|s| s.to_str())
                    .map(str::to_owned)
                    .or_else(|| self.project.library(lib).map(|l| l.name.clone()))
                    .unwrap_or_else(|| "?".into());
                let props = path
                    .and_then(|p| self.part_file_pub(&p).ok())
                    .and_then(|p| p.document(r.document).map(|d| d.properties.clone()));
                ComponentInfo {
                    file_name: name,
                    missing: props.is_none(),
                    props: props.unwrap_or_default(),
                    material,
                    flat,
                    // A linked subassembly: its rows in this file's terms.
                    assembly: crate::assembly::tree::assembly_of(&self.project, &self.linked(), r)
                        .map(|a| crate::assembly::tree::rebased(a, r.library)),
                }
            }
        }
    }

    /// Assemble › Bill of Materials.
    pub(super) fn open_bom(&mut self) {
        if self.bom.is_none() {
            self.bom = Some(BomWin::default());
        }
    }

    /// The BOM window.
    pub(super) fn bom_window(&mut self, ctx: &egui::Context) {
        let Some(mut w) = self.bom.take() else {
            return;
        };
        let Some(asm) = self.assembly().cloned() else {
            return;
        };
        let mut rows = bom::rows_in(&asm, w.view, w.merge, w.all_levels, &|r| self.bom_info(r));
        let customs = bom::custom_columns(&rows);
        w.extra.retain(|c| customs.contains(c));
        if let Some((col, asc)) = w.sort {
            let key = |r: &BomRow| -> String {
                match col {
                    0 => format!("{:>8}", r.item),
                    1 => r.part_number.clone(),
                    2 => r.structure.label().into(),
                    4 => format!("{:>8}", r.qty),
                    5 => r.stock_number.clone(),
                    6 => r.description.clone(),
                    7 => r.material.clone(),
                    _ => String::new(),
                }
            };
            // Rows inside a subassembly stay below it: whole blocks are
            // sorted by their first-level row.
            let mut blocks: Vec<Vec<BomRow>> = Vec::new();
            for r in rows.drain(..) {
                match blocks.last_mut() {
                    Some(b) if r.level > 0 => b.push(r),
                    _ => blocks.push(vec![r]),
                }
            }
            blocks.sort_by(|a, b| {
                let o = key(&a[0]).to_lowercase().cmp(&key(&b[0]).to_lowercase());
                if asc { o } else { o.reverse() }
            });
            rows = blocks.into_iter().flatten().collect();
        }
        let mut open = true;
        let mut export = false;
        let mut edits: Vec<(DocRef, &'static str, String)> = Vec::new();
        let mut structures: Vec<(DocRef, BomStructure)> = Vec::new();
        let mut overrides: Vec<(u32, bool)> = Vec::new();
        let mut open_props: Option<u32> = None;
        let mut ops: Vec<BomOp> = Vec::new();
        let twice = bom::duplicate_items(&rows);
        let win = egui::Window::new(format!("Bill of Materials [{}]", self.document_title()))
            .id(egui::Id::new("bom_window"))
            .open(&mut open)
            .resizable(true)
            .default_width(820.0);
        self.layout.place("bom", ctx.content_rect(), true, win).show(ctx, |ui| {
            ui.horizontal(|ui| {
                for (v, label) in [
                    (BomView::ModelData, "Model Data"),
                    (BomView::Structured, "Structured"),
                    (BomView::PartsOnly, "Parts Only"),
                ] {
                    ui.selectable_value(&mut w.view, v, label);
                }
                ui.separator();
                ui.checkbox(&mut w.merge, "Part Number Row Merge")
                    .on_hover_text("Components with one part number are one row");
                ui.separator();
                if ui.button("Export...").on_hover_text("Save the view as a CSV file").clicked() {
                    export = true;
                }
                let can_number = w.view != BomView::ModelData;
                if ui
                    .add_enabled(can_number, egui::Button::new("Renumber Items..."))
                    .on_hover_text("New item numbers in the order shown, from a start value by an increment")
                    .on_disabled_hover_text("Model Data has no item numbers")
                    .clicked()
                {
                    // The smallest number there is; selected rows when two or more are checked.
                    let selected = w.picked.len() >= 2;
                    let lowest = rows
                        .iter()
                        .filter(|r| r.level == 0 && (!selected || w.picked.contains(&r.part_number)))
                        .filter_map(|r| r.item.parse::<u32>().ok())
                        .min()
                        .unwrap_or(1);
                    w.renumber = Some(RenumberUi {
                        selected,
                        start: f64::from(lowest),
                        step: 1.0,
                    });
                }
                ui.menu_button("Columns", |ui| {
                    ui.checkbox(&mut w.thumbnails, "Thumbnail")
                        .on_hover_text("A picture of each component");
                    if !customs.is_empty() {
                        ui.separator();
                        ui.label("Custom Properties");
                        for c in &customs {
                            let mut on = w.extra.contains(c);
                            if ui.checkbox(&mut on, c).changed() {
                                if on {
                                    w.extra.push(c.clone());
                                } else {
                                    w.extra.retain(|x| x != c);
                                }
                            }
                        }
                    }
                });
            });
            if w.view == BomView::Structured {
                ui.horizontal(|ui| {
                    ui.label("Level");
                    ui.selectable_value(&mut w.all_levels, false, "First Level")
                        .on_hover_text("Subassemblies as one row each");
                    ui.selectable_value(&mut w.all_levels, true, "All Levels")
                        .on_hover_text("The rows inside each subassembly below it (1.1, 1.2...)");
                    ui.weak("Reference components are not listed; a Phantom subassembly's components move up a level.");
                });
            }
            let model = w.view == BomView::ModelData;
            if model {
                w.renumber = None;
            }
            let picked = w.picked.len();
            let mut close_renumber = false;
            if let Some(n) = w.renumber.as_mut() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Item Renumber").strong());
                    ui.add_enabled_ui(picked >= 2, |ui| {
                        ui.radio_value(&mut n.selected, true, "Selected Rows");
                    });
                    ui.radio_value(&mut n.selected, false, "All Rows");
                    ui.separator();
                    ui.label("Start");
                    ui.add(super::valuefield::ExprValue::new(&mut n.start).range(0.0..=1_000_000.0));
                    ui.label("Increment");
                    ui.add(super::valuefield::ExprValue::new(&mut n.step).range(1.0..=10_000.0));
                    if ui.add(super::theme::ok_button("OK")).clicked() {
                        let parts: Vec<String> = rows
                            .iter()
                            .filter(|r| r.level == 0)
                            .filter(|r| !n.selected || w.picked.contains(&r.part_number))
                            .map(|r| r.part_number.clone())
                            .collect();
                        ops.push(BomOp::Renumber(parts, n.start.round() as u32, n.step.round() as u32));
                        close_renumber = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close_renumber = true;
                    }
                });
            }
            if close_renumber {
                w.renumber = None;
                w.picked.clear();
            }
            ui.separator();
            egui::ScrollArea::both().auto_shrink([false, true]).show(ui, |ui| {
                egui::Grid::new("bom_grid").striped(true).num_columns(COLS.len() + w.extra.len() + 2).show(ui, |ui| {
                    if w.thumbnails {
                        ui.label(RichText::new("Thumbnail").strong());
                    }
                    if model {
                        ui.label(RichText::new("Component").strong());
                    }
                    for (k, h) in COLS.iter().enumerate() {
                        if model && k == 0 {
                            continue;
                        }
                        let arrow = match w.sort {
                            Some((c, true)) if c == k => " ^",
                            Some((c, false)) if c == k => " v",
                            _ => "",
                        };
                        if ui.button(RichText::new(format!("{h}{arrow}")).strong()).on_hover_text("Sort").clicked() {
                            w.sort = match w.sort {
                                Some((c, true)) if c == k => Some((k, false)),
                                Some((c, false)) if c == k => None,
                                _ => Some((k, true)),
                            };
                        }
                    }
                    for c in &w.extra {
                        ui.label(RichText::new(c).strong());
                    }
                    ui.end_row();
                    for (ri, r) in rows.iter().enumerate() {
                        let doc = r.components.first().copied();
                        if w.thumbnails {
                            self.bom_thumb(ui, &mut w.thumbs, doc, THUMB_SIDE);
                        }
                        // Components of this file can be edited here.
                        let local = r.components.iter().all(|c| c.library.is_none()) && !r.missing;
                        let tip = "A linked part file: open the part to change its Properties";
                        if model {
                            let id = r.occurrences[0];
                            let name = asm.find(id).map_or("?".to_owned(), |o| o.name.clone());
                            ui.horizontal(|ui| {
                                ui.label(name);
                                let mut reference = asm.find(id).is_some_and(|o| o.bom == Some(BomStructure::Reference));
                                if ui.checkbox(&mut reference, "Ref").on_hover_text("This occurrence only: Reference (not in the BOM)").changed() {
                                    overrides.push((id, reference));
                                }
                            });
                        } else if r.level > 0 {
                            // Numbers of a subassembly's rows are its own.
                            ui.label(format!("{}{}", "    ".repeat(r.level), r.item))
                                .on_hover_text("Open the subassembly to change its numbers");
                        } else {
                            let pn = &r.part_number;
                            ui.horizontal(|ui| {
                                let mut on = w.picked.contains(pn);
                                if ui.checkbox(&mut on, "").on_hover_text("Row for Item Renumber").changed() {
                                    if on {
                                        w.picked.insert(pn.clone());
                                    } else {
                                        w.picked.remove(pn);
                                    }
                                }
                                let mut text = match &w.typing {
                                    Some((p, t)) if p == pn => t.clone(),
                                    _ => r.item.clone(),
                                };
                                let tokens = super::theme::tokens();
                                let mut field = egui::TextEdit::singleline(&mut text).desired_width(60.0);
                                // Overridden: blue. The same number twice: a yellow field.
                                if r.item_typed {
                                    field = field.text_color(tokens.link);
                                }
                                if twice.contains(&r.item) {
                                    field = field.background_color(tokens.warn_box_fill);
                                }
                                let resp = ui.add(field).on_hover_text(if twice.contains(&r.item) {
                                    "This item number is used twice"
                                } else if r.item_typed {
                                    "Typed: empty the field for the number it had"
                                } else {
                                    "Type an item number (an override)"
                                });
                                if resp.changed() {
                                    w.typing = Some((pn.clone(), text.clone()));
                                }
                                if resp.lost_focus()
                                    && w.typing.as_ref().is_some_and(|(p, _)| p == pn)
                                {
                                    ops.push(BomOp::Item(pn.clone(), text));
                                    w.typing = None;
                                }
                            });
                        }
                        let mut cell = |ui: &mut egui::Ui, key: &'static str, value: &str| {
                            let mut v = value.to_owned();
                            let resp = ui.add_enabled(local, egui::TextEdit::singleline(&mut v).desired_width(110.0));
                            let resp = if local { resp } else { resp.on_disabled_hover_text(tip) };
                            if resp.changed()
                                && let Some(d) = doc
                            {
                                edits.push((d, key, v));
                            }
                        };
                        if r.missing {
                            ui.colored_label(super::theme::tokens().error, &r.part_number);
                        } else if local {
                            cell(ui, "part_number", &r.part_number);
                        } else {
                            // A linked part file: its Properties window writes into it.
                            ui.horizontal(|ui| {
                                ui.label(&r.part_number);
                                if ui.small_button("...").on_hover_text("Properties of the part file").clicked() {
                                    open_props = Some(r.occurrences[0]);
                                }
                            });
                        }
                        let mut s = r.structure;
                        ui.add_enabled_ui(local, |ui| {
                            egui::ComboBox::from_id_salt(("bom_s", ri))
                                .selected_text(s.label())
                                .show_ui(ui, |ui| {
                                    for x in BomStructure::ALL {
                                        ui.selectable_value(&mut s, x, x.label());
                                    }
                                });
                        });
                        if s != r.structure
                            && let Some(d) = doc
                        {
                            structures.push((d, s));
                        }
                        ui.label(&r.unit);
                        if model || r.level > 0 {
                            ui.label(r.qty.to_string());
                        } else {
                            // Typing makes it Static; Calculate takes the typed one away.
                            ui.push_id(("bom_qty", ri), |ui| {
                                ui.horizontal(|ui| {
                                    let mut q = f64::from(r.qty);
                                    let before = q;
                                    ui.add(super::valuefield::ExprValue::new(&mut q).range(0.0..=1_000_000_000.0));
                                    if q != before {
                                        ops.push(BomOp::Quantity(r.part_number.clone(), Some(q.round() as u32)));
                                    }
                                    if r.qty_static
                                        && ui
                                            .small_button("Calculate")
                                            .on_hover_text("Static Quantity: back to the calculated quantity")
                                            .clicked()
                                    {
                                        ops.push(BomOp::Quantity(r.part_number.clone(), None));
                                    }
                                });
                            });
                        }
                        cell(ui, "stock_number", &r.stock_number);
                        cell(ui, "description", &r.description);
                        ui.label(&r.material);
                        for c in &w.extra {
                            ui.label(r.custom.get(c).map_or("", String::as_str));
                        }
                        ui.end_row();
                    }
                });
            });
            if rows.is_empty() {
                ui.weak("Nothing to list: place components (Assemble > Place).");
            }
        });
        // Changes to the components' Properties (one Undo step per edit).
        if !edits.is_empty() || !structures.is_empty() || !overrides.is_empty() || !ops.is_empty() {
            self.checkpoint("Bill of Materials");
            let mut renamed: Vec<(String, String)> = Vec::new();
            for (d, key, v) in edits {
                // A chosen variant's row: its own Properties (spec 3.9), which its
                // part applies when that variant is placed.
                if let Some(variant) = d.variant {
                    let key_row = crate::part::variants::RowKey::Prop(key.to_owned());
                    let cell = crate::part::variants::Cell::Text(v.clone());
                    if let Some(doc) = self.project.document_mut(d.document)
                        && let DocumentData::Part(p) = &mut doc.data
                        && p.set_variant_info(variant, &key_row, &cell).is_ok()
                    {
                        p.variants_sync();
                    }
                    continue;
                }
                if let Some(doc) = self.project.document_mut(d.document) {
                    let before = doc.properties.effective("part_number", &doc.name);
                    // An edited Part Number equal to the file name stays the default.
                    let v = if key == "part_number" && v == doc.name {
                        String::new()
                    } else {
                        v
                    };
                    doc.properties.set(key, &v);
                    renamed.push((before, doc.properties.effective("part_number", &doc.name)));
                }
            }
            let view = w.view;
            if let Some(a) = self.assembly_mut() {
                for (old, new) in renamed {
                    a.bom.rename(&old, &new);
                }
                for op in ops {
                    match op {
                        BomOp::Item(part, text) => a.bom.set_item(view, &part, &text),
                        BomOp::Quantity(part, q) => a.bom.set_quantity(&part, q),
                        BomOp::Renumber(parts, start, step) => {
                            a.bom.renumber(view, &parts, start, step);
                        }
                    }
                }
            }
            for (d, s) in structures {
                if let Some(doc) = self.project.document_mut(d.document)
                    && matches!(doc.data, DocumentData::Part(_) | DocumentData::Assembly(_))
                {
                    doc.properties.bom_structure = s;
                }
            }
            if let Some(a) = self.assembly_mut() {
                for (id, on) in overrides {
                    if let Some(o) = a.find_mut(id) {
                        o.bom = on.then_some(BomStructure::Reference);
                    }
                }
            }
        }
        if let Some(occ) = open_props {
            self.open_component_iproperties(occ);
        }
        if export {
            self.bom_export = Some(bom::to_csv(&rows, &w.extra));
            self.open_dialog(super::files::DialogPurpose::ExportBom);
        }
        if open {
            self.bom = Some(w);
        } else {
            self.history.break_coalescing();
        }
    }
}
