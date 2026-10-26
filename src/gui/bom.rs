//! Assemble › Bill of Materials (`docs/spec/3.6-bom.md`): the Model Data,
//! Structured and Parts Only views as tabs, Part Number Row Merge, sorting
//! by a column, optional custom iProperty columns, editing the components'
//! Part Number / Stock Number / Description / BOM Structure (components of
//! this file; a linked part file is read only), the Reference override per
//! occurrence, and Export to CSV.

use eframe::egui::{self, Color32, RichText};

use super::CadCraftApp;
use crate::assembly::bom::{self, BomRow, BomView, ComponentInfo};
use crate::document::iprops::BomStructure;
use crate::document::{DocRef, DocumentData};

/// The open window.
pub(super) struct BomWin {
    view: BomView,
    merge: bool,
    /// Sort by column (index) and direction.
    sort: Option<(usize, bool)>,
    /// Custom iProperty columns shown.
    extra: Vec<String>,
}

impl Default for BomWin {
    fn default() -> Self {
        Self {
            view: BomView::Structured,
            merge: true,
            sort: None,
            extra: Vec::new(),
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
        let mut rows = bom::rows(&asm, w.view, w.merge, &|r| self.bom_info(r));
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
            rows.sort_by(|a, b| {
                let o = key(a).to_lowercase().cmp(&key(b).to_lowercase());
                if asc { o } else { o.reverse() }
            });
        }
        let mut open = true;
        let mut export = false;
        let mut edits: Vec<(DocRef, &'static str, String)> = Vec::new();
        let mut structures: Vec<(DocRef, BomStructure)> = Vec::new();
        let mut overrides: Vec<(u32, bool)> = Vec::new();
        let mut open_props: Option<u32> = None;
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
                if !customs.is_empty() {
                    ui.menu_button("Columns", |ui| {
                        ui.label("Custom iProperties");
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
                    });
                }
            });
            if w.view == BomView::Structured {
                ui.weak("First level (sub-assemblies come later). Reference and Phantom components are not listed.");
            }
            ui.separator();
            let model = w.view == BomView::ModelData;
            egui::ScrollArea::both().auto_shrink([false, true]).show(ui, |ui| {
                egui::Grid::new("bom_grid").striped(true).num_columns(COLS.len() + w.extra.len() + 1).show(ui, |ui| {
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
                    for r in &rows {
                        let doc = r.components.first().copied();
                        // Components of this file can be edited here.
                        let local = r.components.iter().all(|c| c.library.is_none()) && !r.missing;
                        let tip = "A linked part file: open the part to change its iProperties";
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
                        } else {
                            ui.label(&r.item);
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
                            ui.colored_label(Color32::from_rgb(235, 80, 70), &r.part_number);
                        } else if local {
                            cell(ui, "part_number", &r.part_number);
                        } else {
                            // A linked part file: its iProperties window writes into it.
                            ui.horizontal(|ui| {
                                ui.label(&r.part_number);
                                if ui.small_button("...").on_hover_text("iProperties of the part file").clicked() {
                                    open_props = Some(r.occurrences[0]);
                                }
                            });
                        }
                        let mut s = r.structure;
                        ui.add_enabled_ui(local, |ui| {
                            egui::ComboBox::from_id_salt(("bom_s", r.occurrences[0]))
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
                        ui.label(r.qty.to_string());
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
        // Changes to the components' iProperties (one Undo step per edit).
        if !edits.is_empty() || !structures.is_empty() || !overrides.is_empty() {
            self.checkpoint("Bill of Materials");
            for (d, key, v) in edits {
                if let Some(doc) = self.project.document_mut(d.document) {
                    // An edited Part Number equal to the file name stays the default.
                    let v = if key == "part_number" && v == doc.name {
                        String::new()
                    } else {
                        v
                    };
                    doc.properties.set(key, &v);
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
