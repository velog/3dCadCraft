//! Annotate › Table (Empty / CSV file) and Annotate › Revision: Revision
//! Table, Revision Tag, Revision Cloud (`docs/spec/4.7-drawing-commands.md`
//! §3.5). The Table dialog sets the source (Select View hands over to the
//! bend table of a flat pattern view), then the table follows the cursor by
//! its upper left corner (snapping onto the border corner or the title
//! block) and a click places it. Revision Tag without a revision table asks
//! for the table first (Inventor). Double-clicks open Edit Table, Edit
//! Revision Table, Edit Revision Tag and the cloud's options.

use std::path::Path;

use eframe::egui::{self, Color32, Rect, RichText};

use super::CadCraftApp;
use super::drawing::{DrwTool, ShownView};
use super::drawing_annot::paint_prims;
use super::valuefield::ExprValue;
use crate::drawing::AnnotationKind;
use crate::drawing::revision::{
    self, RevRow, RevScope, RevisionCloud, RevisionTable, RevisionTag, TagShape,
};
use crate::drawing::sheet as fmt;
use crate::drawing::table::{self, GeneralTable};

const PREVIEW: Color32 = Color32::from_rgb(60, 140, 90);
const HOVER: Color32 = Color32::from_rgb(230, 120, 40);

/// Source of a new general table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TableSource {
    Empty,
    File,
}

/// The Table dialog of a general table.
#[derive(Clone, Debug)]
pub(super) struct TableDialog {
    source: TableSource,
    columns: usize,
    rows: usize,
    path: String,
    headers: bool,
}

impl Default for TableDialog {
    fn default() -> Self {
        Self {
            source: TableSource::Empty,
            columns: table::DEFAULT_SIZE.0,
            rows: table::DEFAULT_SIZE.1,
            path: String::new(),
            headers: true,
        }
    }
}

/// The Revision Table dialog (settings), before the table is placed.
#[derive(Clone, Debug)]
pub(super) struct RevDialog {
    rt: RevisionTable,
    /// Started by Revision Tag: tags follow the table.
    then_tags: bool,
}

/// Revision Cloud options (kept while the command runs).
#[derive(Clone, Copy, Debug)]
pub(super) struct CloudOpts {
    pub arc: f64,
    pub to_view: bool,
}

impl Default for CloudOpts {
    fn default() -> Self {
        Self {
            arc: 8.0,
            to_view: true,
        }
    }
}

/// An edit window of one of these annotations.
#[derive(Clone, Debug)]
pub(super) enum RevEdit {
    Table {
        id: u32,
        t: GeneralTable,
        row: usize,
    },
    Revisions {
        id: u32,
        t: RevisionTable,
        row: usize,
    },
    Tag {
        id: u32,
        t: RevisionTag,
    },
    Cloud {
        id: u32,
        c: RevisionCloud,
    },
}

/// State of these commands in the drawing UI.
#[derive(Clone, Debug, Default)]
pub(super) struct RevUi {
    pub table_dialog: Option<TableDialog>,
    pub rev_dialog: Option<RevDialog>,
    pub cloud: CloudOpts,
    pub edit: Option<RevEdit>,
}

impl RevUi {
    pub(super) fn open(&self) -> bool {
        self.table_dialog.is_some() || self.rev_dialog.is_some() || self.edit.is_some()
    }

    pub(super) fn close(&mut self) {
        self.table_dialog = None;
        self.rev_dialog = None;
        self.edit = None;
    }
}

/// Today's date for a new revision row.
fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

impl CadCraftApp {
    /// Annotate › Table: a selected flat pattern view makes a bend table;
    /// otherwise the Table dialog (Empty / File).
    pub(super) fn open_table(&mut self) {
        let flat = self.drw.sel.is_some_and(|v| {
            self.drawing()
                .and_then(|d| Some(d.flat_of(d.sheet()?, v)))
                .unwrap_or(false)
        });
        if flat {
            self.open_bend_table();
            return;
        }
        self.open_general_table(TableSource::Empty);
    }

    /// The Table dialog of a general table.
    pub(super) fn open_general_table(&mut self, source: TableSource) {
        self.drw.tool = None;
        self.drw.dialog = None;
        self.drw.planned = None;
        self.drw.bend_table = None;
        self.bom_cancel();
        self.drw.rev.close();
        self.drw.rev.table_dialog = Some(TableDialog {
            source,
            ..Default::default()
        });
    }

    /// File › Table Source picked a CSV file.
    pub(super) fn table_source_picked(&mut self, path: &Path) {
        if let Some(d) = self.drw.rev.table_dialog.as_mut() {
            d.path = crate::document::library::without_verbatim(path.to_path_buf())
                .display()
                .to_string();
            d.source = TableSource::File;
        }
    }

    /// Annotate › Revision Table.
    pub(super) fn open_revision_table(&mut self, then_tags: bool) {
        self.drw.tool = None;
        self.drw.dialog = None;
        self.drw.planned = None;
        self.drw.bend_table = None;
        self.bom_cancel();
        self.drw.rev.close();
        if !then_tags && self.revision_table().is_some() {
            self.file_status = Some(Err(
                "Revision Table: the drawing has a revision table (double-click it to edit it)"
                    .into(),
            ));
            return;
        }
        self.drw.rev.rev_dialog = Some(RevDialog {
            rt: RevisionTable::default(),
            then_tags,
        });
    }

    /// Annotate › Revision Tag: without a revision table, its dialog first.
    pub(super) fn start_revision_tag(&mut self) {
        if self.revision_table().is_none() {
            self.open_revision_table(true);
            return;
        }
        self.drw.rev.close();
        self.drw.dialog = None;
        self.drw.sel = None;
        self.drw.sel_ann = None;
        self.drw.tool = Some(DrwTool::RevisionTag(None));
    }

    /// Annotate › Revision Cloud.
    pub(super) fn start_revision_cloud(&mut self) {
        self.drw.rev.close();
        self.drw.dialog = None;
        self.drw.sel = None;
        self.drw.sel_ann = None;
        self.drw.tool = Some(DrwTool::RevisionCloud(Vec::new()));
    }

    /// The revision table that counts here: the active sheet's, else a
    /// whole-drawing one on any sheet.
    pub(super) fn revision_table(&self) -> Option<&RevisionTable> {
        let d = self.drawing()?;
        fn of(s: &crate::drawing::Sheet) -> Option<&RevisionTable> {
            s.annotations.iter().find_map(|a| match &a.kind {
                AnnotationKind::RevisionTable(t) => Some(t),
                _ => None,
            })
        }
        if let Some(t) = d.sheet().and_then(of) {
            return Some(t);
        }
        d.sheets
            .iter()
            .filter_map(of)
            .find(|t| t.scope == RevScope::Drawing)
    }

    pub(super) fn rev_prompt(&self) -> Option<String> {
        if self.drw.rev.table_dialog.is_some() {
            return Some("Table: choose Empty (columns, rows) or a CSV file, then OK".into());
        }
        if self.drw.rev.rev_dialog.is_some() {
            return Some("Revision Table: set the scope and the revision index, then OK".into());
        }
        match &self.drw.rev.edit {
            Some(RevEdit::Table { .. }) => {
                return Some(
                    "Edit Table: type the cells, insert or remove rows and columns, then OK".into(),
                );
            }
            Some(RevEdit::Revisions { .. }) => {
                return Some("Edit Revision Table: Add Revision Row adds the next revision; type the cells, then OK".into());
            }
            Some(RevEdit::Tag { .. }) => {
                return Some("Edit Revision Tag: the value and the shape, then OK".into());
            }
            Some(RevEdit::Cloud { .. }) => {
                return Some("Revision Cloud: the arc length, then OK".into());
            }
            None => {}
        }
        Some(match &self.drw.tool {
            Some(DrwTool::GeneralTable(_)) => {
                "Table: click where the table goes (its upper left corner)".into()
            }
            Some(DrwTool::RevisionTable(..)) => {
                "Revision Table: click where the table goes (it snaps onto the border corner or the title block)".into()
            }
            Some(DrwTool::RevisionTag(None)) => "Revision Tag: click where the tag goes, or an edge first for a leader (right-click: Done)".into(),
            Some(DrwTool::RevisionTag(Some(_))) => "Revision Tag: click where the tag goes".into(),
            Some(DrwTool::RevisionCloud(p)) if p.len() < 3 => {
                "Revision Cloud: click the corners of the cloud".into()
            }
            Some(DrwTool::RevisionCloud(_)) => "Revision Cloud: click more corners; double-click, Enter or right-click closes the cloud".into(),
            _ => return None,
        })
    }

    /// A table's upper left corner near `q`, snapped so that its upper
    /// right corner sits on the border's, or so that it stands on the
    /// title block.
    fn snap_table(&self, q: [f64; 2], width: f64, height: f64) -> [f64; 2] {
        let Some(sheet) = self.drawing().and_then(|d| d.sheet()) else {
            return q;
        };
        let fr = fmt::frame(sheet).1;
        let mut targets = vec![[fr[0] - width, fr[1]]];
        if let Some((_, th)) = fmt::title_rect(sheet) {
            targets.push([th[0] - width, th[1] + height]);
        }
        let tol = (12.0 / self.drw.cam.zoom).max(4.0);
        targets
            .into_iter()
            .find(|p| (p[0] - q[0]).hypot(p[1] - q[1]) < tol)
            .unwrap_or_else(|| super::drawing::snap(q))
    }

    /// A table standing on the title block keeps standing on it when its
    /// rows change (it grows upwards): the new upper left corner of table
    /// `id` with `rows` data rows.
    fn keep_standing(&self, id: u32, at: [f64; 2], rows: usize, title: bool) -> [f64; 2] {
        let Some(sheet) = self.drawing().and_then(|d| d.sheet()) else {
            return at;
        };
        let Some((_, th)) = fmt::title_rect(sheet) else {
            return at;
        };
        let look = &self.drw.look;
        let old = sheet
            .annotations
            .iter()
            .find(|a| a.id == id)
            .and_then(|a| match &a.kind {
                AnnotationKind::Table(t) => Some((t.at, t.rows.len(), !t.title.is_empty())),
                AnnotationKind::RevisionTable(t) => Some((t.at, t.rows.len(), !t.title.is_empty())),
                _ => None,
            });
        let Some((old_at, old_rows, old_title)) = old else {
            return at;
        };
        let bottom = old_at[1] - table::height(old_rows, old_title, look);
        if (bottom - th[1]).abs() > 0.5 {
            return at;
        }
        [at[0], th[1] + table::height(rows, title, look)]
    }

    /// Placing a general or revision table.
    pub(super) fn rev_table_tool(
        &mut self,
        painter: &egui::Painter,
        rect: Rect,
        response: &egui::Response,
    ) {
        let cam = self.drw.cam;
        let Some(q) = response.hover_pos().map(|p| cam.to_sheet(rect, p)) else {
            return;
        };
        let look = self.drw.look.clone();
        match self.drw.tool.clone() {
            Some(DrwTool::GeneralTable(mut t)) => {
                let h = table::height(t.rows.len(), !t.title.is_empty(), &look);
                t.at = self.snap_table(q, t.width(), h);
                paint_prims(painter, &cam, rect, &table::prims(&t, &look), PREVIEW);
                if response.clicked() {
                    self.edit_drawing("Table", move |d| {
                        d.add_annotation(AnnotationKind::Table(t));
                    });
                    self.drw.tool = None;
                }
            }
            Some(DrwTool::RevisionTable(dlg, then_tags)) => {
                let mut rt = dlg.place(q, &today());
                let h = table::height(rt.rows.len(), !rt.title.is_empty(), &look);
                rt.at = self.snap_table(q, rt.width(), h);
                paint_prims(painter, &cam, rect, &revision::prims(&rt, &look), PREVIEW);
                if response.clicked() {
                    let placed = rt.clone();
                    self.edit_drawing("Revision Table", move |d| {
                        d.add_annotation(AnnotationKind::RevisionTable(placed));
                    });
                    self.write_revision_property(&rt);
                    self.drw.tool = then_tags.then_some(DrwTool::RevisionTag(None));
                }
            }
            _ => {}
        }
    }

    /// Revision Tag: an edge gives the leader's arrow, then the tag's place.
    pub(super) fn revision_tag_tool(
        &mut self,
        painter: &egui::Painter,
        rect: Rect,
        response: &egui::Response,
        views: &[ShownView],
    ) {
        let Some(DrwTool::RevisionTag(tip)) = self.drw.tool.clone() else {
            return;
        };
        let cam = self.drw.cam;
        let Some(p) = response.hover_pos() else {
            return;
        };
        let q = cam.to_sheet(rect, p);
        let rev = self
            .revision_table()
            .map(|t| t.active_rev().to_owned())
            .unwrap_or_default();
        let look = self.drw.look.clone();
        if tip.is_none() {
            let hit = self.geometry_under(views, rect, p);
            if let Some(g) = &hit {
                painter.circle_stroke(
                    cam.to_screen(rect, g.at),
                    5.0,
                    egui::Stroke::new(2.0, HOVER),
                );
            }
            if response.clicked() {
                match hit {
                    Some(g) => self.drw.tool = Some(DrwTool::RevisionTag(Some((g.view, g.at)))),
                    None => {
                        let view = self.drw.hover;
                        self.add_revision_tag(views, view, q, None, rev);
                    }
                }
                return;
            }
        }
        let preview = RevisionTag {
            view: None,
            at: q,
            tip: tip.map(|(_, t)| t),
            rev: rev.clone(),
            shape: TagShape::default(),
        };
        paint_prims(
            painter,
            &cam,
            rect,
            &revision::tag_prims(&preview, [0.0, 0.0], &look),
            PREVIEW,
        );
        if response.clicked()
            && let Some((view, t)) = tip
        {
            self.add_revision_tag(views, Some(view), q, Some(t), rev);
            self.drw.tool = Some(DrwTool::RevisionTag(None));
        }
    }

    fn add_revision_tag(
        &mut self,
        views: &[ShownView],
        view: Option<u32>,
        at: [f64; 2],
        tip: Option<[f64; 2]>,
        rev: String,
    ) {
        let origin = view
            .and_then(|id| views.iter().find(|v| v.id == id))
            .map(|v| v.position);
        let rel = |p: [f64; 2]| match origin {
            Some(o) => [p[0] - o[0], p[1] - o[1]],
            None => p,
        };
        let tag = RevisionTag {
            view: origin.and(view),
            at: rel(super::drawing::snap(at)),
            tip: tip.map(rel),
            rev,
            shape: TagShape::default(),
        };
        self.edit_drawing("Revision Tag", move |d| {
            d.add_annotation(AnnotationKind::RevisionTag(tag));
        });
    }

    /// Revision Cloud: clicks add corners; a double-click, Enter or the
    /// right button closes it.
    pub(super) fn revision_cloud_tool(
        &mut self,
        painter: &egui::Painter,
        rect: Rect,
        response: &egui::Response,
        views: &[ShownView],
    ) {
        let Some(DrwTool::RevisionCloud(mut pts)) = self.drw.tool.clone() else {
            return;
        };
        let cam = self.drw.cam;
        let q = response.hover_pos().map(|p| cam.to_sheet(rect, p));
        if response.clicked()
            && let Some(q) = q
            && pts
                .last()
                .is_none_or(|l| (l[0] - q[0]).hypot(l[1] - q[1]) > 0.5)
        {
            pts.push(q);
        }
        let enter = response.hovered() && painter.ctx().input(|i| i.key_pressed(egui::Key::Enter));
        if (response.double_clicked() || enter) && pts.len() >= 3 {
            self.drw.tool = Some(DrwTool::RevisionCloud(pts));
            self.finish_revision_cloud(views);
            return;
        }
        let mut shown = pts.clone();
        if let Some(q) = q {
            shown.push(q);
        }
        let c = RevisionCloud {
            view: None,
            pts: shown,
            arc: self.drw.rev.cloud.arc,
        };
        paint_prims(
            painter,
            &cam,
            rect,
            &revision::cloud_prims(&c, [0.0, 0.0], 0.35),
            PREVIEW,
        );
        self.drw.tool = Some(DrwTool::RevisionCloud(pts));
    }

    /// Closes the cloud being drawn (needs three corners).
    pub(super) fn finish_revision_cloud(&mut self, views: &[ShownView]) {
        let Some(DrwTool::RevisionCloud(pts)) = self.drw.tool.clone() else {
            return;
        };
        self.drw.tool = Some(DrwTool::RevisionCloud(Vec::new()));
        if pts.len() < 3 {
            return;
        }
        let opts = self.drw.rev.cloud;
        // Tied to the view holding the outline's middle.
        let mid = [
            pts.iter().map(|p| p[0]).sum::<f64>() / pts.len() as f64,
            pts.iter().map(|p| p[1]).sum::<f64>() / pts.len() as f64,
        ];
        let view = opts
            .to_view
            .then(|| {
                views.iter().find(|v| {
                    (v.lo[0]..=v.hi[0]).contains(&mid[0]) && (v.lo[1]..=v.hi[1]).contains(&mid[1])
                })
            })
            .flatten();
        let o = view.map_or([0.0, 0.0], |v| v.position);
        let c = RevisionCloud {
            view: view.map(|v| v.id),
            pts: pts.iter().map(|p| [p[0] - o[0], p[1] - o[1]]).collect(),
            arc: opts.arc,
        };
        self.edit_drawing("Revision Cloud", move |d| {
            d.add_annotation(AnnotationKind::RevisionCloud(c));
        });
    }

    /// Update property on revision number edit: the active revision goes to
    /// the drawing's Revision Number (Entire Drawing) or the sheet's
    /// revision (Active Sheet).
    fn write_revision_property(&mut self, rt: &RevisionTable) {
        if !rt.update_property {
            return;
        }
        let rev = rt.active_rev().to_owned();
        match rt.scope {
            RevScope::Sheet => {
                if let Some(s) = self.drawing_sheet_mut() {
                    s.revision = rev;
                }
            }
            RevScope::Drawing => {
                if let Some(d) = self.project.document_mut(self.part_id) {
                    d.properties.set("revision", &rev);
                }
            }
        }
    }

    fn drawing_sheet_mut(&mut self) -> Option<&mut crate::drawing::Sheet> {
        match self.project.document_mut(self.part_id).map(|d| &mut d.data) {
            Some(crate::document::DocumentData::Drawing(d)) => d.sheet_mut(),
            _ => None,
        }
    }

    /// Double-click on a general table, revision table, tag or cloud.
    pub(super) fn open_rev_edit(&mut self, id: u32) -> bool {
        let Some(kind) = self
            .drawing()
            .and_then(|d| d.sheet())
            .and_then(|s| s.annotations.iter().find(|a| a.id == id))
            .map(|a| a.kind.clone())
        else {
            return false;
        };
        self.drw.rev.edit = Some(match kind {
            AnnotationKind::Table(t) => RevEdit::Table { id, t, row: 0 },
            AnnotationKind::RevisionTable(t) => RevEdit::Revisions {
                id,
                row: t.active().unwrap_or(0),
                t,
            },
            AnnotationKind::RevisionTag(t) => RevEdit::Tag { id, t },
            AnnotationKind::RevisionCloud(c) => RevEdit::Cloud { id, c },
            _ => return false,
        });
        true
    }

    /// The Table, Revision Table, edit and cloud option windows.
    pub(super) fn rev_windows(&mut self, ctx: &egui::Context) {
        self.table_dialog_window(ctx);
        self.rev_dialog_window(ctx);
        self.rev_edit_window(ctx);
        if matches!(self.drw.tool, Some(DrwTool::RevisionCloud(_))) {
            let mut o = self.drw.rev.cloud;
            let win = egui::Window::new("Revision Cloud")
                .resizable(false)
                .collapsible(false)
                .default_width(240.0);
            self.layout
                .place("drawing_rev_cloud", ctx.content_rect(), false, win)
                .show(ctx, |ui| {
                    egui::Grid::new("rc_opts").num_columns(2).show(ui, |ui| {
                        ui.label("Arc Length");
                        ui.add(ExprValue::new(&mut o.arc).range(1.0..=100.0).suffix(" mm"));
                        ui.end_row();
                    });
                    ui.checkbox(&mut o.to_view, "Associate to View")
                        .on_hover_text("The cloud moves with the view holding its middle");
                });
            self.drw.rev.cloud = o;
        }
    }

    fn table_dialog_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.drw.rev.table_dialog.clone() else {
            return;
        };
        let (mut ok, mut cancel, mut open, mut view, mut browse) =
            (false, false, true, false, false);
        let win = egui::Window::new("Table")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .default_width(400.0);
        self.layout
            .place("drawing_general_table", ctx.content_rect(), false, win)
            .show(ctx, |ui| {
                ui.label(RichText::new("Source").strong());
                ui.horizontal(|ui| {
                    if ui.radio(false, "Select View").on_hover_text("A flat pattern view: its bend table").clicked() {
                        view = true;
                    }
                    ui.radio_value(&mut d.source, TableSource::Empty, "Empty");
                    ui.radio_value(&mut d.source, TableSource::File, "File (CSV)");
                });
                ui.add_space(4.0);
                match d.source {
                    TableSource::Empty => {
                        egui::Grid::new("gt_size").num_columns(2).show(ui, |ui| {
                            ui.label("Columns");
                            ui.add(ExprValue::new(&mut d.columns).range(1..=50));
                            ui.end_row();
                            ui.label("Data Rows");
                            ui.add(ExprValue::new(&mut d.rows).range(0..=500));
                            ui.end_row();
                        });
                    }
                    TableSource::File => {
                        ui.horizontal(|ui| {
                            ui.add_sized([280.0, 18.0], egui::TextEdit::singleline(&mut d.path).hint_text("file.csv"));
                            if ui.button("Browse...").clicked() {
                                browse = true;
                            }
                        });
                        ui.checkbox(&mut d.headers, "Use First Row for Column Headers");
                        ui.weak("Reading stops at the first blank row; the cells are copied (no link to the file)");
                        ui.add_enabled(false, egui::Checkbox::new(&mut false, "Excel files (.xlsx), Start Cell, Column Header Row"))
                            .on_disabled_hover_text("Spec 4.7 Table: Excel sources are planned");
                    }
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() || super::keys::panel_ok_key(ui, false) {
                        ok = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        if cancel || !open {
            self.drw.rev.table_dialog = None;
            return;
        }
        if view {
            self.drw.rev.table_dialog = None;
            self.open_bend_table();
            return;
        }
        self.drw.rev.table_dialog = Some(d.clone());
        if browse {
            self.open_dialog(super::files::DialogPurpose::TableSource);
            return;
        }
        if ok {
            let t = match d.source {
                TableSource::Empty => Ok(GeneralTable::empty([0.0, 0.0], d.columns, d.rows)),
                TableSource::File => std::fs::read_to_string(d.path.trim())
                    .map_err(|e| e.to_string())
                    .and_then(|text| GeneralTable::from_csv([0.0, 0.0], &text, d.headers))
                    .map(|mut t| {
                        t.source = Path::new(d.path.trim())
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        t
                    }),
            };
            match t {
                Ok(t) => {
                    self.drw.rev.table_dialog = None;
                    self.drw.tool = Some(DrwTool::GeneralTable(t));
                }
                Err(e) => self.file_status = Some(Err(format!("Table: {e}"))),
            }
        }
    }

    fn rev_dialog_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.drw.rev.rev_dialog.clone() else {
            return;
        };
        let (mut ok, mut cancel, mut open) = (false, false, true);
        let win = egui::Window::new("Revision Table")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .default_width(360.0);
        self.layout
            .place("drawing_revision_table", ctx.content_rect(), false, win)
            .show(ctx, |ui| {
                rev_settings(ui, &mut d.rt);
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() || super::keys::panel_ok_key(ui, false) {
                        ok = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        if cancel || !open {
            self.drw.rev.rev_dialog = None;
            return;
        }
        if ok {
            self.drw.rev.rev_dialog = None;
            self.drw.tool = Some(DrwTool::RevisionTable(d.rt, d.then_tags));
            return;
        }
        self.drw.rev.rev_dialog = Some(d);
    }

    fn rev_edit_window(&mut self, ctx: &egui::Context) {
        let Some(mut e) = self.drw.rev.edit.clone() else {
            return;
        };
        let (mut ok, mut cancel, mut open) = (false, false, true);
        let title = match &e {
            RevEdit::Table { .. } => "Edit Table",
            RevEdit::Revisions { .. } => "Edit Revision Table",
            RevEdit::Tag { .. } => "Edit Revision Tag",
            RevEdit::Cloud { .. } => "Revision Cloud",
        };
        // Tables get a large resizable grid; a tag or cloud a small box.
        let grid = matches!(e, RevEdit::Table { .. } | RevEdit::Revisions { .. });
        let key = if grid {
            "drawing_rev_edit"
        } else {
            "drawing_rev_small"
        };
        let win = egui::Window::new(title)
            .id(egui::Id::new(key))
            .open(&mut open)
            .resizable(grid)
            .collapsible(false);
        let win = if grid {
            win.default_size([800.0, 380.0])
        } else {
            win.default_width(320.0)
        };
        self.layout
            .place(key, ctx.content_rect(), false, win)
            .show(ctx, |ui| {
                match &mut e {
                    RevEdit::Table { t, row, .. } => general_editor(ui, t, row),
                    RevEdit::Revisions { t, row, .. } => revision_editor(ui, t, row),
                    RevEdit::Tag { t, .. } => {
                        egui::Grid::new("rtag").num_columns(2).show(ui, |ui| {
                            ui.label("Revision Tag Value");
                            ui.text_edit_singleline(&mut t.rev);
                            ui.end_row();
                            ui.label("Revision Tag Type");
                            egui::ComboBox::from_id_salt("rtag_shape")
                                .selected_text(t.shape.label())
                                .show_ui(ui, |ui| {
                                    for s in TagShape::ALL {
                                        ui.selectable_value(&mut t.shape, s, s.label());
                                    }
                                });
                            ui.end_row();
                        });
                        let mut leader = t.tip.is_some();
                        if ui
                            .add_enabled(leader, egui::Checkbox::new(&mut leader, "Leader"))
                            .changed()
                            && !leader
                        {
                            t.tip = None;
                        }
                    }
                    RevEdit::Cloud { c, .. } => {
                        egui::Grid::new("rcloud").num_columns(2).show(ui, |ui| {
                            ui.label("Arc Length");
                            ui.add(ExprValue::new(&mut c.arc).range(1.0..=100.0).suffix(" mm"));
                            ui.end_row();
                        });
                    }
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() || super::keys::panel_ok_key(ui, false) {
                        ok = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        if cancel || !open {
            self.drw.rev.edit = None;
            return;
        }
        if ok {
            self.drw.rev.edit = None;
            let (id, kind, label) = match e.clone() {
                RevEdit::Table { id, mut t, .. } => {
                    t.at = self.keep_standing(id, t.at, t.rows.len(), !t.title.is_empty());
                    (id, AnnotationKind::Table(t), "Edit Table")
                }
                RevEdit::Revisions { id, mut t, .. } => {
                    t.at = self.keep_standing(id, t.at, t.rows.len(), !t.title.is_empty());
                    (id, AnnotationKind::RevisionTable(t), "Edit Revision Table")
                }
                RevEdit::Tag { id, t } => (id, AnnotationKind::RevisionTag(t), "Edit Revision Tag"),
                RevEdit::Cloud { id, c } => {
                    (id, AnnotationKind::RevisionCloud(c), "Revision Cloud")
                }
            };
            self.edit_drawing(label, move |d| {
                if let Some(a) = d
                    .sheet_mut()
                    .and_then(|s| s.annotations.iter_mut().find(|a| a.id == id))
                {
                    a.kind = kind;
                }
            });
            if let RevEdit::Revisions { t, .. } = &e {
                self.write_revision_property(t);
            }
            return;
        }
        self.drw.rev.edit = Some(e);
    }
}

/// Table Scope, Revision Index, Delete Rule of a revision table.
fn rev_settings(ui: &mut egui::Ui, rt: &mut RevisionTable) {
    ui.label(RichText::new("Table Scope").strong());
    ui.horizontal(|ui| {
        ui.radio_value(&mut rt.scope, RevScope::Drawing, "Entire Drawing");
        ui.radio_value(&mut rt.scope, RevScope::Sheet, "Active Sheet");
    });
    ui.label(RichText::new("Revision Index").strong());
    ui.checkbox(&mut rt.auto_index, "Auto-Index");
    ui.add_enabled_ui(rt.auto_index, |ui| {
        ui.horizontal(|ui| {
            let before = rt.numeric;
            ui.radio_value(&mut rt.numeric, false, "Alpha")
                .on_hover_text("Skips I, O, Q, S, X and Z");
            ui.radio_value(&mut rt.numeric, true, "Numeric");
            if before != rt.numeric {
                rt.start = if rt.numeric { "1".into() } else { "A".into() };
            }
        });
        ui.horizontal(|ui| {
            ui.label("Start Value");
            ui.add_sized([60.0, 18.0], egui::TextEdit::singleline(&mut rt.start));
        });
    });
    ui.checkbox(&mut rt.update_property, "Update property on revision number edit")
        .on_hover_text("The active revision goes to the drawing's Revision Number (Entire Drawing) or the sheet's revision (Active Sheet)");
    ui.label(RichText::new("Delete Rule").strong());
    let mut auto = rt.keep.is_some();
    ui.horizontal(|ui| {
        ui.radio_value(&mut auto, false, "Manual");
        ui.radio_value(&mut auto, true, "Auto");
    });
    if auto {
        let mut n = rt.keep.unwrap_or(5) as usize;
        ui.horizontal(|ui| {
            ui.label("Rows to Keep");
            ui.add(ExprValue::new(&mut n).range(1..=100));
        });
        rt.keep = Some(n as u32);
    } else {
        rt.keep = None;
    }
}

/// Title row of an editor.
fn title_row(ui: &mut egui::Ui, title: &mut String, default: &str) {
    let mut show = !title.is_empty();
    ui.horizontal(|ui| {
        if ui.checkbox(&mut show, "Title").changed() {
            *title = if show { default.into() } else { String::new() };
        }
        ui.add_enabled(show, egui::TextEdit::singleline(title).desired_width(260.0));
    });
}

fn general_editor(ui: &mut egui::Ui, t: &mut GeneralTable, row: &mut usize) {
    title_row(ui, &mut t.title, "TABLE");
    if !t.source.is_empty() {
        ui.weak(format!("Read from {}", t.source));
    }
    ui.horizontal(|ui| {
        if ui
            .button("Insert Row")
            .on_hover_text("Above the selected row")
            .clicked()
        {
            t.insert_row(*row);
        }
        if ui.button("Add Row").clicked() {
            t.insert_row(t.rows.len());
            *row = t.rows.len() - 1;
        }
        if ui
            .add_enabled(!t.rows.is_empty(), egui::Button::new("Remove Row"))
            .clicked()
        {
            if *row < t.rows.len() {
                t.rows.remove(*row);
            }
            *row = (*row).min(t.rows.len().saturating_sub(1));
        }
        if ui.button("Add Column").clicked() {
            t.insert_column(t.columns.len());
        }
    });
    let mut drop = None;
    let n = t.columns.len();
    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .max_height(360.0)
        .show(ui, |ui| {
            egui::Grid::new("gt_edit").striped(true).show(ui, |ui| {
                ui.label("");
                for (k, c) in t.columns.iter_mut().enumerate() {
                    ui.vertical(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut c.title)
                                .desired_width(c.width as f32 * 3.0),
                        );
                        ui.horizontal(|ui| {
                            ui.add(
                                ExprValue::new(&mut c.width)
                                    .range(5.0..=200.0)
                                    .suffix(" mm"),
                            );
                            if ui
                                .add_enabled(n > 1, egui::Button::new("x"))
                                .on_hover_text("Remove the column")
                                .clicked()
                            {
                                drop = Some(k);
                            }
                        });
                    });
                }
                ui.end_row();
                for (r, cells) in t.rows.iter_mut().enumerate() {
                    if ui
                        .selectable_label(*row == r, format!("{}", r + 1))
                        .clicked()
                    {
                        *row = r;
                    }
                    for (k, c) in t.columns.iter().enumerate() {
                        if let Some(s) = cells.get_mut(k) {
                            ui.add(
                                egui::TextEdit::singleline(s).desired_width(c.width as f32 * 3.0),
                            );
                        }
                    }
                    ui.end_row();
                }
            });
        });
    if let Some(k) = drop {
        t.remove_column(k);
    }
}

fn revision_editor(ui: &mut egui::Ui, t: &mut RevisionTable, row: &mut usize) {
    egui::CollapsingHeader::new("Table Scope, Revision Index, Delete Rule")
        .default_open(false)
        .show(ui, |ui| rev_settings(ui, t));
    title_row(ui, &mut t.title, "REVISION HISTORY");
    ui.horizontal(|ui| {
        if ui
            .button("Add Revision Row")
            .on_hover_text(
                "The next revision with today's date, at the end; it becomes the active row",
            )
            .clicked()
        {
            t.add_revision(&today());
            *row = t.rows.len() - 1;
        }
        if ui
            .button("Insert Row")
            .on_hover_text("An empty row above the selected one, not a revision")
            .clicked()
        {
            let k = (*row).min(t.rows.len());
            t.rows.insert(
                k,
                RevRow {
                    inserted: true,
                    ..Default::default()
                },
            );
        }
        if ui
            .add_enabled(!t.rows.is_empty(), egui::Button::new("Remove Row"))
            .clicked()
        {
            if *row < t.rows.len() {
                t.rows.remove(*row);
            }
            *row = (*row).min(t.rows.len().saturating_sub(1));
        }
    });
    let active = t.active();
    let keys: Vec<(String, String, f64)> = t
        .columns
        .iter()
        .map(|c| (c.key.clone(), c.title.clone(), c.width))
        .collect();
    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .max_height(360.0)
        .show(ui, |ui| {
            egui::Grid::new("rt_edit").striped(true).show(ui, |ui| {
                ui.label("");
                for c in &mut t.columns {
                    ui.vertical(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut c.title)
                                .desired_width(c.width as f32 * 2.5),
                        );
                        ui.add(
                            ExprValue::new(&mut c.width)
                                .range(5.0..=200.0)
                                .suffix(" mm"),
                        );
                    });
                }
                ui.end_row();
                for (r, rr) in t.rows.iter_mut().enumerate() {
                    let label = if Some(r) == active {
                        format!("{} (active)", r + 1)
                    } else {
                        format!("{}", r + 1)
                    };
                    if ui.selectable_label(*row == r, label).clicked() {
                        *row = r;
                    }
                    for (key, _, w) in &keys {
                        if let Some(s) = rr.cell_mut(key) {
                            ui.add(egui::TextEdit::singleline(s).desired_width(*w as f32 * 2.5));
                        }
                    }
                    ui.end_row();
                }
            });
        });
}
