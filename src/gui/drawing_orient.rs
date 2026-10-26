//! The base view's preview while the Drawing View dialog is open
//! (`docs/spec/4-drawings.md` §3.2a): the preview sits on the sheet and is
//! dragged by its frame, a corner drag changes the scale (snapping to the
//! standard scales, Ctrl free), triangles on its border add projected views,
//! and a ViewCube at its upper right corner sets the orientation — a face,
//! edge or corner, the arrows and roll buttons, a drag on the cube orbits
//! freely. The cube's menu (its arrow or a right click) opens Custom View:
//! free orbit on the sheet and Rotate at Angle, closed by Finish Custom View.
//! While the orientation moves, the model's edges are drawn without hidden
//! line removal; the real lines follow from the worker.

use std::sync::Arc;

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Shape, Stroke, Vec2};
use nalgebra::{Rotation3, Unit};

use super::CadCraftApp;
use super::camera::OrbitCamera;
use super::drawing::{DrwTool, ShownView, draw_view_colored};
use super::valuefield::ExprValue;
use super::viewcube::{self, CubeHit};
use crate::drawing::hlr::{ViewLines, ViewModel};
use crate::drawing::{self as drw, ProjectDir, ViewFrame, ViewKind, ViewOrientation};

const PREVIEW: Color32 = Color32::from_rgb(60, 140, 90);
const HANDLE: Color32 = Color32::from_rgb(40, 120, 230);
/// Gap between the base view and a projected view added by a triangle
/// (sheet mm).
const PROJECT_GAP: f64 = 15.0;

/// Preview state of the Drawing View dialog.
#[derive(Clone, Debug, Default)]
pub(super) struct OrientUi {
    /// Where a new base view sits (sheet mm); set on the dialog's first
    /// frame on the sheet.
    pub at: Option<[f64; 2]>,
    /// Projected views added with the border triangles (created with the
    /// base view at OK).
    pub projected: Vec<ProjectDir>,
    /// Where those go (sheet mm), from the last frame.
    pub targets: Vec<[f64; 2]>,
    drag: Option<Drag>,
    /// Custom View is open.
    pub custom: Option<CustomView>,
    /// The ViewCube's menu is open.
    menu: bool,
    /// The model's edges for the preview while the orientation moves.
    wire: Option<Arc<ViewModel>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag {
    /// Free orbit (the cube, or anywhere in Custom View).
    Orbit,
    /// Moving the preview: grab offset (position − pointer, sheet mm).
    Move([f64; 2]),
    /// A corner: the scale and the corner's distance from the centre at the
    /// start.
    Scale(f64, f64),
}

/// Custom View: the orientation to go back to on Cancel and the Rotate at
/// Angle settings.
#[derive(Clone, Debug)]
pub(super) struct CustomView {
    pub before: ViewOrientation,
    pub axis: Axis,
    /// Increment (degrees).
    pub step: f64,
}

/// Rotate at Angle axes (Inventor's Incremental View Rotate).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Axis {
    /// Sheet horizontal.
    Horizontal,
    /// Sheet vertical.
    Vertical,
    /// Square to the sheet.
    Normal,
}

/// The camera looking as `frame` does (for the ViewCube).
fn cam_of(frame: &ViewFrame) -> OrbitCamera {
    let mut c = OrbitCamera::default();
    c.set_frame(frame.toward, frame.up);
    c
}

fn orientation_of(cam: &OrbitCamera) -> ViewOrientation {
    let (back, up) = cam.frame();
    ViewOrientation::from_frame(&ViewFrame::new(back, up))
}

/// The orientation after a click on the ViewCube.
pub(super) fn cube_hit(o: ViewOrientation, hit: CubeHit) -> ViewOrientation {
    let mut c = cam_of(&o.frame());
    match hit {
        CubeHit::Home => return ViewOrientation::IsoTopRight,
        CubeHit::Dir(d) => c.look_from(viewcube::direction(d)),
        CubeHit::Compass(Some(d)) => {
            let dir = viewcube::compass_direction(&c, d);
            c.look_from(dir);
        }
        CubeHit::Compass(None) => return o,
        CubeHit::Arrow(x, y) => match viewcube::arrow_direction(&c, x, y) {
            Some(d) => c.look_from(d),
            None => return o,
        },
        CubeHit::Roll(sign) => c.roll += f64::from(sign) * std::f64::consts::FRAC_PI_2,
    }
    orientation_of(&c)
}

/// Rotate at Angle: turns the model by `deg` about a sheet axis (positive:
/// counter-clockwise seen from the axis' tip — right, up or the viewer).
pub(super) fn rotate(o: ViewOrientation, axis: Axis, deg: f64) -> ViewOrientation {
    let f = o.frame();
    let a = match axis {
        Axis::Horizontal => f.right,
        Axis::Vertical => f.up,
        Axis::Normal => f.toward,
    };
    // Turning the model one way turns the frame the other.
    let r = Rotation3::from_axis_angle(&Unit::new_normalize(a), -deg.to_radians());
    ViewOrientation::from_frame(&ViewFrame::new(r * f.toward, r * f.up))
}

/// Polylines on the view plane, their extent's middle and size.
type Wire = (Vec<Vec<[f64; 2]>>, [f64; 2], (f64, f64));

/// The model's edges seen in `frame` (no hidden line removal): polylines
/// and the extent's middle and size.
fn wire_lines(m: &ViewModel, f: &ViewFrame) -> Wire {
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    let lines: Vec<Vec<[f64; 2]>> = m
        .edges
        .iter()
        .map(|e| {
            e.pts
                .iter()
                .map(|p| {
                    let q = f.project(p);
                    for k in 0..2 {
                        lo[k] = lo[k].min(q[k]);
                        hi[k] = hi[k].max(q[k]);
                    }
                    q
                })
                .collect()
        })
        .collect();
    if lo[0] > hi[0] {
        return (lines, [0.0, 0.0], (0.0, 0.0));
    }
    (
        lines,
        [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5],
        (hi[0] - lo[0], hi[1] - lo[1]),
    )
}

/// Where a projected view added by a triangle goes: beside the base view
/// (half sizes `half`, sheet mm).
fn projected_offset(dir: ProjectDir, half: [f64; 2]) -> [f64; 2] {
    let (dx, dy) = match dir {
        ProjectDir::Right => (1.0, 0.0),
        ProjectDir::Left => (-1.0, 0.0),
        ProjectDir::Up => (0.0, 1.0),
        ProjectDir::Down => (0.0, -1.0),
        ProjectDir::Iso(x, y) => (f64::from(x), f64::from(y)),
    };
    [
        dx * (2.0 * half[0] + PROJECT_GAP),
        dy * (2.0 * half[1] + PROJECT_GAP),
    ]
}

/// The eight projection directions of the border triangles.
const TRIANGLES: [ProjectDir; 8] = [
    ProjectDir::Right,
    ProjectDir::Left,
    ProjectDir::Up,
    ProjectDir::Down,
    ProjectDir::Iso(1, 1),
    ProjectDir::Iso(-1, 1),
    ProjectDir::Iso(1, -1),
    ProjectDir::Iso(-1, -1),
];

/// A filled triangle pointing along `d` (screen) with its tip at `tip`.
fn triangle(tip: Pos2, d: Vec2, size: f32) -> Vec<Pos2> {
    let d = d.normalized();
    let n = Vec2::new(-d.y, d.x);
    let base = tip - d * size;
    vec![tip, base + n * size * 0.6, base - n * size * 0.6]
}

/// The nearest standard scale.
fn snap_scale(s: f64) -> f64 {
    drw::SCALES
        .iter()
        .copied()
        .min_by(|a, b| (a.ln() - s.ln()).abs().total_cmp(&(b.ln() - s.ln()).abs()))
        .unwrap_or(s)
}

impl CadCraftApp {
    /// The default place of a new base view: the upper left part of the
    /// sheet.
    fn default_view_at(&self) -> [f64; 2] {
        self.drawing()
            .and_then(|d| d.sheet().map(|s| s.dims()))
            .map_or([100.0, 150.0], |(w, h)| [w * 0.3, h * 0.62])
    }

    /// Does the open Drawing View dialog show a preview on the sheet (a new
    /// base view, or Edit View of a base view)?
    pub(super) fn orient_active(&self) -> bool {
        self.drw
            .dialog
            .as_ref()
            .is_some_and(|d| d.model.is_some() && !d.projected)
    }

    /// The preview, its handles and the ViewCube of the Drawing View dialog;
    /// takes the sheet's clicks and drags while the dialog is open.
    pub(super) fn orient_tool(
        &mut self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        rect: Rect,
        response: &egui::Response,
    ) {
        let Some(mut dlg) = self.drw.dialog.clone() else {
            if self.drw.tool == Some(DrwTool::Base) {
                self.drw.tool = None;
            }
            return;
        };
        let Some(model) = dlg.model.filter(|_| !dlg.projected) else {
            return;
        };
        let cam = self.drw.cam;
        let to = |q: [f64; 2]| cam.to_screen(rect, q);
        let pointer = response.hover_pos();
        // Where the preview is.
        let (pos, stored) = match dlg.edit {
            None => (*dlg.orient.at.get_or_insert(self.default_view_at()), None),
            Some(id) => {
                let Some(v) = self.drawing().and_then(|d| d.sheet()?.view(id).cloned()) else {
                    return;
                };
                let stored = match v.kind {
                    ViewKind::Base { orientation, .. } => Some(orientation),
                    _ => None,
                };
                (v.position, stored)
            }
        };
        let frame = dlg.orientation.frame();
        let flat = dlg.sheet_metal == drw::SheetView::Flat;
        let orbiting = dlg.orient.drag == Some(Drag::Orbit);
        let lines = if orbiting {
            None
        } else {
            self.view_lines(Some(ctx), model, &frame, None, flat)
        };
        if dlg.edit.is_none()
            && dlg.auto_scale
            && let Some(l) = &lines
            && let Some(dims) = self.drawing().and_then(|d| d.sheet().map(|s| s.dims()))
        {
            dlg.scale = drw::fit_scale(l.size(), dims, 0.35);
        }
        let scale = dlg.scale;
        // The preview: the real lines, else the model's edges.
        let changed = dlg.edit.is_none()
            || stored != Some(dlg.orientation)
            || self
                .drawing()
                .and_then(|d| d.sheet()?.view(dlg.edit?).map(|v| v.scale))
                != Some(scale);
        let size = match &lines {
            Some(l) => {
                if changed {
                    self.draw_preview(painter, rect, l.clone(), pos, scale, dlg.style);
                }
                l.size()
            }
            None => {
                if dlg.orient.wire.is_none() && !flat {
                    dlg.orient.wire = self.model_job(model, false).map(|j| Arc::new(j.model()));
                }
                match &dlg.orient.wire {
                    Some(w) => {
                        let (pl, c, size) = wire_lines(w, &frame);
                        let stroke = Stroke::new(1.0, PREVIEW);
                        for l in pl {
                            let pts: Vec<Pos2> = l
                                .iter()
                                .map(|q| {
                                    to([
                                        pos[0] + (q[0] - c[0]) * scale,
                                        pos[1] + (q[1] - c[1]) * scale,
                                    ])
                                })
                                .collect();
                            painter.add(Shape::line(pts, stroke));
                        }
                        size
                    }
                    None => {
                        painter.text(
                            to(pos),
                            Align2::CENTER_CENTER,
                            "Computing view...",
                            FontId::proportional(13.0),
                            PREVIEW,
                        );
                        (40.0 / scale, 30.0 / scale)
                    }
                }
            }
        };
        let half = [
            (size.0 * scale * 0.5).max(5.0),
            (size.1 * scale * 0.5).max(5.0),
        ];
        let lo = to([pos[0] - half[0], pos[1] + half[1]]);
        let hi = to([pos[0] + half[0], pos[1] - half[1]]);
        let frame_rect = Rect::from_two_pos(lo, hi).expand(6.0);
        dashed(painter, frame_rect, PREVIEW);
        // Corner handles (scale).
        let corners = [
            frame_rect.left_top(),
            frame_rect.right_top(),
            frame_rect.left_bottom(),
            frame_rect.right_bottom(),
        ];
        for c in corners {
            painter.rect_filled(Rect::from_center_size(c, Vec2::splat(7.0)), 0.0, HANDLE);
        }
        // Projected views: previews and the border triangles (new views).
        let mut triangles = Vec::new();
        let mut removers = Vec::new();
        if dlg.edit.is_none() {
            let projection = self
                .drawing()
                .map_or(drw::Projection::ThirdAngle, |d| d.standard.projection);
            dlg.orient.targets.clear();
            for &dir in &dlg.orient.projected {
                let at = {
                    let o = projected_offset(dir, half);
                    [pos[0] + o[0], pos[1] + o[1]]
                };
                dlg.orient.targets.push(at);
                let pf = frame.projected(dir, projection);
                if !orbiting && let Some(l) = self.view_lines(Some(ctx), model, &pf, None, flat) {
                    self.draw_preview(painter, rect, l, at, scale, dlg.style);
                }
                let x = to(at) + Vec2::new(0.0, -14.0);
                painter.circle_filled(x, 7.0, Color32::from_rgb(200, 60, 50));
                painter.text(
                    x,
                    Align2::CENTER_CENTER,
                    "x",
                    FontId::proportional(11.0),
                    Color32::WHITE,
                );
                removers.push((x, dir));
            }
            let c = frame_rect.center();
            for dir in TRIANGLES {
                if dlg.orient.projected.contains(&dir) {
                    continue;
                }
                let (d, tip) = match dir {
                    ProjectDir::Right => (Vec2::X, Pos2::new(frame_rect.right() + 14.0, c.y)),
                    ProjectDir::Left => (-Vec2::X, Pos2::new(frame_rect.left() - 14.0, c.y)),
                    ProjectDir::Up => (-Vec2::Y, Pos2::new(c.x, frame_rect.top() - 14.0)),
                    ProjectDir::Down => (Vec2::Y, Pos2::new(c.x, frame_rect.bottom() + 14.0)),
                    ProjectDir::Iso(x, y) => {
                        let d = Vec2::new(f32::from(x), -f32::from(y));
                        let corner = Pos2::new(
                            if x > 0 {
                                frame_rect.right()
                            } else {
                                frame_rect.left()
                            },
                            if y > 0 {
                                frame_rect.top()
                            } else {
                                frame_rect.bottom()
                            },
                        );
                        (d, corner + d.normalized() * 16.0)
                    }
                };
                let tri = triangle(tip, d, 10.0);
                let hot = pointer.is_some_and(|p| p.distance(tip - d.normalized() * 5.0) < 9.0);
                painter.add(Shape::convex_polygon(
                    tri,
                    if hot { HANDLE } else { PREVIEW },
                    Stroke::NONE,
                ));
                triangles.push((tip - d.normalized() * 5.0, dir));
            }
        }
        // The ViewCube at the upper right corner.
        let cube_cam = cam_of(&frame);
        let centre = Pos2::new(
            (frame_rect.right() + 70.0).min(rect.right() - 70.0),
            (frame_rect.top() + 20.0).max(rect.top() + 70.0),
        );
        let vp = Rect::from_min_max(
            Pos2::new(rect.left(), centre.y - 84.0),
            Pos2::new(centre.x + 88.0, rect.bottom()),
        );
        viewcube::draw(painter, &cube_cam, vp, pointer, orbiting);
        let menu_btn = Rect::from_center_size(
            viewcube::area(vp).right_bottom() + Vec2::new(-6.0, -6.0),
            Vec2::splat(14.0),
        );
        painter.rect_filled(menu_btn, 2.0, Color32::from_gray(235));
        painter.add(Shape::convex_polygon(
            vec![
                menu_btn.center() + Vec2::new(-4.0, -2.0),
                menu_btn.center() + Vec2::new(4.0, -2.0),
                menu_btn.center() + Vec2::new(0.0, 3.0),
            ],
            Color32::from_gray(60),
            Stroke::NONE,
        ));

        // Input.
        let press = ctx.input(|i| i.pointer.press_origin());
        if response.drag_started_by(egui::PointerButton::Primary)
            && let Some(p) = press
        {
            let q = cam.to_sheet(rect, p);
            dlg.orient.drag = if matches!(viewcube::pick(&cube_cam, vp, p), Some(CubeHit::Dir(_)))
                || (dlg.orient.custom.is_some() && !viewcube::active_area(vp).contains(p))
            {
                Some(Drag::Orbit)
            } else if corners.iter().any(|c| c.distance(p) < 8.0) {
                Some(Drag::Scale(
                    scale,
                    (q[0] - pos[0]).hypot(q[1] - pos[1]).max(1e-3),
                ))
            } else if dlg.edit.is_none() && frame_rect.contains(p) {
                Some(Drag::Move([pos[0] - q[0], pos[1] - q[1]]))
            } else {
                None
            };
        }
        if response.dragged_by(egui::PointerButton::Primary)
            && let Some(p) = response.interact_pointer_pos()
        {
            let q = cam.to_sheet(rect, p);
            match dlg.orient.drag {
                Some(Drag::Orbit) => {
                    let mut c = cube_cam.clone();
                    let d = response.drag_delta();
                    c.orbit(d.x, d.y);
                    dlg.orientation = orientation_of(&c);
                }
                Some(Drag::Move(g)) => dlg.orient.at = Some([q[0] + g[0], q[1] + g[1]]),
                Some(Drag::Scale(s0, d0)) => {
                    let s = s0 * (q[0] - pos[0]).hypot(q[1] - pos[1]) / d0;
                    let free = ctx.input(|i| i.modifiers.ctrl);
                    dlg.scale = if free {
                        (s * 1000.0).round().max(1.0) / 1000.0
                    } else {
                        snap_scale(s)
                    };
                    dlg.auto_scale = false;
                }
                None => {}
            }
        }
        if response.drag_stopped() || !ctx.input(|i| i.pointer.primary_down()) {
            dlg.orient.drag = None;
        }
        if response.clicked()
            && let Some(p) = pointer
        {
            if menu_btn.expand(2.0).contains(p) {
                dlg.orient.menu = !dlg.orient.menu;
            } else if let Some(hit) = viewcube::pick(&cube_cam, vp, p) {
                dlg.orientation = cube_hit(dlg.orientation, hit);
                dlg.orient.menu = false;
            } else if let Some(&(_, dir)) = removers.iter().find(|(x, _)| x.distance(p) < 8.0) {
                dlg.orient.projected.retain(|d| *d != dir);
            } else if let Some(&(_, dir)) = triangles.iter().find(|(t, _)| t.distance(p) < 9.0) {
                dlg.orient.projected.push(dir);
            } else if dlg.orient.menu {
                dlg.orient.menu = false;
            } else if dlg.edit.is_none() && dlg.orient.custom.is_none() {
                // A click elsewhere moves the preview there.
                dlg.orient.at = Some(cam.to_sheet(rect, p));
            }
        }
        if response.secondary_clicked()
            && pointer.is_some_and(|p| viewcube::active_area(vp).contains(p))
        {
            dlg.orient.menu = true;
        }
        if dlg.orient.menu {
            self.drw_cube_menu(ctx, &mut dlg, menu_btn.right_bottom());
        }
        self.drw.dialog = Some(dlg);
    }

    fn draw_preview(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        lines: Arc<ViewLines>,
        at: [f64; 2],
        scale: f64,
        style: drw::ViewStyle,
    ) {
        let v = ShownView {
            id: 0,
            lo: at,
            hi: at,
            lines: Some(lines),
            position: at,
            scale,
            style,
        };
        draw_view_colored(painter, &self.drw.cam, rect, &v, PREVIEW, self.drw.weights);
    }

    /// The ViewCube's menu (Inventor's arrow under the cube / right click).
    fn drw_cube_menu(
        &mut self,
        ctx: &egui::Context,
        dlg: &mut super::drawing::ViewDialog,
        at: Pos2,
    ) {
        egui::Area::new(egui::Id::new("drw_cube_menu"))
            .fixed_pos(at)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_min_width(190.0);
                    let _ = ui.selectable_label(true, "Orthographic");
                    ui.add_enabled(false, egui::Button::new("Perspective"))
                        .on_disabled_hover_text("Perspective views: planned (spec 4 §3.2a)");
                    ui.add_enabled(false, egui::Button::new("Perspective with Ortho Faces"))
                        .on_disabled_hover_text("Perspective views: planned (spec 4 §3.2a)");
                    ui.separator();
                    if ui.button("Custom View Orientation").clicked() {
                        dlg.orient.custom = Some(CustomView {
                            before: dlg.orientation,
                            axis: Axis::Vertical,
                            step: 15.0,
                        });
                        dlg.orient.menu = false;
                    }
                    ui.add_enabled(false, egui::Button::new("Current View Orientation"))
                        .on_disabled_hover_text(
                            "The model's view in its window: planned (spec 4 §3.2a)",
                        );
                    ui.add_enabled(false, egui::Button::new("Saved Camera"))
                        .on_disabled_hover_text("Presentations: planned (spec 4 §3.2a)");
                    ui.separator();
                    if ui.button("Home View").clicked() {
                        dlg.orientation = ViewOrientation::IsoTopRight;
                        dlg.orient.menu = false;
                    }
                });
            });
    }

    /// The Custom View window (free orbit on the sheet, Rotate at Angle,
    /// Finish Custom View).
    pub(super) fn custom_view_window(&mut self, ctx: &egui::Context) {
        let Some(mut dlg) = self.drw.dialog.clone() else {
            return;
        };
        let Some(mut cv) = dlg.orient.custom.clone() else {
            return;
        };
        let mut finish = false;
        let mut cancel = false;
        let win = egui::Window::new("Custom View")
            .resizable(false)
            .collapsible(false)
            .default_width(260.0);
        self.layout
            .place("drawing_custom_view", ctx.content_rect(), false, win)
            .show(ctx, |ui| {
                ui.weak("Drag on the sheet to orbit the model; the ViewCube works too.");
                ui.separator();
                ui.label(egui::RichText::new("Rotate at Angle").strong());
                ui.horizontal(|ui| {
                    ui.radio_value(&mut cv.axis, Axis::Horizontal, "Horizontal");
                    ui.radio_value(&mut cv.axis, Axis::Vertical, "Vertical");
                    ui.radio_value(&mut cv.axis, Axis::Normal, "Normal");
                });
                ui.horizontal(|ui| {
                    ui.label("Increment");
                    ui.add(ExprValue::new(&mut cv.step).range(0.1..=180.0).suffix("°"));
                    if ui.button(" - ").on_hover_text("Rotate clockwise").clicked() {
                        dlg.orientation = rotate(dlg.orientation, cv.axis, -cv.step);
                    }
                    if ui
                        .button(" + ")
                        .on_hover_text("Rotate counter-clockwise")
                        .clicked()
                    {
                        dlg.orientation = rotate(dlg.orientation, cv.axis, cv.step);
                    }
                });
                ui.add_enabled(false, egui::Button::new("View Face"))
                    .on_disabled_hover_text(
                        "Turn a picked face to the sheet: planned (spec 4 §3.2a)",
                    );
                ui.label(format!("Orientation: {}", dlg.orientation.label()));
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Finish Custom View").clicked()
                        || super::keys::panel_ok_key(ui, false)
                    {
                        finish = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        if cancel {
            dlg.orientation = cv.before;
            dlg.orient.custom = None;
        } else if finish {
            dlg.orient.custom = None;
        } else {
            dlg.orient.custom = Some(cv);
        }
        self.drw.dialog = Some(dlg);
    }
}

/// A dashed rectangle.
fn dashed(p: &egui::Painter, r: Rect, c: Color32) {
    let stroke = Stroke::new(1.0, c);
    for (a, b) in [
        (r.left_top(), r.right_top()),
        (r.right_top(), r.right_bottom()),
        (r.right_bottom(), r.left_bottom()),
        (r.left_bottom(), r.left_top()),
    ] {
        p.extend(Shape::dashed_line(&[a, b], stroke, 5.0, 4.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cube_clicks_give_standard_orientations() {
        let o = ViewOrientation::Front;
        assert_eq!(cube_hit(o, CubeHit::Dir([0, 0, 1])), ViewOrientation::Top);
        assert_eq!(cube_hit(o, CubeHit::Dir([1, 0, 0])), ViewOrientation::Right);
        assert_eq!(
            cube_hit(o, CubeHit::Dir([1, -1, 1])),
            ViewOrientation::IsoTopRight
        );
        // From the front, the right arrow goes to the right side.
        assert_eq!(cube_hit(o, CubeHit::Arrow(1, 0)), ViewOrientation::Right);
        assert_eq!(cube_hit(o, CubeHit::Home), ViewOrientation::IsoTopRight);
    }

    #[test]
    fn roll_and_rotate_turn_the_sheet_picture() {
        let f = ViewOrientation::Front;
        // Rolled by 90°: the same direction, up turned.
        let r = cube_hit(f, CubeHit::Roll(1));
        let fr = r.frame();
        assert!((fr.toward - f.frame().toward).norm() < 1e-9);
        assert!(fr.up.dot(&f.frame().up).abs() < 1e-9);
        assert!(matches!(r, ViewOrientation::Custom { .. }));
        // Four quarter turns come back to Front.
        let mut o = f;
        for _ in 0..4 {
            o = rotate(o, Axis::Normal, 90.0);
        }
        assert_eq!(o, ViewOrientation::Front);
        // 90° about the vertical axis from the front shows a side.
        let side = rotate(f, Axis::Vertical, 90.0);
        assert!(matches!(
            side,
            ViewOrientation::Left | ViewOrientation::Right
        ));
        // 90° about the horizontal axis shows the top or the bottom.
        let tb = rotate(f, Axis::Horizontal, 90.0).frame();
        assert!(tb.toward.z.abs() > 1.0 - 1e-9);
    }

    #[test]
    fn scales_snap_to_the_standard_list() {
        assert_eq!(snap_scale(0.48), 0.5);
        assert_eq!(snap_scale(1.1), 1.0);
    }

    #[test]
    fn projected_views_go_beside_the_base_view() {
        let o = projected_offset(ProjectDir::Right, [20.0, 10.0]);
        assert_eq!(o, [40.0 + PROJECT_GAP, 0.0]);
        let o = projected_offset(ProjectDir::Iso(-1, 1), [20.0, 10.0]);
        assert_eq!(o, [-(40.0 + PROJECT_GAP), 20.0 + PROJECT_GAP]);
    }
}
