//! Sketches of drawing views (`docs/spec/4-drawings.md`, stages D-S5 /
//! D-S6): the part sketch's tools working in a view's plane on the sheet.
//! The sketch is drawn and picked through [`overlay::Projector`] (view
//! plane → sheet → screen), so its geometry, constraint glyphs, dimensions,
//! previews and inference glyphs look as in a part sketch. Points of the
//! view's geometry (end points, centres, middles) snap and are tracked
//! (horizontal / vertical through them) as the section line's points were.
//!
//! A section line is such a sketch of its parent view: Section View opens
//! it with the Line tool; Finish Sketch takes its lines, end to end, as the
//! line; Edit Section Line opens it again.

use eframe::egui::{self, Color32, Pos2, Rect, Stroke};

use super::CadCraftApp;
use super::drawing::{DrwTool, SheetCam, ShownView};
use super::drawing_section::SectionDraft;
use super::overlay::{self, Projector};
use super::sketch_tools::{Align, ConstraintTool, Pick, SketchTools, Snap, Tool};
use crate::drawing::ViewKind;
use crate::drawing::annot::Placement;
use crate::drawing::view_sketch::ViewSketch;
use crate::kernel::Point3;
use crate::part::PartSketch;
use crate::sketch::{ConstraintId, Point2, PointId};

/// What finishing a view sketch does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SketchFor {
    /// The line of a new section view (the view is placed next).
    NewSection,
    /// The line of section view `id`.
    Section(u32),
    /// The profile of a new break out of the sketch's view.
    BreakOut,
}

/// A view sketch being edited.
pub(super) struct DrwSketch {
    /// The view whose plane it lies in (a section line: the parent).
    pub view: u32,
    /// What Finish Sketch does with it.
    pub owner: SketchFor,
    /// The sketch (its plane is the view plane), wrapped for the tools.
    pub ps: PartSketch,
    /// The part sketch's tools.
    pub tools: SketchTools,
    /// A dimension waiting for its value: the constraint and the value
    /// typed so far.
    pub dim: Option<(ConstraintId, f64)>,
    /// Project Geometry is running: clicks on the view's geometry copy it
    /// into the sketch as fixed reference geometry.
    pub project: bool,
    /// The point being dragged (Select tool).
    pub drag: Option<PointId>,
    /// Finish Sketch was asked for outside the sheet view (the ribbon):
    /// the next frame of the view finishes it.
    pub finish: bool,
}

/// The tools of a view sketch's toolbar, in Inventor's Sketch tab order.
const TOOLBAR: [Tool; 17] = [
    Tool::Select,
    Tool::Line,
    Tool::CircleCenter,
    Tool::ArcThreePoint,
    Tool::RectTwoPoint,
    Tool::Point,
    Tool::Fillet,
    Tool::Dimension,
    Tool::Constrain(ConstraintTool::Coincident),
    Tool::Constrain(ConstraintTool::Parallel),
    Tool::Constrain(ConstraintTool::Perpendicular),
    Tool::Constrain(ConstraintTool::Horizontal),
    Tool::Constrain(ConstraintTool::Vertical),
    Tool::Constrain(ConstraintTool::Equal),
    Tool::Constrain(ConstraintTool::Fix),
    Tool::Trim,
    Tool::Offset,
];

impl DrwSketch {
    /// A sketch of view `view` holding `sketch`, the Line tool running.
    pub(super) fn new(view: u32, owner: SketchFor, sketch: ViewSketch) -> Self {
        let mut tools = SketchTools::default();
        tools.set_tool(Tool::Line);
        Self {
            view,
            owner,
            ps: PartSketch::new("Section Line", sketch.0),
            tools,
            dim: None,
            project: false,
            drag: None,
            finish: false,
        }
    }

    /// Projects view geometry into the sketch (Project Geometry): a straight
    /// edge as a reference line between fixed points, a whole circle with a
    /// fixed centre and radius (both construction, so a section line never
    /// takes them), an end point or centre as a fixed point. Positions are
    /// the view plane's.
    /// An arc goes the way its drawn piece `piece` runs (its points as the
    /// view shows them); any other curve becomes fixed construction lines
    /// through that piece.
    pub(super) fn project_edge(
        &mut self,
        e: &crate::drawing::hlr::ViewEdge,
        piece: Option<&[[f64; 2]]>,
    ) -> Result<(), String> {
        let sk = &mut self.ps.sketch;
        let p = |q: [f64; 2]| Point2::new(q[0], q[1]);
        let construction = |sk: &mut crate::sketch::Sketch, refs: &mut Vec<_>, id| {
            // Construction: a reference, never part of a section line.
            let _ = sk.set_style(id, crate::sketch::Style::Construction);
            refs.push(id);
        };
        if let Some((c, r)) = e.circle {
            let whole = e.full_circle || (e.a[0] - e.b[0]).hypot(e.a[1] - e.b[1]) <= 1e-9;
            if whole {
                let center = sk.add_fixed_point(p(c));
                let circle = sk.add_circle_at(center, r).map_err(|x| x.to_string())?;
                sk.set_radius_fixed(circle, true)
                    .map_err(|x| x.to_string())?;
                construction(sk, &mut self.ps.reference_lines, circle);
                return Ok(());
            }
            // An arc: counter-clockwise from start to end through a point
            // of its drawn piece, else the other way round.
            let mid = piece
                .and_then(|pl| pl.get(pl.len() / 2).copied())
                .ok_or("Project Geometry: the arc is not drawn in this view")?;
            let ang = |q: [f64; 2]| (q[1] - c[1]).atan2(q[0] - c[0]);
            let tau = std::f64::consts::TAU;
            let ccw = |from: f64, to: f64| (to - from).rem_euclid(tau);
            let (a0, b0, m0) = (ang(e.a), ang(e.b), ang(mid));
            let (s, t) = if ccw(a0, m0) < ccw(a0, b0) {
                (e.a, e.b)
            } else {
                (e.b, e.a)
            };
            let center = sk.add_fixed_point(p(c));
            let start = sk.add_fixed_point(p(s));
            let end = sk.add_fixed_point(p(t));
            let arc = sk.add_arc(center, start, end).map_err(|x| x.to_string())?;
            construction(sk, &mut self.ps.reference_lines, arc);
            return Ok(());
        }
        if e.straight {
            let a = sk.add_fixed_point(p(e.a));
            let b = sk.add_fixed_point(p(e.b));
            let line = sk.add_line_between(a, b).map_err(|x| x.to_string())?;
            construction(sk, &mut self.ps.reference_lines, line);
            return Ok(());
        }
        // Another curve (an ellipse, a spline): fixed lines through at most
        // 24 of its drawn points, end to end.
        let pts = piece
            .filter(|pl| pl.len() >= 2)
            .ok_or("Project Geometry: the curve is not drawn in this view")?;
        let step = pts.len().div_ceil(24).max(1);
        let mut keep: Vec<[f64; 2]> = pts.iter().step_by(step).copied().collect();
        if let Some(l) = pts.last()
            && keep.last() != Some(l)
        {
            keep.push(*l);
        }
        let ids: Vec<_> = keep.iter().map(|q| sk.add_fixed_point(p(*q))).collect();
        for w in ids.windows(2) {
            let line = sk.add_line_between(w[0], w[1]).map_err(|x| x.to_string())?;
            construction(sk, &mut self.ps.reference_lines, line);
        }
        Ok(())
    }

    /// Projects a point of the view (fixed).
    pub(super) fn project_point(&mut self, q: Point2) {
        self.ps.sketch.add_fixed_point(q);
    }

    /// Delete: the selected curves (reference geometry too) and the points
    /// left alone by them.
    pub(super) fn delete_selected(&mut self) -> bool {
        let sel = std::mem::take(&mut self.tools.selected);
        let sk = &mut self.ps.sketch;
        let mut gone = false;
        for e in sel {
            let ends = sk.entity(e).map(|x| x.points()).unwrap_or_default();
            if sk.remove_entity(e).is_ok() {
                gone = true;
                self.ps.reference_lines.retain(|r| *r != e);
                for p in ends {
                    let used = sk.entities().any(|(_, x)| x.points().contains(&p));
                    if !used {
                        let _ = sk.remove_point(p);
                    }
                }
            }
        }
        for p in std::mem::take(&mut self.tools.selected_points) {
            gone |= sk.remove_point(p).is_ok();
        }
        if gone {
            let _ = sk.solve();
        }
        gone
    }

    /// Moves point `id` to `to`, the rest following (as a part sketch's
    /// drag); a move the constraints refuse leaves the sketch as it was.
    pub(super) fn drag_point(&mut self, id: PointId, to: Point2) {
        let sk = &mut self.ps.sketch;
        if sk.point(id).is_none_or(|q| q.fixed) {
            return;
        }
        let before = sk.clone();
        let _ = sk.move_point(id, to);
        let _ = sk.set_fixed(id, true);
        let held = solved(sk);
        let _ = sk.set_fixed(id, false);
        // Held exactly under the pointer, else pulled to the nearest place
        // the constraints allow; else the move is undone.
        if !(held || solved(sk)) {
            *sk = before;
        }
    }

    /// Status-bar prompt.
    pub(super) fn prompt(&self) -> String {
        let what = match self.owner {
            SketchFor::NewSection => "Section line",
            SketchFor::Section(_) => "Edit Section Line",
            SketchFor::BreakOut => "Break Out profile",
        };
        format!("{what}: {} (Enter: Finish Sketch)", self.tools.prompt())
    }
}

/// Solves `sk`: true when every constraint holds (a failed solve leaves
/// the sketch as it was, and is an `Ok` report too).
fn solved(sk: &mut crate::sketch::Sketch) -> bool {
    matches!(sk.solve(), Ok(r) if r.status == crate::sketch::SolveStatus::Solved)
}

/// A view's plane on the screen: view-plane mm → sheet → screen.
struct SheetProjector {
    cam: SheetCam,
    pl: Placement,
}

impl Projector for SheetProjector {
    fn project(&self, p: &Point3, rect: Rect) -> Option<Pos2> {
        Some(self.cam.to_screen(rect, self.pl.to_sheet([p.x, p.y])))
    }
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
    /// Break Out on view `view`: its profile sketch opens with the
    /// rectangle tool; Finish Sketch opens the dialog.
    pub(super) fn open_break_out_sketch(&mut self, view: u32) {
        self.open_view_sketch(view, SketchFor::BreakOut, ViewSketch::default());
        if let Some(s) = self.drw.sketch.as_mut() {
            s.tools.set_tool(Tool::RectTwoPoint);
        }
    }

    /// Opens a view sketch (the running drawing command ends).
    pub(super) fn open_view_sketch(&mut self, view: u32, owner: SketchFor, sketch: ViewSketch) {
        self.drw.tool = None;
        self.drw.sel = Some(view);
        self.drw.sel_ann = None;
        self.drw.section_track.clear();
        self.drw.sketch = Some(Box::new(DrwSketch::new(view, owner, sketch)));
        // Without the floating toolbar the ribbon's Sketch tab has the tools.
        if !self.sketch_toolbar {
            self.tab = super::ribbon::Tab::Sketch;
        }
    }

    /// Edit Section Line of section view `id`: its sketch (made from its
    /// points when it has none).
    pub(super) fn edit_section_line(&mut self, id: u32) {
        let Some(v) = self.drawing().and_then(|d| d.sheet()?.view(id)).cloned() else {
            return;
        };
        let ViewKind::Section { parent, sketch, .. } = &v.kind else {
            return;
        };
        let line = v.kind.section_line().unwrap_or_default();
        let sketch = sketch
            .as_deref()
            .cloned()
            .unwrap_or_else(|| ViewSketch::from_line(&line));
        self.open_view_sketch(*parent, SketchFor::Section(id), sketch);
    }

    /// Finish Sketch: the sketch's lines become the section line (a new
    /// section view is placed next). Returns false (and says why) when they
    /// are no line.
    pub(super) fn finish_view_sketch(&mut self, views: &[ShownView]) -> bool {
        let Some(s) = self.drw.sketch.take() else {
            return false;
        };
        let sketch = ViewSketch(s.ps.sketch.clone());
        if s.owner == SketchFor::BreakOut {
            let loops = super::drawing_breakout::profile_loops(&sketch.0);
            if loops.is_empty() {
                self.file_status = Some(Err("Break Out: the sketch has no closed profile".into()));
                self.drw.sketch = Some(s);
                return false;
            }
            self.drw.tool = Some(DrwTool::BreakOut(super::drawing_breakout::BoDraft::new(
                s.view, loops,
            )));
            self.drw.sel = Some(s.view);
            return true;
        }
        let line = match sketch.line() {
            Ok(l) if l.len() >= 2 => l,
            Ok(_) => {
                self.file_status = Some(Err("Section View: draw a line first".into()));
                self.drw.sketch = Some(s);
                return false;
            }
            Err(e) => {
                self.file_status = Some(Err(format!("Section View: {e}")));
                self.drw.sketch = Some(s);
                return false;
            }
        };
        match s.owner {
            SketchFor::NewSection => {
                let Some(pv) = views.iter().find(|v| v.id == s.view) else {
                    return false;
                };
                let Some(pl) = placement(pv) else {
                    return false;
                };
                let pts = line.iter().map(|q| pl.to_sheet(*q)).collect();
                self.drw.tool = Some(DrwTool::Section(SectionDraft::placing(s.view, pts, sketch)));
            }
            SketchFor::BreakOut => {}
            SketchFor::Section(id) => {
                let pl = views
                    .iter()
                    .find(|v| v.id == s.view)
                    .and_then(|v| v.lines.clone());
                let r = self.edit_drawing("Edit Section Line", |d| {
                    d.set_section_sketch(id, sketch, pl.as_deref())
                });
                if let Some(Err(e)) = r {
                    self.history.cancel();
                    self.file_status = Some(Err(format!("Edit Section Line: {e}")));
                }
                self.history.break_coalescing();
                self.drw.sel = Some(id);
            }
        }
        self.drw.section_track.clear();
        true
    }

    /// Keys of a view sketch (read before the drawing's own shortcuts):
    /// L Line, D Dimension, Enter Finish Sketch, Esc ends the tool's picks,
    /// then the tool, then the sketch. Returns whether a sketch is open.
    pub(super) fn view_sketch_keys(&mut self, ctx: &egui::Context) -> bool {
        let Some(s) = self.drw.sketch.as_mut() else {
            return false;
        };
        if s.dim.is_some() {
            return true; // the value box has the keys
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Delete)) {
            s.delete_selected();
            return true;
        }
        let (l, d, enter, esc) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::L),
                i.key_pressed(egui::Key::D),
                i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::Escape),
            )
        });
        if l {
            s.tools.set_tool(Tool::Line);
        } else if d {
            s.tools.set_tool(Tool::Dimension);
        } else if esc {
            if s.project {
                s.project = false;
            } else if s.tools.pick_count() > 0 || s.tools.tool != Tool::Select {
                s.tools.escape();
            } else {
                self.drw.sketch = None;
                self.drw.section_track.clear();
                self.file_status = Some(Ok("Section line: cancelled".into()));
            }
        } else if enter {
            let views = self.shown_views(ctx);
            self.finish_view_sketch(&views);
        }
        true
    }

    /// The open view sketch in the sheet view: drawing, picks, clicks, the
    /// right-click menu and the dimension value box.
    pub(super) fn view_sketch_ui(
        &mut self,
        painter: &egui::Painter,
        rect: Rect,
        response: &egui::Response,
        views: &[ShownView],
    ) {
        let Some(mut s) = self.drw.sketch.take() else {
            return;
        };
        let Some(pl) = views.iter().find(|v| v.id == s.view).and_then(placement) else {
            self.drw.sketch = Some(s);
            return;
        };
        let cam = self.drw.cam;
        let proj = SheetProjector { cam, pl };
        let to_view = |p: [f64; 2]| {
            let q = pl.to_view(p);
            Point2::new(q[0], q[1])
        };
        let pointer = response.hover_pos();
        let ctrl = response.ctx.input(|i| i.modifiers.command);
        // The pick: the sketch's own points, middles and curves first, then
        // the view's geometry (tracked), else the pointer.
        let mut guides = Vec::new();
        let pick = pointer.map(|p| {
            let sk = &s.ps.sketch;
            let drawing = s.tools.drawing();
            let point_px = if drawing {
                overlay::POINT_SNAP_PX
            } else {
                overlay::POINT_PICK_PX
            };
            let point = overlay::pick_any_point(&proj, rect, sk, p, point_px);
            let mid = overlay::pick_midpoint(&proj, rect, sk, p, overlay::POINT_SNAP_PX);
            let curve = overlay::pick_entity(&proj, rect, sk, p, overlay::CURVE_SNAP_PX);
            let plain = |pos: Point2, snap: Snap| Pick {
                pos,
                snap,
                infer: None,
                align: Align::default(),
            };
            let mm_per_px = 1.0 / (cam.zoom * pl.scale).max(1e-9);
            if let Some(id) = point
                && let Some(q) = sk.point(id)
            {
                s.tools
                    .infer(sk, plain(q.position, Snap::Point(id)), ctrl, mm_per_px)
            } else if let Some((e, m)) = mid.filter(|_| drawing) {
                s.tools
                    .infer(sk, plain(m, Snap::Midpoint(e)), ctrl, mm_per_px)
            } else if let Some((e, on)) = curve {
                s.tools
                    .infer(sk, plain(on, Snap::OnEntity(e)), ctrl, mm_per_px)
            } else {
                let (q, on_geometry, g) = self.section_point(views, rect, p, None);
                guides = g;
                let raw = plain(to_view(q), Snap::None);
                if on_geometry {
                    // On the view's geometry: exactly there.
                    raw
                } else {
                    // Tracked or free: the sketch's own inference too
                    // (horizontal / vertical to the previous point).
                    s.tools.infer(sk, raw, ctrl, mm_per_px)
                }
            }
        });

        // The sketch, the tool's preview, the snap glyph.
        let style = overlay::SketchStyle {
            dim_display: self.dim_display,
            color: Color32::from_rgb(20, 90, 200),
            fully_color: Color32::from_rgb(20, 120, 40),
            fully: Vec::new(),
            dof: None,
            hovered_constraint: None,
            selected_constraints: s.tools.selected_constraints.clone(),
            reference: overlay::REFERENCE,
            show_labels: true,
            highlighted: match pick.as_ref().map(|p| p.snap) {
                Some(Snap::Point(id)) => Some(id),
                _ => None,
            },
            selected: s.tools.selected.clone(),
            selected_points: s.tools.selected_points.clone(),
        };
        overlay::draw_sketch(painter, &proj, rect, &s.ps, &style);
        super::drawing_section::paint_guides(
            painter,
            |q| cam.to_screen(rect, q),
            &self.drw.section_track,
            &guides,
        );
        if let Some(p) = &pick {
            let lines = s.tools.preview(&s.ps, p);
            overlay::draw_preview(painter, &proj, rect, &s.ps.sketch, &lines);
            overlay::draw_snap(painter, &proj, rect, &s.ps.sketch, p);
        }

        // The toolbar (the sketch tab of the view sketch).
        let mut finish = std::mem::take(&mut s.finish);
        let mut cancel = false;
        if self.sketch_toolbar {
            self.view_sketch_toolbar(&response.ctx, rect, &mut s, &mut finish, &mut cancel);
        }

        // Select: a press on a point drags it.
        let press = response
            .drag_started_by(egui::PointerButton::Primary)
            .then(|| response.ctx.input(|i| i.pointer.press_origin()))
            .flatten();
        if s.tools.tool == Tool::Select
            && !s.project
            && let Some(p) = press
        {
            s.drag = overlay::pick_any_point(&proj, rect, &s.ps.sketch, p, overlay::POINT_SNAP_PX);
        }
        if let (Some(id), Some(p)) = (s.drag, pointer)
            && response.dragged()
        {
            s.drag_point(id, to_view(cam.to_sheet(rect, p)));
        }
        if response.drag_stopped() {
            s.drag = None;
        }

        // Project Geometry: a click on the view's geometry.
        if s.project
            && response.clicked()
            && let Some(p) = pointer
        {
            let pv = views.iter().find(|v| v.id == s.view);
            match self.geometry_under(views, rect, p) {
                Some(g) if g.view == s.view && g.is_point() => {
                    s.project_point(to_view(g.at));
                }
                Some(g) if g.view == s.view => {
                    let lines = pv.and_then(|v| v.lines.as_deref());
                    let edge = lines.and_then(|l| l.edge(&g.edge)).cloned();
                    let piece = lines.and_then(|l| l.edge_piece(&g.edge));
                    if let Some(e) = edge
                        && let Err(msg) = s.project_edge(&e, piece)
                    {
                        self.file_status = Some(Err(msg));
                    }
                }
                _ => {
                    self.file_status = Some(Err(
                        "Project Geometry: click an edge, a circle or a point of this view".into(),
                    ));
                }
            }
        }

        // Select: a click on a curve selects it (Shift / Ctrl add).
        if s.tools.tool == Tool::Select
            && !s.project
            && response.clicked()
            && let Some(p) = pointer
        {
            let add = response
                .ctx
                .input(|i| i.modifiers.shift || i.modifiers.command);
            if !add {
                s.tools.selected.clear();
                s.tools.selected_points.clear();
            }
            if let Some((e, _)) =
                overlay::pick_entity(&proj, rect, &s.ps.sketch, p, overlay::CURVE_SNAP_PX)
            {
                if let Some(k) = s.tools.selected.iter().position(|x| *x == e) {
                    s.tools.selected.remove(k);
                } else {
                    s.tools.selected.push(e);
                }
            }
        }

        // Clicks of the drawing tools.
        if s.dim.is_none()
            && !s.project
            && s.tools.tool != Tool::Select
            && response.clicked()
            && let Some(p) = pick
        {
            match s.tools.click(&mut s.ps, p) {
                Ok(Some(_)) => {
                    if let Err(e) = s.ps.sketch.solve() {
                        self.file_status = Some(Err(format!("Section line: {e}")));
                    }
                }
                Ok(None) => {}
                Err(msg) => self.file_status = Some(Err(msg)),
            }
            // A new dimension asks for its value.
            if let Some(e) = s.tools.dim_edit.take() {
                let v = s.ps.sketch.constraint(e.constraint).and_then(|c| c.value());
                s.dim = v.map(|v| (e.constraint, v));
            }
        }
        if response.double_clicked() {
            s.tools.escape();
            s.tools.set_tool(Tool::Line);
        }

        // The right-click menu (with the toolbar off, every tool of it).
        let mut toolbar = self.sketch_toolbar;
        response.context_menu(|ui| {
            if ui.button("Finish Sketch").clicked() {
                finish = true;
                ui.close();
            }
            if ui.button("Line (L)").clicked() {
                s.tools.set_tool(Tool::Line);
                ui.close();
            }
            if ui.button("Dimension (D)").clicked() {
                s.tools.set_tool(Tool::Dimension);
                ui.close();
            }
            if !toolbar {
                ui.menu_button("Tools", |ui| {
                    for t in TOOLBAR {
                        if ui
                            .selectable_label(!s.project && s.tools.tool == t, t.label())
                            .clicked()
                        {
                            s.project = false;
                            s.tools.escape();
                            s.tools.set_tool(t);
                            ui.close();
                        }
                    }
                });
                if ui.selectable_label(s.project, "Project Geometry").clicked() {
                    s.project = !s.project;
                    s.tools.escape();
                    ui.close();
                }
                if ui.button("Delete (Delete)").clicked() {
                    s.delete_selected();
                    ui.close();
                }
            }
            if ui.button("Restart chain").clicked() {
                s.tools.escape();
                s.tools.set_tool(Tool::Line);
                ui.close();
            }
            ui.separator();
            if ui.checkbox(&mut toolbar, "Floating Toolbar").clicked() {
                ui.close();
            }
            if ui.button("Cancel").clicked() {
                cancel = true;
                ui.close();
            }
        });
        self.sketch_toolbar = toolbar;

        // The value of a new dimension, over its text.
        if let Some((c, mut value)) = s.dim {
            let at =
                s.ps.sketch
                    .dimension_anchor(c)
                    .and_then(|a| proj.project(&Point3::new(a.x, a.y, 0.0), rect))
                    .unwrap_or(rect.center());
            let mut apply = false;
            let mut drop = false;
            egui::Area::new(egui::Id::new("drw_sketch_dim"))
                .order(egui::Order::Foreground)
                .fixed_pos(at - egui::vec2(60.0, 14.0))
                .show(&response.ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let r =
                                ui.add(super::valuefield::ExprValue::new(&mut value).suffix(" mm"));
                            if !r.has_focus() && !r.lost_focus() {
                                r.request_focus();
                            }
                            apply |=
                                r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                            apply |= ui
                                .small_button("✔")
                                .on_hover_text("Apply (Enter)")
                                .clicked();
                            drop |= ui
                                .small_button("x")
                                .on_hover_text("Keep the measured value (Esc)")
                                .clicked();
                        });
                    });
                });
            drop |= response.ctx.input(|i| i.key_pressed(egui::Key::Escape));
            s.dim = Some((c, value));
            if apply {
                let before = s.ps.sketch.clone();
                let ok = s.ps.sketch.set_dimension(c, value).is_ok() && solved(&mut s.ps.sketch);
                if ok {
                    s.dim = None;
                } else {
                    s.ps.sketch = before;
                    self.file_status = Some(Err(
                        "Dimension: that value cannot be reached with the other constraints".into(),
                    ));
                }
            } else if drop {
                s.dim = None;
            }
        }

        // A thin frame round the view being sketched in.
        if let Some(v) = views.iter().find(|v| v.id == s.view) {
            let r = Rect::from_two_pos(cam.to_screen(rect, v.lo), cam.to_screen(rect, v.hi));
            painter.rect_stroke(
                r.expand(4.0),
                0.0,
                Stroke::new(1.0, Color32::from_rgb(20, 90, 200)),
                egui::StrokeKind::Middle,
            );
        }

        self.drw.sketch = Some(s);
        if finish {
            self.finish_view_sketch(views);
        } else if cancel {
            self.drw.sketch = None;
            self.drw.section_track.clear();
        }
    }
}

impl CadCraftApp {
    /// The toolbar over the sheet while a view sketch is open: its tools
    /// (the part sketch's icons and names), Project Geometry, Delete,
    /// Finish Sketch and Cancel.
    fn view_sketch_toolbar(
        &mut self,
        ctx: &egui::Context,
        rect: Rect,
        s: &mut DrwSketch,
        finish: &mut bool,
        cancel: &mut bool,
    ) {
        let palette = super::icons::Palette::ENABLED;
        egui::Area::new(egui::Id::new("drw_sketch_toolbar"))
            .order(egui::Order::Foreground)
            .fixed_pos(rect.left_top() + egui::vec2(10.0, 10.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let icon = |ui: &mut egui::Ui,
                                    on: bool,
                                    i: super::icons::Icon,
                                    tip: &str| {
                            let (r, resp) = ui
                                .allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::click());
                            if on {
                                ui.painter()
                                    .rect_filled(r, 3.0, ui.visuals().selection.bg_fill);
                            } else if resp.hovered() {
                                ui.painter().rect_filled(
                                    r,
                                    3.0,
                                    ui.visuals().widgets.hovered.bg_fill,
                                );
                            }
                            super::icons::paint(ui.painter(), r.shrink(3.0), i, &palette);
                            resp.on_hover_text(tip).clicked()
                        };
                        for t in TOOLBAR {
                            let on = !s.project && s.tools.tool == t;
                            if icon(ui, on, super::ribbon::tool_icon(t), t.label()) {
                                s.project = false;
                                s.tools.escape();
                                s.tools.set_tool(t);
                            }
                        }
                        ui.separator();
                        let pg = super::icons::Icon::ProjectGeometry;
                        if icon(
                            ui,
                            s.project,
                            pg,
                            "Project Geometry: click edges, circles or points of the view",
                        ) {
                            s.project = !s.project;
                            s.tools.escape();
                        }
                        if icon(
                            ui,
                            false,
                            super::icons::Icon::DocDelete,
                            "Delete the selected curves (Delete)",
                        ) {
                            s.delete_selected();
                        }
                        ui.separator();
                        if ui.button("Finish Sketch").clicked() {
                            *finish = true;
                        }
                        if ui.button("Cancel").clicked() {
                            *cancel = true;
                        }
                    });
                });
            });
    }
}

/// Whether a view sketch takes `a` (the ribbon's Sketch tab in a drawing):
/// its toolbar's tools, Project Geometry, Construction, Delete, Finish
/// Sketch.
pub(super) fn view_sketch_takes(a: super::Action) -> bool {
    use super::Action;
    match a {
        Action::Tool(t) => TOOLBAR.contains(&t),
        Action::ProjectGeometry
        | Action::Format(crate::sketch::Style::Construction)
        | Action::App(super::commands::AppCmd::Delete)
        | Action::FinishSketch => true,
        _ => false,
    }
}

impl CadCraftApp {
    /// A command of the ribbon's Sketch tab (or its alias) while a view
    /// sketch is open: the toolbar's tools, Project Geometry, Construction,
    /// Delete and Finish Sketch drive the view sketch; the other sketch
    /// commands say they are not there yet. False: not a sketch command
    /// (the drawing's own and the application's commands run as usual).
    pub(super) fn view_sketch_action(&mut self, action: super::Action) -> bool {
        use super::Action;
        let Some(s) = self.drw.sketch.as_mut() else {
            return false;
        };
        match action {
            _ if !view_sketch_takes(action) => {
                let sketch_only = super::commands::by_action(action).is_some_and(|c| {
                    c.valid_in(super::commands::Env::Sketch)
                        && !c.valid_in(super::commands::Env::Drawing)
                });
                if !sketch_only {
                    return false;
                }
                let name = super::commands::by_action(action).map_or("This command", |c| c.name);
                self.file_status = Some(Err(format!(
                    "{name}: not available in a drawing sketch yet"
                )));
            }
            Action::Tool(t) => {
                s.project = false;
                s.tools.escape();
                s.tools.set_tool(t);
            }
            Action::ProjectGeometry => {
                s.project = !s.project;
                s.tools.escape();
            }
            Action::Format(crate::sketch::Style::Construction) => {
                s.tools.construction = !s.tools.construction;
            }
            Action::App(super::commands::AppCmd::Delete) => {
                s.delete_selected();
            }
            Action::FinishSketch => s.finish = true,
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f64, y: f64) -> Pick {
        Pick {
            pos: Point2::new(x, y),
            snap: Snap::None,
            infer: None,
            align: Align::default(),
        }
    }

    /// Project Geometry of an arc follows its drawn piece: the quarter from
    /// (10, 0) to (0, 10) drawn through 45° is that quarter, drawn through
    /// 225° the other three; a curve that is no circle becomes fixed
    /// construction lines through its points.
    #[test]
    fn projected_arcs_keep_their_side() {
        use crate::drawing::hlr::ViewEdge;
        use crate::sketch::Entity;
        let edge = |circle, straight| ViewEdge {
            name: Some("Box1:E2".into()),
            a: [10.0, 0.0],
            b: [0.0, 10.0],
            circle,
            full_circle: false,
            straight,
            visible: true,
            note: None,
            hole: None,
        };
        let arc_ends = |mid: [f64; 2]| {
            let mut s = DrwSketch::new(1, SketchFor::NewSection, ViewSketch::default());
            let arc = edge(Some(([0.0, 0.0], 10.0)), false);
            s.project_edge(&arc, Some(&[arc.a, mid, arc.b])).unwrap();
            let sk = &s.ps.sketch;
            let (_, e) = sk
                .entities()
                .find(|(_, e)| matches!(e, Entity::Arc { .. }))
                .expect("an arc");
            let Entity::Arc { start, end, .. } = e else {
                panic!("an arc");
            };
            let q = |id| {
                let p = sk.point(id).unwrap().position;
                [p.x.round(), p.y.round()]
            };
            (q(*start), q(*end), s.ps.reference_lines.len())
        };
        let r = 10.0 / 2f64.sqrt();
        assert_eq!(
            arc_ends([r, r]),
            ([10.0, 0.0], [0.0, 10.0], 1),
            "the quarter"
        );
        assert_eq!(
            arc_ends([-r, -r]),
            ([0.0, 10.0], [10.0, 0.0], 1),
            "three quarters"
        );
        let mut s = DrwSketch::new(1, SketchFor::NewSection, ViewSketch::default());
        let pts: Vec<[f64; 2]> = (0..=60)
            .map(|k| {
                let t = std::f64::consts::FRAC_PI_2 * f64::from(k) / 60.0;
                [10.0 * t.cos(), 5.0 * t.sin()]
            })
            .collect();
        s.project_edge(&edge(None, false), Some(&pts)).unwrap();
        let n = s.ps.reference_lines.len();
        assert!((20..=25).contains(&n), "{n} lines");
        assert!(s.ps.sketch.points().all(|(_, p)| p.fixed));
        let mut empty = DrwSketch::new(1, SketchFor::NewSection, ViewSketch::default());
        assert!(empty.project_edge(&edge(None, false), None).is_err());
    }

    /// Projected view geometry is fixed construction: a line drawn to its
    /// end is the section line, the projection is not part of it; Delete
    /// removes a selected projection and its points.
    #[test]
    fn projected_edges_stay_out_of_the_line() {
        use crate::drawing::hlr::ViewEdge;
        let mut s = DrwSketch::new(1, SketchFor::NewSection, ViewSketch::default());
        let edge = ViewEdge {
            name: Some("Box1:E1".into()),
            a: [0.0, 0.0],
            b: [40.0, 0.0],
            circle: None,
            full_circle: false,
            straight: true,
            visible: true,
            note: None,
            hole: None,
        };
        s.project_edge(&edge, None).unwrap();
        let end =
            s.ps.sketch
                .points()
                .find(|(_, p)| p.position.x > 39.0)
                .map(|(id, _)| id)
                .unwrap();
        let on_end = Pick {
            pos: Point2::new(40.0, 0.0),
            snap: Snap::Point(end),
            infer: None,
            align: Align::default(),
        };
        s.tools.click(&mut s.ps, at(40.0, 30.0)).unwrap();
        s.tools.click(&mut s.ps, on_end).unwrap();
        s.ps.sketch.solve().unwrap();
        let line = ViewSketch(s.ps.sketch.clone()).line().unwrap();
        assert_eq!(line.len(), 2);
        assert!(
            (line[1][0] - 40.0).abs() < 1e-9 && line[1][1].abs() < 1e-9,
            "{line:?}"
        );
        let projected = s.ps.reference_lines[0];
        s.tools.selected = vec![projected];
        assert!(s.delete_selected());
        assert!(s.ps.sketch.entity(projected).is_none());
        // The shared end stays with the section line; the other end goes.
        assert_eq!(s.ps.sketch.points().count(), 2);
    }

    /// Dragging the free end of a vertical line keeps it vertical (the
    /// point is pulled onto the line's x), as a part sketch's drag does.
    #[test]
    fn a_drag_keeps_the_constraints() {
        let mut s = DrwSketch::new(1, SketchFor::NewSection, ViewSketch::default());
        let mut v = at(0.0, -30.0);
        v.infer = Some(super::super::sketch_tools::Infer::Vertical);
        s.tools.click(&mut s.ps, at(0.0, 0.0)).unwrap();
        s.tools.click(&mut s.ps, v).unwrap();
        s.ps.sketch.solve().unwrap();
        let first =
            s.ps.sketch
                .points()
                .find(|(_, p)| p.position.y > -1.0)
                .map(|(id, _)| id)
                .unwrap();
        s.ps.sketch.set_fixed(first, true).unwrap();
        let end =
            s.ps.sketch
                .points()
                .find(|(_, p)| p.position.y < -29.0)
                .map(|(id, _)| id)
                .unwrap();
        s.drag_point(end, Point2::new(12.0, -40.0));
        let q = s.ps.sketch.point(end).unwrap().position;
        assert!(q.x.abs() < 1e-6, "still vertical: {q:?}");
    }

    /// Three clicks of the Line tool give a line of two segments.
    #[test]
    fn line_tool_clicks_make_the_section_line() {
        let mut s = DrwSketch::new(1, SketchFor::NewSection, ViewSketch::default());
        for p in [at(0.0, 30.0), at(0.0, -10.0), at(20.0, -10.0)] {
            s.tools.click(&mut s.ps, p).unwrap();
            s.ps.sketch.solve().unwrap();
        }
        let line = ViewSketch(s.ps.sketch.clone()).line().unwrap();
        assert_eq!(line.len(), 3, "{line:?}");
    }
}
