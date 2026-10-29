//! Break (spec 4.10): the command (the view, the dialog's options over the
//! sheet, two clicks for the break lines, preview), Edit Break, Delete
//! Break and dragging a break's lines on the sheet.

use eframe::egui::{self, Color32, Rect, Stroke};

use super::CadCraftApp;
use super::drawing::{DrwTool, ShownView};
use crate::drawing::annot::Placement;
use crate::drawing::brk::{self, BreakOrientation, BreakStyle, ViewBreak};

const PREVIEW: Color32 = Color32::from_rgb(230, 120, 20);
const PICKED: Color32 = Color32::from_rgb(20, 90, 200);
/// How near (px) the pointer must be to a break line to grab it.
const GRAB_PX: f32 = 6.0;

/// The Break command: the view, the options and the first break line.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct BreakDraft {
    /// The view to break.
    pub view: Option<u32>,
    /// The options (its stretch is set by the clicks).
    pub b: ViewBreak,
    /// The first break line: a true view-plane coordinate along the axis.
    pub first: Option<f64>,
}

impl Default for BreakDraft {
    fn default() -> Self {
        Self {
            view: None,
            b: ViewBreak::new(BreakOrientation::Horizontal, 0.0, 0.0),
            first: None,
        }
    }
}

impl BreakDraft {
    /// Started with a view selected.
    pub fn with_view(id: u32) -> Self {
        Self {
            view: Some(id),
            ..Default::default()
        }
    }

    /// The status-bar prompt.
    pub fn prompt(&self) -> &'static str {
        match (self.view, self.first) {
            (None, _) => "Break: click the view to break",
            (Some(_), None) => "Break: click the first break line in the view",
            (Some(_), Some(_)) => "Break: click the second break line (Esc: cancel)",
        }
    }
}

/// A break's lines being dragged: the view, the break, which line (0: the
/// first piece's end, 1: the second's, 2: both), the break when the drag
/// began and where it began (sheet mm).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct BreakGrip {
    view: u32,
    k: usize,
    which: usize,
    b: ViewBreak,
    from: [f64; 2],
}

/// Edit Break: the view, the break and its options being edited.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct BreakEdit {
    pub view: u32,
    pub k: usize,
    pub b: ViewBreak,
}

/// A view's placement (true view-plane points ↔ sheet).
pub(super) fn placement(v: &ShownView) -> Option<Placement> {
    let l = v.lines.as_deref()?;
    Some(Placement {
        position: v.position,
        scale: v.scale,
        center: l.center(),
        breaks: l.breaks,
    })
}

/// The options of a break (the command's dialog and Edit Break): Style,
/// Orientation (only for a new break), Display, Gap, Symbols, Propagate.
pub(super) fn break_options_ui(ui: &mut egui::Ui, b: &mut ViewBreak, new: bool) {
    ui.label(egui::RichText::new("Style").strong());
    ui.horizontal(|ui| {
        ui.radio_value(&mut b.style, BreakStyle::Rectangular, "Rectangular")
            .on_hover_text("Wavy break edges across the cut geometry");
        ui.radio_value(&mut b.style, BreakStyle::Structural, "Structural")
            .on_hover_text("Straight break lines with break symbols");
    });
    ui.label(egui::RichText::new("Orientation").strong());
    ui.add_enabled_ui(new, |ui| {
        ui.horizontal(|ui| {
            ui.radio_value(
                &mut b.orientation,
                BreakOrientation::Horizontal,
                "Horizontal",
            )
            .on_hover_text("Shorten the view left to right (vertical break lines)");
            ui.radio_value(&mut b.orientation, BreakOrientation::Vertical, "Vertical")
                .on_hover_text("Shorten the view top to bottom (horizontal break lines)");
            ui.add_enabled(false, egui::RadioButton::new(false, "Aligned"))
                .on_disabled_hover_text(
                    "Planned: break lines square to a picked edge (docs/spec/4.10-break-view.md)",
                );
        });
    })
    .response
    .on_disabled_hover_text(
        "The orientation cannot be changed: delete the break and place it again",
    );
    ui.label(egui::RichText::new("Display").strong());
    let mut d = i32::from(b.display);
    ui.horizontal(|ui| {
        ui.label("Min");
        let tip = match b.style {
            BreakStyle::Rectangular => "How many waves the break edges have",
            BreakStyle::Structural => "How high the break symbols are",
        };
        ui.add(
            egui::Slider::new(&mut d, 1..=10)
                .show_value(false)
                .clamping(egui::SliderClamping::Always),
        )
        .on_hover_text(tip);
        ui.label("Max");
    });
    b.display = d.clamp(1, 10) as u8;
    ui.horizontal(|ui| {
        ui.label("Gap");
        let mut g = b.gap;
        ui.add(super::valuefield::ExprValue::new(&mut g).suffix(" mm"))
            .on_hover_text("The distance between the pieces on the sheet");
        if g > 0.0 && g.is_finite() {
            b.gap = g;
        }
    });
    ui.add_enabled_ui(b.style == BreakStyle::Structural, |ui| {
        ui.horizontal(|ui| {
            ui.label("Symbols");
            for n in 1..=3u8 {
                ui.radio_value(&mut b.symbols, n, n.to_string());
            }
        });
    })
    .response
    .on_disabled_hover_text("Structural style only");
    ui.add_enabled(
        false,
        egui::Checkbox::new(&mut false, "Propagate to parent view"),
    )
    .on_disabled_hover_text(
        "Planned: the break applied to the parent view too (docs/spec/4.10-break-view.md)",
    );
}

/// A line across view `v` at the true coordinate `u` along `axis` (sheet
/// mm, from the view's frame).
fn across(v: &ShownView, pl: &Placement, axis: usize, u: f64) -> [[f64; 2]; 2] {
    let mut q = [0.0; 2];
    q[axis] = u;
    let s = pl.to_sheet(q)[axis];
    let mut a = v.lo;
    let mut b = v.hi;
    a[axis] = s;
    b[axis] = s;
    [a, b]
}

impl CadCraftApp {
    /// The running Break command.
    pub(super) fn break_tool(
        &mut self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        rect: Rect,
        response: &egui::Response,
        views: &[ShownView],
    ) {
        let Some(DrwTool::Break(mut d)) = self.drw.tool.clone() else {
            return;
        };
        let cam = self.drw.cam;
        let to = |q: [f64; 2]| cam.to_screen(rect, q);
        let Some(id) = d.view else {
            if response.clicked()
                && let Some(id) = self.drw.hover
            {
                d.view = Some(id);
                self.drw.sel = Some(id);
                self.drw.tool = Some(DrwTool::Break(d));
            }
            return;
        };
        let Some(pv) = views.iter().find(|v| v.id == id) else {
            return;
        };
        let Some(pl) = placement(pv) else {
            return;
        };
        // The dialog's options, over the sheet.
        egui::Area::new(egui::Id::new("drw_break_options"))
            .order(egui::Order::Foreground)
            .fixed_pos(rect.left_top() + egui::vec2(10.0, 10.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.label(egui::RichText::new("Break").strong());
                    break_options_ui(ui, &mut d.b, d.first.is_none());
                });
            });
        let axis = d.b.axis();
        let stroke = |c: Color32| Stroke::new(1.5, c);
        if let Some(u) = d.first {
            let [a, b] = across(pv, &pl, axis, u);
            painter.line_segment([to(a), to(b)], stroke(PICKED));
        }
        let at = response.hover_pos().map(|p| cam.to_sheet(rect, p));
        if let Some(q) = at {
            let u = pl.to_view(q)[axis];
            let [a, b] = across(pv, &pl, axis, u);
            painter.line_segment([to(a), to(b)], stroke(PREVIEW));
            if let Some(f) = d.first {
                // The stretch to be removed.
                let [c, _] = across(pv, &pl, axis, f);
                let r = Rect::from_two_pos(to(c), to(b));
                painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(230, 120, 20, 40));
            }
            if response.clicked() {
                match d.first {
                    None => d.first = Some(u),
                    Some(f) => {
                        let mut b = d.b.clone();
                        b.from = f.min(u);
                        b.to = f.max(u);
                        let extent = pv.lines.as_deref().map(|l| (l.min, l.max));
                        self.add_break(id, b, extent);
                        return;
                    }
                }
            }
        }
        self.drw.tool = Some(DrwTool::Break(d));
    }

    /// Adds break `b` to view `id` (one Undo step); a break removing
    /// nothing of the view or overlapping another is refused.
    fn add_break(&mut self, id: u32, b: ViewBreak, extent: Option<([f64; 2], [f64; 2])>) {
        let extent = extent.map(|(lo, hi)| (lo[b.axis()], hi[b.axis()]));
        let inside = extent.is_some_and(|(lo, hi)| b.to.min(hi) - b.from.max(lo) > 1e-6);
        let r = if inside {
            self.edit_drawing("Break", move |doc| doc.add_break(id, b))
        } else {
            Some(Err(brk::BreakError::Empty))
        };
        match r {
            Some(Ok(_)) => {
                self.drw.tool = None;
                self.drw.sel = Some(id);
                self.file_status = Some(Ok("Break: the view is broken".into()));
            }
            Some(Err(e)) => {
                if inside {
                    self.history.cancel();
                }
                self.file_status = Some(Err(format!("Break: {e}")));
                // The second line again.
                if let Some(DrwTool::Break(d)) = &mut self.drw.tool {
                    d.first = None;
                }
            }
            None => {}
        }
    }

    /// The break of a view whose lines are near sheet point `q`: the view,
    /// the break and which line (0, 1, or 2 in the gap).
    pub(super) fn break_near(
        &self,
        views: &[ShownView],
        q: [f64; 2],
    ) -> Option<(u32, usize, usize)> {
        let sheet = self.drawing()?.sheet()?;
        let near = f64::from(GRAB_PX) / self.drw.cam.zoom.max(1e-3);
        for v in views {
            let Some(dv) = sheet.view(v.id) else {
                continue;
            };
            if dv.breaks.is_empty()
                || q[0] < v.lo[0]
                || q[0] > v.hi[0]
                || q[1] < v.lo[1]
                || q[1] > v.hi[1]
            {
                continue;
            }
            let (Some(pl), Some(l)) = (placement(v), v.lines.as_deref()) else {
                continue;
            };
            for c in l.breaks.cuts() {
                let mut a = [0.0; 2];
                a[c.axis] = c.from;
                let mut b = [0.0; 2];
                b[c.axis] = c.to;
                let (sa, sb) = (pl.to_sheet(a)[c.axis], pl.to_sheet(b)[c.axis]);
                let s = q[c.axis];
                if (s - sa).abs() < near {
                    return Some((v.id, c.index, 0));
                }
                if (s - sb).abs() < near {
                    return Some((v.id, c.index, 1));
                }
                if s > sa && s < sb {
                    return Some((v.id, c.index, 2));
                }
            }
        }
        None
    }

    /// Idle: dragging a break line changes how much is removed, dragging
    /// in the gap moves the break; released, one Undo step. Returns
    /// whether it took the pointer.
    pub(super) fn break_grips(
        &mut self,
        painter: &egui::Painter,
        rect: Rect,
        response: &egui::Response,
        views: &[ShownView],
    ) -> bool {
        let cam = self.drw.cam;
        let at = response.hover_pos().map(|p| cam.to_sheet(rect, p));
        if let Some(g) = self.drw.break_drag.clone() {
            let Some(v) = views.iter().find(|v| v.id == g.view) else {
                self.drw.break_drag = None;
                return false;
            };
            if let Some(q) = at {
                let du = (q[g.b.axis()] - g.from[g.b.axis()]) / v.scale;
                let mut b = g.b.clone();
                match g.which {
                    0 => b.from += du,
                    1 => b.to += du,
                    _ => {
                        b.from += du;
                        b.to += du;
                    }
                }
                let (view, k) = (g.view, g.k);
                if b.to - b.from > 1e-3
                    && self
                        .drawing()
                        .and_then(|d| d.sheet()?.view(view))
                        .is_some_and(|dv| {
                            dv.breaks.get(k) != Some(&b)
                                && brk::check(&b, &dv.breaks, Some(k)).is_ok()
                        })
                {
                    let _ = self.edit_drawing("Edit Break", move |d| d.set_break(view, k, b));
                }
            }
            if response.drag_stopped() {
                self.drw.break_drag = None;
                self.history.break_coalescing();
            }
            return true;
        }
        let press = response
            .drag_started_by(egui::PointerButton::Primary)
            .then(|| response.ctx.input(|i| i.pointer.press_origin()))
            .flatten()
            .map(|p| cam.to_sheet(rect, p));
        let Some((view, k, which)) = press.or(at).and_then(|q| self.break_near(views, q)) else {
            return false;
        };
        // The grabbed line, highlighted.
        if let (Some(v), Some(b)) = (
            views.iter().find(|v| v.id == view),
            self.drawing()
                .and_then(|d| d.sheet()?.view(view)?.breaks.get(k).cloned()),
        ) && let Some(pl) = placement(v)
        {
            let axis = b.axis();
            let to = |q: [f64; 2]| cam.to_screen(rect, q);
            let lines: Vec<f64> = match which {
                0 => vec![b.from],
                1 => vec![b.to],
                _ => vec![b.from, b.to],
            };
            for u in lines {
                let [p, q] = across(v, &pl, axis, u);
                painter.line_segment([to(p), to(q)], Stroke::new(2.0, PREVIEW));
            }
            if which < 2 {
                response.ctx.set_cursor_icon(if axis == 0 {
                    egui::CursorIcon::ResizeHorizontal
                } else {
                    egui::CursorIcon::ResizeVertical
                });
            }
            if let Some(from) = press {
                self.drw.break_drag = Some(BreakGrip {
                    view,
                    k,
                    which,
                    b,
                    from,
                });
                self.drw.sel = Some(view);
                return true;
            }
        }
        // Only a grab of a line takes the pointer; the gap still selects
        // and drags the view.
        which < 2 && response.dragged()
    }

    /// The Edit Break window (from the break's right-click menu).
    pub(super) fn break_edit_ui(&mut self, ctx: &egui::Context) {
        let Some(mut e) = self.drw.break_edit.take() else {
            return;
        };
        let mut open = true;
        let mut ok = false;
        let mut cancel = false;
        egui::Window::new("Edit Break")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                break_options_ui(ui, &mut e.b, false);
                ui.separator();
                ui.horizontal(|ui| {
                    ok = ui.button("OK").clicked() || super::keys::panel_ok_key(ui, true);
                    cancel = ui.button("Cancel").clicked();
                });
            });
        if ok {
            let (view, k, b) = (e.view, e.k, e.b.clone());
            match self.edit_drawing("Edit Break", move |d| d.set_break(view, k, b)) {
                Some(Err(err)) => {
                    self.history.cancel();
                    self.file_status = Some(Err(format!("Edit Break: {err}")));
                    self.drw.break_edit = Some(e);
                }
                _ => self.history.break_coalescing(),
            }
            return;
        }
        if open && !cancel {
            self.drw.break_edit = Some(e);
        }
    }

    /// Opens Edit Break of break `k` of view `view`.
    pub(super) fn open_break_edit(&mut self, view: u32, k: usize) {
        let b = self
            .drawing()
            .and_then(|d| d.sheet()?.view(view)?.breaks.get(k).cloned());
        if let Some(b) = b {
            self.drw.break_edit = Some(BreakEdit { view, k, b });
        }
    }

    /// Delete Break: the view is whole there again (one Undo step).
    pub(super) fn delete_break(&mut self, view: u32, k: usize) {
        if self.edit_drawing("Delete Break", move |d| d.remove_break(view, k)) != Some(true) {
            self.history.cancel();
        }
        self.history.break_coalescing();
    }
}
