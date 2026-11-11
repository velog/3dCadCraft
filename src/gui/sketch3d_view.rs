//! The 3D sketch in the view (`docs/spec/1.10-3d-sketch.md`): dimensions
//! drawn on the geometry and edited in place, constraint glyphs that can be
//! selected and deleted, selection of points and lines, and the 3D Move
//! triad (Inventor's 3D Move/Rotate): an arrow per axis (drag along it, or
//! click it to type the distance) and a square per plane (drag in it); the
//! sketch's constraints are solved as the selection moves.

use eframe::egui::{self, Pos2, Rect, RichText, Stroke};

use super::CadCraftApp;
use crate::kernel::{Point3, Real, Vector3};
use crate::part::sketch3d::{Constraint3, Sketch3d};

/// What a label in the view stands for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Label3 {
    /// Dimension `k` (its value shown).
    Dim(usize),
    /// Glyph of constraint `k` (by a line; a two-line constraint has two).
    Glyph(usize),
}

/// The 3D Move triad of the open 3D sketch.
#[derive(Clone, Debug, Default)]
pub(super) struct Gizmo {
    /// The drag running: axis (0..3) or plane (3 + the normal's axis), the
    /// sketch when it began, where on the axis / plane it began.
    pub drag: Option<(usize, Sketch3d, Point3)>,
    /// An arrow (or a ring) clicked: its handle and the distance (angle,
    /// radians) being typed; whether the box has been focused.
    pub typing: Option<(usize, Real, bool)>,
}

/// How far out the rotation rings lie, as a fraction of the arrows' length.
const RING: Real = 0.75;

/// Points round ring `k` of a triad at `o` with arrows `len` long.
fn ring_points(o: Point3, len: Real, k: usize) -> Vec<Point3> {
    let (i, j) = ((k + 1) % 3, (k + 2) % 3);
    (0..=48)
        .map(|n| {
            let a = std::f64::consts::TAU * n as Real / 48.0;
            o + (axis(i) * a.cos() + axis(j) * a.sin()) * (RING * len)
        })
        .collect()
}

/// `q` turned by `angle` about the axis `k` through `o`.
fn turned(q: Point3, o: Point3, k: usize, angle: Real) -> Point3 {
    let r = nalgebra::Rotation3::from_axis_angle(&nalgebra::Unit::new_normalize(axis(k)), angle);
    o + r * (q - o)
}

/// A handle of the triad: axis arrows 0..3, plane squares 3..6 (3 + the
/// plane's normal axis), rotation rings 6..9 (6 + the axis turned about).
fn axis(k: usize) -> Vector3 {
    let mut a = Vector3::zeros();
    a[k % 3] = 1.0;
    a
}

fn seg_dist(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-9)).clamp(0.0, 1.0);
    (a + ab * t).distance(p)
}

impl CadCraftApp {
    fn sk3(&self) -> Option<(&Sketch3d, &super::sketch3d::Sk3dEdit)> {
        let e = self.sketch3d.as_ref()?;
        Some((self.part().sketches3d.get(e.index)?, e))
    }

    /// Where each dimension and glyph of the open sketch is drawn (screen).
    pub(super) fn sk3_labels(&self, rect: Rect) -> Vec<(Label3, Pos2, String)> {
        let Some((s, _)) = self.sk3() else {
            return Vec::new();
        };
        let pr = |q: Point3| self.camera.project(&q, rect);
        // A line's screen middle and its screen normal.
        let line_at = |l: u32| -> Option<(Pos2, egui::Vec2)> {
            let l = s.line(l)?;
            let (a, b) = (pr(s.at(l.a)?)?, pr(s.at(l.b)?)?);
            let d = (b - a).normalized();
            Some((a + (b - a) * 0.5, egui::vec2(-d.y, d.x)))
        };
        let mut out = Vec::new();
        // Glyphs stack along a line's side: count them per line.
        let mut used: Vec<(u32, f32)> = Vec::new();
        let mut slot = |l: u32| {
            let n = used.iter().filter(|(x, _)| *x == l).count() as f32;
            used.push((l, n));
            n
        };
        for (k, c) in s.constraints.iter().enumerate() {
            match *c {
                Constraint3::Length(l, v) => {
                    if let Some((m, n)) = line_at(l) {
                        out.push((Label3::Dim(k), m + n * 16.0, format!("{v:.2}")));
                    }
                }
                Constraint3::Distance(a, b, v) => {
                    if let (Some(p), Some(q)) = (s.at(a).and_then(pr), s.at(b).and_then(pr)) {
                        out.push((
                            Label3::Dim(k),
                            p + (q - p) * 0.5 - egui::vec2(0.0, 12.0),
                            format!("{v:.2}"),
                        ));
                    }
                }
                Constraint3::Angle(a, b, v) => {
                    let corner = s
                        .line(a)
                        .zip(s.line(b))
                        .and_then(|(x, y)| [x.a, x.b].into_iter().find(|p| *p == y.a || *p == y.b));
                    let at = match corner.and_then(|p| s.at(p)).and_then(pr) {
                        Some(p) => Some(p + egui::vec2(18.0, -18.0)),
                        None => line_at(a)
                            .zip(line_at(b))
                            .map(|((p, _), (q, _))| p + (q - p) * 0.5),
                    };
                    if let Some(at) = at {
                        out.push((Label3::Dim(k), at, format!("{:.1}°", v.to_degrees())));
                    }
                }
                _ => {
                    let text = match *c {
                        Constraint3::Parallel(..) => "//".to_owned(),
                        Constraint3::Perpendicular(..) => "_|_".to_owned(),
                        Constraint3::AlongAxis(_, k) => ["X", "Y", "Z"][k.min(2)].to_owned(),
                        Constraint3::Equal(..) => "=".to_owned(),
                        _ => "?".to_owned(),
                    };
                    for l in c.lines() {
                        if let Some((m, n)) = line_at(l) {
                            let i = slot(l);
                            out.push((Label3::Glyph(k), m - n * (14.0 + 18.0 * i), text.clone()));
                        }
                    }
                }
            }
        }
        out
    }

    /// The label under `p`.
    pub(super) fn sk3_label_at(&self, p: Pos2, rect: Rect) -> Option<Label3> {
        self.sk3_labels(rect)
            .into_iter()
            .filter(|(_, at, t)| {
                let w = 4.0 + 7.0 * t.chars().count() as f32;
                (p.x - at.x).abs() <= w / 2.0 + 2.0 && (p.y - at.y).abs() <= 10.0
            })
            .map(|(l, ..)| l)
            .next()
    }

    /// Draws the labels, the selection and the triad of the open sketch.
    pub(super) fn sk3_draw_view(&self, painter: &egui::Painter, rect: Rect, pointer: Option<Pos2>) {
        let Some((s, e)) = self.sk3() else { return };
        let c = super::theme::canvas();
        let hover = pointer.and_then(|p| self.sk3_label_at(p, rect));
        for (l, at, text) in self.sk3_labels(rect) {
            let w = 4.0 + 7.0 * text.chars().count() as f32;
            let r = Rect::from_center_size(at, egui::vec2(w, 16.0));
            let picked = match l {
                Label3::Glyph(k) => e.sel_cons == Some(k),
                Label3::Dim(k) => e.dim_edit.is_some_and(|(x, _)| x == k),
            };
            let edge = if picked {
                c.selected_edge
            } else if hover == Some(l) {
                super::theme::hover_edge()
            } else {
                c.dimension
            };
            painter.rect_filled(r, 3.0, c.label_fill);
            painter.rect_stroke(r, 3.0, Stroke::new(1.0, edge), egui::StrokeKind::Inside);
            painter.text(
                at,
                egui::Align2::CENTER_CENTER,
                &text,
                egui::FontId::proportional(12.0),
                c.label_text,
            );
        }
        // Distance dimensions: a thin line between their points.
        for cst in &s.constraints {
            if let Constraint3::Distance(a, b, _) = *cst
                && let (Some(p), Some(q)) = (
                    s.at(a).and_then(|q| self.camera.project(&q, rect)),
                    s.at(b).and_then(|q| self.camera.project(&q, rect)),
                )
            {
                painter.extend(egui::Shape::dashed_line(
                    &[p, q],
                    Stroke::new(1.0, c.dimension),
                    6.0,
                    4.0,
                ));
            }
        }
        for id in &e.sel {
            if let Some(p) = s.at(*id).and_then(|q| self.camera.project(&q, rect)) {
                painter.circle_stroke(p, 6.0, Stroke::new(2.5, c.selected_edge));
            }
        }
        if let Some(g) = &e.gizmo
            && let Some((o, len)) = self.sk3_gizmo_frame(rect)
        {
            let cols = [c.axis_x, c.axis_y, c.axis_z];
            let hot = pointer.and_then(|p| self.sk3_gizmo_hit(p, rect));
            for (k, &col) in cols.iter().enumerate() {
                let tip = o + axis(k) * len;
                if let (Some(a), Some(b)) = (
                    self.camera.project(&o, rect),
                    self.camera.project(&tip, rect),
                ) {
                    let w = if hot == Some(k) || g.drag.as_ref().is_some_and(|d| d.0 == k) {
                        4.0
                    } else {
                        2.5
                    };
                    painter.arrow(a, b - a, Stroke::new(w, col));
                }
                // The plane square between the two other axes.
                let (i, j) = ((k + 1) % 3, (k + 2) % 3);
                let q = o + (axis(i) + axis(j)) * (0.3 * len);
                if let Some(sq) = self.camera.project(&q, rect) {
                    let on = hot == Some(3 + k) || g.drag.as_ref().is_some_and(|d| d.0 == 3 + k);
                    let r = Rect::from_center_size(sq, egui::vec2(10.0, 10.0));
                    painter.rect_filled(
                        r,
                        1.0,
                        super::theme::with_alpha(col, if on { 220 } else { 110 }),
                    );
                }
                // The rotation ring about this axis.
                let on = hot == Some(6 + k) || g.drag.as_ref().is_some_and(|d| d.0 == 6 + k);
                let pts: Vec<Pos2> = ring_points(o, len, k)
                    .iter()
                    .filter_map(|q| self.camera.project(q, rect))
                    .collect();
                painter.add(egui::Shape::line(
                    pts,
                    Stroke::new(
                        if on { 3.0 } else { 1.5 },
                        super::theme::with_alpha(col, if on { 255 } else { 170 }),
                    ),
                ));
            }
        }
    }

    /// The triad's origin (the selection's middle) and its arrow length in
    /// model units (a fixed size on screen).
    pub(super) fn sk3_gizmo_frame(&self, rect: Rect) -> Option<(Point3, Real)> {
        let (s, e) = self.sk3()?;
        let pts: Vec<Point3> = e.sel.iter().filter_map(|id| s.at(*id)).collect();
        if pts.is_empty() {
            return None;
        }
        let o = pts
            .iter()
            .fold(Point3::origin(), |a, p| a + p.coords / pts.len() as Real);
        let p0 = self.camera.project(&o, rect)?;
        let px = (0..3)
            .filter_map(|k| self.camera.project(&(o + axis(k)), rect))
            .map(|q| q.distance(p0) as Real)
            .fold(0.0, Real::max);
        (px > 1e-9).then(|| (o, 70.0 / px))
    }

    /// The triad handle under `p`: an arrow 0..3 or a plane square 3..6.
    pub(super) fn sk3_gizmo_hit(&self, p: Pos2, rect: Rect) -> Option<usize> {
        self.sketch3d.as_ref()?.gizmo.as_ref()?;
        let (o, len) = self.sk3_gizmo_frame(rect)?;
        for k in 0..3 {
            let (i, j) = ((k + 1) % 3, (k + 2) % 3);
            let q = o + (axis(i) + axis(j)) * (0.3 * len);
            if self
                .camera
                .project(&q, rect)
                .is_some_and(|sq| sq.distance(p) <= 8.0)
            {
                return Some(3 + k);
            }
        }
        let arrow = (0..3).find(|&k| {
            match (
                self.camera.project(&(o + axis(k) * (0.2 * len)), rect),
                self.camera.project(&(o + axis(k) * len), rect),
            ) {
                (Some(a), Some(b)) => seg_dist(p, a, b) <= 8.0,
                _ => false,
            }
        });
        arrow.or_else(|| {
            (0..3)
                .filter_map(|k| {
                    let pts: Vec<Pos2> = ring_points(o, len, k)
                        .iter()
                        .filter_map(|q| self.camera.project(q, rect))
                        .collect();
                    let d = pts
                        .windows(2)
                        .map(|w| seg_dist(p, w[0], w[1]))
                        .fold(f32::MAX, f32::min);
                    (d <= 6.0).then_some((d, 6 + k))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, h)| h)
        })
    }

    /// Where the cursor ray meets handle `h`'s axis line or plane.
    fn sk3_on_handle(&self, h: usize, o: Point3, p: Pos2, rect: Rect) -> Option<Point3> {
        let (ro, rd) = self.camera.ray(p, rect)?;
        let rd = rd.try_normalize(1e-12)?;
        if h >= 6 {
            // A ring: where the ray meets its plane.
            let n = axis(h - 6);
            let den = rd.dot(&n);
            if den.abs() < 1e-9 {
                return None;
            }
            let t = (o - ro).dot(&n) / den;
            return Some(ro + rd * t);
        }
        if h < 3 {
            let a = axis(h);
            let w = ro - o;
            let ad = a.dot(&rd);
            let den = 1.0 - ad * ad;
            if den < 1e-9 {
                return None; // looking along the axis
            }
            let t = (w.dot(&a) - ad * w.dot(&rd)) / den;
            Some(o + a * t)
        } else {
            let n = axis(h - 3);
            let den = rd.dot(&n);
            if den.abs() < 1e-9 {
                return None;
            }
            let t = (o - ro).dot(&n) / den;
            Some(ro + rd * t)
        }
    }

    /// Moves the selected points of `start` by `d`, keeping them there
    /// while the rest of the sketch follows its constraints; `None` when the
    /// constraints cannot be met.
    fn sk3_moved(start: &Sketch3d, sel: &[u32], d: Vector3) -> Option<Sketch3d> {
        Self::sk3_transformed(start, sel, |q| q + d)
    }

    /// [`Self::sk3_moved`] for any motion `f` of the selected points (a turn
    /// about a ring's axis).
    pub(super) fn sk3_transformed(
        start: &Sketch3d,
        sel: &[u32],
        f: impl Fn(Point3) -> Point3,
    ) -> Option<Sketch3d> {
        let mut s = start.clone();
        let mut held = Vec::new();
        for p in &mut s.points {
            if sel.contains(&p.id) && !p.fixed {
                let q = f(Point3::from(p.at));
                p.at = [q.x, q.y, q.z];
                p.fixed = true;
                held.push(p.id);
            }
        }
        if held.is_empty() {
            return None;
        }
        let ok = s.solve().is_ok();
        for p in &mut s.points {
            if held.contains(&p.id) {
                p.fixed = false;
            }
        }
        s.refresh_helices();
        ok.then_some(s)
    }

    /// The view's input for the open 3D sketch's triad: drags of its
    /// handles. Returns whether the press went to the triad.
    pub(super) fn sk3_gizmo_input(
        &mut self,
        response: &egui::Response,
        ui: &egui::Ui,
        rect: Rect,
    ) -> bool {
        let Some(e) = self.sketch3d.as_ref() else {
            return false;
        };
        if e.gizmo.is_none() {
            return false;
        }
        let sel = e.sel.clone();
        let index = e.index;
        if response.drag_started_by(egui::PointerButton::Primary)
            && let Some(p) = ui.input(|i| i.pointer.press_origin())
            && let Some(h) = self.sk3_gizmo_hit(p, rect)
            && let Some((o, _)) = self.sk3_gizmo_frame(rect)
            && let Some(at) = self.sk3_on_handle(h, o, p, rect)
            && let Some(start) = self.part().sketches3d.get(index).cloned()
        {
            self.checkpoint("3D Move");
            if let Some(g) = self.sketch3d.as_mut().and_then(|e| e.gizmo.as_mut()) {
                g.drag = Some((h, start, at));
                g.typing = None;
            }
            return true;
        }
        let drag = self
            .sketch3d
            .as_ref()
            .and_then(|e| e.gizmo.as_ref())
            .and_then(|g| g.drag.clone());
        if let Some((h, start, at0)) = drag {
            if h >= 6 {
                // A turn: the angle about the ring's axis from where it began,
                // in 5 degree steps (Ctrl: free).
                if response.dragged_by(egui::PointerButton::Primary)
                    && let Some(o) = Self::sel_middle(&start, &sel)
                    && let Some(cursor) = response.interact_pointer_pos()
                    && let Some(at) = self.sk3_on_handle(h, o, cursor, rect)
                {
                    let k = h - 6;
                    let n = axis(k);
                    let (u, v) = (at0 - o, at - o);
                    let mut a = n.dot(&u.cross(&v)).atan2(u.dot(&v));
                    if !ui.input(|i| i.modifiers.ctrl) {
                        let step = 5f64.to_radians();
                        a = (a / step).round() * step;
                    }
                    if let Some(moved) = Self::sk3_transformed(&start, &sel, |q| turned(q, o, k, a))
                        && let Some(slot) = self.part_mut().sketches3d.get_mut(index)
                    {
                        *slot = moved;
                    }
                }
            } else if response.dragged_by(egui::PointerButton::Primary)
                && let Some(cursor) = response.interact_pointer_pos()
                && let Some(at) = self.sk3_on_handle(h, at0, cursor, rect)
                && let Some(moved) = Self::sk3_moved(&start, &sel, at - at0)
                && let Some(slot) = self.part_mut().sketches3d.get_mut(index)
            {
                *slot = moved;
            }
            if response.drag_stopped() {
                if let Some(g) = self.sketch3d.as_mut().and_then(|e| e.gizmo.as_mut()) {
                    g.drag = None;
                }
                self.history.break_coalescing();
            }
            return true;
        }
        false
    }

    /// The middle of the points `sel` of `s`.
    fn sel_middle(s: &Sketch3d, sel: &[u32]) -> Option<Point3> {
        let pts: Vec<Point3> = sel.iter().filter_map(|id| s.at(*id)).collect();
        (!pts.is_empty()).then(|| {
            pts.iter()
                .fold(Point3::origin(), |a, p| a + p.coords / pts.len() as Real)
        })
    }

    /// A click on an arrow (a ring) of the triad: the distance (angle) box
    /// opens there.
    pub(super) fn sk3_gizmo_click(&mut self, p: Pos2, rect: Rect) -> bool {
        match self.sk3_gizmo_hit(p, rect) {
            Some(h) if !(3..6).contains(&h) => {
                if let Some(g) = self.sketch3d.as_mut().and_then(|e| e.gizmo.as_mut()) {
                    g.typing = Some((h, 0.0, false));
                }
                true
            }
            Some(_) => true,
            None => false,
        }
    }

    /// The in-place boxes of the open sketch: a dimension's value and the
    /// triad's typed distance (the same expression field as everywhere).
    pub(super) fn sk3_view_boxes(&mut self, ctx: &egui::Context, rect: Rect) {
        let Some(e) = self.sketch3d.clone() else {
            return;
        };
        let table = self.part().parameter_table();
        // A dimension being edited.
        if let Some((k, focused)) = e.dim_edit {
            let Some(s) = self.part().sketches3d.get(e.index) else {
                return;
            };
            let Some(c) = s.constraints.get(k).copied() else {
                if let Some(x) = self.sketch3d.as_mut() {
                    x.dim_edit = None;
                }
                return;
            };
            let at = self
                .sk3_labels(rect)
                .into_iter()
                .find(|(l, ..)| *l == Label3::Dim(k))
                .map(|(_, p, _)| p);
            let (Some(at), Some(v)) = (at, c.value()) else {
                return;
            };
            let angle = matches!(c, Constraint3::Angle(..));
            let id = egui::Id::new(("sk3_dim", e.index, k));
            let mut commit = None;
            egui::Area::new(id.with("area"))
                .fixed_pos(at - egui::vec2(50.0, 12.0))
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_width(110.0);
                        let spec = super::valuefield::FieldSpec {
                            id,
                            value: v,
                            unit: if angle {
                                crate::part::expr::Unit::Deg
                            } else {
                                crate::part::expr::Unit::Mm
                            },
                            name: None,
                            expr: None,
                            table: &table,
                            quantity: None,
                        };
                        if let Some(t) = super::valuefield::value_field(ui, &spec) {
                            commit = Some(t);
                        }
                        if !focused {
                            ui.memory_mut(|m| m.request_focus(id.with("te")));
                        }
                    });
                });
            if let Some(x) = self.sketch3d.as_mut() {
                x.dim_edit = Some((k, true));
            }
            if let Some(text) = commit {
                let unit = if angle {
                    crate::part::expr::Unit::Deg
                } else {
                    crate::part::expr::Unit::Mm
                };
                match crate::part::params::eval_field_in(&table, &text, unit, None) {
                    Ok((_, nv, _)) => self.sk3_set_dimension(k, nv),
                    Err(err) => self.file_status = Some(Err(format!("3D Sketch: {err}"))),
                }
                if let Some(x) = self.sketch3d.as_mut() {
                    x.dim_edit = None;
                }
            }
        }
        // The triad's typed distance.
        if let Some(g) = &e.gizmo
            && let Some((k, d, focused)) = g.typing
            && let Some((o, len)) = self.sk3_gizmo_frame(rect)
            && let Some(tip) = self.camera.project(
                &if k >= 6 {
                    ring_points(o, len, k - 6)[6]
                } else {
                    o + axis(k) * len
                },
                rect,
            )
        {
            let ring = k >= 6;
            let unit = if ring {
                crate::part::expr::Unit::Deg
            } else {
                crate::part::expr::Unit::Mm
            };
            let id = egui::Id::new(("sk3_move", e.index, k));
            let mut commit = None;
            egui::Area::new(id.with("area"))
                .fixed_pos(tip + egui::vec2(8.0, -12.0))
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_width(130.0);
                        ui.label(
                            RichText::new(if ring {
                                format!("Rotate about {}", ["X", "Y", "Z"][k - 6])
                            } else {
                                format!("Move along {}", ["X", "Y", "Z"][k])
                            })
                            .small(),
                        );
                        let spec = super::valuefield::FieldSpec {
                            id,
                            value: d,
                            unit,
                            name: None,
                            expr: None,
                            table: &table,
                            quantity: None,
                        };
                        if let Some(t) = super::valuefield::value_field(ui, &spec) {
                            commit = Some(t);
                        }
                        if !focused {
                            ui.memory_mut(|m| m.request_focus(id.with("te")));
                        }
                    });
                });
            if let Some(g) = self.sketch3d.as_mut().and_then(|e| e.gizmo.as_mut()) {
                g.typing = Some((k, d, true));
            }
            if let Some(text) = commit {
                match crate::part::params::eval_field_in(&table, &text, unit, None) {
                    Ok((_, dist, _)) => {
                        let start = self.part().sketches3d.get(e.index).cloned();
                        let moved = start.and_then(|s| {
                            if ring {
                                Self::sk3_transformed(&s, &e.sel, |q| turned(q, o, k - 6, dist))
                            } else {
                                Self::sk3_moved(&s, &e.sel, axis(k) * dist)
                            }
                        });
                        match moved {
                            Some(moved) => {
                                self.checkpoint("3D Move");
                                self.history.break_coalescing();
                                if let Some(slot) = self.part_mut().sketches3d.get_mut(e.index) {
                                    *slot = moved;
                                }
                                self.file_status = None;
                            }
                            None => {
                                self.file_status = Some(Err(
                                    "3D Move: the constraints do not let the selection move there"
                                        .into(),
                                ))
                            }
                        }
                    }
                    Err(err) => self.file_status = Some(Err(format!("3D Move: {err}"))),
                }
                if let Some(g) = self.sketch3d.as_mut().and_then(|e| e.gizmo.as_mut()) {
                    g.typing = None;
                }
            }
        }
    }

    /// A new value of dimension `k` (one Undo step; refused when it cannot
    /// be met).
    pub(super) fn sk3_set_dimension(&mut self, k: usize, v: Real) {
        let Some(e) = self.sketch3d.as_ref() else {
            return;
        };
        let index = e.index;
        let Some(mut s) = self.part().sketches3d.get(index).cloned() else {
            return;
        };
        let Some(c) = s.constraints.get_mut(k) else {
            return;
        };
        match c {
            Constraint3::Length(_, x)
            | Constraint3::Distance(_, _, x)
            | Constraint3::Angle(_, _, x) => *x = v,
            _ => return,
        }
        if !(v.is_finite() && v >= 0.0) {
            self.file_status = Some(Err(
                "3D Sketch: a dimension must be a number of 0 or more".into()
            ));
            return;
        }
        match s.solve() {
            Ok(()) => {
                s.refresh_helices();
                self.checkpoint("Edit Dimension");
                self.history.break_coalescing();
                if let Some(slot) = self.part_mut().sketches3d.get_mut(index) {
                    *slot = s;
                }
                self.file_status = None;
            }
            Err(err) => self.file_status = Some(Err(err)),
        }
    }

    /// Selection with no tool running: a label (dimension: edit it; glyph:
    /// select it), a point (Ctrl adds), a line (its two points), empty
    /// space (clears). Returns whether the click was taken.
    pub(super) fn sk3_select_click(&mut self, p: Pos2, rect: Rect, ctrl: bool) -> bool {
        if let Some(l) = self.sk3_label_at(p, rect) {
            if let Some(e) = self.sketch3d.as_mut() {
                match l {
                    Label3::Dim(k) => e.dim_edit = Some((k, false)),
                    Label3::Glyph(k) => e.sel_cons = Some(k),
                }
            }
            return true;
        }
        let Some((s, _)) = self.sk3() else {
            return false;
        };
        let pr = |q: Point3| self.camera.project(&q, rect);
        let point = s
            .points
            .iter()
            .filter_map(|q| Some((pr(Point3::from(q.at))?.distance(p), q.id)))
            .filter(|(d, _)| *d <= super::overlay::POINT_SNAP_PX)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, id)| vec![id]);
        let line = || {
            s.lines
                .iter()
                .filter_map(|l| {
                    let (a, b) = (pr(s.at(l.a)?)?, pr(s.at(l.b)?)?);
                    Some((seg_dist(p, a, b), vec![l.a, l.b]))
                })
                .filter(|(d, _)| *d <= super::overlay::CURVE_SNAP_PX)
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, v)| v)
        };
        let picked = point.or_else(line).unwrap_or_default();
        if let Some(e) = self.sketch3d.as_mut() {
            e.sel_cons = None;
            e.dim_edit = None;
            if !ctrl {
                e.sel.clear();
            }
            for id in picked {
                if let Some(i) = e.sel.iter().position(|x| *x == id) {
                    if ctrl {
                        e.sel.remove(i);
                    }
                } else {
                    e.sel.push(id);
                }
            }
            // A selection shows the triad at once (as the 2D sketch's
            // selection moves by dragging): arrows, squares and rings.
            e.gizmo = (!e.sel.is_empty()).then(Gizmo::default);
        }
        true
    }

    /// Selects every point of the chain through the selection (Inventor:
    /// double click selects the connected curves).
    pub(super) fn sk3_select_chain(&mut self) {
        let Some((s, e)) = self.sk3() else { return };
        let Ok(chains) = s.chains() else { return };
        let mut sel = e.sel.clone();
        for chain in chains {
            let pts: Vec<u32> = chain.iter().flat_map(|(_, a, b)| [*a, *b]).collect();
            if pts.iter().any(|p| sel.contains(p)) {
                for p in pts {
                    if !sel.contains(&p) {
                        sel.push(p);
                    }
                }
            }
        }
        if let Some(e) = self.sketch3d.as_mut() {
            e.gizmo = (!sel.is_empty()).then(Gizmo::default);
            e.sel = sel;
        }
    }

    /// 3D Move: the triad on the selection.
    pub(super) fn sk3_start_move(&mut self) {
        let Some(e) = self.sketch3d.as_mut() else {
            self.file_status = Some(Err("3D Move: open a 3D sketch first".into()));
            return;
        };
        if e.sel.is_empty() {
            self.file_status = Some(Err(
                "3D Move: select points or lines of the 3D sketch first (no tool running)".into(),
            ));
            return;
        }
        e.tool = None;
        e.gizmo = Some(Gizmo::default());
        self.file_status = None;
    }

    /// Deletes the selected constraint (Delete key / its menu).
    pub(super) fn sk3_delete_selected_constraint(&mut self) {
        let Some(k) = self.sketch3d.as_ref().and_then(|e| e.sel_cons) else {
            return;
        };
        self.checkpoint("Delete Constraint");
        let index = self.sketch3d.as_ref().map_or(0, |e| e.index);
        if let Some(s) = self.part_mut().sketches3d.get_mut(index) {
            s.remove_constraint(k);
        }
        if let Some(e) = self.sketch3d.as_mut() {
            e.sel_cons = None;
        }
    }
}
