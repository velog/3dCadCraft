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
use super::sketch_tools::{Align, Pick, SketchTools, Snap, Tool};
use crate::drawing::ViewKind;
use crate::drawing::annot::Placement;
use crate::drawing::view_sketch::ViewSketch;
use crate::kernel::Point3;
use crate::part::PartSketch;
use crate::sketch::{ConstraintId, Point2};

/// What finishing a view sketch does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SketchFor {
    /// The line of a new section view (the view is placed next).
    NewSection,
    /// The line of section view `id`.
    Section(u32),
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
}

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
        }
    }

    /// Status-bar prompt.
    pub(super) fn prompt(&self) -> String {
        let what = match self.owner {
            SketchFor::NewSection => "Section line",
            SketchFor::Section(_) => "Edit Section Line",
        };
        format!("{what}: {} (Enter: Finish Sketch)", self.tools.prompt())
    }
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
    })
}

impl CadCraftApp {
    /// Opens a view sketch (the running drawing command ends).
    pub(super) fn open_view_sketch(&mut self, view: u32, owner: SketchFor, sketch: ViewSketch) {
        self.drw.tool = None;
        self.drw.sel = Some(view);
        self.drw.sel_ann = None;
        self.drw.section_track.clear();
        self.drw.sketch = Some(Box::new(DrwSketch::new(view, owner, sketch)));
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
            if s.tools.pick_count() > 0 || s.tools.tool != Tool::Select {
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
            Point2::new(
                pl.center[0] + (p[0] - pl.position[0]) / pl.scale,
                pl.center[1] + (p[1] - pl.position[1]) / pl.scale,
            )
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

        // Clicks.
        if s.dim.is_none()
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

        // The right-click menu.
        let mut finish = false;
        let mut cancel = false;
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
            if ui.button("Restart chain").clicked() {
                s.tools.escape();
                s.tools.set_tool(Tool::Line);
                ui.close();
            }
            ui.separator();
            if ui.button("Cancel").clicked() {
                cancel = true;
                ui.close();
            }
        });

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
                let ok = s.ps.sketch.set_dimension(c, value).is_ok() && s.ps.sketch.solve().is_ok();
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
