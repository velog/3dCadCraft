//! Annotate › Table on a flat pattern view: the bend table
//! (`docs/spec/4.8-sheet-metal-drawings.md` §3.4). The Table dialog (source
//! view, Column Chooser, Bend ID format, prefix, title, tags), then the table
//! follows the cursor and a click places it with its tags; a double-click on
//! a bend table opens the same dialog with Reverse Direction per bend.

use eframe::egui::{self, Rect, RichText};

use super::CadCraftApp;
use super::drawing::{DrwTool, ShownView};
use super::drawing_annot::paint_prims;
use super::valuefield::ExprValue;
use crate::drawing::AnnotationKind;
use crate::drawing::annot::Placement;
use crate::drawing::bend_table::{self, BendTable};

const PREVIEW: egui::Color32 = egui::Color32::from_rgb(60, 140, 90);

/// The Table dialog for a bend table.
#[derive(Clone, Debug)]
pub(super) struct BtDialog {
    /// The flat pattern view (none yet: the view selector is active).
    pub view: Option<u32>,
    pub bt: BendTable,
    /// OK pressed: the table follows the cursor.
    pub placing: bool,
    /// Edit of an existing table.
    pub edit: Option<u32>,
    /// The view's bend count (Reverse Direction rows).
    bends: usize,
}

impl CadCraftApp {
    /// Annotate › Table: a selected flat pattern view is the source.
    pub(super) fn open_bend_table(&mut self) {
        let view = self.drw.sel.filter(|&v| {
            self.drawing()
                .and_then(|d| Some(d.flat_of(d.sheet()?, v)))
                .unwrap_or(false)
        });
        self.drw.tool = None;
        self.drw.dialog = None;
        self.drw.planned = None;
        self.bom_cancel();
        self.drw.sel_ann = None;
        self.drw.bend_table = Some(BtDialog {
            view,
            bt: BendTable::new(view.unwrap_or(0), [0.0, 0.0]),
            placing: false,
            edit: None,
            bends: 0,
        });
        self.drw.tool = Some(DrwTool::BendTable);
    }

    /// Double-click on a bend table.
    pub(super) fn open_bend_table_edit(&mut self, id: u32) {
        let Some(bt) = self.drawing().and_then(|d| {
            d.sheet()?
                .annotations
                .iter()
                .find(|a| a.id == id)
                .and_then(|a| match &a.kind {
                    AnnotationKind::BendTable(b) => Some(b.clone()),
                    _ => None,
                })
        }) else {
            return;
        };
        self.drw.bend_table = Some(BtDialog {
            view: Some(bt.view),
            bt,
            placing: false,
            edit: Some(id),
            bends: 0,
        });
    }

    pub(super) fn bend_table_prompt(&self) -> Option<String> {
        let d = self.drw.bend_table.as_ref()?;
        Some(if d.placing {
            "Table: click where the bend table goes (its upper right corner)".into()
        } else if d.view.is_none() {
            "Table: select a flat pattern view: its bends make the bend table".into()
        } else {
            "Table: set the columns and the Bend ID format, then OK".into()
        })
    }

    /// The view picks and the placing preview.
    pub(super) fn bend_table_tool(
        &mut self,
        painter: &egui::Painter,
        rect: Rect,
        response: &egui::Response,
        views: &[ShownView],
    ) {
        let Some(dlg) = self.drw.bend_table.clone() else {
            self.drw.tool = None;
            return;
        };
        let cam = self.drw.cam;
        let at = response.hover_pos().map(|p| cam.to_sheet(rect, p));
        if !dlg.placing {
            if response.clicked()
                && let Some(id) = self.drw.hover
            {
                let flat = views
                    .iter()
                    .find(|v| v.id == id)
                    .and_then(|v| v.lines.as_ref())
                    .is_some_and(|l| !l.bends.is_empty());
                if flat {
                    if let Some(d) = self.drw.bend_table.as_mut() {
                        d.view = Some(id);
                        d.bt.view = id;
                    }
                } else {
                    self.file_status = Some(Err(
                        "Table: pick a flat pattern view (tables of other sources are planned)"
                            .into(),
                    ));
                }
            }
            return;
        }
        let Some(q) = at else {
            return;
        };
        let Some(v) = views.iter().find(|v| v.id == dlg.bt.view) else {
            return;
        };
        let Some(lines) = v.lines.as_deref() else {
            return;
        };
        let mut bt = dlg.bt.clone();
        bt.at = q;
        let pl = Placement {
            position: v.position,
            scale: v.scale,
            center: lines.center(),
            breaks: lines.breaks,
        };
        let prims = bend_table::prims(&bt, lines, pl, &self.drw.look);
        paint_prims(painter, &cam, rect, &prims, PREVIEW);
        if response.clicked() {
            self.edit_drawing("Bend Table", |d| {
                d.add_annotation(AnnotationKind::BendTable(bt))
            });
            self.drw.bend_table = None;
            self.drw.tool = None;
        }
    }

    /// The Table window (new table or Edit).
    pub(super) fn bend_table_window(&mut self, ctx: &egui::Context) {
        let Some(mut dlg) = self.drw.bend_table.clone() else {
            return;
        };
        if dlg.placing {
            return;
        }
        let view_name = dlg.view.and_then(|v| {
            self.drawing()
                .and_then(|d| d.sheet()?.view(v).map(|x| x.identifier.clone()))
        });
        let mut ok = false;
        let mut cancel = false;
        let mut open = true;
        let mut general = None;
        let title = if dlg.edit.is_some() {
            "Edit Table"
        } else {
            "Table"
        };
        let win = egui::Window::new(title)
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .default_width(420.0);
        self.layout
            .place("drawing_bend_table", ctx.content_rect(), false, win)
            .show(ctx, |ui| {
                ui.label(RichText::new("Source").strong());
                ui.horizontal(|ui| {
                    let mut src = 0;
                    ui.radio_value(&mut src, 0, "Select View");
                    // A new table may switch to the general table's sources.
                    let new = dlg.edit.is_none();
                    for (other, src) in [
                        ("Empty", super::drawing_revision::TableSource::Empty),
                        ("File (CSV)", super::drawing_revision::TableSource::File),
                    ] {
                        if ui
                            .add_enabled(new, egui::RadioButton::new(false, other))
                            .clicked()
                        {
                            general = Some(src);
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("View");
                    match &view_name {
                        Some(n) => ui.label(format!("{n} (flat pattern)")),
                        None => ui.weak("click a flat pattern view"),
                    };
                });
                ui.add_space(4.0);
                ui.label(RichText::new("Columns").strong());
                let mut drop = None;
                let mut swap = None;
                let n = dlg.bt.columns.len();
                egui::Grid::new("bt_cols").num_columns(4).show(ui, |ui| {
                    for (k, c) in dlg.bt.columns.iter_mut().enumerate() {
                        ui.add_sized([140.0, 18.0], egui::TextEdit::singleline(&mut c.title));
                        ui.add(
                            ExprValue::new(&mut c.width)
                                .range(5.0..=200.0)
                                .suffix(" mm"),
                        );
                        ui.horizontal(|ui| {
                            if ui.add_enabled(k > 0, egui::Button::new("Up")).clicked() {
                                swap = Some(k - 1);
                            }
                            if ui
                                .add_enabled(k + 1 < n, egui::Button::new("Down"))
                                .clicked()
                            {
                                swap = Some(k);
                            }
                        });
                        if ui.add_enabled(n > 1, egui::Button::new("Remove")).clicked() {
                            drop = Some(k);
                        }
                        ui.end_row();
                    }
                });
                if let Some(k) = swap {
                    dlg.bt.columns.swap(k, k + 1);
                }
                if let Some(k) = drop {
                    dlg.bt.columns.remove(k);
                }
                ui.horizontal(|ui| {
                    ui.label("Column Chooser:");
                    for (key, name) in bend_table::COLUMNS {
                        if dlg.bt.columns.iter().all(|c| c.key != key)
                            && ui.button(format!("+ {name}")).clicked()
                        {
                            dlg.bt.columns.push(bend_table::column(key));
                        }
                    }
                });
                ui.add_space(4.0);
                ui.label(RichText::new("BEND ID").strong());
                ui.horizontal(|ui| {
                    ui.radio_value(&mut dlg.bt.alpha, false, "Numeric");
                    ui.radio_value(&mut dlg.bt.alpha, true, "Alpha");
                    ui.label("Prefix");
                    ui.add(egui::TextEdit::singleline(&mut dlg.bt.prefix).desired_width(60.0));
                });
                ui.horizontal(|ui| {
                    ui.label("Title");
                    ui.add(egui::TextEdit::singleline(&mut dlg.bt.title).desired_width(200.0));
                });
                ui.checkbox(&mut dlg.bt.tags, "Bend tags in the view");
                if dlg.edit.is_some() && dlg.bends > 0 {
                    ui.add_space(4.0);
                    ui.label(RichText::new("Reverse Direction").strong());
                    ui.horizontal_wrapped(|ui| {
                        for k in 0..dlg.bends {
                            let mut on = dlg.bt.reversed.contains(&k);
                            if ui.checkbox(&mut on, dlg.bt.id(k)).changed() {
                                dlg.bt.reversed.retain(|&x| x != k);
                                if on {
                                    dlg.bt.reversed.push(k);
                                    dlg.bt.reversed.sort_unstable();
                                }
                            }
                        }
                    });
                }
                ui.separator();
                ui.horizontal(|ui| {
                    let ready = dlg.view.is_some();
                    if ui.add_enabled(ready, egui::Button::new("OK")).clicked()
                        || (ready && super::keys::panel_ok_key(ui, false))
                    {
                        ok = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        if cancel || !open {
            self.drw.bend_table = None;
            if self.drw.tool == Some(DrwTool::BendTable) {
                self.drw.tool = None;
            }
            return;
        }
        if let Some(src) = general {
            self.open_general_table(src);
            return;
        }
        if ok {
            match dlg.edit {
                Some(id) => {
                    let bt = dlg.bt.clone();
                    self.edit_drawing("Edit Table", |d| {
                        if let Some(a) = d
                            .sheet_mut()
                            .and_then(|s| s.annotations.iter_mut().find(|a| a.id == id))
                        {
                            a.kind = AnnotationKind::BendTable(bt);
                        }
                    });
                    self.drw.bend_table = None;
                }
                None => {
                    dlg.placing = true;
                    self.drw.bend_table = Some(dlg);
                }
            }
            return;
        }
        self.drw.bend_table = Some(dlg);
    }

    /// Keeps the dialog's bend count (Reverse Direction rows) from this
    /// frame's views.
    pub(super) fn bend_table_sync(&mut self, views: &[ShownView]) {
        if let Some(d) = &mut self.drw.bend_table
            && let Some(v) = d.view
            && let Some(l) = views
                .iter()
                .find(|x| x.id == v)
                .and_then(|x| x.lines.as_ref())
        {
            d.bends = l.bends.len();
        }
    }
}
