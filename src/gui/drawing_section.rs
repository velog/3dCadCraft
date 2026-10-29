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
/// A tracking guide: from a tracked point to the snapped pointer (sheet
/// mm).
type Guide = ([f64; 2], [f64; 2]);

/// Tracked points of a section line (the newest are kept).
const TRACKED: usize = 3;
/// Screen distance at which the pointer snaps to a tracked point's
/// horizontal / vertical (pixels).
const TRACK_PX: f64 = 8.0;

/// Draws the tracking guides (dotted, from the tracked point) and marks
/// the tracked points.
pub(super) fn paint_guides(
    painter: &egui::Painter,
    to: impl Fn([f64; 2]) -> egui::Pos2,
    track: &[[f64; 2]],
    guides: &[Guide],
) {
    for t in track {
        let c = to(*t);
        let s = Stroke::new(1.0, PREVIEW);
        painter.line_segment([c + egui::vec2(-4.0, 0.0), c + egui::vec2(4.0, 0.0)], s);
        painter.line_segment([c + egui::vec2(0.0, -4.0), c + egui::vec2(0.0, 4.0)], s);
    }
    for (a, b) in guides {
        painter.extend(egui::Shape::dotted_line(
            &[to(*a), to(*b)],
            PREVIEW,
            6.0,
            1.0,
        ));
    }
}

/// Section View in progress: the line's points so far (sheet mm) and
/// whether the line is finished.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct SectionDraft {
    parent: Option<u32>,
    pts: Vec<[f64; 2]>,
    done: bool,
    /// The line's sketch (Finish Sketch of the line's view sketch).
    sketch: Option<Box<crate::drawing::view_sketch::ViewSketch>>,
}

/// Detail View in progress (sheet mm).
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct DetailDraft {
    parent: Option<u32>,
    center: Option<[f64; 2]>,
    /// The fence's half sizes (a circle: its radius twice).
    size: Option<[f64; 2]>,
    /// The dialog's options (Fence Shape, Cutout Shape, boundary,
    /// connection line); a rectangle's size is set by the clicks.
    look: crate::drawing::DetailLook,
}

/// Auxiliary View in progress: the parent and the edge's ends (parent's
/// view plane).
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct AuxDraft {
    edge: Option<(u32, [f64; 2], [f64; 2])>,
}

impl SectionDraft {
    /// The view of a finished line is placed next: `pts` on the sheet.
    pub(super) fn placing(
        parent: u32,
        pts: Vec<[f64; 2]>,
        sketch: crate::drawing::view_sketch::ViewSketch,
    ) -> Self {
        Self {
            parent: Some(parent),
            pts,
            done: true,
            sketch: Some(Box::new(sketch)),
        }
    }

    /// Status-bar prompt.
    pub(super) fn prompt(&self) -> &'static str {
        match (self.parent, self.pts.len(), self.done) {
            (None, ..) => "Section View: click the parent view",
            (_, 0, _) => "Section View: click the first point of the section line",
            (_, 1, _) => {
                "Section View: click the next point (end points, centres, middles, horizontal / vertical snap)"
            }
            (_, _, false) => {
                "Section View: click more points for an offset section; right-click, Enter or double-click ends the line"
            }
            _ => "Section View: move to the side to look from and click to place the view",
        }
    }

    /// Ends the line (two points or more): the view is placed next.
    pub(super) fn finish(&mut self) -> bool {
        if self.parent.is_some() && self.pts.len() >= 2 && !self.done {
            self.done = true;
            return true;
        }
        false
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
        match (self.parent, self.center, self.size) {
            (None, _, _) => "Detail View: click the parent view",
            (_, None, _) => "Detail View: click the centre of the detail",
            (_, _, None) if self.rectangular() => "Detail View: click a corner of the rectangle",
            (_, _, None) => "Detail View: click to set the circle's size",
            _ => "Detail View: click where the enlarged view goes",
        }
    }

    /// Fence Shape Rectangular.
    fn rectangular(&self) -> bool {
        self.look.rect.is_some()
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
    Some(l.breaks.lengthen([
        c[0] + (p[0] - v.position[0]) / v.scale,
        c[1] + (p[1] - v.position[1]) / v.scale,
    ]))
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
        if !matches!(tool, DrwTool::Section(_)) {
            self.drw.section_track.clear();
        }
        match tool {
            DrwTool::Section(mut d) => {
                let Some(parent) = d.parent else {
                    // The parent picked: its sketch opens with the Line tool.
                    if response.clicked()
                        && let Some(id) = self.drw.hover
                    {
                        self.open_view_sketch(
                            id,
                            super::drawing_sketch::SketchFor::NewSection,
                            Default::default(),
                        );
                    }
                    return;
                };
                let Some(pv) = views.iter().find(|v| v.id == parent) else {
                    return;
                };
                if !d.done {
                    for w in d.pts.windows(2) {
                        painter.line_segment([to(w[0]), to(w[1])], Stroke::new(2.0, PICKED));
                    }
                    if let Some(p) = pointer {
                        let last = d.pts.last().copied();
                        let (q, on_geometry, guides) = self.section_point(views, rect, p, last);
                        paint_guides(painter, to, &self.drw.section_track, &guides);
                        if let Some(last) = last {
                            painter.line_segment([to(last), to(q)], Stroke::new(2.0, PREVIEW));
                        }
                        if on_geometry {
                            painter.circle_stroke(to(q), 6.0, Stroke::new(1.5, PICKED));
                        }
                        painter.circle_filled(to(q), 3.0, PREVIEW);
                        let enter = response.hovered()
                            && painter.ctx().input(|i| i.key_pressed(egui::Key::Enter));
                        if response.double_clicked() || enter {
                            d.finish();
                        } else if response.clicked()
                            && d.pts
                                .last()
                                .is_none_or(|l| (q[0] - l[0]).hypot(q[1] - l[1]) > 1.0)
                        {
                            d.pts.push(q);
                        }
                        self.drw.tool = Some(DrwTool::Section(d));
                    }
                    return;
                }
                for w in d.pts.windows(2) {
                    painter.line_segment([to(w[0]), to(w[1])], Stroke::new(2.0, PICKED));
                }
                let line: Option<Vec<[f64; 2]>> = d.pts.iter().map(|p| to_view(pv, *p)).collect();
                let (Some(line), Some(q)) = (line, at) else {
                    return;
                };
                // Points snapped to the parent's edges stay on them.
                let pl = pv.lines.clone();
                let sketch = d.sketch.clone();
                self.child_preview(ctx, painter, rect, response, "Section View", |doc| {
                    let id = doc.add_section_view_line(parent, &line, q)?;
                    match &sketch {
                        Some(sk) => doc
                            .set_section_sketch(id, (**sk).clone(), pl.as_deref())
                            .ok()?,
                        None => {
                            doc.set_section_line(id, &line, pl.as_deref());
                        }
                    }
                    Some(id)
                });
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
                // The dialog's options, over the sheet.
                let before = d.look;
                detail_options(ctx, rect, &mut d.look);
                if d.look != before {
                    self.drw.tool = Some(DrwTool::Detail(d.clone()));
                }
                let fence_at = |c: [f64; 2], h: [f64; 2], color: Color32| {
                    let f = crate::drawing::hlr::Fence {
                        center: c,
                        radius: h[0],
                        half: d.rectangular().then_some(h),
                        jagged: false,
                    };
                    let pts: Vec<egui::Pos2> = f.outline().into_iter().map(&to).collect();
                    painter.add(egui::Shape::line(pts, Stroke::new(1.5, color)));
                };
                match (d.center, d.size) {
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
                            let h = if d.rectangular() {
                                [(q[0] - c[0]).abs(), (q[1] - c[1]).abs()]
                            } else {
                                let r = (q[0] - c[0]).hypot(q[1] - c[1]);
                                [r, r]
                            };
                            fence_at(c, h, PREVIEW);
                            if response.clicked() && h[0].min(h[1]) > 0.5 {
                                d.size = Some(h);
                                self.drw.tool = Some(DrwTool::Detail(d));
                            }
                        }
                    }
                    (Some(c), Some(h)) => {
                        fence_at(c, h, PICKED);
                        let (Some(rc), Some(q)) = (to_view(pv, c), at) else {
                            return;
                        };
                        let model = [h[0] / pv.scale, h[1] / pv.scale];
                        let look = crate::drawing::DetailLook {
                            rect: d.rectangular().then_some(model),
                            ..d.look
                        };
                        let scale = detail_scale(pv.scale);
                        self.child_preview(ctx, painter, rect, response, "Detail View", |doc| {
                            doc.add_detail_view_with(parent, rc, model[0], look, q, scale)
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
                            breaks: l.breaks,
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
            if let Some(f) = v.kind.fence() {
                let cam = self.drw.cam;
                let pts: Vec<egui::Pos2> = f
                    .outline()
                    .iter()
                    .map(|q| {
                        cam.to_screen(
                            rect,
                            [
                                v.position[0] + (q[0] - f.center[0]) * scale,
                                v.position[1] + (q[1] - f.center[1]) * scale,
                            ],
                        )
                    })
                    .collect();
                painter.add(egui::Shape::line(pts, Stroke::new(1.0, PREVIEW)));
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
    let l = v.lines.as_deref()?;
    let c = l.center();
    let q = l.breaks.shorten(q);
    Some([
        v.position[0] + (q[0] - c[0]) * v.scale,
        v.position[1] + (q[1] - c[1]) * v.scale,
    ])
}

/// The grip of a section line being dragged: its section view, which grip
/// (a point index, or `points + k` for segment k), the line's points when
/// the drag began and where the pointer pressed (sheet mm).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct SectionGrip {
    view: u32,
    which: usize,
    pts: Vec<[f64; 2]>,
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
        self.section_snap_at(views, rect, pointer).0
    }

    /// [`Self::section_snap`] and whether the point is a point of the view
    /// geometry (an end point, a centre, a middle).
    fn section_snap_at(
        &self,
        views: &[ShownView],
        rect: Rect,
        pointer: egui::Pos2,
    ) -> ([f64; 2], bool) {
        let cam = self.drw.cam;
        if let Some(q) = self.point_under(views, rect, pointer) {
            return (q, true);
        }
        let at = cam.to_sheet(rect, pointer);
        if let Some((vid, a, b)) = self.straight_edge_under(views, rect, pointer)
            && let Some(v) = views.iter().find(|v| v.id == vid)
            && let Some(m) = from_view(v, [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5])
            && (m[0] - at[0]).hypot(m[1] - at[1]) * cam.zoom < 10.0
        {
            return (m, true);
        }
        (at, false)
    }

    /// A section line's next point under the pointer, as the sketch's Line
    /// tool places it: a point of the view geometry is taken as it is (and
    /// remembered for tracking); elsewhere the pointer snaps to the vertical
    /// and horizontal through the remembered points and through `last`
    /// (the line's previous point; within 4° of horizontal / vertical as
    /// well). Returns the point, whether it is a geometry point, and the
    /// guide lines to draw (from a tracked point to the result).
    pub(super) fn section_point(
        &mut self,
        views: &[ShownView],
        rect: Rect,
        pointer: egui::Pos2,
        last: Option<[f64; 2]>,
    ) -> ([f64; 2], bool, Vec<Guide>) {
        let (mut q, on_geometry) = self.section_snap_at(views, rect, pointer);
        let track = &mut self.drw.section_track;
        if on_geometry {
            if !track
                .iter()
                .any(|t| (t[0] - q[0]).hypot(t[1] - q[1]) < 1e-6)
            {
                track.push(q);
                if track.len() > TRACKED {
                    track.remove(0);
                }
            }
            return (q, true, Vec::new());
        }
        let tol = TRACK_PX / self.drw.cam.zoom;
        let sources: Vec<[f64; 2]> = track.iter().rev().copied().chain(last).collect();
        let mut guides = Vec::new();
        let mut axis_from: [Option<[f64; 2]>; 2] = [None, None];
        for i in 0..2 {
            let best = sources
                .iter()
                .filter(|s| (s[i] - q[i]).abs() < tol)
                .min_by(|a, b| (a[i] - q[i]).abs().total_cmp(&(b[i] - q[i]).abs()));
            if let Some(s) = best {
                q[i] = s[i];
                axis_from[i] = Some(*s);
            }
        }
        // The previous point: within 4° of horizontal / vertical.
        if let Some(l) = last
            && axis_from.iter().all(Option::is_none)
        {
            let before = q;
            hv(&mut q, l);
            for i in 0..2 {
                if q[i] != before[i] {
                    axis_from[i] = Some(l);
                }
            }
        }
        for s in axis_from.into_iter().flatten() {
            guides.push((s, q));
        }
        (q, false, guides)
    }

    /// Section lines on their parents: hovering a line shows its grips (its
    /// points and the middles of its segments); dragging a point moves it
    /// with the sketch snaps (and horizontal / vertical to its neighbour),
    /// dragging a segment moves that segment; the section is computed again
    /// when the drag ends (one Undo step). Returns whether it took the
    /// pointer.
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
        let lines: Vec<(u32, Vec<[f64; 2]>)> = sheet
            .views
            .iter()
            .filter_map(|v| {
                let ViewKind::Section { parent, .. } = v.kind else {
                    return None;
                };
                let pv = views.iter().find(|x| x.id == parent)?;
                let pts: Option<Vec<[f64; 2]>> = v
                    .kind
                    .section_line()?
                    .iter()
                    .map(|q| from_view(pv, *q))
                    .collect();
                Some((v.id, pts?))
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
            lines.iter().find_map(|(id, pts)| {
                if let Some(k) = pts.iter().position(|p| d(*p) < near) {
                    return Some((*id, k, pts.clone()));
                }
                pts.windows(2).enumerate().find_map(|(k, w)| {
                    let (a, b) = (w[0], w[1]);
                    let v = [b[0] - a[0], b[1] - a[1]];
                    let l2 = v[0] * v[0] + v[1] * v[1];
                    let t = (((q[0] - a[0]) * v[0] + (q[1] - a[1]) * v[1]) / l2.max(1e-12))
                        .clamp(0.0, 1.0);
                    let p = [a[0] + v[0] * t, a[1] + v[1] * t];
                    (d(p) < near * 0.7).then(|| (*id, pts.len() + k, pts.clone()))
                })
            })
        });
        if let Some(g) = self.drw.section_drag.clone() {
            // Dragging: the line as it will be.
            let mut pts = g.pts.clone();
            let n = pts.len();
            if let (Some(p), Some(q)) = (pointer, at) {
                if g.which < n {
                    let other = if g.which > 0 {
                        pts[g.which - 1]
                    } else {
                        pts[1]
                    };
                    let (s, on_geometry, guides) = self.section_point(views, rect, p, Some(other));
                    paint_guides(painter, to, &self.drw.section_track, &guides);
                    if on_geometry {
                        painter.circle_stroke(to(s), 6.0, Stroke::new(1.5, PICKED));
                    }
                    pts[g.which] = s;
                } else {
                    let k = g.which - n;
                    let d = [q[0] - g.from[0], q[1] - g.from[1]];
                    for i in [k, k + 1] {
                        pts[i] = [pts[i][0] + d[0], pts[i][1] + d[1]];
                    }
                }
            }
            for w in pts.windows(2) {
                painter.line_segment([to(w[0]), to(w[1])], Stroke::new(2.0, PREVIEW));
            }
            for p in &pts {
                painter.rect_filled(
                    Rect::from_center_size(to(*p), egui::vec2(7.0, 7.0)),
                    0.0,
                    PREVIEW,
                );
            }
            if response.drag_stopped() {
                self.drw.section_drag = None;
                self.drw.section_track.clear();
                let ok = pts
                    .windows(2)
                    .all(|w| (w[0][0] - w[1][0]).hypot(w[0][1] - w[1][1]) > 1.0);
                if ok {
                    self.set_section_line(g.view, &pts, views);
                }
            }
            return true;
        }
        let Some((id, which, pts)) = hit else {
            return false;
        };
        // Grips of the hovered line: its points and its segments' middles.
        let n = pts.len();
        let mids: Vec<[f64; 2]> = pts
            .windows(2)
            .map(|w| [(w[0][0] + w[1][0]) * 0.5, (w[0][1] + w[1][1]) * 0.5])
            .collect();
        for (k, p) in pts.iter().chain(&mids).enumerate() {
            let on = k == which;
            let r = if on { 9.0 } else { 7.0 };
            painter.rect_stroke(
                Rect::from_center_size(to(*p), egui::vec2(r, r)),
                0.0,
                Stroke::new(1.5, if on { PICKED } else { PREVIEW }),
                egui::StrokeKind::Middle,
            );
        }
        let _ = n;
        if response.double_clicked() {
            self.edit_section_line(id);
            return true;
        }
        if let Some(q) = press {
            self.drw.section_drag = Some(SectionGrip {
                view: id,
                which,
                pts,
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

    /// Moves section view `id`'s line to the sheet points `pts` (on its
    /// parent): one Undo step; the section is computed again.
    fn set_section_line(&mut self, id: u32, pts: &[[f64; 2]], views: &[ShownView]) {
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
        let Some(line) = pts
            .iter()
            .map(|p| to_view(pv, *p))
            .collect::<Option<Vec<_>>>()
        else {
            return;
        };
        if line.len() < 2 {
            return;
        }
        let pl = pv.lines.clone();
        self.edit_drawing("Edit Section Line", move |d| {
            d.set_section_line(id, &line, pl.as_deref());
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

/// The Detail View dialog's options (Inventor's Fence Shape and Cutout
/// Shape groups): Display Full Detail Boundary needs Smooth, Display
/// Connection Line the full boundary.
fn detail_options(ctx: &egui::Context, rect: Rect, look: &mut crate::drawing::DetailLook) {
    egui::Area::new(egui::Id::new("drw_detail_options"))
        .order(egui::Order::Foreground)
        .fixed_pos(rect.left_top() + egui::vec2(10.0, 10.0))
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                detail_look_ui(ui, look);
            });
        });
}

/// The rows of a detail view's look (the command's options and its
/// Properties).
pub(super) fn detail_look_ui(ui: &mut egui::Ui, look: &mut crate::drawing::DetailLook) {
    ui.label(egui::RichText::new("Fence Shape").strong());
    ui.horizontal(|ui| {
        let mut rect = look.rect.is_some();
        ui.radio_value(&mut rect, false, "Circular");
        ui.radio_value(&mut rect, true, "Rectangular");
        if rect != look.rect.is_some() {
            look.rect = rect.then_some([1.0, 1.0]);
        }
    });
    ui.label(egui::RichText::new("Cutout Shape").strong());
    ui.horizontal(|ui| {
        ui.radio_value(&mut look.jagged, true, "Jagged");
        ui.radio_value(&mut look.jagged, false, "Smooth");
    });
    ui.add_enabled(
        !look.jagged,
        egui::Checkbox::new(&mut look.full_boundary, "Display Full Detail Boundary"),
    )
    .on_disabled_hover_text("Needs the Smooth cutout");
    ui.add_enabled(
        !look.jagged && look.full_boundary,
        egui::Checkbox::new(&mut look.connection, "Display Connection Line"),
    )
    .on_disabled_hover_text("Needs the full detail boundary");
}

/// A detail fence being dragged on its parent: the detail view, the grip
/// (0: the centre, 1: the size), the fence when the drag began and where
/// the pointer pressed (sheet mm).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct FenceGrip {
    view: u32,
    which: usize,
    fence: crate::drawing::hlr::Fence,
    from: [f64; 2],
}

impl CadCraftApp {
    /// Detail fences on their parents: hovering one shows its grips (the
    /// centre, and the circle's right point / the rectangle's corner);
    /// dragging the centre moves the fence, the other grip sizes it; the
    /// detail follows when the drag ends (one Undo step). Returns whether
    /// it took the pointer.
    pub(super) fn detail_fence_grips(
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
        // Every fence on the sheet: (detail id, fence, parent placement).
        let fences: Vec<(u32, crate::drawing::hlr::Fence, Placement)> = sheet
            .views
            .iter()
            .filter_map(|v| {
                let f = v.kind.fence()?;
                let pv = views.iter().find(|x| Some(x.id) == v.parent())?;
                let pl = Placement {
                    position: pv.position,
                    scale: pv.scale,
                    center: pv.lines.as_deref()?.center(),
                    breaks: pv.lines.as_deref()?.breaks,
                };
                Some((v.id, f, pl))
            })
            .collect();
        let to = |q: [f64; 2]| cam.to_screen(rect, q);
        let grips = |f: &crate::drawing::hlr::Fence, pl: &Placement| {
            let h = f.half_size();
            let size = match f.half {
                Some(_) => [f.center[0] + h[0], f.center[1] + h[1]],
                None => [f.center[0] + f.radius, f.center[1]],
            };
            [pl.to_sheet(f.center), pl.to_sheet(size)]
        };
        let pointer = response.hover_pos();
        let at = pointer.map(|p| cam.to_sheet(rect, p));
        let press = response
            .drag_started_by(egui::PointerButton::Primary)
            .then(|| response.ctx.input(|i| i.pointer.press_origin()))
            .flatten()
            .map(|p| cam.to_sheet(rect, p));
        let near = 7.0 / cam.zoom;
        let d = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]);
        if let Some(g) = self.drw.fence_drag.clone() {
            let Some((_, _, pl)) = fences.iter().find(|(id, ..)| *id == g.view) else {
                self.drw.fence_drag = None;
                return false;
            };
            let mut f = g.fence;
            if let Some(q) = at {
                let v = [(q[0] - g.from[0]) / pl.scale, (q[1] - g.from[1]) / pl.scale];
                if g.which == 0 {
                    f.center = [f.center[0] + v[0], f.center[1] + v[1]];
                } else {
                    let p = pl.to_view(q);
                    let (dx, dy) = (p[0] - f.center[0], p[1] - f.center[1]);
                    match f.half {
                        Some(_) => f.half = Some([dx.abs().max(0.1), dy.abs().max(0.1)]),
                        None => f.radius = dx.hypot(dy).max(0.1),
                    }
                }
            }
            let pts: Vec<egui::Pos2> = f.outline().iter().map(|q| to(pl.to_sheet(*q))).collect();
            painter.add(egui::Shape::line(pts, Stroke::new(2.0, PREVIEW)));
            if response.drag_stopped() {
                self.drw.fence_drag = None;
                let id = g.view;
                let ok = self
                    .edit_drawing("Edit Detail Fence", |doc| {
                        doc.set_detail_fence(id, f.center, f.radius, f.half)
                    })
                    .unwrap_or(false);
                if !ok {
                    self.history.cancel();
                }
                self.history.break_coalescing();
            }
            return true;
        }
        let hit = press.or(at).and_then(|q| {
            fences.iter().find_map(|(id, f, pl)| {
                let g = grips(f, pl);
                if let Some(k) = (0..2).find(|k| d(g[*k], q) < near) {
                    return Some((*id, *f, *pl, Some(k)));
                }
                // On the outline: its grips show.
                let p = pl.to_view(q);
                (f.side(p).abs() * pl.scale < near).then_some((*id, *f, *pl, None))
            })
        });
        let Some((id, f, pl, which)) = hit else {
            return false;
        };
        for (k, p) in grips(&f, &pl).into_iter().enumerate() {
            let on = which == Some(k);
            let r = if on { 9.0 } else { 7.0 };
            painter.rect_stroke(
                Rect::from_center_size(to(p), egui::vec2(r, r)),
                0.0,
                Stroke::new(1.5, if on { PICKED } else { PREVIEW }),
                egui::StrokeKind::Middle,
            );
        }
        if let (Some(q), Some(k)) = (press, which) {
            self.drw.fence_drag = Some(FenceGrip {
                view: id,
                which: k,
                fence: f,
                from: q,
            });
            return true;
        }
        if response.clicked() && which.is_some() {
            self.drw.sel = Some(id);
            self.drw.sel_ann = None;
            return true;
        }
        false
    }
}
