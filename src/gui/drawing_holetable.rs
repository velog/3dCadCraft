//! Hole Table - View / Selection / Selected Feature
//! (`docs/spec/4.7-drawing-commands.md`): click the view, the origin
//! indicator's point (a view that has one already keeps it), the holes
//! (Selection: any number, then Enter or right-click; Selected Feature: one
//! hole stands for its feature), then the table follows the cursor by its
//! upper left corner and a click places it. A double-click on a table opens
//! Edit Hole Table (title, tags).

use eframe::egui::{self, Color32, Rect, RichText};

use super::CadCraftApp;
use super::drawing::{DrwTool, ShownView};
use super::drawing_annot::paint_prims;
use crate::drawing::AnnotationKind;
use crate::drawing::annot::Placement;
use crate::drawing::hole_table::{self, HoleScope, HoleTable};

const PICKED: Color32 = Color32::from_rgb(40, 120, 230);
const PREVIEW: Color32 = Color32::from_rgb(60, 140, 90);
const HOVER: Color32 = Color32::from_rgb(230, 120, 40);

/// Which Hole Table command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HtMode {
    View,
    Selection,
    Feature,
}

/// A Hole Table command in progress.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct HtDraft {
    pub mode: HtMode,
    view: Option<u32>,
    /// The origin (view plane).
    origin: Option<[f64; 2]>,
    /// Picked holes (Selection) or the one hole of the feature.
    holes: Vec<String>,
    /// The table follows the cursor.
    placing: bool,
}

impl HtDraft {
    pub(super) fn new(mode: HtMode) -> HtDraft {
        HtDraft {
            mode,
            view: None,
            origin: None,
            holes: Vec::new(),
            placing: false,
        }
    }

    fn title(&self) -> &'static str {
        match self.mode {
            HtMode::View => "Hole Table - View",
            HtMode::Selection => "Hole Table - Selection",
            HtMode::Feature => "Hole Table - Selected Feature",
        }
    }

    pub(super) fn prompt(&self) -> String {
        let t = self.title();
        if self.view.is_none() {
            format!("{t}: select the view whose holes are listed")
        } else if self.origin.is_none() {
            format!("{t}: click a point of the view for the origin indicator")
        } else if self.placing {
            format!("{t}: click where the table goes (its upper left corner)")
        } else if self.mode == HtMode::Feature {
            format!("{t}: click a hole of the feature to list")
        } else {
            format!(
                "{t}: click holes ({} picked); Enter or right-click when done",
                self.holes.len()
            )
        }
    }

    /// Right-click / Enter: Selection goes on to the placement.
    pub(super) fn cont(&mut self) {
        if self.origin.is_some() && self.mode == HtMode::Selection && !self.holes.is_empty() {
            self.placing = true;
        }
    }

    fn scope(&self) -> HoleScope {
        match self.mode {
            HtMode::View => HoleScope::View,
            HtMode::Selection => HoleScope::Holes(self.holes.clone()),
            HtMode::Feature => HoleScope::Feature(
                self.holes
                    .first()
                    .map(|k| hole_table::feature_of(k).to_owned())
                    .unwrap_or_default(),
            ),
        }
    }
}

/// Edit Hole Table.
#[derive(Clone, Debug)]
pub(super) struct HtEdit {
    pub id: u32,
    pub ht: HoleTable,
}

/// A sheet point in a view's plane.
fn to_view(v: &ShownView, p: [f64; 2]) -> Option<[f64; 2]> {
    let l = v.lines.as_deref()?;
    let c = l.center();
    Some(l.breaks.lengthen([
        c[0] + (p[0] - v.position[0]) / v.scale,
        c[1] + (p[1] - v.position[1]) / v.scale,
    ]))
}

fn placement(v: &ShownView) -> Option<Placement> {
    Some(Placement {
        position: v.position,
        scale: v.scale,
        center: v.lines.as_deref()?.center(),
        breaks: v.lines.as_deref()?.breaks,
    })
}

impl CadCraftApp {
    pub(super) fn start_hole_table(&mut self, mode: HtMode) {
        self.drw.dialog = None;
        self.drw.sel_ann = None;
        let mut d = HtDraft::new(mode);
        d.view = self.drw.sel;
        self.drw.sel = None;
        self.drw.tool = Some(DrwTool::HoleTable(d));
    }

    /// The origin of an existing hole table of view `view` (one origin
    /// indicator per view, as in Inventor).
    fn view_origin(&self, view: u32) -> Option<[f64; 2]> {
        self.drawing()?
            .sheet()?
            .annotations
            .iter()
            .find_map(|a| match &a.kind {
                AnnotationKind::HoleTable(h) if h.view == view => Some(h.origin),
                _ => None,
            })
    }

    pub(super) fn hole_table_tool(
        &mut self,
        painter: &egui::Painter,
        rect: Rect,
        response: &egui::Response,
        views: &[ShownView],
    ) {
        let Some(DrwTool::HoleTable(mut d)) = self.drw.tool.clone() else {
            return;
        };
        let cam = self.drw.cam;
        let pointer = response.hover_pos();
        let q = pointer.map(|p| cam.to_sheet(rect, p));
        if d.view.is_none() {
            // This click picks the view only (not the origin as well).
            if response.clicked()
                && let Some(id) = self.drw.hover
            {
                d.view = Some(id);
            }
            self.drw.tool = Some(DrwTool::HoleTable(d));
            return;
        }
        if let Some(id) = d.view
            && d.origin.is_none()
        {
            d.origin = self.view_origin(id);
        }
        let Some(v) = d.view.and_then(|id| views.iter().find(|v| v.id == id)) else {
            self.drw.tool = Some(DrwTool::HoleTable(d));
            return;
        };
        let (Some(lines), Some(pl)) = (v.lines.clone(), placement(v)) else {
            self.drw.tool = Some(DrwTool::HoleTable(d));
            return;
        };
        let holes = hole_table::view_holes(&lines);
        if d.origin.is_none() {
            // The origin: a point of the view's geometry (snapped), else
            // where the click is.
            let snap = pointer.and_then(|p| self.point_under(views, rect, p));
            if let Some(s) = snap {
                painter.circle_stroke(cam.to_screen(rect, s), 6.0, egui::Stroke::new(1.5, HOVER));
            }
            if response.clicked()
                && let Some(at) = snap.or(q)
            {
                d.origin = to_view(v, at);
                if d.mode == HtMode::View {
                    d.placing = true;
                }
            }
            self.drw.tool = Some(DrwTool::HoleTable(d));
            return;
        }
        if let Some(o) = d.origin {
            paint_prims(
                painter,
                &cam,
                rect,
                &hole_table::origin_prims(pl.to_sheet(o)),
                PICKED,
            );
        }
        if d.mode == HtMode::View {
            d.placing = true;
        }
        if !d.placing {
            // Hole picks: the hole whose circle is nearest the pointer.
            let tol = 8.0 / cam.zoom;
            let hit = q.and_then(|q| {
                holes
                    .iter()
                    .map(|h| {
                        let c = pl.to_sheet(h.centre);
                        let r = h.radius * pl.scale;
                        (((q[0] - c[0]).hypot(q[1] - c[1]) - r).abs(), h)
                    })
                    .filter(|(dist, h)| *dist <= tol.max(h.radius * pl.scale))
                    .min_by(|a, b| a.0.total_cmp(&b.0))
                    .map(|(_, h)| h.clone())
            });
            for h in &holes {
                let on = d.holes.contains(&h.key)
                    || (d.mode == HtMode::Feature
                        && d.holes.first().is_some_and(|k| {
                            hole_table::feature_of(k) == hole_table::feature_of(&h.key)
                        }));
                let hov = hit.as_ref().is_some_and(|x| x.key == h.key);
                if on || hov {
                    painter.circle_stroke(
                        cam.to_screen(rect, pl.to_sheet(h.centre)),
                        (h.radius * pl.scale * cam.zoom) as f32 + 2.0,
                        egui::Stroke::new(2.0, if on { PICKED } else { HOVER }),
                    );
                }
            }
            if response.clicked()
                && let Some(h) = hit
            {
                match d.mode {
                    HtMode::Feature => {
                        d.holes = vec![h.key];
                        d.placing = true;
                    }
                    _ => {
                        if let Some(k) = d.holes.iter().position(|x| *x == h.key) {
                            d.holes.remove(k);
                        } else {
                            d.holes.push(h.key);
                        }
                    }
                }
            }
            if response.hovered() && painter.ctx().input(|i| i.key_pressed(egui::Key::Enter)) {
                d.cont();
            }
            self.drw.tool = Some(DrwTool::HoleTable(d));
            return;
        }
        // Placing: the table at the cursor.
        let (Some(q), Some(origin)) = (q, d.origin) else {
            self.drw.tool = Some(DrwTool::HoleTable(d));
            return;
        };
        let ht = HoleTable::new(v.id, origin, q, d.scope());
        if ht.tagged(&lines).is_empty() {
            self.file_status = Some(Err(format!("{}: the view shows no holes", d.title())));
            self.drw.tool = None;
            return;
        }
        let prims = hole_table::prims(&ht, &lines, pl, &self.drw.look);
        paint_prims(painter, &cam, rect, &prims, PREVIEW);
        if response.clicked() {
            let title = d.title();
            self.edit_drawing(title, move |doc| {
                doc.add_annotation(AnnotationKind::HoleTable(ht));
            });
            self.drw.tool = None;
            return;
        }
        self.drw.tool = Some(DrwTool::HoleTable(d));
    }

    /// Double-click on a hole table.
    pub(super) fn open_hole_table_edit(&mut self, id: u32) {
        let Some(ht) = self.drawing().and_then(|d| {
            d.sheet()?
                .annotations
                .iter()
                .find(|a| a.id == id)
                .and_then(|a| match &a.kind {
                    AnnotationKind::HoleTable(h) => Some(h.clone()),
                    _ => None,
                })
        }) else {
            return;
        };
        self.drw.hole_table_edit = Some(HtEdit { id, ht });
    }

    pub(super) fn hole_table_window(&mut self, ctx: &egui::Context) {
        let Some(mut e) = self.drw.hole_table_edit.clone() else {
            return;
        };
        let (mut ok, mut cancel, mut open) = (false, false, true);
        let win = egui::Window::new("Edit Hole Table")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .default_width(380.0);
        self.layout
            .place("drawing_hole_table", ctx.content_rect(), false, win)
            .show(ctx, |ui| {
                let mut show = !e.ht.title.is_empty();
                if ui.checkbox(&mut show, "Show Title").changed() {
                    e.ht.title = if show {
                        "HOLE TABLE".into()
                    } else {
                        String::new()
                    };
                }
                ui.add_enabled_ui(show, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Title");
                        ui.text_edit_singleline(&mut e.ht.title);
                    });
                });
                ui.checkbox(&mut e.ht.tags, "Hole tags in the view");
                ui.add_space(4.0);
                ui.label(RichText::new("Columns").strong());
                egui::Grid::new("ht_cols").num_columns(2).show(ui, |ui| {
                    for c in &mut e.ht.columns {
                        ui.add_sized([140.0, 18.0], egui::TextEdit::singleline(&mut c.title));
                        ui.add(
                            super::valuefield::ExprValue::new(&mut c.width)
                                .range(5.0..=200.0)
                                .suffix(" mm"),
                        );
                        ui.end_row();
                    }
                });
                ui.add_space(4.0);
                ui.weak(match &e.ht.scope {
                    HoleScope::View => "Lists every hole of the view".to_string(),
                    HoleScope::Holes(k) => format!("Lists {} picked holes", k.len()),
                    HoleScope::Feature(f) => format!("Lists the holes of {f}"),
                });
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
            self.drw.hole_table_edit = None;
            return;
        }
        if ok {
            let (id, ht) = (e.id, e.ht.clone());
            self.edit_drawing("Edit Hole Table", move |d| {
                if let Some(a) = d
                    .sheet_mut()
                    .and_then(|s| s.annotations.iter_mut().find(|a| a.id == id))
                {
                    a.kind = AnnotationKind::HoleTable(ht);
                }
            });
            self.drw.hole_table_edit = None;
            return;
        }
        self.drw.hole_table_edit = Some(e);
    }
}
