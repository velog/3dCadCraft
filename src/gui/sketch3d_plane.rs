//! The 3D sketch's drawing plane in the view (`docs/spec/1.10-3d-sketch.md`,
//! Drawing plane): a click in empty space puts the point where the cursor's
//! ray meets the active plane (XY / XZ / YZ through the last point, or a
//! plane aligned to); Shift+Space cycles it, Align to Plane picks one, the
//! point is pulled onto the plane's axes through the last point (inference;
//! Ortho with Ctrl+R forces it), and a light grid square at the cursor shows
//! the plane, with its name and the in-plane deltas beside it.

use eframe::egui::{self, Align2, FontId, Pos2, Rect, Stroke};

use super::CadCraftApp;
use super::sketch3d::Tool3;
use crate::kernel::{Point3, Real};
use crate::part::sketch3d_plane::DrawPlane;

/// Half the grid square's side on screen (pixels).
const SQUARE_PX: f32 = 36.0;

impl CadCraftApp {
    /// Whether the running tool places points (and so draws on the plane).
    pub(super) fn sk3_draws_on_plane(&self) -> bool {
        matches!(
            self.sketch3d.as_ref().and_then(|e| e.tool),
            Some(Tool3::Line | Tool3::Point | Tool3::Arc | Tool3::Spline | Tool3::Helix)
        )
    }

    /// The point the part's planes pass through: the last point placed (the
    /// last pick of Arc / Spline / Helix, else the line's last point).
    pub(super) fn sk3_plane_through(&self) -> Option<Point3> {
        let e = self.sketch3d.as_ref()?;
        let s = self.part().sketches3d.get(e.index)?;
        e.picks.last().copied().or(e.last).and_then(|id| s.at(id))
    }

    /// Where a click at `p` lands on the drawing plane, after inference;
    /// the axis it snapped to (0 or 1). `None`: no tool draws, or the plane
    /// is seen edge-on.
    pub(super) fn sk3_plane_point(&self, p: Pos2, rect: Rect) -> Option<(Point3, Option<usize>)> {
        if !self.sk3_draws_on_plane() {
            return None;
        }
        let e = self.sketch3d.as_ref().filter(|e| !e.align_pick)?;
        let (o, d) = self.camera.ray(p, rect)?;
        let through = self.sk3_plane_through();
        let q = e.plane.ray_point(through, o, d)?;
        let Some(last) = through else {
            return Some((q, None));
        };
        let screen = |a: Point3, b: Point3| {
            Some(
                self.camera
                    .project(&a, rect)?
                    .distance(self.camera.project(&b, rect)?),
            )
        };
        Some(
            e.plane
                .infer(last, q, super::overlay::CURVE_SNAP_PX, e.ortho, screen),
        )
    }

    /// Shift+Space: the next drawing plane.
    pub(super) fn sk3_cycle_plane(&mut self) {
        if let Some(e) = self.sketch3d.as_mut() {
            e.plane = e.plane.next();
            e.align_pick = false;
        }
    }

    /// Ctrl+R: Ortho on / off.
    pub(super) fn sk3_toggle_ortho(&mut self) {
        if let Some(e) = self.sketch3d.as_mut() {
            e.ortho = !e.ortho;
        }
    }

    /// Align to Plane is waiting for its click: a planar face, a work or an
    /// origin plane becomes the drawing plane. True when the click was taken.
    pub(super) fn sk3_align_click(&mut self, p: Pos2, rect: Rect) -> bool {
        if !self.sketch3d.as_ref().is_some_and(|e| e.align_pick) {
            return false;
        }
        let picked = self.plane_under_pub(p, rect).and_then(|r| {
            let part = self.part();
            let plane = crate::part::placement::plane_of(&r, &part.body, &part.work).ok()?;
            let name = match &r {
                crate::part::placement::PlaneRef::Origin(o) => format!("{o:?} Plane"),
                crate::part::placement::PlaneRef::Face(n) => n.clone(),
                crate::part::placement::PlaneRef::Work(id) => id.clone(),
                other => format!("{other:?}"),
            };
            Some(DrawPlane::Aligned { plane, name })
        });
        match picked {
            Some(pl) => {
                if let Some(e) = self.sketch3d.as_mut() {
                    e.plane = pl;
                    e.align_pick = false;
                }
                self.file_status = None;
            }
            None => {
                self.file_status = Some(Err(
                    "3D Sketch: Align to Plane takes a planar face, a work or an origin plane"
                        .into(),
                ));
            }
        }
        true
    }

    /// The status bar's note on the plane: `Plane: XY (Shift+Space)`.
    pub(super) fn sk3_plane_status(&self) -> Option<String> {
        let e = self.sketch3d.as_ref()?;
        if e.align_pick {
            return Some("Align to Plane: click a planar face, a work or an origin plane".into());
        }
        if !self.sk3_draws_on_plane() {
            return None;
        }
        Some(format!(
            "Plane: {} (Shift+Space){}",
            e.plane.label(),
            if e.ortho { " · Ortho (Ctrl+R)" } else { "" }
        ))
    }

    /// The plane at the cursor: a light grid square in the plane's colour
    /// (the colour of its normal's axis), its name and the deltas from the
    /// last point, and the inference line when the point snapped to an axis.
    pub(super) fn sk3_draw_plane(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        pointer: Option<Pos2>,
    ) {
        let (Some(e), Some(p)) = (self.sketch3d.as_ref(), pointer) else {
            return;
        };
        if !rect.contains(p) || e.align_pick {
            return;
        }
        let Some((q, snapped)) = self.sk3_plane_point(p, rect) else {
            return;
        };
        let c = super::theme::canvas();
        let col = match &e.plane {
            DrawPlane::Xy => c.axis_z,
            DrawPlane::Xz => c.axis_y,
            DrawPlane::Yz => c.axis_x,
            DrawPlane::Aligned { .. } => c.sketch_open,
        };
        let Some(sq) = self.camera.project(&q, rect) else {
            return;
        };
        // Millimetres per pixel at the point, along the plane's first axis.
        let [a, b] = e.plane.axes();
        let mm_per_px = self
            .camera
            .project(&(q + a), rect)
            .map(|s| s.distance(sq))
            .filter(|d| *d > 1e-6)
            .map_or(1.0, |d| 1.0 / d as Real);
        let h = SQUARE_PX as Real * mm_per_px;
        let faint = super::theme::with_alpha(col, 60);
        let edge = super::theme::with_alpha(col, 150);
        let corner = |u: Real, v: Real| self.camera.project(&(q + a * u + b * v), rect);
        // The square and three lines each way inside it.
        let corners: Vec<Pos2> = [(-h, -h), (h, -h), (h, h), (-h, h)]
            .iter()
            .filter_map(|(u, v)| corner(*u, *v))
            .collect();
        if corners.len() == 4 {
            painter.add(egui::Shape::closed_line(corners, Stroke::new(1.2, edge)));
        }
        for k in 1..4 {
            let t = -h + 2.0 * h * k as Real / 4.0;
            for (s, t2) in [(corner(t, -h), corner(t, h)), (corner(-h, t), corner(h, t))] {
                if let (Some(s), Some(t2)) = (s, t2) {
                    painter.line_segment([s, t2], Stroke::new(1.0, faint));
                }
            }
        }
        // The inference line through the last point.
        let through = self.sk3_plane_through();
        if let (Some(k), Some(last)) = (snapped, through) {
            let dir = e.plane.axes()[k];
            let far = (q - last).norm().max(h) * 1.5;
            if let (Some(s), Some(t)) = (
                self.camera.project(&(last - dir * far), rect),
                self.camera.project(&(last + dir * far), rect),
            ) {
                let axis_col = [c.axis_x, c.axis_y, c.axis_z][dir.iamax().min(2)];
                painter.add(egui::Shape::dashed_line(
                    &[s, t],
                    Stroke::new(1.2, axis_col),
                    6.0,
                    4.0,
                ));
            }
        }
        // The plane's name and Dynamic Input's deltas along it.
        let mut text = e.plane.label();
        if let Some(last) = through {
            let [du, dv] = e.plane.deltas(last, q);
            let [nu, nv] = e.plane.axis_names();
            text.push_str(&format!(
                "   {nu} {du:.2}   {nv} {dv:.2}   L {:.2}",
                (q - last).norm()
            ));
        }
        painter.text(
            // Above the cursor: the value next to it sits below.
            sq + egui::vec2(SQUARE_PX * 0.6, -SQUARE_PX * 0.6),
            Align2::LEFT_BOTTOM,
            text,
            FontId::proportional(12.0),
            col,
        );
    }
}
