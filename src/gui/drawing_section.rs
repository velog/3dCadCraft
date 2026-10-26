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
        // A point snapped to view geometry (end points, centres).
        let snapped = pointer
            .and_then(|p| self.point_under(views, rect, p))
            .or(at);
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
                            // Horizontal / vertical within 4°.
                            let dx = q[0] - a[0];
                            let dy = q[1] - a[1];
                            let ang = dy.atan2(dx).to_degrees().rem_euclid(90.0);
                            if !(4.0..=86.0).contains(&ang) {
                                if dx.abs() > dy.abs() {
                                    q[1] = a[1];
                                } else {
                                    q[0] = a[0];
                                }
                            }
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
            super::drawing::draw_view_colored(painter, &self.drw.cam, rect, &shown, PREVIEW);
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
