//! View navigation commands with keys (spec 0.3): Previous / Next View (F5,
//! Shift+F5), Zoom Window (Shift+F3), Zoom Selected (End).

use eframe::egui::{self, Color32, Rect, Stroke};

use super::camera::OrbitCamera;
use super::select::SelItem;
use crate::kernel::Point3;

/// How long the camera has to rest to count as a view of its own (s).
const REST: f64 = 0.35;
/// Views remembered.
const KEPT: usize = 64;

/// The views the user has rested on, with the place in that list.
#[derive(Clone, Debug, Default)]
pub struct ViewHistory {
    views: Vec<OrbitCamera>,
    at: usize,
    /// The camera in the last frame and when it last moved.
    seen: Option<OrbitCamera>,
    moved_at: f64,
}

fn same(a: &OrbitCamera, b: &OrbitCamera) -> bool {
    (a.target - b.target).norm() < 1e-6
        && (a.distance - b.distance).abs() < 1e-6
        && (a.yaw - b.yaw).abs() < 1e-6
        && (a.pitch - b.pitch).abs() < 1e-6
        && (a.roll - b.roll).abs() < 1e-6
}

impl ViewHistory {
    /// Called every frame: a camera at rest becomes the newest view; views
    /// after the current one are dropped, as in a browser's history.
    pub fn watch(&mut self, cam: &OrbitCamera, now: f64) {
        match &self.seen {
            Some(s) if same(s, cam) => {}
            _ => {
                self.seen = Some(cam.clone());
                self.moved_at = now;
            }
        }
        if now - self.moved_at < REST {
            return;
        }
        if self.views.get(self.at).is_some_and(|v| same(v, cam)) {
            return;
        }
        if !self.views.is_empty() {
            self.views.truncate(self.at + 1);
        }
        self.views.push(cam.clone());
        if self.views.len() > KEPT {
            self.views.remove(0);
        }
        self.at = self.views.len() - 1;
    }

    /// The view before the current one.
    pub fn previous(&mut self) -> Option<OrbitCamera> {
        self.at = self.at.checked_sub(1)?;
        self.views.get(self.at).cloned()
    }

    /// The view after the current one.
    pub fn next(&mut self) -> Option<OrbitCamera> {
        let v = self.views.get(self.at + 1).cloned()?;
        self.at += 1;
        Some(v)
    }
}

/// The camera that shows the screen rectangle `zoom` of the view `rect` as
/// large as it fits: centred on it, scaled by the larger of its width and
/// height ratios.
pub fn zoomed_to(cam: &OrbitCamera, zoom: Rect, rect: Rect) -> OrbitCamera {
    let mut out = cam.clone();
    let delta = zoom.center() - rect.center();
    // `pan` moves the view with the pointer: the rectangle's middle comes to
    // the view's middle when it is dragged there.
    out.pan(-delta.x, -delta.y, rect.height());
    let ratio = (zoom.width() / rect.width()).max(zoom.height() / rect.height());
    out.distance = (cam.distance * f64::from(ratio)).max(1e-3);
    out
}

impl super::CadCraftApp {
    /// Previous / next view of the history.
    pub(super) fn view_step(&mut self, back: bool) {
        let v = if back {
            self.view_history.previous()
        } else {
            self.view_history.next()
        };
        match v {
            Some(v) => {
                let before = self.camera.clone();
                self.camera = v;
                // Rests on the view it came to: no new entry.
                self.view_cube.animate(before, &mut self.camera);
            }
            None => {
                self.file_status = Some(Ok(if back {
                    "Previous View: this is the first view".to_owned()
                } else {
                    "Next View: this is the last view".to_owned()
                }));
            }
        }
    }

    /// Zoom Window's drag in the view. True while it owns the mouse.
    pub(super) fn zoom_window_input(
        &mut self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        rect: Rect,
    ) -> bool {
        let Some(start) = self.zoom_window else {
            return false;
        };
        let pointer = ctx.pointer_interact_pos();
        let (down, released, esc) = ctx.input(|i| {
            (
                i.pointer.primary_down(),
                i.pointer.primary_released(),
                i.key_pressed(egui::Key::Escape),
            )
        });
        if esc {
            self.zoom_window = None;
            return true;
        }
        let Some(p) = pointer else { return true };
        match start {
            None => {
                if down && rect.contains(p) {
                    self.zoom_window = Some(Some(p));
                }
            }
            Some(a) => {
                let box_rect = Rect::from_two_pos(a, p);
                painter.rect_stroke(
                    box_rect,
                    0.0,
                    Stroke::new(1.0, Color32::from_rgb(40, 110, 220)),
                    egui::StrokeKind::Inside,
                );
                if released || !down {
                    self.zoom_window = None;
                    if box_rect.width() > 4.0 && box_rect.height() > 4.0 {
                        let before = self.camera.clone();
                        self.camera = zoomed_to(&before, box_rect, rect);
                        self.view_cube.animate(before, &mut self.camera);
                    }
                }
            }
        }
        true
    }

    /// Zoom Selected: frames what is selected; with nothing selected, all.
    pub(super) fn zoom_selected(&mut self) {
        let bounds = if self.is_drawing() {
            None
        } else if self.is_assembly() {
            let ids = self.asm.sel.clone();
            let mut b = None;
            for id in ids {
                if let Some((lo, hi)) = self.asm_box(id) {
                    b = grow(grow(b, lo), hi);
                }
            }
            b
        } else {
            self.part_selection_bounds()
        };
        let before = self.camera.clone();
        match bounds {
            Some((lo, hi)) => self.camera.fit(lo, hi),
            None => self.fit_view(),
        }
        self.view_cube.animate(before, &mut self.camera);
    }

    /// The box of the selected faces, edges, features, bodies and sketch
    /// curves of the active part.
    fn part_selection_bounds(&self) -> Option<(Point3, Point3)> {
        let part = self.part();
        let mut b = None;
        if let Some(i) = self.editing {
            let sk = &part.sketches[i].sketch;
            for e in &self.tools.selected {
                for q in super::overlay::polyline(sk, *e) {
                    b = grow(b, sk.plane.point_at(q.x, q.y));
                }
            }
            return b;
        }
        let opts = crate::kernel::TessellationOptions::default();
        for item in &self.sel {
            match item {
                SelItem::Edge(id) => {
                    if let Ok(pts) = part.body.sample_edge(*id, &opts) {
                        for q in pts {
                            b = grow(b, q);
                        }
                    }
                }
                SelItem::Face(name) => {
                    for (_, f) in part.body.faces() {
                        if f.name.as_deref() == Some(name.as_str())
                            && let Some(m) = &f.mesh
                        {
                            for q in &m.positions {
                                b = grow(b, *q);
                            }
                        }
                    }
                }
                SelItem::Feature(id) => {
                    for fid in crate::part::select::faces_of_feature(&part.body, id) {
                        if let Some(m) = part.body.face(fid).and_then(|f| f.mesh.as_ref()) {
                            for q in &m.positions {
                                b = grow(b, *q);
                            }
                        }
                    }
                }
                SelItem::Vertex(_) | SelItem::Body(_) | SelItem::Sketch(_) | SelItem::Point(..) => {
                }
            }
        }
        b
    }
}

/// `b` grown to take `q`.
fn grow(b: Option<(Point3, Point3)>, q: Point3) -> Option<(Point3, Point3)> {
    Some(match b {
        Some((lo, hi)) => (lo.inf(&q), hi.sup(&q)),
        None => (q, q),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::Pos2;

    fn p(x: f32, y: f32) -> Pos2 {
        Pos2::new(x, y)
    }

    fn cam(d: f64) -> OrbitCamera {
        OrbitCamera {
            distance: d,
            ..OrbitCamera::default()
        }
    }

    #[test]
    fn history_keeps_views_at_rest_only() {
        let mut h = ViewHistory::default();
        h.watch(&cam(100.0), 0.0);
        h.watch(&cam(100.0), 1.0);
        // Passing through: too short a rest.
        h.watch(&cam(90.0), 1.1);
        h.watch(&cam(80.0), 1.2);
        h.watch(&cam(80.0), 2.0);
        assert_eq!(h.views.len(), 2);
        assert!((h.previous().map_or(0.0, |c| c.distance) - 100.0).abs() < 1e-9);
        assert!(h.previous().is_none());
        assert!((h.next().map_or(0.0, |c| c.distance) - 80.0).abs() < 1e-9);
        assert!(h.next().is_none());
    }

    #[test]
    fn a_new_view_after_going_back_drops_the_later_ones() {
        let mut h = ViewHistory::default();
        for (k, d) in [10.0, 20.0, 30.0].into_iter().enumerate() {
            h.watch(&cam(d), k as f64);
            h.watch(&cam(d), k as f64 + 0.5);
        }
        assert!(h.previous().is_some());
        h.watch(&cam(40.0), 5.0);
        h.watch(&cam(40.0), 6.0);
        assert!(h.next().is_none());
        assert!((h.previous().map_or(0.0, |c| c.distance) - 20.0).abs() < 1e-9);
    }

    #[test]
    fn zoom_window_centres_and_scales() {
        let rect = Rect::from_min_max(p(0.0, 0.0), p(800.0, 600.0));
        let zoom = Rect::from_min_max(p(100.0, 100.0), p(300.0, 250.0));
        let c = OrbitCamera {
            projection: crate::gui::camera::Projection::Orthographic,
            ..OrbitCamera::default()
        };
        let out = zoomed_to(&c, zoom, rect);
        assert!((out.distance - c.distance * 0.25).abs() < 1e-5);
        // The point that was at the rectangle's middle is at the view's middle.
        let target = c.target;
        let before = c.project(&target, rect).unwrap_or(p(0.0, 0.0));
        let after = out.project(&target, rect).unwrap_or(p(0.0, 0.0));
        assert!(after.x > before.x, "{before:?} {after:?}");
        assert!(after.y > before.y, "{before:?} {after:?}");
    }
}
