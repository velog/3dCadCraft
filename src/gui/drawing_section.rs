//! Section View, Detail View and Auxiliary View commands
//! (`docs/spec/4-drawings.md` §3.2, stage D2): pick the parent, draw the
//! section line / the detail circle / pick the edge, then place the view —
//! the preview is the real view (computed on the drawing's worker), aligned
//! with its parent where the view type requires it.

use eframe::egui::{self, Color32, Rect, Stroke};

use super::CadCraftApp;
use super::drawing::{DrwTool, ShownView};
use crate::drawing::annot::Placement;
use crate::drawing::{SCALES, ViewKind};

const PREVIEW: Color32 = Color32::from_rgb(60, 140, 90);
const PICKED: Color32 = Color32::from_rgb(40, 120, 230);

/// Section View in progress (sheet mm).
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct SectionDraft {
    parent: Option<u32>,
    a: Option<[f64; 2]>,
    b: Option<[f64; 2]>,
}

/// Detail View in progress (sheet mm).
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct DetailDraft {
    parent: Option<u32>,
    center: Option<[f64; 2]>,
    radius: Option<f64>,
}

/// Auxiliary View in progress: the parent and the edge's ends (parent's
/// view plane).
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct AuxDraft {
    edge: Option<(u32, [f64; 2], [f64; 2])>,
}

impl SectionDraft {
    /// Status-bar prompt.
    pub(super) fn prompt(&self) -> &'static str {
        match (self.parent, self.a, self.b) {
            (None, _, _) => "Section View: click the parent view",
            (_, None, _) => "Section View: click the first point of the section line",
            (_, _, None) => "Section View: click the second point (horizontal / vertical snap)",
            _ => "Section View: move to the side to look from and click to place the view",
        }
    }
}

impl DetailDraft {
    /// A draft whose parent is already chosen (the selected view).
    pub(super) fn with_parent(p: u32) -> Self {
        Self {
            parent: Some(p),
            ..Self::default()
        }
    }

    /// Status-bar prompt.
    pub(super) fn prompt(&self) -> &'static str {
        match (self.parent, self.center, self.radius) {
            (None, _, _) => "Detail View: click the parent view",
            (_, None, _) => "Detail View: click the centre of the detail",
            (_, _, None) => "Detail View: click to set the circle's size",
            _ => "Detail View: click where the enlarged view goes",
        }
    }
}

impl AuxDraft {
    /// Status-bar prompt.
    pub(super) fn prompt(&self) -> &'static str {
        match self.edge {
            None => "Auxiliary View: click a straight edge of a view",
            Some(_) => "Auxiliary View: move square to the edge and click to place the view",
        }
    }
}

/// A sheet point in a view's plane.
fn to_view(v: &ShownView, p: [f64; 2]) -> Option<[f64; 2]> {
    let l = v.lines.as_deref()?;
    let c = l.center();
    Some([
        c[0] + (p[0] - v.position[0]) / v.scale,
        c[1] + (p[1] - v.position[1]) / v.scale,
    ])
}

/// The default detail scale: the next preset at least twice the parent's.
fn detail_scale(parent: f64) -> f64 {
    SCALES
        .iter()
        .copied()
        .find(|s| *s >= parent * 2.0 - 1e-9)
        .unwrap_or(parent * 2.0)
}

impl CadCraftApp {
    /// The running Section / Detail / Auxiliary command.
    pub(super) fn child_view_tool(
        &mut self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        rect: Rect,
        response: &egui::Response,
        views: &[ShownView],
    ) {
        let Some(tool) = self.drw.tool.clone() else {
            return;
        };
        let cam = self.drw.cam;
        let pointer = response.hover_pos();
        let at = pointer.map(|p| cam.to_sheet(rect, p));
        let to = |q: [f64; 2]| cam.to_screen(rect, q);
        // A point snapped as a sketch point: end points, centres, the middle
        // of a straight edge.
        let snapped = pointer.map(|p| self.section_snap(views, rect, p));
        match tool {
            DrwTool::Section(mut d) => {
                let Some(parent) = d.parent else {
                    if response.clicked()
                        && let Some(id) = self.drw.hover
                    {
                        d.parent = Some(id);
                        self.drw.sel = Some(id);
                        self.drw.tool = Some(DrwTool::Section(d));
                    }
                    return;
                };
                let Some(pv) = views.iter().find(|v| v.id == parent) else {
                    return;
                };
                match (d.a, d.b) {
                    (None, _) => {
                        if let Some(q) = snapped {
                            painter.circle_filled(to(q), 4.0, PREVIEW);
                            if response.clicked() {
                                d.a = Some(q);
                                self.drw.tool = Some(DrwTool::Section(d));
                            }
                        }
                    }
                    (Some(a), None) => {
                        if let Some(mut q) = snapped {
                            hv(&mut q, a);
                            painter.line_segment([to(a), to(q)], Stroke::new(2.0, PREVIEW));
                            if response.clicked() && (q[0] - a[0]).hypot(q[1] - a[1]) > 1.0 {
                                d.b = Some(q);
                                self.drw.tool = Some(DrwTool::Section(d));
                            }
                        }
                    }
                    (Some(a), Some(b)) => {
                        painter.line_segment([to(a), to(b)], Stroke::new(2.0, PICKED));
                        let (Some(ra), Some(rb), Some(q)) = (to_view(pv, a), to_view(pv, b), at)
                        else {
                            return;
                        };
                        self.child_preview(ctx, painter, rect, response, "Section View", |doc| {
                            doc.add_section_view(parent, ra, rb, q)
                        });
                    }
                }
            }
            DrwTool::Detail(mut d) => {
                let Some(parent) = d.parent else {
                    if response.clicked()
                        && let Some(id) = self.drw.hover
                    {
                        d.parent = Some(id);
                        self.drw.sel = Some(id);
                        self.drw.tool = Some(DrwTool::Detail(d));
                    }
                    return;
                };
                let Some(pv) = views.iter().find(|v| v.id == parent) else {
                    return;
                };
                match (d.center, d.radius) {
                    (None, _) => {
                        if let Some(q) = snapped {
                            painter.circle_filled(to(q), 4.0, PREVIEW);
                            if response.clicked() {
                                d.center = Some(q);
                                self.drw.tool = Some(DrwTool::Detail(d));
                            }
                        }
                    }
                    (Some(c), None) => {
                        if let Some(q) = at {
                            let r = (q[0] - c[0]).hypot(q[1] - c[1]);
                            painter.circle_stroke(
                                to(c),
                                (r * cam.zoom) as f32,
                                Stroke::new(1.5, PREVIEW),
                            );
                            if response.clicked() && r > 0.5 {
                                d.radius = Some(r);
                                self.drw.tool = Some(DrwTool::Detail(d));
                            }
                        }
                    }
                    (Some(c), Some(r)) => {
                        painter.circle_stroke(
                            to(c),
                            (r * cam.zoom) as f32,
                            Stroke::new(1.5, PICKED),
                        );
                        let (Some(rc), Some(q)) = (to_view(pv, c), at) else {
                            return;
                        };
                        let model_r = r / pv.scale;
                        let scale = detail_scale(pv.scale);
                        self.child_preview(ctx, painter, rect, response, "Detail View", |doc| {
                            doc.add_detail_view(parent, rc, model_r, q, scale)
                        });
                    }
                }
            }
            DrwTool::Auxiliary(d) => match d.edge {
                None => {
                    let line = pointer.and_then(|p| self.straight_edge_under(views, rect, p));
                    if let Some((view, a, b)) = line
                        && let Some(v) = views.iter().find(|v| v.id == view)
                        && let Some(l) = v.lines.as_deref()
                    {
                        let pl = Placement {
                            position: v.position,
                            scale: v.scale,
                            center: l.center(),
                        };
                        painter.line_segment(
                            [to(pl.to_sheet(a)), to(pl.to_sheet(b))],
                            Stroke::new(3.0, PREVIEW),
                        );
                        if response.clicked() {
                            self.drw.sel = Some(view);
                            self.drw.tool = Some(DrwTool::Auxiliary(AuxDraft {
                                edge: Some((view, a, b)),
                            }));
                        }
                    }
                }
                Some((parent, a, b)) => {
                    let Some(q) = at else {
                        return;
                    };
                    self.child_preview(ctx, painter, rect, response, "Auxiliary View", |doc| {
                        doc.add_auxiliary_view(parent, a, b, q)
                    });
                }
            },
            _ => {}
        }
    }

    /// Preview of a child view made by `make` on a copy of the drawing; a
    /// click makes it for real (one Undo step) and ends the command.
    fn child_preview(
        &mut self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        rect: Rect,
        response: &egui::Response,
        label: &str,
        make: impl Fn(&mut crate::drawing::DrawingDocument) -> Option<u32>,
    ) {
        let Some(mut doc) = self.drawing().cloned() else {
            return;
        };
        let Some(id) = make(&mut doc) else {
            return;
        };
        let i = doc.active_sheet;
        let Some(sheet) = doc.sheets.get(i) else {
            return;
        };
        let Some(v) = sheet.view(id).cloned() else {
            return;
        };
        let Some(job) = doc.lines_job(sheet, id) else {
            return;
        };
        let (scale, style) = doc.effective(sheet, id).unwrap_or((v.scale, v.style));
        // A detail view needs its parent's lines first.
        let parents = match &job {
            crate::drawing::LinesJob::Detail { .. } => self.sheet_lines(Some(ctx), &doc, i),
            _ => Default::default(),
        };
        if let Some(lines) = self.job_lines(Some(ctx), &job, &parents) {
            let shown = ShownView {
                id,
                lo: v.position,
                hi: v.position,
                lines: Some(lines),
                position: v.position,
                scale,
                style,
            };
            super::drawing::draw_view_colored(
                painter,
                &self.drw.cam,
                rect,
                &shown,
                PREVIEW,
                self.drw.weights,
            );
            if let ViewKind::Detail { radius, .. } = v.kind {
                painter.circle_stroke(
                    self.drw.cam.to_screen(rect, v.position),
                    (radius * scale * self.drw.cam.zoom) as f32,
                    Stroke::new(1.0, PREVIEW),
                );
            }
        }
        if response.clicked() {
            let made = self.edit_drawing(label, |d| make(d));
            match made {
                Some(Some(id)) => {
                    self.drw.sel = Some(id);
                    self.drw.tool = None;
                    self.file_status = Some(Ok(format!("{label}: placed")));
                }
                _ => {
                    self.history.cancel();
                    self.file_status = Some(Err(format!("{label}: could not be placed here")));
                }
            }
        }
    }
}

/// A view-plane point of `v` on the sheet.
fn from_view(v: &ShownView, q: [f64; 2]) -> Option<[f64; 2]> {
    let c = v.lines.as_deref()?.center();
    Some([
        v.position[0] + (q[0] - c[0]) * v.scale,
        v.position[1] + (q[1] - c[1]) * v.scale,
    ])
}

/// The grip of a section line being dragged: its section view, which grip
/// (0: first end, 1: second end, 2: the whole line), the line's ends when
/// the drag began and where the pointer pressed (sheet mm).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct SectionGrip {
    view: u32,
    which: u8,
    ends: [[f64; 2]; 2],
    from: [f64; 2],
}

impl CadCraftApp {
    /// A point for a section line under the pointer, as sketch points snap:
    /// end points and centres of view geometry, then the middle of a
    /// straight edge; else the pointer.
    pub(super) fn section_snap(
        &self,
        views: &[ShownView],
        rect: Rect,
        pointer: egui::Pos2,
    ) -> [f64; 2] {
        let cam = self.drw.cam;
        if let Some(q) = self.point_under(views, rect, pointer) {
            return q;
        }
        let at = cam.to_sheet(rect, pointer);
        if let Some((vid, a, b)) = self.straight_edge_under(views, rect, pointer)
            && let Some(v) = views.iter().find(|v| v.id == vid)
            && let Some(m) = from_view(v, [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5])
            && (m[0] - at[0]).hypot(m[1] - at[1]) * cam.zoom < 10.0
        {
            return m;
        }
        at
    }

    /// Section lines on their parents: hovering a line shows its grips (its
    /// ends and its middle); dragging an end moves it with the sketch snaps
    /// (and horizontal / vertical to the other end), dragging the line moves
    /// it whole; the section is computed again when the drag ends (one Undo
    /// step). Returns whether it took the pointer.
    pub(super) fn section_line_grips(
        &mut self,
        painter: &egui::Painter,
        rect: Rect,
        response: &egui::Response,
        views: &[ShownView],
    ) -> bool {
        let cam = self.drw.cam;
        let Some(sheet) = self.drawing().and_then(|d| d.sheet()) else {
            return false;
        };
        // Every section line on the sheet (sheet mm).
        let lines: Vec<(u32, [f64; 2], [f64; 2])> = sheet
            .views
            .iter()
            .filter_map(|v| match v.kind {
                ViewKind::Section { parent, a, b, .. } => {
                    let pv = views.iter().find(|x| x.id == parent)?;
                    Some((v.id, from_view(pv, a)?, from_view(pv, b)?))
                }
                _ => None,
            })
            .collect();
        let to = |q: [f64; 2]| cam.to_screen(rect, q);
        let pointer = response.hover_pos();
        let at = pointer.map(|p| cam.to_sheet(rect, p));
        // The grip under the pointer — where the button went down when a
        // drag starts (egui reports it a few pixels later).
        let press = response
            .drag_started_by(egui::PointerButton::Primary)
            .then(|| response.ctx.input(|i| i.pointer.press_origin()))
            .flatten()
            .map(|p| cam.to_sheet(rect, p));
        let near = 7.0 / cam.zoom;
        let hit = press.or(at).and_then(|q| {
            let d = |p: [f64; 2]| (p[0] - q[0]).hypot(p[1] - q[1]);
            lines.iter().find_map(|&(id, a, b)| {
                if d(a) < near {
                    Some((id, 0u8, a, b))
                } else if d(b) < near {
                    Some((id, 1, a, b))
                } else {
                    let v = [b[0] - a[0], b[1] - a[1]];
                    let l2 = v[0] * v[0] + v[1] * v[1];
                    let t = (((q[0] - a[0]) * v[0] + (q[1] - a[1]) * v[1]) / l2.max(1e-12))
                        .clamp(0.0, 1.0);
                    let p = [a[0] + v[0] * t, a[1] + v[1] * t];
                    (d(p) < near * 0.7).then_some((id, 2, a, b))
                }
            })
        });
        if let Some(g) = self.drw.section_drag {
            // Dragging: the line as it will be.
            let (mut a, mut b) = (g.ends[0], g.ends[1]);
            if let (Some(p), Some(q)) = (pointer, at) {
                match g.which {
                    0 | 1 => {
                        let mut s = self.section_snap(views, rect, p);
                        let other = if g.which == 0 { b } else { a };
                        hv(&mut s, other);
                        if g.which == 0 {
                            a = s;
                        } else {
                            b = s;
                        }
                    }
                    _ => {
                        let d = [q[0] - g.from[0], q[1] - g.from[1]];
                        a = [a[0] + d[0], a[1] + d[1]];
                        b = [b[0] + d[0], b[1] + d[1]];
                    }
                }
            }
            painter.line_segment([to(a), to(b)], Stroke::new(2.0, PREVIEW));
            for p in [a, b] {
                painter.rect_filled(
                    Rect::from_center_size(to(p), egui::vec2(7.0, 7.0)),
                    0.0,
                    PREVIEW,
                );
            }
            if response.drag_stopped() {
                self.drw.section_drag = None;
                if (a[0] - b[0]).hypot(a[1] - b[1]) > 1.0 {
                    self.set_section_line(g.view, a, b, views);
                }
            }
            return true;
        }
        let Some((id, which, a, b)) = hit else {
            return false;
        };
        // Grips of the hovered line.
        let m = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
        for (k, p) in [a, b, m].into_iter().enumerate() {
            let on = k as u8 == which;
            let r = if on { 9.0 } else { 7.0 };
            painter.rect_stroke(
                Rect::from_center_size(to(p), egui::vec2(r, r)),
                0.0,
                Stroke::new(1.5, if on { PICKED } else { PREVIEW }),
                egui::StrokeKind::Middle,
            );
        }
        if let Some(q) = press {
            self.drw.section_drag = Some(SectionGrip {
                view: id,
                which,
                ends: [a, b],
                from: q,
            });
            return true;
        }
        if response.clicked() {
            self.drw.sel = Some(id);
            self.drw.sel_ann = None;
            return true;
        }
        false
    }

    /// Moves section view `id`'s line to sheet points `a`, `b` (on its
    /// parent): one Undo step; the section is computed again.
    fn set_section_line(&mut self, id: u32, a: [f64; 2], b: [f64; 2], views: &[ShownView]) {
        let Some(parent) = self
            .drawing()
            .and_then(|d| d.sheet()?.view(id))
            .and_then(|v| v.parent())
        else {
            return;
        };
        let Some(pv) = views.iter().find(|v| v.id == parent) else {
            return;
        };
        let (Some(ra), Some(rb)) = (to_view(pv, a), to_view(pv, b)) else {
            return;
        };
        self.edit_drawing("Edit Section Line", move |d| {
            if let Some(v) = d.sheet_mut().and_then(|s| s.view_mut(id))
                && let ViewKind::Section { a, b, .. } = &mut v.kind
            {
                *a = ra;
                *b = rb;
            }
        });
        self.history.break_coalescing();
    }
}

/// Horizontal / vertical to `other` within 4° (the sketch's inference).
fn hv(q: &mut [f64; 2], other: [f64; 2]) {
    let dx = q[0] - other[0];
    let dy = q[1] - other[1];
    let ang = dy.atan2(dx).to_degrees().rem_euclid(90.0);
    if !(4.0..=86.0).contains(&ang) {
        if dx.abs() > dy.abs() {
            q[1] = other[1];
        } else {
            q[0] = other[0];
        }
    }
}
