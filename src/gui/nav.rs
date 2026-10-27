//! Navigation shared by the part, sketch and assembly views
//! (`docs/spec/0.3-view.md` §Orbit and zoom): Inventor's Default Orbit Type
//! (Free / Constrained), the orbit pivot at the model point under the
//! cursor (only when there is one), zoom to the cursor, and zooming out by
//! itself when new geometry (a sketch's dimensions, a placed component)
//! leaves the view.

use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};

use super::CadCraftApp;
use crate::kernel::Point3;

impl CadCraftApp {
    /// One frame of an orbit drag. The pivot is taken when the drag starts:
    /// the model point under the press (Application Options › Orbit about
    /// the point under the cursor), else the centre of the screen.
    pub(super) fn nav_orbit(&mut self, delta: Vec2, rect: Rect, press: Option<Pos2>) {
        let pivot = match self.nav_pivot {
            Some(p) => p,
            None => {
                let p = if self.orbit_at_cursor {
                    press.and_then(|q| self.model_point_under(q, rect))
                } else {
                    None
                };
                self.nav_pivot = Some(p);
                p
            }
        };
        self.camera
            .orbit_about(pivot, delta.x, delta.y, self.orbit_type);
    }

    /// The orbit drag ended (no orbit this frame).
    pub(super) fn nav_orbit_end(&mut self) {
        self.nav_pivot = None;
    }

    /// The model point under a screen point: a component's face in an
    /// assembly, a face of the part's body otherwise.
    fn model_point_under(&self, pos: Pos2, rect: Rect) -> Option<Point3> {
        if self.is_assembly() {
            return self.asm_pick_pub(pos, rect).map(|(_, p)| p);
        }
        let (o, d) = self.camera.ray(pos, rect)?;
        let d = d.try_normalize(1e-12)?;
        let (_, t) = self.picker().pick(&o, &d)?;
        Some(o + d * t)
    }

    /// The mouse wheel: zoom (to the cursor unless that is switched off).
    pub(super) fn nav_zoom(&mut self, scroll: f32, pointer: Option<Pos2>, rect: Rect) {
        let s = if self.invert_wheel { -scroll } else { scroll };
        match pointer {
            Some(p) if self.zoom_to_cursor => self.camera.zoom_at(s, p, rect),
            _ => self.camera.zoom(s),
        }
    }

    /// The pivot of a running orbit, drawn as Inventor's small centre mark
    /// (on a layer above the view's picture).
    pub(super) fn draw_orbit_pivot(&self, ctx: &egui::Context, rect: Rect) {
        let Some(Some(p)) = self.nav_pivot else {
            return;
        };
        let Some(s) = self.camera.project(&p, rect) else {
            return;
        };
        let painter = ctx
            .layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("orbit_pivot"),
            ))
            .with_clip_rect(rect);
        let ink = Stroke::new(1.5, Color32::from_rgb(40, 40, 40));
        painter.circle_filled(s, 4.0, Color32::from_rgb(120, 200, 255));
        painter.circle_stroke(s, 4.0, ink);
        painter.line_segment([s - Vec2::X * 9.0, s - Vec2::X * 5.0], ink);
        painter.line_segment([s + Vec2::X * 5.0, s + Vec2::X * 9.0], ink);
        painter.line_segment([s - Vec2::Y * 9.0, s - Vec2::Y * 5.0], ink);
        painter.line_segment([s + Vec2::Y * 5.0, s + Vec2::Y * 9.0], ink);
    }

    /// Zooms out by itself (animated, the view direction kept) when new
    /// geometry no longer fits the view: the edited sketch after a change
    /// (a typed size, a dimension), a component just placed in an
    /// assembly. Nothing happens while a mouse button is down (a drag) or
    /// when Application Options › Zoom out to new geometry is off.
    pub(super) fn keep_in_view(&mut self, ctx: &egui::Context, rect: Rect) {
        let Some((sig, watched)) = self.watched_geometry() else {
            self.fit_sig = None;
            return;
        };
        let Some(old) = self.fit_sig else {
            // A sketch just opened / an assembly just shown: only remember.
            self.fit_sig = Some(sig);
            return;
        };
        if old == sig || ctx.input(|i| i.pointer.any_down()) {
            return;
        }
        self.fit_sig = Some(sig);
        let Some((lo, hi)) = watched else {
            return;
        };
        if !self.fit_new_geometry || self.camera.box_in_view(lo, hi, rect, 8.0) {
            return;
        }
        let before = self.camera.clone();
        self.fit_view();
        self.view_cube.animate(before, &mut self.camera);
        ctx.request_repaint();
    }

    /// What [`Self::keep_in_view`] watches: a signature that changes with
    /// the geometry, and its box. The edited sketch (its points and circle
    /// radii), else the assembly's components (their count: a placement or
    /// a paste, not a drag).
    fn watched_geometry(&self) -> Option<(u64, Option<(Point3, Point3)>)> {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        if let Some(a) = self.assembly() {
            a.occurrences.len().hash(&mut h);
            a.occurrences.iter().map(|o| o.id).max().hash(&mut h);
            return Some((h.finish(), self.assembly_box()));
        }
        let ps = self.part().sketches.get(self.editing?)?;
        let sk = &ps.sketch;
        let mut lo = [f64::INFINITY; 2];
        let mut hi = [f64::NEG_INFINITY; 2];
        let mut grow = |x: f64, y: f64, r: f64| {
            lo = [lo[0].min(x - r), lo[1].min(y - r)];
            hi = [hi[0].max(x + r), hi[1].max(y + r)];
        };
        for (_, p) in sk.points() {
            grow(p.position.x, p.position.y, 0.0);
            p.position.x.to_bits().hash(&mut h);
            p.position.y.to_bits().hash(&mut h);
        }
        for (_, e) in sk.entities() {
            if let crate::sketch::Entity::Circle { center, radius } = e
                && let Some(c) = sk.point(*center)
            {
                grow(c.position.x, c.position.y, *radius);
                radius.to_bits().hash(&mut h);
            }
        }
        let sig = h.finish();
        if lo[0] > hi[0] {
            return Some((sig, None));
        }
        let plane = &sk.plane;
        let corners = [
            plane.point_at(lo[0], lo[1]),
            plane.point_at(hi[0], lo[1]),
            plane.point_at(lo[0], hi[1]),
            plane.point_at(hi[0], hi[1]),
        ];
        let b = corners[1..]
            .iter()
            .fold((corners[0], corners[0]), |(a, b), p| (a.inf(p), b.sup(p)));
        Some((sig, Some(b)))
    }
}
